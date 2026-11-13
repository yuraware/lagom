### Added
- `lagom recall <id>` can print only part of a saved output: `--grep TEXT` (matching lines with their line numbers; lowercase TEXT matches any case), `--lines A-B`, `--head N`, `--tail N`. The compression footer and the `lagom-run` skill point to them, so agents fetch only what they need.
