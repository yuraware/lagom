### Added
- `lagom bench compress --compare BASELINE --max-growth PCT` fails when total compressed output grows more than `PCT` percent against a baseline. CI uses it, together with the `must_keep` check, as a fidelity gate on every pull request.
