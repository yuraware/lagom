//! The rewrite safety table (SEC-06): commands an agent might run, each with whether
//! Lagom rewrites it to `lagom run -- …` or leaves it alone. A rewrite only puts
//! `lagom run --` in front of the command, so it must be one the shell reads exactly as
//! Lagom does; anything ambiguous is left alone. Add a row for every new rule or bug.

use lagom::rewrite::{Wrapped, rewrite, split_cd};

const BIN: &str = "/usr/local/bin/lagom";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Expect {
    Rewrite,
    Skip,
}
use Expect::{Rewrite as R, Skip as S};

const TABLE: &[(Expect, &str)] = &[
    // Plain commands of programs on the list.
    (R, "git status"),
    (R, "git diff --stat"),
    (R, "git log --oneline -20"),
    (R, "cargo test"),
    (R, "cargo build --release"),
    (R, "cargo clippy --all-targets -- -D warnings"),
    (R, "go test ./..."),
    (R, "npm test"),
    (R, "npm install"),
    (R, "pnpm build"),
    (R, "yarn lint"),
    (R, "pytest -q"),
    (R, "python3 -m pytest tests"),
    (R, "python script.py"),
    (R, "pip install -r requirements.txt"),
    (R, "make"),
    (R, "make test"),
    (R, "ls -la"),
    (R, "tree src"),
    (R, "rg TODO"),
    (R, "grep -rn needle src"),
    (R, "find . -name Cargo.toml"),
    (R, "docker ps"),
    (R, "kubectl get pods"),
    (R, "terraform plan"),
    (R, "gh pr list"),
    (R, "brew list"),
    (R, "tsc --noEmit"),
    (R, "eslint ."),
    (R, "mvn -q package"),
    (R, "gradle build"),
    (R, "dotnet test"),
    (R, "swift build"),
    (R, "flutter test"),
    (R, "uv run pytest"),
    (R, "ruff check ."),
    (R, "mypy src"),
    // Programs not on the list, builtins, keywords and wrappers.
    (S, "cat README.md"),
    (S, "echo hello"),
    (S, "rm -rf target"),
    (S, "sudo git status"),
    (S, "time cargo test"),
    (S, "nohup cargo build"),
    (S, "exec git status"),
    (S, "command git status"),
    (S, "builtin echo x"),
    (S, "env git status"),
    (S, "xargs git add"),
    (S, "watch git status"),
    (S, "if"),
    (S, "export PATH=/x"),
    (S, "source .venv/bin/activate"),
    (S, ". ./env.sh"),
    (S, "cd src"),
    (S, "alias g=git"),
    (S, "GIT status"),
    (S, "git.exe status"),
    (S, "gi? status"),
    (S, "g* status"),
    (S, "\\git status"),
    (S, "'git' status"),
    (S, "\"git\" status"),
    (S, "g\"it\" status"),
    (S, "$GIT status"),
    (S, "${GIT} status"),
    (S, "~/bin/git status"),
    (S, "./node_modules/.bin/jest"),
    (S, "/usr/bin/git status"),
    (S, "bin/rails test"),
    (S, "gіt status"), // Cyrillic і
    // Environment assignments in front.
    (S, "RUST_LOG=debug cargo test"),
    (S, "CI=1 npm test"),
    (S, "A=1 B=2 make"),
    (S, "=x git status"),
    // Pipes, lists and background jobs.
    (S, "git log | head"),
    (S, "git log|head"),
    (S, "cargo test 2>&1 | tail -20"),
    (S, "git status && git diff"),
    (S, "git status || true"),
    (S, "git status; git diff"),
    (S, "git status ;"),
    (S, "cargo build &"),
    (S, "cargo build & wait"),
    (S, "git status |& cat"),
    (S, "npm test || exit 1"),
    // Redirections and here-documents.
    (S, "git log > log.txt"),
    (S, "git log >> log.txt"),
    (S, "git diff 2> /dev/null"),
    (S, "git diff &> out"),
    (S, "git apply < fix.patch"),
    (S, "cat <<EOF\nx\nEOF"),
    (S, "git commit -F- <<'EOF'\nmsg\nEOF"),
    (S, "grep x <<< \"$text\""),
    (S, "git diff >| out"),
    (S, "git log 1>&2"),
    // Command and process substitution, subshells and arithmetic.
    (S, "git log $(git merge-base HEAD main)..HEAD"),
    (S, "git checkout `git rev-parse HEAD`"),
    (S, "ls $((1 + 2))"),
    (S, "diff <(git show HEAD:a) a"),
    (S, "tee >(wc -l)"),
    (S, "(cd src && make)"),
    (S, "( git status )"),
    (S, "{ git status; }"),
    (S, "python -c \"print(1)\""),
    (S, "git log --format=\"$(echo %H)\""),
    (S, "ls \"$(pwd)\""),
    // Multi-line commands and continuations.
    (S, "git status\ngit diff"),
    (S, "cargo test \\\n  --workspace"),
    (S, "git status\n"),
    (S, "\ngit status"),
    (S, "git status\r\ngit diff"),
    (S, "git log \\"),
    (S, "git log --oneline\\"),
    // Quoting: balanced quotes are passed through untouched.
    (R, "git commit -m 'fix the bug'"),
    (R, "git commit -m \"fix the bug\""),
    (R, "git commit -m \"it's fixed\""),
    (R, "git commit -m 'say \"hi\"'"),
    (R, "git log --format='%h %s'"),
    (R, "git log --format=\"%h %an\""),
    (R, "rg 'fn main' src"),
    (R, "grep -rn \"TODO:\" src"),
    (R, "git log --grep='fix: '"),
    (R, "git commit -m \"a \\\"quoted\\\" word\""),
    (R, "find . -name '*.rs'"),
    (R, "git log --author=\"A B\" --since=\"2 weeks ago\""),
    // Quoting that hides an operator is still skipped (the check is textual).
    (S, "git commit -m 'a; b'"),
    (S, "git commit -m \"a && b\""),
    (S, "rg 'a|b' src"),
    (S, "rg -e 'a|b'"),
    (S, "grep '<div>' index.html"),
    (S, "git log --grep='(fix)'"),
    // Unbalanced quotes and escapes.
    (S, "git commit -m \"unterminated"),
    (S, "git commit -m 'unterminated"),
    (S, "git log --format='%h"),
    (S, "rg \"a\\\""),
    (S, "git commit -m 'it\\'s'"),
    (S, "git status \\"),
    // Backslashes that the shell resolves the same way with or without the prefix.
    (R, "rg 'a\\.b'"),
    (R, "grep -E \"a\\\\b\" file"),
    (R, "ls my\\ dir"),
    (R, "git log --format=%h\\ %s"),
    (R, "find . -name \\*.rs"),
    // Globs, tilde, braces and variables: expanded by the shell before `lagom run` sees them.
    (R, "ls *.rs"),
    (R, "ls src/**/*.rs"),
    (R, "ls ~/projects"),
    (R, "ls ~"),
    (R, "git diff HEAD~1"),
    (R, "git log @{u}.."),
    (R, "ls {a,b}.txt"),
    (R, "ls $HOME"),
    (R, "ls \"$HOME/x\""),
    (R, "cargo test ${TEST_FILTER}"),
    (R, "git log -n $N"),
    (R, "ls [ab]*.rs"),
    (R, "git show HEAD^"),
    (R, "git log ^main HEAD"),
    // `--` and options.
    (R, "git log -- src/main.rs"),
    (R, "git diff -- ."),
    (R, "cargo test -- --nocapture"),
    (R, "npm test -- -t parser"),
    (R, "git checkout -- file"),
    (R, "rg -- -pattern"),
    // Comments.
    (R, "git status # see what changed"),
    (R, "git log --grep=#123"),
    (S, "# git status"),
    (S, "#git status"),
    (S, "git status # a; b"),
    // Leading and trailing whitespace.
    (R, "  git status"),
    (R, "git status  "),
    (R, "\tgit status"),
    (R, "git\tstatus"),
    (R, "git  status"),
    (S, ""),
    (S, "   "),
    (S, "\t"),
    // Unicode and invisible characters.
    (R, "git commit -m 'café'"),
    (R, "rg 'naïve'"),
    (R, "ls 文档"),
    (R, "git commit -m '✓ done'"),
    (S, "git\u{a0}status"),
    (S, "git status\u{a0}--short"),
    (S, "git\u{3000}status"),
    (S, "git\u{2028}status"),
    (S, "git status\u{200b}"),
    (S, "git\u{200b} status"),
    (S, "git log \u{202e}txt.exe"),
    (S, "git status\u{feff}"),
    (S, "git sta\u{ad}tus"),
    (S, "git status\u{b}"),
    (S, "git status\u{c}"),
    (S, "git status\r"),
    (S, "git\rstatus"),
    (S, "git status\u{0}"),
    (S, "git status\u{1b}[31m"),
    (S, "git status\u{85}"),
    // Long-running and interactive commands, whose output must stream.
    (S, "npm run dev"),
    (S, "npm start"),
    (S, "yarn dev"),
    (S, "pnpm serve"),
    (S, "cargo watch -x test"),
    (S, "jest --watch"),
    (S, "vitest watch"),
    (S, "tsc -w"),
    (S, "docker logs -f app"),
    (S, "kubectl logs --follow pod"),
    (S, "docker run -it ubuntu"),
    (S, "docker attach app"),
    (S, "python manage.py shell"),
    (S, "node repl"),
    (S, "npm test -- --watch"),
    (S, "npm run \"dev\""),
    (S, "npm run 'start'"),
    // A leading `cd <dir> &&`.
    (R, "cd src && cargo test"),
    (R, "cd src&&cargo test"),
    (R, "cd backend && pytest -q"),
    (R, "cd 'my dir' && git status"),
    (R, "cd ../other && git log --oneline"),
    (R, "cd /tmp/x && ls"),
    (R, "cd café && ls"),
    (R, "  cd src && git status  "),
    (S, "cd - && git status"),
    (S, "cd -- && git status"),
    (S, "cd -P src && git status"),
    (S, "cd ~ && git status"),
    (S, "cd ~/src && git status"),
    (S, "cd $HOME && git status"),
    (S, "cd \"$DIR\" && git status"),
    (S, "cd \"my dir\" && git status"),
    (S, "cd my dir && git status"),
    (S, "cd src* && git status"),
    (S, "cd sr? && git status"),
    (S, "cd {a,b} && git status"),
    (S, "cd `pwd` && git status"),
    (S, "cd $(git rev-parse --show-toplevel) && git status"),
    (S, "cd 'a&&b' && git status"),
    (S, "cd src && cd lib && git status"),
    (S, "cd src && git status && git diff"),
    (S, "cd src && git status | head"),
    (S, "cd src; git status"),
    (S, "cd src || exit && git status"),
    (S, "cd src && npm run dev"),
    (S, "cd src && echo hi"),
    (S, "cd src && RUST_LOG=1 cargo test"),
    (S, "cd src\u{a0}x && git status"),
    (S, "cd 'src\u{a0}x' && git status"),
    (S, "cd src && "),
    (S, "cd  && git status"),
    (S, "cd 'src && git status"),
];

/// The command a rewrite runs, with Lagom's prefix taken out and spacing ignored: a
/// rewrite may only add the prefix (and spaces around `&&`), never change the command.
fn squeeze(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn every_row_of_the_safety_table_holds() {
    assert!(
        TABLE.len() >= 200,
        "{} rows; the table needs at least 200",
        TABLE.len()
    );
    let mut wrong = Vec::new();
    for (expect, command) in TABLE {
        let got = rewrite(command, BIN, None, Wrapped::default());
        match (expect, &got) {
            (R, Some(out)) => {
                let prefix = format!("{BIN} run -- ");
                let unprefixed = out.replacen(&prefix, "", 1);
                if !out.contains(&prefix) || squeeze(&unprefixed) != squeeze(command) {
                    wrong.push(format!("{command:?}: rewritten wrongly to {out:?}"));
                }
                if split_cd(command).is_some() && !out.trim_start().starts_with("cd ") {
                    wrong.push(format!("{command:?}: the cd moved: {out:?}"));
                }
            }
            (S, None) => {}
            (R, None) => wrong.push(format!("{command:?}: expected a rewrite, was left alone")),
            (S, Some(out)) => wrong.push(format!(
                "{command:?}: expected to be left alone, became {out:?}"
            )),
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} rows wrong:\n{}",
        wrong.len(),
        TABLE.len(),
        wrong.join("\n")
    );
}
