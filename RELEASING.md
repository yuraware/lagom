# Releasing

For the maintainer. A release goes to five channels: GitHub Releases, `install.sh` (which reads them), the Homebrew tap, PyPI (`lagom-ai`) and crates.io. Steps in order; `X.Y.Z` is the new version.

Three steps can't be undone: the PyPI upload, `cargo publish` (only a yank) and the tap commit (only a new one). A release candidate tests everything before them.

## 1. Prepare `main`

1. Bump `version` in `Cargo.toml`, then `cargo build` so `Cargo.lock` follows. The wheel and the archives read the version from there.
2. `python3 -I scripts/release_notes.py X.Y.Z` merges `changelog.d/` into `CHANGELOG.md` and deletes the fragments. Read the new section; `--dry-run` prints it first.
3. `cargo publish --dry-run`.
4. Commit (`chore: release X.Y.Z`) and get it onto `main`. CI on that commit must be green: `test`, `msrv`, `latency` and `fidelity` (`changelog` runs on pull requests).

## 2. Release candidate

```sh
git tag vX.Y.Z-rc.1 && git push origin vX.Y.Z-rc.1
```

- `release` builds the four archives, checks the Arm Linux binary on an Arm runner, writes `SHA256SUMS` with a provenance attestation, and publishes a GitHub **pre-release**. The `homebrew` job skips pre-releases.
- `python` builds and tests the six wheels and the sdist and publishes nothing: the wheel version comes from `Cargo.toml`, so publishing would release the final version.
- Try the candidate: `curl -fsSL https://raw.githubusercontent.com/yuraware/lagom/main/install.sh | sh -s -- --version X.Y.Z-rc.1 --no-agents`, then `lagom --version`.

## 3. Release

```sh
git tag vX.Y.Z && git push origin vX.Y.Z
```

The tag must equal the version in `Cargo.toml`; both workflows stop otherwise.

1. **`release`**: the GitHub Release with the archives, `SHA256SUMS` and notes taken from `CHANGELOG.md`. Then its `homebrew` job commits `Formula/lagom.rb` to `yuraware/homebrew-tap`. Without the `HOMEBREW_TAP_TOKEN` secret it only warns: check its log.
2. **`python`**: its `publish` job waits for that GitHub Release (up to 60 minutes) and then uploads to PyPI through trusted publishing. If the `pypi` environment has reviewers, approve it in the run.
3. **crates.io**: once the GitHub Release exists (`cargo binstall` downloads its archives), `cargo publish` from the tagged commit.

## 4. Check every channel

On macOS and on Linux, each in a fresh shell:

```sh
curl -fsSL https://raw.githubusercontent.com/yuraware/lagom/main/install.sh | sh -s -- --no-agents
brew install yuraware/tap/lagom      # macOS
pip install lagom-ai                 # in a fresh venv
cargo binstall lagom
```

Each must print `lagom X.Y.Z` for `lagom --version` and `lgm --version`. The `smoke` workflow does this on both systems, plus `lagom install claude`, `lagom status` and `lagom uninstall` in a temporary home: `release` already ran it for `install.sh` and Homebrew, and after `cargo publish` you run it for all four channels with `gh workflow run smoke.yml -f version=X.Y.Z`. Also check:
- the tap's latest commit is `lagom X.Y.Z`;
- `https://pypi.org/project/lagom-ai/` and `https://crates.io/crates/lagom` show X.Y.Z;
- `gh attestation verify lagom-X.Y.Z-<target>.tar.gz --repo yuraware/lagom` passes for one archive.

## 5. After

- If a version has a security bug, yank it (`cargo yank lagom@<bad>`) and update `SECURITY.md`.
- Once GAIN-07 exists, update its price table if the model prices changed.

## When a step fails

- **Before the final tag:** fix on `main` and start again with a new `-rc.N` tag.
- **`release` fails after the final tag:** delete the tag and any draft release, fix, and tag again. Nothing has reached PyPI, the tap or crates.io yet.
- **`release` succeeded but PyPI failed:** fix `python.yml` and re-run its failed jobs on the same tag. The GitHub Release stays.
