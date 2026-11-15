### Fixed
- Compressing output that Lagom already compressed changes nothing, so recompressing (for example a nested `lagom run`) never drops more lines.
