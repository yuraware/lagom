//! Local state under `~/.lagom` (or `$LAGOM_HOME`): originals for `lagom recall`
//! and an append-only stats log. Plain files keep the MVP dependency-free;
//! SQLite arrives with session memory.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_RECALL_FILES: usize = 500;

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn lagom_dir() -> PathBuf {
    std::env::var_os("LAGOM_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".lagom"))
}

fn recall_dir() -> PathBuf {
    lagom_dir().join("recall")
}

/// Short stable id (FNV-1a) so the same output maps to the same recall entry.
pub fn content_id(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in content.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:010x}", hash & 0xff_ffff_ffff)
}

/// Save the full original output and return its id.
pub fn save_original(content: &str) -> std::io::Result<String> {
    let dir = recall_dir();
    fs::create_dir_all(&dir)?;
    let id = content_id(content);
    fs::write(dir.join(format!("{id}.log")), content)?;
    prune_recall(&dir);
    Ok(id)
}

pub fn load_original(id: &str) -> Option<String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    fs::read_to_string(recall_dir().join(format!("{id}.log"))).ok()
}

fn prune_recall(dir: &PathBuf) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if files.len() <= MAX_RECALL_FILES {
        return;
    }
    files.sort();
    for (_, path) in files.iter().take(files.len() - MAX_RECALL_FILES) {
        let _ = fs::remove_file(path);
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Append one event to `stats.jsonl`. Errors are ignored: stats must never break a command.
pub fn record(mut event: serde_json::Value) {
    if let Some(obj) = event.as_object_mut() {
        obj.insert("ts".into(), now_secs().into());
    }
    let dir = lagom_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("stats.jsonl"))
    {
        let _ = writeln!(file, "{event}");
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Totals {
    pub runs: u64,
    pub compressed_runs: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub injections: u64,
    pub injected_bytes: u64,
}

pub fn totals() -> Totals {
    let mut t = Totals::default();
    let Ok(text) = fs::read_to_string(lagom_dir().join("stats.jsonl")) else {
        return t;
    };
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let n = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        match v.get("kind").and_then(|k| k.as_str()) {
            Some("run") => {
                t.runs += 1;
                t.bytes_in += n("in");
                t.bytes_out += n("out");
                if n("in") > n("out") {
                    t.compressed_runs += 1;
                }
            }
            Some("inject") => {
                t.injections += 1;
                t.injected_bytes += n("bytes");
            }
            _ => {}
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_id_is_stable_hex() {
        let a = content_id("hello");
        assert_eq!(a, content_id("hello"));
        assert_ne!(a, content_id("hello!"));
        assert_eq!(a.len(), 10);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn load_rejects_path_tricks() {
        assert_eq!(load_original("../etc/passwd"), None);
        assert_eq!(load_original(""), None);
    }
}
