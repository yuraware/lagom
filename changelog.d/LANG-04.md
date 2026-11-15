### Added
- With the `lean-code` style, Rust and Python projects get one line about their tests: follow the existing pattern (`tests/`, `#[cfg(test)]`, pytest, `test_*.py`), found also in Cargo workspace members and `src/`-layout packages. Projects where no tests are found get no line.
