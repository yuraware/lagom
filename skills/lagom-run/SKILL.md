---
name: lagom-run
description: Run shell commands with long output (tests, builds, logs, git history, searches) through Lagom's compressor to save tokens, and recover full output when needed. Use before running noisy commands, or when output contains "[lagom: … lagom recall <id>]".
---
- Run noisy commands as `lagom run -- <command> [args…]`. Output keeps the start, the end, every error/warning/file:line line, and collapses repeats. The exit code is unchanged.
- Use it for one program with arguments. For pipes or `&&`, run the plain command or wrap it: `lagom run -- sh -c '<command>'`. A leading `cd <dir> &&` is fine: `cd crates/core && lagom run -- cargo test`.
- Pipe text through the same compressor: `<command> | lagom compress`.
- A line `[lagom: N → M lines; full output: lagom recall <id> …]` means lines were omitted. Only if the omitted part matters, fetch just what you need: `lagom recall <id> --grep TEXT` (matching lines with line numbers; lowercase TEXT matches any case) or `lagom recall <id> --lines A-B`. Use plain `lagom recall <id>` only when you need everything.
- Do not wrap interactive or long-running commands (dev servers, watch modes, `tail -f`).
- Some agents rewrite simple commands to `lagom run` automatically; then just run commands normally.
