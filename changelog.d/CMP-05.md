### Fixed
- `lagom run` holds at most 64 MB of output in memory. Bigger output streams straight into the recall store, and the agent sees the start, the end and up to 400 error and warning lines from the middle. A 290 MB output peaked at 75 MB of memory.
- Finding error and warning lines no longer allocates per line, so large outputs process about 2.5× faster.
