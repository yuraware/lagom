### Changed
- No error line is dropped silently: each one is shown or counted in a notice that names `lagom recall`, and a property test now checks this on the corpus and on generated outputs. `docs/compression.md` describes the search grouping and this rule.

### Fixed
- JSON output keeps failing Kubernetes pods whose reason has no error word (`ImagePullBackOff`, `ErrImagePull`, `OOMKilled`, `Evicted`), and pods in phase `Failed`.
