//! Any command output: compression never panics, in each mode and exit status.
#![no_main]

use lagom::compress::{Options, compress, compress_with};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&flags, rest)) = data.split_first() else {
        return;
    };
    let raw = String::from_utf8_lossy(rest);
    let failed = flags & 1 == 1;
    let _ = compress(&raw, failed);
    let keep = ["TODO".to_string()];
    let opts = Options {
        keep: &keep,
        lossless: flags & 2 == 2,
    };
    let _ = compress_with(&raw, failed, &opts);
});
