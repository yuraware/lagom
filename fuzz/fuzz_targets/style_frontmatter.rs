//! Any style file: reading its frontmatter never panics, and once its `<lagom>` tags are
//! neutralized none is left, so a style can't close Lagom's block early.
#![no_main]

use lagom::style::{Style, neutralize_tags, parse_list, strip_frontmatter};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = strip_frontmatter(text);
    let _ = parse_list(text);
    let style = Style {
        name: "fuzz".into(),
        source: "project",
        path: None,
        raw: text.into(),
    };
    let _ = style.description();
    let once = neutralize_tags(&style.body());
    assert_eq!(neutralize_tags(&once), once, "a lagom tag survived");
});
