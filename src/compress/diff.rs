//! Unified diffs (`git diff`, `git show`, `git log -p`, `diff -u`).
//!
//! Every `+`/`-` line of normal files is kept (up to a generous cap), with file and hunk
//! headers. Unchanged context is cut to one line around each change. Lockfiles become
//! a one-line summary: their churn is noise to an agent, and `lagom recall` has it.

use super::Limits;

const LOCKFILES: [&str; 11] = [
    "Cargo.lock",
    "poetry.lock",
    "uv.lock",
    "Pipfile.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "go.sum",
    "Gemfile.lock",
    "composer.lock",
    "bun.lock",
];

fn is_lockfile(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    LOCKFILES.contains(&name)
}

/// Path from `diff --git a/x b/y` (the `b/` side), or the last word without prefixes
/// (`--no-prefix`, `diff.mnemonicPrefix`).
fn diff_path(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("diff --git ")?;
    Some(match rest.rfind(" b/") {
        Some(b) => &rest[b + 3..],
        None => rest.rsplit(' ').next().unwrap_or(rest),
    })
}

/// Old and new line counts from a hunk header `@@ -a,b +c,d @@` (a missing count is 1).
fn hunk_len(header: &str) -> Option<(usize, usize)> {
    let mut ranges = header.strip_prefix("@@ -")?.split(' ');
    let count = |r: &str| r.split_once(',').map_or(Some(1), |(_, n)| n.parse().ok());
    let old = count(ranges.next()?)?;
    let new = count(ranges.next()?.strip_prefix('+')?)?;
    Some((old, new))
}

/// A line of a `GIT binary patch` body: `literal N`, `delta N`, blank, or base85 data.
fn is_binary_patch_line(l: &str) -> bool {
    const B85: &str = "!#$%&()*+-;<=>?@^_`{|}~";
    l.is_empty()
        || l.starts_with("literal ")
        || l.starts_with("delta ")
        || l.starts_with(|c: char| c.is_ascii_alphabetic())
            && l.chars()
                .all(|c| c.is_ascii_alphanumeric() || B85.contains(c))
}

fn looks_like_diff(lines: &[String]) -> bool {
    let header = lines
        .iter()
        .any(|l| l.starts_with("diff --git ") || l.starts_with("+++ "));
    header && lines.iter().any(|l| l.starts_with("@@ "))
}

struct File<'a> {
    path: Option<&'a str>,
    header: Vec<&'a str>,
    hunks: Vec<Vec<&'a str>>,
    /// Lines of a `GIT binary patch` body, which are counted, not kept.
    binary: Option<usize>,
}

/// `Some(lines)` for a `git diff` longer than the budget; `None` otherwise, including
/// diffs without `diff --git` headers (`diff -u`), which the general path shortens.
pub fn compress(lines: &[String], limits: Limits) -> Option<Vec<String>> {
    if lines.len() <= limits.max_lines || !looks_like_diff(lines) {
        return None;
    }
    let max_changes = limits.max_lines * 4;
    // Split into text outside files (commit headers) and per-file sections.
    let mut out = Vec::new();
    let mut file: Option<File> = None;
    let mut kept_changes = 0usize;
    let mut saw_file = false;
    // Old and new lines still due in the current hunk, from its `@@` header. A blank
    // context line arrives as "" (trailing spaces are trimmed), so the counts, not the
    // first character, say where a hunk ends.
    let mut left: Option<(usize, usize)> = None;
    let flush = |f: File, out: &mut Vec<String>, kept: &mut usize| {
        let (adds, dels) = f.hunks.iter().flatten().fold((0, 0), |(a, d), l| {
            (
                a + l.starts_with('+') as usize,
                d + l.starts_with('-') as usize,
            )
        });
        out.extend(f.header.iter().map(|s| s.to_string()));
        if let Some(n) = f.binary {
            out.push(format!("[binary patch: {n} lines, not shown]"));
        }
        if f.path.is_some_and(is_lockfile) && !f.hunks.is_empty() {
            out.push(format!(
                "[lockfile {}: {} lines changed (+{adds} -{dels}), not shown]",
                f.path.unwrap_or("?"),
                adds + dels
            ));
            return;
        }
        if *kept >= max_changes && !f.hunks.is_empty() {
            out.push(format!("[+{adds} -{dels} lines in this file, not shown]"));
            return;
        }
        let mut dropped = 0usize;
        for hunk in &f.hunks {
            let is_change = |i: usize| hunk.get(i).is_some_and(|l| l.starts_with(['+', '-']));
            for (i, line) in hunk.iter().enumerate() {
                if i == 0 {
                    out.push(line.to_string()); // @@ header
                } else if is_change(i) {
                    if *kept < max_changes {
                        out.push(line.to_string());
                        *kept += 1;
                    } else {
                        dropped += 1;
                    }
                } else if (is_change(i - 1) || is_change(i + 1)) && *kept < max_changes
                    || line.starts_with('\\')
                {
                    out.push(line.to_string()); // one context line next to a change
                }
            }
        }
        if dropped > 0 {
            out.push(format!("… +{dropped} more changed lines in this file"));
        }
    };
    for line in lines {
        let l = line.as_str();
        if let Some(path) = diff_path(l) {
            if let Some(f) = file.take() {
                flush(f, &mut out, &mut kept_changes);
            }
            saw_file = true;
            left = None;
            file = Some(File {
                path: Some(path),
                header: vec![l],
                hunks: Vec::new(),
                binary: None,
            });
            continue;
        }
        // Inside a hunk: a change, a context line (blank ones included), or `\ No newline`.
        let in_hunk = match left {
            Some((old, new)) => {
                (old > 0 || new > 0) && (l.is_empty() || l.starts_with([' ', '+', '-']))
                    || l.starts_with('\\')
            }
            // A header without counts: fall back to the first character.
            None => l.starts_with([' ', '+', '-', '\\']),
        };
        match file.as_mut() {
            Some(f) if l.starts_with("@@ ") => {
                f.hunks.push(vec![l]);
                left = hunk_len(l);
            }
            Some(f) if !f.hunks.is_empty() && in_hunk => {
                if let Some((old, new)) = left.as_mut() {
                    match l.as_bytes().first() {
                        Some(b'+') => *new = new.saturating_sub(1),
                        Some(b'-') => *old = old.saturating_sub(1),
                        Some(b'\\') => {}
                        _ => (*old, *new) = (old.saturating_sub(1), new.saturating_sub(1)),
                    }
                }
                if let Some(h) = f.hunks.last_mut() {
                    h.push(l);
                }
            }
            Some(f) if f.hunks.is_empty() && f.binary.is_none() && l == "GIT binary patch" => {
                f.binary = Some(1);
            }
            Some(File {
                binary: Some(n), ..
            }) if is_binary_patch_line(l) => *n += 1,
            Some(f) if f.hunks.is_empty() && !l.starts_with("commit ") => {
                // `index …` and `--- a/x` / `+++ b/x` repeat the `diff --git` line; keep
                // them only when they say more (new or deleted file, rename).
                let redundant = l.starts_with("index ")
                    || f.path.is_some_and(|p| {
                        (l.starts_with("+++ ") || l.starts_with("--- ")) && l.ends_with(p)
                    });
                if !redundant {
                    f.header.push(l);
                }
            }
            Some(_) => {
                // Text after a file section (next commit header in `git log -p`).
                if let Some(f) = file.take() {
                    flush(f, &mut out, &mut kept_changes);
                }
                out.push(l.to_string());
            }
            None => out.push(l.to_string()),
        }
    }
    if let Some(f) = file.take() {
        flush(f, &mut out, &mut kept_changes);
    }
    saw_file.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    fn diff(files: usize, changes: usize, context: usize) -> String {
        let mut s = String::new();
        for f in 0..files {
            s.push_str(&format!("diff --git a/src/f{f}.rs b/src/f{f}.rs\nindex 1..2 100644\n--- a/src/f{f}.rs\n+++ b/src/f{f}.rs\n"));
            s.push_str(&format!("@@ -1,{0} +1,{0} @@\n", changes + context));
            for c in 0..context {
                s.push_str(&format!(" context {f} {c}\n"));
            }
            for c in 0..changes {
                s.push_str(&format!("-old {f} {c}\n+new {f} {c}\n"));
            }
        }
        s
    }

    #[test]
    fn keeps_every_change_trims_context_and_collapses_lockfiles() {
        let mut s = diff(5, 20, 30);
        s.push_str("diff --git a/Cargo.lock b/Cargo.lock\n--- a/Cargo.lock\n+++ b/Cargo.lock\n@@ -1,600 +1,600 @@\n");
        for i in 0..300 {
            s.push_str(&format!("-version = \"1.{i}\"\n+version = \"2.{i}\"\n"));
        }
        let out = compress(&lines(&s), Limits::default()).unwrap().join("\n");
        for f in 0..5 {
            for c in 0..20 {
                assert!(
                    out.contains(&format!("+new {f} {c}"))
                        && out.contains(&format!("-old {f} {c}"))
                );
            }
            assert!(out.contains(&format!("diff --git a/src/f{f}.rs b/src/f{f}.rs")));
            assert!(
                !out.contains(&format!("+++ b/src/f{f}.rs")),
                "redundant header dropped"
            );
            assert!(
                out.contains(&format!(" context {f} 29")),
                "context next to a change"
            );
            assert!(
                !out.contains(&format!(" context {f} 0\n")),
                "far context dropped"
            );
        }
        assert!(out.contains("[lockfile Cargo.lock: 600 lines changed (+300 -300), not shown]"));
        assert!(!out.contains("version = \"2.5\""));
    }

    #[test]
    fn keeps_headers_that_carry_information() {
        let mut s = String::from(
            "diff --git a/new.rs b/new.rs\nnew file mode 100644\nindex 0000000..1\n--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1,200 @@\n",
        );
        for i in 0..200 {
            s.push_str(&format!("+line {i}\n"));
        }
        s.push_str("diff --git a/old.rs b/renamed.rs\nsimilarity index 90%\nrename from old.rs\nrename to renamed.rs\n");
        let out = compress(&lines(&s), Limits::default()).unwrap().join("\n");
        for kept in [
            "new file mode 100644",
            "--- /dev/null",
            "rename from old.rs",
            "rename to renamed.rs",
            "+line 199",
        ] {
            assert!(out.contains(kept), "{kept}");
        }
        assert!(!out.contains("index 0000000"));
    }

    #[test]
    fn caps_changes_but_names_every_file() {
        let s = diff(40, 30, 0); // 2,400 change lines
        let out = compress(&lines(&s), Limits::default()).unwrap().join("\n");
        for f in 0..40 {
            assert!(
                out.contains(&format!("diff --git a/src/f{f}.rs")),
                "file {f} named"
            );
        }
        assert!(out.contains("[+30 -30 lines in this file, not shown]"));
        assert!(out.lines().count() < 700);
    }

    /// A real `git diff` (lagom's own history plus a `cargo update` of `Cargo.lock`)
    /// with blank context lines, which arrive as "" and used to end the file's section.
    #[test]
    fn real_diff_with_blank_context_keeps_every_change() {
        let input = include_str!("../../tests/fixtures/compress/git-diff-real/input.txt");
        // As `normalize_lines` hands them over: trailing spaces trimmed.
        let input: Vec<String> = input.lines().map(|l| l.trim_end().to_string()).collect();
        assert!(
            input.iter().any(|l| l.is_empty()),
            "fixture has blank context"
        );
        let out = compress(&input, Limits::default()).unwrap();
        let mut lockfile = false;
        for l in &input {
            if let Some(p) = diff_path(l) {
                lockfile = is_lockfile(p);
            } else if !lockfile
                && l.starts_with(['+', '-'])
                && !l.starts_with("+++ ")
                && !l.starts_with("--- ")
            {
                assert!(out.contains(l), "change line lost: {l}");
            }
        }
        let out = out.join("\n");
        assert!(out.contains("[lockfile Cargo.lock: 26 lines changed (+9 -17), not shown]"));
        assert!(!out.contains("serde_json 1.0.140"), "lockfile body hidden");
        assert!(out.lines().count() < input.len() - 100, "context trimmed");
    }

    #[test]
    fn hunk_counts_end_a_hunk_and_no_prefix_headers_count() {
        // The header promises 5 lines, but a collapsed blank line left 4: the commit line
        // after them is not context.
        let mut s = String::from("diff --git Cargo.lock Cargo.lock\n@@ -1,300 +1,300 @@\n");
        for i in 0..300 {
            s.push_str(&format!("-v{i}\n+w{i}\n"));
        }
        s.push_str("diff --git src/a.rs src/a.rs\n--- src/a.rs\n+++ src/a.rs\n@@ -1,4 +1,5 @@\n a\n\n-b\n+c\n+d\n\\ No newline at end of file\ncommit def456\n");
        let out = compress(&lines(&s), Limits::default()).unwrap().join("\n");
        assert!(
            out.contains("[lockfile Cargo.lock: 600 lines changed"),
            "{out}"
        );
        assert!(
            out.contains("\\ No newline at end of file\ncommit def456"),
            "{out}"
        );
        assert!(!out.contains("+++ src/a.rs"), "redundant header dropped");
    }

    #[test]
    fn binary_patches_collapse_and_plain_diff_u_is_left_to_the_general_path() {
        let mut s = String::from(
            "diff --git a/logo.png b/logo.png\nindex 1..2 100644\nGIT binary patch\nliteral 4096\n",
        );
        for _ in 0..200 {
            s.push_str("zcmV-Z1JbRFQ&!+hd;o|R`n2=>XtYP4mRGN|a$0iw*mTz@Q>sm#\n");
        }
        s.push_str("\nliteral 0\nHcmV?d00001\n\ndiff --git a/a.rs b/a.rs\n@@ -1 +1 @@\n-x\n+y\n");
        let out = compress(&lines(&s), Limits::default()).unwrap().join("\n");
        assert!(
            out.contains("[binary patch: 206 lines, not shown]"),
            "{out}"
        );
        assert!(!out.contains("zcmV") && out.contains("-x\n+y"), "{out}");

        let mut plain =
            String::from("--- old.txt\t2026-01-01\n+++ new.txt\t2026-01-02\n@@ -1,200 +1,200 @@\n");
        for i in 0..200 {
            plain.push_str(&format!("-a{i}\n+b{i}\n"));
        }
        assert!(compress(&lines(&plain), Limits::default()).is_none());
    }

    #[test]
    fn keeps_commit_headers_in_log_patch_and_ignores_non_diffs() {
        let mut s =
            String::from("commit abc123\nAuthor: Dev <dev@example.com>\n\n    fix the parser\n\n");
        s.push_str(&diff(3, 50, 2));
        let out = compress(&lines(&s), Limits::default()).unwrap().join("\n");
        assert!(out.starts_with("commit abc123") && out.contains("    fix the parser"));
        let plain: String = (0..300).map(|i| format!("+ not a diff {i}\n")).collect();
        assert!(compress(&lines(&plain), Limits::default()).is_none());
    }
}
