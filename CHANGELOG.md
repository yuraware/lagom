# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-11-17

### Added
- `lagom install` / `uninstall` accept `--dry-run` and `--yes`, ask once before installing in a terminal, and end with a one-line summary per agent.
- `lagom uninstall --all --purge` removes Lagom from every agent and deletes `~/.lagom` and `~/.config/lagom` (asks first; `--yes` in scripts), then prints the command that removes the binary. It deletes only folders Lagom created (a `.lagom-dir` file marks them), so a `LAGOM_HOME` pointing elsewhere is refused.

### Fixed
- The compressor runs in linear time; outputs with many similar lines (search results) no longer slow it down.
- `lagom install` keeps a symlinked settings file a symlink and writes its target.
- `lagom install` stops on Windows with a clear message instead of writing hooks that need a POSIX shell. `lagom uninstall` still works there.
- Cursor hooks read the project folder from `workspace_roots` when the payload has no `cwd`.

### Security
- Claude Code permission rules are matched exactly as Claude Code does: a rule without `*` matches only that exact command (it used to also match the command with extra arguments), and `Bash(x:*)` needs a word boundary (it used to match `xyz`).
- Allow rules in a project's committed `.claude/settings.json`, or in a `.claude/settings.local.json` tracked by git, no longer let Lagom rewrite commands: Claude Code applies them only after workspace trust.
- Managed settings (`managed-settings.json` and `managed-settings.d/`) are read; their deny rules always win, and `allowManagedPermissionRulesOnly` is honored. Lagom doesn't rewrite when a policy it can't read may apply (macOS MDM profile, Windows).
- Project allow rules count only from `.claude/settings.local.json` in the folder the session started in or its repository root, and only when git confirms the file is untracked. A managed settings file that can't be read stops the rewrite.
- Cursor: deny rules in a project's `.cursor/cli.json` stop the rewrite.
- `~/.lagom` is private (folders 0700, files 0600), and `lagom install` keeps the permissions of the settings files it edits.
- Hooks ignore input over 8 MB, JSON that is not an object, commands over 64 KB and a `cwd` over 4 KB, staying silent instead of failing.

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
