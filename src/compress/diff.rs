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

/// Path from `diff --git a/x b/y` (the `b/` side).
fn diff_path(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("diff --git ")?;
    let b = rest.rfind(" b/")?;
    Some(&rest[b + 3..])
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
}

/// `Some(lines)` for a unified diff longer than the budget; `None` otherwise.
pub fn compress(lines: &[String], limits: Limits) -> Option<Vec<String>> {
    if lines.len() <= limits.max_lines || !looks_like_diff(lines) {
        return None;
    }
    let max_changes = limits.max_lines * 4;
    // Split into text outside files (commit headers) and per-file sections.
    let mut out = Vec::new();
    let mut file: Option<File> = None;
    let mut kept_changes = 0usize;
    let flush = |f: File, out: &mut Vec<String>, kept: &mut usize| {
        let (adds, dels) = f.hunks.iter().flatten().fold((0, 0), |(a, d), l| {
            (
                a + l.starts_with('+') as usize,
                d + l.starts_with('-') as usize,
            )
        });
        out.extend(f.header.iter().map(|s| s.to_string()));
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
            file = Some(File {
                path: Some(path),
                header: vec![l],
                hunks: Vec::new(),
            });
            continue;
        }
        match file.as_mut() {
            Some(f) if l.starts_with("@@ ") => f.hunks.push(vec![l]),
            Some(f) if !f.hunks.is_empty() && l.starts_with([' ', '+', '-', '\\']) => {
                if let Some(h) = f.hunks.last_mut() {
                    h.push(l);
                }
            }
            Some(f) if f.hunks.is_empty() && !l.starts_with("commit ") => {
                // `index …` and `--- a/x` / `+++ b/x` repeat the `diff --git` line; keep
                // them only when they say more (new or deleted file, rename).
                let redundant = l.starts_with("index ")
                    || f.path.is_some_and(|p| {
                        l.strip_prefix("+++ b/") == Some(p) || l.strip_prefix("--- a/") == Some(p)
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
    Some(out)
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
