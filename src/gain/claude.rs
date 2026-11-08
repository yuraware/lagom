//! Claude Code transcripts: `$CLAUDE_CONFIG_DIR/projects/<project>/<session>.jsonl`
//! (default `~/.claude/projects`), one JSON event per line.
//!
//! Assistant events carry `message.usage`. **Streaming writes the same message several
//! times** (same `message.id`), so each id is counted once, using its last line.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::{SessionUsage, Usage};

struct Message {
    session: String,
    model: Option<String>,
    /// First and last time this message was written (streaming writes it repeatedly).
    first_ts: Option<String>,
    last_ts: Option<String>,
    sidechain: bool,
    usage: Usage,
}

fn parse_line(line: &str) -> Option<(String, Message)> {
    // Cheap filter first: most lines are not assistant messages with usage.
    if !line.contains("\"usage\"") || !line.contains("\"assistant\"") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    if v["type"] != "assistant" {
        return None;
    }
    let m = &v["message"];
    let u = m.get("usage")?;
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let id = m["id"].as_str().or(v["uuid"].as_str())?.to_string();
    Some((
        id,
        Message {
            session: v["sessionId"].as_str().unwrap_or("").to_string(),
            model: m["model"].as_str().map(String::from),
            first_ts: v["timestamp"].as_str().map(String::from),
            last_ts: v["timestamp"].as_str().map(String::from),
            sidechain: v["isSidechain"] == true,
            usage: Usage {
                input: n("input_tokens"),
                output: n("output_tokens"),
                cache_write: n("cache_creation_input_tokens"),
                cache_read: n("cache_read_input_tokens"),
                messages: 1,
            },
        },
    ))
}

/// Usage per session from transcript text (one or more files' worth).
pub fn read_transcripts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<SessionUsage> {
    let mut by_id: HashMap<String, Message> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for text in texts {
        for line in text.lines() {
            if let Some((id, mut msg)) = parse_line(line) {
                match by_id.get_mut(&id) {
                    // Last line per id wins, but the message started at its first line.
                    Some(old) => {
                        msg.first_ts = old.first_ts.take().or(msg.first_ts);
                        *old = msg;
                    }
                    None => {
                        by_id.insert(id.clone(), msg);
                        order.push(id);
                    }
                }
            }
        }
    }
    let mut sessions: Vec<SessionUsage> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for id in &order {
        let m = &by_id[id];
        let i = *index.entry(m.session.clone()).or_insert_with(|| {
            sessions.push(SessionUsage {
                session_id: m.session.clone(),
                ..Default::default()
            });
            sessions.len() - 1
        });
        let s = &mut sessions[i];
        if m.sidechain {
            s.subagents.add(&m.usage)
        } else {
            s.main.add(&m.usage);
            let u = &m.usage;
            s.turns
                .push((u.input + u.cache_write + u.cache_read, u.output));
        }
        if m.model.is_some() {
            s.model.clone_from(&m.model);
        }
        if s.first_ts.is_none() {
            s.first_ts.clone_from(&m.first_ts);
        }
        if m.last_ts > s.last_ts {
            s.last_ts.clone_from(&m.last_ts); // ISO 8601 strings sort by time
        }
    }
    sessions
}

/// Claude Code's projects folder.
pub fn projects_dir() -> PathBuf {
    crate::install::claude_dir().join("projects")
}

/// All `*.jsonl` files under `dir` (subagent transcripts live in subfolders),
/// optionally only those modified since `since`.
pub fn transcript_files(dir: &Path, since: Option<SystemTime>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "jsonl") {
                let recent = since.is_none_or(|t| {
                    e.metadata()
                        .and_then(|m| m.modified())
                        .is_ok_and(|m| m >= t)
                });
                if recent {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

/// Usage of every Claude Code session whose transcript changed since `since`.
pub fn sessions(since: Option<SystemTime>) -> Vec<SessionUsage> {
    let texts: Vec<String> = transcript_files(&projects_dir(), since)
        .iter()
        .filter_map(|p| fs::read_to_string(p).ok())
        .collect();
    read_transcripts(texts.iter().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_transcript_totals() {
        let text = include_str!("../../tests/fixtures/transcripts/claude-session.jsonl");
        let s = read_transcripts([text]);
        assert_eq!(s.len(), 2, "two sessions");
        let a = &s[0];
        assert_eq!(a.session_id, "sess-a");
        // msg_1 streamed 3 times (last: output 40), msg_2 once; msg_3 is a subagent.
        assert_eq!(
            a.main,
            Usage {
                input: 5,
                output: 40 + 12,
                cache_write: 1000,
                cache_read: 2000 + 3000,
                messages: 2
            }
        );
        assert_eq!(
            a.subagents,
            Usage {
                input: 7,
                output: 9,
                cache_write: 0,
                cache_read: 500,
                messages: 1
            }
        );
        assert_eq!(a.total().output, 61);
        assert_eq!(a.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(a.first_ts.as_deref(), Some("2026-11-08T10:00:01Z"));
        assert_eq!(a.last_ts.as_deref(), Some("2026-11-08T10:00:09Z"));
        assert_eq!(s[1].session_id, "sess-b");
        assert_eq!(s[1].main.output, 100);
    }

    #[test]
    fn same_message_in_two_files_counts_once() {
        let line = r#"{"type":"assistant","sessionId":"s","message":{"id":"m","usage":{"output_tokens":5}}}"#;
        let s = read_transcripts([line, line]);
        assert_eq!(s[0].main.output, 5);
        assert_eq!(s[0].main.messages, 1);
    }

    /// `cargo test -- --ignored real_transcripts --nocapture` prints totals for this
    /// machine's transcripts (numbers only).
    #[test]
    #[ignore]
    fn real_transcripts() {
        let all = sessions(None);
        let mut t = Usage::default();
        for s in &all {
            t.add(&s.total());
        }
        eprintln!("{} sessions: {t:?}", all.len());
    }
}
