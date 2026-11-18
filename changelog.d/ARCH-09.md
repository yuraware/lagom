### Fixed
- Piping Lagom into a reader that stops early, such as `lagom recall <id> | head`, no longer panics with "Broken pipe"; Lagom stops quietly. `lagom run` still returns the command's exit code.
