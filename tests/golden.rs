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
    assert!(cases.len() >= 15, "expected at least 15 cases, found {}", cases.len());

    let mut failures = Vec::new();
    for case in &cases {
        let name = case.file_name().unwrap().to_string_lossy();
        let input = fs::read_to_string(case.join("input.txt")).unwrap();
        let meta: Value =
            serde_json::from_str(&fs::read_to_string(case.join("meta.json")).unwrap()).unwrap();
        let failed = meta["exit_code"].as_i64().unwrap_or(0) != 0;
        let out = compress(&input, failed, Limits::default()).text;

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
            let line = out
                .lines()
                .zip(expected.lines())
                .position(|(a, b)| a != b)
                .unwrap_or(out.lines().count().min(expected.lines().count()));
            failures.push(format!(
                "{name}: output differs from expected.txt at line {} (run with LAGOM_BLESS=1 if intended)",
                line + 1
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
