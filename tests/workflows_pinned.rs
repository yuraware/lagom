//! Every GitHub Action a workflow uses must be pinned to a full commit SHA (QA-01).
//! A tag like `@v5` can be moved to other code; a SHA cannot. Every workflow also starts
//! from a read-only token (SEC-07).

use std::fs;
use std::path::Path;

/// Returns `owner/repo@ref` for a `uses:` line, or `None` for other lines.
fn action_ref(line: &str) -> Option<&str> {
    let rest = line.trim_start().trim_start_matches("- ");
    let value = rest.strip_prefix("uses:")?.trim();
    let value = value.split('#').next().unwrap_or("").trim();
    Some(value.trim_matches(|c| c == '"' || c == '\''))
}

/// Local actions (`./path`) and Docker images are not tags on a repo, so only
/// `owner/repo@ref` needs a SHA.
fn unpinned(action: &str) -> bool {
    if action.starts_with("./") || action.starts_with("docker://") {
        return false;
    }
    match action.rsplit_once('@') {
        Some((_, rev)) => !(rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit())),
        None => true,
    }
}

/// (file name, text) of every workflow.
fn workflows() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    let mut found = Vec::new();
    for entry in fs::read_dir(&dir).expect("read .github/workflows") {
        let path = entry.expect("dir entry").path();
        let is_yaml = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("yml" | "yaml")
        );
        if is_yaml {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            found.push((name, fs::read_to_string(&path).expect("read workflow")));
        }
    }
    assert!(!found.is_empty(), "no workflows in {}", dir.display());
    found
}

#[test]
fn every_action_is_pinned_by_sha() {
    let mut problems = Vec::new();
    let mut checked = 0;
    for (name, text) in workflows() {
        for (n, line) in text.lines().enumerate() {
            if let Some(action) = action_ref(line) {
                checked += 1;
                if unpinned(action) {
                    problems.push(format!("{name}:{}: {action}", n + 1));
                }
            }
        }
    }
    assert!(checked > 0, "no `uses:` lines found");
    assert!(
        problems.is_empty(),
        "pin these actions to a 40-character commit SHA, with the tag in a comment:\n{}",
        problems.join("\n")
    );
}

/// What is wrong with a workflow's top-level `permissions:`, if anything. Every workflow
/// starts from a read-only token (SEC-07): a job that needs more asks for it itself, so
/// a new workflow never inherits the repository's default, which may allow writes.
fn top_level_permissions_problem(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(i) = lines.iter().position(|l| l.starts_with("permissions:")) else {
        return Some("no top-level `permissions:`".into());
    };
    let value = |l: &str| l.split('#').next().unwrap_or("").trim().to_string();
    let inline = value(&lines[i]["permissions:".len()..]);
    if !inline.is_empty() {
        return (inline != "{}" && inline != "read-all")
            .then(|| format!("top-level `permissions: {inline}` isn't read-only"));
    }
    lines[i + 1..]
        .iter()
        .take_while(|l| l.starts_with(' ') || l.trim().is_empty())
        .map(|l| value(l))
        .find(|v| !v.is_empty() && !v.ends_with(": read") && !v.ends_with(": none"))
        .map(|v| format!("top-level `{v}` isn't read-only: grant it in the job that needs it"))
}

#[test]
fn every_workflow_starts_from_a_read_only_token() {
    let problems: Vec<String> = workflows()
        .into_iter()
        .filter_map(|(name, text)| {
            Some(format!("{name}: {}", top_level_permissions_problem(&text)?))
        })
        .collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn permissions_check_tells_read_from_write() {
    let ok = "on: push\npermissions:\n  contents: read\n\njobs:\n  a:\n    permissions:\n      contents: write\n";
    assert_eq!(top_level_permissions_problem(ok), None);
    assert_eq!(top_level_permissions_problem("permissions: {}\n"), None);
    assert!(top_level_permissions_problem("on: push\njobs:\n").is_some());
    assert!(top_level_permissions_problem("permissions: write-all\n").is_some());
    let write = "permissions:\n  contents: read\n  packages: write # publish\njobs:\n";
    assert!(top_level_permissions_problem(write).is_some());
}

#[test]
fn pin_check_tells_tags_from_shas() {
    let sha = "3d3c42e5aac5ba805825da76410c181273ba90b1";
    assert_eq!(
        action_ref(&format!("      - uses: actions/checkout@{sha} # v7.0.1")),
        Some(format!("actions/checkout@{sha}").as_str())
    );
    assert_eq!(
        action_ref("        uses: 'actions/upload-artifact@v4'"),
        Some("actions/upload-artifact@v4")
    );
    assert_eq!(action_ref("      - run: cargo test"), None);

    assert!(!unpinned(&format!("actions/checkout@{sha}")));
    assert!(unpinned("actions/checkout@v5"));
    assert!(unpinned("dtolnay/rust-toolchain@stable"));
    assert!(unpinned("actions/checkout"));
    assert!(unpinned("actions/checkout@3d3c42e")); // short SHAs can collide
    assert!(!unpinned("./.github/actions/setup"));
    assert!(!unpinned("docker://alpine:3.20"));
}
