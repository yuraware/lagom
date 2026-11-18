# How Lagom is benchmarked

This page explains how Lagom's published numbers are made, so you can check them or rerun them. The numbers themselves are on [Benchmarks](benchmarks.md) once a run is published. That page is generated from the stored results, and it says which results it came from.

Lagom makes two kinds of claims, measured in two different ways:

| Claim | Measured with | Costs to rerun |
|---|---|---|
| Compression keeps what matters and shrinks output | `lagom bench compress` on a fixed corpus of command outputs | Nothing: it runs locally in under a second |
| A style or compression changes an agent's cost, turns or success | `bench/ab.py`: real agent sessions on fixed tasks, with and without Lagom | API usage, capped by a budget you set |

## What is measured

The A/B headline is **cost per passed task**, always next to the **pass rate**, the **turns** and the **seconds**. The four are never folded into one score, because a style can make answers cheaper and wrong. Tokens removed by compression are a regression check, never a headline: fewer tokens in a tool's output doesn't mean the session cost less.

## Arms

Every run compares the same tasks under several arms:

- `baseline`: the agent as installed, with no Lagom.
- `terse-control`: the agent with one extra instruction, "Answer concisely." Lagom's styles have to beat this, not only the baseline, because any short instruction makes answers shorter.
- `lagom:<style>`: Lagom's hooks and that style, installed in the run's folder.
- `candidate:<style>`: the same with a new Lagom build, run next to the release, so a claim of "faster" names what it was compared with.

## Tasks

40 tasks in five suites under `bench/tasks/`. Each task has a starting folder, hidden checks the agent never sees, and a reference solution:

| Suite | Tasks | Judged by |
|---|---|---|
| `smoke/` | 2 small Python tasks, for quick checks | a hidden Python script |
| `rust/` | 10 tasks on small Rust crates: bug fixes, features, API changes, a refactor | a hidden `cargo test` file |
| `python/` | 10 tasks on small Python packages | hidden pytest-style tests |
| `output-heavy/` | 8 tasks where the agent must read big output: a 40,000-line log, 405 tests with 3 failing, 60 compiler warnings… Two are the same task at a small size, where Lagom should change nothing | the check regenerates the tests or data, so editing them doesn't help |
| `prose/` | 10 explain, plan and review questions, four of them safety probes: a SQL injection, a leaked key, `rm -rf` and `git reset --hard`, an exact error message | regular expressions for the facts an answer must contain, or must not |

Every task is marked with its kind (`small-fix`, `feature`, `build` or `prose`), and 9 are marked `risky` before any run, the ones where a careful engineer would add a test. The report splits results by kind, so a loss on one kind can't hide behind a win on another.

`python3 -I bench/ab.py --selftest` checks every task: the starting folder must fail its checks and the reference solution must pass them. A paid run refuses to start if this fails.

## Agents, models and sign-in

Claude Code first, then Codex. Every result line records the agent, the model and the Lagom version.

How the agent signs in decides what a number may be used for:

| `--auth` | Uses | Costs reported as | Published? |
|---|---|---|---|
| `api` | an API key and `--budget-usd` | Claude Code's estimate at list prices | yes |
| `login` | your own subscription | a list-price estimate, not what the plan bills | only if the plan's terms allow it |
| `local` | a local model through Ollama | nothing | never: it checks the harness |

## How a run works

1. The task's starting folder is copied to a new temporary folder outside any git repository, and its setup script generates any large files.
2. For a Lagom arm, the run's project settings get Lagom's hooks and the style.
3. The agent runs headless with JSON output. Claude Code gets `--setting-sources project --strict-mcp-config --no-session-persistence`, so your own settings, MCP servers and history stay out. It never gets `--bare`, which would skip Lagom's hooks.
4. Commands, when a task allows them, run in Claude Code's own sandbox: writes stay in the run's folder, and the network is closed. Codex runs with `--sandbox workspace-write`.
5. When the agent stops, the hidden checks are copied in and decide pass or fail.

Arms are interleaved: every task runs under every arm before the next repeat, and the arms' order rotates per task and repeat. One throwaway call per model comes first, so no arm pays for filling the prompt cache. A run that takes longer than the timeout (15 minutes by default) is stopped and counts as failed. The harness stops starting runs once the budget is spent.

## How tokens and cost are counted

Each agent's own usage report, never an estimate of Lagom's:

- **Claude Code:** tokens from `modelUsage` in its final result, which includes subagents (the `usage` field leaves them out). Cost from `total_cost_usd`, Claude Code's estimate at list prices.
- **Codex:** tokens from each `turn.completed` event. Codex reports no cost.
- **Tool calls:** counted by type from the agent's event stream.
- **Tool output:** the bytes the agent read from its tools, divided by 4 for an estimate in tokens.

## Statistics

`bench/stats.py`, standard library only:

1. For each task and arm, the median over the repeats.
2. For each arm, the ratio of its median to the baseline's on every task, and the geometric mean of those ratios. A ratio below 1 means the arm used less.
3. A 95% interval for that mean, from a paired bootstrap over tasks: 10,000 resamples, seed 1, so rerunning on the same results gives the same interval.
4. A two-sided exact sign test on how many tasks went each way.
5. Pass rates with Wilson 95% intervals.

An arm counts as compared only if Lagom actually acted in at least two thirds of its runs, by injecting its style or compressing a command. Otherwise its comparisons are marked "doesn't count".

## Known biases and limits

- **The tasks are small and synthetic.** Real repositories are bigger and messier; a style that helps here may not help there.
- **Agents are not deterministic.** Repeats and medians reduce noise, but a rerun gives different raw numbers. The statistics on stored results are exact.
- **Costs are list-price estimates.** Discounts, batch pricing and subscriptions are not modeled.
- **Models change.** A result is for the model and agent version it names.
- **Prose checks are patterns.** Each one passes a written terse answer and fails a written wrong one, but a correct answer worded in an unexpected way can still fail.
- **Speed includes Lagom's own overhead:** its hooks and `lagom run` are extra processes, and that time is counted, not removed.

## Reproduce a table

Two tables can be reproduced without paying for anything, from a fresh clone:

```sh
git clone https://github.com/yuraware/lagom && cd lagom
cargo build --release

# 1. The compression table in bench/README.md ("Results").
./target/release/lagom bench compress | sed -n '/^| family/,/^$/p'

# 2. The A/B report's headline table, from the stored sample results.
python3 -I bench/report.py bench/testdata/sample-runs.jsonl --out /tmp/lagom-report --resamples 2000
diff /tmp/lagom-report/benchmarks.md bench/testdata/report/benchmarks.md && echo "identical"
```

The first prints the per-family table that `bench/README.md` publishes. The second rebuilds the sample report byte for byte; it is synthetic data, there to show that the statistics and the page come out the same anywhere.

To rerun the A/B benchmark itself, with your own key and a budget:

```sh
python3 -I bench/ab.py --selftest
python3 -I bench/ab.py --agent claude --auth api --lagom-bin target/release/lagom \
    --arms baseline,terse-control,lagom:terse --repeats 3 --budget-usd 20
python3 -I bench/stats.py bench/results/<date>/runs.jsonl --report
```

Your raw numbers will differ, because agents aren't deterministic, but they should land inside the published intervals. A published page rebuilds from its stored results with `python3 -I bench/report.py --check`.
