//! Codex session logs: `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl` (default
//! `~/.codex`), one event per line.
//!
//! Usage comes from `token_usage_record` lines (one per API response, keyed by
//! `response_id`). Older logs only have `event_msg` / `token_count` events: then each
//! event's `last_token_usage` counts, skipping events whose running total didn't move.
//! OpenAI-style `input_tokens` **include** `cached_input_tokens`, so uncached input is
//! the difference.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use serde_json::Value;

use super::{SessionUsage, Usage};

fn usage(u: &Value) -> Option<Usage> {
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    u.is_object().then(|| {
        let cached = n("cached_input_tokens");
        Usage {
            input: n("input_tokens").saturating_sub(cached),
            output: n("output_tokens"),
            cache_write: n("cache_write_input_tokens"),
            cache_read: cached,
            messages: 1,
        }
    })
}

/// Usage of one rollout file (one session).
pub fn read_rollout(text: &str) -> Option<SessionUsage> {
    let mut s = SessionUsage::default();
    let (mut model, mut seen) = (None::<String>, HashSet::new());
    let mut fallback: Vec<(Option<String>, Usage)> = Vec::new();
    let mut last_total: Option<String> = None;
    let mut has_records = false;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let p = &v["payload"];
        let ts = v["timestamp"].as_str().map(String::from);
        match (v["type"].as_str(), p["type"].as_str()) {
            (Some("session_meta"), _) => {
                if let Some(id) = p["session_id"].as_str().or(p["id"].as_str()) {
                    s.session_id = id.to_string();
                }
            }
            (Some("turn_context"), _) => model = p["model"].as_str().map(String::from).or(model),
            (Some("token_usage_record"), _) => {
                let id = p["response_id"].as_str().unwrap_or_default().to_string();
                let Some(u) = usage(&p["usage"]) else {
                    continue;
                };
                if !seen.insert(id) {
                    continue; // same response written twice
                }
                has_records = true;
                if s.session_id.is_empty() {
                    s.session_id = p["session_id"].as_str().unwrap_or_default().to_string();
                }
                add(&mut s, u, ts, model.clone());
            }
            (Some("event_msg"), Some("token_count")) => {
                let total = p["info"]["total_token_usage"].to_string();
                if p["info"].is_null() || last_total.as_deref() == Some(total.as_str()) {
                    continue;
                }
                last_total = Some(total);
                if let Some(u) = usage(&p["info"]["last_token_usage"]) {
                    fallback.push((ts, u));
                }
            }
            _ => {}
        }
    }
    if !has_records {
        for (ts, u) in fallback {
            add(&mut s, u, ts, model.clone());
        }
    }
    (!s.session_id.is_empty() && s.main.messages > 0).then(|| {
        s.agent = "codex".into();
        s
    })
}

fn add(s: &mut SessionUsage, u: Usage, ts: Option<String>, model: Option<String>) {
    s.main.add(&u);
    s.turns
        .push((u.input + u.cache_write + u.cache_read, u.output));
    if s.first_ts.is_none() {
        s.first_ts.clone_from(&ts);
    }
    if ts > s.last_ts {
        s.last_ts = ts;
    }
    if model.is_some() {
        s.model = model;
    }
}

pub fn sessions_dir() -> PathBuf {
    crate::install::codex_dir().join("sessions")
}

/// Usage of every Codex session whose log changed since `since`.
pub fn sessions(since: Option<SystemTime>) -> Vec<SessionUsage> {
    let mut by_id: HashMap<String, SessionUsage> = HashMap::new();
    for path in super::claude::transcript_files(&sessions_dir(), since) {
        let is_rollout = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("rollout-"));
        let Some(s) = is_rollout
            .then(|| fs::read_to_string(&path).ok())
            .flatten()
            .and_then(|t| read_rollout(&t))
        else {
            continue;
        };
        by_id.entry(s.session_id.clone()).or_insert(s); // a resumed session may appear twice
    }
    let mut out: Vec<SessionUsage> = by_id.into_values().collect();
    out.sort_by(|a, b| a.first_ts.cmp(&b.first_ts));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_count_once_and_cached_input_is_not_double_counted() {
        let s = read_rollout(include_str!(
            "../../tests/fixtures/transcripts/codex-rollout.jsonl"
        ))
        .unwrap();
        assert_eq!(s.session_id, "019a-codex-1");
        assert_eq!(s.agent, "codex");
        assert_eq!(s.model.as_deref(), Some("gpt-6.1-sol"));
        // resp_1: 25677 in (13056 cached), 201 out; resp_2: 30000 in (25000 cached), 50 out.
        assert_eq!(
            s.main,
            Usage {
                input: 12621 + 5000,
                output: 251,
                cache_write: 0,
                cache_read: 38056,
                messages: 2
            }
        );
        assert_eq!(s.turns, vec![(25677, 201), (30000, 50)]);
        assert_eq!(s.first_ts.as_deref(), Some("2026-10-07T18:13:59Z"));
    }

    #[test]
    fn falls_back_to_token_count_events_without_records() {
        let text = r#"{"type":"session_meta","payload":{"id":"old-1"}}
{"type":"event_msg","timestamp":"t1","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100},"last_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":7}}}}
{"type":"event_msg","timestamp":"t2","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100},"last_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":7}}}}
{"type":"event_msg","timestamp":"t3","payload":{"type":"token_count","info":null}}
{"type":"event_msg","timestamp":"t4","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":300},"last_token_usage":{"input_tokens":200,"cached_input_tokens":150,"output_tokens":3}}}}"#;
        let s = read_rollout(text).unwrap();
        assert_eq!(
            s.main,
            Usage {
                input: 60 + 50,
                output: 10,
                cache_write: 0,
                cache_read: 190,
                messages: 2
            }
        );
        assert!(read_rollout(r#"{"type":"session_meta","payload":{"id":"empty"}}"#).is_none());
    }
}
