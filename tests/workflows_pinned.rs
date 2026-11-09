//! Every GitHub Action a workflow uses must be pinned to a full commit SHA (QA-01).
//! A tag like `@v5` can be moved to other code; a SHA cannot.

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

#[test]
fn every_action_is_pinned_by_sha() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    let mut problems = Vec::new();
    let mut checked = 0;
    for entry in fs::read_dir(&dir).expect("read .github/workflows") {
        let path = entry.expect("dir entry").path();
        let is_yaml = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("yml" | "yaml")
        );
        if !is_yaml {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read workflow");
        for (n, line) in text.lines().enumerate() {
            if let Some(action) = action_ref(line) {
                checked += 1;
                if unpinned(action) {
                    let name = path.file_name().unwrap().to_string_lossy();
                    problems.push(format!("{name}:{}: {action}", n + 1));
                }
            }
        }
    }
    assert!(checked > 0, "no `uses:` lines found in {}", dir.display());
    assert!(
        problems.is_empty(),
        "pin these actions to a 40-character commit SHA, with the tag in a comment:\n{}",
        problems.join("\n")
    );
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
