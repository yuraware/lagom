### Changed
- Compression is about twice as fast: 1 MB of mixed real output in about 16 ms (was 32 ms), without changing any compressed result.

### Fixed
- Escape sequences such as `ESC ( B` (character set) are removed completely; before, one stray character was left behind.
- An escape sequence with no terminator (a broken window title, a cut-off color code) is removed only up to the end of its line; before, it swallowed the rest of the output.
