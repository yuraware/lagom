//! Any shell command: deciding whether and how to rewrite it never panics, and a
//! rewritten command always runs through `lagom run --`.
#![no_main]

use lagom::rewrite::{Wrapped, rewrite, skip_reason};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(command) = std::str::from_utf8(data) else {
        return;
    };
    let extra = ["mytool".to_string()];
    let exclude = ["git".to_string()];
    for wrapped in [
        Wrapped::default(),
        Wrapped {
            extra: &extra,
            exclude: &exclude,
        },
    ] {
        let _ = skip_reason(command, wrapped);
        if let Some(out) = rewrite(command, "/usr/local/bin/lagom", Some("claude:abc"), wrapped) {
            assert!(
                out.contains("/usr/local/bin/lagom run --session claude:abc -- "),
                "{out}"
            );
        }
    }
});
