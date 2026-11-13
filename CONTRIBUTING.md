# Contributing

Thanks for helping. Lagom is small on purpose, so keep changes focused. [docs/dev/testing.md](docs/dev/testing.md) explains how to run every kind of test.

## Before you open a pull request

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass.
- New behaviour has a test; a bug fix has a test that failed before the fix.
- User-visible changes add a fragment in `changelog.d/` (see [changelog.d/README.md](changelog.d/README.md)).
- No new crate unless the project's dependency budget lists it.
- One commit message line, in [Conventional Commits](https://www.conventionalcommits.org/) form.

## Rules that are never relaxed

1. **Fail open.** If Lagom has a problem, the agent's command or request runs unchanged.
2. **Never hide a failure.** Every error line is either shown or counted in a visible notice that gives the `lagom recall` command; anything dropped is recoverable with `lagom recall`.
3. **Never widen permissions.** A rewrite never grants what the user hasn't; deny and ask rules win.

## Review checklist: text Lagom shows to the model

Everything Lagom injects or adds to output is read by the model as context. Check each change against this list:

- [ ] **Injected text stays in one `<lagom> … </lagom>` block.** Style and project text goes through `neutralize_tags`, so no form of the tag (`</lagom >`, `< /LAGOM>`, `<lagom attr>`) can open or close the block.
- [ ] **Markers are plain status, not instructions.** Write `[lagom: 400 → 90 lines; full output: lagom recall <id>]`, never "you must…", "ignore…", "system:" or anything that imitates the agent's own prompts.
- [ ] **Nothing is hidden from the user.** Rewritten commands stay visible in the agent's UI, as `lagom run -- <command>`; no silent edits to what the agent or the user sees.
- [ ] **No new text sources without trust.** Don't add a way for a repository's files to reach the model or turn a feature on. Turning something *off* is the only thing an untrusted file may do. Known exception until project trust lands: project styles and a project's style list are used as they are.
- [ ] **Output content is data.** Lagom never interprets text inside command output as configuration or commands.
- [ ] **Size stays small.** Agents cap how much hook output reaches the model, so keep injected text short and markers to one line. Lagom doesn't enforce a cap on style text yet.
