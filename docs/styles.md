# Styles

A style is a set of rules injected at session start. Built-in:

- **`terse`:** say less. Answer first, no filler, exact technical details kept; full clarity for security warnings and destructive steps.
- **`lean-code`:** write less code. Need → existing code → standard library → installed dependency → the smallest new code, plus a list of things never to cut (validation, error handling, security).

Default: `terse` + `lean-code`.

## Use

```sh
lagom style                          # active styles
lagom style list                     # built-in, user and project styles
lagom style show terse               # print a style
lagom style use terse                # user default
lagom style use --project lean-code  # this project only (.lagom/config.json)
lagom style use off                  # no styles
```

## Write your own

A style is a Markdown file with `name` and `description` frontmatter; the body is injected as-is:

```markdown
---
name: team
description: Our review voice.
---
Answer first. Bullets only. Cite files as path:line. Never shorten error messages.
```

Lagom looks for style files in this order:
1. `.lagom/styles/<name>.md` in the project (the nearest parent folder wins).
2. `~/.config/lagom/styles/<name>.md`.
3. The built-in styles.

A file with a built-in name (`terse`, `lean-code`) replaces that style.

Active styles come from the first of these that is set:
1. `LAGOM_STYLES` (for example `LAGOM_STYLES=terse`).
2. `.lagom/config.json` in the project.
3. `~/.config/lagom/config.json`.

## Turn off

`lagom style use off` (or `--project` for one repo). `LAGOM_STYLES=off` for one session.

With the defaults the rules cost about 400 tokens per session; `lagom stats` shows the total.

## Real usage per style

`lagom stats --usage [--days N]` (default 30 days) reads Claude Code's transcripts and Codex's session logs and shows, per agent and style set: sessions, turns, the median output and input tokens per turn, and the share of input read from the prompt cache. Sessions without styles are the baseline; sessions an experiment held out are listed as `holdout (experiment)`. Sessions that ended before Lagom recorded session ids are listed apart, because whether they got styles is unknown. For Codex, matching sessions to styles assumes the hook's session id is the log's id; that is not verified yet. The numbers are observational: sessions differ in their work, so a difference is not proof of a style's effect.

## Measure a style

Styles cost tokens (the injected rules) and should save more than they cost (shorter answers, less code). To measure it on your own work:

```sh
lagom experiment start --holdout 0.2 --days 14   # about 20% of new sessions run without styles
lagom stats --experiment                         # styled vs holdout output tokens per turn, 95% interval
lagom experiment stop
```

Compression stays on in both groups, so holdout sessions still get the one line that explains the `[lagom: …]` recall footer; they lose only the styles. Each session is assigned by a hash of its id, so it and its subagents always share a group. Results need a few dozen sessions in each group before the interval is useful. Supported for Claude Code and Codex, whose logs record token usage; Cursor sessions always get styles. `lagom experiment stop` ends the experiment early and keeps its results for `lagom stats --experiment`. Turn off other tools that add answer rules at session start (Caveman, for example) while it runs: they act on both groups and blur the difference.
