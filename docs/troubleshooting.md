# Troubleshooting

Start with `lagom status`: it shows which agents have Lagom's hooks and skills, and the active styles.

## Nothing happens in my agent

1. **Start a new session.** Hooks and skills load when a session starts.
2. **Codex:** hooks stay inactive until trusted. Open Codex, run `/hooks`, and approve the `lagom hook codex …` entries. Codex asks again whenever a hook command changes, for example after Lagom moves to a new path.
3. **Run a hook by hand.** It should print JSON with your style rules:
   ```sh
   echo '{"cwd": "'"$PWD"'"}' | lagom hook claude session-start
   ```
   No output means styles are off (`lagom style`) or `LAGOM_DISABLE` is set.
4. **The binary moved.** Hooks store the path of the `lagom` that installed them. After moving or reinstalling it some other way, run `lagom install` again.

## A command isn't compressed

The hook rewrites a command to `lagom run -- …` only when all of these hold:

- **You already allow it** (Claude Code and Cursor). Lagom never grants a permission you haven't. Add allow rules for read-only commands you run often:
  ```json
  // ~/.claude/settings.json
  { "permissions": { "allow": ["Bash(git log:*)", "Bash(git diff:*)", "Bash(cargo test:*)"] } }
  ```
  ```json
  // ~/.cursor/cli-config.json
  { "permissions": { "allow": ["Shell(git)", "Shell(cargo)"] } }
  ```
  In Claude Code's bypass mode no rule is needed. A deny or ask rule always blocks the rewrite. Allow rules in a project's committed `.claude/settings.json` don't count (Claude Code applies them only after workspace trust); put them in your user settings or the project's uncommitted `.claude/settings.local.json`. Rule syntax follows Claude Code: `Bash(git log)` matches only that exact command, `Bash(git log *)` also matches it with arguments.
- **It is one known program** (git, cargo, npm, pytest, go, rg, docker, …) with no `|`, `&&`, `;`, redirects, subshells or `VAR=value` in front.
- **It isn't long-running:** dev servers, watch modes, `-f`/`--follow` and background commands are left alone.

Check a decision by hand (empty output means "not rewritten"):
```sh
echo '{"cwd": "'"$PWD"'", "tool_name": "Bash", "permission_mode": "default",
       "tool_input": {"command": "git log"}}' | lagom hook claude pre-tool
```

You can always compress by hand: `lagom run -- <command>`.

## I need the full output

Compressed output ends with `[lagom: N → M lines; full output: lagom recall <id> …]`. Run `lagom recall <id>`, or fetch only what you need: `--grep TEXT` (matching lines with their numbers), `--lines A-B`, `--head N`, `--tail N`. Lagom keeps the last 500 originals; older ones are gone.

To skip compression for one command: `LAGOM_RAW=1 git log`. To turn all hooks off: `LAGOM_DISABLE=1`.

## Lagom together with Caveman, ponytail or rtk

- **Two style tools** inject two sets of rules every session, and they can contradict each other (for example two different "be brief" rule sets). Keep one, or keep only Lagom's code style: `lagom style use lean-code`.
- **Two command rewriters** (Lagom and rtk) both try to rewrite the same shell commands. Use one of them for shell output.
- Lagom never edits other tools' hooks. Remove them with those tools' own uninstall commands.

## `lagom install` fails

- **"is not a plain JSON object; not changing it":** the agent's settings file isn't valid JSON (comments, a trailing comma, a half-saved edit). Lagom refuses to touch it. Fix the file, or restore `<file>.lagom.bak` if Lagom made one, then run `lagom install` again.
- **"skipped skill lagom: … exists and is not Lagom's":** you already have a skill folder with that name. Lagom leaves it alone; rename yours if you want Lagom's.

## Other

- **`cargo install` says binary `lgm` already exists:** that's the unrelated `lgm` crate. Use `cargo install lagom --bin lagom` to install only `lagom`.
- **The first run of a new build is slow on macOS** (up to a second): macOS scans unsigned binaries once. Later runs take a few milliseconds. Release builds will be signed.
- **`--purge` says "refusing to delete":** `LAGOM_HOME` or `XDG_CONFIG_HOME` points at your home folder or above it. Unset it, or delete the right folder by hand.
- **Undo everything:** `lagom uninstall --all --purge`. See [install](install.md).
