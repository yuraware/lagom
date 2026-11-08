//! `lagom bench compress`: run the compressor over a corpus of real outputs and report
//! sizes, `must_keep` retention and time. Exit code 1 if a `must_keep` string is lost
//! in a case without a `known_gap`, so CI can use it as a fidelity gate.
//!
//! A case is a folder with `input.txt` and `meta.json`:
//! `{"exit_code": 1, "must_keep": ["error[E0308]"], "known_gap": "CMP-07"}`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use serde_json::{Value, json};

use crate::compress::{Limits, compress};

const DEFAULT_DIRS: [&str; 2] = ["bench/corpus", "tests/fixtures/compress"];

#[derive(Debug)]
pub struct CaseResult {
    pub case: String,
    pub bytes_in: usize,
    pub bytes_out: usize,
    pub must_keep: usize,
    pub lost: Vec<String>,
    pub known_gap: Option<String>,
    pub micros: u128,
}

/// Find every case folder (one containing `meta.json`) below `dir`, sorted.
fn find_cases(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    if entries.iter().any(|p| p.ends_with("meta.json")) {
        out.push(dir.to_path_buf());
    }
    for p in entries.into_iter().filter(|p| p.is_dir()) {
        find_cases(&p, out);
    }
}

pub fn run_case(dir: &Path, root: &Path) -> Result<CaseResult, String> {
    let read = |name: &str| {
        fs::read(dir.join(name)).map_err(|e| format!("{}: {e}", dir.join(name).display()))
    };
    let input = String::from_utf8_lossy(&read("input.txt")?).into_owned();
    let meta: Value = serde_json::from_slice(&read("meta.json")?)
        .map_err(|e| format!("{}: {e}", dir.join("meta.json").display()))?;
    let failed = meta["exit_code"].as_i64().unwrap_or(0) != 0;
    let start = Instant::now();
    let out = compress(&input, failed, Limits::default()).text;
    let micros = start.elapsed().as_micros();
    let keep: Vec<&str> = meta["must_keep"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let case = dir.strip_prefix(root).unwrap_or(dir).display().to_string();
    Ok(CaseResult {
        case: if case.is_empty() {
            dir.display().to_string()
        } else {
            case
        },
        bytes_in: input.len(),
        bytes_out: out.len(),
        must_keep: keep.len(),
        lost: keep
            .iter()
            .filter(|k| !out.contains(*k))
            .map(|k| k.to_string())
            .collect(),
        known_gap: meta["known_gap"].as_str().map(String::from),
        micros,
    })
}

fn tokens(bytes: usize) -> usize {
    bytes.div_ceil(4)
}

fn pct(saved: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        saved as f64 * 100.0 / total as f64
    }
}

fn to_json(results: &[CaseResult]) -> Value {
    let (bin, bout): (usize, usize) = results
        .iter()
        .fold((0, 0), |a, r| (a.0 + r.bytes_in, a.1 + r.bytes_out));
    json!({
        "v": 1,
        "cases": results.iter().map(|r| json!({
            "case": r.case, "bytes_in": r.bytes_in, "bytes_out": r.bytes_out,
            "must_keep": r.must_keep, "lost": r.lost, "known_gap": r.known_gap, "micros": r.micros,
        })).collect::<Vec<_>>(),
        "totals": {"cases": results.len(), "bytes_in": bin, "bytes_out": bout,
                   "tokens_in_est": tokens(bin), "tokens_out_est": tokens(bout)},
    })
}

/// Markdown report. `baseline` (an earlier `--json` file) adds a change column.
pub fn report(results: &[CaseResult], baseline: Option<&Value>) -> String {
    let before = |case: &str| {
        baseline?
            .get("cases")?
            .as_array()?
            .iter()
            .find(|c| c["case"] == case)?
            .get("bytes_out")?
            .as_u64()
    };
    let mut s = String::from("| case | in | out | saved | kept | µs |");
    s.push_str(if baseline.is_some() {
        " vs baseline |\n|---|---:|---:|---:|---|---:|---:|\n"
    } else {
        "\n|---|---:|---:|---:|---|---:|\n"
    });
    for r in results {
        let kept = format!(
            "{}/{}{}",
            r.must_keep - r.lost.len(),
            r.must_keep,
            match (&r.known_gap, r.lost.is_empty()) {
                (_, true) => String::new(),
                (Some(task), false) => format!(" (known gap: {task})"),
                (None, false) => " **LOST**".to_string(),
            }
        );
        s.push_str(&format!(
            "| {} | {} | {} | {:.0}% | {} | {} |",
            r.case,
            r.bytes_in,
            r.bytes_out,
            pct(r.bytes_in.saturating_sub(r.bytes_out), r.bytes_in),
            kept,
            r.micros
        ));
        if baseline.is_some() {
            s.push_str(&match before(&r.case) {
                Some(b) => format!(" {:+} |", r.bytes_out as i64 - b as i64),
                None => " new |".to_string(),
            });
        }
        s.push('\n');
    }
    let (bin, bout, us): (usize, usize, u128) = results.iter().fold((0, 0, 0), |a, r| {
        (a.0 + r.bytes_in, a.1 + r.bytes_out, a.2 + r.micros)
    });
    let lost = results
        .iter()
        .filter(|r| !r.lost.is_empty() && r.known_gap.is_none())
        .count();
    let gaps = results
        .iter()
        .filter(|r| !r.lost.is_empty() && r.known_gap.is_some())
        .count();
    s.push_str(&format!(
        "\n{} cases: {bin} → {bout} bytes ({:.1}% smaller; about {} → {} tokens at 4 bytes per token), {:.1} ms total. \
         Unexpected must_keep losses: {lost}. Known gaps: {gaps}.\n",
        results.len(), pct(bin.saturating_sub(bout), bin), tokens(bin), tokens(bout), us as f64 / 1000.0
    ));
    if let Some(b) = baseline.and_then(|b| b["totals"]["bytes_out"].as_u64()) {
        s.push_str(&format!(
            "Baseline total out: {b} bytes ({:+} now).\n",
            bout as i64 - b as i64
        ));
    }
    s
}

/// Total output growth against a baseline, in percent (`None` without one).
fn growth_pct(results: &[CaseResult], baseline: &Value) -> Option<f64> {
    let before = baseline["totals"]["bytes_out"].as_u64()? as f64;
    let now: usize = results.iter().map(|r| r.bytes_out).sum();
    (before > 0.0).then(|| (now as f64 - before) * 100.0 / before)
}

/// `lagom bench compress [DIR…] [--json FILE] [--compare BASELINE.json [--max-growth PCT]]`.
pub fn run(args: &[String]) -> ExitCode {
    const USAGE: &str = "usage: lagom bench compress [DIR…] [--json FILE] [--compare BASELINE.json [--max-growth PCT]]";
    let Some(("compress", rest)) = args.split_first().map(|(a, r)| (a.as_str(), r)) else {
        eprintln!("lagom: {USAGE}");
        return ExitCode::from(2);
    };
    let (mut dirs, mut json_out, mut compare, mut max_growth) = (Vec::new(), None, None, None);
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json_out = it.next().cloned(),
            "--compare" => compare = it.next().cloned(),
            "--max-growth" => match it.next().and_then(|v| v.parse::<f64>().ok()) {
                Some(v) => max_growth = Some(v),
                None => {
                    eprintln!("lagom: --max-growth needs a number (percent)\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            s if s.starts_with('-') => {
                eprintln!("lagom: unknown option `{s}`\n{USAGE}");
                return ExitCode::from(2);
            }
            d => dirs.push(PathBuf::from(d)),
        }
    }
    if dirs.is_empty() {
        dirs = DEFAULT_DIRS
            .iter()
            .map(PathBuf::from)
            .filter(|d| d.is_dir())
            .collect();
    }
    let mut cases = Vec::new();
    for d in &dirs {
        let start = cases.len();
        find_cases(d, &mut cases);
        if cases.len() == start {
            eprintln!(
                "lagom: no cases (folders with meta.json) under {}",
                d.display()
            );
        }
    }
    if cases.is_empty() {
        eprintln!("lagom: nothing to run. {USAGE}");
        return ExitCode::from(2);
    }
    let mut results = Vec::new();
    for case in &cases {
        let root = dirs
            .iter()
            .find(|d| case.starts_with(d))
            .map_or(Path::new(""), |d| d.as_path());
        match run_case(case, root.parent().unwrap_or(root)) {
            Ok(r) => results.push(r),
            Err(e) => {
                eprintln!("lagom: {e}");
                return ExitCode::from(2);
            }
        }
    }
    let baseline = match compare.map(|p| fs::read(&p).map_err(|e| format!("{p}: {e}"))) {
        Some(Ok(b)) => serde_json::from_slice::<Value>(&b).ok(),
        Some(Err(e)) => {
            eprintln!("lagom: {e}");
            return ExitCode::from(2);
        }
        None => None,
    };
    if max_growth.is_some() && baseline.is_none() {
        eprintln!("lagom: --max-growth needs --compare BASELINE.json");
        return ExitCode::from(2);
    }
    print!("{}", report(&results, baseline.as_ref()));
    let too_big = match (
        max_growth,
        baseline.as_ref().and_then(|b| growth_pct(&results, b)),
    ) {
        (Some(limit), Some(g)) if g > limit => {
            println!("Total output grew {g:.1}% against the baseline (limit {limit}%).");
            true
        }
        _ => false,
    };
    if let Some(path) = json_out {
        let text = serde_json::to_string_pretty(&to_json(&results)).unwrap_or_default() + "\n";
        if let Err(e) = fs::write(&path, text) {
            eprintln!("lagom: cannot write {path}: {e}");
            return ExitCode::from(2);
        }
    }
    if too_big
        || results
            .iter()
            .any(|r| !r.lost.is_empty() && r.known_gap.is_none())
    {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
