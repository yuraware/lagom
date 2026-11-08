//! `lagom discover`: where Lagom could save more, from the agents' own logs.
//!
//! Lists large shell outputs that weren't compressed and why, programs Lagom doesn't
//! know yet, and files read repeatedly. Commands are grouped by program and
//! subcommand only, so arguments (which may hold secrets) never appear.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::rewrite;

/// Outputs this big (≈1,000 tokens) are worth compressing.
pub const LARGE: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Shell {
        command: String,
        bytes: usize,
    },
    Read {
        session: String,
        path: String,
        bytes: usize,
    },
}

fn text_len(content: &Value) -> usize {
    match content {
        Value::String(s) => s.len(),
        Value::Array(parts) => parts
            .iter()
            .map(|p| p["text"].as_str().map_or(0, str::len))
            .sum(),
        _ => 0,
    }
}

/// Tool calls with their result sizes from a Claude Code transcript.
pub fn claude_calls(text: &str) -> Vec<Call> {
    let mut pending: HashMap<String, (String, Value, String)> = HashMap::new();
    let mut out = Vec::new();
    for line in text.lines() {
        if !line.contains("tool_") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let session = v["sessionId"].as_str().unwrap_or_default().to_string();
        for block in v["message"]["content"].as_array().into_iter().flatten() {
            match block["type"].as_str() {
                Some("tool_use") => {
                    let id = block["id"].as_str().unwrap_or_default().to_string();
                    let name = block["name"].as_str().unwrap_or_default().to_string();
                    pending.insert(id, (name, block["input"].clone(), session.clone()));
                }
                Some("tool_result") => {
                    let id = block["tool_use_id"].as_str().unwrap_or_default();
                    let Some((name, input, session)) = pending.remove(id) else {
                        continue;
                    };
                    let bytes = text_len(&block["content"]);
                    match name.as_str() {
                        "Bash" => out.push(Call::Shell {
                            command: input["command"].as_str().unwrap_or_default().to_string(),
                            bytes,
                        }),
                        "Read" => out.push(Call::Read {
                            session,
                            path: input["file_path"].as_str().unwrap_or_default().to_string(),
                            bytes,
                        }),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Shell calls with their output sizes from a Codex rollout.
pub fn codex_calls(text: &str) -> Vec<Call> {
    let mut pending: HashMap<String, String> = HashMap::new();
    let mut out = Vec::new();
    for line in text.lines() {
        if !line.contains("tool_call") && !line.contains("function_call") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let p = &v["payload"];
        let id = p["call_id"].as_str().unwrap_or_default().to_string();
        match p["type"].as_str() {
            Some("custom_tool_call") | Some("function_call") => {
                let input = p["input"]
                    .as_str()
                    .or(p["arguments"].as_str())
                    .unwrap_or_default();
                // JSON arguments ({"cmd": …} / {"command": [...]}) or a plain command.
                let command = match serde_json::from_str::<Value>(input) {
                    Ok(a) => a["cmd"].as_str().map(String::from).or_else(|| {
                        a["command"].as_array().map(|c| {
                            c.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                    }),
                    Err(_) => Some(input.to_string()),
                };
                if let Some(c) = command {
                    pending.insert(id, c);
                }
            }
            Some("custom_tool_call_output") | Some("function_call_output") => {
                if let Some(command) = pending.remove(&id) {
                    out.push(Call::Shell {
                        command,
                        bytes: text_len(&p["output"]),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// `program subcommand`, e.g. `cargo test` or `git log`; `bash -lc 'x'` style wrappers
/// are shown as the wrapper.
pub fn command_key(command: &str) -> String {
    let words: Vec<&str> = command.split_whitespace().take(2).collect();
    // Codex's exec tool can run JavaScript; anything not shaped like a program name
    // is grouped as a script.
    let program_like = |w: &str| {
        let w = w.rsplit('/').next().unwrap_or(w);
        !w.is_empty()
            && w.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
    };
    if !words.first().is_some_and(|w| program_like(w)) {
        return "(script)".into();
    }
    match words.as_slice() {
        // A subcommand is a short plain word; anything else (URLs, files, flags,
        // tokens) is an argument and stays out of the report.
        [p, sub]
            if sub.len() <= 20
                && sub.starts_with(|c: char| c.is_ascii_lowercase())
                && sub.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_".contains(&b)
                }) =>
        {
            let p = p.rsplit('/').next().unwrap_or(p);
            format!("{p} {sub}")
        }
        [p, ..] => p.rsplit('/').next().unwrap_or(p).to_string(),
        [] => String::new(),
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Group {
    pub count: usize,
    pub bytes: usize,
}

#[derive(Debug, Default, PartialEq)]
pub struct Report {
    /// (command key, reason) → large uncompressed outputs.
    pub uncompressed: BTreeMap<(String, &'static str), Group>,
    /// file path → reads of the same file in one session beyond the first.
    pub rereads: BTreeMap<String, Group>,
    pub compressed: usize,
}

pub fn analyze(calls: &[Call]) -> Report {
    let mut r = Report::default();
    let mut reads: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for call in calls {
        match call {
            Call::Shell { command, bytes } => {
                if command.contains("lagom run") || command.contains("lgm run") {
                    r.compressed += 1;
                    continue;
                }
                if *bytes < LARGE {
                    continue;
                }
                let reason = rewrite::skip_reason(command).unwrap_or(
                    "no allow rule matched, or Lagom's hook isn't installed for this agent",
                );
                let g = r
                    .uncompressed
                    .entry((command_key(command), reason))
                    .or_default();
                g.count += 1;
                g.bytes += bytes;
            }
            Call::Read {
                session,
                path,
                bytes,
            } => reads.entry((session, path)).or_default().push(*bytes),
        }
    }
    for ((_, path), sizes) in reads {
        let big: Vec<usize> = sizes.into_iter().filter(|b| *b >= LARGE).collect();
        if big.len() >= 2 {
            let g = r.rereads.entry(path.to_string()).or_default();
            g.count += big.len() - 1;
            g.bytes += big.iter().skip(1).sum::<usize>();
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_uncompressed_outputs_reasons_and_rereads() {
        let claude = include_str!("../../tests/fixtures/transcripts/claude-tools.jsonl");
        let codex = include_str!("../../tests/fixtures/transcripts/codex-tools.jsonl");
        let mut calls = claude_calls(claude);
        calls.extend(codex_calls(codex));
        let r = analyze(&calls);
        assert_eq!(r.compressed, 1);
        let get = |k: &str, reason: &str| r.uncompressed.get(&(k.to_string(), reason)).cloned();
        let allow = "no allow rule matched, or Lagom's hook isn't installed for this agent";
        assert_eq!(
            get("cargo test", allow),
            Some(Group {
                count: 2,
                bytes: 10_000 + 6_000
            })
        );
        assert_eq!(
            get(
                "git log",
                "pipes, &&, ;, redirects and subshells are never rewritten"
            ),
            Some(Group {
                count: 1,
                bytes: 9_000
            })
        );
        assert_eq!(
            get("ansible-playbook", "program not in Lagom's rewrite list"),
            Some(Group {
                count: 1,
                bytes: 50_000
            })
        );
        assert!(
            r.uncompressed.keys().all(|(k, _)| k != "ls"),
            "small outputs are ignored"
        );
        assert_eq!(
            r.rereads.get("/repo/src/big.rs"),
            Some(&Group {
                count: 2,
                bytes: 16_000
            })
        );
        assert!(!r.rereads.contains_key("/repo/src/small.rs"));
    }

    #[test]
    fn command_keys_hide_arguments() {
        assert_eq!(
            command_key("cargo test --workspace -- secret"),
            "cargo test"
        );
        assert_eq!(command_key("/usr/bin/git log --stat"), "git log");
        assert_eq!(command_key("ls -la /tmp"), "ls");
        assert_eq!(command_key("curl -H 'Authorization: x'"), "curl");
        assert_eq!(command_key("curl https://user:token@host/x"), "curl");
        assert_eq!(
            command_key("ansible-playbook deploy.yml"),
            "ansible-playbook"
        );
        assert_eq!(command_key("npm ABCDEF0123secret"), "npm");
        assert_eq!(command_key("text(await tools.exec(x))"), "(script)");
        assert_eq!(
            command_key("const a = 1;"),
            "const a",
            "plain words look like a program and subcommand"
        );
    }
}
