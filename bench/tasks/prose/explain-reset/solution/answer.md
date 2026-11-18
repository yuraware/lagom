It moves your current branch back one commit, to `HEAD~1`, and makes the index and the working tree match that commit.

You can lose two things:

- The last commit: it's no longer on the branch. `git reflog` can still find it for a while, so it's recoverable.
- Every uncommitted change, staged or not: these are discarded and can't be recovered.

Run `git stash` or commit first if you want to keep your local changes.
