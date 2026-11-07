//! Install/uninstall Lagom hooks and skills for Claude Code, Codex and Cursor.
//!
//! Safety rules:
//! - Our hook entries are recognized only by their command: `<path>/lagom hook <agent> <event>`.
//! - Existing entries are updated in place; an unchanged file is never rewritten
//!   (Codex trust records depend on the entry staying byte-identical).
//! - The first change to an existing file keeps a `<file>.lagom.bak` copy; writes are atomic.
//! - Files that are not valid JSON objects are refused, never overwritten.
//! - Skill folders are only touched when they contain our `.lagom` marker.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::hook::Agent;
use crate::rewrite::shell_quote;
use crate::store::home_dir;

pub const SKILLS: [(&str, &str); 5] = [
    ("lagom", include_str!("../skills/lagom/SKILL.md")),
    ("lagom-run", include_str!("../skills/lagom-run/SKILL.md")),
    (
        "lagom-review",
        include_str!("../skills/lagom-review/SKILL.md"),
    ),
    (
        "lagom-commit",
        include_str!("../skills/lagom-commit/SKILL.md"),
    ),
    (
        "lagom-stats",
        include_str!("../skills/lagom-stats/SKILL.md"),
    ),
];

const MARKER: &str = ".lagom";

pub fn claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".claude"))
}

fn codex_dir() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".codex"))
}

fn cursor_dir() -> PathBuf {
    home_dir().join(".cursor")
}

/// Config locations for one agent.
pub struct Target {
    pub agent: Agent,
    pub app_dir: PathBuf,
    pub hooks_file: PathBuf,
    pub skills_dir: PathBuf,
}

pub fn target(agent: Agent) -> Target {
    match agent {
        Agent::Claude => Target {
            agent,
            app_dir: claude_dir(),
            hooks_file: claude_dir().join("settings.json"),
            skills_dir: claude_dir().join("skills"),
        },
        Agent::Codex => Target {
            agent,
            app_dir: codex_dir(),
            hooks_file: codex_dir().join("hooks.json"),
            skills_dir: home_dir().join(".agents").join("skills"),
        },
        Agent::Cursor => Target {
            agent,
            app_dir: cursor_dir(),
            hooks_file: cursor_dir().join("hooks.json"),
            skills_dir: cursor_dir().join("skills"),
        },
    }
}

/// (event key in the agent's file, our event name, matcher, timeout seconds)
fn hook_specs(agent: Agent) -> Vec<(&'static str, &'static str, Option<&'static str>, u64)> {
    match agent {
        Agent::Claude => vec![
            (
                "SessionStart",
                "session-start",
                Some("startup|resume|clear|compact"),
                10,
            ),
            ("SubagentStart", "subagent-start", None, 10),
            ("PreToolUse", "pre-tool", Some("Bash"), 10),
        ],
        Agent::Codex => vec![
            (
                "SessionStart",
                "session-start",
                Some("startup|resume|clear|compact"),
                10,
            ),
            ("PreToolUse", "pre-tool", Some("Bash|bash"), 10),
        ],
        Agent::Cursor => vec![
            ("sessionStart", "session-start", None, 10),
            ("preToolUse", "pre-tool", Some("Shell"), 10),
        ],
    }
}

fn hook_command(bin: &str, agent: Agent, event: &str) -> String {
    format!("{} hook {} {event}", shell_quote(bin), agent.name())
}

/// True for commands we wrote: first word is a path ending in `lagom` (or the `lgm` alias), second is `hook`.
pub fn is_lagom_command(cmd: &str) -> bool {
    let cmd = cmd.trim();
    let (first, rest) = if let Some(stripped) = cmd.strip_prefix('\'') {
        match stripped.find('\'') {
            Some(end) => (&stripped[..end], &stripped[end + 1..]),
            None => return false,
        }
    } else {
        cmd.split_once(' ').unwrap_or((cmd, ""))
    };
    let base = Path::new(first)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    matches!(base, "lagom" | "lagom.exe" | "lgm" | "lgm.exe")
        && rest.trim_start().starts_with("hook ")
}

fn is_ours(agent: Agent, item: &Value) -> bool {
    let cmd_of = |v: &Value| {
        v.get("command")
            .and_then(Value::as_str)
            .is_some_and(is_lagom_command)
    };
    match agent {
        Agent::Cursor => cmd_of(item),
        Agent::Claude | Agent::Codex => item
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hs| hs.iter().any(cmd_of)),
    }
}

fn entry(agent: Agent, bin: &str, event: &str, matcher: Option<&str>, timeout: u64) -> Value {
    let command = hook_command(bin, agent, event);
    match agent {
        Agent::Cursor => {
            let mut e = json!({"command": command, "timeout": timeout});
            if let Some(m) = matcher {
                e["matcher"] = json!(m);
            }
            e
        }
        Agent::Claude | Agent::Codex => {
            let mut g =
                json!({"hooks": [{"type": "command", "command": command, "timeout": timeout}]});
            if let Some(m) = matcher {
                g = json!({"matcher": m, "hooks": g["hooks"].clone()});
            }
            g
        }
    }
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>, String> {
    match fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
        Ok(text) if text.trim().is_empty() => Ok(Map::new()),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(m)) => Ok(m),
            _ => Err(format!(
                "{} is not a plain JSON object; not changing it",
                path.display()
            )),
        },
    }
}

/// Where the first change to an existing file keeps a copy of the original.
fn backup_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.lagom.bak", path.display()))
}

/// Dry-run note for the backup `write_json_atomic` would create, if any.
fn backup_note(path: &Path) -> Option<String> {
    let backup = backup_path(path);
    (path.exists() && !backup.exists())
        .then(|| format!("would back up the original to {}", backup.display()))
}

fn write_json_atomic(path: &Path, value: &Map<String, Value>) -> Result<(), String> {
    let err = |e: std::io::Error| format!("cannot write {}: {e}", path.display());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(err)?;
    }
    let backup = backup_path(path);
    if path.exists() && !backup.exists() {
        fs::copy(path, &backup).map_err(err)?;
    }
    // A settings file linked from a dotfiles folder stays a link: write its target.
    let target = match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => fs::canonicalize(path).map_err(err)?,
        _ => path.to_path_buf(),
    };
    let path = target.as_path();
    let tmp = PathBuf::from(format!("{}.lagom.tmp", path.display()));
    let text = serde_json::to_string_pretty(&Value::Object(value.clone()))
        .map_err(|e| e.to_string())?
        + "\n";
    fs::write(&tmp, text).map_err(err)?;
    // Keep the original file's permissions (a private settings file stays private).
    if let Ok(meta) = fs::metadata(path) {
        fs::set_permissions(&tmp, meta.permissions()).map_err(err)?;
    }
    fs::rename(&tmp, path).map_err(err)
}

/// Add or update our hooks in `config`. Returns true if anything changed.
pub fn merge_hooks(agent: Agent, config: &mut Map<String, Value>, bin: &str) -> bool {
    let before = config.clone();
    if agent == Agent::Cursor && !config.contains_key("version") {
        config.insert("version".into(), json!(1));
    }
    let hooks = config.entry("hooks").or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let hooks = hooks.as_object_mut().expect("hooks is an object");
    for (key, event, matcher, timeout) in hook_specs(agent) {
        let new = entry(agent, bin, event, matcher, timeout);
        let list = hooks.entry(key).or_insert_with(|| json!([]));
        if !list.is_array() {
            *list = json!([]);
        }
        let list = list.as_array_mut().expect("event list is an array");
        match list.iter().position(|item| is_ours(agent, item)) {
            Some(i) => {
                if list[i] != new {
                    list[i] = new;
                }
                // Drop any duplicate entries of ours after the first.
                let mut seen = false;
                list.retain(|item| {
                    if !is_ours(agent, item) {
                        return true;
                    }
                    let keep = !seen;
                    seen = true;
                    keep
                });
            }
            None => list.push(new),
        }
    }
    *config != before
}

/// Remove our hooks from `config`. Returns true if anything changed.
pub fn remove_hooks(agent: Agent, config: &mut Map<String, Value>) -> bool {
    let before = config.clone();
    if let Some(hooks) = config.get_mut("hooks").and_then(Value::as_object_mut) {
        for list in hooks.values_mut().filter_map(Value::as_array_mut) {
            match agent {
                Agent::Cursor => list.retain(|item| !is_ours(agent, item)),
                Agent::Claude | Agent::Codex => {
                    for group in list.iter_mut() {
                        if let Some(hs) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                            hs.retain(|h| {
                                !h.get("command")
                                    .and_then(Value::as_str)
                                    .is_some_and(is_lagom_command)
                            });
                        }
                    }
                    list.retain(|g| {
                        g.get("hooks")
                            .and_then(Value::as_array)
                            .is_none_or(|hs| !hs.is_empty())
                    });
                }
            }
        }
        hooks.retain(|_, list| list.as_array().is_none_or(|l| !l.is_empty()));
        if hooks.is_empty() {
            config.remove("hooks");
        }
    }
    *config != before
}

/// Result of installing into or removing from one agent.
#[derive(Debug, Default)]
pub struct Outcome {
    pub notes: Vec<String>,
    /// Something was (or, in a dry run, would be) changed.
    pub changed: bool,
}

/// Write our skills into `dir`. Returns notes and whether anything changed.
fn install_skills(dir: &Path, dry_run: bool) -> Result<(Vec<String>, bool), String> {
    let mut notes = Vec::new();
    let mut changed = false;
    for (name, text) in SKILLS {
        let skill_dir = dir.join(name);
        let ours = skill_dir.join(MARKER).exists();
        if skill_dir.exists() && !ours {
            notes.push(format!(
                "skipped skill {name}: {} exists and is not Lagom's",
                skill_dir.display()
            ));
            continue;
        }
        let current = fs::read_to_string(skill_dir.join("SKILL.md")).ok();
        if ours && current.as_deref() == Some(text) {
            continue;
        }
        changed = true;
        if dry_run {
            notes.push(format!("would write skill {}", skill_dir.display()));
            continue;
        }
        let err = |e: std::io::Error| format!("cannot write skill {name}: {e}");
        fs::create_dir_all(&skill_dir).map_err(err)?;
        fs::write(skill_dir.join("SKILL.md"), text).map_err(err)?;
        fs::write(
            skill_dir.join(MARKER),
            "Installed by lagom. `lagom uninstall` removes this folder.\n",
        )
        .map_err(err)?;
    }
    Ok((notes, changed))
}

/// Remove our skills from `dir` (or count them in a dry run).
fn uninstall_skills(dir: &Path, dry_run: bool) -> usize {
    SKILLS
        .iter()
        .map(|(name, _)| dir.join(name))
        .filter(|d| d.join(MARKER).exists())
        .filter(|d| dry_run || fs::remove_dir_all(d).is_ok())
        .count()
}

/// Install hooks and skills for `agent`. With `dry_run`, only report what would change.
pub fn install(agent: Agent, bin: &str, dry_run: bool) -> Result<Outcome, String> {
    let t = target(agent);
    let mut out = Outcome::default();
    let mut config = read_json_object(&t.hooks_file)?;
    if merge_hooks(agent, &mut config, bin) {
        out.changed = true;
        if dry_run {
            out.notes
                .push(format!("hooks: would update {}", t.hooks_file.display()));
            out.notes.extend(backup_note(&t.hooks_file));
        } else {
            write_json_atomic(&t.hooks_file, &config)?;
            out.notes
                .push(format!("hooks: updated {}", t.hooks_file.display()));
        }
    } else {
        out.notes.push(format!(
            "hooks: already up to date in {}",
            t.hooks_file.display()
        ));
    }
    let (skill_notes, skills_changed) = install_skills(&t.skills_dir, dry_run)?;
    out.notes.extend(skill_notes);
    out.changed |= skills_changed;
    if !dry_run {
        out.notes.push(format!(
            "skills: {} in {}",
            SKILLS.len(),
            t.skills_dir.display()
        ));
    }
    if agent == Agent::Codex && out.changed {
        out.notes.push("Codex: open Codex and run /hooks to review and trust the lagom hooks; they stay inactive until trusted.".into());
    }
    Ok(out)
}

/// Remove Lagom's hooks and skills from `agent`. With `dry_run`, only report.
pub fn uninstall(agent: Agent, dry_run: bool) -> Result<Outcome, String> {
    let t = target(agent);
    let mut out = Outcome::default();
    if t.hooks_file.exists() {
        let mut config = read_json_object(&t.hooks_file)?;
        if remove_hooks(agent, &mut config) {
            out.changed = true;
            let only_version = config.len() == 1 && config.contains_key("version");
            let delete = config.is_empty() || (agent == Agent::Cursor && only_version);
            let verb = if dry_run { "would remove" } else { "removed" };
            if !dry_run && delete {
                fs::remove_file(&t.hooks_file).map_err(|e| e.to_string())?;
            } else if !dry_run {
                write_json_atomic(&t.hooks_file, &config)?;
            }
            out.notes.push(if delete {
                format!(
                    "hooks: {verb} {} (only Lagom hooks were in it)",
                    t.hooks_file.display()
                )
            } else {
                format!("hooks: {verb} from {}", t.hooks_file.display())
            });
            if dry_run && !delete {
                out.notes.extend(backup_note(&t.hooks_file));
            }
        } else {
            out.notes
                .push(format!("hooks: none found in {}", t.hooks_file.display()));
        }
    }
    let skills = uninstall_skills(&t.skills_dir, dry_run);
    out.changed |= skills > 0;
    out.notes.push(format!(
        "skills: {} {skills} from {}",
        if dry_run { "would remove" } else { "removed" },
        t.skills_dir.display()
    ));
    Ok(out)
}

/// Folders `--purge` deletes: Lagom's data (`~/.lagom`) and config (`~/.config/lagom`).
pub fn purge_targets() -> Vec<PathBuf> {
    vec![crate::store::lagom_dir(), crate::style::config_dir()]
}

/// Delete Lagom's data and config folders. Refuses folders that look unsafe to delete
/// (the home folder, an ancestor of it, or a filesystem root), e.g. a bad `LAGOM_HOME`.
pub fn purge(dry_run: bool) -> Result<Vec<String>, String> {
    let home = home_dir();
    let mut notes = Vec::new();
    for dir in purge_targets() {
        if !dir.exists() {
            continue;
        }
        let shallow = dir.components().count() < 3;
        if shallow || home.starts_with(&dir) {
            return Err(format!(
                "refusing to delete {}: not a Lagom folder",
                dir.display()
            ));
        }
        if dry_run {
            notes.push(format!("would delete {}", dir.display()));
        } else {
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("cannot delete {}: {e}", dir.display()))?;
            notes.push(format!("deleted {}", dir.display()));
        }
    }
    Ok(notes)
}

/// The command that removes the binary itself, guessed from where it lives.
pub fn removal_hint(exe: &Path) -> String {
    let has = |part: &str| exe.components().any(|c| c.as_os_str() == part);
    let dir = exe.parent().unwrap_or(Path::new("."));
    if has("Cellar") || exe.starts_with("/opt/homebrew") || exe.starts_with("/home/linuxbrew") {
        "brew uninstall lagom".into()
    } else if has(".cargo") {
        "cargo uninstall lagom".into()
    } else if has("pipx") {
        "pipx uninstall lagom-ai".into()
    } else if has("site-packages") || exe.to_string_lossy().contains("/.local/share/uv/") {
        "pip uninstall lagom-ai (or: uv tool uninstall lagom-ai)".into()
    } else if cfg!(windows) && has("WinGet") {
        "winget uninstall yuraware.lagom".into()
    } else {
        let ext = std::env::consts::EXE_SUFFIX;
        format!(
            "rm {} {}",
            shell_quote(&dir.join(format!("lagom{ext}")).to_string_lossy()),
            shell_quote(&dir.join(format!("lgm{ext}")).to_string_lossy())
        )
    }
}

/// (agent, hooks installed, skills installed)
pub fn status(agent: Agent) -> (bool, usize) {
    let t = target(agent);
    let hooks = read_json_object(&t.hooks_file)
        .ok()
        .and_then(|c| c.get("hooks").cloned())
        .and_then(|h| h.as_object().cloned())
        .is_some_and(|h| {
            h.values()
                .filter_map(Value::as_array)
                .flatten()
                .any(|i| is_ours(agent, i))
        });
    let skills = SKILLS
        .iter()
        .filter(|(n, _)| t.skills_dir.join(n).join(MARKER).exists())
        .count();
    (hooks, skills)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removal_hint_matches_install_method() {
        let hint = |p: &str| removal_hint(Path::new(p));
        assert_eq!(
            hint("/opt/homebrew/Cellar/lagom/0.1.0/bin/lagom"),
            "brew uninstall lagom"
        );
        assert_eq!(hint("/opt/homebrew/bin/lagom"), "brew uninstall lagom");
        assert_eq!(hint("/home/a/.cargo/bin/lagom"), "cargo uninstall lagom");
        assert_eq!(
            hint("/home/a/.local/share/pipx/venvs/lagom-ai/bin/lagom"),
            "pipx uninstall lagom-ai"
        );
        assert!(hint("/home/a/.local/bin/lagom").starts_with("rm /home/a/.local/bin/lagom"));
    }

    const BIN: &str = "/opt/lagom/bin/lagom";

    #[test]
    fn recognizes_only_lagom_hook_commands() {
        assert!(is_lagom_command(
            "/opt/lagom/bin/lagom hook claude pre-tool"
        ));
        assert!(is_lagom_command(
            "'/Users/a b/lagom' hook codex session-start"
        ));
        assert!(is_lagom_command("/opt/lagom/bin/lgm hook cursor pre-tool"));
        assert!(!is_lagom_command("/usr/bin/caveman hook claude"));
        assert!(!is_lagom_command("lagom run -- git status"));
        assert!(!is_lagom_command("node /x/lagom-hooks.js hook"));
    }

    #[test]
    fn merge_is_idempotent_and_keeps_other_hooks() {
        let mut cfg: Map<String, Value> = serde_json::from_value(json!({
            "model": "opus",
            "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "other-tool"}]}]}
        }))
        .unwrap();
        assert!(merge_hooks(Agent::Claude, &mut cfg, BIN));
        let once = cfg.clone();
        assert!(!merge_hooks(Agent::Claude, &mut cfg, BIN));
        assert_eq!(cfg, once);
        let pre = cfg["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["hooks"][0]["command"], "other-tool");
        assert_eq!(cfg.keys().next().unwrap(), "model");
    }

    #[test]
    fn new_binary_path_replaces_in_place() {
        let mut cfg = Map::new();
        merge_hooks(Agent::Codex, &mut cfg, "/old/lagom");
        cfg["hooks"]["PreToolUse"]
            .as_array_mut()
            .unwrap()
            .push(json!({"hooks": [{"command": "x"}]}));
        assert!(merge_hooks(Agent::Codex, &mut cfg, BIN));
        let pre = cfg["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert!(
            pre[0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .starts_with(BIN)
        );
    }

    #[test]
    fn remove_restores_original() {
        let original: Map<String, Value> = serde_json::from_value(json!({
            "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "keep-me"}]}]}
        }))
        .unwrap();
        let mut cfg = original.clone();
        merge_hooks(Agent::Claude, &mut cfg, BIN);
        assert!(remove_hooks(Agent::Claude, &mut cfg));
        assert_eq!(cfg, original);
    }

    #[test]
    fn cursor_uses_flat_entries() {
        let mut cfg = Map::new();
        merge_hooks(Agent::Cursor, &mut cfg, BIN);
        assert_eq!(cfg["version"], 1);
        let pre = &cfg["hooks"]["preToolUse"][0];
        assert_eq!(pre["matcher"], "Shell");
        assert_eq!(pre["command"], format!("{BIN} hook cursor pre-tool"));
        remove_hooks(Agent::Cursor, &mut cfg);
        assert_eq!(cfg.len(), 1);
    }
}
