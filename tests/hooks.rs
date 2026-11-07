//! Hook contract tests: fixed agent payloads in, exact hook output out. See tests/hooks/README.md.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn copy_dir(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn cases(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for agent in fs::read_dir(root)
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_dir())
    {
        for case in fs::read_dir(agent.path()).unwrap().flatten() {
            out.push((
                agent.file_name().to_string_lossy().into_owned(),
                case.path(),
            ));
        }
    }
    out.sort();
    out
}

#[test]
fn hook_contract_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hooks");
    let bin = env!("CARGO_BIN_EXE_lagom");
    let bless = std::env::var_os("LAGOM_BLESS").is_some();
    let mut failures = Vec::new();
    let all = cases(&root);
    assert!(all.len() >= 15, "found {} cases", all.len());

    for (agent, case) in &all {
        let name = case.file_name().unwrap().to_string_lossy().into_owned();
        let event = name.split("--").next().unwrap();
        let home =
            std::env::temp_dir().join(format!("lagom-hooks-{agent}-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join("repo")).unwrap();
        if case.join("home").is_dir() {
            copy_dir(&case.join("home"), &home);
        }
        let home_str = home.to_string_lossy().into_owned();
        let input = fs::read_to_string(case.join("input.json"))
            .unwrap()
            .replace("<HOME>", &home_str);

        let mut child = Command::new(bin)
            .args(["hook", agent, event])
            .env("HOME", &home)
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("LAGOM_HOME")
            .env_remove("LAGOM_STYLES")
            .env_remove("LAGOM_DISABLE")
            .env_remove("LAGOM_REWRITE")
            .current_dir(&home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout)
            .replace(bin, "<LAGOM>")
            .replace(&home_str, "<HOME>");
        let actual = if stdout.trim().is_empty() {
            String::new()
        } else {
            let v: serde_json::Value = serde_json::from_str(stdout.trim())
                .unwrap_or_else(|e| panic!("{agent}/{name}: output is not JSON ({e}): {stdout}"));
            serde_json::to_string_pretty(&v).unwrap() + "\n"
        };
        let _ = fs::remove_dir_all(&home);
        if !out.status.success() {
            failures.push(format!("{agent}/{name}: exit {:?}", out.status.code()));
        }

        let expected_path = case.join("expected.json");
        if bless {
            fs::write(&expected_path, &actual).unwrap();
        } else if fs::read_to_string(&expected_path).unwrap_or_default() != actual {
            failures.push(format!(
                "{agent}/{name}: output differs from expected.json:\n{actual}"
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
