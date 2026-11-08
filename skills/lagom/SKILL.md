---
name: lagom
description: Show or change Lagom's answer and code styles (terse answers, lean code, custom styles). Use when the user says /lagom, "lagom on/off", "be terse", "less verbose", "normal mode", or asks to switch, list, or create a style.
---
Lagom styles are Markdown rules injected at session start. Manage them with the `lagom` CLI:

- Show active styles and where they come from: `lagom style`
- List all styles: `lagom style list`
- Switch for this user: `lagom style use terse lean-code` (any names, space-separated)
- Switch for this project only: `lagom style use --project terse`
- Turn off: `lagom style use off`

New styles: create `.lagom/styles/<name>.md` (project) or `~/.config/lagom/styles/<name>.md` (user) with frontmatter `name` and `description`, then the rules as the body. A project or user file with a built-in name (`terse`, `lean-code`) overrides it.

Changes apply from the next session. To apply now, follow the new style's rules from this point on (read it with `lagom style show <name>`).

Reply with one line: which styles are active now.
