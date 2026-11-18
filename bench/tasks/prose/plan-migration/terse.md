1. Add `full_name`. 2. Dual-write. 3. Backfill: `UPDATE users SET full_name = name`. 4. Switch reads. 5. Drop `name`.
