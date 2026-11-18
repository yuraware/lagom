### Added
- `compress.keep` in a config file lists plain substrings, such as `["SECURITY", "ACME-"]`, that make a line count as important to the compressor: it survives the line budget, isn't shortened and isn't folded into a repeat. The library adds `compress_keeping`.
