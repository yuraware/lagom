//! The arguments of one command, checked against what it accepts (ARCH-03). Every command
//! uses this, so a mistyped flag stops with the command's usage instead of being ignored.

use std::process::ExitCode;

/// Switches (`--yes`), options with a value (`--days 7` or `--days=7`) and other words.
#[derive(Debug, Default, PartialEq)]
pub struct Args {
    switches: Vec<&'static str>,
    options: Vec<(&'static str, String)>,
    pub positionals: Vec<String>,
}

impl Args {
    /// `spec` names what the command accepts: a switch as `--name`, with aliases after `|`
    /// (`--yes|-y`), and an option that takes a value with a trailing `=` (`--days=`).
    /// At most `max_positionals` other words. Anything else is an error that names it.
    pub fn parse(
        args: &[String],
        spec: &[&'static str],
        max_positionals: usize,
    ) -> Result<Args, String> {
        let mut out = Args::default();
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            if !arg.starts_with('-') || arg == "-" {
                if out.positionals.len() == max_positionals {
                    return Err(format!("unexpected argument `{arg}`"));
                }
                out.positionals.push(arg.clone());
                continue;
            }
            let (name, inline) = match arg.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (arg.as_str(), None),
            };
            let Some(entry) = spec
                .iter()
                .find(|s| s.trim_end_matches('=').split('|').any(|n| n == name))
            else {
                return Err(format!("unknown option `{arg}`"));
            };
            let canonical = entry.trim_end_matches('=').split('|').next().unwrap_or("");
            if entry.ends_with('=') {
                let value = match inline {
                    Some(v) => v.to_string(),
                    None => it
                        .next()
                        .ok_or_else(|| format!("{canonical} needs a value"))?
                        .clone(),
                };
                out.options.push((canonical, value));
            } else if inline.is_some() {
                return Err(format!("{canonical} takes no value"));
            } else {
                out.switches.push(canonical);
            }
        }
        Ok(out)
    }

    /// Whether a switch was given, by its first name in the spec.
    pub fn has(&self, name: &str) -> bool {
        self.switches.contains(&name)
    }

    /// An option's value; the last one wins when it is given twice.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Prints the problem and the command's usage, and returns exit code 2.
pub fn usage_error(problem: &str, usage: &str) -> ExitCode {
    eprintln!("lagom: {problem}\nusage: {usage}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], spec: &[&'static str], max: usize) -> Result<Args, String> {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        Args::parse(&args, spec, max)
    }

    #[test]
    fn switches_options_and_positionals() {
        let a = parse(
            &["claude", "-y", "--days=7", "codex", "--days", "9"],
            &["--yes|-y", "--days="],
            2,
        )
        .unwrap();
        assert!(a.has("--yes"));
        assert_eq!(a.value("--days"), Some("9"));
        assert_eq!(a.positionals, ["claude", "codex"]);
        assert!(!a.has("--days"));
    }

    #[test]
    fn anything_unexpected_is_named() {
        let err = |args: &[&str]| parse(args, &["--yes", "--days="], 1).unwrap_err();
        assert_eq!(err(&["--anything"]), "unknown option `--anything`");
        assert_eq!(err(&["-x"]), "unknown option `-x`");
        assert_eq!(err(&["a", "b"]), "unexpected argument `b`");
        assert_eq!(err(&["--days"]), "--days needs a value");
        assert_eq!(err(&["--yes=1"]), "--yes takes no value");
        // A lone `-` (stdin) and negative-looking values after an option are words.
        assert!(parse(&["-"], &[], 1).is_ok());
        assert_eq!(
            parse(&["--days", "-1"], &["--days="], 0)
                .unwrap()
                .value("--days"),
            Some("-1")
        );
    }
}
