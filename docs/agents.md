# Agents

| Agent | Hooks file | Skills | Session start | Subagents | Command rewrite |
|---|---|---|---|---|---|
| Claude Code | `~/.claude/settings.json` | `~/.claude/skills/` | ✓ (also after `/clear`, compaction and fork) | ✓ | ✓ |
| Codex | `~/.codex/hooks.json` | `~/.agents/skills/` | ✓ (also after `/clear`, compaction and fork) | – | ✓ after `/hooks` trust |
| Cursor | `~/.cursor/hooks.json` | `~/.cursor/skills/` | ✓ (new chats only, see below) | – | ✓ |

`CLAUDE_CONFIG_DIR` and `CODEX_HOME` are respected.

## What Lagom registers

- **Session start** (and Claude Code's subagent start): injects the active styles.
- **Before a shell command** (Claude Code `Bash`, Codex `Bash`, Cursor `Shell`): rewrites simple commands to `lagom run -- …`. See [compression](compression.md).

**After compaction:** Claude Code and Codex run the session-start hook again when they compact a chat, so the styles stay. Cursor runs `sessionStart` only for a new chat, and its `preCompact` hook can't add context, so after Cursor compacts a long chat the styles may fade. Start a new chat to restore them.

Every hook fails open: if Lagom has a problem, it prints nothing (Cursor: `{}`) and the agent continues unchanged. `LAGOM_DISABLE=1` turns all hooks off without uninstalling.

## Skills

| Skill | What it does |
|---|---|
| `lagom` | Show or switch styles |
| `lagom-run` | Run a command with compressed output |
| `lagom-review` | Code review with one line per finding |
| `lagom-commit` | Short Conventional Commits message |
| `lagom-stats` | Show savings |

A skill folder that already exists and wasn't installed by Lagom is never overwritten.

More agents are planned (Gemini CLI, OpenCode, Copilot and others).
