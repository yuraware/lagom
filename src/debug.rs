//! `LAGOM_DEBUG=1` (ARCH-05): one line per decision on stderr and in `~/.lagom/debug.log`
//! (1 MiB, then moved to `debug.1.log`). Hooks print nothing on errors by design, so this
//! is how a user sees why one did nothing. Sizes, ids and decisions only: never a
//! payload, a command's text or its output.

use std::fmt::Display;
use std::io::Write;

/// `debug.log` moves to `debug.1.log`, replacing it, once it reaches this size.
const DEBUG_CAP: u64 = 1024 * 1024;

/// Whether `LAGOM_DEBUG` is set to something other than empty, `0`, `off` or `false`.
pub fn enabled() -> bool {
    std::env::var("LAGOM_DEBUG").is_ok_and(|v| {
        !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "off" | "false" | "no"
        )
    })
}

/// Writes `lagom: <message>` when debugging is on. Errors are ignored: a debug line must
/// never change what a command or hook does.
pub fn log(message: impl Display) {
    if !enabled() {
        return;
    }
    let line = format!("lagom: {message}");
    let _ = writeln!(std::io::stderr(), "{line}");
    crate::store::append_capped(
        "debug.log",
        "debug.1.log",
        DEBUG_CAP,
        &format!("{} {line}", crate::store::now_secs()),
    );
}
