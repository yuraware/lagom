### Added
- Search results (`rg -n`, `grep -rn`, `git grep -n`) are grouped by file: the first matches of the first 20 files in full, every other file still named with its match count. Output with any error line keeps the general compressor, so compiler errors are never cut.
