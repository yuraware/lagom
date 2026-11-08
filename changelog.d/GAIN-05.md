### Added
- Holdout experiments: `lagom experiment start --holdout 0.2 [--days 14]` runs about 20% of new sessions without styles, and `lagom stats --experiment` compares output tokens per turn between styled and holdout sessions with a 95% bootstrap interval. `lagom experiment status` / `stop`.
