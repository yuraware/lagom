### Changed
- `lagom experiment start` names the hooks of other tools that add text at session start (in Claude Code, Codex and Cursor, and enabled Claude Code plugins), since they reach both groups and blur the result.
- `~/.lagom/stats.jsonl` moves to `stats.1.jsonl` at 16 MB, replacing the older file, so the stats stay under about 32 MB; `lagom stats` reads both.
