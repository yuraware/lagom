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
    assert_eq!(
        fs::read_to_string(home.join(".codex/hooks.json")).unwrap(),
        codex_before
    );

    let status = text(&lagom(&home, &["status"], None));
    assert!(status.contains("claude  hooks: yes"), "{status}");

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
        for _ in 0..50 {
            if stdin.write_all(&chunk).is_err() {
                break; // the hook stopped reading: expected
            }
        }
    });
    let out = child.wait_with_output().unwrap();
    let _ = writer.join();
    assert!(out.status.success());
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
}

#[test]
fn uninstall_all_purge_removes_everything_but_backups() {
    let home = sandbox("purge");
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(home.join(".claude/settings.json"), r#"{"model":"opus"}"#).unwrap();
    assert!(lagom(&home, &["install", "--yes"], None).status.success());
    lagom(&home, &["style", "use", "terse"], None);
    lagom(&home, &["run", "--", "echo", "hi"], None);
    assert!(home.join(".lagom").exists() && home.join(".config/lagom").exists());

    // Without --yes and without a terminal, purge refuses and deletes nothing.
    let out = lagom(&home, &["uninstall", "--all", "--purge"], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(home.join(".lagom").exists());

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
    assert!(!home.join(".lagom").exists());
    assert!(!home.join(".config/lagom").exists());
    assert!(!home.join(".claude/skills/lagom").exists());
    assert!(home.join(".claude/settings.json.lagom.bak").exists());
    let settings = fs::read_to_string(home.join(".claude/settings.json")).unwrap();
    assert!(!settings.contains("hook claude"), "{settings}");
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
    assert_eq!(grow("x", &json_path).status.code(), Some(2));
    let no_base = lagom(
        &home,
        &["bench", "compress", &dir, "--max-growth", "5"],
        None,
    );
    assert_eq!(no_base.status.code(), Some(2));
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

    let stats = text(&lagom(&home, &["stats"], None));
    assert!(
        stats.contains("by agent:") && stats.contains("claude") && stats.contains("manual"),
        "{stats}"
    );
    assert!(
        stats.contains("by style:") && stats.contains("terse+lean-code"),
        "{stats}"
    );
}
