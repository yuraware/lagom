# Install

## Get the binary

```sh
brew install yuraware/tap/lagom    # Homebrew
cargo install lagom                # or crates.io (needs Rust 1.87+)
```

| | Homebrew | Cargo |
|---|---|---|
| Upgrade | `brew upgrade lagom` | `cargo install lagom --force` |
| Unreleased `main` | `brew install --HEAD yuraware/tap/lagom` | `cargo install --git https://github.com/yuraware/lagom` |
| Remove the binary | `brew uninstall lagom` | `cargo uninstall lagom` |

Homebrew builds from source, so the first install also downloads Rust as a build dependency. The formula lives in [yuraware/homebrew-tap](https://github.com/yuraware/homebrew-tap). From a local checkout, use `cargo install --path .`. An install script for macOS and Linux and `pip install lagom-ai` are planned.

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

`--purge` needs `--yes` when not run in a terminal, and works only without agent names: the data it deletes is shared by every agent. It keeps the `.lagom.bak` backups. It deletes only folders Lagom created: the default `~/.lagom` and `~/.config/lagom`, or a folder holding Lagom's `.lagom-dir` marker, so a `LAGOM_HOME` pointing at one of your own folders is refused. At the end it prints the command that removes the binary for the way you installed it, for example `brew uninstall lagom`.

Run `lagom uninstall` before removing the binary, so no hook is left pointing at a missing file.
