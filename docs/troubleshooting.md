# Troubleshooting

Start with `lagom status`: it shows which agents have Lagom's hooks and skills, and the active styles.

`lagom doctor` checks more, one line each with a fix (`--json` for scripts; it exits 1 when a check fails):

- the binary in every hook exists and is this version;
- Codex has a trust record for each of Lagom's hooks;
- other tools' hooks in the same files: a second set of answer rules at session start (Caveman, Ponytail), a second shell-command rewriter (rtk, Caveman), and Graphify, which blur a [style experiment](styles.md#measure-a-style);
- only you can read `~/.lagom`;
- Claude Code's managed and server-managed policy, and whether it stops Lagom from rewriting commands.

## Nothing happens in my agent

1. **Start a new session.** Hooks and skills load when a session starts.
2. **Codex:** hooks stay inactive until trusted. Open Codex, run `/hooks`, and approve the `lagom hook codex …` entries. Codex asks again whenever a hook command changes, for example after Lagom moves to a new path.
3. **Run a hook by hand.** It should print JSON with your style rules:
   ```sh
   echo '{"cwd": "'"$PWD"'"}' | lagom hook claude session-start
   ```
   No output means styles are off (`lagom style`) or `LAGOM_DISABLE` is set.
4. **The binary moved.** Hooks store the path of the `lagom` that installed them. After moving or reinstalling it some other way, run `lagom install` again.

## See why a hook did nothing

Hooks print nothing on errors, so they never get in the agent's way. Set `LAGOM_DEBUG=1` to see each decision: on stderr, and in `~/.lagom/debug.log` (at most 1 MB, then moved to `debug.1.log`). For example:

```
lagom: pre-tool claude: left alone: no allow rule matches, or the cd leaves the project
lagom: hook claude pre-tool: payload ignored: not JSON (25 bytes)
```

The lines hold sizes and decisions, never a command's text or a payload. An agent passes your shell's environment to its hooks, so start the agent from a shell where `LAGOM_DEBUG=1` is set. In Claude Code you can also put it under `"env"` in `~/.claude/settings.json`.

## A command isn't compressed

The hook rewrites a command to `lagom run -- …` only when all of these hold:

- **You already allow it** (Claude Code and Cursor). Lagom never grants a permission you haven't. Add allow rules for read-only commands you run often; `lagom permissions suggest` lists the ones your Claude Code logs show, with the rules to copy (it never adds them itself):
  ```json
  // ~/.claude/settings.json
  { "permissions": { "allow": ["Bash(git log:*)", "Bash(git diff:*)", "Bash(cargo test:*)"] } }
  ```
  ```json
  // ~/.cursor/cli-config.json
  { "permissions": { "allow": ["Shell(git)", "Shell(cargo)"] } }
  ```
  In Claude Code's bypass mode no rule is needed. A deny or ask rule always blocks the rewrite. Allow rules in a project's committed `.claude/settings.json` don't count (Claude Code applies them only after workspace trust); put them in your user settings or the project's uncommitted `.claude/settings.local.json`. Rule syntax follows Claude Code: `Bash(git log)` matches only that exact command, `Bash(git log *)` also matches it with arguments.
- **It is one known program** (git, cargo, npm, pytest, go, rg, docker, …) with no `|`, `&&`, `;`, redirects, subshells or `VAR=value` in front. The one exception is a leading `cd <dir> &&` into the project (`cd crates/core && cargo test`): the command after it is rewritten, if your rules allow it.
- **It isn't long-running:** dev servers, watch modes, `-f`/`--follow` and background commands are left alone.
- **The shell reads it as Lagom does:** no unbalanced quotes, trailing backslash, control characters (a `\r` from a Windows file, for example), unusual whitespace or invisible characters.

Check a decision by hand (empty output means "not rewritten"):
```sh
echo '{"cwd": "'"$PWD"'", "tool_name": "Bash", "permission_mode": "default",
       "tool_input": {"command": "git log"}}' | lagom hook claude pre-tool
```

You can always compress by hand: `lagom run -- <command>`.

## I need the full output

Compressed output ends with `[lagom: N → M lines; full output: lagom recall <id> …]`. Run `lagom recall <id>`, or fetch only what you need: `--grep TEXT` (matching lines with their numbers), `--lines A-B`, `--head N`, `--tail N`. Lagom keeps the last 500 originals, up to 200 MB and 14 days; older ones are gone. `lagom recall --list` shows what is kept.

To skip compression for one command: `LAGOM_RAW=1 git log`. To turn all hooks off: `LAGOM_DISABLE=1`.

## Lagom together with Caveman, ponytail or rtk

- **Two style tools** inject two sets of rules every session, and they can contradict each other (for example two different "be brief" rule sets). Keep one, or keep only Lagom's code style: `lagom style use lean-code`.
- **Two command rewriters** (Lagom and rtk) both try to rewrite the same shell commands. Use one of them for shell output.
- Lagom never edits other tools' hooks. Remove them with those tools' own uninstall commands.

## `lagom install` fails

- **"is not a plain JSON object; not changing it":** the agent's settings file isn't valid JSON (comments, a trailing comma, a half-saved edit). Lagom refuses to touch it. Fix the file, or restore `<file>.lagom.bak` if Lagom made one, then run `lagom install` again.
- **"skipped skill lagom: … exists and is not Lagom's":** you already have a skill folder with that name. Lagom leaves it alone; rename yours if you want Lagom's.
- **"`lagom install` doesn't support Windows yet":** the hook commands Lagom writes need a POSIX shell, so install refuses on Windows instead of writing hooks that can't run. `lagom uninstall` still works.

## Other

- **`cargo install` says binary `lgm` already exists:** that's the unrelated `lgm` crate. Use `cargo install lagom --bin lagom` to install only `lagom`.
- **The first run of a new build is slow on macOS** (up to a second): macOS scans unsigned binaries once. Later runs take a few milliseconds. Release builds will be signed.
- **`--purge` says "refusing to delete … Lagom didn't create it":** purge deletes only the default `~/.lagom` and `~/.config/lagom`, or a folder holding Lagom's `.lagom-dir` marker. A `LAGOM_HOME` or `XDG_CONFIG_HOME` pointing at a folder of yours, or one an older Lagom created before the marker existed, is refused. Check what is in it and delete it by hand.
- **`--purge` says "rerun with --yes to confirm":** outside a terminal there is no one to ask, so nothing was changed. Add `--yes`. `--purge` also refuses agent names (`lagom uninstall --all --purge`), because the data it deletes is shared by every agent.
- **Undo everything:** `lagom uninstall --all --purge`. See [install](install.md).
