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

/// Why `command` would not be rewritten, or `None` if it would be.
pub fn skip_reason(command: &str) -> Option<&'static str> {
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
    if !PROGRAMS.contains(&first) {
        return Some("program not in Lagom's rewrite list");
    }
    if words.any(|w| STREAMING_ARGS.contains(&w.trim_matches(['"', '\'']))) {
        return Some("long-running or watch mode");
    }
    None
}

/// True for programs Lagom rewrites (`git`, `cargo`, …).
pub fn is_known_program(name: &str) -> bool {
    PROGRAMS.contains(&name)
}

/// Returns the rewritten command, or None to leave it unchanged.
/// `session` is an `agent:id` token for stats; it is dropped unless it's a plain id.
pub fn rewrite(command: &str, lagom_bin: &str, session: Option<&str>) -> Option<String> {
    if skip_reason(command).is_some() {
        return None;
    }
    let trimmed = command.trim();
    let session = session
        .filter(|s| {
            s.split_once(':')
                .is_some_and(|(a, i)| crate::store::is_safe_id(a) && crate::store::is_safe_id(i))
        })
        .map(|s| format!(" --session {s}"))
        .unwrap_or_default();
    Some(format!(
        "{} run{session} -- {trimmed}",
        shell_quote(lagom_bin)
    ))
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

    const BIN: &str = "/usr/local/bin/lagom";

    #[test]
    fn session_token_is_added_only_when_safe() {
        assert_eq!(
            rewrite("git log", BIN, Some("claude:9b2f-11")).unwrap(),
            format!("{BIN} run --session claude:9b2f-11 -- git log")
        );
        for bad in ["claude:a b", "claude:$(rm)", "no-colon", "claude:", "x;y:z"] {
            assert_eq!(
                rewrite("git log", BIN, Some(bad)).unwrap(),
                format!("{BIN} run -- git log"),
                "{bad}"
            );
        }
    }

    #[test]
    fn simple_known_commands_are_wrapped() {
        assert_eq!(
            rewrite("git status", BIN, None).unwrap(),
            "/usr/local/bin/lagom run -- git status"
        );
        assert_eq!(
            rewrite("git commit -m \"fix: x y\"", BIN, None).unwrap(),
            "/usr/local/bin/lagom run -- git commit -m \"fix: x y\""
        );
        assert!(rewrite("cargo test --all", BIN, None).is_some());
    }

    #[test]
    fn compound_or_unknown_commands_are_left_alone() {
        for cmd in [
            "git log | head",
            "cd src && cargo test",
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
            assert_eq!(rewrite(cmd, BIN, None), None, "{cmd}");
        }
    }

    #[test]
    fn streaming_commands_are_left_alone() {
        assert_eq!(rewrite("npm run dev", BIN, None), None);
        assert_eq!(rewrite("cargo watch", BIN, None), None);
        assert_eq!(rewrite("docker logs -f api", BIN, None), None);
    }

    #[test]
    fn paths_with_spaces_are_quoted() {
        assert_eq!(shell_quote("/Users/a b/lagom"), "'/Users/a b/lagom'");
        assert_eq!(shell_quote("/usr/bin/lagom"), "/usr/bin/lagom");
    }
}
