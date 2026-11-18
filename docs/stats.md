# Savings and stats

`lagom stats` shows what Lagom saved and what it cost, from data on your machine. Every section says how its numbers were made, and ends with what it covers.

```sh
lagom stats                          # the last 30 days (UTC)
lagom stats --days 7                 # or --since 2026-11-01, or --all-time
lagom stats --agent claude --style terse
lagom stats --project .              # this repository only
lagom stats --json                   # the same numbers, for scripts
```

Filters: `--agent` (`claude`, `codex`, `cursor`, `manual` for commands you ran by hand), `--model`, `--style`, `--project` (a path such as `.`, or its id) and `--session`. A value that never occurs is an error that lists the known ones. Times are UTC, and the window uses each event's own time: a session counts when its last message falls inside it.

## What each number means

Each number is **measured** (read from a log), **estimated** (computed with a stated rule) or **unknown** (not measured yet). The report says which.

| Section | Shows | How |
|---|---|---|
| Compression | Commands Lagom ran, bytes before and after, tokens saved, per command | Estimated: tokens ≈ bytes ÷ 4. Commands Lagom didn't wrap aren't counted; `lagom discover` lists them |
| Injection | How often the style rules were added, and their size, per style | Estimated: tokens ≈ bytes ÷ 4 |
| Output | Median output tokens per main-conversation turn, per style and agent | Measured from Claude Code's and Codex's own logs. Observational: sessions differ in their work, so this doesn't prove what a style does. Cursor keeps no usage data, so its sessions are counted as unavailable |
| Cache | Prompt-cache reads and writes, and the share of input read from the cache, per agent and per day | Measured from the same logs. A day is flagged ⚠ when its share fell below 80% of the previous day's after a Lagom change (a new style set or version): losing the cache can cost more than compression saves |
| Cost | Estimated cost of the measured sessions, per agent | Estimated at list prices per million tokens, with the source and the date they were checked. Cache writes use the 5-minute price, and Claude Haiku 5.5 its price for prompts up to 100,000 tokens. Sessions on a model with no known price are counted and named |
| Net | Tokens saved by compression minus tokens of injected rules, per agent and per day | Estimated. **Style savings are unknown** until a holdout experiment measures them |

`lagom stats --usage` shows the full per-style table behind the Output section: sessions, turns, output and input tokens per turn, and cache share.

## Prices

Prices for Claude models are built in, from [Anthropic's pricing page](https://platform.claude.com/docs/en/about-claude/pricing). Add a model, such as one Codex uses, or replace a price in `~/.config/lagom/prices.json`, in US dollars per million tokens:

```json
{"gpt-6.1-sol": {"input": 1.25, "output": 10, "cache_write": 1.25, "cache_read": 0.125}}
```

An entry without all four numbers is reported and not used.

## Measure what styles save

Only an experiment turns "style savings: unknown" into a measured number: some sessions run without styles, and `lagom stats --experiment` compares the two groups with a 95% interval. See [Measure a style](styles.md#measure-a-style).

## What leaves your machine

Nothing. `lagom stats` reads `~/.lagom/stats.jsonl` (byte counts, program names, ids; never output or arguments, see [Data and privacy](privacy.md)) and the agents' own logs on disk (`~/.claude/projects/`, `~/.codex/sessions/`). It makes no network calls; the price table is part of the binary.
