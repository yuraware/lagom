1. Add the new column: `ALTER TABLE users ADD COLUMN full_name text;` (no default, so it's instant).
2. Deploy app code that writes both `name` and `full_name`, still reading `name`.
3. Backfill in batches: `UPDATE users SET full_name = name WHERE full_name IS NULL AND id BETWEEN ...;`.
4. Add `NOT NULL` to `full_name` once every row has it (via a `CHECK ... NOT VALID` constraint, then `VALIDATE`).
5. Deploy code that reads and writes only `full_name`.
6. Drop the old column: `ALTER TABLE users DROP COLUMN name;`.
