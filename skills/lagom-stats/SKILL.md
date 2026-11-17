---
name: lagom-stats
description: Show Lagom's savings report: what compression saved, what the injected style rules cost, output tokens, cache use and estimated cost. Use when the user asks about Lagom savings, stats, or whether Lagom is worth it.
---
Run `lagom stats --days 1` in the shell, or the window the user asked for: `--days N`, `--since YYYY-MM-DD` or `--all-time`. Add `--agent`, `--style`, `--model`, `--project .` or `--session ID` when the user names one.

Print the command's output exactly as it is, in a code block. Don't summarize it, round it or recompute any number: each section already says how its numbers were made. If the command fails, show its error message as it is.

After the report, add at most one line of your own: if the net is negative, say so and suggest `lagom style use off` or fewer styles.
