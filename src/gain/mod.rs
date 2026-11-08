//! Real token usage from agents' own session logs (for `lagom stats`).

pub mod claude;
pub mod codex;
pub mod experiment;

use std::collections::{BTreeMap, HashMap};

/// Token counts as the provider reported them.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    /// Assistant messages counted (after removing streaming duplicates).
    pub messages: u64,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_write += other.cache_write;
        self.cache_read += other.cache_read;
        self.messages += other.messages;
    }
}

/// One agent session's usage.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SessionUsage {
    /// `claude` or `codex`.
    pub agent: String,
    pub session_id: String,
    /// Model of the last counted message.
    pub model: Option<String>,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
    /// The main conversation.
    pub main: Usage,
    /// Subagent (sidechain) messages.
    pub subagents: Usage,
    /// Per main-conversation turn: (input incl. cache, output) tokens, in order.
    pub turns: Vec<(u64, u64)>,
}

impl SessionUsage {
    pub fn total(&self) -> Usage {
        let mut t = self.main;
        t.add(&self.subagents);
        t
    }
}

/// Usage of all sessions that ran with one style set (or none).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StyleUsage {
    /// Style set such as `terse+lean-code`, or [`NO_STYLES`].
    pub style: String,
    pub sessions: usize,
    pub turns: usize,
    /// Median over main-conversation turns.
    pub median_output_per_turn: u64,
    pub median_input_per_turn: u64,
    /// Share of input tokens read from the prompt cache, 0.0-1.0.
    pub cache_read_share: f64,
    pub total: Usage,
}

/// Label for sessions Lagom injected no styles into.
pub const NO_STYLES: &str = "no styles injected";

fn median(mut v: Vec<u64>) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        v[mid]
    } else {
        (v[mid - 1] + v[mid]) / 2
    }
}

/// Group sessions by the style set Lagom injected (`styles`: session id → style key).
/// Observational: different sessions do different work, so differences between
/// styles are not proof of an effect (holdout experiments are, see GAIN-05).
pub fn by_style(sessions: &[SessionUsage], styles: &HashMap<String, String>) -> Vec<StyleUsage> {
    let mut groups: BTreeMap<&str, Vec<&SessionUsage>> = BTreeMap::new();
    for s in sessions {
        let key = styles.get(&s.session_id).map_or(NO_STYLES, String::as_str);
        groups.entry(key).or_default().push(s);
    }
    groups
        .into_iter()
        .map(|(style, group)| {
            let mut total = Usage::default();
            let (mut inputs, mut outputs) = (Vec::new(), Vec::new());
            for s in &group {
                total.add(&s.total());
                for &(i, o) in &s.turns {
                    inputs.push(i);
                    outputs.push(o);
                }
            }
            let input_all = total.input + total.cache_write + total.cache_read;
            StyleUsage {
                style: style.to_string(),
                sessions: group.len(),
                turns: outputs.len(),
                median_output_per_turn: median(outputs),
                median_input_per_turn: median(inputs),
                cache_read_share: if input_all == 0 {
                    0.0
                } else {
                    total.cache_read as f64 / input_all as f64
                },
                total,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, turns: &[(u64, u64)]) -> SessionUsage {
        let mut s = SessionUsage {
            session_id: id.into(),
            turns: turns.to_vec(),
            ..Default::default()
        };
        for &(i, o) in turns {
            s.main.add(&Usage {
                input: 0,
                output: o,
                cache_write: 0,
                cache_read: i,
                messages: 1,
            });
        }
        s
    }

    #[test]
    fn groups_sessions_by_style_with_medians() {
        let sessions = vec![
            session("a", &[(1000, 100), (2000, 300), (3000, 200)]),
            session("b", &[(1000, 50)]),
            session("c", &[(500, 400), (500, 600)]),
        ];
        let styles = HashMap::from([
            ("a".to_string(), "terse+lean-code".to_string()),
            ("b".to_string(), "terse+lean-code".to_string()),
        ]);
        let r = by_style(&sessions, &styles);
        assert_eq!(r.len(), 2);
        let none = r.iter().find(|s| s.style == NO_STYLES).unwrap();
        assert_eq!(
            (none.sessions, none.turns, none.median_output_per_turn),
            (1, 2, 500)
        );
        let terse = r.iter().find(|s| s.style == "terse+lean-code").unwrap();
        assert_eq!((terse.sessions, terse.turns), (2, 4));
        assert_eq!(terse.median_output_per_turn, (100 + 200) / 2); // 50,100,200,300
        assert_eq!(terse.median_input_per_turn, 1500); // 1000,1000,2000,3000
        assert_eq!(terse.total.output, 650);
        assert!((terse.cache_read_share - 1.0).abs() < 1e-9);
    }
}
