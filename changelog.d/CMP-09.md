### Added
- Long timestamped logs (app logs, access logs, `docker logs`, `kubectl logs`, syslog) collapse lines that differ only in numbers, times and ids into the first and last example plus `[×N lines like this: …]`. ERROR/WARN/FATAL/CRIT lines and HTTP 5xx responses are never collapsed. When more of them remain than fit, error, CRIT, SEVERE, ALERT and 5xx lines are kept before warnings, and any left out are counted. `ls -l` listings are not treated as logs.

### Fixed
- Access logs are no longer mistaken for search results (a "path" in search output can't contain spaces).
