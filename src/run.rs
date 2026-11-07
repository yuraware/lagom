//! `lagom run -- <cmd> [args…]`: run a command, compress its combined output,
//! keep its exit code. Anything going wrong inside Lagom falls back to the raw output.

use std::io::{Read, Write};
use std::process::{Command, ExitCode};

use serde_json::json;

use crate::compress::{Limits, compress};
use crate::store;

pub fn run(args: &[String]) -> ExitCode {
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

    let mut raw_bytes = Vec::new();
    let mut reader = reader;
    let read_ok = reader.read_to_end(&mut raw_bytes).is_ok();
    let status = child.wait();
    let code = status.as_ref().ok().and_then(|s| s.code()).unwrap_or(1);

    let raw = String::from_utf8_lossy(&raw_bytes);
    let output = if read_ok {
        render(&raw, code, program)
    } else {
        raw.to_string()
    };

    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(output.as_bytes());
    let _ = stdout.flush();
    ExitCode::from(code.clamp(0, 255) as u8)
}

/// Compress, store the original when lines were dropped, record stats.
pub fn render(raw: &str, code: i32, program: &str) -> String {
    let compressed = compress(raw, code != 0, Limits::default());
    let mut out = compressed.text;
    if compressed.lossy {
        match store::save_original(raw) {
            Ok(id) => out.push_str(&format!(
                "[lagom: {} → {} lines; full output: lagom recall {id} (or part: --grep TEXT, --lines A-B)]\n",
                raw.lines().count(),
                out.lines().count()
            )),
            // Without a stored original, never drop anything.
            Err(_) => out = raw.to_string(),
        }
    }
    store::record(json!({
        "kind": "run",
        "cmd": program_name(program),
        "in": raw.len(),
        "out": out.len(),
        "exit": code,
    }));
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
