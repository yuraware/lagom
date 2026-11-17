//! Decide whether an agent's shell command should run through `lagom run`.
//!
//! Only simple commands are rewritten: one known program, no shell operators.
//! Prefixing such a command keeps the shell's own quoting, globbing and `$VAR`
//! expansion intact, so behavior does not change; only the output gets shorter.

/// Programs whose output is usually long and safe to compress.
const PROGRAMS: [&str; 44] = [
    "git",
    "cargo",
    "go",
    "npm",
    "pnpm",
    "yarn",
    "bun",
    "npx",
    "node",
    "deno",
    "python",
    "python3",
    "pytest",
    "pip",
    "pip3",
    "uv",
    "poetry",
    "ruff",
    "mypy",
    "tsc",
    "eslint",
    "jest",
    "vitest",
    "make",
    "cmake",
    "gradle",
    "mvn",
    "dotnet",
    "swift",
    "xcodebuild",
    "flutter",
    "dart",
    "docker",
    "kubectl",
    "terraform",
    "ls",
    "find",
    "grep",
    "rg",
    "tree",
    "brew",
    "gh",
    "rustc",
    "clippy-driver",
];

/// Arguments that usually mean a long-running or interactive process; their output must stream.
const STREAMING_ARGS: [&str; 12] = [
    "dev", "serve", "start", "watch", "--watch", "-w", "-f", "--follow", "repl", "shell", "attach",
    "-it",
];

/// Characters that make a command more than "one program with arguments".
const SHELL_OPERATORS: [char; 9] = ['|', '&', ';', '<', '>', '`', '\n', '(', ')'];

/// A leading `cd <dir> && <command>`, the most common compound command agents write:
/// `(dir, command)`. The directory must be a plain word or a single-quoted string with
/// no shell metacharacters, variables or globs.
pub fn split_cd(command: &str) -> Option<(&str, &str)> {
    let rest = command.trim().strip_prefix("cd ")?;
    let (dir, cmd) = rest.split_once("&&")?;
    let (dir, cmd) = (dir.trim(), cmd.trim());
    let bare = dir
        .strip_prefix('\'')
        .and_then(|d| d.strip_suffix('\''))
        .unwrap_or(dir);
    let quoted = bare.len() != dir.len();
    // `cd -` goes to the previous folder, `cd --` and `cd -P` to the home folder: none of
    // them is the path they look like, so options are never accepted.
    let ok = !bare.is_empty()
        && !bare.starts_with('-')
        && !bare.contains(['\'', '"', '$', '`', '\\', '*', '?', '[', '~', '{'])
        && !bare.contains(SHELL_OPERATORS)
        && (quoted || !bare.contains(char::is_whitespace));
    (ok && !cmd.is_empty()).then_some((bare, cmd))
}

/// Why `command` would not be rewritten, or `None` if it would be.
pub fn skip_reason(command: &str, wrapped: Wrapped) -> Option<&'static str> {
    match split_cd(command) {
        Some((_, cmd)) => single_command_skip_reason(cmd, wrapped),
        None => single_command_skip_reason(command, wrapped),
    }
}

fn single_command_skip_reason(command: &str, wrapped: Wrapped) -> Option<&'static str> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Some("empty command");
    }
    if trimmed.contains(SHELL_OPERATORS) || trimmed.contains("$(") {
        return Some("pipes, &&, ;, redirects and subshells are never rewritten");
    }
    let mut words = trimmed.split_whitespace();
    let first = words.next().unwrap_or_default();
    if first.contains('=') {
        return Some("starts with an environment assignment");
    }
    if first.contains('/') {
        return Some("program given by path");
    }
    if let Some(why) = wrapped.reason_to_skip(first) {
        return Some(why);
    }
    if words.any(|w| STREAMING_ARGS.contains(&w.trim_matches(['"', '\'']))) {
        return Some("long-running or watch mode");
    }
    None
}

/// True for programs on Lagom's built-in list (`git`, `cargo`, …).
pub fn is_known_program(name: &str) -> bool {
    PROGRAMS.contains(&name)
}

/// Which programs are wrapped: the built-in list, plus `rewrite.extra` from the user's
/// config, minus `rewrite.exclude` (FLT-18). The default is the built-in list.
#[derive(Clone, Copy, Debug, Default)]
pub struct Wrapped<'a> {
    pub extra: &'a [String],
    pub exclude: &'a [String],
}

impl Wrapped<'_> {
    fn reason_to_skip(&self, program: &str) -> Option<&'static str> {
        if self.exclude.iter().any(|p| p == program) {
            Some("program excluded by rewrite.exclude")
        } else if PROGRAMS.contains(&program) || self.extra.iter().any(|p| p == program) {
            None
        } else {
            Some("program not in Lagom's rewrite list")
        }
    }
}

/// Returns the rewritten command, or None to leave it unchanged.
/// `session` is an `agent:id` token for stats; it is dropped unless it's a plain id.
pub fn rewrite(
    command: &str,
    lagom_bin: &str,
    session: Option<&str>,
    wrapped: Wrapped,
) -> Option<String> {
    if skip_reason(command, wrapped).is_some() {
        return None;
    }
    let session = session
        .filter(|s| {
            s.split_once(':')
                .is_some_and(|(a, i)| crate::store::is_safe_id(a) && crate::store::is_safe_id(i))
        })
        .map(|s| format!(" --session {s}"))
        .unwrap_or_default();
    let run = format!("{} run{session} --", shell_quote(lagom_bin));
    Some(match split_cd(command) {
        // Keep the `cd` exactly as written; compress the command after it.
        Some((_, cmd)) => {
            let trimmed = command.trim();
            let cd = trimmed[..trimmed.find("&&").unwrap_or(0)].trim_end();
            format!("{cd} && {run} {cmd}")
        }
        None => format!("{run} {}", command.trim()),
    })
}

/// Quote a path for POSIX shells only when needed.
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+:@".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_adds_programs_and_exclude_removes_them() {
        let (extra, exclude) = (vec!["just".to_string()], vec!["docker".to_string()]);
        let wrapped = Wrapped {
            extra: &extra,
            exclude: &exclude,
        };
        assert!(rewrite("just build", BIN, None, wrapped).is_some());
        assert_eq!(rewrite("docker ps", BIN, None, wrapped), None);
        assert_eq!(
            skip_reason("docker ps", wrapped),
            Some("program excluded by rewrite.exclude")
        );
        assert!(rw("docker ps", BIN, None).is_some(), "on the built-in list");
        assert_eq!(rw("just build", BIN, None), None);
        // Excluding wins over adding, and the streaming checks still apply to added programs.
        let both = vec!["just".to_string()];
        let w = Wrapped {
            extra: &extra,
            exclude: &both,
        };
        assert_eq!(rewrite("just build", BIN, None, w), None);
        // Output already cut to a budget stays off the default list (OVERVIEW §11).
        assert_eq!(
            skip_reason("graphify query auth", Wrapped::default()),
            Some("program not in Lagom's rewrite list")
        );
    }

    /// `rewrite` with the built-in program list.
    fn rw(command: &str, bin: &str, session: Option<&str>) -> Option<String> {
        rewrite(command, bin, session, Wrapped::default())
    }

    const BIN: &str = "/usr/local/bin/lagom";

    #[test]
    fn cd_options_are_never_split() {
        for c in [
            "cd - && git log",
            "cd -- && git log",
            "cd -P && git log",
            "cd '-' && git log",
        ] {
            assert_eq!(split_cd(c), None, "{c}");
            assert_eq!(rw(c, BIN, None), None, "{c}");
        }
    }

    #[test]
    fn cd_prefix_is_kept_and_the_command_compressed() {
        assert_eq!(
            rw("cd src && cargo test", BIN, None).unwrap(),
            format!("cd src && {BIN} run -- cargo test")
        );
        assert_eq!(
            rw("cd 'my dir/x' && git log -3", BIN, None).unwrap(),
            format!("cd 'my dir/x' && {BIN} run -- git log -3")
        );
        assert_eq!(
            split_cd("cd crates/core && cargo test"),
            Some(("crates/core", "cargo test"))
        );
        for cmd in [
            "cd $HOME && git log",
            "cd ~/x && git log",
            "cd x && git log | head",
            "cd x && cd y && git log",
            "cd && git log",
            "cd x; git log",
            "cd \"x\" && git log",
            "cd a b && git log",
            "cd x && npm run dev",
            "cd x && echo hi",
            "cd `pwd` && git log",
        ] {
            assert_eq!(rw(cmd, BIN, None), None, "{cmd}");
        }
    }

    #[test]
    fn session_token_is_added_only_when_safe() {
        assert_eq!(
            rw("git log", BIN, Some("claude:9b2f-11")).unwrap(),
            format!("{BIN} run --session claude:9b2f-11 -- git log")
        );
        for bad in ["claude:a b", "claude:$(rm)", "no-colon", "claude:", "x;y:z"] {
            assert_eq!(
                rw("git log", BIN, Some(bad)).unwrap(),
                format!("{BIN} run -- git log"),
                "{bad}"
            );
        }
    }

    #[test]
    fn simple_known_commands_are_wrapped() {
        assert_eq!(
            rw("git status", BIN, None).unwrap(),
            "/usr/local/bin/lagom run -- git status"
        );
        assert_eq!(
            rw("git commit -m \"fix: x y\"", BIN, None).unwrap(),
            "/usr/local/bin/lagom run -- git commit -m \"fix: x y\""
        );
        assert!(rw("cargo test --all", BIN, None).is_some());
    }

    #[test]
    fn compound_or_unknown_commands_are_left_alone() {
        for cmd in [
            "git log | head",
            "cd src; cargo test", // `cd <dir> &&` is supported, `;` is not
            "npm test > out.txt",
            "echo hi",
            "FOO=1 cargo test",
            "LAGOM_RAW=1 git log", // documented escape hatch
            "command git log",     // documented escape hatch
            "env git log",
            "./script.sh",
            "git log $(git rev-parse HEAD)",
            "lagom run -- git status",
            "",
        ] {
            assert_eq!(rw(cmd, BIN, None), None, "{cmd}");
        }
    }

    #[test]
    fn streaming_commands_are_left_alone() {
        assert_eq!(rw("npm run dev", BIN, None), None);
        assert_eq!(rw("cargo watch", BIN, None), None);
        assert_eq!(rw("docker logs -f api", BIN, None), None);
    }

    #[test]
    fn paths_with_spaces_are_quoted() {
        assert_eq!(shell_quote("/Users/a b/lagom"), "'/Users/a b/lagom'");
        assert_eq!(shell_quote("/usr/bin/lagom"), "/usr/bin/lagom");
    }
}
