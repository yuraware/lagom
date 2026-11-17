### Added
- `LAGOM_DEBUG=1` explains what Lagom decided, such as why a hook left a command alone or ignored its input: one line per decision on stderr and in `~/.lagom/debug.log` (1 MB, then rotated once). It holds sizes and decisions, never commands or payloads.
