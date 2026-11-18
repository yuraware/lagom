//! Lagom's settings, from one loader (ARCH-04). Each setting comes from the first layer
//! that sets it: the environment, the project's `.lagom/config.json` (nearest parent
//! folder), the user's `~/.config/lagom/config.json`, then the default. Only keys that
//! something reads exist; a project file may only turn things off until SEC-01 trusts it.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::style;

/// Keys Lagom reads. Anything else in a config file is reported as unknown.
const KNOWN: [&str; 2] = ["styles", "rewrite"];

#[derive(Debug, PartialEq)]
pub struct Config {
    /// Active style names; empty means styles are off.
    pub styles: Vec<String>,
    /// Whether the pre-tool hook may rewrite commands to `lagom run`.
    pub rewrite: bool,
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

        // `LAGOM_REWRITE` decides alone. Otherwise any file can turn rewriting off, and
        // none can turn it back on: a cloned repository must not undo the user's `false`.
        let rewrite = match env_rewrite {
            Some(v) => !matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            ),
            None => !layers.iter().any(|(name, map)| match map.get("rewrite") {
                None => false,
                Some(Value::Bool(on)) => !on,
                Some(_) => {
                    problems.push(format!("{name}: `rewrite` must be true or false"));
                    false
                }
            }),
        };

        Config {
            styles,
            rewrite,
            problems,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
                "user: `rewrite` must be true or false",
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
