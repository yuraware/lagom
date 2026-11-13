# Command-output compression

Long command output (tests, builds, `git log`, searches) is shortened before the agent reads it:

- ANSI colors removed, progress bars resolved to their final state, blank-line runs collapsed.
- Exact repeats become one line with a count (`[×37]`); runs of lines that differ only in numbers keep their first and last line.
- Over 120 lines, the output keeps its first 30 and last 50 lines, plus up to 40 error, warning and `file:line` lines from the middle, in the order they appear. Failed commands get double these limits. Anything past them is left out, so check the footer below when a long log has many errors.
- JSON and JSON Lines (`gh api`, `kubectl -o json`, `jq`, `go test -json`) over 120 lines, or with a line over 400 characters, are printed as compact JSON. Arrays over 20 items keep their first 3 and last 2 items and every item that reports a problem anywhere inside it: a failure key (`status: 500`, `success: false`, a non-null `error`, `level: "error"`) or text with an error word or a `file:line` location. A marker such as `"… 34 more items (keys: id, status, error)"` counts the rest. Objects are never cut.
- Timestamped logs (app and access logs, `docker logs`, `kubectl logs`) over 120 lines: lines that differ only in numbers, times and ids collapse to the first and last one plus `[×N lines like this: …]`. Lines with a problem level (ERROR, WARN, FATAL, CRIT …), an HTTP 5xx or an error word are never collapsed. If too many remain, error, CRIT and 5xx lines are kept before warnings, and any left out are counted. `ls -l` listings are not treated as logs.
- Diffs (`git diff`, `git show`, `git log -p`) over 120 lines keep every changed line with its file and hunk headers, plus one unchanged line beside each change. Past 480 changed lines (960 for a failed command), later files show only their `+`/`-` counts. Lockfiles (`Cargo.lock`, `package-lock.json`, `uv.lock` …) and `GIT binary patch` bodies become one line each. A diff without `diff --git` headers (`diff -u`) is shortened like any other output.
- Exit codes are unchanged.
- Anything dropped stays available: the output ends with `[lagom: N → M lines; full output: lagom recall <id> (or part: --grep TEXT, --lines A-B)]`. `lagom recall <id> --grep error` prints only matching lines with their line numbers (lowercase text matches any case, so `error` also finds `ERROR`; text with a capital letter matches exactly); `--lines 120-160`, `--head N` and `--tail N` print a slice.

## Automatic: command rewrite

A pre-tool hook turns a simple command such as `git log --stat` into `lagom run --session claude:<id> -- git log --stat`. The session id only tags the command's line in `~/.lagom/stats.jsonl`.

- **Rewritten:** only one known program (git, cargo, npm, pytest, go, rg, docker, …) with no pipes, `&&`, `;`, redirects or subshells, and no environment assignment in front.
- **Also rewritten:** one leading `cd <dir> &&` before such a command (`cd crates/core && cargo test` becomes `cd crates/core && lagom run --session claude:<id> -- cargo test`). The directory must be a plain or single-quoted path with no variables or globs, and not an option: `cd -` and `cd --` are left alone. In Claude Code and Cursor the rewrite is allowed only when your rules allow the command and the `cd` stays inside the project.
- **Left alone:** dev servers, watch modes, `-f`/`--follow`, background commands.
- **Permissions never widen:**
  - **Claude Code and Cursor:** a command is rewritten (and allowed) only if your own allow rule already matches it, for example `Bash(git log:*)` or `Shell(git)`. Deny and ask rules always win. In Claude's `bypassPermissions` mode, commands are rewritten without granting anything.
    - Lagom reads rules the way Claude Code applies them: managed (organization) settings, your user settings, and a project's `.claude/settings.local.json` when it is your own file (not committed to git) in the folder the session started in or its repository root. Allow rules in a project's shared `.claude/settings.json` are **not** used, because Claude Code applies them only after you trust the folder, which Lagom can't see. Deny and ask rules from every file always count. If a managed settings file can't be read, Lagom doesn't rewrite.
    - **Cursor:** allow rules come from `~/.cursor/cli-config.json`. A project's `.cursor/cli.json` deny rules count; its allow rules don't, because a cloned repository supplies them.
    - Claude Code checks deny rules against the rewritten command, so Lagom must see every deny rule itself. It doesn't rewrite when an organization policy it can't read may apply (a macOS MDM profile, or Windows). Server-managed settings from the claude.ai admin console can't be detected locally: if your organization uses them, set `LAGOM_DISABLE=1` or keep rewriting off.
  - **Codex:** Lagom hands Codex the rewritten command, and Codex applies its own approval policy and sandbox to it. Commands you approved earlier by prefix may be asked about once more, because they now start with `lagom run --session codex:<id>`, and the id changes with every session. This has not been checked in a live Codex session yet.

## Find what isn't compressed

`lagom discover [--days N]` (default 30) reads Claude Code's and Codex's logs on this machine and lists large command outputs (4 KB or more) that weren't compressed, with the reason (no allow rule, a compound command, a program Lagom doesn't know). It also shows each tool's share of large outputs and the files whose same part was read again in one session. Commands appear as the program name, plus the subcommand for programs Lagom knows (`git log`, `cargo test`); other arguments never appear.

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
