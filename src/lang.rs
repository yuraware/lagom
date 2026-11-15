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

#[cfg(test)]
mod tests {
    use super::*;

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
