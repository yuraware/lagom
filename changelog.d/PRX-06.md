### Added
- `lagom::json_splice` finds JSON values by path (`messages[12].content[0].content`) and replaces string values in place, leaving every other byte of the body unchanged, so the proxy can shrink requests without breaking the prompt cache.
