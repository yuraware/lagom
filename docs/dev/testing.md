# Testing

| What | Command | Notes |
|---|---|---|
| Unit and integration tests | `cargo test` | Integration tests run the real binary against a throwaway `HOME`; they never touch your agent config |
| Compressor golden cases | `cargo test --test golden` | Real outputs in `tests/fixtures/compress/`. After an intended change: `LAGOM_BLESS=1 cargo test --test golden`, then review the diff. Regenerate inputs with `scripts/gen_compress_fixtures.sh` |
| Hook contract cases | `cargo test --test hooks` | One payload per agent and event in `tests/hooks/`. Bless the same way with `LAGOM_BLESS=1` |
| Release-notes script | `python3 -I -m unittest discover -s scripts -p 'test_*.py'` | Standard library only |
| Lint | `cargo fmt --check && cargo clippy --all-targets -- -D warnings` | CI runs both |
| Live Claude Code check | `scripts/e2e_claude.sh` | Costs one short Haiku session. See below |
| Hook and compression speed | `bench/latency.sh` | Needs [hyperfine](https://github.com/sharkdp/hyperfine). Uses a throwaway `HOME`. Prints medians against the budgets (hooks < 10 ms, `lagom run` overhead < 5 ms, 1 MB < 20 ms) and writes `target/bench/latency.json`. CI runs it on Linux and macOS |

## Live Claude Code check

`scripts/e2e_claude.sh` builds Lagom, creates a temporary git repo with 300 files, and puts Lagom's
hooks and one allow rule (`Bash(git show:*)`) in that repo's `.claude/settings.local.json`. It runs:

```sh
claude -p --setting-sources project,local --model haiku "…git show --stat HEAD…"
```

`--setting-sources project,local` means your user settings, and any hooks other tools installed there,
are not loaded. Lagom writes its stats to a temporary `LAGOM_HOME`. The check passes when the
session-start injection fired and `git show` ran through `lagom run` with smaller output.
