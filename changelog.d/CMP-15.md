### Fixed
- Timestamped logs (`2026-11-08T10:00:00 …`) are no longer mistaken for search results. Before, such a log could be compressed to "0 matches in 0 files", dropping its ERROR lines.
- `WARN` lines count as warnings and survive compression (only "warning" did before).
