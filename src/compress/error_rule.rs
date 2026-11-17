//! The error rule (D-35), as a property test: every error line of the input is in the
//! output, or counted in a notice that names `lagom recall`. Checked on the corpus, the
//! golden fixtures and seeded generated outputs.

use std::fs;
use std::path::Path;

use super::{compress, is_error_priority, strip_ansi};

/// Error lines of `raw` that are not in `out`, ignoring whitespace (the JSON compressor
/// re-serializes, repeats get `[×N]`), and how many error lines `out`'s notices count.
fn check(raw: &str, failed: bool) -> Result<(), String> {
    let out = compress(raw, failed).text;
    let squeeze = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let out_flat = squeeze(&out);
    let cleaned = strip_ansi(raw);
    let missing: Vec<&str> = cleaned
        .lines()
        // The last `\r` segment is what a terminal shows, and what Lagom keeps.
        .map(|l| l.rsplit('\r').next().unwrap_or(l))
        .filter(|l| is_error_priority(l) && !out_flat.contains(&squeeze(l)))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let counted: usize = out
        .lines()
        .filter(|l| l.contains("more error lines left out") && l.contains("lagom recall"))
        .filter_map(|l| {
            l.split_whitespace()
                .nth(1)?
                .trim_start_matches('[')
                .parse::<usize>()
                .ok()
        })
        .sum();
    if missing.len() <= counted {
        return Ok(());
    }
    Err(format!(
        "{} error lines missing, {counted} counted; first: {:?}",
        missing.len(),
        missing[0]
    ))
}

fn cases(dir: &Path) -> Vec<(String, String, bool)> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let input = path.join("input.txt");
        if input.is_file() {
            let meta = fs::read_to_string(path.join("meta.json")).unwrap_or_default();
            let failed = !meta.contains("\"exit_code\": 0") && meta.contains("\"exit_code\"");
            let raw = fs::read_to_string(&input).expect("read input.txt");
            found.push((path.display().to_string(), raw, failed));
        } else if path.is_dir() {
            found.extend(cases(&path));
        }
    }
    found
}

#[test]
fn every_error_line_is_kept_or_counted_on_the_corpus() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut all = cases(&root.join("bench/corpus"));
    all.extend(cases(&root.join("tests/fixtures/compress")));
    assert!(all.len() > 40, "corpus not found: {} cases", all.len());
    let failures: Vec<String> = all
        .iter()
        .flat_map(|(name, raw, failed)| {
            // Each case both as it ran and with the other exit status.
            [*failed, !*failed].into_iter().filter_map(move |f| {
                check(raw, f)
                    .err()
                    .map(|e| format!("{name} (failed={f}): {e}"))
            })
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// xorshift64*: seeded, so a failure names a seed that reproduces it.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// One generated output: a mix of the shapes real commands print, or, one time in four,
/// JSON Lines as `go test -json` prints them, so the JSON compressor is checked too.
fn generate(rng: &mut Rng) -> String {
    let n = rng.below(1500);
    if rng.below(4) == 0 {
        return (0..n)
            .map(|i| match rng.below(12) {
                0 => format!("{{\"Action\":\"fail\",\"Test\":\"Test{i}\"}}\n"),
                1 => format!("{{\"Action\":\"output\",\"Output\":\"--- FAIL: Test{i} (0.00s)\\n\"}}\n"),
                2 => format!("{{\"Action\":\"output\",\"Output\":\"    m_test.go:{i}: error: got {i}\\n\"}}\n"),
                _ => format!("{{\"Action\":\"pass\",\"Test\":\"Test{i}\",\"Elapsed\":0.0{}}}\n", rng.below(9)),
            })
            .collect();
    }
    let mut out = String::new();
    for i in 0..n {
        let line = match rng.below(20) {
            0 => format!(
                "src/m{}.rs:{}:{}: error: cannot find value `v{i}`",
                rng.below(9),
                i,
                rng.below(80)
            ),
            1 => format!("error[E0{}]: mismatched types", 300 + rng.below(99)),
            2 => format!(
                "\x1b[1m\x1b[31merror\x1b[0m: could not compile `crate{}`",
                rng.below(5)
            ),
            3 => format!("test tests::case_{i} ... FAILED"),
            4 => format!("thread 'main' panicked at src/lib.rs:{i}:9:"),
            5 => format!(
                "2026-11-17T10:{:02}:{:02}Z ERROR worker {i} failed to connect",
                i % 60,
                rng.below(60)
            ),
            6 => format!(
                "2026-11-17T10:{:02}:00Z CRIT disk {} full",
                i % 60,
                rng.below(4)
            ),
            7 => format!(
                "10.0.0.{} - - [17/Nov/2026:10:00:{:02}] \"GET /a/{i} HTTP/1.1\" 503 12",
                rng.below(9),
                i % 60
            ),
            8 => format!(
                "{{\"Action\":\"fail\",\"Test\":\"Test{i}\",\"Elapsed\":0.{}}}",
                rng.below(9)
            ),
            9 => format!("warning: unused variable `w{i}` at src/w.rs:{i}"),
            10 => format!("pkg/f{}.py:{i}:1: E501 line too long", rng.below(30)),
            11 => "Compiling dependency v1.0.0".to_string(),
            12 => format!(
                "Downloading {}%\r Downloading {}%",
                rng.below(50),
                50 + rng.below(50)
            ),
            13 => "x".repeat(300 + rng.below(400)),
            14 => format!("step {i} of {n} done"),
            15 => String::new(),
            16 => format!("npm ERR! code E{}", 400 + rng.below(100)),
            _ => format!("ok {i}"),
        };
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[test]
fn every_error_line_is_kept_or_counted_on_generated_output() {
    let mut failures = Vec::new();
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let raw = generate(&mut rng);
        let failed = rng.below(2) == 0;
        if let Err(e) = check(&raw, failed) {
            failures.push(format!("seed {seed} (failed={failed}): {e}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
