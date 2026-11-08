### Added
- JSON output (`gh api`, `kubectl -o json`, `jq`) and JSON Lines shorten arrays over 20 items to the first 3, the last 2 and every item that reports a problem (`status: "failed"`, `status >= 400`, `success: false`, a non-null `error`, `level: "error"` …), with a marker naming the dropped keys. Output stays valid, compact JSON.
