//! Local state under `~/.lagom` (or `$LAGOM_HOME`): originals for `lagom recall`
//! and an append-only stats log. Plain files keep the MVP dependency-free;
//! SQLite arrives with session memory.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Recall store limits: whichever is hit first prunes the oldest entries.
#[derive(Clone, Copy, Debug)]
pub struct RecallLimits {
    pub max_files: usize,
    pub max_bytes: u64,
    pub max_age: std::time::Duration,
}

impl Default for RecallLimits {
    fn default() -> Self {
        RecallLimits {
            max_files: 500,
            max_bytes: 200 * 1024 * 1024,
            max_age: std::time::Duration::from_secs(14 * 24 * 3600),
        }
    }
}

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

/// File that marks a folder as Lagom's own, so `lagom uninstall --purge` may delete it.
pub const DIR_MARKER: &str = ".lagom-dir";

/// Create Lagom's data folder (private) and mark it as Lagom's.
fn lagom_home() -> std::io::Result<PathBuf> {
    let dir = lagom_dir();
    private_dir(&dir)?;
    mark_dir(&dir);
    Ok(dir)
}

/// Write [`DIR_MARKER`] into a folder Lagom created. Best effort: a missing marker only
/// makes `--purge` refuse the folder.
pub fn mark_dir(dir: &Path) {
    let marker = dir.join(DIR_MARKER);
    if !marker.exists() {
        let _ = fs::write(
            marker,
            "Created by Lagom. `lagom uninstall --purge` deletes this folder.\n",
        );
    }
}

fn recall_dir() -> PathBuf {
    lagom_dir().join("recall")
}

/// Short stable id (FNV-1a) so the same output maps to the same recall entry.
pub fn content_id(content: &str) -> String {
    format_id(fnv(FNV_OFFSET, content.as_bytes()))
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;

fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn format_id(hash: u64) -> String {
    format!("{:010x}", hash & 0xff_ffff_ffff)
}

/// Streams an original too large to keep in memory straight into the recall store.
/// Its id is the same FNV hash `content_id` would give the whole text.
pub struct Spill {
    dir: PathBuf,
    tmp: PathBuf,
    file: fs::File,
    hash: u64,
}

impl Spill {
    pub fn create() -> std::io::Result<Spill> {
        private_dir(&lagom_dir())?;
        Spill::create_in(&recall_dir())
    }

    pub fn create_in(dir: &Path) -> std::io::Result<Spill> {
        private_dir(dir)?;
        let tmp = dir.join(format!(".spill-{}.tmp", std::process::id()));
        let file = private_file(fs::OpenOptions::new().write(true).truncate(true), &tmp)?;
        Ok(Spill {
            dir: dir.to_path_buf(),
            tmp,
            file,
            hash: FNV_OFFSET,
        })
    }

    /// A spill whose writes fail, as on a full disk.
    #[cfg(test)]
    pub fn failing_in(dir: &Path) -> std::io::Result<Spill> {
        let s = Spill::create_in(dir)?;
        Ok(Spill {
            file: fs::File::open(&s.tmp)?, // read-only: every write fails
            ..s
        })
    }

    pub fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.hash = fnv(self.hash, data);
        self.file.write_all(data)
    }

    /// Move the file into place as `<id>.log` and return the id.
    pub fn finish(mut self, program: &str) -> std::io::Result<String> {
        self.file.flush()?;
        let id = format_id(self.hash);
        fs::rename(&self.tmp, self.dir.join(format!("{id}.log")))?;
        let _ = private_file(
            fs::OpenOptions::new().write(true).truncate(true),
            &self.dir.join(format!("{id}.cmd")),
        )
        .and_then(|mut f| f.write_all(program.as_bytes()));
        prune_recall(&self.dir, RecallLimits::default(), SystemTime::now());
        Ok(id)
    }
}

/// Save the full original output of `program` and return its id.
pub fn save_original(content: &str, program: &str) -> std::io::Result<String> {
    lagom_home()?;
    let dir = recall_dir();
    private_dir(&dir)?;
    let id = content_id(content);
    let path = dir.join(format!("{id}.log"));
    private_file(fs::OpenOptions::new().write(true).truncate(true), &path)?
        .write_all(content.as_bytes())?;
    // Program name only (never arguments): shown by `lagom recall --list`.
    let _ = private_file(
        fs::OpenOptions::new().write(true).truncate(true),
        &dir.join(format!("{id}.cmd")),
    )
    .and_then(|mut f| f.write_all(program.as_bytes()));
    prune_recall(&dir, RecallLimits::default(), SystemTime::now());
    Ok(id)
}

/// The shape of an id that [`content_id`] makes: 10 hex digits.
pub fn is_recall_id(id: &str) -> bool {
    id.len() == 10 && id.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn load_original(id: &str) -> Option<String> {
    if !is_recall_id(id) {
        return None;
    }
    // Spilled originals are raw bytes: show them even if they aren't valid UTF-8.
    let bytes = fs::read(recall_dir().join(format!("{id}.log"))).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Which part of a recalled output to print. Applied in order: `lines`, `grep`, `head`, `tail`.
#[derive(Debug, Default, PartialEq)]
pub struct Selection {
    /// 1-based inclusive range; `None` end means "to the last line".
    pub lines: Option<(usize, Option<usize>)>,
    /// Keep lines containing this text; printed as `N: line`. Smart case, as in `rg`:
    /// all-lowercase text matches any case (`error` finds `ERROR`), otherwise exact.
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
            if pat.chars().any(char::is_uppercase) {
                lines.retain(|(_, l)| l.contains(pat.as_str()));
            } else {
                lines.retain(|(_, l)| l.to_lowercase().contains(pat.as_str()));
            }
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

/// One saved original, as listed by `lagom recall --list`.
#[derive(Debug)]
pub struct RecallEntry {
    pub id: String,
    pub program: String,
    pub bytes: u64,
    pub modified: SystemTime,
}

/// Saved originals, newest first.
pub fn list_recall() -> Vec<RecallEntry> {
    let mut out = entries(&recall_dir(), true);
    out.sort_by_key(|e| std::cmp::Reverse(e.modified));
    out
}

/// Delete every saved original. Returns how many were removed.
pub fn clear_recall() -> usize {
    let dir = recall_dir();
    remove_stale_spills(&dir, SystemTime::now());
    let all = entries(&dir, false);
    for e in &all {
        remove_entry(&dir, &e.id);
    }
    all.len()
}

/// Saved originals in `dir`. `with_program` also reads each `.cmd` file; pruning, which
/// runs after every save, doesn't need it.
fn entries(dir: &Path, with_program: bool) -> Vec<RecallEntry> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    read.flatten()
        .filter_map(|e| {
            let path = e.path();
            let id = path
                .file_name()?
                .to_str()?
                .strip_suffix(".log")?
                .to_string();
            let meta = e.metadata().ok()?;
            let program = if with_program {
                fs::read_to_string(dir.join(format!("{id}.cmd"))).unwrap_or_default()
            } else {
                String::new()
            };
            Some(RecallEntry {
                id,
                program,
                bytes: meta.len(),
                modified: meta.modified().ok()?,
            })
        })
        .collect()
}

fn remove_entry(dir: &Path, id: &str) {
    let _ = fs::remove_file(dir.join(format!("{id}.log")));
    let _ = fs::remove_file(dir.join(format!("{id}.cmd")));
}

/// Drop entries older than `max_age`, then the oldest until both the count and the
/// total size fit. The newest entry is always kept.
fn prune_recall(dir: &Path, limits: RecallLimits, now: SystemTime) {
    remove_stale_spills(dir, now);
    let mut all = entries(dir, false);
    all.sort_by_key(|e| std::cmp::Reverse(e.modified)); // newest first
    let mut kept_bytes = 0u64;
    for (i, e) in all.iter().enumerate() {
        let too_old = now
            .duration_since(e.modified)
            .is_ok_and(|age| age > limits.max_age);
        let over = i >= limits.max_files || kept_bytes + e.bytes > limits.max_bytes;
        if i > 0 && (too_old || over) {
            remove_entry(dir, &e.id);
        } else {
            kept_bytes += e.bytes;
        }
    }
}

/// Remove `.spill-<pid>.tmp` files that a killed `lagom run` left behind. A spill that is
/// still being written was touched moments ago; one untouched for an hour is abandoned.
fn remove_stale_spills(dir: &Path, now: SystemTime) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for e in read.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        let stale = e.metadata().and_then(|m| m.modified()).is_ok_and(|m| {
            now.duration_since(m)
                .is_ok_and(|age| age > std::time::Duration::from_secs(3600))
        });
        if name.starts_with(".spill-") && name.ends_with(".tmp") && stale {
            let _ = fs::remove_file(e.path());
        }
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

/// Who an event belongs to: the agent and session (from the hook payload) and the
/// project (a hash of the repository root, never the path itself).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Attribution {
    pub agent: Option<String>,
    pub session: Option<String>,
    pub project: Option<String>,
}

impl Attribution {
    /// From a `--session agent:id` token (as written by the pre-tool rewrite) and a folder.
    pub fn from_token(token: Option<&str>, cwd: &Path) -> Attribution {
        let (agent, session) = match token.and_then(|t| t.split_once(':')) {
            Some((a, s)) if is_safe_id(a) && is_safe_id(s) => {
                (Some(a.to_string()), Some(s.to_string()))
            }
            _ => (None, None),
        };
        Attribution {
            agent,
            session,
            project: project_id(cwd),
        }
    }

    fn apply(&self, obj: &mut serde_json::Map<String, serde_json::Value>) {
        for (k, v) in [
            ("agent", &self.agent),
            ("session", &self.session),
            ("project", &self.project),
        ] {
            if let Some(v) = v {
                obj.insert(k.into(), v.clone().into());
            }
        }
    }
}

/// Session and agent ids go into commands and stats: letters, digits, `-`, `_`, `.` only.
pub fn is_safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// Short stable id for the git repository containing `dir`: a hash of its root path.
pub fn project_id(dir: &Path) -> Option<String> {
    let root = dir.ancestors().find(|d| d.join(".git").exists())?;
    // Real path: hooks get `cwd` from the agent, `lagom run` from the OS, and the two
    // can differ by symlinks (macOS `/var` → `/private/var`).
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    Some(content_id(&root.to_string_lossy()))
}

/// Append one event to `stats.jsonl`. Errors are ignored: stats must never break a command.
pub fn record(event: serde_json::Value) {
    record_for(event, &Attribution::default());
}

/// Like [`record`], tagged with agent, session and project.
pub fn record_for(mut event: serde_json::Value, who: &Attribution) {
    if let Some(obj) = event.as_object_mut() {
        obj.insert("v".into(), 1.into());
        obj.insert("ts".into(), now_secs().into());
        who.apply(obj);
    }
    let Ok(dir) = lagom_home() else {
        return;
    };
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
    /// Per agent: `manual` for `lagom run` typed by hand, `unknown` for events an older
    /// Lagom wrote before it recorded the agent.
    pub by_agent: std::collections::BTreeMap<String, Bucket>,
    /// Injections per style set, such as `terse+lean-code`.
    pub by_style: std::collections::BTreeMap<String, Bucket>,
}

#[derive(Debug, Default, PartialEq, Clone, Copy)]
pub struct Bucket {
    pub runs: u64,
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
        if v.get("v")
            .and_then(|x| x.as_u64())
            .is_some_and(|ver| ver > 1)
        {
            continue; // written by a newer Lagom
        }
        let n = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        let agent = match v.get("agent").and_then(|a| a.as_str()) {
            Some(a) => a,
            None if v.get("v").is_some() => "manual",
            None => "unknown",
        }
        .to_string();
        match v.get("kind").and_then(|k| k.as_str()) {
            Some("run") => {
                t.runs += 1;
                t.bytes_in += n("in");
                t.bytes_out += n("out");
                if n("in") > n("out") {
                    t.compressed_runs += 1;
                }
                let b = t.by_agent.entry(agent).or_default();
                b.runs += 1;
                b.bytes_in += n("in");
                b.bytes_out += n("out");
            }
            Some("inject") => {
                t.injections += 1;
                t.injected_bytes += n("bytes");
                let b = t.by_agent.entry(agent).or_default();
                b.injections += 1;
                b.injected_bytes += n("bytes");
                let styles: Vec<&str> = v
                    .get("styles")
                    .and_then(|s| s.as_array())
                    .map(|a| a.iter().filter_map(|s| s.as_str()).collect())
                    .unwrap_or_default();
                let key = if styles.is_empty() {
                    "unknown".to_string()
                } else {
                    styles.join("+")
                };
                let s = t.by_style.entry(key).or_default();
                s.injections += 1;
                s.injected_bytes += n("bytes");
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
    fn stale_spill_files_are_pruned_and_fresh_ones_kept() {
        use std::time::Duration;
        let dir = std::env::temp_dir().join(format!("lagom-spills-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        for (name, age) in [
            (".spill-1.tmp", 7200),
            (".spill-2.tmp", 10),
            ("note.tmp", 7200),
        ] {
            let p = dir.join(name);
            fs::write(&p, "x").unwrap();
            fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(now - Duration::from_secs(age))
                .unwrap();
        }
        prune_recall(&dir, RecallLimits::default(), now);
        assert!(
            !dir.join(".spill-1.tmp").exists(),
            "abandoned spill removed"
        );
        assert!(
            dir.join(".spill-2.tmp").exists(),
            "a spill being written stays"
        );
        assert!(
            dir.join("note.tmp").exists(),
            "only spill files are touched"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recall_pruning_limits_count_size_and_age() {
        use std::time::Duration;
        let dir = std::env::temp_dir().join(format!("lagom-cmp04-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        let add = |id: &str, bytes: usize, age_s: u64| {
            let p = dir.join(format!("{id}.log"));
            fs::write(&p, "x".repeat(bytes)).unwrap();
            fs::write(dir.join(format!("{id}.cmd")), "git").unwrap();
            let f = fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(now - Duration::from_secs(age_s)).unwrap();
        };
        let ids = |d: &Path| {
            let mut v: Vec<String> = entries(d, false).into_iter().map(|e| e.id).collect();
            v.sort();
            v
        };
        let limits = RecallLimits {
            max_files: 3,
            max_bytes: 1000,
            max_age: Duration::from_secs(100),
        };

        for (i, id) in ["a", "b", "c", "d"].iter().enumerate() {
            add(id, 10, 10 + i as u64); // a newest .. d oldest
        }
        prune_recall(&dir, limits, now);
        assert_eq!(ids(&dir), ["a", "b", "c"], "count limit drops the oldest");
        assert!(!dir.join("d.cmd").exists(), "sidecar removed too");

        add("old", 10, 500);
        prune_recall(&dir, limits, now);
        assert!(!ids(&dir).contains(&"old".to_string()), "age limit");

        add("big", 900, 1); // newest; with it, a/b/c no longer fit in 1000 bytes
        prune_recall(&dir, limits, now);
        assert_eq!(
            ids(&dir),
            ["a", "b", "big"],
            "size limit keeps newest that fit"
        );

        add("huge", 5000, 0); // bigger than the limit on its own: still kept, as the newest
        prune_recall(&dir, limits, now);
        assert_eq!(ids(&dir), ["huge"]);
        let _ = fs::remove_dir_all(&dir);
    }

    fn sel(args: &[&str]) -> Selection {
        Selection::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn recall_selection() {
        let text = "a\nb error\nc\nd ERROR\ne\n";
        assert_eq!(sel(&[]).apply(text), text);
        assert_eq!(sel(&["--lines", "2-3"]).apply(text), "b error\nc\n");
        assert_eq!(sel(&["--lines", "4-"]).apply(text), "d ERROR\ne\n");
        assert_eq!(sel(&["--lines", "2"]).apply(text), "b error\n");
        assert_eq!(
            sel(&["--grep", "error"]).apply(text),
            "2: b error\n4: d ERROR\n",
            "lowercase text matches any case"
        );
        assert_eq!(sel(&["--grep", "ERROR"]).apply(text), "4: d ERROR\n");
        assert_eq!(sel(&["--head", "2"]).apply(text), "a\nb error\n");
        assert_eq!(sel(&["--tail", "1"]).apply(text), "e\n");
        assert_eq!(
            sel(&["--lines", "3-", "--grep", "error"]).apply(text),
            "4: d ERROR\n"
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
