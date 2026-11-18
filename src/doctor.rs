//! `lagom doctor` (HK-02): one line per check, with a mark and the command that fixes it.
//! Hooks fail silently by design, so this is where a broken install shows.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::hook::Agent;
use crate::install::{self, CONTEXT_EVENTS, ForeignHook, LagomHook};
use crate::rewrite::shell_quote;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    Ok,
    Info,
    Warn,
    Fail,
}

impl Mark {
    pub fn name(self) -> &'static str {
        match self {
            Mark::Ok => "ok",
            Mark::Info => "info",
            Mark::Warn => "warn",
            Mark::Fail => "fail",
        }
    }
}

#[derive(Debug)]
pub struct Check {
    pub name: String,
    pub mark: Mark,
    pub detail: String,
    pub fix: Option<String>,
}

fn check(
    name: impl Into<String>,
    mark: Mark,
    detail: impl Into<String>,
    fix: Option<String>,
) -> Check {
    Check {
        name: name.into(),
        mark,
        detail: detail.into(),
        fix,
    }
}

/// Every check, in the order they print.
pub fn checks(cwd: &Path) -> Vec<Check> {
    let mut out = Vec::new();
    for agent in [Agent::Claude, Agent::Codex, Agent::Cursor] {
        hooks(agent, &mut out);
    }
    codex_trust(&mut out);
    other_tools(cwd, &mut out);
    data_dir(&mut out);
    claude_policy(cwd, &mut out);
    out
}

pub fn text(checks: &[Check]) -> String {
    let mut s = String::new();
    for c in checks {
        s += &format!("{:<4}  {}: {}\n", c.mark.name(), c.name, c.detail);
        if let Some(fix) = &c.fix {
            s += &format!("      fix: {fix}\n");
        }
    }
    s
}

pub fn json(checks: &[Check]) -> Value {
    json!({
        "lagom": env!("CARGO_PKG_VERSION"),
        "checks": checks.iter().map(|c| json!({
            "check": c.name,
            "mark": c.mark.name(),
            "detail": c.detail,
            "fix": c.fix,
        })).collect::<Vec<_>>(),
    })
}

/// The binary in every Lagom hook exists and is this version.
fn hooks(agent: Agent, out: &mut Vec<Check>) {
    let t = install::target(agent);
    let name = format!("{} hooks", agent.name());
    let install = format!("lagom install {}", agent.name());
    let hooks = match install::lagom_hooks(agent) {
        Ok(hooks) => hooks,
        Err(e) => {
            let fix = format!(
                "repair or move {}, then run {install}",
                t.hooks_file.display()
            );
            out.push(check(name, Mark::Fail, e, Some(fix)));
            return;
        }
    };
    if hooks.is_empty() {
        if t.app_dir.exists() {
            out.push(check(name, Mark::Info, "not installed", Some(install)));
        }
        return;
    }
    let ours = format!("lagom {}", env!("CARGO_PKG_VERSION"));
    let mut bins: Vec<&str> = hooks.iter().map(|h| h.bin.as_str()).collect();
    bins.sort();
    bins.dedup();
    for bin in bins {
        let (mark, detail) = match version_of(bin) {
            Some(v) if v == ours => (Mark::Ok, format!("{bin} ({v})")),
            Some(v) => (Mark::Warn, format!("{bin} is {v}, this is {ours}")),
            None if Path::new(bin).is_absolute() && !Path::new(bin).exists() => (
                Mark::Fail,
                format!("{bin} doesn't exist, so the hooks do nothing"),
            ),
            None => (
                Mark::Fail,
                format!("{bin} doesn't run, so the hooks do nothing"),
            ),
        };
        let fix = (mark != Mark::Ok).then(|| install.clone());
        out.push(check(name.clone(), mark, detail, fix));
    }
}

/// What `<bin> --version` prints, or `None` when it doesn't run within 5 seconds.
fn version_of(bin: &str) -> Option<String> {
    let bin = bin.to_string();
    let output = crate::hook::with_deadline(Duration::from_secs(5), move || {
        std::process::Command::new(bin)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
    })?
    .ok()?;
    let v = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !v.is_empty()).then_some(v)
}

/// Codex runs a hook only after the user trusts it in `/hooks`, and asks again when it
/// changes. It keeps one record per hook in `config.toml`:
/// `[hooks.state."<hooks file>:<event>:<group>:<handler>"]` with a `trusted_hash`
/// (codex-rs `hooks/src/engine/discovery.rs`). The hash format isn't documented, so this
/// checks that each of Lagom's hooks has a record, not that it is current.
fn codex_trust(out: &mut Vec<Check>) {
    let Ok(hooks) = install::lagom_hooks(Agent::Codex) else {
        return;
    };
    if hooks.is_empty() {
        return;
    }
    let file = install::target(Agent::Codex).hooks_file;
    let config = fs::read_to_string(install::codex_dir().join("config.toml")).unwrap_or_default();
    let mut untrusted: Vec<&str> = hooks
        .iter()
        .filter(|h| !codex_trusts(&config, &file, h))
        .map(|h| h.event.as_str())
        .collect();
    untrusted.dedup();
    let check = if untrusted.is_empty() {
        check(
            "codex trust",
            Mark::Ok,
            "Codex has a trust record for each of Lagom's hooks (it lists a changed hook again in /hooks)",
            None,
        )
    } else {
        check(
            "codex trust",
            Mark::Warn,
            if untrusted.len() == 1 {
                format!(
                    "Codex skips Lagom's {} hook until you trust it",
                    untrusted[0]
                )
            } else {
                format!(
                    "Codex skips Lagom's {} hooks until you trust them",
                    untrusted.join(" and ")
                )
            },
            Some("open Codex, run /hooks and trust Lagom's hooks".into()),
        )
    };
    out.push(check);
}

fn codex_trusts(config: &str, file: &Path, h: &LagomHook) -> bool {
    let mut files = vec![file.to_path_buf()];
    files.extend(fs::canonicalize(file));
    files.iter().any(|f| {
        let key = format!(
            "{}:{}:{}:{}",
            f.display(),
            snake_case(&h.event),
            h.group,
            h.handler
        );
        let headers = [
            format!(
                "[hooks.state.\"{}\"]",
                key.replace('\\', "\\\\").replace('"', "\\\"")
            ),
            format!("[hooks.state.'{key}']"),
        ];
        let mut lines = config
            .lines()
            .skip_while(|l| !headers.contains(&l.trim().to_string()));
        lines.next().is_some()
            && lines
                .take_while(|l| !l.trim_start().starts_with('['))
                .any(|l| l.trim_start().starts_with("trusted_hash"))
    })
}

/// `PreToolUse` → `pre_tool_use`, the event label in Codex's trust keys.
fn snake_case(event: &str) -> String {
    let mut s = String::new();
    for (i, c) in event.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            s.push('_');
        }
        s.push(c.to_ascii_lowercase());
    }
    s
}

/// Tools known to add answer rules at session start, as Lagom does (ponytail #332).
const STYLE_TOOLS: [&str; 2] = ["caveman", "ponytail"];
/// Tools known to rewrite shell commands, as Lagom does.
const REWRITERS: [&str; 4] = ["rtk", "caveman", "ponytail", "claude-mem"];

/// Other tools' hooks in the same files: a second set of answer rules at session start,
/// a second shell-command rewriter, and Graphify. Each also blurs a style experiment
/// (D-38). One line per tool, naming its events.
fn other_tools(cwd: &Path, out: &mut Vec<Check>) {
    let experiment = crate::gain::experiment::Experiment::active().is_some();
    // (kind, where, tool, events)
    let mut found: Vec<(&str, String, String, Vec<String>)> = Vec::new();
    let mut graphify: Vec<String> = Vec::new();
    for h in install::foreign_hooks() {
        let (place, tool) = match &h.plugin {
            Some(name) => (format!("{} plugin", h.agent), name.clone()),
            None => (format!("{} hook", h.agent), program(&h.command)),
        };
        let kind = if h.command.contains("graphify") {
            let at = format!("{tool} ({place}, {})", h.event);
            if !graphify.contains(&at) {
                graphify.push(at);
            }
            continue;
        } else if CONTEXT_EVENTS.contains(&h.event.as_str()) {
            "context"
        } else if runs_before_shell(&h) {
            "shell"
        } else {
            continue;
        };
        match found
            .iter_mut()
            .find(|(k, p, t, _)| *k == kind && *p == place && *t == tool)
        {
            Some((_, _, _, events)) if !events.contains(&h.event) => events.push(h.event),
            Some(_) => {}
            None => found.push((kind, place, tool, vec![h.event])),
        }
    }
    for (kind, place, tool, events) in found {
        let at = format!("{tool} ({place}, {})", events.join(", "));
        let words = format!("{tool} {place}");
        out.push(match (kind, known(&words, &STYLE_TOOLS), known(&words, &REWRITERS)) {
            ("context", Some(_), _) => check(
                "stacked styles",
                Mark::Warn,
                format!("{at} adds its own answer rules, so the model gets two sets"),
                Some("keep one: remove its hooks, or run `lagom style use off`".into()),
            ),
            ("context", None, _) => check(
                "session-start text",
                if experiment { Mark::Warn } else { Mark::Info },
                format!("{at} adds text at session start; during a style experiment it reaches both groups and blurs the result"),
                experiment.then(|| "turn it off until the experiment ends".into()),
            ),
            (_, _, Some(_)) => check(
                "second rewriter",
                Mark::Warn,
                format!("{at} rewrites shell commands too, so the two wrap each other's output"),
                Some("keep one: remove its hooks, or run `lagom uninstall`".into()),
            ),
            _ => check(
                "shell hook",
                Mark::Info,
                format!("{at} sees shell commands before they run; if it rewrites them, it and Lagom wrap each other"),
                None,
            ),
        });
    }
    for file in instruction_files(cwd) {
        let has_block = fs::read_to_string(&file).is_ok_and(|t| {
            t.lines()
                .any(|l| l.trim().eq_ignore_ascii_case("## graphify"))
        });
        if has_block {
            graphify.push(file.display().to_string());
        }
    }
    if !graphify.is_empty() {
        let (mark, fix) = if experiment {
            (
                Mark::Warn,
                Some("turn Graphify off until the experiment ends".into()),
            )
        } else {
            (Mark::Info, None)
        };
        out.push(check(
            "graphify",
            mark,
            format!("Graphify ({}) steers what the agent reads, so it changes answers in both groups of a style experiment", graphify.join("; ")),
            fix,
        ));
    }
}

/// A hook that runs before a shell command: Claude Code's and Codex's `PreToolUse` for
/// Bash (or every tool), Cursor's `preToolUse` for Shell and `beforeShellExecution`.
fn runs_before_shell(h: &ForeignHook) -> bool {
    match h.event.as_str() {
        "beforeShellExecution" => true,
        "PreToolUse" | "preToolUse" => match h.matcher.as_deref() {
            None | Some("" | "*") => true,
            Some(m) => ["Bash", "bash", "Shell"].iter().any(|t| m.contains(t)),
        },
        _ => false,
    }
}

/// The first of `names` that `text` runs, by a path or word named after it
/// (`caveman-blocks`).
fn known(text: &str, names: &[&'static str]) -> Option<&'static str> {
    let words: Vec<String> = text
        .split(|c: char| c.is_whitespace() || "/'\"@".contains(c))
        .map(str::to_ascii_lowercase)
        .collect();
    names.iter().copied().find(|name| {
        words
            .iter()
            .any(|w| w == name || w.starts_with(&format!("{name}-")))
    })
}

/// Programs that run a script: the script names the tool better.
const INTERPRETERS: [&str; 8] = [
    "node", "python", "python3", "bash", "sh", "bun", "deno", "uv",
];

/// A short name for what a hook command runs: its program, or the script an interpreter
/// runs (`node caveman-hook.js`).
fn program(command: &str) -> String {
    let base = |w: &str| {
        Path::new(w)
            .file_name()
            .map_or(w.to_string(), |n| n.to_string_lossy().into_owned())
    };
    let (first, rest) = install::first_word(command.trim());
    let name = base(&first);
    let (second, _) = install::first_word(rest.trim());
    if INTERPRETERS.contains(&name.as_str()) && !second.is_empty() && !second.starts_with('-') {
        format!("{name} {}", base(&second))
    } else {
        name
    }
}

/// The instruction files the agents read: the user's `~/.claude/CLAUDE.md`, and
/// `CLAUDE.md` and `AGENTS.md` in `cwd` and every folder above it.
fn instruction_files(cwd: &Path) -> Vec<PathBuf> {
    let mut files = vec![install::claude_dir().join("CLAUDE.md")];
    for dir in cwd.ancestors() {
        files.push(dir.join("CLAUDE.md"));
        files.push(dir.join("AGENTS.md"));
    }
    files.retain(|f| f.is_file());
    files
}

/// `~/.lagom` holds raw command output, which can contain secrets: only the user may
/// read it (folders 0700, files 0600).
fn data_dir(out: &mut Vec<Check>) {
    let dir = crate::store::lagom_dir();
    let name = "data folder";
    if !dir.exists() {
        out.push(check(
            name,
            Mark::Ok,
            format!("{} doesn't exist yet", dir.display()),
            None,
        ));
        return;
    }
    #[cfg(unix)]
    {
        let mut open = Vec::new();
        readable_by_others(&fs::canonicalize(&dir).unwrap_or(dir.clone()), &mut open);
        out.push(match open.first() {
            None => check(
                name,
                Mark::Ok,
                format!("only you can read {}", dir.display()),
                None,
            ),
            Some(first) => check(
                name,
                Mark::Warn,
                format!(
                    "other users can open {} in {}, e.g. {}",
                    if open.len() == 1 {
                        "1 item".to_string()
                    } else {
                        format!("{} items", open.len())
                    },
                    dir.display(),
                    first.display()
                ),
                Some(format!(
                    "chmod -R go-rwx {}",
                    shell_quote(&dir.to_string_lossy())
                )),
            ),
        });
    }
    #[cfg(not(unix))]
    out.push(check(
        name,
        Mark::Info,
        format!(
            "permissions of {} aren't checked on this system",
            dir.display()
        ),
        None,
    ));
}

/// Entries under `path` (itself included) that group or others can read, write or enter.
/// The folder marker holds no data and is skipped; symbolic links aren't followed.
#[cfg(unix)]
fn readable_by_others(path: &Path, open: &mut Vec<PathBuf>) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    let is_marker = path
        .file_name()
        .is_some_and(|n| n == crate::store::DIR_MARKER);
    if meta.permissions().mode() & 0o077 != 0 && !is_marker && !meta.is_symlink() {
        open.push(path.to_path_buf());
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path).into_iter().flatten().flatten() {
            readable_by_others(&entry.path(), open);
        }
    }
}

/// Claude Code's managed and server-managed policy, and whether Lagom still rewrites
/// commands under it (HK-12).
fn claude_policy(cwd: &Path, out: &mut Vec<Check>) {
    if !install::claude_dir().exists() {
        return;
    }
    let p = crate::hook::claude_policy(cwd);
    let name = "claude policy";
    let files: Vec<String> = p
        .managed_files
        .iter()
        .map(|f| f.display().to_string())
        .collect();
    let remote = install::claude_dir().join("remote-settings.json");
    if p.opaque {
        out.push(check(
            name,
            Mark::Warn,
            "Claude Code may apply an organization policy Lagom can't read (an MDM profile, or the registry on Windows), so Lagom leaves Claude Code's commands uncompressed",
            None,
        ));
    } else if !p.rewrites {
        let unreadable = if p.server_managed == Some(false) {
            remote.display().to_string()
        } else {
            files.join(", ")
        };
        out.push(check(
            name,
            Mark::Warn,
            format!("Lagom can't read {unreadable}, which may deny commands, so it leaves Claude Code's commands uncompressed"),
            Some(format!("check that you can read it: ls -l {}", shell_quote(&unreadable))),
        ));
    } else {
        let managed = if files.is_empty() {
            "no managed settings".to_string()
        } else {
            format!("managed settings in {}", files.join(", "))
        };
        let server = match p.server_managed {
            Some(_) => "server-managed settings cached",
            None => "no server-managed settings",
        };
        out.push(check(
            name,
            Mark::Ok,
            format!(
                "{managed}, {server}; Lagom rewrites commands and follows their deny and ask rules"
            ),
            None,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_event_labels_are_snake_case() {
        assert_eq!(snake_case("PreToolUse"), "pre_tool_use");
        assert_eq!(snake_case("SessionStart"), "session_start");
    }

    #[test]
    fn known_tools_are_found_by_their_paths() {
        let cmd = "/Users/me/.local/share/caveman-blocks/bin/caveman-blocks hook --harness claude";
        assert_eq!(known(cmd, &REWRITERS), Some("caveman"));
        assert_eq!(known("rtk hook claude", &REWRITERS), Some("rtk"));
        assert_eq!(known("ponytail@market", &STYLE_TOOLS), Some("ponytail"));
        assert_eq!(known("/usr/bin/artkit check", &REWRITERS), None);
        assert_eq!(known("guard-tool check", &REWRITERS), None);
    }

    #[test]
    fn a_hook_is_named_by_its_program_or_script() {
        assert_eq!(
            program("'/opt/caveman/bin/caveman-proxy' native-hook claude"),
            "caveman-proxy"
        );
        assert_eq!(
            program("'/usr/bin/node' '/x/dist/hook.js' --fast"),
            "node hook.js"
        );
        assert_eq!(program("bash -c 'echo hi'"), "bash");
    }

    #[test]
    fn a_codex_trust_record_needs_a_trusted_hash() {
        let file = Path::new("/home/me/.codex/hooks.json");
        let hook = LagomHook {
            event: "PreToolUse".into(),
            group: 1,
            handler: 0,
            bin: "lagom".into(),
        };
        let trusted = "[hooks.state.\"/home/me/.codex/hooks.json:pre_tool_use:1:0\"]\ntrusted_hash = \"sha256:ab\"\n";
        assert!(codex_trusts(trusted, file, &hook));
        let disabled = "[hooks.state.\"/home/me/.codex/hooks.json:pre_tool_use:1:0\"]\nenabled = false\n[x]\ntrusted_hash = \"y\"\n";
        assert!(!codex_trusts(disabled, file, &hook));
        let other_position = trusted.replace(":1:0", ":0:0");
        assert!(!codex_trusts(&other_position, file, &hook));
    }
}
