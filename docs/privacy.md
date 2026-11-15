# Data and privacy

Everything stays on your machine. No telemetry and no network calls.

| Path | What | Kept |
|---|---|---|
| `~/.lagom/recall/` | Full originals of compressed output, for `lagom recall`, plus the program name | Last 500, at most 200 MB, at most 14 days. `lagom recall --list` / `--clear` |
| `~/.lagom/stats.jsonl` | Byte counts, program names, agent and event names. Never output | Until you delete it |
| `~/.config/lagom/` | Your styles and config | Until you delete it |
| `<repo>/.lagom/` | Project styles and config, only if you create them | — |

- `~/.lagom` is readable only by you (folders 0700, files 0600): recalled output can contain secrets that commands printed.
- `LAGOM_HOME` moves the data folder; `XDG_CONFIG_HOME` moves the config folder.
- `LAGOM_DISABLE=1` turns all hooks off.
- `lagom uninstall --all --purge` deletes both folders.
