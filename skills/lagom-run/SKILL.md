---
name: lagom-run
description: Run shell commands with long output (tests, builds, logs, git history, searches) through Lagom's compressor to save tokens, and recover full output when needed. Use before running noisy commands, or when output contains "[lagom: … lagom recall <id>]".
---
- Run noisy commands as `lagom run -- <command> [args…]`. Output keeps the start, the end, every error/warning/file:line line, and collapses repeats. The exit code is unchanged.
- Use it for one program with arguments. For pipes or `&&`, run the plain command or wrap it: `lagom run -- sh -c '<command>'`.
- Pipe text through the same compressor: `<command> | lagom compress`.
- A line `[lagom: N → M lines; full output: lagom recall <id>]` means lines were omitted. Run `lagom recall <id>` only if the omitted part matters.
- Do not wrap interactive or long-running commands (dev servers, watch modes, `tail -f`).
- Some agents rewrite simple commands to `lagom run` automatically; then just run commands normally.
