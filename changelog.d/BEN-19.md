### Changed
- `lagom bench compress` reports per family, with the median saving per case and how many outputs got smaller, instead of one byte-weighted total that let the largest logs stand for every command. `--json` adds `families` and `totals.compressed`.
