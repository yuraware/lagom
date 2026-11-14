//! `lagom run -- <cmd> [args…]`: run a command, compress its combined output,
//! keep its exit code. Anything going wrong inside Lagom falls back to the raw output.

use std::io::{Read, Write};
use std::process::{Command, ExitCode};

use serde_json::json;

use crate::compress::{Limits, compress};
use crate::store;

/// `session`: the `agent:id` token from `lagom run --session …` (set by the pre-tool hook).
pub fn run(args: &[String], session: Option<&str>) -> ExitCode {
    let who = store::Attribution::from_token(session, &std::env::current_dir().unwrap_or_default());
    let Some((program, rest)) = args.split_first() else {
        eprintln!("usage: lagom run -- <command> [args…]");
        return ExitCode::from(2);
    };

    let (reader, writer) = match std::io::pipe() {
        Ok(p) => p,
        Err(e) => return fallback_exec(program, rest, &e),
    };
    let writer_err = match writer.try_clone() {
        Ok(w) => w,
        Err(e) => return fallback_exec(program, rest, &e),
    };
    let mut cmd = Command::new(program);
    cmd.args(rest).stdout(writer).stderr(writer_err);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("lagom: cannot run `{program}`: {e}");
            return ExitCode::from(127);
        }
    };
    // Drop our copies of the write end so reading ends when the child exits.
    drop(cmd);

    let captured = capture(reader, MEMORY_CAP, KEEP_EDGE, store::Spill::create);
    let status = child.wait();
    let code = status.as_ref().ok().and_then(|s| s.code()).unwrap_or(1);

    let output = match captured {
        Ok(Captured::Full(bytes)) => render(&String::from_utf8_lossy(&bytes), code, program, &who),
        Ok(Captured::Spilled {
            kept,
            spill,
            lines,
            bytes,
            cut_short,
        }) => render_spilled(
            &String::from_utf8_lossy(&kept),
            spill,
            (lines, bytes),
            cut_short,
            code,
            program,
            &who,
        ),
        // Never shown as if complete: say that reading stopped.
        Err((partial, e)) => format!(
            "{}\n[lagom: reading the command's output failed ({e}); what came before is shown, the rest is missing]\n",
            String::from_utf8_lossy(&partial).trim_end_matches('\n')
        ),
    };

    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(output.as_bytes());
    let _ = stdout.flush();
    ExitCode::from(code.clamp(0, 255) as u8)
}

/// Output kept in memory before streaming the rest to the recall store.
const MEMORY_CAP: usize = 64 * 1024 * 1024;
/// After overflow: bytes kept from the start and from the end.
const KEEP_EDGE: usize = 1024 * 1024;
/// After overflow: important lines kept from the middle (each cut to 400 chars).
const MAX_MIDDLE_LINES: usize = 400;

pub enum Captured {
    Full(Vec<u8>),
    /// The full output went to `spill`; `kept` holds the start, important middle
    /// lines and the end. `spill` is an error when the spill file stopped accepting
    /// writes: reading went on (the command still runs to its end and keeps its exit
    /// code), but the full output couldn't be stored. `cut_short` is set when reading
    /// the command's output failed before its end.
    Spilled {
        kept: Vec<u8>,
        spill: std::io::Result<store::Spill>,
        lines: usize,
        bytes: u64,
        cut_short: Option<std::io::Error>,
    },
}

/// Read all of `reader`, holding at most about `cap` bytes in memory. If the output
/// is bigger, everything streams into a spill file and only the first and last `edge`
/// bytes (on line boundaries) plus important middle lines are kept. If no spill file
/// can be created, keep reading into memory (fail open: never lose output).
pub fn capture<R: Read, S: FnOnce() -> std::io::Result<store::Spill>>(
    mut reader: R,
    cap: usize,
    edge: usize,
    make_spill: S,
) -> Result<Captured, (Vec<u8>, std::io::Error)> {
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(Captured::Full(buf)),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err((buf, e)),
        }
        if buf.len() > cap {
            break;
        }
    }
    let Ok(spill) = make_spill() else {
        // Can't store the original: don't drop anything.
        return match reader.read_to_end(&mut buf) {
            Ok(_) => Ok(Captured::Full(buf)),
            Err(e) => Err((buf, e)),
        };
    };
    let mut spill = Ok(spill);
    let write = |spill: &mut std::io::Result<store::Spill>, data: &[u8]| {
        if let Ok(s) = spill {
            if let Err(e) = s.write(data) {
                *spill = Err(e); // keep reading: stopping would kill the command
            }
        }
    };
    write(&mut spill, &buf);
    let mut bytes = buf.len() as u64;
    let mut lines = count_lines(&buf);

    let head_end = line_end_within(&buf, edge);
    let head = buf[..head_end].to_vec();
    let mut middle = Vec::new();
    let mut carry = Vec::new();
    let tail_start = buf.len().saturating_sub(edge).max(head_end);
    scan_important(&buf[head_end..tail_start], &mut carry, &mut middle);
    let mut tail = buf[tail_start..].to_vec();
    drop(buf);

    let mut cut_short = None;
    loop {
        let n = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                cut_short = Some(e);
                break;
            }
        };
        write(&mut spill, &chunk[..n]);
        bytes += n as u64;
        lines += count_lines(&chunk[..n]);
        tail.extend_from_slice(&chunk[..n]);
        if tail.len() > 2 * edge {
            let evict = tail.len() - edge;
            scan_important(&tail[..evict], &mut carry, &mut middle);
            tail.drain(..evict);
        }
    }
    // Start the tail on a whole line. The cut-off piece finishes the line in `carry`,
    // which straddles the middle and the tail: check it like any middle line.
    if let Some(nl) = tail.iter().position(|b| *b == b'\n') {
        scan_important(&tail[..=nl], &mut carry, &mut middle);
        tail.drain(..=nl);
    }
    let mut kept = head;
    if !kept.ends_with(b"\n") && !kept.is_empty() {
        kept.push(b'\n');
    }
    for line in middle {
        kept.extend_from_slice(&line);
        kept.push(b'\n');
    }
    kept.extend_from_slice(&tail);
    Ok(Captured::Spilled {
        kept,
        spill,
        lines,
        bytes,
        cut_short,
    })
}

fn count_lines(b: &[u8]) -> usize {
    b.iter().filter(|c| **c == b'\n').count()
}

/// End of the last whole line within the first `limit` bytes (or `limit` if none).
fn line_end_within(b: &[u8], limit: usize) -> usize {
    let limit = limit.min(b.len());
    b[..limit]
        .iter()
        .rposition(|c| *c == b'\n')
        .map_or(limit, |i| i + 1)
}

/// Collect important lines from `data`, continuing a partial line in `carry`.
fn scan_important(data: &[u8], carry: &mut Vec<u8>, keep: &mut Vec<Vec<u8>>) {
    let mut rest = data;
    while let Some(nl) = rest.iter().position(|b| *b == b'\n') {
        carry.extend_from_slice(&rest[..nl]);
        if keep.len() < MAX_MIDDLE_LINES {
            let line = String::from_utf8_lossy(carry); // borrows when valid UTF-8
            if crate::compress::is_important(&line) {
                keep.push(line.chars().take(400).collect::<String>().into_bytes());
            }
        }
        carry.clear();
        rest = &rest[nl + 1..];
    }
    // Bound a pathological endless line.
    let room = 4096usize.saturating_sub(carry.len());
    carry.extend_from_slice(&rest[..rest.len().min(room)]);
}

/// Like `render`, for output that was too big to keep: the original is already stored.
fn render_spilled(
    kept: &str,
    spill: std::io::Result<store::Spill>,
    (lines, bytes): (usize, u64),
    cut_short: Option<std::io::Error>,
    code: i32,
    program: &str,
    who: &store::Attribution,
) -> String {
    let compressed = compress(kept, code != 0, Limits::default());
    let mut out = compressed.text;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    let note = if compressed.errors_despite_success {
        " exit code 0 but error lines found, so more were kept;"
    } else {
        ""
    };
    if let Some(e) = cut_short {
        out.push_str(&format!(
            "[lagom: reading the command's output failed after {lines} lines ({e}); the rest is missing]\n"
        ));
    }
    match spill.and_then(|s| s.finish(&program_name(program))) {
        Ok(id) => out.push_str(&format!(
            "[lagom: {lines} → {} lines;{note} full output: lagom recall {id} (or part: --grep TEXT, --lines A-B)]\n",
            out.lines().count()
        )),
        Err(e) => out.push_str(&format!(
            "[lagom: {lines} lines, {bytes} bytes; showing start, end and error lines only (could not store the full output: {e})]\n"
        )),
    }
    store::record_for(
        json!({
            "kind": "run",
            "cmd": program_name(program),
            "in": bytes,
            "out": out.len(),
            "exit": code,
        }),
        who,
    );
    out
}

/// Compress, store the original when lines were dropped, record stats.
pub fn render(raw: &str, code: i32, program: &str, who: &store::Attribution) -> String {
    let compressed = compress(raw, code != 0, Limits::default());
    let mut out = compressed.text;
    if compressed.lossy {
        match store::save_original(raw, &program_name(program)) {
            Ok(id) => out.push_str(&format!(
                "[lagom: {} → {} lines;{} full output: lagom recall {id} (or part: --grep TEXT, --lines A-B)]\n",
                raw.lines().count(),
                out.lines().count(),
                if compressed.errors_despite_success {
                    " exit code 0 but error lines found, so more were kept;"
                } else {
                    ""
                }
            )),
            // Without a stored original, never drop anything.
            Err(_) => out = raw.to_string(),
        }
    }
    store::record_for(
        json!({
            "kind": "run",
            "cmd": program_name(program),
            "in": raw.len(),
            "out": out.len(),
            "exit": code,
        }),
        who,
    );
    out
}

fn program_name(program: &str) -> String {
    std::path::Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program)
        .to_string()
}

fn fallback_exec(program: &str, rest: &[String], err: &std::io::Error) -> ExitCode {
    eprintln!("lagom: compression unavailable ({err}); running uncompressed");
    match Command::new(program).args(rest).status() {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(e) => {
            eprintln!("lagom: cannot run `{program}`: {e}");
            ExitCode::from(127)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_output_streams_to_recall_and_keeps_start_end_and_errors() {
        let dir = std::env::temp_dir().join(format!("lagom-cmp05-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut input = String::new();
        for i in 0..20_000 {
            input.push_str(&format!("row {i} ok\n"));
            if i == 9_000 {
                input.push_str("src/db.rs:42: error: connection reset\n");
            }
        }
        let spill_dir = dir.clone();
        let got = capture(input.as_bytes(), 50_000, 5_000, move || {
            store::Spill::create_in(&spill_dir)
        });
        let Ok(Captured::Spilled {
            kept,
            spill: Ok(spill),
            lines,
            bytes,
            cut_short: None,
        }) = got
        else {
            panic!("expected a spill");
        };
        let kept = String::from_utf8(kept).unwrap();
        assert!(kept.starts_with("row 0 ok\n"));
        assert!(kept.ends_with("row 19999 ok\n"));
        assert!(kept.contains("src/db.rs:42: error: connection reset"));
        assert!(kept.len() < 12_000, "kept {} bytes", kept.len());
        assert_eq!((lines, bytes as usize), (20_001, input.len()));
        let id = spill.finish("app").unwrap();
        assert_eq!(
            id,
            store::content_id(&input),
            "same id as an in-memory save"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("{id}.log"))).unwrap(),
            input
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Big output whose important lines sit near the end, around where the tail starts.
    fn errors_near_the_end() -> String {
        let mut input = String::new();
        for i in 0..20_000 {
            input.push_str(&format!("row {i} ok with some padding text\n"));
        }
        for i in 0..150 {
            input.push_str(&format!(
                "src/x.rs:{i}: error: failure number {i} of many\n"
            ));
        }
        input
    }

    #[test]
    fn a_failing_spill_keeps_reading_and_keeps_every_error_line() {
        let dir = std::env::temp_dir().join(format!("lagom-spillfail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let input = errors_near_the_end();
        let spill_dir = dir.clone();
        let got = capture(input.as_bytes(), 50_000, 5_000, move || {
            store::Spill::failing_in(&spill_dir)
        });
        let Ok(Captured::Spilled {
            kept,
            spill: Err(_),
            bytes,
            cut_short: None,
            ..
        }) = got
        else {
            panic!("expected a spill that failed");
        };
        assert_eq!(
            bytes as usize,
            input.len(),
            "read to the end despite the failure"
        );
        let kept = String::from_utf8(kept).unwrap();
        for i in 0..150 {
            let line = format!("src/x.rs:{i}: error: failure number {i} of many");
            assert!(kept.contains(&line), "lost {line}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_read_error_after_spilling_is_reported() {
        struct Breaks(Vec<u8>, usize);
        impl Read for Breaks {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.1 >= self.0.len() {
                    return Err(std::io::Error::other("device gone"));
                }
                let n = buf.len().min(self.0.len() - self.1).min(4096);
                buf[..n].copy_from_slice(&self.0[self.1..self.1 + n]);
                self.1 += n;
                Ok(n)
            }
        }
        let dir = std::env::temp_dir().join(format!("lagom-readerr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let spill_dir = dir.clone();
        let got = capture(
            Breaks(errors_near_the_end().into_bytes(), 0),
            50_000,
            5_000,
            move || store::Spill::create_in(&spill_dir),
        );
        assert!(matches!(
            got,
            Ok(Captured::Spilled {
                cut_short: Some(_),
                ..
            })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
