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

/// Entry point used by `main`. Always exits successfully.
pub fn run(agent: &str, event: &str) {
    let payload = read_payload(std::io::stdin().lock());
    let response = payload
        .as_ref()
        .zip(Agent::parse(agent))
        .and_then(|(p, a)| respond(a, event, p, &lagom_bin()));
    match (response, Agent::parse(agent)) {
        (Some(out), _) => println!("{out}"),
        (None, Some(Agent::Cursor)) => println!("{{}}"),
        (None, _) => {}
    }
}

/// Path written into hooks: [`choose_bin`] for the running process.
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
        "session-start" | "subagent-start" => inject(agent, event, &cwd),
        "pre-tool" => pre_tool(agent, payload, &cwd, bin),
        _ => None,
    }
}

fn inject(agent: Agent, event: &str, cwd: &Path) -> Option<Value> {
    let text = style::render(cwd);
    if text.is_empty() {
        return None;
    }
    store::record(
        json!({"kind": "inject", "agent": agent.name(), "event": event, "bytes": text.len()}),
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
    if command.len() > MAX_COMMAND {
        return None;
    }
    let rewritten = rewrite::rewrite(command, bin)?;

    // updatedInput replaces the whole input: keep every original field.
    let mut updated = input.clone();
    updated["command"] = Value::String(rewritten);

    match agent {
        Agent::Claude => {
            let rules = Rules::load(&claude_settings_files(cwd), "Bash");
            if rules.denies(command) || rules.asks(command) {
                return None;
            }
            let mut out = json!({
                "hookEventName": "PreToolUse",
                "permissionDecisionReason": "lagom: compress output",
                "updatedInput": updated,
            });
            if rules.allows(command) {
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
            let rules = Rules::load(&[cursor_cli_config()], "Shell");
            if rules.denies(command) || !rules.allows(command) {
                return None;
            }
            Some(json!({"continue": true, "permission": "allow", "updated_input": updated}))
        }
    }
}

fn claude_settings_files(cwd: &Path) -> Vec<PathBuf> {
    let mut files = vec![
        crate::install::claude_dir().join("settings.json"),
        crate::install::claude_dir().join("settings.local.json"),
    ];
    for dir in cwd.ancestors() {
        files.push(dir.join(".claude").join("settings.json"));
        files.push(dir.join(".claude").join("settings.local.json"));
    }
    files
}

fn cursor_cli_config() -> PathBuf {
    store::home_dir().join(".cursor").join("cli-config.json")
}

/// Permission rules like `Bash(git status:*)` collected from settings files.
#[derive(Default, Debug)]
pub struct Rules {
    allow: Vec<String>,
    deny: Vec<String>,
    ask: Vec<String>,
}

impl Rules {
    pub fn load(files: &[PathBuf], tool: &str) -> Rules {
        let mut rules = Rules::default();
        for file in files {
            let Ok(text) = fs::read_to_string(file) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            let Some(perms) = v.get("permissions") else {
                continue;
            };
            for (key, list) in [
                ("allow", &mut rules.allow),
                ("deny", &mut rules.deny),
                ("ask", &mut rules.ask),
            ] {
                for rule in perms
                    .get(key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(pattern) = rule.as_str().and_then(|r| tool_pattern(r, tool)) {
                        list.push(pattern);
                    }
                }
            }
        }
        rules
    }

    pub fn allows(&self, cmd: &str) -> bool {
        self.allow.iter().any(|p| pattern_matches(p, cmd))
    }
    pub fn denies(&self, cmd: &str) -> bool {
        self.deny.iter().any(|p| pattern_matches(p, cmd))
    }
    pub fn asks(&self, cmd: &str) -> bool {
        self.ask.iter().any(|p| pattern_matches(p, cmd))
    }
}

/// `Bash(git push:*)` -> `git push*`; bare `Bash` -> `*`. Other tools -> None.
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
        Some(prefix) => format!("{prefix}*"),
        None => inner.to_string(),
    })
}

/// Glob match where `*` matches any text. A pattern without `*` also matches
/// the same command followed by arguments (Cursor's `Shell(git)` style).
pub fn pattern_matches(pattern: &str, cmd: &str) -> bool {
    let cmd = cmd.trim();
    if !pattern.contains('*') {
        return cmd == pattern || cmd.starts_with(&format!("{pattern} "));
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut rest = cmd;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(pos) => rest = &rest[pos + part.len()..],
                None => return false,
            }
        }
    }
    rest.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn rule_patterns() {
        assert_eq!(
            tool_pattern("Bash(git status:*)", "Bash").unwrap(),
            "git status*"
        );
        assert_eq!(tool_pattern("Bash", "Bash").unwrap(), "*");
        assert_eq!(tool_pattern("Read(./x)", "Bash"), None);
        assert!(pattern_matches("git status*", "git status --short"));
        assert!(pattern_matches("git * --force", "git push origin --force"));
        assert!(!pattern_matches("git push*", "git status"));
        assert!(pattern_matches("git", "git log"));
        assert!(!pattern_matches("git", "gitk"));
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
