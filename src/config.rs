//! Lagom's settings, from one loader (ARCH-04). Each setting comes from the first layer
//! that sets it: the environment, the project's `.lagom/config.json` (nearest parent
//! folder), the user's `~/.config/lagom/config.json`, then the default. Only keys that
//! something reads exist; a project file may only turn things off until SEC-01 trusts it.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::style;

/// Keys Lagom reads. Anything else in a config file is reported as unknown.
const KNOWN: [&str; 5] = ["styles", "rewrite", "compress", "hints", "test_note"];

#[derive(Debug, PartialEq)]
pub struct Config {
    /// Active style names; empty means styles are off.
    pub styles: Vec<String>,
    /// Whether the pre-tool hook may rewrite commands to `lagom run`.
    pub rewrite: bool,
    /// Programs added to the wrapped list (`rewrite.extra`, the user's file only).
    pub rewrite_extra: Vec<String>,
    /// Programs taken off it (`rewrite.exclude`, any file).
    pub rewrite_exclude: Vec<String>,
    /// Substrings that make a line important to the compressor (`compress.keep`, any file:
    /// keeping more lines only ever shows more).
    pub compress_keep: Vec<String>,
    /// `compress.lossless` in any file: only transforms that carry no information.
    pub compress_lossless: bool,
    /// `hints`: add a line naming the project's dependencies to the code style (LANG-02).
    /// Off by default until LANG-06 measures it; only the user's own file can turn it on.
    pub hints: bool,
    /// `test_note`: the code style's line about the project's tests. On by default; any file
    /// can turn it off (LANG-06 measures it with and without).
    pub test_note: bool,
    /// Malformed files, wrong value types and unknown keys. `lagom status` shows them,
    /// and `LAGOM_DEBUG` prints them on stderr.
    pub problems: Vec<String>,
}

/// One config file: `Ok(None)` when it doesn't exist, `Err` when it can't be read or isn't
/// a JSON object. Callers ignore a bad file and must never overwrite it.
pub fn read_file(path: &Path) -> Result<Option<Map<String, Value>>, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: can't read it: {e}", path.display())),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(Some(map)),
        Ok(_) => Err(format!("{}: not a JSON object", path.display())),
        Err(e) => Err(format!("{}: not valid JSON ({e})", path.display())),
    }
}

/// Program names for `rewrite.extra` and `rewrite.exclude`: plain words such as `just` or
/// `terraform`, never a path or anything a shell would expand.
fn program_names(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| {
                    !s.is_empty()
                        && s.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
                })
                .map(String::from)
        })
        .collect()
}

/// A list of non-empty strings, such as `compress.keep`.
fn text_list(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|v| v.as_str().filter(|s| !s.is_empty()).map(String::from))
        .collect()
}

/// A config file's contents with the name to report it by.
type Layer<'a> = (&'a str, &'a Map<String, Value>);

impl Config {
    pub fn load(cwd: &Path) -> Config {
        let mut problems = Vec::new();
        let mut read = |path: &Path| match read_file(path) {
            Ok(map) => map.map(|m| (path.display().to_string(), m)),
            Err(e) => {
                problems.push(e);
                None
            }
        };
        let project = style::find_project_file(cwd, "config.json").and_then(|p| read(&p));
        let user = read(&style::config_dir().join("config.json"));
        let mut config = Config::from_layers(
            std::env::var("LAGOM_STYLES").ok().as_deref(),
            std::env::var("LAGOM_REWRITE").ok().as_deref(),
            project.as_ref().map(|(n, m)| (n.as_str(), m)),
            user.as_ref().map(|(n, m)| (n.as_str(), m)),
        );
        problems.append(&mut config.problems);
        config.problems = problems;
        for p in &config.problems {
            crate::debug::log(format_args!("config: {p}"));
        }
        config
    }

    /// The settings from given layers, highest first after the environment.
    pub fn from_layers(
        env_styles: Option<&str>,
        env_rewrite: Option<&str>,
        project: Option<Layer>,
        user: Option<Layer>,
    ) -> Config {
        let mut problems = Vec::new();
        let layers: Vec<Layer> = [project, user].into_iter().flatten().collect();
        let project_name = project.map(|(name, _)| name);
        for (name, map) in &layers {
            for key in map.keys().filter(|k| !KNOWN.contains(&k.as_str())) {
                problems.push(format!("{name}: unknown key `{key}`"));
            }
        }

        let styles = match env_styles {
            Some(s) => style::parse_list(s),
            None => layers
                .iter()
                .find_map(|(name, map)| match map.get("styles")? {
                    Value::Array(items) if items.iter().all(Value::is_string) => Some(
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect(),
                    ),
                    _ => {
                        problems.push(format!("{name}: `styles` must be a list of names"));
                        None
                    }
                })
                .unwrap_or_else(|| style::DEFAULT_STYLES.map(String::from).to_vec()),
        };

        // `rewrite` is `true`/`false`, or `{"enabled", "extra", "exclude"}`. Any file can
        // turn rewriting off or exclude a program; none can turn it back on, and only the
        // user's own file can add programs: a cloned repository may only turn things off
        // until SEC-01 trusts it.
        let (mut off, mut rewrite_extra, mut rewrite_exclude) = (false, Vec::new(), Vec::new());
        for (name, map) in &layers {
            let from_project = Some(*name) == project_name;
            match map.get("rewrite") {
                None => {}
                Some(Value::Bool(on)) => off |= !on,
                Some(Value::Object(fields)) => {
                    for (key, value) in fields {
                        match (key.as_str(), value) {
                            ("enabled", Value::Bool(on)) => off |= !on,
                            ("extra", _) if from_project => problems.push(format!(
                                "{name}: `rewrite.extra` is ignored in a project file; add programs in your own config"
                            )),
                            ("extra" | "exclude", _) => match program_names(value) {
                                Some(names) if key == "extra" => rewrite_extra.extend(names),
                                Some(names) => rewrite_exclude.extend(names),
                                None => problems.push(format!(
                                    "{name}: `rewrite.{key}` must be a list of program names"
                                )),
                            },
                            ("enabled", _) => problems
                                .push(format!("{name}: `rewrite.enabled` must be true or false")),
                            _ => problems.push(format!("{name}: unknown key `rewrite.{key}`")),
                        }
                    }
                }
                Some(_) => problems.push(format!(
                    "{name}: `rewrite` must be true, false or an object"
                )),
            }
        }
        let (mut compress_keep, mut compress_lossless) = (Vec::new(), false);
        for (name, map) in &layers {
            match map.get("compress") {
                None => {}
                Some(Value::Object(fields)) => {
                    for (key, value) in fields {
                        match key.as_str() {
                            "keep" => match text_list(value) {
                                Some(items) => compress_keep.extend(items),
                                None => problems.push(format!(
                                    "{name}: `compress.keep` must be a list of non-empty strings"
                                )),
                            },
                            "lossless" => match value {
                                Value::Bool(on) => compress_lossless |= on,
                                _ => problems.push(format!(
                                    "{name}: `compress.lossless` must be true or false"
                                )),
                            },
                            _ => problems.push(format!("{name}: unknown key `compress.{key}`")),
                        }
                    }
                }
                Some(_) => problems.push(format!("{name}: `compress` must be an object")),
            }
        }

        let (mut hints_on, mut hints_off) = (false, false);
        for (name, map) in &layers {
            match map.get("hints") {
                None => {}
                Some(Value::Bool(false)) => hints_off = true,
                Some(Value::Bool(true)) if Some(*name) == project_name => problems.push(format!(
                    "{name}: `hints` can only be turned on in your own config"
                )),
                Some(Value::Bool(true)) => hints_on = true,
                Some(_) => problems.push(format!("{name}: `hints` must be true or false")),
            }
        }

        let mut test_note = true;
        for (name, map) in &layers {
            match map.get("test_note") {
                None | Some(Value::Bool(true)) => {}
                Some(Value::Bool(false)) => test_note = false,
                Some(_) => problems.push(format!("{name}: `test_note` must be true or false")),
            }
        }

        // `LAGOM_REWRITE` alone decides on or off.
        let rewrite = match env_rewrite {
            Some(v) => !matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            ),
            None => !off,
        };

        Config {
            styles,
            rewrite,
            rewrite_extra,
            rewrite_exclude,
            compress_keep,
            compress_lossless,
            hints: hints_on && !hints_off,
            test_note,
            problems,
        }
    }

    /// The programs the hook wraps, for [`crate::rewrite`].
    pub fn wrapped(&self) -> crate::rewrite::Wrapped<'_> {
        crate::rewrite::Wrapped {
            extra: &self.rewrite_extra,
            exclude: &self.rewrite_exclude,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_test_note_is_on_until_any_file_turns_it_off() {
        let off = map(json!({"test_note": false}));
        assert!(Config::from_layers(None, None, None, None).test_note);
        assert!(!Config::from_layers(None, None, Some(("project", &off)), None).test_note);
        assert!(!Config::from_layers(None, None, None, Some(("user", &off))).test_note);
    }

    #[test]
    fn hints_are_off_until_the_users_own_file_turns_them_on() {
        let on = map(json!({"hints": true}));
        let off = map(json!({"hints": false}));
        assert!(!Config::from_layers(None, None, None, None).hints);
        assert!(Config::from_layers(None, None, None, Some(("user", &on))).hints);
        let project_on = Config::from_layers(None, None, Some(("project", &on)), None);
        assert!(!project_on.hints);
        assert!(
            project_on.problems[0].contains("only be turned on in your own config"),
            "{:?}",
            project_on.problems
        );
        assert!(
            !Config::from_layers(None, None, Some(("project", &off)), Some(("user", &on))).hints
        );
    }

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn styles_come_from_the_first_layer_that_sets_them() {
        let project = map(json!({"styles": ["lean-code"]}));
        let user = map(json!({"styles": ["terse"]}));
        let p = Some(("project", &project));
        let u = Some(("user", &user));
        let c = |e: Option<&str>, p, u| Config::from_layers(e, None, p, u).styles;
        assert_eq!(c(Some("terse, lean-code"), p, u), ["terse", "lean-code"]);
        assert_eq!(c(Some("off"), p, u), Vec::<String>::new());
        assert_eq!(c(None, p, u), ["lean-code"]);
        assert_eq!(c(None, None, u), ["terse"]);
        assert_eq!(c(None, None, None), ["terse", "lean-code"]);
        // A project file without `styles` leaves them to the user's file.
        let other = map(json!({"rewrite": false}));
        assert_eq!(c(None, Some(("project", &other)), u), ["terse"]);
    }

    #[test]
    fn rewrite_can_be_turned_off_by_any_layer_and_on_only_by_the_environment() {
        let off = map(json!({"rewrite": false}));
        let on = map(json!({"rewrite": true}));
        let c = |e: Option<&str>, p, u| Config::from_layers(None, e, p, u).rewrite;
        assert!(c(None, None, None), "default: on");
        assert!(!c(Some("off"), None, None));
        assert!(c(Some("on"), Some(("p", &off)), Some(("u", &off))));
        assert!(!c(None, Some(("p", &off)), None), "project off");
        assert!(!c(None, None, Some(("u", &off))), "user off");
        assert!(
            !c(None, Some(("p", &on)), Some(("u", &off))),
            "project can't undo user off"
        );
        assert!(c(None, Some(("p", &on)), Some(("u", &on))));
    }

    #[test]
    fn rewrite_extra_comes_from_the_user_and_exclude_from_any_file() {
        let user = map(json!({"rewrite": {"extra": ["just", "terraform"], "exclude": ["docker"]}}));
        let project = map(json!({"rewrite": {"extra": ["evil"], "exclude": ["git"]}}));
        let c = Config::from_layers(
            None,
            None,
            Some(("project", &project)),
            Some(("user", &user)),
        );
        assert!(c.rewrite);
        assert_eq!(c.rewrite_extra, ["just", "terraform"]);
        assert_eq!(c.rewrite_exclude, ["git", "docker"]);
        assert_eq!(
            c.problems,
            [
                "project: `rewrite.extra` is ignored in a project file; add programs in your own config"
            ]
        );
        // The object form can turn rewriting off too; the boolean form keeps working.
        let off = map(json!({"rewrite": {"enabled": false}}));
        assert!(!Config::from_layers(None, None, None, Some(("u", &off))).rewrite);
        assert!(
            !Config::from_layers(
                None,
                None,
                None,
                Some(("u", &map(json!({"rewrite": false}))))
            )
            .rewrite
        );
        // Only plain program names count.
        let bad = map(json!({"rewrite": {"extra": ["../bin/x"], "exclude": "git", "speed": 1}}));
        let c = Config::from_layers(None, None, None, Some(("u", &bad)));
        assert!(c.rewrite_extra.is_empty() && c.rewrite_exclude.is_empty());
        assert_eq!(c.problems.len(), 3, "{:?}", c.problems);
    }

    #[test]
    fn compress_lossless_is_on_when_any_file_says_so() {
        let on = map(json!({"compress": {"lossless": true}}));
        let off = map(json!({"compress": {"lossless": false}}));
        let c = |p, u| Config::from_layers(None, None, p, u).compress_lossless;
        assert!(!c(None, None));
        assert!(c(Some(("p", &on)), Some(("u", &off))));
        assert!(c(None, Some(("u", &on))));
        let bad = map(json!({"compress": {"lossless": "yes"}}));
        let cfg = Config::from_layers(None, None, None, Some(("u", &bad)));
        assert!(!cfg.compress_lossless && cfg.problems.len() == 1);
    }

    #[test]
    fn compress_keep_comes_from_every_file() {
        let project = map(json!({"compress": {"keep": ["SECURITY"]}}));
        let user = map(json!({"compress": {"keep": ["panicked at", "ACME-"]}}));
        let c = Config::from_layers(None, None, Some(("p", &project)), Some(("u", &user)));
        assert_eq!(c.compress_keep, ["SECURITY", "panicked at", "ACME-"]);
        assert!(c.problems.is_empty());
        let bad = map(json!({"compress": {"keep": ["", 3], "fast": true}}));
        let c = Config::from_layers(None, None, None, Some(("u", &bad)));
        assert!(c.compress_keep.is_empty());
        assert_eq!(c.problems.len(), 2, "{:?}", c.problems);
    }

    #[test]
    fn wrong_types_and_unknown_keys_are_reported_and_ignored() {
        let bad = map(json!({"styles": "terse", "rewrite": "no", "colour": 1}));
        let c = Config::from_layers(None, None, None, Some(("user", &bad)));
        assert_eq!(c.styles, ["terse", "lean-code"]);
        assert!(c.rewrite);
        assert_eq!(
            c.problems,
            [
                "user: unknown key `colour`",
                "user: `styles` must be a list of names",
                "user: `rewrite` must be true, false or an object",
            ]
        );
    }

    #[test]
    fn read_file_tells_missing_from_malformed() {
        let dir = std::env::temp_dir().join(format!("lagom-config-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_file(&dir.join("none.json")), Ok(None));
        fs::write(dir.join("bad.json"), "{not json").unwrap();
        assert!(
            read_file(&dir.join("bad.json"))
                .unwrap_err()
                .contains("not valid JSON")
        );
        fs::write(dir.join("list.json"), "[1]").unwrap();
        assert!(
            read_file(&dir.join("list.json"))
                .unwrap_err()
                .contains("not a JSON object")
        );
        fs::write(dir.join("ok.json"), r#"{"styles": []}"#).unwrap();
        assert!(read_file(&dir.join("ok.json")).unwrap().is_some());
        let _ = fs::remove_dir_all(&dir);
    }
}
