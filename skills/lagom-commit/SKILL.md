---
name: lagom-commit
description: Write a short Conventional Commits message from the staged diff. Use when asked to write a commit message or commit changes concisely.
---
1. Read `git diff --staged` (if empty, `git diff` and say nothing is staged).
2. Write `type(scope): summary`: imperative mood, at most 72 characters, no trailing period. Types: feat, fix, refactor, perf, docs, test, build, ci, chore.
3. Add a body only if the reason is not obvious from the summary: 1–3 short lines on why, not what.
4. Mention breaking changes with a `BREAKING CHANGE:` footer.
Keep any trailer lines the user or repository requires. Commit only if the user asked to commit.
