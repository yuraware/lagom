# Changelog fragments

Every change that users would notice adds one file here instead of editing `CHANGELOG.md`, so
parallel branches never conflict. Name it after the task or branch, for example `HK-02.md`:

```markdown
### Fixed
- Hooks no longer break after `brew upgrade`.
```

Allowed headings ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/) order): `Added`,
`Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`. A fragment may use several.

At release time `python3 -I scripts/release_notes.py <version>` merges every fragment into a new
`CHANGELOG.md` section and deletes them. CI fails a pull request that changes `src/` without a
fragment, unless it has the `no-changelog` label.
