### Added
- `lagom discover [--days N]` reads Claude Code and Codex logs and lists large command outputs Lagom didn't compress, grouped by program, plus the subcommand for programs Lagom knows (`git log`), so arguments never appear. It shows the reason and what to do, each tool's share of large outputs (shell, Read, MCP …), and files whose same part was read again in one session.
