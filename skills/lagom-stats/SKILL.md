---
name: lagom-stats
description: Show how much Lagom saved: command-output compression versus the size of injected style rules. Use when the user asks about Lagom savings, stats, or whether Lagom is worth it.
---
Run `lagom stats` and report its numbers in 2–4 lines: commands compressed, output saved, style rules injected, and the net result. Token figures are estimates (about 4 bytes per token); say so if the user asks for exact counts. If net savings are negative, say so plainly and suggest `lagom style use off` or fewer styles.
