//! Deterministic command-output compressor.
//!
//! Rules: never hide failures, keep exact text of what survives, and report
//! anything dropped so the caller can store the original for `lagom recall`.

mod diff;
mod search;

/// Output budget. Failed commands get double, so errors are never squeezed out.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_lines: usize,
    pub head: usize,
    pub tail: usize,
    pub max_important: usize,
    pub max_line_chars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_lines: 120,
            head: 30,
            tail: 50,
            max_important: 40,
            max_line_chars: 400,
        }
    }
}

impl Limits {
    fn for_failure(self) -> Self {
        Limits {
            max_lines: self.max_lines * 2,
            head: self.head * 2,
            tail: self.tail * 2,
            max_important: self.max_important * 2,
            max_line_chars: self.max_line_chars,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Compressed {
    pub text: String,
    /// True when content was dropped (not just reformatted); the original should be kept for recall.
    pub lossy: bool,
    /// The command reported success but its output has error lines, so it got the
    /// larger failure budget.
    pub errors_despite_success: bool,
}

/// Compress raw command output. `failed` means a non-zero exit code.
pub fn compress(raw: &str, failed: bool, limits: Limits) -> Compressed {
    if is_compressed(raw) {
        // Compressing twice must change nothing (and must not drop more lines).
        return Compressed {
            text: raw.to_string(),
            lossy: false,
            errors_despite_success: false,
        };
    }
    // Some tools exit 0 and still print errors: treat those like failures.
    let errors_despite_success = !failed && raw.lines().any(is_error_line);
    let limits = if failed || errors_despite_success {
        limits.for_failure()
    } else {
        limits
    };
    let mut lossy = false;

    let cleaned = strip_ansi(raw);
    let lines = normalize_lines(&cleaned);
    if let Some(grouped) =
        search::compress(&lines, limits).or_else(|| diff::compress(&lines, limits))
    {
        let mut text = grouped.join("\n");
        if raw.ends_with('\n') {
            text.push('\n');
        }
        return Compressed {
            text,
            lossy: true,
            errors_despite_success,
        };
    }
    let lines = collapse_repeats(lines, &mut lossy);
    let lines: Vec<String> = lines
        .into_iter()
        .map(|l| shorten_line(l, limits.max_line_chars, &mut lossy))
        .collect();
    let lines = keep_head_tail_important(lines, limits, &mut lossy);

    let mut text = lines.join("\n");
    if raw.ends_with('\n') && !text.is_empty() {
        text.push('\n');
    }
    Compressed {
        text,
        lossy,
        errors_despite_success,
    }
}

/// A line that reports a failure, as opposed to merely mentioning one: `error:`,
/// `error[E…]`, `fatal:`, `panicked at`, a Python traceback, `FAILED`, `npm ERR!`,
/// `Exception in thread`, `segmentation fault`. Summaries like "0 failed" don't count.
pub fn is_error_line(line: &str) -> bool {
    const MARKERS: [&str; 8] = [
        "error:",
        "error[",
        "fatal:",
        "panicked at",
        "traceback (most recent call last)",
        "err!",
        "exception in thread",
        "segmentation fault",
    ];
    let bytes = line.as_bytes();
    MARKERS.iter().any(|m| {
        let m = m.as_bytes();
        bytes.windows(m.len()).any(|w| w.eq_ignore_ascii_case(m))
    }) || line
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == "FAILED" || w == "FAIL")
}

/// True if `text` already went through Lagom: it carries one of the markers the
/// compressor writes (`… [N lines omitted]`, `… [N similar lines]`, `  [×N]`,
/// `…[+N chars]`, the search summary's `  … +N more` and `N more files:`) or
/// `lagom run`'s recall footer.
pub fn is_compressed(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_end();
        if line.starts_with("[lagom: ") && line.contains("lagom recall ") {
            return true;
        }
        let bracket = |start: &str, end: &str| {
            line.strip_prefix(start)
                .and_then(|r| r.strip_suffix(end))
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        };
        if bracket("… [", " lines omitted]")
            || bracket("… [", " similar lines]")
            || bracket("  … +", " more")
            || bracket("", " more files:")
            || bracket("… +", " more changed lines in this file")
            || (line.starts_with("[lockfile ") && line.ends_with(", not shown]"))
            || (line.starts_with("[+") && line.ends_with(" lines in this file, not shown]"))
        {
            return true;
        }
        let tail = |marker: &str, end: &str| {
            line.rfind(marker).is_some_and(|i| {
                let n = &line[i + marker.len()..];
                n.strip_suffix(end)
                    .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            })
        };
        tail("  [×", "]") || tail(" …[+", " chars]")
    })
}

/// Remove ANSI escape sequences (colors, cursor moves, OSC titles/links).
pub fn strip_ansi(s: &str) -> String {
    let Some(first) = s.find('\u{1b}') else {
        return s.to_string();
    };
    let mut out = String::with_capacity(s.len());
    out.push_str(&s[..first]);
    let mut rest = &s[first..];
    // Copy the text between escapes in bulk; the sequences themselves are ASCII.
    while let Some(esc) = rest.find('\u{1b}') {
        out.push_str(&rest[..esc]);
        let tail = &rest[esc + 1..];
        let b = tail.as_bytes();
        let skip = match b.first() {
            // CSI: parameters, then one final byte in '@'..='~'.
            Some(b'[') => b[1..]
                .iter()
                .position(|c| (b'@'..=b'~').contains(c))
                .map_or(b.len(), |k| k + 2),
            // OSC: ends with BEL or ESC '\'.
            Some(b']') => {
                let mut k = 1;
                loop {
                    match b.get(k) {
                        None => break b.len(),
                        Some(0x07) => break k + 1,
                        Some(0x1b) if b.get(k + 1) == Some(&b'\\') => break k + 2,
                        _ => k += 1,
                    }
                }
            }
            // Other escapes (ECMA-48): intermediate bytes 0x20-0x2F, then one final
            // character, e.g. `ESC ( B` (charset) or `ESC =`.
            Some(_) => {
                let n = b.iter().take_while(|c| (0x20..=0x2f).contains(*c)).count();
                tail[n..].chars().next().map_or(n, |c| n + c.len_utf8())
            }
            None => 0,
        };
        rest = &tail[skip..];
    }
    out.push_str(rest);
    out
}

/// Split into lines, resolve carriage-return progress bars to their final state,
/// trim trailing spaces and collapse runs of blank lines.
fn normalize_lines(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in s.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line = line.rsplit('\r').next().unwrap_or(line).trim_end();
        if line.is_empty() && out.last().is_some_and(|l| l.is_empty()) {
            continue;
        }
        out.push(line.to_string());
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

/// Exact repeats become one line with a count (no information lost).
/// Runs of 4+ lines that differ only in numbers (progress, counters) keep
/// their first and last line.
fn collapse_repeats(mut lines: Vec<String>, lossy: &mut bool) -> Vec<String> {
    let n = lines.len();
    // One pass each: shapes, then where each run of same-shape lines ends. Scanning the
    // run again from every line was quadratic on output where nothing collapses
    // (e.g. search results: same shape, but important because of file:line).
    let shapes: Vec<(u64, bool)> = lines.iter().map(|l| shape_key(l)).collect();
    let mut run_end = vec![0; n];
    for i in (0..n).rev() {
        run_end[i] = if i + 1 < n && shapes[i + 1] == shapes[i] {
            run_end[i + 1]
        } else {
            i + 1
        };
    }

    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i < lines.len() {
        let mut j = i + 1;
        while j < lines.len() && lines[j] == lines[i] {
            j += 1;
        }
        let exact = j - i;
        if exact > 1 && !lines[i].is_empty() {
            out.push(format!("{}  [×{exact}]", std::mem::take(&mut lines[i])));
            i = j;
            continue;
        }

        let (_, has_shape_mark) = shapes[i];
        let k = if lines[i].is_empty() {
            i + 1
        } else {
            run_end[i]
        };
        let similar = k - i;
        if similar >= 4 && has_shape_mark && !is_important(&lines[i]) {
            out.push(std::mem::take(&mut lines[i]));
            out.push(format!("… [{} similar lines]", similar - 2));
            out.push(std::mem::take(&mut lines[k - 1]));
            *lossy = true;
            i = k;
            continue;
        }

        out.push(std::mem::take(&mut lines[i]));
        i += 1;
    }
    out
}

/// Hash of the line's "digit shape" (each run of digits counts as one `#`), plus
/// whether that shape contains a `#`. Two lines that differ only in their numbers
/// get the same key. No allocation: this runs on every line.
fn shape_key(line: &str) -> (u64, bool) {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut push = |b: u8| {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    };
    let (mut in_digits, mut has_mark) = (false, false);
    for &b in line.as_bytes() {
        if b.is_ascii_digit() {
            if !in_digits {
                push(b'#');
                has_mark = true;
            }
            in_digits = true;
        } else {
            push(b);
            has_mark |= b == b'#';
            in_digits = false;
        }
    }
    (hash, has_mark)
}

fn shorten_line(line: String, max_chars: usize, lossy: &mut bool) -> String {
    if line.len() <= max_chars {
        return line; // bytes >= chars, so it already fits
    }
    let count = line.chars().count();
    if count <= max_chars || is_important(&line) {
        return line;
    }
    *lossy = true;
    let keep: String = line.chars().take(max_chars).collect();
    format!("{keep} …[+{} chars]", count - max_chars)
}

/// Lines that must survive compression: errors, failures, warnings, file:line locations.
pub fn is_important(line: &str) -> bool {
    const WORDS: [&str; 14] = [
        "error",
        "fail",
        "panic",
        "exception",
        "traceback",
        "fatal",
        "warn", // also covers "warning"
        "denied",
        "not found",
        "cannot",
        "undefined",
        "assert",
        "expected",
        "segmentation fault",
    ];
    if line.as_bytes().contains(&b':') && has_file_location(line) {
        return true;
    }
    // Lowercase once (on the stack for normal lines), then use std's fast substring
    // search for each word. This runs on every line of big outputs.
    let mut stack = [0u8; 512];
    let heap;
    let lower: &[u8] = if line.len() <= stack.len() {
        let buf = &mut stack[..line.len()];
        buf.copy_from_slice(line.as_bytes());
        buf.make_ascii_lowercase();
        buf
    } else {
        heap = line.to_ascii_lowercase().into_bytes();
        &heap
    };
    // ASCII lowercasing keeps UTF-8 valid.
    let lower = std::str::from_utf8(lower).unwrap_or_default();
    WORDS.iter().any(|w| lower.contains(w))
}

/// Matches `name.ext:12` style locations used by compilers, linters and stack traces.
fn has_file_location(line: &str) -> bool {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b':' || i == 0 || i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_digit() {
            continue;
        }
        let before = &line[..i];
        let word = before
            .rsplit(|c: char| c.is_whitespace() || c == '(' || c == '"')
            .next()
            .unwrap_or("");
        if let Some(dot) = word.rfind('.') {
            let ext = &word[dot + 1..];
            if !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
                return true;
            }
        }
    }
    false
}

fn keep_head_tail_important(lines: Vec<String>, limits: Limits, lossy: &mut bool) -> Vec<String> {
    if lines.len() <= limits.max_lines {
        return lines;
    }
    *lossy = true;
    let head_end = limits.head.min(lines.len());
    let tail_start = lines.len().saturating_sub(limits.tail).max(head_end);

    let mut out: Vec<String> = lines[..head_end].to_vec();
    let mut skipped = 0usize;
    let mut kept_important = 0usize;
    for line in &lines[head_end..tail_start] {
        if kept_important < limits.max_important && is_important(line) {
            if skipped > 0 {
                out.push(format!("… [{skipped} lines omitted]"));
                skipped = 0;
            }
            out.push(line.clone());
            kept_important += 1;
        } else {
            skipped += 1;
        }
    }
    if skipped > 0 {
        out.push(format!("… [{skipped} lines omitted]"));
    }
    out.extend_from_slice(&lines[tail_start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamped_log_keeps_error_and_warn_lines() {
        let mut raw = String::new();
        for i in 0..3000 {
            raw.push_str(&format!(
                "2026-11-08T10:{:02}:{:02}.{:03}Z INFO  request id=req-{i:05x} status=200 took={}ms\n",
                i / 60 % 60, i % 60, i % 1000, i % 40 + 2
            ));
            if i == 1200 {
                raw.push_str(
                    "2026-11-08T10:20:00.000Z ERROR database connection lost: host=db-1\n",
                );
            }
            if i == 2000 {
                raw.push_str("2026-11-08T10:33:20.000Z WARN  slow query took 1532ms\n");
            }
        }
        let c = compress(&raw, false, Limits::default());
        assert!(
            c.text.contains("ERROR database connection lost"),
            "{}",
            &c.text[..300.min(c.text.len())]
        );
        assert!(c.text.contains("WARN  slow query"));
        assert!(c.text.contains("req-00000"), "start kept");
    }

    #[test]
    fn strip_ansi_edge_cases() {
        let e = "\u{1b}";
        assert_eq!(strip_ansi("plain é text"), "plain é text");
        assert_eq!(strip_ansi(&format!("a{e}[1;31mred{e}[0m b")), "ared b");
        assert_eq!(
            strip_ansi(&format!("x{e}[12")),
            "x",
            "unterminated CSI drops the rest"
        );
        assert_eq!(
            strip_ansi(&format!("{e}]8;;http://x{e}\\link{e}]8;;{e}\\")),
            "link",
            "OSC with ESC \\"
        );
        assert_eq!(
            strip_ansi(&format!("{e}]0;title{e}x\u{7}after")),
            "after",
            "ESC inside OSC"
        );
        assert_eq!(strip_ansi(&format!("a{e}]0;never ends")), "a");
        assert_eq!(
            strip_ansi(&format!("a{e}(Bb{e}éc")),
            "abc",
            "other escapes drop one char"
        );
        assert_eq!(strip_ansi(&format!("end{e}")), "end");
        assert_eq!(
            strip_ansi(&format!("{e}[é1mok")),
            "ok",
            "multi-byte inside CSI"
        );
    }

    #[test]
    fn error_lines_are_reported_not_merely_mentioned() {
        for yes in [
            "src/a.rs:3: error: boom",
            "error[E0308]: mismatched types",
            "fatal: not a git repository",
            "thread 'main' panicked at src/x.rs:1:1",
            "Traceback (most recent call last):",
            "npm ERR! code ERESOLVE",
            "--- FAIL: TestX (0.00s)",
            "FAILED tests/test_a.py::test_b",
        ] {
            assert!(is_error_line(yes), "{yes}");
        }
        for no in [
            "test result: ok. 60 passed; 0 failed",
            "src/errors.rs:12:fn handle()",
            "warning: unused variable",
            "Failed attempts: 0",
            "error_count = 0",
            "FAILURES_ALLOWED=3",
        ] {
            assert!(!is_error_line(no), "{no}");
        }
    }

    #[test]
    fn errors_with_exit_zero_get_the_failure_budget() {
        let mut raw: String = (0..400).map(|i| format!("step {i} done\n")).collect();
        let hidden: Vec<String> = (0..60)
            .map(|i| format!("module_{i}.py:{i}: error: bad import"))
            .collect();
        raw.push_str(&hidden.join("\n"));
        raw.push('\n');
        raw.push_str(&(0..400).map(|i| format!("tail {i}\n")).collect::<String>());
        let ok = compress(&raw, false, Limits::default());
        assert!(ok.errors_despite_success);
        let kept = hidden
            .iter()
            .filter(|l| ok.text.contains(l.as_str()))
            .count();
        assert!(
            kept > Limits::default().max_important,
            "kept {kept} error lines"
        );
        assert!(!compress("all good\n", false, Limits::default()).errors_despite_success);
        assert!(
            !compress(&raw, true, Limits::default()).errors_despite_success,
            "already a failure"
        );
    }

    #[test]
    fn compressing_twice_changes_nothing() {
        let raw: String = (0..500)
            .map(|i| format!("row {i} value {}\n", i * 7 % 13))
            .chain(["src/app.rs:9: error: boom\n".to_string()])
            .collect();
        let once = compress(&raw, true, Limits::default());
        assert!(once.lossy && is_compressed(&once.text));
        let twice = compress(&once.text, true, Limits::default());
        assert_eq!(twice.text, once.text);
        assert!(!twice.lossy);
        // With `lagom run`'s footer too.
        let footed = format!(
            "{}[lagom: 501 → 90 lines; full output: lagom recall 3fa9c01d2e]\n",
            once.text
        );
        assert_eq!(compress(&footed, false, Limits::default()).text, footed);
    }

    #[test]
    fn plain_output_is_not_mistaken_for_compressed() {
        for line in [
            "… [many lines omitted]",
            "price [×]",
            "see lagom recall docs",
            "[lagom: notes]",
            "ok  [×3] extra",
        ] {
            assert!(!is_compressed(line), "{line}");
        }
        assert!(is_compressed("ok  [×3]"));
        assert!(is_compressed("x …[+12 chars]"));
        assert!(
            is_compressed("a.rs (9)\n  … +6 more"),
            "search summary marker"
        );
        assert!(is_compressed("x (1)\n12 more files:\n  d/: a (1)"));
    }

    #[test]
    fn many_similar_important_lines_stay_linear() {
        // 100k search-result lines: same digit shape, all important (file:line).
        // The old quadratic scan took minutes here; linear takes well under a second.
        let raw: String = (0..100_000)
            .map(|i| format!("src/mod_{i}.rs:{i}:fn handler_{i}()\n"))
            .collect();
        let start = std::time::Instant::now();
        let c = compress(&raw, false, Limits::default());
        assert!(c.lossy);
        assert!(
            c.text.contains("src/mod_0.rs (1)"),
            "search results are grouped by file"
        );
        // The same shape outside search format (stack-trace style) takes the general path.
        let trace: String = (0..100_000)
            .map(|i| format!("  at src/mod_{i}.rs:{i} in handler_{i}\n"))
            .collect();
        let c = compress(&trace, false, Limits::default());
        assert!(c.lossy && c.text.contains("at src/mod_0.rs:0 in handler_0"));
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn short_clean_output_is_unchanged() {
        let raw = "On branch main\nnothing to commit, working tree clean\n";
        let c = compress(raw, false, Limits::default());
        assert_eq!(c.text, raw);
        assert!(!c.lossy);
    }

    #[test]
    fn strips_ansi_colors_and_osc_links() {
        let raw = "\u{1b}[31merror\u{1b}[0m: \u{1b}]8;;http://x\u{7}link\u{1b}]8;;\u{7}";
        assert_eq!(strip_ansi(raw), "error: link");
    }

    #[test]
    fn progress_bar_keeps_final_state() {
        let raw = "Downloading 10%\rDownloading 50%\rDownloading 100%\ndone\n";
        let c = compress(raw, false, Limits::default());
        assert_eq!(c.text, "Downloading 100%\ndone\n");
    }

    #[test]
    fn exact_repeats_get_a_count() {
        let raw = "warn: x\nok\nok\nok\nend";
        let c = compress(raw, false, Limits::default());
        assert_eq!(c.text, "warn: x\nok  [×3]\nend");
        assert!(!c.lossy);
    }

    #[test]
    fn similar_numbered_lines_collapse() {
        let raw: String = (1..=10)
            .map(|i| format!("Compiling crate{i} v0.{i}.0\n"))
            .collect();
        let c = compress(&raw, false, Limits::default());
        assert_eq!(
            c.text,
            "Compiling crate1 v0.1.0\n… [8 similar lines]\nCompiling crate10 v0.10.0\n"
        );
        assert!(c.lossy);
    }

    #[test]
    fn long_output_keeps_head_tail_and_errors() {
        let mut raw = String::new();
        for i in 0..500 {
            raw.push_str(&format!(
                "line {i} is fine but different {}\n",
                "x".repeat(i % 7)
            ));
            if i == 250 {
                raw.push_str("src/main.rs:42: error: something broke\n");
            }
        }
        let c = compress(&raw, false, Limits::default());
        assert!(c.lossy);
        assert!(c.text.contains("src/main.rs:42: error: something broke"));
        assert!(c.text.contains("line 0 is fine"));
        assert!(c.text.contains("line 499 is fine"));
        assert!(c.text.contains("lines omitted"));
        // Exit code 0 with an error line: the failure budget (240 lines) applies.
        assert!(c.errors_despite_success);
        assert!(c.text.lines().count() < 250);
    }

    #[test]
    fn failures_get_a_bigger_budget() {
        let raw: String = (0..200)
            .map(|i| format!("step {i} {}\n", "y".repeat(i % 5)))
            .collect();
        assert!(compress(&raw, false, Limits::default()).lossy);
        assert!(!compress(&raw, true, Limits::default()).lossy);
    }

    #[test]
    fn important_similar_lines_are_not_collapsed() {
        let raw: String = (1..=6)
            .map(|i| format!("error: test {i} failed\n"))
            .collect();
        let c = compress(&raw, true, Limits::default());
        assert_eq!(c.text, raw);
    }

    #[test]
    fn file_locations_are_important() {
        assert!(is_important("  at foo (src/app.ts:12:5)"));
        assert!(is_important("main.go:7: undefined: x"));
        assert!(!is_important("time 12:30 elapsed"));
    }
}
