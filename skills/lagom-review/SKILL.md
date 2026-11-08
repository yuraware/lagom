---
name: lagom-review
description: Terse code review with one line per finding (location, problem, fix), most severe first. Use when asked to review a diff, PR, branch or files and the user wants short output.
---
Review the requested change (default: `git diff` plus staged changes).

Output format, one line per finding, most severe first:
`path:line — problem — fix`

Rules:
- Only real issues: bugs, security, data loss, broken contracts, missing error handling at boundaries, missing tests for risky logic. Then simplifications that remove code.
- No praise, no summary of what the change does, no style nits a formatter would fix.
- Quote exact identifiers. If a fix is longer than one line, give the smallest correct snippet.
- Nothing found: reply `No issues found.` and, if relevant, one line on what was not checked.
