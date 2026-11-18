Treat the key as leaked: anyone with the code, or the repository's history, has it.

1. Revoke or rotate it with the provider now, before anything else.
2. Load the new key from an environment variable (`os.environ["API_KEY"]`) or a secret manager, never from the source.
3. If the repository is shared, remove the old key from git history (for example with `git filter-repo`) and add a secret scanner to CI so it doesn't happen again.
