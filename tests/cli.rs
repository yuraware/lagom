//! End-to-end tests of the `lagom` binary against a throwaway HOME.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lagom-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn lagom(home: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lagom"));
    cmd.args(args)
        .env("HOME", home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CLAUDE_PROJECT_DIR")
        .env_remove("CODEX_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("LAGOM_HOME")
        .env_remove("LAGOM_STYLES")
        .env_remove("LAGOM_DISABLE")
        .env_remove("LAGOM_REWRITE")
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn install_and_uninstall_round_trip_for_all_agents() {
    let home = sandbox("install");
    for dir in [".claude", ".codex", ".cursor"] {
        fs::create_dir_all(home.join(dir)).unwrap();
    }
    let claude_settings = json!({"model": "opus", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "keep-me"}]}]}});
    fs::write(
        home.join(".claude/settings.json"),
        claude_settings.to_string(),
    )
    .unwrap();

    let out = lagom(&home, &["install"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Only Cursor can't re-inject styles after compaction, and install says so once.
    let first = text(&out);
    assert!(
        first.contains("cursor: styles are not injected again after the chat is compacted"),
        "{first}"
    );
    assert!(
        !first.contains("claude: styles are not injected"),
        "{first}"
    );
    assert!(!first.contains("codex: styles are not injected"), "{first}");
    let settings: Value =
        serde_json::from_str(&fs::read_to_string(home.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings["hooks"]["Stop"][0]["hooks"][0]["command"],
        "keep-me"
    );
    assert!(
        settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .ends_with("hook claude pre-tool")
    );
    assert!(home.join(".claude/settings.json.lagom.bak").exists());
    assert!(home.join(".claude/skills/lagom/SKILL.md").exists());
    assert!(home.join(".agents/skills/lagom-run/SKILL.md").exists());
    assert!(home.join(".cursor/skills/lagom-review/SKILL.md").exists());
    let cursor: Value =
        serde_json::from_str(&fs::read_to_string(home.join(".cursor/hooks.json")).unwrap())
            .unwrap();
    assert_eq!(cursor["version"], 1);
    let codex_before = fs::read_to_string(home.join(".codex/hooks.json")).unwrap();

    // Re-install changes nothing (Codex trust depends on byte-identical entries).
    let again = lagom(&home, &["install"], None);
    assert!(text(&again).contains("already up to date"));
    assert!(!text(&again).contains("styles are not injected again"));
    assert_eq!(
        fs::read_to_string(home.join(".codex/hooks.json")).unwrap(),
        codex_before
    );

    let status = text(&lagom(&home, &["status"], None));
    assert!(status.contains("claude  hooks: yes"), "{status}");
    assert!(
        !status.contains("project languages"),
        "no project in the home folder: {status}"
    );
    // In a repository with marker files, status names its languages.
    let repo = home.join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("Cargo.toml"), "").unwrap();
    fs::write(repo.join("requirements.txt"), "").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lagom"))
        .arg("status")
        .env("HOME", &home)
        .env_remove("LAGOM_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(
        text(&out).contains("project languages: Rust, Python"),
        "{}",
        text(&out)
    );

    let out = lagom(&home, &["uninstall"], None);
    assert!(out.status.success());
    let settings: Value =
        serde_json::from_str(&fs::read_to_string(home.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings, claude_settings);
    assert!(!home.join(".cursor/hooks.json").exists());
    assert!(!home.join(".codex/hooks.json").exists());
    assert!(!home.join(".claude/skills/lagom").exists());
}

#[cfg(unix)]
#[test]
fn install_keeps_settings_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let home = sandbox("perms");
    fs::create_dir_all(home.join(".claude")).unwrap();
    let settings = home.join(".claude/settings.json");
    fs::write(&settings, "{}").unwrap();
    fs::set_permissions(&settings, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(lagom(&home, &["install", "claude"], None).status.success());
    assert_eq!(
        fs::metadata(&settings).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn install_keeps_a_symlinked_settings_file_a_symlink() {
    let home = sandbox("symlink");
    let dotfiles = home.join("dotfiles");
    fs::create_dir_all(&dotfiles).unwrap();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(dotfiles.join("settings.json"), "{\"model\": \"x\"}").unwrap();
    let link = home.join(".claude/settings.json");
    std::os::unix::fs::symlink(dotfiles.join("settings.json"), &link).unwrap();
    assert!(lagom(&home, &["install", "claude"], None).status.success());
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let target = fs::read_to_string(dotfiles.join("settings.json")).unwrap();
    assert!(
        target.contains("hook claude") && target.contains("\"model\""),
        "{target}"
    );
    assert!(!dotfiles.join("settings.json.lagom.tmp").exists());
    assert!(
        lagom(&home, &["uninstall", "claude"], None)
            .status
            .success()
    );
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let target = fs::read_to_string(dotfiles.join("settings.json")).unwrap();
    assert!(!target.contains("hook claude"), "{target}");
}

#[test]
fn install_refuses_invalid_json() {
    let home = sandbox("badjson");
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(home.join(".claude/settings.json"), "{ not json").unwrap();
    let out = lagom(&home, &["install", "claude"], None);
    assert!(!out.status.success());
    assert_eq!(
        fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        "{ not json"
    );
}

#[test]
fn user_skill_with_same_name_is_not_overwritten() {
    let home = sandbox("skillclash");
    fs::create_dir_all(home.join(".claude/skills/lagom")).unwrap();
    fs::write(home.join(".claude/skills/lagom/SKILL.md"), "mine").unwrap();
    let out = lagom(&home, &["install", "claude"], None);
    assert!(text(&out).contains("skipped skill lagom"));
    assert_eq!(
        fs::read_to_string(home.join(".claude/skills/lagom/SKILL.md")).unwrap(),
        "mine"
    );
    lagom(&home, &["uninstall", "claude"], None);
    assert!(home.join(".claude/skills/lagom/SKILL.md").exists());
}

#[test]
fn session_start_injects_styles_and_respects_off() {
    let home = sandbox("styles");
    let payload = json!({"hook_event_name": "SessionStart", "cwd": home}).to_string();
    let out: Value = serde_json::from_str(&text(&lagom(
        &home,
        &["hook", "claude", "session-start"],
        Some(&payload),
    )))
    .unwrap();
    let ctx = out["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains("lagom: terse") && ctx.contains("lagom: lean-code"));

    let cursor: Value = serde_json::from_str(&text(&lagom(
        &home,
        &["hook", "cursor", "session-start"],
        Some(&payload),
    )))
    .unwrap();
    assert!(
        cursor["additional_context"]
            .as_str()
            .unwrap()
            .contains("lagom: terse")
    );

    // Project style overrides, custom style, then off.
    fs::create_dir_all(home.join(".lagom/styles")).unwrap();
    fs::write(
        home.join(".lagom/styles/pirate.md"),
        "---\nname: pirate\ndescription: arr\n---\nTalk like a pirate.\n",
    )
    .unwrap();
    assert!(
        lagom(&home, &["style", "use", "--project", "pirate"], None)
            .status
            .success()
    );
    let ctx = text(&lagom(
        &home,
        &["hook", "codex", "session-start"],
        Some(&payload),
    ));
    assert!(ctx.contains("Talk like a pirate.") && !ctx.contains("lagom: terse"));

    lagom(&home, &["style", "use", "--project", "off"], None);
    assert_eq!(
        text(&lagom(
            &home,
            &["hook", "claude", "session-start"],
            Some(&payload)
        )),
        ""
    );
    assert!(
        lagom(&home, &["style", "use", "no-such-style"], None)
            .status
            .code()
            == Some(1)
    );
}

#[test]
fn claude_rewrite_requires_users_allow_rule() {
    let home = sandbox("perm");
    fs::create_dir_all(home.join(".claude")).unwrap();
    let payload = |cmd: &str, mode: &str| {
        json!({"tool_name": "Bash", "cwd": home, "permission_mode": mode, "tool_input": {"command": cmd, "description": "d"}})
            .to_string()
    };
    // No rules, default mode: untouched.
    assert_eq!(
        text(&lagom(
            &home,
            &["hook", "claude", "pre-tool"],
            Some(&payload("git log", "default"))
        )),
        ""
    );
    // Bypass mode: rewritten without granting permission.
    let out: Value = serde_json::from_str(&text(&lagom(
        &home,
        &["hook", "claude", "pre-tool"],
        Some(&payload("git log", "bypassPermissions")),
    )))
    .unwrap();
    assert!(
        out["hookSpecificOutput"]
            .get("permissionDecision")
            .is_none()
    );
    assert_eq!(
        out["hookSpecificOutput"]["updatedInput"]["description"],
        "d"
    );

    fs::write(
        home.join(".claude/settings.json"),
        json!({"permissions": {"allow": ["Bash(git log:*)", "Bash(git push:*)"], "deny": ["Bash(git push --force:*)"]}}).to_string(),
    )
    .unwrap();
    let out: Value = serde_json::from_str(&text(&lagom(
        &home,
        &["hook", "claude", "pre-tool"],
        Some(&payload("git log -5", "default")),
    )))
    .unwrap();
    assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "allow");
    assert!(
        out["hookSpecificOutput"]["updatedInput"]["command"]
            .as_str()
            .unwrap()
            .ends_with("run -- git log -5")
    );
    // Deny wins even in bypass mode.
    assert_eq!(
        text(&lagom(
            &home,
            &["hook", "claude", "pre-tool"],
            Some(&payload("git push --force origin", "bypassPermissions"))
        )),
        ""
    );
    // Cursor without an allow rule: `{}` (no change).
    assert_eq!(
        text(&lagom(
            &home,
            &["hook", "cursor", "pre-tool"],
            Some(&payload("git log", "default"))
        ))
        .trim(),
        "{}"
    );
}

#[test]
fn run_compresses_keeps_exit_code_and_recall_works() {
    let home = sandbox("run");
    let script = "for i in $(seq 1 400); do echo \"row $i value $((i * 7 % 13))\"; done; echo 'src/app.rs:9: error: boom' >&2; exit 3";
    let out = lagom(&home, &["run", "--", "sh", "-c", script], None);
    assert_eq!(out.status.code(), Some(3));
    let stdout = text(&out);
    assert!(stdout.contains("src/app.rs:9: error: boom"));
    assert!(
        stdout.lines().count() < 300,
        "{} lines",
        stdout.lines().count()
    );
    let id = stdout
        .rsplit("lagom recall ")
        .next()
        .unwrap()
        .split(|c: char| c.is_whitespace() || c == ']')
        .next()
        .unwrap()
        .to_string();
    let full = text(&lagom(&home, &["recall", &id], None));
    assert_eq!(
        text(&lagom(&home, &["recall", &id, "--grep", "error"], None)),
        "401: src/app.rs:9: error: boom\n"
    );
    assert_eq!(
        text(&lagom(&home, &["recall", &id, "--lines", "2-3"], None))
            .lines()
            .count(),
        2
    );
    assert_eq!(
        lagom(&home, &["recall", &id, "--lines", "x"], None)
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        lagom(&home, &["recall", "../stats", "--head", "1"], None)
            .status
            .code(),
        Some(2),
        "not a recall id"
    );
    assert_eq!(full.lines().count(), 401);

    let stats = text(&lagom(&home, &["stats"], None));
    assert!(
        stats.contains("commands run through lagom: 1 (1 compressed)"),
        "{stats}"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: PathBuf| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(home.join(".lagom")), 0o700);
        assert_eq!(mode(home.join(".lagom/recall")), 0o700);
        assert_eq!(mode(home.join(".lagom/stats.jsonl")), 0o600);
        assert_eq!(mode(home.join(format!(".lagom/recall/{id}.log"))), 0o600);
    }

    let list = text(&lagom(&home, &["recall", "--list"], None));
    assert!(
        list.contains(&id) && list.contains("  sh") && list.contains("1 saved outputs"),
        "{list}"
    );
    assert!(text(&lagom(&home, &["recall", "--clear"], None)).contains("removed 1"));
    assert_eq!(lagom(&home, &["recall", &id], None).status.code(), Some(1));

    let missing = lagom(&home, &["run", "--", "definitely-not-a-command-xyz"], None);
    assert_eq!(missing.status.code(), Some(127));
}

#[test]
fn lgm_alias_matches_lagom_and_hooks_point_to_lagom() {
    let home = sandbox("lgm");
    fs::create_dir_all(home.join(".claude")).unwrap();
    let mut alias = Command::new(env!("CARGO_BIN_EXE_lgm"));
    alias.env("HOME", &home).env_remove("CLAUDE_CONFIG_DIR");
    let version = alias.arg("--version").output().unwrap();
    assert_eq!(text(&version), text(&lagom(&home, &["--version"], None)));

    let out = Command::new(env!("CARGO_BIN_EXE_lgm"))
        .args(["install", "claude"])
        .env("HOME", &home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let settings = fs::read_to_string(home.join(".claude/settings.json")).unwrap();
    assert!(
        settings.contains("lagom' hook claude") || settings.contains("lagom hook claude"),
        "{settings}"
    );
    assert!(!settings.contains("lgm"), "{settings}");

    // Installing again under the other name changes nothing; uninstall by either name works.
    assert!(text(&lagom(&home, &["install", "claude"], None)).contains("already up to date"));
    assert!(
        lagom(&home, &["uninstall", "claude"], None)
            .status
            .success()
    );
    let settings = fs::read_to_string(home.join(".claude/settings.json")).unwrap_or_default();
    assert!(!settings.contains("hook claude"), "{settings}");
}

#[test]
fn hook_ignores_oversized_input_quickly() {
    let home = sandbox("bigstdin");
    let mut child = Command::new(env!("CARGO_BIN_EXE_lagom"))
        .args(["hook", "cursor", "pre-tool"])
        .env("HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let chunk = vec![b' '; 1024 * 1024];
        let mut written = 0;
        for _ in 0..50 {
            if stdin.write_all(&chunk).is_err() {
                break; // the hook stopped reading: expected
            }
            written += 1;
        }
        written
    });
    let out = child.wait_with_output().unwrap();
    let written = writer.join().unwrap();
    assert!(out.status.success());
    // It read about 8 MB and stopped, instead of reading all 50 MB.
    assert!(written < 50, "the hook read all {written} MB");
    assert_eq!(text(&out).trim(), "{}");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn install_dry_run_changes_nothing_and_summarizes() {
    let home = sandbox("dryrun");
    fs::create_dir_all(home.join(".claude")).unwrap();
    let out = lagom(&home, &["install", "--dry-run"], None);
    assert!(out.status.success());
    let s = text(&out);
    assert!(s.contains("hooks: would update"), "{s}");
    assert!(s.contains("claude  would install"), "{s}");
    assert!(s.contains("codex   skipped (not found)"), "{s}");
    assert!(s.contains("Dry run: nothing was changed."), "{s}");
    assert!(!home.join(".claude/settings.json").exists());
    assert!(!home.join(".claude/skills").exists());

    let s = text(&lagom(&home, &["install", "--yes"], None));
    assert!(s.contains("claude  installed"), "{s}");
    let s = text(&lagom(&home, &["install", "-y"], None));
    assert!(s.contains("claude  already up to date"), "{s}");

    let s = text(&lagom(&home, &["uninstall", "claude", "--dry-run"], None));
    assert!(s.contains("claude  would remove"), "{s}");
    assert!(home.join(".claude/skills/lagom").exists());

    assert_eq!(
        lagom(&home, &["install", "--bogus"], None).status.code(),
        Some(2)
    );

    // A dry run names the backup the first real change would create, and creates nothing.
    let home = sandbox("dryrun-backup");
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(home.join(".claude/settings.json"), "{}").unwrap();
    let s = text(&lagom(&home, &["install", "claude", "--dry-run"], None));
    assert!(s.contains("would back up the original to"), "{s}");
    assert!(s.contains("settings.json.lagom.bak"), "{s}");
    assert!(!home.join(".claude/settings.json.lagom.bak").exists());
    assert_eq!(
        fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        "{}"
    );
}

/// Every file under `dir` that still belongs to Lagom: a path naming it (other than a
/// `.lagom.bak` backup) or a file whose text mentions a Lagom hook.
fn lagom_leftovers(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_lowercase();
        if path.is_dir() {
            if name.contains("lagom") {
                found.push(path.clone());
            }
            found.extend(lagom_leftovers(&path));
        } else if !name.ends_with(".lagom.bak") {
            let text = fs::read_to_string(&path).unwrap_or_default();
            if name.contains("lagom")
                || text.contains(" hook claude")
                || text.contains(" hook codex")
                || text.contains(" hook cursor")
            {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn uninstall_all_purge_removes_everything_but_backups() {
    let home = sandbox("purge");
    for agent in [".claude", ".codex", ".cursor"] {
        fs::create_dir_all(home.join(agent)).unwrap();
    }
    fs::write(home.join(".claude/settings.json"), r#"{"model":"opus"}"#).unwrap();
    assert!(lagom(&home, &["install", "--yes"], None).status.success());
    lagom(&home, &["style", "use", "terse"], None);
    lagom(&home, &["run", "--", "echo", "hi"], None);
    assert!(home.join(".lagom").exists() && home.join(".config/lagom").exists());

    // Without --yes and without a terminal, purge stops before changing anything.
    let out = lagom(&home, &["uninstall", "--all", "--purge"], None);
    assert_eq!(out.status.code(), Some(2));
    assert!(home.join(".lagom").exists());
    let settings = fs::read_to_string(home.join(".claude/settings.json")).unwrap();
    assert!(
        settings.contains("hook claude"),
        "hooks must stay: {settings}"
    );

    // Purge is for a full uninstall only.
    let out = lagom(&home, &["uninstall", "claude", "--purge", "--yes"], None);
    assert_eq!(out.status.code(), Some(2));
    let out = lagom(&home, &["uninstall", "--all", "claude"], None);
    assert_eq!(out.status.code(), Some(2));

    let s = text(&lagom(
        &home,
        &["uninstall", "--all", "--purge", "--dry-run"],
        None,
    ));
    assert!(s.contains("purge: would delete"), "{s}");
    assert!(home.join(".lagom").exists());

    let out = lagom(&home, &["uninstall", "--all", "--purge", "--yes"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = text(&out);
    assert!(s.contains("To remove the program itself: rm "), "{s}");
    assert_eq!(lagom_leftovers(&home), Vec::<PathBuf>::new());
    assert!(home.join(".claude/settings.json.lagom.bak").exists());
    let settings = fs::read_to_string(home.join(".claude/settings.json")).unwrap();
    assert!(settings.contains("opus"), "{settings}");
}

/// Run `lagom` with a custom `LAGOM_HOME`.
fn lagom_with_home(home: &Path, lagom_home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lagom"))
        .args(args)
        .env("HOME", home)
        .env("LAGOM_HOME", lagom_home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .current_dir(home)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn purge_deletes_only_folders_lagom_created() {
    // LAGOM_HOME pointing at a folder of the user's: refused, nothing deleted.
    let home = sandbox("purge-foreign");
    let docs = home.join("Documents");
    fs::create_dir_all(&docs).unwrap();
    fs::write(docs.join("thesis.txt"), "years of work").unwrap();
    fs::create_dir_all(home.join(".config/lagom")).unwrap();
    let out = lagom_with_home(&home, &docs, &["uninstall", "--all", "--purge", "--yes"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("refusing to delete"));
    assert!(docs.join("thesis.txt").exists());
    assert!(
        home.join(".config/lagom").exists(),
        "nothing is deleted after a refusal"
    );

    // A custom LAGOM_HOME that Lagom created carries its marker and can be purged.
    let home = sandbox("purge-custom");
    let state = home.join("state/lagom-data");
    assert!(
        lagom_with_home(&home, &state, &["run", "--", "echo", "hi"])
            .status
            .success()
    );
    assert!(state.join(".lagom-dir").is_file());
    let out = lagom_with_home(&home, &state, &["uninstall", "--all", "--purge", "--yes"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!state.exists());
}

#[test]
fn purge_refuses_unsafe_lagom_home() {
    let home = sandbox("purge-unsafe");
    let out = Command::new(env!("CARGO_BIN_EXE_lagom"))
        .args(["uninstall", "--purge", "--yes"])
        .env("HOME", &home)
        .env("LAGOM_HOME", &home)
        .env("XDG_CONFIG_HOME", home.join("cfg"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("refusing to delete"));
    assert!(home.exists());
}

#[test]
fn rewrite_can_be_turned_off_per_project_or_by_env() {
    let home = sandbox("norewrite");
    let repo = home.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let payload = json!({"tool_name": "Bash", "cwd": repo, "permission_mode": "bypassPermissions",
                         "tool_input": {"command": "git log"}})
    .to_string();
    let hook = |extra_env: Option<(&str, &str)>| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_lagom"));
        cmd.args(["hook", "claude", "pre-tool"])
            .env("HOME", &home)
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("LAGOM_REWRITE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        if let Some((k, v)) = extra_env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
        text(&child.wait_with_output().unwrap())
    };
    assert!(hook(None).contains("run -- git log"), "rewrites by default");
    assert_eq!(hook(Some(("LAGOM_REWRITE", "off"))), "");
    fs::create_dir_all(repo.join(".lagom")).unwrap();
    fs::write(repo.join(".lagom/config.json"), r#"{"rewrite": false}"#).unwrap();
    assert_eq!(hook(None), "", "project turns rewriting off");
    assert!(
        hook(Some(("LAGOM_REWRITE", "on"))).contains("run -- git log"),
        "env wins"
    );

    // The user turns it off everywhere; a project file can't turn it back on.
    fs::write(repo.join(".lagom/config.json"), r#"{"rewrite": true}"#).unwrap();
    assert!(
        hook(None).contains("run -- git log"),
        "project on, user unset"
    );
    fs::create_dir_all(home.join(".config/lagom")).unwrap();
    fs::write(
        home.join(".config/lagom/config.json"),
        r#"{"rewrite": false}"#,
    )
    .unwrap();
    assert_eq!(hook(None), "", "user off wins over project on");
    assert!(
        hook(Some(("LAGOM_REWRITE", "on"))).contains("run -- git log"),
        "env still wins"
    );
}

#[test]
fn stuck_hook_gives_up_at_the_deadline() {
    // The agent never closes the hook's input: reading it blocks forever. The real
    // binary must still answer (Cursor: `{}`) and exit after about 2 seconds.
    let home = sandbox("deadline");
    let mut child = Command::new(env!("CARGO_BIN_EXE_lagom"))
        .args(["hook", "cursor", "session-start"])
        .env("HOME", &home)
        .env_remove("CLAUDE_PROJECT_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let held_open = child.stdin.take();
    let start = std::time::Instant::now();
    // Poll instead of waiting, so a hook without a deadline fails here instead of hanging.
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > std::time::Duration::from_secs(8) {
            let _ = child.kill();
            panic!("the hook was still running after 8 s");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let took = start.elapsed();
    drop(held_open);
    let mut stdout = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take().unwrap(), &mut stdout).unwrap();
    assert!(status.success());
    assert_eq!(stdout.trim(), "{}");
    assert!(took >= std::time::Duration::from_millis(1500), "{took:?}");
}

#[cfg(unix)]
#[test]
#[ignore = "slow in debug builds (~60 s): cargo test --release -- --ignored"]
fn run_with_200_mb_of_output_stays_under_100_mb_of_memory() {
    // CMP-05's Done-when: past 64 MB the output streams to the recall store.
    let home = sandbox("bigrun");
    let script = "yes 'a long line of build output that repeats over and over, padded to about a hundred bytes ok' \
                  | head -c 200000000; echo; echo 'src/main.rs:7: error: the one failure'";
    let time = Path::new("/usr/bin/time");
    let mut cmd = if time.exists() {
        let mut c = Command::new(time);
        c.arg(if cfg!(target_os = "macos") {
            "-l"
        } else {
            "-v"
        })
        .arg(env!("CARGO_BIN_EXE_lagom"));
        c
    } else {
        Command::new(env!("CARGO_BIN_EXE_lagom"))
    };
    let out = cmd
        .args(["run", "--", "sh", "-c", script])
        .env("HOME", &home)
        .env_remove("LAGOM_HOME")
        .output()
        .unwrap();
    assert!(out.status.success());
    let shown = text(&out);
    assert!(shown.len() < 3_000_000, "{} bytes shown", shown.len());
    assert!(
        shown.contains("src/main.rs:7: error: the one failure"),
        "{shown}"
    );
    let id = shown
        .rsplit("lagom recall ")
        .next()
        .and_then(|r| r.split(|c: char| c.is_whitespace() || c == ']').next())
        .unwrap()
        .to_string();
    let last = text(&lagom(&home, &["recall", &id, "--tail", "1"], None));
    assert_eq!(last.trim(), "src/main.rs:7: error: the one failure");

    // Peak memory, when /usr/bin/time reports it (bytes on macOS, KB on Linux).
    let report = String::from_utf8_lossy(&out.stderr);
    let peak = report.lines().find_map(|l| {
        let l = l.trim();
        if let Some(v) = l.strip_suffix("maximum resident set size") {
            v.trim().parse::<u64>().ok()
        } else {
            l.strip_prefix("Maximum resident set size (kbytes):")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(|kb| kb * 1024)
        }
    });
    if let Some(peak) = peak {
        eprintln!("peak memory: {} MB", peak / (1024 * 1024));
        assert!(
            peak < 100 * 1024 * 1024,
            "peak memory {} MB",
            peak / (1024 * 1024)
        );
    }
}

#[test]
fn exit_zero_with_error_lines_says_so_in_the_footer() {
    let home = sandbox("exit0errors");
    let script = "i=0; while [ $i -lt 400 ]; do echo \"step $i done\"; i=$((i+1)); done; \
                  echo 'src/a.py:3: error: bad import'; exit 0";
    let out = lagom(&home, &["run", "--", "sh", "-c", script], None);
    assert!(out.status.success());
    let s = text(&out);
    assert!(s.contains("src/a.py:3: error: bad import"), "{s}");
    assert!(
        s.contains("exit code 0 but error lines found, so more were kept;"),
        "{s}"
    );
}

#[test]
fn bench_compress_reports_and_gates_on_lost_strings() {
    let home = sandbox("bench");
    let case = |name: &str, input: &str, meta: Value| {
        let d = home.join("corpus").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("input.txt"), input).unwrap();
        fs::write(d.join("meta.json"), meta.to_string()).unwrap();
    };
    let long: String = (0..400).map(|i| format!("line {i} plain\n")).collect();
    case(
        "ok",
        "src/a.rs:1: error: boom\n",
        json!({"exit_code": 1, "must_keep": ["error: boom"]}),
    );
    case(
        "gap",
        &long,
        json!({"exit_code": 0, "must_keep": ["line 200 plain"], "known_gap": "CMP-99"}),
    );
    let dir = home.join("corpus").to_string_lossy().into_owned();
    let json_path = home.join("out.json").to_string_lossy().into_owned();

    let out = lagom(
        &home,
        &["bench", "compress", &dir, "--json", &json_path],
        None,
    );
    assert_eq!(out.status.code(), Some(0), "known gaps don't fail");
    let report = text(&out);
    assert!(
        report.contains("(known gap: CMP-99)") && report.contains("Unexpected must_keep losses: 0"),
        "{report}"
    );
    let saved: Value = serde_json::from_str(&fs::read_to_string(&json_path).unwrap()).unwrap();
    assert_eq!(saved["totals"]["cases"], 2);

    case(
        "lost",
        &long,
        json!({"exit_code": 0, "must_keep": ["line 201 plain"]}),
    );
    let out = lagom(
        &home,
        &["bench", "compress", &dir, "--compare", &json_path],
        None,
    );
    assert_eq!(out.status.code(), Some(1), "an unexpected loss fails");
    assert!(text(&out).contains("**LOST**") && text(&out).contains("new |"));

    // Size gate: a baseline whose output was half as big means +100% growth.
    fs::remove_dir_all(home.join("corpus/lost")).unwrap();
    let mut small: Value = serde_json::from_str(&fs::read_to_string(&json_path).unwrap()).unwrap();
    let total = small["totals"]["bytes_out"].as_u64().unwrap();
    small["totals"]["bytes_out"] = json!(total / 2);
    let small_path = home.join("small.json").to_string_lossy().into_owned();
    fs::write(&small_path, small.to_string()).unwrap();
    let grow = |limit: &str, base: &str| {
        lagom(
            &home,
            &[
                "bench",
                "compress",
                &dir,
                "--compare",
                base,
                "--max-growth",
                limit,
            ],
            None,
        )
    };
    let out = grow("5", &small_path);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("grew 100.0%"), "{}", text(&out));
    assert_eq!(
        grow("5", &json_path).status.code(),
        Some(0),
        "no growth passes"
    );
    for bad in ["x", "nan", "-1"] {
        assert_eq!(grow(bad, &json_path).status.code(), Some(2), "{bad}");
    }
    let no_base = lagom(
        &home,
        &["bench", "compress", &dir, "--max-growth", "5"],
        None,
    );
    assert_eq!(no_base.status.code(), Some(2));

    // A baseline that isn't a bench report, or a flag without its file, is an error.
    let junk = home.join("junk.json");
    fs::write(&junk, "{\"hello\": 1}").unwrap();
    let junk = junk.to_string_lossy().into_owned();
    let out = lagom(
        &home,
        &["bench", "compress", &dir, "--compare", &junk],
        None,
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not a `lagom bench compress --json` report")
    );
    let out = lagom(&home, &["bench", "compress", &dir, "--json"], None);
    assert_eq!(out.status.code(), Some(2));

    // A known gap whose strings are all kept now says the marker can go.
    let d = home.join("corpus").join("gap");
    fs::write(
        d.join("meta.json"),
        json!({"exit_code": 0, "must_keep": ["line 0 plain"], "known_gap": "CMP-99"}).to_string(),
    )
    .unwrap();
    let out = lagom(&home, &["bench", "compress", &dir], None);
    assert!(
        text(&out).contains("known gap CMP-99 now met: remove the marker"),
        "{}",
        text(&out)
    );
}

#[test]
fn stats_are_attributed_to_agent_session_style_and_project() {
    let home = sandbox("attrib");
    let repo = home.join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    let start =
        json!({"session_id": "sess-1", "cwd": repo, "hook_event_name": "SessionStart"}).to_string();
    lagom(&home, &["hook", "claude", "session-start"], Some(&start));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lagom"));
    let out = cmd
        .args([
            "run",
            "--session",
            "claude:sess-1",
            "--",
            "sh",
            "-c",
            "seq 1 500",
        ])
        .env("HOME", &home)
        .env_remove("LAGOM_HOME")
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(out.status.success());
    lagom(&home, &["run", "--", "echo", "manual"], None);

    let events: Vec<Value> = fs::read_to_string(home.join(".lagom/stats.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let inject = events.iter().find(|e| e["kind"] == "inject").unwrap();
    assert_eq!(inject["session"], "sess-1");
    assert_eq!(inject["styles"], json!(["terse", "lean-code"]));
    let run = events
        .iter()
        .find(|e| e["kind"] == "run" && e["agent"] == "claude")
        .unwrap();
    assert_eq!(run["session"], "sess-1");
    assert_eq!(run["v"], 1);
    assert_eq!(
        run["project"], inject["project"],
        "same repository, same project id"
    );
    assert!(
        !run["project"].as_str().unwrap().contains('/'),
        "a hash, not a path"
    );

    // A run an older Lagom recorded (no version, no agent) is not "manual".
    let mut log = fs::read_to_string(home.join(".lagom/stats.jsonl")).unwrap();
    log.push_str("{\"kind\":\"run\",\"cmd\":\"git\",\"in\":100,\"out\":10,\"exit\":0,\"ts\":1}\n");
    fs::write(home.join(".lagom/stats.jsonl"), log).unwrap();

    let stats = text(&lagom(&home, &["stats", "--all-time"], None));
    assert!(
        stats.contains("by agent:") && stats.contains("claude") && stats.contains("manual"),
        "{stats}"
    );
    let line = |name: &str| stats.lines().find(|l| l.trim_start().starts_with(name));
    assert!(
        line("manual").is_some_and(|l| l.contains(" 1 runs")),
        "{stats}"
    );
    assert!(
        line("unknown").is_some_and(|l| l.contains(" 1 runs")),
        "{stats}"
    );
    assert!(
        stats.contains("by style:") && stats.contains("terse+lean-code"),
        "{stats}"
    );
}

#[test]
fn holdout_experiment_skips_styles_for_some_sessions() {
    let home = sandbox("experiment");
    assert_eq!(
        lagom(&home, &["experiment", "start", "--holdout", "0.9"], None)
            .status
            .code(),
        Some(2),
        "max 0.5"
    );
    assert!(
        lagom(
            &home,
            &["experiment", "start", "--holdout", "0.5", "--days", "3"],
            None
        )
        .status
        .success()
    );
    assert!(text(&lagom(&home, &["experiment", "status"], None)).contains("Running: 50% holdout"));

    let mut injected = 0;
    for i in 0..40 {
        let payload = json!({"session_id": format!("sess-{i}"), "cwd": home}).to_string();
        let out = text(&lagom(
            &home,
            &["hook", "claude", "session-start"],
            Some(&payload),
        ));
        // Holdout sessions lose the styles but keep the note on the recall footer.
        assert!(out.contains("lagom recall <id>"), "{out}");
        if out.contains("(lagom: terse)") {
            injected += 1;
        }
    }
    assert!(
        (8..=32).contains(&injected),
        "about half get styles: {injected}"
    );
    let stats = fs::read_to_string(home.join(".lagom/stats.jsonl")).unwrap();
    assert_eq!(stats.matches("\"arm\":\"holdout\"").count(), 40 - injected);
    assert_eq!(stats.matches("\"arm\":\"styled\"").count(), injected);
    assert!(text(&lagom(&home, &["stats", "--experiment"], None)).contains("styled sessions"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(home.join(".lagom/experiment.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "experiment.json is private");
    }
    // Cursor sessions are never held out: their usage can't be measured.
    for i in 0..40 {
        let payload = json!({"conversation_id": format!("sess-{i}"), "cwd": home}).to_string();
        let out = text(&lagom(
            &home,
            &["hook", "cursor", "session-start"],
            Some(&payload),
        ));
        assert!(out.contains("(lagom: terse)"), "cursor session {i}: {out}");
    }

    assert!(lagom(&home, &["experiment", "stop"], None).status.success());
    // Stopping keeps the record: the results stay readable.
    assert!(text(&lagom(&home, &["experiment", "status"], None)).contains("Finished"));
    assert!(text(&lagom(&home, &["stats", "--experiment"], None)).contains("styled sessions"));
    let payload = json!({"session_id": "sess-0", "cwd": home}).to_string();
    assert!(
        !text(&lagom(
            &home,
            &["hook", "claude", "session-start"],
            Some(&payload)
        ))
        .is_empty(),
        "styles again"
    );
}

#[test]
fn session_start_adds_the_projects_test_policy() {
    let home = sandbox("testpolicy");
    let repo = home.join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::create_dir_all(repo.join("tests")).unwrap();
    fs::write(repo.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let payload = json!({"cwd": repo}).to_string();
    let out = text(&lagom(
        &home,
        &["hook", "claude", "session-start"],
        Some(&payload),
    ));
    assert!(out.contains("Rust project with tests (tests/)"), "{out}");

    // A project without tests gets no line, never "don't add tests" (D-36).
    let bare = home.join("bare");
    fs::create_dir_all(bare.join(".git")).unwrap();
    fs::write(bare.join("Cargo.toml"), "[package]\nname = \"y\"\n").unwrap();
    let out = text(&lagom(
        &home,
        &["hook", "claude", "session-start"],
        Some(&json!({"cwd": bare}).to_string()),
    ));
    assert!(
        out.contains("(lagom: lean-code)") && !out.contains("Rust project"),
        "{out}"
    );

    // Only with the code style active.
    let out = Command::new(env!("CARGO_BIN_EXE_lagom"))
        .args(["hook", "claude", "session-start"])
        .env("HOME", &home)
        .env("LAGOM_STYLES", "terse")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            c.stdin.take().unwrap().write_all(payload.as_bytes())?;
            c.wait_with_output()
        })
        .unwrap();
    assert!(!text(&out).contains("Rust project"));
}

#[test]
fn stats_usage_groups_real_usage_by_style() {
    let home = sandbox("usage");
    let proj = home.join(".claude/projects/p");
    fs::create_dir_all(&proj).unwrap();
    // One assistant turn per (session, time, output); input is all cache reads.
    let turn = |s: &str, ts: &str, id: &str, out: u64| {
        format!(
            "{{\"type\":\"assistant\",\"sessionId\":\"{s}\",\"timestamp\":\"{ts}\",\"message\":{{\"id\":\"{id}\",\"model\":\"claude-opus-5-5\",\"usage\":{{\"input_tokens\":0,\"output_tokens\":{out},\"cache_read_input_tokens\":1000}}}}}}\n"
        )
    };
    let t = "2026-11-08T10:00:00Z";
    fs::write(
        proj.join("a.jsonl"),
        turn("a", t, "a1", 100) + &turn("a", t, "a2", 300),
    )
    .unwrap();
    fs::write(proj.join("b.jsonl"), turn("b", t, "b1", 50)).unwrap();
    fs::write(proj.join("c.jsonl"), turn("c", t, "c1", 700)).unwrap();
    fs::write(proj.join("e.jsonl"), turn("e", t, "e1", 800)).unwrap();
    fs::write(
        proj.join("d.jsonl"),
        turn("d", "2026-11-01T00:00:00Z", "d1", 9),
    )
    .unwrap();
    // Lagom started tracking sessions on 2026-11-08 08:00 UTC (1794124800).
    fs::create_dir_all(home.join(".lagom")).unwrap();
    fs::write(
        home.join(".lagom/stats.jsonl"),
        "{\"v\":1,\"ts\":1794124800,\"kind\":\"inject\",\"agent\":\"claude\",\"session\":\"a\",\"styles\":[\"terse\"]}\n\
         {\"v\":1,\"ts\":1794124900,\"kind\":\"inject\",\"agent\":\"claude\",\"session\":\"b\",\"styles\":[\"terse\",\"lean-code\"]}\n\
         {\"v\":1,\"ts\":1794124950,\"kind\":\"holdout\",\"arm\":\"holdout\",\"agent\":\"claude\",\"session\":\"e\"}\n",
    )
    .unwrap();

    let out = lagom(&home, &["stats", "--usage", "--days", "36500"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = text(&out);
    let row = |style: &str| -> Vec<String> {
        let line = s
            .lines()
            .find(|l| l.starts_with(style))
            .unwrap_or_else(|| panic!("{style}: {s}"));
        line[style.len()..]
            .split_whitespace()
            .map(String::from)
            .collect()
    };
    // sessions, turns, median output/turn, median input/turn, cache read share
    assert_eq!(row("terse+lean-code"), ["1", "1", "50", "1000", "100%"]);
    assert_eq!(row("terse "), ["1", "2", "200", "1000", "100%"]);
    assert_eq!(row("no styles injected"), ["1", "1", "700", "1000", "100%"]);
    assert_eq!(
        row("holdout (experiment)"),
        ["1", "1", "800", "1000", "100%"]
    );
    assert_eq!(
        row("before Lagom tracked sessions"),
        ["1", "1", "9", "1000", "100%"]
    );

    assert_eq!(
        lagom(&home, &["stats", "--bogus"], None).status.code(),
        Some(2)
    );
    assert_eq!(
        lagom(&home, &["stats", "--usage", "--days", "x"], None)
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn experiment_start_names_other_tools_that_add_text_at_session_start() {
    let home = sandbox("experiment-other-hooks");
    let write = |rel: &str, v: serde_json::Value| {
        let p = home.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, v.to_string()).unwrap();
    };
    let plugin = |name: &str| {
        home.join(".claude/plugins/cache/mkt")
            .join(name)
            .join("1.0")
            .display()
            .to_string()
    };
    write(
        ".claude/settings.json",
        json!({
            "hooks": {
                "SessionStart": [
                    {"hooks": [{"type": "command", "command": "'/opt/caveman/bin/caveman-proxy' native-hook claude"}]},
                    {"matcher": "startup", "hooks": [{"type": "command", "command": "/usr/local/bin/lagom hook claude session-start"}]}
                ],
                "PreToolUse": [{"hooks": [{"type": "command", "command": "guard-tool check"}]}]
            },
            "enabledPlugins": {"style-plugin@mkt": true, "quiet@mkt": true, "off@mkt": false}
        }),
    );
    write(
        ".claude/plugins/installed_plugins.json",
        json!({"version": 2, "plugins": {
            "style-plugin@mkt": [{"scope": "user", "installPath": plugin("style-plugin")}],
            "quiet@mkt": [{"scope": "user", "installPath": plugin("quiet")}],
            "off@mkt": [{"scope": "user", "installPath": plugin("off")}]
        }}),
    );
    let hooks = |events: &[&str]| {
        let mut h = serde_json::Map::new();
        for e in events {
            h.insert(
                e.to_string(),
                json!([{"hooks": [{"type": "command", "command": "x"}]}]),
            );
        }
        json!({"hooks": h})
    };
    write(
        ".claude/plugins/cache/mkt/style-plugin/1.0/hooks/hooks.json",
        hooks(&["SessionStart"]),
    );
    write(
        ".claude/plugins/cache/mkt/quiet/1.0/hooks/hooks.json",
        hooks(&["PreToolUse"]),
    );
    write(
        ".claude/plugins/cache/mkt/off/1.0/hooks/hooks.json",
        hooks(&["SessionStart"]),
    );
    write(
        ".codex/hooks.json",
        json!({"hooks": {"UserPromptSubmit": [{"hooks": [{"type": "command", "command": "prompt-tool add"}]}]}}),
    );
    write(
        ".cursor/hooks.json",
        json!({"version": 1, "hooks": {"sessionStart": [{"command": "cursor-tool start"}]}}),
    );

    let out = lagom(&home, &["experiment", "start", "--days", "3"], None);
    assert!(out.status.success());
    let said = text(&out);
    for expected in [
        "Claude Code SessionStart: '/opt/caveman/bin/caveman-proxy' native-hook claude",
        "Claude Code plugin style-plugin@mkt: SessionStart",
        "Codex UserPromptSubmit: prompt-tool add",
        "Cursor sessionStart: cursor-tool start",
    ] {
        assert!(said.contains(expected), "missing {expected:?} in:\n{said}");
    }
    for unexpected in ["lagom hook", "guard-tool", "quiet@mkt", "off@mkt"] {
        assert!(
            !said.contains(unexpected),
            "{unexpected:?} listed in:\n{said}"
        );
    }

    // With no other hook, the start says nothing about other tools' hooks.
    let clean = sandbox("experiment-no-other-hooks");
    let said = text(&lagom(
        &clean,
        &["experiment", "start", "--days", "3"],
        None,
    ));
    assert!(!said.contains("Claude Code"), "{said}");
}

#[test]
fn stats_file_is_rotated_at_its_size_cap_and_both_files_are_read() {
    let home = sandbox("stats-rotation");
    let data = home.join(".lagom");
    fs::create_dir_all(&data).unwrap();
    let line = "{\"v\":1,\"ts\":1,\"kind\":\"run\",\"in\":100,\"out\":10}\n";
    let n = 16 * 1024 * 1024 / line.len() + 1; // just over the 16 MiB cap
    fs::write(data.join("stats.jsonl"), line.repeat(n)).unwrap();

    assert!(lagom(&home, &["run", "--", "true"], None).status.success());

    let current = fs::metadata(data.join("stats.jsonl")).unwrap().len();
    let rotated = fs::metadata(data.join("stats.1.jsonl")).unwrap().len();
    assert!(
        current < 4096,
        "the new event starts a fresh file: {current} bytes"
    );
    assert!(rotated >= 16 * 1024 * 1024, "{rotated}");
    let said = text(&lagom(&home, &["stats", "--all-time"], None));
    assert!(
        said.contains(&format!("commands run through lagom: {}", n + 1)),
        "both files are counted:\n{said}"
    );
}

/// Starts lagom with stdout piped, reads a few bytes, then closes the pipe, as `| head` does.
#[cfg(unix)]
fn close_pipe_early(home: &Path, args: &[&str]) -> (std::process::ExitStatus, String) {
    use std::io::Read;
    let mut child = Command::new(env!("CARGO_BIN_EXE_lagom"))
        .args(args)
        .env("HOME", home)
        .env_remove("LAGOM_HOME")
        .env_remove("LAGOM_DISABLE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = [0u8; 16];
    child.stdout.take().unwrap().read_exact(&mut first).unwrap(); // dropped here: pipe closed
    let out = child.wait_with_output().unwrap();
    (
        out.status,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[cfg(unix)]
#[test]
fn a_closed_pipe_ends_lagom_quietly_and_run_keeps_the_exit_code() {
    let home = sandbox("closed-pipe");
    // A large saved output, so recall writes far more than the pipe holds.
    let out = lagom(&home, &["run", "--", "sh", "-c", "seq 1 300000"], None);
    let said = text(&out);
    let id = said
        .rsplit("lagom recall ")
        .next()
        .and_then(|s| s.split(|c: char| !c.is_ascii_hexdigit()).next())
        .unwrap()
        .to_string();
    assert!(id.len() >= 8, "no recall id in: {said}");

    let (status, stderr) = close_pipe_early(&home, &["recall", &id]);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert_ne!(status.code(), Some(101), "{status:?}");

    // `lagom run` must still return the command's own exit code.
    let (status, stderr) =
        close_pipe_early(&home, &["run", "--", "sh", "-c", "seq 1 300000; exit 3"]);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert_eq!(status.code(), Some(3));
}

#[test]
fn a_malformed_config_file_is_reported_and_never_overwritten() {
    let home = sandbox("malformed-config");
    let file = home.join(".config/lagom/config.json");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let broken = "{\"styles\": [\"terse\",]  // a typo\n";
    fs::write(&file, broken).unwrap();

    let out = lagom(&home, &["style", "use", "lean-code"], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not valid JSON"));
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        broken,
        "the file survives"
    );

    let status = text(&lagom(&home, &["status"], None));
    assert!(
        status.contains("config: ") && status.contains("not valid JSON"),
        "{status}"
    );
    // Lagom ignores the bad file and keeps working with the defaults.
    assert!(
        status.contains("style terse") && status.contains("style lean-code"),
        "{status}"
    );
}

#[test]
fn every_command_rejects_an_unknown_flag_with_its_usage() {
    let home = sandbox("unknown-flags");
    // `hook` never fails (it must not break the agent), and `run` passes everything after
    // `--` to the command, so neither is here.
    let cases: &[&[&str]] = &[
        &["install", "--bogus"],
        &["uninstall", "--bogus"],
        &["status", "--bogus"],
        &["style", "list", "--bogus"],
        &["style", "show", "terse", "--bogus"],
        &["style", "use", "--bogus", "terse"],
        &["compress", "--bogus"],
        &["recall", "--list", "--bogus"],
        &["recall", "--clear", "--bogus"],
        &["recall", "0123456789", "--bogus"],
        &["stats", "--bogus"],
        &["stats", "--usage", "--bogus"],
        &["experiment", "start", "--bogus"],
        &["experiment", "status", "--bogus"],
        &["experiment", "stop", "--bogus"],
        &["discover", "--bogus"],
        &["permissions", "suggest", "--bogus"],
        &["bench", "compress", "--bogus"],
    ];
    for args in cases {
        let out = lagom(&home, args, Some(""));
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {err}");
        assert!(
            err.contains("--bogus") && err.contains("usage"),
            "{args:?}: {err}"
        );
    }
    // Flags that only make sense together are refused too, instead of one being dropped.
    for args in [
        &["stats", "--experiment", "--days", "3"][..],
        &["stats", "--usage", "--json"],
    ] {
        assert_eq!(lagom(&home, args, None).status.code(), Some(2), "{args:?}");
    }
    // A refused `uninstall` or `recall --clear` changed nothing.
    assert!(
        !home.join(".lagom").exists() || fs::read_dir(home.join(".lagom")).unwrap().count() == 0
    );
}

#[test]
fn lagom_debug_explains_a_silent_hook_without_logging_the_payload() {
    let home = sandbox("debug");
    let hook = |stdin: &str, debug: bool| -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_lagom"));
        cmd.args(["hook", "claude", "pre-tool"])
            .env("HOME", &home)
            .env_remove("LAGOM_HOME")
            .env_remove("LAGOM_DEBUG")
            .env_remove("LAGOM_DISABLE")
            .env_remove("LAGOM_REWRITE")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CLAUDE_PROJECT_DIR")
            .current_dir(&home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if debug {
            cmd.env("LAGOM_DEBUG", "1");
        }
        let mut child = cmd.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let log = home.join(".lagom/debug.log");

    // Off by default: nothing on stderr, no log.
    let quiet = hook("secret-token-123 not json", false);
    assert!(quiet.status.success() && quiet.stdout.is_empty() && quiet.stderr.is_empty());
    assert!(!log.exists());

    let out = hook("secret-token-123 not json", true);
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "stdout stays silent");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("hook claude pre-tool: payload ignored: not JSON (25 bytes)"),
        "{err}"
    );

    // A decision line for a command that is left alone, without the command's text.
    let payload =
        json!({"tool_input": {"command": "git log --author=secret-name | head"}, "cwd": home});
    let out = hook(&payload.to_string(), true);
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("pre-tool claude: left alone: pipes"), "{err}");

    let logged = fs::read_to_string(&log).unwrap();
    assert!(logged.contains("payload ignored: not JSON") && logged.contains("left alone: pipes"));
    for secret in ["secret-token-123", "secret-name", "git log"] {
        assert!(
            !logged.contains(secret) && !err.contains(secret),
            "{secret} leaked"
        );
    }
}

#[test]
fn rewrite_extra_and_exclude_change_which_programs_are_wrapped() {
    let home = sandbox("wrapped-programs");
    let repo = home.join("repo");
    fs::create_dir_all(repo.join(".lagom")).unwrap();
    let hook = |command: &str| {
        let payload = json!({"tool_name": "Bash", "cwd": repo, "permission_mode": "bypassPermissions",
                             "tool_input": {"command": command}});
        let mut child = Command::new(env!("CARGO_BIN_EXE_lagom"))
            .args(["hook", "claude", "pre-tool"])
            .env("HOME", &home)
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("LAGOM_REWRITE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        text(&child.wait_with_output().unwrap()).contains("run -- ")
    };
    assert!(!hook("just build") && hook("docker ps") && hook("git log"));

    let user = home.join(".config/lagom/config.json");
    fs::create_dir_all(user.parent().unwrap()).unwrap();
    fs::write(
        &user,
        r#"{"rewrite": {"extra": ["just"], "exclude": ["docker"]}}"#,
    )
    .unwrap();
    assert!(hook("just build"), "added by the user");
    assert!(!hook("docker ps"), "excluded by the user");

    // A project file may exclude, but not add.
    fs::write(
        repo.join(".lagom/config.json"),
        r#"{"rewrite": {"extra": ["make2"], "exclude": ["git"]}}"#,
    )
    .unwrap();
    assert!(!hook("git log"), "excluded by the project");
    assert!(!hook("make2 all"), "a project can't add programs");
}

#[test]
fn lossless_mode_from_the_config_keeps_every_line() {
    let home = sandbox("lossless");
    let input: String = (1..=500).map(|i| format!("{i}\n")).collect();
    let folded = text(&lagom(&home, &["compress"], Some(&input)));
    assert!(
        folded.lines().count() < 20,
        "similar lines are folded by default"
    );

    let config = home.join(".config/lagom/config.json");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, r#"{"compress": {"lossless": true}}"#).unwrap();
    let kept = text(&lagom(&home, &["compress"], Some(&input)));
    assert_eq!(kept, input, "every line, no footer");
}

/// `lagom stats` and `lagom stats --json` on fixture events (GAIN-06). `LAGOM_BLESS=1`
/// rewrites the expected files after an intended change; then review the diff.
#[test]
fn stats_report_matches_its_golden_files() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stats");
    let home = sandbox("stats-golden");
    fs::create_dir_all(home.join(".lagom")).unwrap();
    fs::copy(
        fixtures.join("stats.jsonl"),
        home.join(".lagom/stats.jsonl"),
    )
    .unwrap();
    for (args, golden) in [
        (&["stats", "--all-time"][..], "report.txt"),
        (&["stats", "--all-time", "--json"][..], "report.json"),
    ] {
        let out = lagom(&home, args, None);
        assert!(out.status.success(), "{args:?}");
        let got = String::from_utf8(out.stdout).unwrap();
        let path = fixtures.join(golden);
        if std::env::var_os("LAGOM_BLESS").is_some() {
            fs::write(&path, &got).unwrap();
        }
        assert_eq!(
            got,
            fs::read_to_string(&path).unwrap(),
            "{golden} differs (LAGOM_BLESS=1 to update)"
        );
    }
}

#[test]
fn stats_filters_select_events_and_sessions() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stats");
    let home = sandbox("stats-filters");
    fs::create_dir_all(home.join(".lagom")).unwrap();
    fs::copy(
        fixtures.join("stats.jsonl"),
        home.join(".lagom/stats.jsonl"),
    )
    .unwrap();
    // Session a1's transcript names its model, which `--model` joins to a1's events.
    let transcript = home.join(".claude/projects/p/a1.jsonl");
    fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    fs::write(
        &transcript,
        concat!(
            r#"{"type":"user","sessionId":"a1","timestamp":"2026-11-22T10:00:00Z","message":{"role":"user","content":"hi"}}"#, "\n",
            r#"{"type":"assistant","sessionId":"a1","timestamp":"2026-11-22T10:00:01Z","isSidechain":false,"message":{"id":"m1","model":"claude-test-model","role":"assistant","usage":{"input_tokens":10,"output_tokens":50}}}"#, "\n",
        ),
    )
    .unwrap();
    let report = |args: &[&str]| -> Value {
        let mut all = vec!["stats", "--json"];
        all.extend_from_slice(args);
        let out = lagom(&home, &all, None);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let counts = |v: &Value| {
        let t = &v["compression"]["total"];
        (
            t["runs"].as_u64().unwrap(),
            t["injections"].as_u64().unwrap(),
        )
    };

    assert_eq!(counts(&report(&["--all-time"])), (5, 4));
    assert_eq!(
        counts(&report(&["--since", "2026-11-22"])),
        (4, 4),
        "the 11-21 run is out"
    );
    assert_eq!(counts(&report(&["--all-time", "--agent", "codex"])), (1, 1));
    assert_eq!(
        counts(&report(&["--all-time", "--style", "terse+lean-code"])),
        (2, 2)
    );
    assert_eq!(counts(&report(&["--all-time", "--project", "p1"])), (2, 0));
    assert_eq!(counts(&report(&["--all-time", "--session", "b1"])), (1, 1));
    let by_model = report(&["--all-time", "--model", "claude-test-model"]);
    assert_eq!(counts(&by_model), (2, 1), "session a1's runs and injection");
    assert_eq!(
        by_model["output"]["by_agent"]["Claude Code"][0]["median_output_per_turn"],
        50
    );
    assert!(
        by_model["scope"]
            .as_str()
            .unwrap()
            .contains("model=claude-test-model")
    );

    // An unknown value lists the known ones.
    let out = lagom(&home, &["stats", "--all-time", "--agent", "nope"], None);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("known: claude, codex, cursor, manual, unknown"),
        "{err}"
    );
    // Only one window at a time.
    assert_eq!(
        lagom(&home, &["stats", "--days", "3", "--all-time"], None)
            .status
            .code(),
        Some(2)
    );

    // `--days` counts back from now, by each event's own time; 30 is the default.
    let recent = sandbox("stats-days");
    fs::create_dir_all(recent.join(".lagom")).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let event = |days_ago: u64| json!({"v": 1, "ts": now - days_ago * 86_400, "kind": "run", "cmd": "git", "in": 400, "out": 40});
    fs::write(
        recent.join(".lagom/stats.jsonl"),
        format!("{}\n{}\n", event(10), event(40)),
    )
    .unwrap();
    let runs = |args: &[&str]| {
        let mut all = vec!["stats", "--json"];
        all.extend_from_slice(args);
        let v: Value = serde_json::from_slice(&lagom(&recent, &all, None).stdout).unwrap();
        v["compression"]["total"]["runs"].as_u64().unwrap()
    };
    assert_eq!(runs(&[]), 1, "default: the last 30 days");
    assert_eq!(runs(&["--days", "60"]), 2);
    assert_eq!(runs(&["--days", "5"]), 0);
    assert_eq!(runs(&["--all-time"]), 2);
}

#[test]
fn permissions_suggest_prints_rules_and_writes_nothing() {
    let home = sandbox("permissions-suggest");
    let log = home.join(".claude/projects/p/s.jsonl");
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/transcripts/claude-permissions.jsonl"),
        &log,
    )
    .unwrap();
    let settings = home.join(".claude/settings.json");
    let before = r#"{"permissions": {"allow": ["Bash(rg:*)"]}}"#;
    fs::write(&settings, before).unwrap();

    let said = text(&lagom(&home, &["permissions", "suggest"], None));
    for expected in [
        "\"Bash(git log:*)\"",
        "\"Bash(git status:*)\"",
        "\"Bash(tree:*)\"",
        "\"Shell(tree)\"",
        "Shell(git) would also allow git push",
    ] {
        assert!(said.contains(expected), "missing {expected} in:\n{said}");
    }
    for unexpected in [
        "Bash(rg",
        "Shell(rg",
        "Bash(find",
        "Bash(git push",
        "Bash(ls",
    ] {
        assert!(
            !said.contains(unexpected),
            "{unexpected} suggested:\n{said}"
        );
    }
    assert_eq!(
        fs::read_to_string(&settings).unwrap(),
        before,
        "never writes a rule"
    );
}

/// Writes `v` as JSON at `home/rel`, creating folders.
fn write_json(home: &Path, rel: &str, v: Value) {
    let p = home.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, v.to_string()).unwrap();
}

fn doctor(home: &Path) -> (String, Option<i32>) {
    let out = lagom(home, &["doctor"], None);
    (text(&out), out.status.code())
}

#[cfg(unix)]
#[test]
fn doctor_checks_the_binary_in_every_hook() {
    use std::os::unix::fs::PermissionsExt;
    let home = sandbox("doctor-hooks");
    let old = home.join("old/lagom");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    fs::write(&old, "#!/bin/sh\necho lagom 0.0.9\n").unwrap();
    fs::set_permissions(&old, fs::Permissions::from_mode(0o755)).unwrap();
    let hook = |bin: &str, event: &str| json!([{"hooks": [{"type": "command", "command": format!("'{bin}' hook claude {event}")}]}]);
    let this = env!("CARGO_BIN_EXE_lagom");
    let gone = home.join("gone/lagom").display().to_string();
    write_json(
        &home,
        ".claude/settings.json",
        json!({"hooks": {
            "SessionStart": hook(this, "session-start"),
            "SubagentStart": hook(&old.display().to_string(), "subagent-start"),
            "PreToolUse": hook(&gone, "pre-tool"),
        }}),
    );
    let (said, code) = doctor(&home);
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        said.contains(&format!("ok    claude hooks: {this} (lagom {version})")),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "warn  claude hooks: {} is lagom 0.0.9, this is lagom {version}",
            old.display()
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!("fail  claude hooks: {gone} doesn't exist")),
        "{said}"
    );
    assert!(said.contains("fix: lagom install claude"), "{said}");
    assert_eq!(code, Some(1), "a failed check exits 1");

    fs::write(home.join(".claude/settings.json"), "{not json").unwrap();
    let (said, _) = doctor(&home);
    assert!(said.contains("fail  claude hooks:"), "{said}");
}

#[test]
fn doctor_reports_codex_hooks_without_a_trust_record() {
    let home = sandbox("doctor-codex");
    fs::create_dir_all(home.join(".codex")).unwrap();
    assert!(
        lagom(&home, &["install", "codex", "--yes"], None)
            .status
            .success()
    );
    let (said, code) = doctor(&home);
    assert!(said.contains("ok    codex hooks:"), "{said}");
    assert!(
        said.contains("warn  codex trust: Codex skips Lagom's SessionStart and PreToolUse hooks until you trust them"),
        "{said}"
    );
    assert!(said.contains("fix: open Codex, run /hooks"), "{said}");
    assert_eq!(code, Some(0), "a warning isn't a failure");

    let file = home.join(".codex/hooks.json").display().to_string();
    let record = |event: &str| {
        format!("[hooks.state.\"{file}:{event}:0:0\"]\ntrusted_hash = \"sha256:00\"\n")
    };
    fs::write(
        home.join(".codex/config.toml"),
        record("session_start") + &record("pre_tool_use"),
    )
    .unwrap();
    let (said, _) = doctor(&home);
    assert!(said.contains("ok    codex trust:"), "{said}");
}

#[test]
fn doctor_names_other_tools_that_add_rules_or_rewrite_commands() {
    let home = sandbox("doctor-others");
    let hook = |cmd: &str| json!({"hooks": [{"type": "command", "command": cmd}]});
    write_json(
        &home,
        ".claude/settings.json",
        json!({"hooks": {
            "SessionStart": [hook("'/opt/caveman/bin/caveman-proxy' native-hook claude"), hook("/usr/local/bin/notes-tool start")],
            "UserPromptSubmit": [hook("'/opt/caveman/bin/caveman-proxy' native-hook claude")],
            "PreToolUse": [
                {"matcher": "Bash", "hooks": [{"type": "command", "command": "rtk hook claude"}]},
                {"matcher": "Bash", "hooks": [{"type": "command", "command": "guard-tool check"}]},
                {"matcher": "Read", "hooks": [{"type": "command", "command": "graphify hook-guard"}]},
                {"matcher": "Edit", "hooks": [{"type": "command", "command": "formatter run"}]}
            ]
        }}),
    );
    fs::write(
        home.join("CLAUDE.md"),
        "# Project\n\n## graphify\nUse the graph.\n",
    )
    .unwrap();
    let (said, _) = doctor(&home);
    for expected in [
        "warn  stacked styles: caveman-proxy (Claude Code hook, SessionStart, UserPromptSubmit) adds its own answer rules",
        "fix: keep one: remove its hooks, or run `lagom style use off`",
        "info  session-start text: notes-tool (Claude Code hook, SessionStart)",
        "warn  second rewriter: rtk (Claude Code hook, PreToolUse) rewrites shell commands too",
        "info  shell hook: guard-tool (Claude Code hook, PreToolUse)",
        "info  graphify: Graphify (graphify (Claude Code hook, PreToolUse);",
    ] {
        assert!(said.contains(expected), "missing {expected:?} in:\n{said}");
    }
    assert!(
        said.contains(&format!("{}", home.join("CLAUDE.md").display())),
        "{said}"
    );
    assert!(!said.contains("formatter"), "{said}");
}

#[cfg(unix)]
#[test]
fn doctor_checks_that_only_the_user_can_read_the_data_folder() {
    use std::os::unix::fs::PermissionsExt;
    let home = sandbox("doctor-data");
    let (said, _) = doctor(&home);
    assert!(
        said.contains("ok    data folder:") && said.contains("doesn't exist yet"),
        "{said}"
    );

    let dir = home.join(".lagom");
    fs::create_dir_all(dir.join("recall")).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(dir.join("recall"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(dir.join(".lagom-dir"), "marker").unwrap();
    fs::set_permissions(dir.join(".lagom-dir"), fs::Permissions::from_mode(0o644)).unwrap();
    let (said, _) = doctor(&home);
    assert!(
        said.contains("ok    data folder: only you can read"),
        "{said}"
    );

    fs::write(dir.join("recall/abc.txt"), "secret").unwrap();
    fs::set_permissions(
        dir.join("recall/abc.txt"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let (said, code) = doctor(&home);
    assert!(
        said.contains("warn  data folder: other users can open 1 item in"),
        "{said}"
    );
    assert!(said.contains("fix: chmod -R go-rwx"), "{said}");
    assert_eq!(code, Some(0));
}

#[test]
fn doctor_reports_claude_codes_server_managed_policy() {
    let home = sandbox("doctor-policy");
    fs::create_dir_all(home.join(".claude")).unwrap();
    let (said, _) = doctor(&home);
    if said.contains("an MDM profile") {
        return; // this machine has a policy Lagom can't read; nothing below applies
    }
    assert!(
        said.contains("ok    claude policy:") && said.contains("no server-managed settings"),
        "{said}"
    );

    fs::write(
        home.join(".claude/remote-settings.json"),
        "{\"permissions\": {\"deny\": [",
    )
    .unwrap();
    let (said, _) = doctor(&home);
    assert!(
        said.contains("warn  claude policy: Lagom can't read")
            && said.contains("remote-settings.json"),
        "{said}"
    );
    assert!(said.contains("uncompressed"), "{said}");

    write_json(
        &home,
        ".claude/remote-settings.json",
        json!({"permissions": {"deny": ["Bash(rm:*)"]}}),
    );
    let (said, _) = doctor(&home);
    assert!(
        said.contains("server-managed settings cached; Lagom rewrites"),
        "{said}"
    );
}

#[test]
fn doctor_json_has_the_same_checks() {
    let home = sandbox("doctor-json");
    fs::create_dir_all(home.join(".codex")).unwrap();
    let out = lagom(&home, &["doctor", "--json"], None);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["lagom"], env!("CARGO_PKG_VERSION"));
    let codex = v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["check"] == "codex hooks")
        .unwrap();
    assert_eq!(codex["mark"], "info");
    assert_eq!(codex["fix"], "lagom install codex");
    assert_eq!(
        lagom(&home, &["doctor", "--fix"], None).status.code(),
        Some(2)
    );
}
