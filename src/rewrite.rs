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

/// Returns the rewritten command, or None to leave it unchanged.
pub fn rewrite(command: &str, lagom_bin: &str) -> Option<String> {
    let trimmed = command.trim();
    if trimmed.is_empty() || trimmed.contains(SHELL_OPERATORS) || trimmed.contains("$(") {
        return None;
    }
    let mut words = trimmed.split_whitespace();
    let first = words.next()?;
    if first.contains('=') || first.contains('/') {
        return None; // env assignment or explicit path: leave as written
    }
    if !PROGRAMS.contains(&first) {
        return None;
    }
    if words.any(|w| STREAMING_ARGS.contains(&w.trim_matches(['"', '\'']))) {
        return None;
    }
    Some(format!("{} run -- {trimmed}", shell_quote(lagom_bin)))
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
    fn simple_known_commands_are_wrapped() {
        assert_eq!(
            rewrite("git status", BIN).unwrap(),
            "/usr/local/bin/lagom run -- git status"
        );
        assert_eq!(
            rewrite("git commit -m \"fix: x y\"", BIN).unwrap(),
            "/usr/local/bin/lagom run -- git commit -m \"fix: x y\""
        );
        assert!(rewrite("cargo test --all", BIN).is_some());
    }

    #[test]
    fn compound_or_unknown_commands_are_left_alone() {
        for cmd in [
            "git log | head",
            "cd src && cargo test",
            "npm test > out.txt",
            "echo hi",
            "FOO=1 cargo test",
            "./script.sh",
            "git log $(git rev-parse HEAD)",
            "lagom run -- git status",
            "",
        ] {
            assert_eq!(rewrite(cmd, BIN), None, "{cmd}");
        }
    }

    #[test]
    fn streaming_commands_are_left_alone() {
        assert_eq!(rewrite("npm run dev", BIN), None);
        assert_eq!(rewrite("cargo watch", BIN), None);
        assert_eq!(rewrite("docker logs -f api", BIN), None);
    }

    #[test]
    fn paths_with_spaces_are_quoted() {
        assert_eq!(shell_quote("/Users/a b/lagom"), "'/Users/a b/lagom'");
        assert_eq!(shell_quote("/usr/bin/lagom"), "/usr/bin/lagom");
    }
}
