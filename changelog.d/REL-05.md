### Added
- `install.sh` installs the prebuilt `lagom` binary on macOS and Linux with one command. It checks the download against the release's `SHA256SUMS`, installs to `~/.local/bin` (or `LAGOM_INSTALL_DIR`) without sudo, takes `--version X.Y.Z`, and warns when the folder is not on `PATH`.
