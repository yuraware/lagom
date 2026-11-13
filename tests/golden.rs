//! Golden tests for the compressor: real command outputs in `tests/fixtures/compress/<case>/`.
//!
//! Each case has `input.txt`, `meta.json` (`exit_code`, `must_keep`) and `expected.txt`.
//! Every `must_keep` string must survive compression. Run with `LAGOM_BLESS=1` to rewrite
//! `expected.txt` after an intended change, then review the diff.
//! Regenerate inputs with `scripts/gen_compress_fixtures.sh`.

use std::fs;
use std::path::Path;

use lagom::compress::{Limits, compress};
use serde_json::Value;

#[test]
fn compressor_golden_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compress");
    let bless = std::env::var_os("LAGOM_BLESS").is_some();
    let mut cases: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    cases.sort();
    assert!(
        cases.len() >= 15,
        "expected at least 15 cases, found {}",
        cases.len()
    );

    let mut failures = Vec::new();
    for case in &cases {
        let name = case.file_name().unwrap().to_string_lossy();
        let input = fs::read_to_string(case.join("input.txt")).unwrap();
        let meta: Value =
            serde_json::from_str(&fs::read_to_string(case.join("meta.json")).unwrap()).unwrap();
        let failed = meta["exit_code"].as_i64().unwrap_or(0) != 0;
        let out = compress(&input, failed, Limits::default()).text;
        if compress(&out, failed, Limits::default()).text != out {
            failures.push(format!("{name}: compressing the output again changed it"));
        }

        for keep in meta["must_keep"].as_array().into_iter().flatten() {
            let keep = keep.as_str().unwrap();
            if !out.contains(keep) {
                failures.push(format!("{name}: lost must_keep {keep:?}"));
            }
        }
        let expected_path = case.join("expected.txt");
        if bless {
            fs::write(&expected_path, &out).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&expected_path).unwrap_or_default();
        if out != expected {
            failures.push(format!(
                "{name}: output differs from expected.txt (run with LAGOM_BLESS=1 if intended)\n{}",
                diff(&expected, &out)
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// The changed region between `expected` and `actual`, with two lines of context:
/// `-` lines are expected, `+` lines are what the compressor produced now.
fn diff(expected: &str, actual: &str) -> String {
    let (e, a): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
    let start = e.iter().zip(&a).take_while(|(x, y)| x == y).count();
    let common_end = e[start..]
        .iter()
        .rev()
        .zip(a[start..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (e_end, a_end) = (e.len() - common_end, a.len() - common_end);
    let from = start.saturating_sub(2);
    let mut out = format!(
        "@@ line {}: expected {} lines, got {} @@\n",
        start + 1,
        e.len(),
        a.len()
    );
    for line in &e[from..start] {
        out += &format!("  {line}\n");
    }
    const MAX: usize = 20;
    for line in e[start..e_end].iter().take(MAX) {
        out += &format!("- {line}\n");
    }
    for line in a[start..a_end].iter().take(MAX) {
        out += &format!("+ {line}\n");
    }
    if e_end - start > MAX || a_end - start > MAX {
        out += "  … (more changed lines)\n";
    }
    for line in e[e_end..].iter().take(2) {
        out += &format!("  {line}\n");
    }
    if expected.ends_with('\n') != actual.ends_with('\n') {
        out += "  (the final newline differs)\n";
    }
    out
}

#[test]
fn diff_shows_the_changed_lines_with_context() {
    let d = diff("a\nb\nc\nd\ne\n", "a\nb\nX\nd\ne\n");
    assert_eq!(
        d,
        "@@ line 3: expected 5 lines, got 5 @@\n  a\n  b\n- c\n+ X\n  d\n  e\n"
    );
    let d = diff("a\nb\n", "a\nb\nc\n");
    assert!(d.contains("+ c") && !d.contains("- "), "{d}");
    assert!(diff("a", "a\n").contains("final newline"));
}
