//! Which languages a project uses, from marker files: the base for language-aware
//! code styles (Rust and Python first).

use std::collections::VecDeque;
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
/// Folders never searched: build output, dependencies, virtual environments.
const SKIP_DIRS: [&str; 6] = [
    "target",
    "node_modules",
    "venv",
    "build",
    "dist",
    "__pycache__",
];

/// Look for existing tests under `root`, breadth first and with hard limits, so a Cargo
/// workspace (`crates/*/tests`) or a `src/`-layout Python package counts too.
pub fn test_policy(root: &Path, langs: &Languages) -> TestPolicy {
    let (mut tests_dir, mut test_dir, mut cfg_test, mut pytest, mut test_files) =
        (false, false, false, false, false);
    pytest |=
        fs::read_to_string(root.join("pyproject.toml")).is_ok_and(|t| t.contains("[tool.pytest"));
    let (mut queue, mut visited, mut reads) = (VecDeque::from([root.to_path_buf()]), 0, 0);
    'walk: while let Some(dir) = queue.pop_front() {
        // Stop as soon as everything this project could show has been found.
        if tests_dir && (!langs.rust || cfg_test) && (!langs.python || pytest) {
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            visited += 1;
            if visited > MAX_ENTRIES {
                break 'walk;
            }
            let Some(name) = e.file_name().to_str().map(String::from) else {
                continue;
            };
            // `file_type` doesn't follow links, so a link loop can't make this walk forever.
            if e.file_type().is_ok_and(|k| k.is_dir()) {
                match name.as_str() {
                    "tests" => tests_dir = true,
                    "test" => test_dir = true,
                    n if n.starts_with('.') || SKIP_DIRS.contains(&n) => {}
                    _ => queue.push_back(e.path()),
                }
            } else if langs.rust && !cfg_test && name.ends_with(".rs") && reads < MAX_FILE_READS {
                reads += 1;
                cfg_test = fs::read_to_string(e.path()).is_ok_and(|t| t.contains("#[cfg(test)]"));
            } else if langs.python {
                pytest |= name == "conftest.py" || name == "pytest.ini";
                test_files |= name.ends_with(".py")
                    && (name.starts_with("test_") || name.ends_with("_test.py"));
            }
        }
    }
    let mut found = Vec::new();
    if tests_dir || test_dir {
        found.push(if tests_dir { "tests/" } else { "test/" });
    }
    if cfg_test {
        found.push("#[cfg(test)] modules");
    }
    if pytest {
        found.push("pytest");
    } else if test_files {
        found.push("test_*.py files");
    }
    TestPolicy { found }
}

/// One line for the code style: the project has tests, so follow their pattern.
/// `None` when no language or no tests were found. There is no "no tests" line
/// (D-36): a missed workspace or nested package would make it wrong advice in every
/// session, so when the search finds nothing, Lagom says nothing.
pub fn project_note(cwd: &Path) -> Option<String> {
    let langs = detect(cwd);
    let root = langs.root.clone()?;
    let policy = test_policy(&root, &langs);
    (!policy.found.is_empty()).then(|| {
        format!(
            "{} project with tests ({}): add or update tests that follow that pattern when you change behaviour.",
            langs.names().join(" and "),
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

        // No tests found: no line at all (D-36), never "don't add tests".
        let py = scratch("py-none");
        fs::write(py.join("setup.py"), "").unwrap();
        assert_eq!(project_note(&py), None);

        let pyt = scratch("py-pytest");
        fs::write(pyt.join("pyproject.toml"), "[tool.pytest.ini_options]\n").unwrap();
        assert!(project_note(&pyt).unwrap().contains("(pytest)"));

        assert_eq!(project_note(&scratch("nothing")), None);

        // A Cargo workspace: tests live in the members, not at the root.
        let ws = scratch("workspace");
        fs::write(
            ws.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::create_dir_all(ws.join("crates/core/src")).unwrap();
        fs::create_dir_all(ws.join("crates/cli/tests")).unwrap();
        fs::write(ws.join("crates/core/src/lib.rs"), "#[cfg(test)]\nmod t {}").unwrap();
        fs::create_dir_all(ws.join("target/debug")).unwrap();
        let note = project_note(&ws).unwrap();
        assert!(note.contains("(tests/, #[cfg(test)] modules)"), "{note}");

        // A `src/`-layout Python package with tests inside the package.
        let src = scratch("py-src");
        fs::write(src.join("pyproject.toml"), "[project]\nname = \"pkg\"\n").unwrap();
        fs::create_dir_all(src.join("src/pkg")).unwrap();
        fs::write(src.join("src/pkg/test_core.py"), "").unwrap();
        let note = project_note(&src).unwrap();
        assert!(
            note.starts_with("Python project with tests (test_*.py files)"),
            "{note}"
        );

        for d in [rust, py, pyt, ws, src] {
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
