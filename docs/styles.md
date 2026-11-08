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

## Measure a style

Styles cost tokens (the injected rules) and should save more than they cost (shorter answers, less code). To measure it on your own work:

```sh
lagom experiment start --holdout 0.2 --days 14   # about 20% of new sessions run without styles
lagom stats --experiment                         # styled vs holdout output tokens per turn, 95% interval
lagom experiment stop
```

Compression stays on in both groups. Each session is assigned by a hash of its id, so it and its subagents always share a group. Results need a few dozen sessions in each group before the interval is useful. Supported for Claude Code and Codex, whose logs record token usage.
