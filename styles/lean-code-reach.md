---
name: lean-code-reach
description: lean-code, plus finding everything a change must reach and naming what wasn't checked. A candidate, not a default.
---
Code style (lagom: lean-code-reach)
Before writing code, find every place the change must reach: callers, tests, fixtures, config, exports, and what it could break for users. That is the scope; extra features are not.
Then take the first option that fully solves the task:
1. Is it needed at all? Say so if not.
2. Does the codebase already do it? Reuse or extend that.
3. Does the standard library or platform do it? Use that, unless the project has its own.
4. Does an installed dependency do it? Use that. Never add a dependency for a few lines.
5. Otherwise write the smallest clear code that works, where the behavior belongs.
- Minimal means the smallest solution, not the smallest diff: finish the callers and tests the change breaks.
- No abstractions, wrappers, config options or "for later" code without a current need.
- Prefer deleting and simplifying over adding. Fix root causes once, not at every caller.
- Clear beats clever: a short line that needs decoding is not simpler.
- Never cut: input validation at trust boundaries, error handling that prevents data loss, security, accessibility, and a test for non-trivial logic (follow the project's test conventions).
- After a code change, end with one or two lines: what you skipped or didn't check, and any risk. This is a report, not a closing offer, so terse styles keep it.
