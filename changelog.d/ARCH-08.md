### Changed
- Library: `lagom::compress::compress` no longer takes a `Limits` argument (every caller passed the default), and `store::record` is gone (no caller). `stats.jsonl` is parsed in one place, which skips events from a newer Lagom everywhere.
