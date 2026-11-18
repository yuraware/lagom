//! Prices per million tokens (GAIN-07), for the estimated costs in `lagom stats`. Output
//! costs about five times input and a cache read a fraction of it, so a net number needs
//! these. Update the table at every release (RELEASING.md).

use std::path::Path;

use serde_json::Value;

use super::Usage;

/// Where the built-in prices come from, and when they were last checked.
pub const SOURCE: &str = "https://platform.claude.com/docs/en/about-claude/pricing";
pub const CHECKED: &str = "2026-11-17";

/// US dollars per million tokens. Cache writes use the 5-minute price, the default
/// lifetime: the agents' logs don't say which lifetime a write had.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

const fn price(input: f64, output: f64, cache_write: f64, cache_read: f64) -> Price {
    Price {
        input,
        output,
        cache_write,
        cache_read,
    }
}

/// Anthropic's first-party API prices, from [`SOURCE`] on [`CHECKED`]. Model ids match by
/// the longest prefix, so `claude-sonnet-4-5-20250929` finds `claude-sonnet-4-5`. Claude
/// Haiku 5.5 is listed at its price for prompts up to 100,000 tokens: session totals can't
/// tell longer prompts apart.
const BUILT_IN: &[(&str, Price)] = &[
    ("claude-fable-5-2", price(8.0, 40.0, 10.0, 0.20)),
    ("claude-mythos-5-2", price(8.0, 40.0, 10.0, 0.20)),
    ("claude-fable-5-1", price(10.0, 50.0, 12.50, 0.25)),
    ("claude-mythos-5-1", price(10.0, 50.0, 12.50, 0.25)),
    ("claude-fable-5", price(10.0, 50.0, 12.50, 1.0)),
    ("claude-mythos-5", price(10.0, 50.0, 12.50, 1.0)),
    ("claude-opus-5-5", price(4.0, 20.0, 5.0, 0.20)),
    ("claude-opus-5", price(5.0, 25.0, 6.25, 0.50)),
    ("claude-opus-4-8", price(5.0, 25.0, 6.25, 0.50)),
    ("claude-opus-4-7", price(5.0, 25.0, 6.25, 0.50)),
    ("claude-opus-4-6", price(5.0, 25.0, 6.25, 0.50)),
    ("claude-opus-4-5", price(5.0, 25.0, 6.25, 0.50)),
    ("claude-opus-4-1", price(15.0, 75.0, 18.75, 1.50)),
    ("claude-opus-4", price(15.0, 75.0, 18.75, 1.50)),
    ("claude-sonnet-5-5", price(2.0, 10.0, 2.50, 0.10)),
    ("claude-sonnet-5", price(2.0, 10.0, 2.50, 0.20)),
    ("claude-sonnet-4-6", price(3.0, 15.0, 3.75, 0.30)),
    ("claude-sonnet-4-5", price(3.0, 15.0, 3.75, 0.30)),
    ("claude-sonnet-4", price(3.0, 15.0, 3.75, 0.30)),
    ("claude-haiku-5-5", price(0.10, 0.50, 0.125, 0.01)),
    ("claude-haiku-4-5", price(1.0, 5.0, 1.25, 0.10)),
];

/// The built-in table plus the user's `~/.config/lagom/prices.json`, which adds models (for
/// example the ones Codex uses) or replaces built-in prices:
/// `{"gpt-6.1-sol": {"input": 1.25, "output": 10, "cache_write": 1.25, "cache_read": 0.125}}`.
#[derive(Debug)]
pub struct Prices {
    table: Vec<(String, Price)>,
    /// Entries of the user's file that were ignored, and why.
    pub problems: Vec<String>,
    /// Whether the user's file was read.
    pub overridden: bool,
}

impl Default for Prices {
    fn default() -> Self {
        Prices {
            table: BUILT_IN.iter().map(|(m, p)| (m.to_string(), *p)).collect(),
            problems: Vec::new(),
            overridden: false,
        }
    }
}

impl Prices {
    /// The built-in table with the user's overrides from `<config dir>/prices.json`.
    pub fn load(config_dir: &Path) -> Prices {
        let mut prices = Prices::default();
        let path = config_dir.join("prices.json");
        match crate::config::read_file(&path) {
            Ok(None) => {}
            Ok(Some(map)) => {
                prices.overridden = true;
                for (model, v) in map {
                    match parse(&v) {
                        Some(p) => prices.set(&model, p),
                        None => prices.problems.push(format!(
                            "{}: `{model}` needs input, output, cache_write and cache_read as numbers",
                            path.display()
                        )),
                    }
                }
            }
            Err(e) => prices.problems.push(e),
        }
        prices
    }

    fn set(&mut self, model: &str, p: Price) {
        match self.table.iter_mut().find(|(m, _)| m == model) {
            Some(entry) => entry.1 = p,
            None => self.table.push((model.to_string(), p)),
        }
    }

    /// The price for a model id: an exact entry, else the longest entry it starts with.
    pub fn lookup(&self, model: &str) -> Option<Price> {
        self.table
            .iter()
            .filter(|(m, _)| model == m || model.starts_with(&format!("{m}-")))
            .max_by_key(|(m, _)| m.len())
            .map(|(_, p)| *p)
    }

    /// Estimated cost in US dollars of `usage` on `model`, or `None` when its price is unknown.
    pub fn cost(&self, model: &str, usage: &Usage) -> Option<f64> {
        let p = self.lookup(model)?;
        let tokens = [
            (usage.input, p.input),
            (usage.output, p.output),
            (usage.cache_write, p.cache_write),
            (usage.cache_read, p.cache_read),
        ];
        Some(
            tokens
                .iter()
                .map(|(n, per_million)| *n as f64 * per_million / 1e6)
                .sum(),
        )
    }
}

fn parse(v: &Value) -> Option<Price> {
    let n = |k: &str| v.get(k)?.as_f64().filter(|x| x.is_finite() && *x >= 0.0);
    Some(price(
        n("input")?,
        n("output")?,
        n("cache_write")?,
        n("cache_read")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64, cache_write: u64, cache_read: u64) -> Usage {
        Usage {
            input,
            output,
            cache_write,
            cache_read,
            messages: 1,
        }
    }

    #[test]
    fn cost_matches_a_manual_calculation() {
        let prices = Prices::default();
        // Claude Opus 5.5: $4 input, $20 output, $5 cache write, $0.20 cache read per million.
        // 1,000 × 4 + 2,000 × 20 + 3,000 × 5 + 400,000 × 0.20 = 139,000 per million = $0.139.
        let cost = prices
            .cost("claude-opus-5-5", &usage(1_000, 2_000, 3_000, 400_000))
            .unwrap();
        assert!((cost - 0.139).abs() < 1e-9, "{cost}");
        // Claude Sonnet 4.5 under its dated id: 10,000 × 3 + 1,000 × 15 = 45,000 per million.
        let cost = prices
            .cost("claude-sonnet-4-5-20250929", &usage(10_000, 1_000, 0, 0))
            .unwrap();
        assert!((cost - 0.045).abs() < 1e-9, "{cost}");
        assert_eq!(prices.cost("gpt-6.1-sol", &usage(1, 1, 1, 1)), None);
    }

    #[test]
    fn the_longest_matching_entry_wins() {
        let prices = Prices::default();
        assert_eq!(prices.lookup("claude-sonnet-5-5").unwrap().cache_read, 0.10);
        assert_eq!(prices.lookup("claude-sonnet-5").unwrap().cache_read, 0.20);
        assert_eq!(prices.lookup("claude-opus-4-20250514").unwrap().input, 15.0);
        assert_eq!(prices.lookup("claude-opus-4-8").unwrap().input, 5.0);
        assert_eq!(prices.lookup("claude-opus-4x"), None);
    }

    #[test]
    fn the_users_file_adds_and_replaces_prices() {
        let dir = std::env::temp_dir().join(format!("lagom-prices-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("prices.json"),
            r#"{"gpt-6.1-sol": {"input": 1.25, "output": 10, "cache_write": 1.25, "cache_read": 0.125},
                "claude-opus-5-5": {"input": 1, "output": 2, "cache_write": 3, "cache_read": 4},
                "broken": {"input": "cheap"}}"#,
        )
        .unwrap();
        let prices = Prices::load(&dir);
        assert!(prices.overridden);
        assert_eq!(prices.lookup("gpt-6.1-sol").unwrap().output, 10.0);
        assert_eq!(prices.lookup("claude-opus-5-5").unwrap().cache_read, 4.0);
        assert_eq!(prices.lookup("broken"), None);
        assert_eq!(prices.problems.len(), 1, "{:?}", prices.problems);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
