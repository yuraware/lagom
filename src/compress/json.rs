//! JSON output (`gh api`, `kubectl -o json`, `jq`, `cargo metadata`, JSON Lines logs).
//!
//! Arrays longer than [`MAX_ITEMS`] keep their first and last items plus every item that
//! reports a problem (`status: "failed"`, `status >= 400`, `success: false`, a non-null
//! `error`, `level: "error"` …). Dropped runs become one marker string. Output is compact.

use serde_json::Value;

use super::Limits;

const MAX_ITEMS: usize = 20;
const KEEP_HEAD: usize = 3;
const KEEP_TAIL: usize = 2;

/// True if an item reports a failure or error.
fn is_interesting(v: &Value) -> bool {
    let Some(obj) = v.as_object() else {
        return false;
    };
    let truthy = |v: &Value| {
        !matches!(v, Value::Null | Value::Bool(false)) && v != "" && v != &Value::Array(vec![])
    };
    obj.iter().any(|(k, v)| {
        let k = k.to_ascii_lowercase();
        let word = |w: &str| {
            v.as_str()
                .is_some_and(|s| s.to_ascii_lowercase().contains(w))
        };
        match k.as_str() {
            "error" | "errors" | "err" | "exception" | "failure" | "failures" => truthy(v),
            "status" | "state" | "result" | "conclusion" | "outcome" => {
                v.as_u64().is_some_and(|n| n >= 400)
                    || word("fail")
                    || word("error")
                    || word("crash")
            }
            "code" | "status_code" | "statuscode" | "http_status" => {
                v.as_u64().is_some_and(|n| n >= 400)
            }
            "success" | "ok" | "passed" => v == &Value::Bool(false),
            "failed" => v == &Value::Bool(true),
            "level" | "severity" | "lvl" => {
                word("err") || word("fatal") || word("crit") || word("warn")
            }
            _ => false,
        }
    })
}

fn keys_of(items: &[Value]) -> String {
    let mut keys: Vec<&str> = Vec::new();
    for item in items.iter().take(50) {
        if let Some(o) = item.as_object() {
            for k in o.keys() {
                if !keys.contains(&k.as_str()) {
                    keys.push(k);
                }
            }
        }
    }
    keys.into_iter().take(12).collect::<Vec<_>>().join(", ")
}

/// Which indexes of `items` to keep.
fn keep_indexes(items: &[Value]) -> Vec<usize> {
    let n = items.len();
    (0..n)
        .filter(|&i| i < KEEP_HEAD || i >= n - KEEP_TAIL || is_interesting(&items[i]))
        .collect()
}

/// Shorten long arrays everywhere in `v`. Returns how many items were dropped.
fn shorten(v: &mut Value) -> usize {
    match v {
        Value::Array(items) => {
            let mut dropped: usize = items.iter_mut().map(shorten).sum();
            if items.len() > MAX_ITEMS {
                let keep = keep_indexes(items);
                let keys = keys_of(items);
                let old = std::mem::take(items);
                let mut last: Option<usize> = None;
                let mut first_marker = true;
                for &i in &keep {
                    let gap = i - last.map_or(0, |l| l + 1);
                    if gap > 0 {
                        items.push(Value::String(marker(gap, &keys, first_marker)));
                        first_marker = false;
                        dropped += gap;
                    }
                    items.push(old[i].clone());
                    last = Some(i);
                }
            }
            dropped
        }
        Value::Object(map) => map.values_mut().map(shorten).sum(),
        _ => 0,
    }
}

fn marker(gap: usize, keys: &str, with_keys: bool) -> String {
    if with_keys && !keys.is_empty() {
        format!("… {gap} more items (keys: {keys})")
    } else {
        format!("… {gap} more items")
    }
}

/// `Some((text, lossy))` if `text` is JSON or JSON Lines worth shortening.
pub fn compress(text: &str, limits: Limits) -> Option<(String, bool)> {
    let line_count = text.lines().count();
    let has_long_line = text.lines().any(|l| l.len() > limits.max_line_chars);
    if line_count <= limits.max_lines && !has_long_line {
        return None;
    }
    let trimmed = text.trim();
    if trimmed.starts_with(['{', '[']) {
        if let Ok(mut v) = serde_json::from_str::<Value>(trimmed) {
            let dropped = shorten(&mut v);
            let mut out = serde_json::to_string(&v).ok()?;
            out.push('\n');
            return Some((out, dropped > 0));
        }
    }
    json_lines(text)
}

/// One JSON value per line (`jq -c`, structured logs).
fn json_lines(text: &str) -> Option<(String, bool)> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let parsed: Vec<Option<Value>> = lines
        .iter()
        .map(|l| {
            let l = l.trim();
            if l.starts_with('{') {
                serde_json::from_str(l).ok()
            } else {
                None
            }
        })
        .collect();
    let ok = parsed.iter().filter(|p| p.is_some()).count();
    if lines.len() <= MAX_ITEMS || ok * 10 < lines.len() * 9 {
        return None;
    }
    // Lines that aren't JSON (a trailing error message, say) are always kept.
    let items: Vec<Value> = parsed
        .iter()
        .map(|p| p.clone().unwrap_or(Value::Null))
        .collect();
    let keys = keys_of(&items);
    let n = lines.len();
    let mut out = String::new();
    let (mut last, mut first_marker, mut dropped) = (None::<usize>, true, 0usize);
    for i in 0..n {
        let keep =
            i < KEEP_HEAD || i >= n - KEEP_TAIL || parsed[i].is_none() || is_interesting(&items[i]);
        if !keep {
            continue;
        }
        let gap = i - last.map_or(0, |l| l + 1);
        if gap > 0 {
            out.push_str(&format!(
                "{}\n",
                marker(gap, &keys, first_marker).replacen(" items", " JSON lines", 1)
            ));
            first_marker = false;
            dropped += gap;
        }
        match &parsed[i] {
            Some(v) => out.push_str(&serde_json::to_string(v).ok()?),
            None => out.push_str(lines[i]),
        }
        out.push('\n');
        last = Some(i);
    }
    Some((out, dropped > 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn items(n: usize) -> Vec<Value> {
        (0..n)
            .map(|i| json!({"id": i, "status": if i % 37 == 0 && i > 0 { "failed" } else { "ok" },
                            "error": if i % 37 == 0 && i > 0 { json!(format!("timeout after {i}ms")) } else { Value::Null }}))
            .collect()
    }

    #[test]
    fn long_arrays_keep_ends_and_failures() {
        let text =
            serde_json::to_string_pretty(&json!({"items": items(400), "total": 400})).unwrap();
        let (out, lossy) = compress(&text, Limits::default()).unwrap();
        assert!(lossy);
        let v: Value = serde_json::from_str(&out).expect("output stays valid JSON");
        let kept = v["items"].as_array().unwrap();
        assert!(
            out.contains("\"id\":0") && out.contains("\"id\":399"),
            "ends kept"
        );
        for i in (37..400).step_by(37) {
            assert!(
                out.contains(&format!("timeout after {i}ms")),
                "failure {i} kept"
            );
        }
        assert!(out.contains("more items (keys: id, status, error)"));
        assert!(kept.len() < 40 && out.contains("\"total\":400"));
        // What the agent sees carries `lagom run`'s recall footer; that is left alone.
        let seen = format!("{out}[lagom: 1 → 1 lines; full output: lagom recall 0123456789]\n");
        let again = super::super::compress(&seen, false, Limits::default());
        assert_eq!(again.text, seen, "compressing twice changes nothing");
    }

    #[test]
    fn json_lines_keep_failures_and_non_json_lines() {
        let mut text: String = items(400).iter().map(|v| format!("{v}\n")).collect();
        text.push_str("error: stream ended early\n");
        let (out, _) = compress(&text, Limits::default()).unwrap();
        assert!(out.contains("timeout after 74ms") && out.contains("error: stream ended early"));
        assert!(out.contains("JSON lines"));
        assert!(out.lines().count() < 40);
    }

    #[test]
    fn interesting_items() {
        for v in [
            json!({"status": 503}),
            json!({"success": false}),
            json!({"failed": true}),
            json!({"level": "ERROR"}),
            json!({"error": {"code": 1}}),
            json!({"conclusion": "failure"}),
        ] {
            assert!(is_interesting(&v), "{v}");
        }
        for v in [
            json!({"error": null}),
            json!({"status": "ok"}),
            json!({"status": 200}),
            json!({"errors": []}),
            json!({"success": true}),
            json!(3),
        ] {
            assert!(!is_interesting(&v), "{v}");
        }
    }

    #[test]
    fn small_or_non_json_output_is_left_alone() {
        assert!(compress("{\"a\": 1}\n", Limits::default()).is_none());
        let prose: String = (0..300).map(|i| format!("line {i}\n")).collect();
        assert!(compress(&prose, Limits::default()).is_none());
    }
}
