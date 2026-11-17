# Benchmarks

| What | Where |
|---|---|
| Hook, `lagom run` and compression speed | `bench/latency.sh` (hyperfine; exit 1 when a median is over budget) |
| Compression corpus | `bench/corpus/<tool>/<case>/` plus the golden cases in `tests/fixtures/compress/` |
| Report on the corpus | `lagom bench compress [--json out.json] [--compare base.json]` (exit 1 on an unexpected `must_keep` loss) |

## Corpus

Each case is a real command output (`input.txt`) and `meta.json`:

```json
{"exit_code": 101, "must_keep": ["tests::bad_parse", "test result: FAILED"], "known_gap": "CMP-07"}
```

- `must_keep`: strings that must survive compression (failing test names, errors, file names). A failing case lists every error line: the whole line, or at least 20 characters of it. `scripts/must_keep_errors.py` adds missing ones, and `cargo test` fails if one is missing.
- `known_gap` (optional): Lagom doesn't keep everything yet; the plan task that fixes it. Benchmarks report these separately and gates skip them.

`scripts/gen_bench_corpus.sh` creates scratch projects (Rust, Python, Go, Node, C, Swift, git, search, JSON, logs, kubectl, docker, make) and runs commands on them. It never uses the network, `../samples`, or personal data, rewrites temporary paths to `/work` and the home folder to `/home/dev`, and refuses outputs that look like secrets.

## The gate

CI's `fidelity` job fails when:

- a `must_keep` string is lost in a case without `known_gap`;
- total output grows more than 5% against `main` (label `size-increase-ok` to allow it);
- a pull request changes an existing case's `must_keep` or `known_gap`, or removes a case, without the `bench-meta-ok` label (`scripts/bench_meta_check.py`). Rewording what a case must keep is how a gate gets passed without the code getting better, so a maintainer has to agree.

Both compare against `main`, not the pull request's base branch.

## Results

The report is per family (the folder a case is in), with the median saving per case and how many outputs got smaller at all. A byte-weighted total would let the two largest logs stand for everything (D-36). `lagom bench compress` prints the table below after the per-case one.

On the scratch-project corpus and the golden cases (67 cases; the `compress` family is `tests/fixtures/compress/`), 2026-11-18:

| family | cases | median saved | compressed |
|---|---:|---:|---:|
| c | 1 | 0% | 0/1 |
| compress | 19 | 69% | 15/19 |
| docker | 1 | 0% | 0/1 |
| git | 10 | 17% | 5/10 |
| go | 6 | 10% | 3/6 |
| json | 3 | 95% | 2/3 |
| k8s | 1 | 0% | 0/1 |
| logs | 2 | 100% | 2/2 |
| make | 1 | 91% | 1/1 |
| node | 2 | 73% | 2/2 |
| python | 6 | 0% | 2/6 |
| rust | 7 | 0% | 3/7 |
| search | 7 | 82% | 6/7 |
| swift | 1 | 1% | 1/1 |

42 of 67 outputs got smaller (63%); the median saving per case is 55%. Error output is mostly short already: the median `python` and `rust` cases save nothing, while search results, JSON and logs save 82–100%.

