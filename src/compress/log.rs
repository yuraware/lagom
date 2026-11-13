//! Timestamped logs (app logs, access logs, `docker logs`, `kubectl logs`, syslog).
//!
//! Each line is reduced to a template where every word containing a digit (times,
//! ids, IPs, sizes, hex) becomes `<n>`. A template seen [`MIN_REPEATS`] times or more
//! keeps its first and last real line plus one `[×N lines like this: …]` marker.
//! Problem lines (ERROR, WARN, FATAL …, HTTP 5xx) are never collapsed.

use std::collections::HashMap;

use super::{Limits, is_important};

const MIN_REPEATS: usize = 3;
/// Bytes at the start of a line where a time must appear.
const TIME_WITHIN: usize = 48;

/// `hh:mm` (digits on both sides of a colon) near the start of the line.
fn has_time(line: &str) -> bool {
    let b = line.as_bytes();
    let end = b.len().min(TIME_WITHIN);
    (1..end.saturating_sub(2)).any(|i| {
        b[i] == b':'
            && b[i - 1].is_ascii_digit()
            && b[i + 1].is_ascii_digit()
            && b[i + 2].is_ascii_digit()
    })
}

/// An `ls -l` entry (`-rw-r--r--  1 dev staff 508 Nov  8 10:20 file0.rs`): it has a
/// time, but it is a listing, and collapsing it would hide the names.
fn is_listing_entry(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() > 10 && b"-dlcbps".contains(&b[0]) && b[1..10].iter().all(|c| b"rwxsStT-".contains(c))
}

fn looks_like_log(lines: &[String]) -> bool {
    let non_empty = lines.iter().filter(|l| !l.is_empty()).count();
    let timed = lines
        .iter()
        .filter(|l| has_time(l) && !is_listing_entry(l))
        .count();
    non_empty > 0 && timed * 10 >= non_empty * 8
}

/// HTTP status 5xx: access-log `"GET /x HTTP/1.1" 503 …`, or `status=503` / `"status":503`.
pub fn is_server_error(line: &str) -> bool {
    let after = |marker: &str| {
        line.match_indices(marker).any(|(i, m)| {
            let rest = line[i + m.len()..].trim_start_matches([' ', '"', ':', '=']);
            let code = rest.as_bytes();
            code.len() >= 3
                && code[0] == b'5'
                && code[1].is_ascii_digit()
                && code[2].is_ascii_digit()
                && code.get(3).is_none_or(|c| !c.is_ascii_digit())
        })
    };
    after("HTTP/1.1\"")
        || after("HTTP/1.0\"")
        || after("HTTP/2\"")
        || after("HTTP/2.0\"")
        || after("status=")
        || after("\"status\":")
        || after("status: ")
}

/// A level word anywhere as a whole word: ERROR, WARN, FATAL, CRIT, PANIC, SEVERE …
fn has_problem_level(line: &str) -> bool {
    line.split(|c: char| !c.is_ascii_alphabetic()).any(|w| {
        matches!(
            w.to_ascii_uppercase().as_str(),
            "ERROR"
                | "ERR"
                | "WARN"
                | "WARNING"
                | "FATAL"
                | "CRIT"
                | "CRITICAL"
                | "PANIC"
                | "SEVERE"
                | "EMERG"
                | "ALERT"
        )
    })
}

pub fn is_problem(line: &str) -> bool {
    has_problem_level(line) || is_server_error(line) || is_important(line)
}

/// Words that contain a digit become `<n>`; everything else stays.
pub fn template(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.bytes().any(|b| b.is_ascii_digit()) {
            out.push_str("<n>");
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in line.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Collapse repeated templates in a long timestamped log. Other output is returned
/// unchanged.
pub fn collapse(lines: Vec<String>, limits: Limits, lossy: &mut bool) -> Vec<String> {
    if lines.len() <= limits.max_lines || !looks_like_log(&lines) {
        return lines;
    }
    let problem: Vec<bool> = lines.iter().map(|l| is_problem(l)).collect();
    let templates: Vec<Option<String>> = lines
        .iter()
        .zip(&problem)
        .map(|(l, p)| (!p && !l.is_empty()).then(|| template(l)))
        .collect();
    let mut count: HashMap<&str, usize> = HashMap::new();
    let mut last: HashMap<&str, usize> = HashMap::new();
    for (i, t) in templates.iter().enumerate() {
        if let Some(t) = t {
            *count.entry(t).or_default() += 1;
            last.insert(t, i);
        }
    }
    let mut seen: HashMap<&str, ()> = HashMap::new();
    let mut out = Vec::new();
    for (i, line) in lines.into_iter().enumerate() {
        let Some(t) = templates[i].as_deref() else {
            out.push(line); // problem line or blank: always kept
            continue;
        };
        let n = count[t];
        if n < MIN_REPEATS {
            out.push(line);
        } else if seen.insert(t, ()).is_none() {
            out.push(line);
            out.push(format!("[×{n} lines like this: {}]", truncate(t, 160)));
        } else if last[t] == i {
            out.push(line);
        } else {
            *lossy = true;
        }
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[String]) -> Vec<String> {
        v.to_vec()
    }

    #[test]
    fn templates_hide_numbers_and_ids() {
        assert_eq!(
            template("2026-11-08T10:00:01.123Z INFO request id=req-00a1f took=12ms"),
            "<n>-<n>-<n>:<n>:<n>.<n> INFO request id=req-<n> took=<n>"
        );
        assert_eq!(
            template("10.0.1.2 - - [08/Nov/2026:10:00:01]"),
            "<n>.<n>.<n>.<n> - - [<n>/Nov/<n>:<n>:<n>:<n>]"
        );
    }

    /// Every 5xx and CRIT line survives the line budget or is counted in its notice.
    #[test]
    fn server_errors_and_crit_lines_are_shown_or_counted() {
        let log: String = (0..3000)
            .map(|i| {
                let time = format!("2026-11-08T10:{:02}:{:02}Z", i / 60 % 60, i % 60);
                match i {
                    _ if i % 30 == 7 => format!("{time} INFO  request id=r{i} status=503\n"),
                    _ if i % 50 == 9 => format!("{time} CRIT  disk {i} almost full\n"),
                    _ => format!("{time} INFO  request id=r{i} status=200\n"),
                }
            })
            .collect();
        let problem = |l: &str| l.contains("status=503") || l.contains(" CRIT ");
        let total = log.lines().filter(|l| problem(l)).count();
        let out = super::super::compress(&log, false, Limits::default()).text;
        let shown = out.lines().filter(|l| problem(l)).count();
        let counted: usize = out
            .lines()
            .find_map(|l| {
                l.strip_prefix("… [")?
                    .split_once(" more error lines left out")
            })
            .and_then(|(n, _)| n.parse().ok())
            .unwrap_or(0);
        assert!(shown > 0 && counted > 0, "{out}");
        assert_eq!(shown + counted, total, "{out}");
    }

    #[test]
    fn ls_listings_are_not_logs() {
        let ls: Vec<String> = (0..200)
            .map(|i| {
                format!(
                    "-rw-r--r--  1 dev  staff  {} Nov  8 10:20 file{i}.rs",
                    500 + i
                )
            })
            .collect();
        let mut lossy = false;
        assert_eq!(collapse(lines(&ls), Limits::default(), &mut lossy), ls);
        assert!(!lossy);
    }

    #[test]
    fn access_log_keeps_5xx_and_collapses_the_rest() {
        let mut log: Vec<String> = (0..2500)
            .map(|i| {
                let code = if i == 700 || i == 1800 { 500 } else { 200 };
                format!("10.0.{}.{} - - [08/Nov/2026:10:{:02}:{:02} +0000] \"GET /static/app.{}.js HTTP/1.1\" {code} {} \"-\" \"Mozilla/5.0\"",
                        i % 5, i % 250, i / 60 % 60, i % 60, i % 3, 1000 + i % 300)
            })
            .collect();
        log.push(
            "10.0.0.1 - - [08/Nov/2026:11:00:00 +0000] \"GET /api HTTP/1.1\" 503 12 \"-\" \"curl\""
                .into(),
        );
        let mut lossy = false;
        let out = collapse(lines(&log), Limits::default(), &mut lossy);
        assert!(lossy);
        assert_eq!(out.iter().filter(|l| l.contains("\" 500 ")).count(), 2);
        assert!(out.iter().any(|l| l.contains("\" 503 ")));
        assert!(out.len() < 20, "{} lines", out.len());
        assert!(
            out.iter()
                .any(|l| l.starts_with("[×") && l.contains("GET /static/app.<n>.js"))
        );
    }

    #[test]
    fn app_log_keeps_every_problem_line() {
        let mut log = Vec::new();
        for i in 0..3000 {
            log.push(format!(
                "2026-11-08T10:{:02}:{:02}Z INFO  request id=req-{i:05x} status=200 took={}ms",
                i / 60 % 60,
                i % 60,
                i % 40
            ));
            if i % 700 == 0 {
                log.push(format!(
                    "2026-11-08T10:{:02}:00Z ERROR db lost host=db-{i}",
                    i / 60 % 60
                ));
                log.push(format!(
                    "2026-11-08T10:{:02}:00Z warn slow query {i}ms",
                    i / 60 % 60
                ));
            }
        }
        let mut lossy = false;
        let out = collapse(log, Limits::default(), &mut lossy);
        assert_eq!(
            out.iter().filter(|l| l.contains("ERROR db lost")).count(),
            5
        );
        assert_eq!(
            out.iter().filter(|l| l.contains("warn slow query")).count(),
            5
        );
        assert!(out.len() < 30);
    }

    #[test]
    fn non_logs_and_short_logs_are_untouched() {
        let prose: Vec<String> = (0..300)
            .map(|i| format!("line {i} without a time"))
            .collect();
        let mut lossy = false;
        assert_eq!(
            collapse(prose.clone(), Limits::default(), &mut lossy),
            prose
        );
        let short: Vec<String> = (0..10).map(|i| format!("10:00:{i:02} INFO tick")).collect();
        assert_eq!(
            collapse(short.clone(), Limits::default(), &mut lossy),
            short
        );
        assert!(!lossy);
        assert!(
            is_server_error("status=503 path=/x")
                && !is_server_error("status=200")
                && !is_server_error("status=5000")
        );
        assert!(
            has_problem_level("x [Warning] y") && !has_problem_level("errorless warnings_count")
        );
    }
}
