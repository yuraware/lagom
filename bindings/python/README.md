# lagom-ai

The [Lagom](https://github.com/yuraware/lagom) command-line tool, packaged for Python installers.

Lagom saves tokens in coding agents (Claude Code, Codex, Cursor): it injects output styles, compresses command output and adds skills. It is a single Rust binary; this package only ships that binary, the same way `ruff` does. There is no Python import yet.

## Install

```sh
pipx install lagom-ai        # or: uv tool install lagom-ai
lagom install                # wire Lagom into the agents it finds
```

Both `lagom` and its short alias `lgm` are put on your `PATH`.

Use a persistent install such as `pipx` or `uv tool install`. Agent hooks call the `lagom` binary by its path, so a throwaway environment (for example `uvx`) is fine for trying a command, but don't run `lagom install` from one: the hooks would break when the cache is cleared.

```sh
uvx --from lagom-ai lagom --version   # try it without installing
```

## Remove

```sh
lagom uninstall           # remove hooks and skills from every agent first
pipx uninstall lagom-ai
```

## Platforms

Wheels are built for macOS (Apple silicon and Intel) and Linux (manylinux and musllinux, x86_64 and aarch64). On other platforms pip falls back to the source distribution, which needs a Rust toolchain.

## License

Apache-2.0. See the [repository](https://github.com/yuraware/lagom) for the source, docs and changelog.
