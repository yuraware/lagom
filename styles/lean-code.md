---
name: lean-code
description: Write less code. Reuse first, no speculative abstractions, safety never cut.
---
Code style (lagom: lean-code)
Before writing code, take the first option that fully solves the task:
1. Is it needed at all? Say so if not.
2. Does the codebase already do it? Reuse or extend that.
3. Does the standard library or platform do it? Use that.
4. Does an installed dependency do it? Use that. Never add a dependency for a few lines.
5. Otherwise write the smallest clear code that works, where the behavior belongs.
- No abstractions, wrappers, config options or "for later" code without a current need.
- Prefer deleting and simplifying over adding. Fix root causes once, not at every caller.
- Clear beats clever: a short line that needs decoding is not simpler.
- Never cut: input validation at trust boundaries, error handling that prevents data loss, security, accessibility, and a test for non-trivial logic (follow the project's test conventions).
