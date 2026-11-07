# lagom

Fewer tokens, simpler answers for coding agents. *Lagom* is Swedish for "just the right amount".

One small Rust binary (~0.5 MB, no runtime, no daemon) that plugs into **Claude Code**, **Codex** and **Cursor**:

- **Styles.** Rules injected at session start that make the agent say less (`terse`) and write less code (`lean-code`). Styles are plain Markdown files, so you can edit them or add your own, per user or per project.
- **Compressor.** Long command output (tests, builds, `git log`, searches) is shortened before the agent reads it: repeats collapsed, progress bars resolved, start and end kept, and **every error, warning and `file:line` line kept**. Exit codes are unchanged. Anything dropped stays available through `lagom recall <id>`.
- **Skills.** `lagom` (switch styles), `lagom-run` (compress output), `lagom-review` (one line per finding), `lagom-commit` (short Conventional Commits), `lagom-stats` (savings).

Status: **0.1.0, first public preview.** Tested on macOS; Linux should work; Windows is not supported yet.

## Install

```sh
cargo install --git https://github.com/yuraware/lagom   # or: cargo install --path .
lagom install            # every agent found: claude, codex, cursor
lagom install claude     # or pick agents
lagom install --dry-run  # show what would change, change nothing
```

Then start a new agent session.

`lgm` is a short alias: `lgm <command>` does exactly what `lagom <command>` does. (If you already have the unrelated `lgm` crate installed, use `cargo install lagom --bin lagom`.)

- **Codex:** hooks stay inactive until you trust them. Open Codex, run `/hooks`, and approve the two `lagom hook codex …` entries.
- **Undo:** `lagom uninstall` removes only Lagom's hooks and skills. `lagom uninstall --all --purge` also deletes Lagom's data and config and tells you how to remove the binary. Before its first change to a settings file, Lagom saves a copy next to it as `<file>.lagom.bak`.

| Agent | Hooks file | Skills | Session start | Subagents | Command rewrite |
|---|---|---|---|---|---|
| Claude Code | `~/.claude/settings.json` | `~/.claude/skills/` | ✓ | ✓ | ✓ |
| Codex | `~/.codex/hooks.json` | `~/.agents/skills/` | ✓ | – | ✓ (after `/hooks` trust) |
| Cursor | `~/.cursor/hooks.json` | `~/.cursor/skills/` | ✓ | – | ✓ |

## How command compression works

A pre-tool hook turns a simple command such as `git log --stat` into `lagom run -- git log --stat`:

- **What gets rewritten:** only a single known program (git, cargo, npm, pytest, go, rg, docker, …) with no pipes, `&&`, redirects or subshells.
- **What is left alone:** dev servers, watch modes and `-f`/`--follow` commands.
- **Permissions never widen:**
  - **Claude Code and Cursor:** a command is rewritten (and allowed) only if your own allow rule already matches it, for example `Bash(git log:*)`. Deny and ask rules always win. In Claude's `bypassPermissions` mode, commands are rewritten without granting anything.
  - **Codex:** Codex re-checks approval and its sandbox on the rewritten command. Commands you approved earlier by prefix may be asked about once more.

You can also compress by hand:

```sh
lagom run -- cargo test        # compressed output, same exit code
some-command | lagom compress  # compress any text
lagom recall 3fa9c01d2e        # full original of a compressed result
lagom stats                    # estimated savings vs. style overhead
```

On this repo's sample projects: `git log --stat -3` 115,863 → 5,462 bytes, `rg -n fn` 136,128 → 10,216 bytes, `find -name '*.py'` 80,692 → 5,598 bytes.

## Styles

```sh
lagom style                          # active styles
lagom style list                     # built-in, user and project styles
lagom style use terse                # user default
lagom style use --project lean-code  # this project only (.lagom/config.json)
lagom style use off
```

Default: `terse` + `lean-code`. A style is a Markdown file with `name` and `description` frontmatter; the body is injected as-is:

```markdown
---
name: team
description: Our review voice.
---
Answer first. Bullets only. Cite files as path:line. Never shorten error messages.
```

Lagom looks for style files in this order:
1. `.lagom/styles/<name>.md` in the project (the nearest parent folder wins).
2. `~/.config/lagom/styles/<name>.md`.
3. The built-in styles.

A file with a built-in name (`terse`, `lean-code`) replaces that style.

Active styles come from the first of these that is set:
1. `LAGOM_STYLES` (for example `LAGOM_STYLES=terse`).
2. `.lagom/config.json` in the project.
3. `~/.config/lagom/config.json`.

## Honest numbers

`lagom stats` reports compression savings and the size of the injected style rules (~400 tokens per session with the defaults). Token figures are estimates (4 bytes ≈ 1 token). Effects of styles on answer length are not measured yet; that needs an A/B harness, planned next.

## Data and privacy

Everything stays local:
- `~/.lagom/recall/` keeps the last 500 originals of compressed output.
- `~/.lagom/stats.jsonl` holds byte counts and program names, not output.

No telemetry, no network. `LAGOM_HOME` moves the data folder; `LAGOM_DISABLE=1` turns all hooks off.

## Develop

```sh
cargo test     # unit + end-to-end tests (sandboxed HOME; never touches your real agent config)
cargo clippy --all-targets
```

License: Apache-2.0.
