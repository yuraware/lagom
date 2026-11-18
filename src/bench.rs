//! `lagom bench compress`: run the compressor over a corpus of real outputs and report
//! sizes, `must_keep` retention and time, per case and per family (median saving and the
//! share of outputs compressed, D-36). Exit code 1 if a `must_keep` string is lost in a
//! case without a `known_gap`, so CI can use it as a fidelity gate.
//!
//! A case is a folder with `input.txt` and `meta.json`:
//! `{"exit_code": 1, "must_keep": ["error[E0308]"], "known_gap": "CMP-07"}`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use serde_json::{Value, json};

use crate::args::{Args, usage_error};
use crate::compress::compress;

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
    let out = compress(&input, failed).text;
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

fn saved_pct(r: &CaseResult) -> f64 {
    pct(r.bytes_in.saturating_sub(r.bytes_out), r.bytes_in)
}

/// One family of cases (the folder a case is in, such as `git` or `rust`): how many,
/// the median saving per case, and how many outputs got smaller at all. Medians, so
/// one huge log can't stand for the whole corpus (D-36).
#[derive(Debug, PartialEq)]
pub struct Family {
    pub name: String,
    pub cases: usize,
    pub median_saved_pct: f64,
    pub compressed: usize,
}

fn family_of(case: &str) -> String {
    Path::new(case)
        .parent()
        .and_then(Path::file_name)
        .map_or_else(|| case.to_string(), |n| n.to_string_lossy().into_owned())
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    match v.len() {
        0 => 0.0,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

/// Families in name order.
pub fn families(results: &[CaseResult]) -> Vec<Family> {
    let mut names: Vec<String> = results.iter().map(|r| family_of(&r.case)).collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|name| {
            let members: Vec<&CaseResult> = results
                .iter()
                .filter(|r| family_of(&r.case) == name)
                .collect();
            Family {
                cases: members.len(),
                median_saved_pct: median(members.iter().map(|r| saved_pct(r)).collect()),
                compressed: members.iter().filter(|r| r.bytes_out < r.bytes_in).count(),
                name,
            }
        })
        .collect()
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
        "families": families(results).iter().map(|f| json!({
            "family": f.name, "cases": f.cases,
            "median_saved_pct": (f.median_saved_pct * 10.0).round() / 10.0,
            "compressed": f.compressed,
        })).collect::<Vec<_>>(),
        "totals": {"cases": results.len(), "bytes_in": bin, "bytes_out": bout,
                   "tokens_in_est": tokens(bin), "tokens_out_est": tokens(bout),
                   "compressed": results.iter().filter(|r| r.bytes_out < r.bytes_in).count()},
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
                // A gap that closed: say so, so the marker doesn't outlive the problem.
                (Some(task), true) => format!(" (known gap {task} now met: remove the marker)"),
                (None, true) => String::new(),
                (Some(task), false) => format!(" (known gap: {task})"),
                (None, false) => " **LOST**".to_string(),
            }
        );
        s.push_str(&format!(
            "| {} | {} | {} | {:.0}% | {} | {} |",
            r.case,
            r.bytes_in,
            r.bytes_out,
            saved_pct(r),
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
    let (bout, us): (usize, u128) = results
        .iter()
        .fold((0, 0), |a, r| (a.0 + r.bytes_out, a.1 + r.micros));
    let lost = results
        .iter()
        .filter(|r| !r.lost.is_empty() && r.known_gap.is_none())
        .count();
    let gaps = results
        .iter()
        .filter(|r| !r.lost.is_empty() && r.known_gap.is_some())
        .count();
    s.push_str(&family_table(results));
    let compressed = results.iter().filter(|r| r.bytes_out < r.bytes_in).count();
    s.push_str(&format!(
        "\n{} cases: {compressed} outputs compressed ({:.0}%), median saving per case {:.0}%, {:.1} ms total. \
         Unexpected must_keep losses: {lost}. Known gaps: {gaps}.\n",
        results.len(),
        pct(compressed, results.len()),
        median(results.iter().map(saved_pct).collect()),
        us as f64 / 1000.0
    ));
    if let Some(b) = baseline.and_then(|b| b["totals"]["bytes_out"].as_u64()) {
        s.push_str(&format!(
            "Baseline total out: {b} bytes ({:+} now).\n",
            bout as i64 - b as i64
        ));
    }
    s
}

/// The per-family table: the published result, since a byte-weighted total lets the largest
/// outputs speak for every command (D-36).
pub fn family_table(results: &[CaseResult]) -> String {
    let mut s =
        String::from("\n| family | cases | median saved | compressed |\n|---|---:|---:|---:|\n");
    for f in families(results) {
        s.push_str(&format!(
            "| {} | {} | {:.0}% | {}/{} |\n",
            f.name, f.cases, f.median_saved_pct, f.compressed, f.cases
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
    const USAGE: &str =
        "lagom bench compress [DIR…] [--json FILE] [--compare BASELINE.json [--max-growth PCT]]";
    let Some(("compress", rest)) = args.split_first().map(|(a, r)| (a.as_str(), r)) else {
        return usage_error("which benchmark? only `compress` exists", USAGE);
    };
    let a = match Args::parse(
        rest,
        &["--json=", "--compare=", "--max-growth="],
        usize::MAX,
    ) {
        Ok(a) => a,
        Err(e) => return usage_error(&e, USAGE),
    };
    let mut dirs: Vec<PathBuf> = a.positionals.iter().map(PathBuf::from).collect();
    let json_out = a.value("--json").map(String::from);
    let compare = a.value("--compare").map(String::from);
    let max_growth = match a.value("--max-growth") {
        None => None,
        Some(v) => match v.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0) {
            Some(v) => Some(v),
            None => return usage_error("--max-growth needs a number (percent)", USAGE),
        },
    };
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
        return usage_error("nothing to run", USAGE);
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
    // A baseline that can't be read or isn't a bench report is an error: silently
    // comparing against nothing would hide the very change the comparison is for.
    let baseline = match compare.map(|p| {
        let b = fs::read(&p).map_err(|e| format!("{p}: {e}"))?;
        serde_json::from_slice::<Value>(&b)
            .ok()
            .filter(|v| v["totals"]["bytes_out"].is_u64() && v["cases"].is_array())
            .ok_or(format!("{p}: not a `lagom bench compress --json` report"))
    }) {
        Some(Ok(b)) => Some(b),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn result(case: &str, bytes_in: usize, bytes_out: usize) -> CaseResult {
        CaseResult {
            case: case.into(),
            bytes_in,
            bytes_out,
            must_keep: 0,
            lost: Vec::new(),
            known_gap: None,
            micros: 0,
        }
    }

    #[test]
    fn families_report_medians_not_byte_totals() {
        // One huge log mustn't speak for its family: the median does.
        let r = [
            result("corpus/logs/huge", 1_000_000, 1_000),
            result("corpus/logs/small-a", 100, 100),
            result("corpus/logs/small-b", 100, 90),
            result("corpus/git/one", 10, 10),
        ];
        let f = families(&r);
        assert_eq!(f[0].name, "git");
        assert_eq!((f[0].cases, f[0].compressed), (1, 0));
        assert_eq!(f[1].name, "logs");
        assert_eq!((f[1].cases, f[1].compressed), (3, 2));
        assert!((f[1].median_saved_pct - 10.0).abs() < 1e-9, "{:?}", f[1]);
        let text = report(&r, None);
        assert!(text.contains("| logs | 3 | 10% | 2/3 |"), "{text}");
        assert!(
            text.contains("4 cases: 2 outputs compressed (50%)"),
            "{text}"
        );
    }

    /// The error lines of a failing case: lines `is_error_line` matches, after colors are
    /// removed. A JSON Lines record (`go test -json`) counts by the text in its `Output`,
    /// written as in JSON (`FAIL\\tpkg`), since the JSON compressor keeps records as JSON.
    fn error_lines(input: &str) -> Vec<String> {
        let mut lines: Vec<String> = crate::compress::strip_ansi(input)
            .lines()
            .map(|l| match serde_json::from_str::<Value>(l) {
                Ok(v) if v.is_object() => {
                    let text = v["Output"].as_str().unwrap_or("").trim();
                    let quoted = Value::String(text.into()).to_string();
                    quoted[1..quoted.len() - 1].to_string()
                }
                _ => l.trim().to_string(),
            })
            .filter(|l| crate::compress::is_error_line(l))
            .collect();
        lines.sort();
        lines.dedup();
        lines
    }

    /// Whether `entry` stands for `line` in a `must_keep`: the whole line, or at least 20
    /// characters of it, so a bare `error:` or `FAIL` can't stand for every error line.
    fn covers(entry: &str, line: &str) -> bool {
        entry == line || (entry.chars().count() >= 20 && line.contains(entry))
    }

    /// Every error line of a failing case is listed in its `must_keep` (BEN-19).
    #[test]
    fn a_failing_case_lists_every_error_line_in_must_keep() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut cases = Vec::new();
        for d in DEFAULT_DIRS {
            find_cases(&root.join(d), &mut cases);
        }
        assert!(cases.len() > 40, "corpus not found");
        let mut problems = Vec::new();
        for case in cases {
            let meta: Value =
                serde_json::from_str(&fs::read_to_string(case.join("meta.json")).unwrap()).unwrap();
            if meta["exit_code"].as_i64().unwrap_or(0) == 0 {
                continue;
            }
            let input = fs::read_to_string(case.join("input.txt")).unwrap_or_default();
            let keep: Vec<&str> = meta["must_keep"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            for line in error_lines(&input) {
                if !keep.iter().any(|k| covers(k, &line)) {
                    problems.push(format!("{}: {line:?}", case.display()));
                }
            }
        }
        assert!(
            problems.is_empty(),
            "error lines missing from must_keep (the whole line, or 20+ characters of it):\n{}",
            problems.join("\n")
        );
    }
}
