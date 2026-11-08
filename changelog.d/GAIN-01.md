### Added
- Groundwork for real token usage: Lagom can read Claude Code session transcripts (`$CLAUDE_CONFIG_DIR/projects`, default `~/.claude/projects`) and total input, output and cache tokens per session, counting each streamed message once and keeping subagent usage separate. Nothing reads them yet; `lagom stats --usage` builds on this.
