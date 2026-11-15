# Benchmarks

| What | Where |
|---|---|
| Hook, `lagom run` and compression speed | `bench/latency.sh` (hyperfine) |
| Compression corpus | `bench/corpus/<tool>/<case>/` plus the golden cases in `tests/fixtures/compress/` |

## Corpus

Each case is a real command output (`input.txt`) and `meta.json`:

```json
{"exit_code": 101, "must_keep": ["tests::bad_parse", "test result: FAILED"], "known_gap": "CMP-07"}
```

- `must_keep`: strings that must survive compression (failing test names, errors, file names).
- `known_gap` (optional): Lagom doesn't keep everything yet; the plan task that fixes it. Benchmarks report these separately and gates skip them.

`scripts/gen_bench_corpus.sh` regenerates every case from commands run on scratch projects it creates (Rust, Python, Go, Node, C, Swift, git, search, JSON, logs, kubectl, docker, make). It never uses the network, `../samples`, or personal data, rewrites temporary paths to `/work` and the home folder to `/home/dev`, and refuses outputs that look like secrets.
