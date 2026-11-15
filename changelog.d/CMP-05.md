### Fixed
- `lagom run` holds at most 64 MB of output in memory. Bigger output streams straight into the recall store, and the agent sees the start, the end and up to 400 error and warning lines from the middle. A 290 MB output peaked at 75 MB of memory.
- Finding error and warning lines no longer allocates per line, so large outputs process about 2.5× faster.
- If the spill file stops accepting writes (a full disk), `lagom run` keeps reading so the command finishes with its own exit code, and says the full output couldn't be stored. If reading the output fails, it says so instead of showing a shortened output as complete.
- `lagom recall` shows saved outputs that aren't valid UTF-8, and abandoned spill files from a killed run are removed after an hour.
