### Changed
- Output that contains error lines (`error:`, `fatal:`, a panic, a traceback, `FAILED`, `npm ERR!` …) gets the larger failure budget even when the command exits 0, and the footer says so. Summaries such as "0 failed" don't count.
