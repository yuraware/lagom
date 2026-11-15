### Fixed
- Timestamped logs (`2026-11-08T10:00:00 …`) are no longer mistaken for search results. Before, such a log could be compressed to "0 matches in 0 files", dropping its ERROR lines.
- `WARN` lines count as warnings and survive compression (only "warning" did before).
- When the middle of a long output is cut, error lines (and log lines at ERROR, FATAL, CRITICAL or PANIC) are kept before warnings, and any error lines that still don't fit are counted in a note pointing to `lagom recall <id> --grep error`.
