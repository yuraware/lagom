### Changed
- Every command (except the agent hooks and the command after `lagom run --`) stops with exit code 2 and its usage on an unknown flag or an extra argument, instead of ignoring it. `lagom recall --clear` no longer runs when given anything else, and `lagom stats --experiment --days 3` is refused instead of dropping `--days`.
