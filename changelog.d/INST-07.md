### Added
- `lagom uninstall --all --purge` removes Lagom from every agent and deletes `~/.lagom` and `~/.config/lagom` (asks first; `--yes` in scripts), then prints the command that removes the binary. It deletes only folders Lagom created (a `.lagom-dir` file marks them), so a `LAGOM_HOME` pointing elsewhere is refused.
