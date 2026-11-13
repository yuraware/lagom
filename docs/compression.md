# Command-output compression

Long command output (tests, builds, `git log`, searches) is shortened before the agent reads it:

- ANSI colors removed, progress bars resolved to their final state, blank-line runs collapsed.
- Exact repeats become one line with a count (`[×37]`); runs of lines that differ only in numbers keep their first and last line.
- Over 120 lines, the output keeps its first 30 and last 50 lines, plus up to 40 error, warning and `file:line` lines from the middle, in the order they appear. Failed commands get double these limits. Anything past them is left out, so check the footer below when a long log has many errors.
- Exit codes are unchanged.
- Anything dropped stays available: the output ends with `[lagom: N → M lines; full output: lagom recall <id> (or part: --grep TEXT, --lines A-B)]`. `lagom recall <id> --grep error` prints only matching lines with their line numbers (lowercase text matches any case, so `error` also finds `ERROR`; text with a capital letter matches exactly); `--lines 120-160`, `--head N` and `--tail N` print a slice.

## Automatic: command rewrite

A pre-tool hook turns a simple command such as `git log --stat` into `lagom run -- git log --stat`.

- **Rewritten:** only one known program (git, cargo, npm, pytest, go, rg, docker, …) with no pipes, `&&`, `;`, redirects or subshells, and no environment assignment in front.
- **Left alone:** dev servers, watch modes, `-f`/`--follow`, background commands.
- **Permissions never widen:**
  - **Claude Code and Cursor:** a command is rewritten (and allowed) only if your own allow rule already matches it, for example `Bash(git log:*)` or `Shell(git)`. Deny and ask rules always win. In Claude's `bypassPermissions` mode, commands are rewritten without granting anything.
    - Lagom reads rules the way Claude Code applies them: managed (organization) settings, your user settings, and a project's `.claude/settings.local.json` when it is your own file (not committed to git) in the folder the session started in or its repository root. Allow rules in a project's shared `.claude/settings.json` are **not** used, because Claude Code applies them only after you trust the folder, which Lagom can't see. Deny and ask rules from every file always count. If a managed settings file can't be read, Lagom doesn't rewrite.
    - **Cursor:** allow rules come from `~/.cursor/cli-config.json`. A project's `.cursor/cli.json` deny rules count; its allow rules don't, because a cloned repository supplies them.
    - Claude Code checks deny rules against the rewritten command, so Lagom must see every deny rule itself. It doesn't rewrite when an organization policy it can't read may apply (a macOS MDM profile, or Windows). Server-managed settings from the claude.ai admin console can't be detected locally: if your organization uses them, set `LAGOM_DISABLE=1` or keep rewriting off.
  - **Codex:** Lagom hands Codex the rewritten command, and Codex applies its own approval policy and sandbox to it. Commands you approved earlier by prefix may be asked about once more, because they now start with `lagom run`. This has not been checked in a live Codex session yet.

## Escape hatches

| To skip compression for | Do this |
|---|---|
| One command | `LAGOM_RAW=1 git log` or `command git log` (anything not starting with a known program is left alone) |
| One project | `{"rewrite": false}` in the project's `.lagom/config.json` |
| You, everywhere | `{"rewrite": false}` in `~/.config/lagom/config.json`, or `LAGOM_REWRITE=off` in your shell |
| All hooks, including styles | `LAGOM_DISABLE=1` |

A project file can only turn rewriting off: `{"rewrite": true}` in a repository doesn't undo your own `false`. `LAGOM_REWRITE=on` overrides any config file that turned rewriting off. Commands you run by hand with `lagom run` are always compressed.

## By hand

```sh
lagom run -- cargo test        # compressed output, same exit code
some-command | lagom compress  # compress any text
lagom recall 3fa9c01d2e        # full original of a compressed result
lagom stats                    # estimated savings vs. style overhead
```

Measured on real outputs: `git log --stat -3` 115,863 → 5,462 bytes, `rg -n fn` 136,128 → 10,216 bytes, `find -name '*.py'` 80,692 → 5,598 bytes. Token figures in `lagom stats` are estimates (4 bytes ≈ 1 token).

## Turn off

`LAGOM_DISABLE=1` turns off all hooks; `lagom uninstall` removes them.
