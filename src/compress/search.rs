//! Search results (`rg -n`, `grep -rn`, `git grep -n`): `path:line:text` per match.
//!
//! Grouped by file: the first matches of the first files in full, every other file
//! still named with its match count. Agents search to learn *which files* match;
//! a cut list of raw lines hides that.

use std::collections::HashMap;

use super::{Limits, is_error_line};

/// Files shown with their matches.
const DETAIL_FILES: usize = 20;
/// Matches shown per detailed file.
const PER_FILE: usize = 3;
/// Characters of match text kept per line.
const MAX_TEXT: usize = 200;
/// File names per summary line.
const NAMES_PER_LINE: usize = 8;

/// `path:12:text` (a match) or `path-12-text` (rg/grep context line).
fn parse(line: &str) -> Option<(&str, &str, &str, bool)> {
    let bytes = line.as_bytes();
    if line.is_empty() || bytes[0].is_ascii_whitespace() {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() {
        let sep = bytes[i];
        if (sep == b':' || sep == b'-') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            let path = &line[..i];
            // A path has a folder or an extension; this rejects timestamps such as
            // `2026-11-08T10:00:00`, which otherwise look like `path-11-…` context lines.
            let looks_like_path = path.contains('/') || path.contains('.');
            if end < bytes.len() && bytes[end] == sep && looks_like_path {
                return Some((path, &line[start..end], &line[end + 1..], sep == b':'));
            }
        }
        i += 1;
    }
    None
}

/// `Some(lines)` if `lines` are search results longer than the budget; `None` to
/// leave them to the general compressor (small output, not search output, or any
/// error line: compiler output looks like `path:line:` too and must never be cut).
pub fn compress(lines: &[String], limits: Limits) -> Option<Vec<String>> {
    if lines.len() <= limits.max_lines {
        return None;
    }
    let non_empty: Vec<&String> = lines
        .iter()
        .filter(|l| !l.is_empty() && *l != "--")
        .collect();
    let parsed: Vec<_> = non_empty.iter().map(|l| parse(l)).collect();
    let hits = parsed.iter().filter(|p| p.is_some()).count();
    let matches = parsed
        .iter()
        .filter(|p| matches!(p, Some((_, _, _, true))))
        .count();
    // Mostly search lines, and mostly real matches (context lines alone never qualify).
    if hits * 10 < non_empty.len() * 9
        || matches * 2 < non_empty.len()
        || lines.iter().any(|l| is_error_line(l))
    {
        return None;
    }

    // Group matches by file, in order of first appearance.
    let mut files: Vec<(&str, Vec<(&str, &str)>)> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut other = Vec::new();
    for (line, p) in non_empty.iter().zip(&parsed) {
        match p {
            Some((path, num, text, true)) => {
                let i = *index.entry(path).or_insert_with(|| {
                    files.push((path, Vec::new()));
                    files.len() - 1
                });
                files[i].1.push((num, text));
            }
            Some((_, _, _, false)) => {} // context lines
            None => other.push(line.as_str()),
        }
    }
    let total: usize = files.iter().map(|(_, m)| m.len()).sum();

    let mut out = vec![format!("{total} matches in {} files", files.len())];
    for (path, matches) in files.iter().take(DETAIL_FILES) {
        out.push(format!("{path} ({})", matches.len()));
        for (num, text) in matches.iter().take(PER_FILE) {
            let text = text.trim();
            let mut shown: String = text.chars().take(MAX_TEXT).collect();
            if shown.len() < text.len() {
                shown.push_str(" …"); // cut: the full line is in the recalled output
            }
            out.push(format!("  {num}: {shown}"));
        }
        if matches.len() > PER_FILE {
            out.push(format!("  … +{} more", matches.len() - PER_FILE));
        }
    }
    let rest = &files[files.len().min(DETAIL_FILES)..];
    if !rest.is_empty() {
        out.push(format!("{} more files:", rest.len()));
        // Group the rest by folder so every file stays named, compactly.
        let mut dirs: Vec<(&str, Vec<String>)> = Vec::new();
        let mut dir_index: HashMap<&str, usize> = HashMap::new();
        for (path, matches) in rest {
            let (dir, name) = path.rsplit_once('/').map_or(("", *path), |(d, n)| (d, n));
            let i = *dir_index.entry(dir).or_insert_with(|| {
                dirs.push((dir, Vec::new()));
                dirs.len() - 1
            });
            dirs[i].1.push(format!("{name} ({})", matches.len()));
        }
        for (dir, names) in dirs {
            let prefix = if dir.is_empty() {
                String::new()
            } else {
                format!("{dir}/: ")
            };
            for chunk in names.chunks(NAMES_PER_LINE) {
                out.push(format!("  {prefix}{}", chunk.join(", ")));
            }
        }
    }
    // Lines that aren't matches (a summary, a warning) come last; say when some are left out.
    const OTHER: usize = 20;
    if other.len() > OTHER {
        out.extend(other[..OTHER].iter().map(|l| l.to_string()));
        out.push(format!(
            "… [{} more lines that aren't matches]",
            other.len() - OTHER
        ));
    } else {
        out.extend(other.into_iter().map(str::to_string));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn parses_matches_and_context() {
        assert_eq!(
            parse("src/a.rs:12:fn x()"),
            Some(("src/a.rs", "12", "fn x()", true))
        );
        assert_eq!(
            parse("src/my-file.rs-12-ctx"),
            Some(("src/my-file.rs", "12", "ctx", false))
        );
        assert_eq!(parse("C:/x/a.rs:3:y"), Some(("C:/x/a.rs", "3", "y", true)));
        assert_eq!(parse("  12: indented"), None);
        assert_eq!(parse("plain text"), None);
        assert_eq!(
            parse("2026-11-08T10:00:00.000Z INFO x"),
            None,
            "timestamp is not a path"
        );
        assert_eq!(
            parse("Makefile:3:all:"),
            None,
            "no folder or extension: not trusted as a path"
        );
    }

    #[test]
    fn every_cut_is_marked() {
        let mut s = String::new();
        for f in 0..30 {
            for m in 0..5 {
                s.push_str(&format!("src/f{f}.rs:{m}:{}\n", "x".repeat(300)));
            }
        }
        for i in 0..15 {
            s.push_str(&format!("note {i}: not a match\n"));
        }
        let out = compress(&lines(&s), Limits::default()).unwrap().join("\n");
        assert!(
            out.contains(&format!("{} …", "x".repeat(MAX_TEXT))),
            "long match text cut with a marker"
        );
        assert!(out.contains("note 14: not a match"));

        let mut s = String::new();
        for f in 0..400 {
            s.push_str(&format!("src/g{f}.rs:1:hit\n"));
        }
        for i in 0..30 {
            s.push_str(&format!("note {i}: not a match\n"));
        }
        let out = compress(&lines(&s), Limits::default()).unwrap().join("\n");
        assert!(
            out.contains("… [10 more lines that aren't matches]"),
            "{out}"
        );
    }

    #[test]
    fn groups_by_file_and_names_every_file() {
        let mut s = String::new();
        for f in 0..50 {
            for m in 0..6 {
                s.push_str(&format!(
                    "pkg_{}/mod_{f}.py:{}:def h_{f}_{m}():\n",
                    f % 4,
                    m * 3 + 1
                ));
            }
        }
        let out = compress(&lines(&s), Limits::default()).unwrap().join("\n");
        assert!(out.starts_with("300 matches in 50 files"));
        for f in 0..50 {
            assert!(
                out.contains(&format!("mod_{f}.py (6)")),
                "file {f} not named:\n{out}"
            );
        }
        assert!(out.contains("  … +3 more"));
        assert!(out.len() < s.len() / 3, "{} vs {}", out.len(), s.len());
    }

    #[test]
    fn leaves_errors_small_and_non_search_output_alone() {
        let errors: String = (0..200)
            .map(|i| format!("src/a.c:{i}:5: error: bad thing {i}\n"))
            .collect();
        assert!(compress(&lines(&errors), Limits::default()).is_none());
        let small: String = (0..10).map(|i| format!("a.rs:{i}:x\n")).collect();
        assert!(compress(&lines(&small), Limits::default()).is_none());
        let prose: String = (0..300).map(|i| format!("line {i} of a log\n")).collect();
        assert!(compress(&lines(&prose), Limits::default()).is_none());
        // Timestamped log lines (the shape that once became "0 matches in 0 files").
        let log: String = (0..3000)
            .map(|i| {
                format!(
                    "2026-11-08T10:{:02}:{:02}.000Z INFO  request id=req-{i:05x} status=200\n",
                    i / 60 % 60,
                    i % 60
                )
            })
            .collect();
        assert!(compress(&lines(&log), Limits::default()).is_none());
        let context_only: String = (0..300).map(|i| format!("src/a.rs-{i}-ctx\n")).collect();
        assert!(compress(&lines(&context_only), Limits::default()).is_none());
    }
}
