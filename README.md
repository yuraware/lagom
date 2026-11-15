# lagom

[![crates.io](https://img.shields.io/crates/v/lagom.svg)](https://crates.io/crates/lagom)

Fewer tokens, simpler answers for coding agents. *Lagom* is Swedish for "just the right amount".

One small Rust binary (~0.6 MB, no runtime, no daemon) that plugs into **Claude Code**, **Codex** and **Cursor**:

- **Styles.** Rules injected at session start that make the agent say less (`terse`) and write less code (`lean-code`). Plain Markdown files: edit them or add your own, per user or per project.
- **Compressor.** Long command output is shortened before the agent reads it. The start, the end and the error, warning and `file:line` lines are kept first. Exit codes are unchanged; anything dropped stays available through `lagom recall <id>`.
- **Skills.** `lagom`, `lagom-run`, `lagom-review`, `lagom-commit`, `lagom-stats`.

Status: **0.1.0, first public preview.** Tested on macOS; Linux should work; Windows is not supported yet.

## Install

```sh
brew install yuraware/tap/lagom    # Homebrew
cargo install lagom                # or crates.io (needs Rust 1.87+)
lagom install                      # connect every agent found; --dry-run shows changes first
```

Then start a new agent session. Codex needs one more step: run `/hooks` in Codex and approve Lagom's hooks. `lgm` works as a short alias for `lagom`. Upgrading, unreleased builds and removal: [docs/install.md](docs/install.md).

| Agent | Session start | Subagents | Command rewrite |
|---|---|---|---|
| Claude Code | ✓ | ✓ | ✓ |
| Codex | ✓ | – | ✓ after `/hooks` trust |
| Cursor | ✓ | – | ✓ |

On real outputs: `git log --stat -3` 115,863 → 5,462 bytes, `rg -n fn` 136,128 → 10,216 bytes. Rewrites never widen permissions: Claude Code and Cursor rewrite only commands your own allow rules already permit.

## Docs

- [Install and remove](docs/install.md)
- [Agents](docs/agents.md)
- [Styles](docs/styles.md): use, write your own, turn off
- [Compression](docs/compression.md): what gets rewritten, permissions, recall
- [Data and privacy](docs/privacy.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Testing](docs/dev/testing.md)

## Honest numbers

`lagom stats` reports compression savings and the size of the injected style rules (~400 tokens per session with the defaults). Token figures are estimates (4 bytes ≈ 1 token). The effect of styles on answer length is not measured yet; an A/B harness is planned.

## Develop

```sh
cargo test     # never touches your real agent config
cargo clippy --all-targets
```

License: Apache-2.0.
