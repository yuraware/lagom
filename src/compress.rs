//! Deterministic command-output compressor.
//!
//! Rules: never hide failures, keep exact text of what survives, and report
//! anything dropped so the caller can store the original for `lagom recall`.

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
}

/// Compress raw command output. `failed` means a non-zero exit code.
pub fn compress(raw: &str, failed: bool, limits: Limits) -> Compressed {
    if is_compressed(raw) {
        // Compressing twice must change nothing (and must not drop more lines).
        return Compressed {
            text: raw.to_string(),
            lossy: false,
        };
    }
    let limits = if failed { limits.for_failure() } else { limits };
    let mut lossy = false;

    let cleaned = strip_ansi(raw);
    let lines = normalize_lines(&cleaned);
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
    Compressed { text, lossy }
}

/// True if `text` already went through Lagom: it carries one of the markers the
/// compressor writes (`… [N lines omitted]`, `… [N similar lines]`, `  [×N]`,
/// `…[+N chars]`) or `lagom run`'s recall footer.
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
        if bracket("… [", " lines omitted]") || bracket("… [", " similar lines]") {
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
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                // CSI: parameters, then one final byte in '@'..='~'.
                for n in chars.by_ref() {
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                // OSC: ends with BEL or ESC '\'.
                while let Some(n) = chars.next() {
                    if n == '\u{7}' {
                        break;
                    }
                    if n == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
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
fn collapse_repeats(lines: Vec<String>, lossy: &mut bool) -> Vec<String> {
    let n = lines.len();
    // One pass each: shapes, then where each run of same-shape lines ends. Scanning the
    // run again from every line was quadratic on output where nothing collapses
    // (e.g. search results: same shape, but important because of file:line).
    let shapes: Vec<String> = lines.iter().map(|l| digit_shape(l)).collect();
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
            out.push(format!("{}  [×{exact}]", lines[i]));
            i = j;
            continue;
        }

        let shape = &shapes[i];
        let k = if lines[i].is_empty() {
            i + 1
        } else {
            run_end[i]
        };
        let similar = k - i;
        if similar >= 4 && shape.contains('#') && !is_important(&lines[i]) {
            out.push(lines[i].clone());
            out.push(format!("… [{} similar lines]", similar - 2));
            out.push(lines[k - 1].clone());
            *lossy = true;
            i = k;
            continue;
        }

        out.push(lines[i].clone());
        i += 1;
    }
    out
}

fn digit_shape(line: &str) -> String {
    let mut shape = String::with_capacity(line.len());
    let mut in_digits = false;
    for c in line.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                shape.push('#');
            }
            in_digits = true;
        } else {
            shape.push(c);
            in_digits = false;
        }
    }
    shape
}

fn shorten_line(line: String, max_chars: usize, lossy: &mut bool) -> String {
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
        "warning",
        "denied",
        "not found",
        "cannot",
        "undefined",
        "assert",
        "expected",
        "segmentation fault",
    ];
    let lower = line.to_ascii_lowercase();
    WORDS.iter().any(|w| lower.contains(w)) || has_file_location(line)
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
        assert!(c.text.contains("src/mod_0.rs:0:"));
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
        assert!(c.text.lines().count() < 130);
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
