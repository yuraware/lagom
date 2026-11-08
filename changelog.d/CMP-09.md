### Added
- Long timestamped logs (app logs, access logs, `docker logs`, `kubectl logs`, syslog) collapse lines that differ only in numbers, times and ids into the first and last example plus `[×N lines like this: …]`. ERROR/WARN/FATAL lines and HTTP 5xx responses are always kept verbatim.

### Fixed
- Access logs are no longer mistaken for search results (a "path" in search output can't contain spaces).
