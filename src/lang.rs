//! Which languages a project uses, from marker files: the base for language-aware
//! code styles (Rust and Python first).

use std::fs;
use std::path::{Path, PathBuf};

/// Folders to look at, from the current one up.
const MAX_LEVELS: usize = 8;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Languages {
    pub rust: bool,
    pub python: bool,
    /// Where the markers were found (nearest folder with any marker).
    pub root: Option<PathBuf>,
}

impl Languages {
    pub fn names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.rust {
            v.push("Rust");
        }
        if self.python {
            v.push("Python");
        }
        v
    }
}

fn is_python_marker(name: &str) -> bool {
    matches!(
        name,
        "pyproject.toml" | "setup.py" | "setup.cfg" | "Pipfile"
    ) || (name.starts_with("requirements") && name.ends_with(".txt"))
}

/// Walk up from `cwd`: one directory listing per level, stopping at the repository
/// root (a folder with `.git`), the home folder, or after [`MAX_LEVELS`] folders.
/// Markers from all visited levels count (a crate inside a Python monorepo is both).
pub fn detect(cwd: &Path) -> Languages {
    let home = crate::store::home_dir();
    let mut found = Languages::default();
    for dir in cwd.ancestors().take(MAX_LEVELS) {
        if dir == home {
            break;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut repo_root = false;
        let mut any = false;
        for e in entries.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            match name {
                ".git" => repo_root = true,
                "Cargo.toml" => {
                    found.rust = true;
                    any = true;
                }
                n if is_python_marker(n) => {
                    found.python = true;
                    any = true;
                }
                _ => {}
            }
        }
        if any && found.root.is_none() {
            found.root = Some(dir.to_path_buf());
        }
        if repo_root {
            break;
        }
    }
    found
}

/// Whether the project already has tests, and roughly what kind.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TestPolicy {
    /// Short description of what was found, e.g. "tests/ and #[cfg(test)] modules".
    pub found: Vec<&'static str>,
}

/// Rust source files read at most, looking for `#[cfg(test)]`.
const MAX_FILE_READS: usize = 20;
/// Directory entries visited at most.
const MAX_ENTRIES: usize = 400;

/// Look for existing tests under `root`, cheaply and with hard limits.
pub fn test_policy(root: &Path, langs: &Languages) -> TestPolicy {
    let mut found = Vec::new();
    let entries: Vec<String> = fs::read_dir(root)
        .map(|r| {
            r.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .take(MAX_ENTRIES)
                .collect()
        })
        .unwrap_or_default();
    let has = |n: &str| entries.iter().any(|e| e == n);
    if has("tests") || has("test") {
        found.push(if has("tests") { "tests/" } else { "test/" });
    }
    if langs.rust && rust_unit_tests(&root.join("src")) {
        found.push("#[cfg(test)] modules");
    }
    if langs.python {
        let pytest = has("conftest.py")
            || has("pytest.ini")
            || fs::read_to_string(root.join("pyproject.toml"))
                .is_ok_and(|t| t.contains("[tool.pytest"));
        let test_files = entries
            .iter()
            .any(|e| e.ends_with(".py") && (e.starts_with("test_") || e.ends_with("_test.py")));
        if pytest {
            found.push("pytest");
        } else if test_files {
            found.push("test_*.py files");
        }
    }
    TestPolicy { found }
}

/// Breadth-first over `src`, reading at most [`MAX_FILE_READS`] `.rs` files.
fn rust_unit_tests(src: &Path) -> bool {
    let (mut queue, mut reads, mut visited) = (vec![src.to_path_buf()], 0, 0);
    while let Some(dir) = queue.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            visited += 1;
            if visited > MAX_ENTRIES || reads >= MAX_FILE_READS {
                return false;
            }
            let p = e.path();
            if p.is_dir() {
                queue.insert(0, p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                reads += 1;
                if fs::read_to_string(&p).is_ok_and(|t| t.contains("#[cfg(test)]")) {
                    return true;
                }
            }
        }
    }
    false
}

/// One line for the code style: follow the project's test pattern, or don't add
/// a test framework the project doesn't use (ponytail #1213). `None` when no
/// language was detected.
pub fn project_note(cwd: &Path) -> Option<String> {
    let langs = detect(cwd);
    let names = langs.names();
    let root = langs.root.clone()?;
    let policy = test_policy(&root, &langs);
    let what = names.join(" and ");
    Some(if policy.found.is_empty() {
        format!(
            "{what} project with no tests found: don't add a test framework or test files unless asked."
        )
    } else {
        format!(
            "{what} project with tests ({}): add or update tests that follow that pattern when you change behaviour.",
            policy.found.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_follows_the_project() {
        let rust = scratch("rust-tests");
        fs::create_dir_all(rust.join("src/a")).unwrap();
        fs::write(rust.join("Cargo.toml"), "").unwrap();
        fs::write(rust.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(rust.join("src/a/b.rs"), "#[cfg(test)]\nmod tests {}").unwrap();
        fs::create_dir_all(rust.join("tests")).unwrap();
        let note = project_note(&rust).unwrap();
        assert!(
            note.starts_with("Rust project with tests (tests/, #[cfg(test)] modules)"),
            "{note}"
        );

        let py = scratch("py-none");
        fs::write(py.join("setup.py"), "").unwrap();
        let note = project_note(&py).unwrap();
        assert!(
            note.contains("no tests found: don't add a test framework"),
            "{note}"
        );

        let pyt = scratch("py-pytest");
        fs::write(pyt.join("pyproject.toml"), "[tool.pytest.ini_options]\n").unwrap();
        assert!(project_note(&pyt).unwrap().contains("(pytest)"));

        assert_eq!(project_note(&scratch("nothing")), None);
        for d in [rust, py, pyt] {
            let _ = fs::remove_dir_all(d);
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lagom-lang-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn detects_rust_python_and_both() {
        let root = scratch("both");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("pyproject.toml"), "").unwrap();
        fs::create_dir_all(root.join("crates/core/src")).unwrap();
        fs::write(root.join("crates/core/Cargo.toml"), "").unwrap();

        let deep = detect(&root.join("crates/core/src"));
        assert!(deep.rust && deep.python, "{deep:?}");
        assert_eq!(deep.root, Some(root.join("crates/core")));
        assert_eq!(deep.names(), ["Rust", "Python"]);

        let top = detect(&root);
        assert!(!top.rust && top.python);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn requirements_files_and_repo_boundary() {
        let root = scratch("req");
        fs::write(root.join("Cargo.toml"), "").unwrap(); // outside the repo below
        let repo = root.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join("requirements-dev.txt"), "").unwrap();
        let l = detect(&repo);
        assert!(l.python && !l.rust, "stops at the repository root: {l:?}");
        assert_eq!(detect(&scratch("empty")), Languages::default());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detection_is_fast() {
        let start = std::time::Instant::now();
        for _ in 0..100 {
            detect(Path::new(env!("CARGO_MANIFEST_DIR")));
        }
        assert!(
            start.elapsed() < std::time::Duration::from_millis(100),
            "{:?}",
            start.elapsed()
        );
        assert!(
            detect(Path::new(env!("CARGO_MANIFEST_DIR"))).rust,
            "this repo is Rust"
        );
    }
}
