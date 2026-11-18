### Added
- Experimental, off by default: `{"hints": true}` in your own config adds one line to the code style naming a Rust project's dependencies from `Cargo.toml` ("Prefer std, then existing deps: …. No new crates without asking."). Only valid crate names are shown, at most 15 and 400 characters; a project file can't turn it on.
