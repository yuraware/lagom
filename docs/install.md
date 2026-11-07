# Install

## Get the binary

```sh
cargo install --git https://github.com/yuraware/lagom   # from source
cargo install --path .                                  # from a checkout
```

Homebrew, an install script for macOS and Linux, Windows packages and `pip install lagom-ai` are planned for the first release.

`lgm` is a short alias: `lgm <command>` does exactly what `lagom <command>` does. If you already have the unrelated `lgm` crate installed, use `cargo install lagom --bin lagom`.

## Connect your agents

```sh
lagom install            # every agent found: Claude Code, Codex, Cursor
lagom install claude     # or name agents
lagom install --dry-run  # list every change, change nothing
lagom install --yes      # don't ask (scripts)
```

In a terminal, `lagom install` asks once before changing anything. It ends with one line per agent: installed, already up to date, skipped (not found) or failed. Start a new agent session afterwards.

- **Codex:** hooks stay inactive until you trust them. Open Codex, run `/hooks`, and approve the two `lagom hook codex …` entries.
- **Backups:** before its first change to a settings file, Lagom saves a copy next to it as `<file>.lagom.bak`. It keeps the file's permissions.
- **Running it again** is safe: hooks are updated in place, never duplicated.

`lagom status` shows which agents have Lagom's hooks and skills.

## Remove

```sh
lagom uninstall                  # remove Lagom's hooks and skills from every agent
lagom uninstall cursor           # or from one agent
lagom uninstall --all --purge    # also delete ~/.lagom and ~/.config/lagom (asks first)
```

`--purge` needs `--yes` when not run in a terminal. It keeps the `.lagom.bak` backups, refuses to delete a folder that is your home folder or above it, and prints the command that removes the binary for the way you installed it, for example `cargo uninstall lagom`.
