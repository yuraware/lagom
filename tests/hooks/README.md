# Hook contract fixtures

One folder per agent and case: `tests/hooks/<agent>/<event>[--variant]/`.

- `input.json`: the hook payload. `<HOME>` is replaced with a sandboxed home folder.
- `home/` (optional): files copied into that home first, for example permission rules.
- `expected.json`: the hook's stdout as JSON, with the home folder shown as `<HOME>` and the
  binary as `<LAGOM>`. An empty file means the hook must print nothing.

`tests/hooks.rs` runs `lagom hook <agent> <event>` on each case. After an intended change, run
`LAGOM_BLESS=1 cargo test --test hooks` and review the diff.

Payload shapes follow each agent's documented hook schema (Claude Code and Codex: PreToolUse /
SessionStart / SubagentStart; Cursor: sessionStart / preToolUse with `workspace_roots`). Replace them
with live captures when available (QA-10).
