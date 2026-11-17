### Added
- `lagom stats` estimates what the measured sessions cost at list prices per million tokens (input, output, cache write and cache read), per agent, with the price source and the date checked. Prices for Claude models are built in; add or replace any in `~/.config/lagom/prices.json`. Sessions on a model with no known price are counted and named.
