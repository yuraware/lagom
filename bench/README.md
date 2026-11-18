# Benchmarks

| What | Where |
|---|---|
| Hook, `lagom run` and compression speed | `bench/latency.sh` (hyperfine; exit 1 when a median is over budget) |
| Compression corpus | `bench/corpus/<tool>/<case>/` plus the golden cases in `tests/fixtures/compress/` |
| Report on the corpus | `lagom bench compress [--json out.json] [--compare base.json]` (exit 1 on an unexpected `must_keep` loss) |
| A/B runs of a real agent on tasks | `bench/ab.py` (below) |

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


## A/B harness

`bench/ab.py` runs the same coding tasks under each arm and judges every run with checks the agent never sees. Standard library only.

```sh
python3 -I bench/ab.py --selftest                                   # every task's references
python3 -I bench/ab.py --agent fake --repeats 2                     # free dry run (bench/fake_agent.py)
python3 -I bench/ab.py --agent claude --arms baseline,terse-control,lagom:terse \
    --repeats 3 --budget-usd 20 [--model sonnet] [--task smoke/py-off-by-one,smoke/py-add-flag]
python3 -I bench/ab.py --agent codex --max-runs 12 ...              # Codex reports no cost
python3 -I bench/ab.py --agent claude --auth local --model qwen3-coder \
    --arms baseline,lagom:terse --task smoke/py-off-by-one,smoke/py-add-flag   # free smoke run
```

- **Sign-in (`--auth`):**
  - `api` (the default) uses `ANTHROPIC_API_KEY` (`OPENAI_API_KEY` for Codex) and needs `--budget-usd`.
  - `login` removes any key and endpoint from the agent's environment, so it uses your own subscription. Costs are then list-price estimates, not what the plan bills, and the report says so. On a rate limit the harness waits (`--backoff`, doubling, `--max-retries`) and retries; `--resume` continues an interrupted run without repeating finished ones. Whether your plan's terms allow benchmark runs on a login is your decision, before a published number uses it.
  - `local` runs Claude Code against a local model through Ollama's Anthropic-compatible API (`--local-url`, default `http://localhost:11434`, Ollama 0.14 or later), or Codex with `--oss`. It needs `--model`, costs nothing, writes to `bench/results/<date>-local/`, and exits 1 unless every output parsed and every Lagom run injected its style. Local results are never published.

- **Arms:** `baseline`; `terse-control`, with only the instruction "Answer concisely." (Claude Code `--append-system-prompt`, Codex `developer_instructions`); and `lagom:<style>`, with Lagom's hooks and that style in the run's project settings. The honest comparison is Lagom against the terse control.
- **A run:** the task's `fixture/` is copied to a fresh folder outside any git repository; the agent runs headless with JSON output; the task's `hidden/` checks are copied in afterwards and decide pass or fail. Arms are interleaved and their order rotates per task and repeat. One throwaway call per model comes first, so no arm pays the cold cache write.
- **Isolation:** Claude Code runs with `--setting-sources project --strict-mcp-config --no-session-persistence` and permissions bypassed, never `--bare` (it skips hooks). Lagom's user config and stats are swapped for the run's own (`XDG_CONFIG_HOME`, `LAGOM_HOME`), and non-Lagom arms set `LAGOM_DISABLE=1`.
- **Commands:** a task that allows Bash runs it in Claude Code's own sandbox (`"sandbox": {"enabled": true, "allowUnsandboxedCommands": false}` in the run's settings): writes stay in the run's folder and the network goes through an empty allow-list. Tasks that don't allow Bash pass `--disallowedTools Bash`. Codex runs with `--sandbox workspace-write`.
- **Spend:** `--budget-usd` is required for Claude Code. No run starts once it is reached, each call gets `--max-budget-usd` with what is left, and a run that reports no cost stops the harness. A paid run refuses to start if the selftest fails.
- **Results:** one JSON line per run in `bench/results/<date>/runs.jsonl` (not committed): the auth mode, status (`ok`, `agent_error`, `bad_output`, `timeout`, `rate_limited`), pass, cost, turns, durations, tokens from `modelUsage` (which counts subagents), and for Lagom arms how often the style was injected and output compressed. A failed or timed-out run is recorded, never dropped.

### Statistics

`bench/stats.py` compares each arm with the baseline, standard library only:

```sh
python3 -I bench/stats.py bench/results/<date>/runs.jsonl [--metric cost_usd|turns|wall_ms|usage.output|answer_words] [--json]
```

For every task, the median of the metric over its repeats; then, per arm, the geometric mean of the arm-to-baseline ratios over the tasks both ran, with a seeded paired bootstrap 95% interval (tasks are resampled, each with its own ratio) and a two-sided exact sign test. Pass rates come with Wilson 95% intervals. A ratio below 1 means the arm used less. Runs without the metric (a timeout has no cost) are left out of its medians but count for the pass rate.

A task is a folder under `bench/tasks/<suite>/`: `task.json` (`prompt`; `check` as an argument list, where `{bench}` stands for the `bench/` folder and `{task}` for the task's; `bash`; and optional `setup` and `solve` commands), `fixture/`, `hidden/` and `solution/` (or a `solve` command), named by its path (`rust/off-by-one`). `--tasks bench/tasks/rust` runs one suite. The selftest requires the fixture alone to fail its checks and fixture plus solution to pass.

| Suite | Tasks | Checks |
|---|---|---|
| `smoke/` | 2 small Python tasks, for the mini run and the local smoke run | a hidden Python script |
| `rust/` | 10 tasks on tiny dependency-free crates: an off-by-one, an overflow, a panicking iterator, case and punctuation in word counts, a CLI flag, a `Display` impl, panics turned into `Result`, a config value checked, a stable sort, and a field renamed across a crate | a hidden `cargo test --test hidden`; the agent may run commands (in the sandbox) |
| `python/` | 10 tasks on tiny packages: accents in slugs, a mutable default argument, dataclass validation, a retry decorator, quoted CSV fields, a `--json` flag for a CLI, a recursive file walk, a deep merge that mustn't modify its inputs, chunks from any iterable, and a function renamed across a package | hidden pytest-style tests (plain asserts), run by `bench/tasks/python/run_hidden.py` so nothing needs installing; `pytest test_hidden.py` runs them too |
| `output-heavy/` | 8 tasks where the agent must read big outputs, generated by each task's `setup` so the repository stays small: 3 failures among 405 unittest tests, a failed payment in a 40,000-line log, 60 compiler warnings across 12 modules, 2 failures among 254 Rust tests, a library move across 150 modules, and per-service failure counts from 4,000 JSON events. Two are the same suite and log at 13 tests and 40 lines, where Lagom should change nothing | the check generates the tests or data again from scratch, so editing them doesn't help |
| `prose/` | 10 explain, plan and review questions about small files (`answer: true`: the agent's final message is the result, saved as `answer.md`). Four are safety probes: a SQL injection to flag, a leaked API key to rotate without repeating it, `rm -rf` and `git reset --hard` with their irreversibility stated, and an exact error line quoted | `check_answer.py` with the task's `facts.json`: regular expressions that must all match, and some that must not. Each task also has a terse correct answer that must pass and a plausible wrong one that must fail |
