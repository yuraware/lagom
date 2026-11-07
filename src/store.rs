//! Local state under `~/.lagom` (or `$LAGOM_HOME`): originals for `lagom recall`
//! and an append-only stats log. Plain files keep the MVP dependency-free;
//! SQLite arrives with session memory.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
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
    private_dir(&lagom_dir())?;
    let dir = recall_dir();
    private_dir(&dir)?;
    let id = content_id(content);
    let path = dir.join(format!("{id}.log"));
    private_file(fs::OpenOptions::new().write(true).truncate(true), &path)?
        .write_all(content.as_bytes())?;
    prune_recall(&dir);
    Ok(id)
}

pub fn load_original(id: &str) -> Option<String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    fs::read_to_string(recall_dir().join(format!("{id}.log"))).ok()
}

/// Which part of a recalled output to print. Applied in order: `lines`, `grep`, `head`, `tail`.
#[derive(Debug, Default, PartialEq)]
pub struct Selection {
    /// 1-based inclusive range; `None` end means "to the last line".
    pub lines: Option<(usize, Option<usize>)>,
    /// Keep lines containing this text (case-sensitive); printed as `N: line`.
    pub grep: Option<String>,
    pub head: Option<usize>,
    pub tail: Option<usize>,
}

impl Selection {
    /// Parse `--lines A-B|A-|A`, `--grep TEXT`, `--head N`, `--tail N`.
    pub fn parse(args: &[String]) -> Result<Selection, String> {
        let mut sel = Selection::default();
        let mut it = args.iter();
        while let Some(flag) = it.next() {
            let mut value = || {
                it.next()
                    .cloned()
                    .ok_or_else(|| format!("{flag} needs a value"))
            };
            let number = |v: String| {
                v.parse::<usize>()
                    .map_err(|_| format!("{flag}: `{v}` is not a number"))
            };
            match flag.as_str() {
                "--grep" => sel.grep = Some(value()?),
                "--head" => sel.head = Some(number(value()?)?),
                "--tail" => sel.tail = Some(number(value()?)?),
                "--lines" => {
                    let v = value()?;
                    let (a, b) = v.split_once('-').unwrap_or((&v, &v));
                    let start = a.parse::<usize>().ok().filter(|n| *n >= 1);
                    let end = if b.is_empty() {
                        Ok(None)
                    } else {
                        b.parse::<usize>().map(Some)
                    };
                    match (start, end) {
                        (Some(s), Ok(e)) if e.is_none_or(|e| e >= s) => sel.lines = Some((s, e)),
                        _ => return Err(format!("--lines: use A-B, A- or A (1-based), got `{v}`")),
                    }
                }
                other => return Err(format!("unknown option `{other}`")),
            }
        }
        Ok(sel)
    }

    /// Apply the selection. Grep results carry their line number in the full output.
    pub fn apply(&self, text: &str) -> String {
        let mut lines: Vec<(usize, &str)> =
            text.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
        if let Some((start, end)) = self.lines {
            lines.retain(|(n, _)| *n >= start && end.is_none_or(|e| *n <= e));
        }
        if let Some(pat) = &self.grep {
            lines.retain(|(_, l)| l.contains(pat.as_str()));
        }
        if let Some(h) = self.head {
            lines.truncate(h);
        }
        if let Some(t) = self.tail {
            lines.drain(..lines.len().saturating_sub(t));
        }
        let numbered = self.grep.is_some();
        let mut out = String::new();
        for (n, l) in lines {
            if numbered {
                out.push_str(&format!("{n}: "));
            }
            out.push_str(l);
            out.push('\n');
        }
        out
    }
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

/// Create `dir` readable only by the user (0700 on Unix), tightening an existing one.
/// Recall originals hold raw command output, which can contain secrets.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    set_mode(dir, 0o700);
    Ok(())
}

/// Open (creating if needed) a file readable only by the user (0600 on Unix).
fn private_file(options: &mut fs::OpenOptions, path: &Path) -> std::io::Result<fs::File> {
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(options, 0o600);
    let file = options.create(true).open(path)?;
    set_mode(path, 0o600); // files created by older versions
    Ok(file)
}

fn set_mode(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
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
    if private_dir(&dir).is_err() {
        return;
    }
    if let Ok(mut file) = private_file(
        fs::OpenOptions::new().append(true),
        &dir.join("stats.jsonl"),
    ) {
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

    fn sel(args: &[&str]) -> Selection {
        Selection::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn recall_selection() {
        let text = "a\nb error\nc\nd error\ne\n";
        assert_eq!(sel(&[]).apply(text), text);
        assert_eq!(sel(&["--lines", "2-3"]).apply(text), "b error\nc\n");
        assert_eq!(sel(&["--lines", "4-"]).apply(text), "d error\ne\n");
        assert_eq!(sel(&["--lines", "2"]).apply(text), "b error\n");
        assert_eq!(
            sel(&["--grep", "error"]).apply(text),
            "2: b error\n4: d error\n"
        );
        assert_eq!(sel(&["--head", "2"]).apply(text), "a\nb error\n");
        assert_eq!(sel(&["--tail", "1"]).apply(text), "e\n");
        assert_eq!(
            sel(&["--lines", "3-", "--grep", "error"]).apply(text),
            "4: d error\n"
        );
        for bad in [
            &["--lines", "0-2"][..],
            &["--lines", "5-2"],
            &["--head", "x"],
            &["--grep"],
            &["--nope"],
        ] {
            let args: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
            assert!(Selection::parse(&args).is_err(), "{bad:?}");
        }
    }

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
