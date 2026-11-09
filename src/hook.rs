//! `lagom hook <agent> <event>`: reads the agent's hook JSON on stdin, prints a response.
//!
//! Invariants:
//! - Fail open: any problem prints nothing (Cursor: `{}`), so the agent proceeds unchanged.
//! - Never grant a permission the user has not granted. Claude Code and Cursor rewrites
//!   carry "allow" only when the user's own allow rule matches the original command;
//!   deny/ask rules or no rule mean no rewrite (except Claude's bypassPermissions mode).
//!   Codex re-checks approval and sandbox on the rewritten command itself.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{rewrite, store, style};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Agent {
    Claude,
    Codex,
    Cursor,
}

impl Agent {
    pub fn parse(s: &str) -> Option<Agent> {
        match s {
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            "cursor" => Some(Agent::Cursor),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Cursor => "cursor",
        }
    }
}

/// Largest hook payload we read. Bigger input is ignored (fail open).
const MAX_PAYLOAD: u64 = 8 * 1024 * 1024;
/// Longest shell command we consider rewriting.
const MAX_COMMAND: usize = 64 * 1024;

/// Longest a hook may take. Agents wait for hooks (we register a 10 s timeout);
/// Lagom gives up well before that and lets the agent continue unchanged.
const HOOK_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);

/// Entry point used by `main`. Always exits successfully.
pub fn run(agent: &str, event: &str) {
    let (agent_name, event) = (agent.to_string(), event.to_string());
    let response = with_deadline(HOOK_DEADLINE, move || {
        let payload = read_payload(std::io::stdin().lock());
        payload
            .as_ref()
            .zip(Agent::parse(&agent_name))
            .and_then(|(p, a)| respond(a, &event, p, &lagom_bin()))
    })
    .flatten();
    match (response, Agent::parse(agent)) {
        (Some(out), _) => println!("{out}"),
        (None, Some(Agent::Cursor)) => println!("{{}}"),
        (None, _) => {}
    }
}

/// Run `work` on a thread and wait at most `deadline`. `None` on timeout or panic;
/// the unfinished thread is abandoned (the process exits right after the hook prints).
fn with_deadline<T: Send + 'static>(
    deadline: std::time::Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.recv_timeout(deadline).ok()
}

/// Parse a hook payload: a JSON object of at most [`MAX_PAYLOAD`] bytes, else `None`.
fn read_payload(input: impl Read) -> Option<Value> {
    let mut bytes = Vec::new();
    input.take(MAX_PAYLOAD + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_PAYLOAD {
        return None;
    }
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .filter(Value::is_object)
}

/// Path written into hooks: [`choose_bin`] for the running process.
pub fn lagom_bin() -> String {
    let exe = std::env::current_exe().ok();
    let argv0 = std::env::args_os().next().map(PathBuf::from);
    let path_var = std::env::var_os("PATH");
    choose_bin(argv0.as_deref(), exe.as_deref(), path_var.as_deref())
        .to_string_lossy()
        .into_owned()
}

/// Pick a binary path for hooks that survives package-manager upgrades.
///
/// Hooks must not point into versioned folders (Homebrew's `Cellar/lagom/0.1.0/…`,
/// a Python `site-packages`, the Nix store): they vanish on upgrade, and Codex ties
/// hook trust to the exact command string. Order:
/// 1. The path the user invoked (or found on `PATH`), if it resolves to this binary.
/// 2. The executable path, unresolved.
/// 3. If that is versioned, `lagom` from `PATH` when it resolves here, else bare `lagom`.
///
/// When run as the `lgm` alias, the `lagom` binary next to it is preferred.
pub fn choose_bin(
    argv0: Option<&Path>,
    exe: Option<&Path>,
    path_var: Option<&std::ffi::OsStr>,
) -> PathBuf {
    let Some(exe) = exe else {
        return PathBuf::from("lagom");
    };
    let target = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let is_this = |p: &Path| p.canonicalize().is_ok_and(|c| c == target);

    let invoked = argv0
        .and_then(|a| {
            if a.components().count() > 1 {
                std::path::absolute(a).ok()
            } else {
                find_on_path(a.as_os_str(), path_var)
            }
        })
        .filter(|p| is_this(p));
    let mut chosen = invoked.unwrap_or_else(|| exe.to_path_buf());
    if is_versioned(&chosen) {
        chosen = find_on_path("lagom".as_ref(), path_var)
            .filter(|p| is_this(p) && !is_versioned(p))
            .unwrap_or_else(|| PathBuf::from("lagom"));
    }
    if chosen.file_stem().and_then(|s| s.to_str()) == Some("lgm") {
        let main = chosen.with_file_name(format!("lagom{}", std::env::consts::EXE_SUFFIX));
        if main.is_file() {
            chosen = main;
        }
    }
    chosen
}

/// Paths that change on upgrade.
fn is_versioned(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("Cellar" | "site-packages" | "_npx")
        )
    }) || path.starts_with("/nix/store")
}

fn find_on_path(name: &std::ffi::OsStr, path_var: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let suffixed = {
        let mut s = name.to_os_string();
        s.push(std::env::consts::EXE_SUFFIX);
        s
    };
    std::env::split_paths(path_var?)
        .flat_map(|dir| [dir.join(name), dir.join(&suffixed)])
        .find(|p| p.is_file())
}

/// Pure decision function (testable): payload in, response JSON out.
pub fn respond(agent: Agent, event: &str, payload: &Value, bin: &str) -> Option<Value> {
    if std::env::var_os("LAGOM_DISABLE").is_some() {
        return None;
    }
    // Claude Code and Codex send `cwd`; Cursor sends `workspace_roots`.
    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .pointer("/workspace_roots/0")
                .and_then(Value::as_str)
        })
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    match event {
        "session-start" | "subagent-start" => inject(agent, event, &cwd, payload),
        "pre-tool" => pre_tool(agent, payload, &cwd, bin),
        _ => None,
    }
}

/// `agent:id` for stats, from the payload's session id (Cursor: conversation id).
fn session_token(agent: Agent, payload: &Value) -> Option<String> {
    let id = payload["session_id"]
        .as_str()
        .or(payload["conversation_id"].as_str())?;
    store::is_safe_id(id).then(|| format!("{}:{id}", agent.name()))
}

fn inject(agent: Agent, event: &str, cwd: &Path, payload: &Value) -> Option<Value> {
    let text = style::render(cwd);
    if text.is_empty() {
        return None;
    }
    let who = store::Attribution::from_token(session_token(agent, payload).as_deref(), cwd);
    // Holdout experiment: some sessions run without styles (compression stays on).
    let arm = who
        .session
        .as_deref()
        .and_then(|s| crate::gain::experiment::Experiment::active().map(|e| e.arm(s)));
    if arm == Some(crate::gain::experiment::Arm::Holdout) {
        store::record_for(
            json!({"kind": "holdout", "arm": "holdout", "event": event}),
            &who,
        );
        return None;
    }
    store::record_for(
        json!({"kind": "inject", "agent": agent.name(), "event": event, "bytes": text.len(),
               "styles": style::active(cwd), "arm": arm.map(|_| "styled")}),
        &who,
    );
    Some(match agent {
        Agent::Cursor => json!({"additional_context": text}),
        Agent::Claude | Agent::Codex => {
            let name = if event == "subagent-start" {
                "SubagentStart"
            } else {
                "SessionStart"
            };
            json!({"hookSpecificOutput": {"hookEventName": name, "additionalContext": text}})
        }
    })
}

fn pre_tool(agent: Agent, payload: &Value, cwd: &Path, bin: &str) -> Option<Value> {
    let input = payload.get("tool_input")?;
    if input.get("run_in_background").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let command = input.get("command")?.as_str()?;
    if command.len() > MAX_COMMAND || !style::rewrite_enabled(cwd) {
        return None;
    }
    let rewritten = rewrite::rewrite(command, bin, session_token(agent, payload).as_deref())?;

    // updatedInput replaces the whole input: keep every original field.
    let mut updated = input.clone();
    updated["command"] = Value::String(rewritten);

    match agent {
        Agent::Claude => {
            if unreadable_claude_policy() {
                return None; // an org policy we can't read might deny this command
            }
            // None: a server-managed policy cache we can't read might deny this command.
            let rules = claude_rules(cwd, &crate::install::claude_dir(), &claude_managed_dir())?;
            let parts = CommandParts::of(command);
            if parts.any(|p| rules.denies(p) || rules.asks(p)) {
                return None;
            }
            let mut out = json!({
                "hookEventName": "PreToolUse",
                "permissionDecisionReason": "lagom: compress output",
                "updatedInput": updated,
            });
            if rules.allows(parts.main) && parts.cd_stays_inside(cwd) {
                out["permissionDecision"] = json!("allow");
            } else if payload.get("permission_mode").and_then(Value::as_str)
                != Some("bypassPermissions")
            {
                return None;
            }
            Some(json!({"hookSpecificOutput": out}))
        }
        Agent::Codex => Some(json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": "lagom: compress output",
            "updatedInput": updated,
        }})),
        Agent::Cursor => {
            let mut rules = Rules::new(Syntax::Cursor);
            rules.add_file(&cursor_cli_config(), "Shell", Source::Trusted);
            let parts = CommandParts::of(command);
            if parts.any(|p| rules.denies(p))
                || !rules.allows(parts.main)
                || !parts.cd_stays_inside(cwd)
            {
                return None;
            }
            Some(json!({"continue": true, "permission": "allow", "updated_input": updated}))
        }
    }
}

/// A command split the way agents check permissions: the whole text, the leading
/// `cd <dir>` (if any) and the command that runs.
struct CommandParts<'a> {
    whole: &'a str,
    cd: Option<(String, &'a str)>,
    main: &'a str,
}

impl<'a> CommandParts<'a> {
    fn of(command: &'a str) -> Self {
        match rewrite::split_cd(command) {
            Some((dir, main)) => CommandParts {
                whole: command,
                cd: Some((format!("cd {dir}"), dir)),
                main,
            },
            None => CommandParts {
                whole: command,
                cd: None,
                main: command,
            },
        }
    }

    /// True if any part matches (deny and ask rules apply to every part).
    fn any(&self, mut f: impl FnMut(&str) -> bool) -> bool {
        f(self.whole) || f(self.main) || self.cd.as_ref().is_some_and(|(cd, _)| f(cd))
    }

    /// A `cd` that stays inside the project folder is read-only; anything else needs
    /// the user's own approval, so Lagom doesn't grant it.
    fn cd_stays_inside(&self, cwd: &Path) -> bool {
        let Some((_, dir)) = &self.cd else {
            return true;
        };
        let dir = Path::new(dir);
        if dir.is_absolute() {
            return dir.starts_with(cwd)
                && !dir
                    .components()
                    .any(|c| c == std::path::Component::ParentDir);
        }
        !dir.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        })
    }
}

/// Where Claude Code reads file-based managed (organization) settings.
fn claude_managed_dir() -> PathBuf {
    PathBuf::from(if cfg!(target_os = "macos") {
        "/Library/Application Support/ClaudeCode"
    } else if cfg!(windows) {
        r"C:\Program Files\ClaudeCode"
    } else {
        "/etc/claude-code"
    })
}

/// True when Claude Code may be enforcing a policy Lagom can't read: a macOS MDM
/// profile, or Windows (policy can live in the registry). Claude Code checks deny
/// rules against the *rewritten* command, so Lagom must see every deny rule itself;
/// when it can't, it doesn't rewrite. Server-managed settings from claude.ai are read
/// from Claude Code's cache by [`claude_remote_policy`].
fn unreadable_claude_policy() -> bool {
    if cfg!(windows) {
        return true;
    }
    let prefs = Path::new("/Library/Managed Preferences");
    let user = std::env::var("USER").unwrap_or_default();
    cfg!(target_os = "macos")
        && (prefs.join("com.anthropic.claudecode.plist").exists()
            || prefs
                .join(user)
                .join("com.anthropic.claudecode.plist")
                .exists())
}

/// Server-managed settings (claude.ai admin console) as Claude Code cached them.
#[derive(Debug, PartialEq)]
enum RemotePolicy {
    /// No cache: no server-managed policy has been delivered to this config folder.
    Absent,
    /// A cache Lagom can't read or parse; it may hold deny rules.
    Unreadable,
    /// The cached settings.
    Settings(Value),
}

/// Read Claude Code's cache of server-managed settings, `<config dir>/remote-settings.json`.
/// Claude Code fetches the policy at startup, refreshes it hourly and applies the cache
/// on later starts; `/logout` deletes it.
fn claude_remote_policy(user_dir: &Path) -> RemotePolicy {
    let file = user_dir.join("remote-settings.json");
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return RemotePolicy::Absent,
        Err(_) => return RemotePolicy::Unreadable,
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(v) if v.is_object() => RemotePolicy::Settings(v),
        _ => RemotePolicy::Unreadable,
    }
}

/// The settings object inside a cached remote payload: the top level, or a `settings`
/// object if the cache wraps it.
fn remote_settings(v: &Value) -> &Value {
    match v.get("settings") {
        Some(inner) if inner.is_object() && v.get("permissions").is_none() => inner,
        _ => v,
    }
}

/// Every `permissions` object in `v`, at any depth up to `depth`. Deny and ask rules are
/// taken from all of them, so an unexpected cache layout can only make Lagom stricter.
fn permission_objects(v: &Value, depth: usize, out: &mut Vec<Value>) {
    let Some(map) = v.as_object() else {
        return;
    };
    for (key, child) in map {
        if key == "permissions" && child.is_object() {
            out.push(json!({ "permissions": child }));
        } else if depth > 0 {
            permission_objects(child, depth - 1, out);
        }
    }
}

/// Collect Claude Code's permission rules the way Claude Code applies them, or `None`
/// when a server-managed policy cache exists but can't be read.
/// - server-managed settings (Claude Code's cache, see [`claude_remote_policy`]): deny
///   and ask rules always count. When the cache holds any setting, it is the managed
///   source Claude Code selects, so file-based managed allow rules no longer count;
/// - managed settings (`managed-settings.json`, then `managed-settings.d/*.json`);
///   with `allowManagedPermissionRulesOnly` (in any managed source), they are the only
///   source of allow rules;
/// - user settings and the config home's `settings.local.json`;
/// - project `.claude/settings.json`: deny and ask only, because Claude Code applies its
///   allow rules only after workspace trust, which Lagom can't see;
/// - project `.claude/settings.local.json`: allow rules only if the file is the user's own
///   (not tracked by git, `.claude` not a symlink); deny and ask always.
fn claude_rules(cwd: &Path, user_dir: &Path, managed_dir: &Path) -> Option<Rules> {
    let mut rules = Rules::new(Syntax::Claude);
    let remote = match claude_remote_policy(user_dir) {
        RemotePolicy::Absent => None,
        RemotePolicy::Unreadable => return None,
        RemotePolicy::Settings(v) => Some(v),
    };
    let remote_selected = remote
        .as_ref()
        .and_then(|v| remote_settings(v).as_object())
        .is_some_and(|m| !m.is_empty());
    let mut managed_only = false;
    if let Some(v) = &remote {
        let settings = remote_settings(v);
        managed_only = settings.get("allowManagedPermissionRulesOnly") == Some(&json!(true));
        rules.add_value(settings, "Bash", Source::Trusted, None);
        let mut nested = vec![];
        permission_objects(v, 4, &mut nested);
        for p in &nested {
            rules.add_value(p, "Bash", Source::RestrictOnly, None);
        }
    }

    let mut managed = vec![managed_dir.join("managed-settings.json")];
    if let Ok(entries) = fs::read_dir(managed_dir.join("managed-settings.d")) {
        let mut drop_ins: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|e| e == "json")
                    && !p
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            })
            .collect();
        drop_ins.sort();
        managed.extend(drop_ins);
    }
    managed_only |= managed.iter().any(|f| {
        read_json(f).is_some_and(|v| v.get("allowManagedPermissionRulesOnly") == Some(&json!(true)))
    });
    let file_managed = if remote_selected {
        Source::RestrictOnly
    } else {
        Source::Trusted
    };
    for f in &managed {
        rules.add_file(f, "Bash", file_managed);
    }
    let others = if managed_only {
        Source::RestrictOnly
    } else {
        Source::Trusted
    };
    rules.add_file(&user_dir.join("settings.json"), "Bash", others);
    rules.add_file(&user_dir.join("settings.local.json"), "Bash", others);
    for dir in cwd.ancestors() {
        let claude = dir.join(".claude");
        if claude == user_dir {
            continue; // the user's own config folder, already read above
        }
        rules.add_file(&claude.join("settings.json"), "Bash", Source::RestrictOnly);
        let local = if managed_only {
            Source::RestrictOnly
        } else {
            Source::LocalFile
        };
        rules.add_file(&claude.join("settings.local.json"), "Bash", local);
    }
    Some(rules)
}

fn cursor_cli_config() -> PathBuf {
    store::home_dir().join(".cursor").join("cli-config.json")
}

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(file).ok()?).ok()
}

/// How far to trust the allow rules of one settings file. Deny and ask rules always count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    /// Allow rules count (user, managed, config home).
    Trusted,
    /// Allow rules ignored (shared project settings; anything under managed-only policy).
    RestrictOnly,
    /// Allow rules count only if the file is the user's own: not tracked by git,
    /// and its `.claude` folder is not a symlink. Checked only when a rule matches.
    LocalFile,
}

/// Rule syntax differs per agent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Syntax {
    /// Claude Code: `Bash(cmd)` exact, `Bash(cmd *)` / `Bash(cmd:*)` prefix on a word
    /// boundary (also the bare command), `*` anywhere matches any text.
    Claude,
    /// Cursor: `Shell(git)` matches `git` and `git …` (base command).
    Cursor,
}

/// Permission rules collected from settings files.
#[derive(Debug)]
pub struct Rules {
    syntax: Syntax,
    allow: Vec<String>,
    local_allow: Vec<(PathBuf, String)>,
    deny: Vec<String>,
    ask: Vec<String>,
}

impl Rules {
    pub fn new(syntax: Syntax) -> Rules {
        Rules {
            syntax,
            allow: vec![],
            local_allow: vec![],
            deny: vec![],
            ask: vec![],
        }
    }

    pub fn add_file(&mut self, file: &Path, tool: &str, source: Source) {
        if let Some(v) = read_json(file) {
            self.add_value(&v, tool, source, Some(file));
        }
    }

    /// Add the rules of one settings object. `file` is needed for [`Source::LocalFile`];
    /// without it, local allow rules are ignored.
    fn add_value(&mut self, v: &Value, tool: &str, source: Source, file: Option<&Path>) {
        let list = |key: &str| -> Vec<String> {
            v.pointer(&format!("/permissions/{key}"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|r| r.as_str().and_then(|r| tool_pattern(r, tool)))
                .collect()
        };
        self.deny.extend(list("deny"));
        self.ask.extend(list("ask"));
        match source {
            Source::Trusted => self.allow.extend(list("allow")),
            Source::RestrictOnly => {}
            Source::LocalFile => {
                if let Some(file) = file {
                    self.local_allow
                        .extend(list("allow").into_iter().map(|p| (file.to_path_buf(), p)));
                }
            }
        }
    }

    pub fn allows(&self, cmd: &str) -> bool {
        self.allow.iter().any(|p| self.matches(p, cmd))
            || self
                .local_allow
                .iter()
                .any(|(file, p)| self.matches(p, cmd) && is_users_own_local_file(file))
    }
    pub fn denies(&self, cmd: &str) -> bool {
        self.deny.iter().any(|p| self.matches(p, cmd))
    }
    pub fn asks(&self, cmd: &str) -> bool {
        self.ask.iter().any(|p| self.matches(p, cmd))
    }

    fn matches(&self, pattern: &str, cmd: &str) -> bool {
        match self.syntax {
            Syntax::Claude => claude_rule_matches(pattern, cmd),
            Syntax::Cursor => cursor_rule_matches(pattern, cmd),
        }
    }
}

/// `.claude/settings.local.json` counts as the user's own when `.claude` isn't a symlink
/// and git doesn't track the file. Not verifiable (git missing) means not trusted.
fn is_users_own_local_file(file: &Path) -> bool {
    let Some(claude) = file.parent() else {
        return false;
    };
    if fs::symlink_metadata(claude).is_ok_and(|m| m.file_type().is_symlink()) {
        return false;
    }
    let Some(project) = claude.parent() else {
        return false;
    };
    match std::process::Command::new("git")
        .arg("-C")
        .arg(project)
        .args([
            "ls-files",
            "--error-unmatch",
            "--",
            ".claude/settings.local.json",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(s) => s.code() != Some(0), // 0 = tracked; 1 = untracked; 128 = not a repo
        Err(_) => false,
    }
}

/// `Bash(git push *)` -> `git push *`; `Bash(git push:*)` -> `git push *`; bare `Bash` -> `*`.
fn tool_pattern(rule: &str, tool: &str) -> Option<String> {
    let rule = rule.trim();
    if rule == tool {
        return Some("*".into());
    }
    let inner = rule
        .strip_prefix(tool)?
        .strip_prefix('(')?
        .strip_suffix(')')?;
    Some(match inner.strip_suffix(":*") {
        Some(prefix) => format!("{prefix} *"),
        None => inner.to_string(),
    })
}

/// Claude Code's Bash rule semantics (code.claude.com/docs/en/permissions):
/// no `*` means the exact command; a trailing ` *` that is the only wildcard also
/// matches the bare command; otherwise `*` matches any text, including none.
pub fn claude_rule_matches(pattern: &str, cmd: &str) -> bool {
    let cmd = cmd.trim();
    if !pattern.contains('*') {
        return cmd == pattern;
    }
    if let Some(prefix) = pattern.strip_suffix(" *") {
        if !prefix.contains('*') && cmd == prefix {
            return true;
        }
    }
    glob_matches(pattern, cmd)
}

/// Cursor's `Shell(git)`: the base command, with or without arguments. `*` is a glob.
pub fn cursor_rule_matches(pattern: &str, cmd: &str) -> bool {
    let cmd = cmd.trim();
    if !pattern.contains('*') {
        return cmd == pattern || cmd.starts_with(&format!("{pattern} "));
    }
    glob_matches(pattern, cmd)
}

/// `*` matches any text (including none); everything else is literal.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let Some((first, rest_parts)) = parts.split_first() else {
        return text.is_empty();
    };
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let Some((last, middle)) = rest_parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(pos) => rest = &rest[pos + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_hook_work_is_abandoned_at_the_deadline() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let out = with_deadline(Duration::from_millis(100), || {
            std::thread::sleep(Duration::from_secs(5));
            1
        });
        assert_eq!(out, None);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(with_deadline(Duration::from_secs(1), || 7), Some(7));
        assert_eq!(
            with_deadline(Duration::from_secs(1), || -> i32 { panic!("boom") }),
            None
        );
    }

    #[test]
    fn payload_limits() {
        assert!(read_payload(&b"{\"a\":1}"[..]).is_some());
        assert!(read_payload(&b"[1,2]"[..]).is_none());
        assert!(read_payload(&b"\"text\""[..]).is_none());
        assert!(read_payload(&b"not json"[..]).is_none());
        assert!(read_payload(&[0xff, 0xfe][..]).is_none());
        let big = format!("{{\"a\":\"{}\"}}", "x".repeat(MAX_PAYLOAD as usize));
        assert!(read_payload(big.as_bytes()).is_none());
    }

    #[test]
    fn huge_command_is_not_rewritten() {
        let cmd = format!("git log {}", "a".repeat(MAX_COMMAND));
        let p = json!({"tool_name": "Bash", "tool_input": {"command": cmd}, "permission_mode": "bypassPermissions"});
        assert!(pre_tool(Agent::Claude, &p, Path::new("/"), "/bin/lagom").is_none());
    }

    #[cfg(unix)]
    fn brew_layout(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("lagom-hk01-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let cellar = root.join("Cellar/lagom/0.1.0/bin");
        fs::create_dir_all(&cellar).unwrap();
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(cellar.join("lagom"), "").unwrap();
        std::os::unix::fs::symlink(cellar.join("lagom"), root.join("bin/lagom")).unwrap();
        std::os::unix::fs::symlink(cellar.join("lagom"), root.join("bin/lgm")).unwrap();
        root
    }

    #[cfg(unix)]
    #[test]
    fn hook_path_avoids_homebrew_cellar() {
        let root = brew_layout("brew");
        let exe = root.join("Cellar/lagom/0.1.0/bin/lagom");
        let path = root.join("bin");
        // Invoked as `lagom` from PATH: the stable symlink wins.
        let got = choose_bin(Some(Path::new("lagom")), Some(&exe), Some(path.as_os_str()));
        assert_eq!(got, root.join("bin/lagom"));
        // Invoked by its Cellar path: still the PATH symlink, never the versioned path.
        let got = choose_bin(Some(&exe), Some(&exe), Some(path.as_os_str()));
        assert_eq!(got, root.join("bin/lagom"));
        // Through the `lgm` alias symlink: the `lagom` next to it.
        let got = choose_bin(Some(Path::new("lgm")), Some(&exe), Some(path.as_os_str()));
        assert_eq!(got, root.join("bin/lagom"));
        // Versioned and not on PATH: bare `lagom`.
        let got = choose_bin(Some(&exe), Some(&exe), None);
        assert_eq!(got, PathBuf::from("lagom"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hook_path_keeps_plain_install_location() {
        let exe = std::env::current_exe().unwrap();
        let got = choose_bin(Some(&exe), Some(&exe), None);
        assert_eq!(got, std::path::absolute(&exe).unwrap());
        assert_eq!(choose_bin(None, None, None), PathBuf::from("lagom"));
    }

    #[test]
    fn claude_rule_syntax_matches_the_docs() {
        let m = claude_rule_matches;
        assert_eq!(
            tool_pattern("Bash(git status:*)", "Bash").unwrap(),
            "git status *"
        );
        assert_eq!(tool_pattern("Bash", "Bash").unwrap(), "*");
        assert_eq!(tool_pattern("Read(./x)", "Bash"), None);
        // Exact without a wildcard: never a prefix (that would widen an allow rule).
        assert!(m("npm run build", "npm run build"));
        assert!(!m("npm run build", "npm run build --watch"));
        assert!(!m("git push", "git push --force"));
        // Trailing " *": word boundary, and also the bare command.
        assert!(m("ls *", "ls -la") && m("ls *", "ls") && !m("ls *", "lsof"));
        assert!(m("git log *", "git log") && m("git log *", "git log --stat"));
        assert!(m("ls*", "lsof") && m("ls*", "ls -la"));
        // `:*` is the same as " *".
        let colon = tool_pattern("Bash(ls:*)", "Bash").unwrap();
        assert!(m(&colon, "ls -la") && m(&colon, "ls") && !m(&colon, "lsof"));
        // `*` anywhere; a trailing " *" with another wildcard needs the space.
        assert!(m("git * --force", "git push origin --force"));
        assert!(m("* --help *", "npm --help x") && !m("* --help *", "npm --help"));
        assert!(m("*", "anything at all"));
    }

    #[test]
    fn cursor_rules_are_base_commands() {
        assert!(cursor_rule_matches("git", "git log"));
        assert!(cursor_rule_matches("git", "git"));
        assert!(!cursor_rule_matches("git", "gitk"));
        assert!(cursor_rule_matches("git *", "git status"));
    }

    fn write(path: &Path, json: Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, json.to_string()).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lagom-hk09-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn managed_deny_and_managed_only_policy_win() {
        let root = scratch("managed");
        let (user, managed, repo) = (
            root.join("home/.claude"),
            root.join("managed"),
            root.join("repo"),
        );
        fs::create_dir_all(&repo).unwrap();
        write(
            &user.join("settings.json"),
            json!({"permissions": {"allow": ["Bash(git *)"]}}),
        );
        write(
            &managed.join("managed-settings.d/20-sec.json"),
            json!({"permissions": {"deny": ["Bash(git push *)"]}}),
        );
        let r = claude_rules(&repo, &user, &managed).unwrap();
        assert!(r.allows("git log") && r.denies("git push --force"));

        write(
            &managed.join("managed-settings.json"),
            json!({"allowManagedPermissionRulesOnly": true}),
        );
        let r = claude_rules(&repo, &user, &managed).unwrap();
        assert!(
            !r.allows("git log"),
            "user allow rules don't count under managed-only policy"
        );
        assert!(r.denies("git push origin"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn server_managed_policy_cache_is_read() {
        let root = scratch("remote");
        let (user, managed, repo) = (
            root.join("home/.claude"),
            root.join("managed"),
            root.join("repo"),
        );
        fs::create_dir_all(&repo).unwrap();
        write(
            &user.join("settings.json"),
            json!({"permissions": {"allow": ["Bash(git *)", "Bash(cargo *)"]}}),
        );
        write(
            &managed.join("managed-settings.json"),
            json!({"permissions": {"allow": ["Bash(make *)"]}}),
        );
        let remote = user.join("remote-settings.json");

        // No cache: user and file-based managed rules apply as before.
        let r = claude_rules(&repo, &user, &managed).unwrap();
        assert!(r.allows("git push") && r.allows("make test"));

        // Cached deny and ask rules win; a non-empty cache is the managed source Claude
        // Code selects, so file-based managed allow rules stop counting.
        write(
            &remote,
            json!({"permissions": {"deny": ["Bash(git push *)"], "ask": ["Bash(cargo publish *)"]}}),
        );
        let r = claude_rules(&repo, &user, &managed).unwrap();
        assert!(r.denies("git push origin") && r.asks("cargo publish --dry-run"));
        assert!(r.allows("git log") && !r.allows("make test"));

        // Managed-only policy from the server: user allow rules don't count.
        write(
            &remote,
            json!({"allowManagedPermissionRulesOnly": true,
                   "permissions": {"allow": ["Bash(git log *)"]}}),
        );
        let r = claude_rules(&repo, &user, &managed).unwrap();
        assert!(r.allows("git log --oneline") && !r.allows("cargo test"));

        // A wrapped or nested layout still contributes its deny rules.
        write(
            &remote,
            json!({"etag": "x", "settings": {"permissions": {"deny": ["Bash(git push *)"]}}}),
        );
        assert!(
            claude_rules(&repo, &user, &managed)
                .unwrap()
                .denies("git push")
        );
        write(
            &remote,
            json!({"payload": {"policy": {"permissions": {"deny": ["Bash(rm *)"]}}}}),
        );
        assert!(
            claude_rules(&repo, &user, &managed)
                .unwrap()
                .denies("rm -rf x")
        );

        // An empty cache selects nothing: file-based managed allow rules count again.
        write(&remote, json!({}));
        assert!(
            claude_rules(&repo, &user, &managed)
                .unwrap()
                .allows("make test")
        );

        // A cache Lagom can't parse may hold deny rules: no rules, so no rewrite.
        fs::write(&remote, "{\"permissions\": {\"deny\": [").unwrap();
        assert!(claude_rules(&repo, &user, &managed).is_none());
        fs::write(&remote, "[]").unwrap();
        assert!(claude_rules(&repo, &user, &managed).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn project_allow_rules_need_trust_lagom_cannot_see() {
        let root = scratch("project");
        let (user, managed, repo) = (
            root.join("home/.claude"),
            root.join("managed"),
            root.join("repo"),
        );
        write(
            &repo.join(".claude/settings.json"),
            json!({"permissions": {"allow": ["Bash(git *)"], "deny": ["Bash(git push *)"]}}),
        );
        let r = claude_rules(&repo.join("src"), &user, &managed).unwrap();
        assert!(
            !r.allows("git log"),
            "committed project allow rules wait for workspace trust"
        );
        assert!(r.denies("git push"), "project deny rules always apply");

        // An untracked local file is the user's own: its allow rules count.
        write(
            &repo.join(".claude/settings.local.json"),
            json!({"permissions": {"allow": ["Bash(git log *)"]}}),
        );
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        if !git(&["init", "-q"]) {
            return; // git not installed: nothing more to check here
        }
        assert!(
            claude_rules(&repo, &user, &managed)
                .unwrap()
                .allows("git log")
        );

        // Committed to the repo, it is repository-supplied: allow rules ignored.
        git(&["add", ".claude/settings.local.json"]);
        assert!(
            !claude_rules(&repo, &user, &managed)
                .unwrap()
                .allows("git log")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn codex_rewrite_keeps_other_fields() {
        let payload =
            json!({"tool_name": "Bash", "tool_input": {"command": "git status", "timeout": 5000}});
        let out = respond(Agent::Codex, "pre-tool", &payload, "/bin/lagom").unwrap();
        let updated = &out["hookSpecificOutput"]["updatedInput"];
        assert_eq!(updated["command"], "/bin/lagom run -- git status");
        assert_eq!(updated["timeout"], 5000);
        assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "allow");
    }

    #[test]
    fn background_and_compound_commands_untouched() {
        let bg = json!({"tool_input": {"command": "git status", "run_in_background": true}});
        assert!(respond(Agent::Codex, "pre-tool", &bg, "/bin/lagom").is_none());
        let compound = json!({"tool_input": {"command": "git status && rm -rf x"}});
        assert!(respond(Agent::Codex, "pre-tool", &compound, "/bin/lagom").is_none());
    }
}
