//! The `lagom stats` report (GAIN-06): what compression saved, what the injected style
//! rules cost, output tokens per style, and the net, by agent and by day. Each section
//! says how its numbers were made.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::{Value, json};

use super::StyleUsage;

/// Bytes per estimated token. A rough rule for English text and code (CMP-10 calibrates it).
pub const BYTES_PER_TOKEN: u64 = 4;
const TOKENS: &str = "tokens ≈ bytes ÷ 4, an estimate";
const OUTPUT: &str = "median output tokens per main-conversation turn, from the agents' own logs; observational, so not proof of a style's effect";
const NET: &str = "tokens saved by compression minus tokens of injected style rules; a style's effect on answer length is not included";
/// Days shown in the text report's daily table; `--json` has every day.
const TEXT_DAYS: usize = 14;

/// Sums for one group: a command, a style, an agent, a day, or everything.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bucket {
    pub runs: u64,
    /// Runs whose output got smaller.
    pub compressed: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub injections: u64,
    pub injected_bytes: u64,
}

impl Bucket {
    fn add_run(&mut self, bytes_in: u64, bytes_out: u64) {
        self.runs += 1;
        self.compressed += u64::from(bytes_in > bytes_out);
        self.bytes_in += bytes_in;
        self.bytes_out += bytes_out;
    }

    fn add_injection(&mut self, bytes: u64) {
        self.injections += 1;
        self.injected_bytes += bytes;
    }

    pub fn saved_tokens(&self) -> u64 {
        self.bytes_in.saturating_sub(self.bytes_out) / BYTES_PER_TOKEN
    }

    pub fn injected_tokens(&self) -> u64 {
        self.injected_bytes / BYTES_PER_TOKEN
    }

    pub fn net_tokens(&self) -> i64 {
        self.saved_tokens() as i64 - self.injected_tokens() as i64
    }

    fn json(&self) -> Value {
        json!({
            "runs": self.runs, "compressed": self.compressed,
            "bytes_in": self.bytes_in, "bytes_out": self.bytes_out,
            "saved_tokens": self.saved_tokens(),
            "injections": self.injections, "injected_bytes": self.injected_bytes,
            "injected_tokens": self.injected_tokens(), "net_tokens": self.net_tokens(),
        })
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub total: Bucket,
    pub by_command: BTreeMap<String, Bucket>,
    pub by_style: BTreeMap<String, Bucket>,
    pub by_agent: BTreeMap<String, Bucket>,
    /// UTC days, `YYYY-MM-DD`.
    pub by_day: BTreeMap<String, Bucket>,
    /// Output tokens per style, per agent, over `output_days`.
    pub output: Vec<(String, Vec<StyleUsage>)>,
    pub output_days: u64,
}

impl Report {
    /// Adds one `stats.jsonl` event (see [`crate::store::for_each_stat`]).
    pub fn add(&mut self, v: &Value) {
        let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
        let agent = match v.get("agent").and_then(Value::as_str) {
            Some(a) => a,
            // `lagom run` typed by hand, or an event from before Lagom recorded agents.
            None if v.get("v").is_some() => "manual",
            None => "unknown",
        };
        let day = v.get("ts").and_then(Value::as_u64).map(utc_day);
        let mut groups: Vec<&mut Bucket> = vec![&mut self.total];
        groups.push(self.by_agent.entry(agent.to_string()).or_default());
        if let Some(day) = day {
            groups.push(self.by_day.entry(day).or_default());
        }
        match v.get("kind").and_then(Value::as_str) {
            Some("run") => {
                let cmd = v.get("cmd").and_then(Value::as_str).unwrap_or("unknown");
                groups.push(self.by_command.entry(cmd.to_string()).or_default());
                for g in groups {
                    g.add_run(n("in"), n("out"));
                }
            }
            Some("inject") => {
                let styles: Vec<&str> = v
                    .get("styles")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                let style = if styles.is_empty() {
                    "unknown".to_string()
                } else {
                    styles.join("+")
                };
                groups.push(self.by_style.entry(style).or_default());
                for g in groups {
                    g.add_injection(n("bytes"));
                }
            }
            _ => {}
        }
    }

    pub fn text(&self) -> String {
        let t = &self.total;
        let mut s = String::new();
        let _ = writeln!(s, "lagom stats, from ~/.lagom/stats.jsonl ({TOKENS})");
        let _ = writeln!(s, "\nCompression");
        let _ = writeln!(
            s,
            "commands run through lagom: {} ({} compressed)",
            t.runs, t.compressed
        );
        let _ = writeln!(
            s,
            "output: {} → {} bytes, saved ~{} tokens",
            t.bytes_in,
            t.bytes_out,
            t.saved_tokens()
        );
        if !self.by_command.is_empty() {
            let _ = writeln!(s, "by command:");
            let mut rows: Vec<_> = self.by_command.iter().collect();
            rows.sort_by(|a, b| {
                b.1.saved_tokens()
                    .cmp(&a.1.saved_tokens())
                    .then(a.0.cmp(b.0))
            });
            for (cmd, b) in rows {
                let _ = writeln!(
                    s,
                    "  {cmd:<12} {:>5} runs  {:>10} → {:>9} bytes  saved ~{} tokens",
                    b.runs,
                    b.bytes_in,
                    b.bytes_out,
                    b.saved_tokens()
                );
            }
        }

        let _ = writeln!(s, "\nInjection");
        let _ = writeln!(
            s,
            "style rules injected: {} times, ~{} tokens",
            t.injections,
            t.injected_tokens()
        );
        if !self.by_style.is_empty() {
            let _ = writeln!(s, "by style:");
            for (style, b) in &self.by_style {
                let _ = writeln!(
                    s,
                    "  {style:<20} {:>5} injections, ~{} tokens",
                    b.injections,
                    b.injected_tokens()
                );
            }
        }

        let _ = writeln!(s, "\nOutput, last {} days ({OUTPUT})", self.output_days);
        if self.output.iter().all(|(_, rows)| rows.is_empty()) {
            let _ = writeln!(s, "no Claude Code or Codex sessions found");
        }
        for (agent, rows) in self.output.iter().filter(|(_, r)| !r.is_empty()) {
            let _ = writeln!(s, "{agent}:");
            for g in rows {
                let _ = writeln!(
                    s,
                    "  {:<30} {:>6} output tokens/turn over {} turns, {} sessions",
                    g.style, g.median_output_per_turn, g.turns, g.sessions
                );
            }
        }

        let _ = writeln!(s, "\nNet ({NET})");
        let _ = writeln!(s, "net: ~{} tokens", t.net_tokens());
        if !self.by_agent.is_empty() {
            let _ = writeln!(s, "by agent:");
            for (agent, b) in &self.by_agent {
                let _ = writeln!(
                    s,
                    "  {agent:<8} net ~{} tokens: {} runs saved ~{}, {} injections cost ~{}",
                    b.net_tokens(),
                    b.runs,
                    b.saved_tokens(),
                    b.injections,
                    b.injected_tokens()
                );
            }
        }
        if !self.by_day.is_empty() {
            let _ = writeln!(s, "by day (last {TEXT_DAYS} with activity, UTC):");
            for (day, b) in self.by_day.iter().rev().take(TEXT_DAYS).rev() {
                let _ = writeln!(s, "  {day}  net ~{} tokens", b.net_tokens());
            }
        }
        s
    }

    pub fn json(&self) -> Value {
        let map = |m: &BTreeMap<String, Bucket>| -> Value {
            m.iter()
                .map(|(k, b)| (k.clone(), b.json()))
                .collect::<serde_json::Map<_, _>>()
                .into()
        };
        let output: serde_json::Map<String, Value> = self
            .output
            .iter()
            .map(|(agent, rows)| {
                let rows: Vec<Value> = rows
                    .iter()
                    .map(|g| {
                        json!({"style": g.style, "sessions": g.sessions, "turns": g.turns,
                               "median_output_per_turn": g.median_output_per_turn,
                               "median_input_per_turn": g.median_input_per_turn})
                    })
                    .collect();
                (agent.clone(), rows.into())
            })
            .collect();
        json!({
            "source": "~/.lagom/stats.jsonl",
            "compression": {"method": TOKENS, "total": self.total.json(), "by_command": map(&self.by_command)},
            "injection": {"method": TOKENS, "by_style": map(&self.by_style)},
            "output": {"method": OUTPUT, "days": self.output_days, "by_agent": output},
            "net": {"method": NET, "tokens": self.total.net_tokens(),
                    "by_agent": map(&self.by_agent), "by_day": map(&self.by_day)},
        })
    }
}

/// `YYYY-MM-DD` in UTC for unix seconds (the civil-from-days algorithm, no date crate).
pub fn utc_day(ts: u64) -> String {
    let z = (ts / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_days() {
        assert_eq!(utc_day(0), "1970-01-01");
        assert_eq!(utc_day(951_782_400), "2000-02-29");
        assert_eq!(utc_day(1_795_392_000), "2026-11-23"); // 2026-11-23T00:00:00Z
        assert_eq!(utc_day(1_795_391_999), "2026-11-22");
    }

    #[test]
    fn events_are_summed_by_command_style_agent_and_day() {
        let mut r = Report::default();
        for e in [
            json!({"v": 1, "ts": 1_795_478_500, "kind": "run", "cmd": "git", "in": 4000, "out": 400, "agent": "claude"}),
            json!({"v": 1, "ts": 1_795_478_600, "kind": "run", "cmd": "cargo", "in": 100, "out": 100}),
            json!({"v": 1, "ts": 1_795_391_000, "kind": "inject", "bytes": 1600, "styles": ["terse"], "agent": "claude"}),
            json!({"ts": 1, "kind": "inject", "bytes": 400}),
        ] {
            r.add(&e);
        }
        assert_eq!(r.total.runs, 2);
        assert_eq!(r.total.compressed, 1);
        assert_eq!(r.total.saved_tokens(), 900);
        assert_eq!(r.total.injected_tokens(), 500);
        assert_eq!(r.total.net_tokens(), 400);
        assert_eq!(r.by_command["git"].saved_tokens(), 900);
        assert_eq!(r.by_style["terse"].injections, 1);
        assert_eq!(r.by_style["unknown"].injections, 1);
        assert_eq!(r.by_agent["claude"].net_tokens(), 900 - 400);
        assert_eq!(r.by_agent["manual"].runs, 1);
        assert_eq!(r.by_agent["unknown"].injections, 1);
        assert_eq!(r.by_day["2026-11-24"].runs, 2);
        assert_eq!(r.by_day["2026-11-22"].injections, 1);
    }

    #[test]
    fn output_rows_are_shown_with_their_method() {
        let mut r = Report {
            output_days: 30,
            ..Report::default()
        };
        let text = r.text();
        assert!(text.contains("no Claude Code or Codex sessions found"));
        r.output.push((
            "Claude Code".into(),
            vec![StyleUsage {
                style: "terse+lean-code".into(),
                sessions: 3,
                turns: 40,
                median_output_per_turn: 410,
                median_input_per_turn: 9000,
                cache_read_share: 0.9,
                total: Default::default(),
            }],
        ));
        let text = r.text();
        assert!(text.contains("median output tokens per main-conversation turn"));
        assert!(
            text.contains("terse+lean-code")
                && text.contains("410 output tokens/turn over 40 turns, 3 sessions")
        );
        assert_eq!(
            r.json()["output"]["by_agent"]["Claude Code"][0]["median_output_per_turn"],
            410
        );
    }
}
