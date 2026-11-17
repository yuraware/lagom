# Security policy

## Report a vulnerability

Report it privately through GitHub: [Security → Report a vulnerability](https://github.com/yuraware/lagom/security/advisories/new). Please don't open a public issue. You'll get a reply within 72 hours.

Include the output of `lagom --version`, the agent and its version (Claude Code, Codex, Cursor), what happened and how to reproduce it.

Lagom runs inside coding agents, so these matter most:
- a rewritten command that runs when your own permission rules wouldn't allow it;
- a hook that exposes data, such as the saved outputs in `~/.lagom/recall/`;
- a project file that changes what Lagom does in a repository you haven't trusted.

## Supported versions

Only the latest minor version gets security fixes. Today that is 0.1.x, from 0.1.1 on.

## Fixed

**0.1.0** (2026-11-07) had three permission bugs in Claude Code's command rewrite:
- a rule without `*` also matched the command with extra arguments;
- `Bash(x:*)` matched without a word boundary, so it also matched `xyz`;
- allow rules from a project's committed `.claude/settings.json` were used, although Claude Code applies them only after you trust the folder.

All three are fixed in **0.1.1**, and 0.1.0 is yanked from crates.io. Upgrade with `cargo install lagom --force` or `brew upgrade lagom`, then run `lagom install` again.
