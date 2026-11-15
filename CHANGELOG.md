# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-07

First preview.

### Added
- `lagom install` / `uninstall` / `status` for Claude Code, Codex and Cursor:
  - hooks that are updated in place, plus a backup and atomic writes;
  - skills marked as Lagom's own;
  - refuses settings files that are not valid JSON.
- Styles:
  - built-in `terse` (say less) and `lean-code` (write less code);
  - Markdown style files per user and per project;
  - `lagom style list|show|use`, and the `LAGOM_STYLES` override.
- Session-start injection for all three agents, and subagent-start injection for Claude Code.
- `lagom run` compressor:
  - strips ANSI codes, resolves progress bars, counts repeats and collapses numbered runs;
  - keeps the head, tail and all error / warning / `file:line` lines, with double the budget for failed commands;
  - keeps exit codes.
- `lagom compress` (stdin), `lagom recall <id>` (originals of compressed output), `lagom stats` (estimated savings minus style overhead).
- Pre-tool command rewrite:
  - simple known commands only;
  - never widens permissions: Claude Code and Cursor need the user's own allow rule, and deny or ask rules always win.
- Skills: `lagom`, `lagom-run`, `lagom-review`, `lagom-commit`, `lagom-stats`.
- Hooks use a stable binary path (for example `/opt/homebrew/bin/lagom`, never the versioned Homebrew Cellar path), so upgrades don't break them.
- `lgm`, a short alias binary for `lagom`. Hooks always point to `lagom`, and hooks written under either name are recognized as Lagom's own.
