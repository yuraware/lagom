//! The `lagom stats` report (GAIN-06): what compression saved, what the injected style
//! rules cost, output tokens per style, and the net, by agent and by day. Each section
//! says how its numbers were made.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

use serde_json::{Value, json};

use super::{SessionUsage, StyleUsage};

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

/// Which events and sessions count (GAIN-10). Dates are UTC; a session counts when its
/// last message falls in the window.
#[derive(Debug, Default)]
pub struct Filter {
    /// Unix seconds; `None` for all time.
    pub since: Option<u64>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub style: Option<String>,
    pub project: Option<String>,
    pub session: Option<String>,
}

/// What events don't say themselves, joined by session id: the style set injected into
/// the session (see [`crate::store::session_styles`]) and the model its transcript names.
#[derive(Debug, Default)]
pub struct Context {
    pub styles: HashMap<String, String>,
    pub models: HashMap<String, String>,
}

/// Every value seen, so an unknown filter value can be answered with the known ones.
#[derive(Debug, Default)]
pub struct Known {
    agents: BTreeSet<String>,
    models: BTreeSet<String>,
    styles: BTreeSet<String>,
    projects: BTreeSet<String>,
    sessions: BTreeSet<String>,
}

fn event_agent(v: &Value) -> &str {
    match v.get("agent").and_then(Value::as_str) {
        Some(a) => a,
        // `lagom run` typed by hand, or an event from before Lagom recorded agents.
        None if v.get("v").is_some() => "manual",
        None => "unknown",
    }
}

/// The style set of an `inject` event, or of the event's session.
fn event_style(v: &Value, ctx: &Context) -> Option<String> {
    if v.get("kind").and_then(Value::as_str) == Some("inject") {
        let styles: Vec<&str> = v
            .get("styles")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        return Some(if styles.is_empty() {
            "unknown".into()
        } else {
            styles.join("+")
        });
    }
    let session = v.get("session").and_then(Value::as_str)?;
    ctx.styles.get(session).cloned()
}

impl Filter {
    fn event_matches(&self, v: &Value, ctx: &Context) -> bool {
        let field = |k: &str| v.get(k).and_then(Value::as_str);
        let session = field("session");
        self.since.is_none_or(|t| {
            v.get("ts")
                .and_then(Value::as_u64)
                .is_some_and(|ts| ts >= t)
        }) && self.agent.as_deref().is_none_or(|a| event_agent(v) == a)
            && self
                .project
                .as_deref()
                .is_none_or(|p| field("project") == Some(p))
            && self.session.as_deref().is_none_or(|id| session == Some(id))
            && self
                .style
                .as_deref()
                .is_none_or(|st| event_style(v, ctx).as_deref() == Some(st))
            && self.model.as_deref().is_none_or(|m| {
                session
                    .and_then(|id| ctx.models.get(id))
                    .map(String::as_str)
                    == Some(m)
            })
    }

    fn session_matches(
        &self,
        s: &SessionUsage,
        ctx: &Context,
        project_sessions: &HashSet<String>,
    ) -> bool {
        let last = s.last_ts.as_deref().and_then(super::iso_to_unix);
        self.since.is_none_or(|t| last.is_some_and(|l| l >= t))
            && self.agent.as_deref().is_none_or(|a| s.agent == a)
            && self
                .model
                .as_deref()
                .is_none_or(|m| s.model.as_deref() == Some(m))
            && self.session.as_deref().is_none_or(|id| s.session_id == id)
            && self
                .style
                .as_deref()
                .is_none_or(|st| ctx.styles.get(&s.session_id).map(String::as_str) == Some(st))
            && (self.project.is_none() || project_sessions.contains(&s.session_id))
    }

    /// An error naming the known values when a filter value never occurs.
    pub fn check(&self, known: &Known) -> Result<(), String> {
        for (flag, value, seen) in [
            ("--agent", &self.agent, &known.agents),
            ("--model", &self.model, &known.models),
            ("--style", &self.style, &known.styles),
            ("--project", &self.project, &known.projects),
            ("--session", &self.session, &known.sessions),
        ] {
            if let Some(v) = value.as_deref().filter(|v| !seen.contains(*v)) {
                let list: Vec<&str> = seen.iter().map(String::as_str).collect();
                let list = if list.is_empty() {
                    "none recorded".to_string()
                } else {
                    list.join(", ")
                };
                return Err(format!(
                    "no {flag} `{v}` in the stats or the agents' logs; known: {list}"
                ));
            }
        }
        Ok(())
    }

    /// The header text for this window and these filters.
    pub fn scope(&self, days: Option<u64>) -> String {
        let window = match (self.since, days) {
            (None, _) => "all time".to_string(),
            (Some(t), Some(d)) => format!("UTC, since {} (last {d} days)", utc_day(t)),
            (Some(t), None) => format!("UTC, since {}", utc_day(t)),
        };
        let filters = self.describe();
        if filters.is_empty() {
            window
        } else {
            format!("{window}, {}", filters.join(", "))
        }
    }

    /// The filters that are set, as `name=value`.
    fn describe(&self) -> Vec<String> {
        [
            ("agent", &self.agent),
            ("model", &self.model),
            ("style", &self.style),
            ("project", &self.project),
            ("session", &self.session),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}={v}")))
        .collect()
    }
}

#[derive(Debug, Default)]
pub struct Report {
    /// The window and filters, for the header: `UTC, since 2026-10-18 (last 30 days)`.
    pub scope: String,
    pub known: Known,
    /// Sessions seen in events of the `--project` filter, to filter the output section.
    project_sessions: HashSet<String>,
    pub total: Bucket,
    pub by_command: BTreeMap<String, Bucket>,
    pub by_style: BTreeMap<String, Bucket>,
    pub by_agent: BTreeMap<String, Bucket>,
    /// UTC days, `YYYY-MM-DD`.
    pub by_day: BTreeMap<String, Bucket>,
    /// Output tokens per style, per agent, for the sessions in the window.
    pub output: Vec<(String, Vec<StyleUsage>)>,
}

impl Report {
    /// Adds a `stats.jsonl` event when it passes `filter`, and notes its values.
    pub fn add_filtered(&mut self, v: &Value, filter: &Filter, ctx: &Context) {
        let field = |k: &str| v.get(k).and_then(Value::as_str).map(String::from);
        self.known.agents.insert(event_agent(v).to_string());
        self.known.projects.extend(field("project"));
        self.known.sessions.extend(field("session"));
        self.known.styles.extend(event_style(v, ctx));
        self.known
            .models
            .extend(field("session").and_then(|id| ctx.models.get(&id).cloned()));
        if filter.event_matches(v, ctx) {
            if filter.project.is_some() {
                self.project_sessions.extend(field("session"));
            }
            self.add(v);
        }
    }

    /// Adds an agent's sessions that pass `filter` to the output section.
    pub fn add_sessions(
        &mut self,
        agent: &str,
        sessions: Vec<SessionUsage>,
        filter: &Filter,
        ctx: &Context,
        tracked_since: Option<u64>,
    ) {
        for s in &sessions {
            self.known.agents.insert(s.agent.clone());
            self.known.models.extend(s.model.clone());
            self.known.sessions.insert(s.session_id.clone());
            self.known
                .styles
                .extend(ctx.styles.get(&s.session_id).cloned());
        }
        let kept: Vec<SessionUsage> = sessions
            .into_iter()
            .filter(|s| filter.session_matches(s, ctx, &self.project_sessions))
            .collect();
        let rows = super::by_style(&kept, &ctx.styles, tracked_since);
        self.output.push((agent.to_string(), rows));
    }

    /// Adds one `stats.jsonl` event, unfiltered.
    pub fn add(&mut self, v: &Value) {
        let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
        let agent = event_agent(v);
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
        let _ = writeln!(
            s,
            "lagom stats, {}, from ~/.lagom/stats.jsonl ({TOKENS})",
            self.scope
        );
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

        let _ = writeln!(s, "\nOutput ({OUTPUT})");
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
            let _ = writeln!(s, "by day (the last {TEXT_DAYS} with activity, UTC):");
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
            "scope": self.scope,
            "compression": {"method": TOKENS, "total": self.total.json(), "by_command": map(&self.by_command)},
            "injection": {"method": TOKENS, "by_style": map(&self.by_style)},
            "output": {"method": OUTPUT, "by_agent": output},
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
        let mut r = Report::default();
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
