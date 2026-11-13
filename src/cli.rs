//! Command-line interface shared by the `lagom` binary and its `lgm` alias.

use std::io::{IsTerminal, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use crate::hook::Agent;
use crate::{hook, install, run, store, style};

const HELP: &str = "lagom — fewer tokens, simpler answers for coding agents

Usage:
  lagom install [claude|codex|cursor ...]   Install hooks and skills (default: every agent found)
                [--dry-run] [--yes]         Show changes only / don't ask for confirmation
  lagom uninstall [claude|codex|cursor ...] Remove Lagom's hooks and skills [--dry-run]
                  [--all] [--purge] [--yes]  --purge also deletes ~/.lagom and ~/.config/lagom
  lagom status                              Show what is installed and which styles are active
  lagom style                               Show active styles
  lagom style list                          List built-in, user and project styles
  lagom style show <name>                   Print a style
  lagom style use [--project] <names…|off>  Set active styles (user or this project)
  lagom run -- <command> [args…]            Run a command and compress its output
            [--session AGENT:ID]            (set by the agent hook, for stats)
  lagom compress                            Compress text from stdin
  lagom recall <id> [--grep TEXT]           Print the full output behind a compressed result
                  [--lines A-B] [--head N] [--tail N]  or only part of it
  lagom recall --list | --clear             List or delete saved outputs
  lagom stats                               Show estimated savings
  lagom experiment start [--holdout 0.2]    Measure styles: some sessions run without them
                  [--days 14] | status | stop
  lagom stats --experiment                  Styled vs holdout output tokens, with a 95% interval
  lagom stats --usage [--days N]            Real token usage per style (Claude Code, Codex)
  lagom bench compress [DIR…] [--json F]    Compress a corpus of real outputs; report sizes and
                  [--compare BASELINE]      kept must_keep strings (exit 1 on an unexpected loss)
  lagom hook <agent> <event>                Used by agent hooks (reads JSON on stdin)
  lagom --version

`lgm` is a short alias: `lgm <command>` is the same as `lagom <command>`.
";

/// Entry point shared by the `lagom` and `lgm` binaries.
pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = |n: usize| args.get(n..).unwrap_or(&[]).to_vec();
    match args.first().map(String::as_str) {
        Some("run") => {
            let mut cmd = rest(1);
            let mut session = None;
            if cmd.first().map(String::as_str) == Some("--session") && cmd.len() >= 2 {
                session = Some(cmd[1].clone());
                cmd.drain(..2);
            }
            if cmd.first().map(String::as_str) == Some("--") {
                cmd.remove(0);
            }
            run::run(&cmd, session.as_deref())
        }
        Some("compress") => cmd_compress(),
        Some("recall") => cmd_recall(&rest(1)),
        Some("hook") => {
            hook::run(
                args.get(1).map(String::as_str).unwrap_or(""),
                args.get(2).map(String::as_str).unwrap_or(""),
            );
            ExitCode::SUCCESS
        }
        Some("install") => cmd_install(&rest(1), true),
        Some("uninstall") => cmd_install(&rest(1), false),
        Some("status") => cmd_status(),
        Some("style") => cmd_style(&rest(1)),
        Some("stats") => match args.get(1).map(String::as_str) {
            Some("--usage") => cmd_usage(&rest(2)),
            Some("--experiment") => cmd_experiment_report(),
            None => cmd_stats(),
            Some(other) => {
                eprintln!(
                    "lagom: unknown option `{other}`\nusage: lagom stats [--usage [--days N] | --experiment]"
                );
                ExitCode::from(2)
            }
        },
        Some("bench") => crate::bench::run(&rest(1)),
        Some("experiment") => cmd_experiment(&rest(1)),
        Some("--version" | "-V" | "version") => {
            println!("lagom {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h" | "help") | None => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("lagom: unknown command `{other}`\n\n{HELP}");
            ExitCode::from(2)
        }
    }
}

fn cwd() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| ".".into())
}

fn cmd_compress() -> ExitCode {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        eprintln!("lagom: stdin is not valid UTF-8 text");
        return ExitCode::from(1);
    }
    let out = run::render(&raw, 0, "compress", &store::Attribution::default());
    let _ = std::io::stdout().write_all(out.as_bytes());
    ExitCode::SUCCESS
}

fn cmd_recall(args: &[String]) -> ExitCode {
    const USAGE: &str =
        "usage: lagom recall <id> [--lines A-B] [--grep TEXT] [--head N] [--tail N]";
    let Some((id, rest)) = args.split_first() else {
        eprintln!("lagom: {USAGE}");
        return ExitCode::from(2);
    };
    match id.as_str() {
        "--list" => {
            let now = std::time::SystemTime::now();
            let all = store::list_recall();
            for e in &all {
                let age = now
                    .duration_since(e.modified)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let age = match age {
                    s if s < 3600 => format!("{}m", s / 60),
                    s if s < 86_400 => format!("{}h", s / 3600),
                    s => format!("{}d", s / 86_400),
                };
                println!(
                    "{}  {:>8}  {:>4} ago  {}",
                    e.id,
                    human_bytes(e.bytes),
                    age,
                    e.program
                );
            }
            let total: u64 = all.iter().map(|e| e.bytes).sum();
            println!(
                "{} saved outputs, {} (kept: last 500, 200 MB, 14 days)",
                all.len(),
                human_bytes(total)
            );
            return ExitCode::SUCCESS;
        }
        "--clear" => {
            println!("removed {} saved outputs", store::clear_recall());
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    let selection = match store::Selection::parse(rest) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lagom: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if !store::is_recall_id(id) {
        eprintln!(
            "lagom: `{id}` is not a recall id (10 hex characters, from a `[lagom: … lagom recall <id> …]` line)\n{USAGE}"
        );
        return ExitCode::from(2);
    }
    match store::load_original(id) {
        Some(text) => {
            print!("{}", selection.apply(&text));
            ExitCode::SUCCESS
        }
        None => {
            eprintln!(
                "lagom: no stored output for `{id}`: it may have been pruned (Lagom keeps up to 500 outputs, 200 MB, for 14 days; see `lagom recall --list`)"
            );
            ExitCode::from(1)
        }
    }
}

fn human_bytes(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{:.1} MB", n as f64 / (1024.0 * 1024.0)),
    }
}

fn parse_agents(names: &[String], only_present: bool) -> Result<Vec<Agent>, String> {
    if names.is_empty() || names.iter().any(|n| n == "all") {
        let all = [Agent::Claude, Agent::Codex, Agent::Cursor];
        return Ok(all
            .into_iter()
            .filter(|a| !only_present || install::target(*a).app_dir.is_dir())
            .collect());
    }
    names
        .iter()
        .map(|n| {
            Agent::parse(n)
                .ok_or_else(|| format!("unknown agent `{n}` (use claude, codex or cursor)"))
        })
        .collect()
}

/// Ask a yes/no question. Enter or "y" means yes; "n", end of input or a read error means no.
fn confirm(question: &str, input: &mut impl std::io::BufRead, out: &mut impl Write) -> bool {
    let _ = write!(out, "{question} [Y/n] ");
    let _ = out.flush();
    let mut answer = String::new();
    match input.read_line(&mut answer) {
        Ok(0) | Err(_) => false,
        Ok(_) => !answer.trim().to_lowercase().starts_with('n'),
    }
}

fn cmd_install(args: &[String], installing: bool) -> ExitCode {
    let (mut dry_run, mut yes, mut purge, mut all, mut names) =
        (false, false, false, false, Vec::new());
    for a in args {
        match a.as_str() {
            "--dry-run" | "-n" => dry_run = true,
            "--yes" | "-y" => yes = true,
            "--all" => all = true, // the default when no agent is named
            "--purge" if !installing => purge = true,
            s if s.starts_with('-') => {
                eprintln!("lagom: unknown option `{s}`");
                return ExitCode::from(2);
            }
            _ => names.push(a.clone()),
        }
    }
    if all && !names.is_empty() {
        eprintln!("lagom: use either --all or agent names, not both");
        return ExitCode::from(2);
    }
    if purge && !names.is_empty() {
        eprintln!(
            "lagom: --purge deletes data every agent shares; run `lagom uninstall --all --purge`"
        );
        return ExitCode::from(2);
    }
    // Decide before changing anything: without a terminal there is no one to ask.
    if purge && !dry_run && !yes && !std::io::stdin().is_terminal() {
        eprintln!("lagom: --purge deletes Lagom's data and config; rerun with --yes to confirm");
        return ExitCode::from(2);
    }
    let agents = match parse_agents(&names, installing) {
        Ok(a) if a.is_empty() => {
            eprintln!(
                "lagom: no Claude Code, Codex or Cursor config found; name an agent, e.g. `lagom install claude`"
            );
            return ExitCode::from(1);
        }
        Ok(a) => a,
        Err(e) => {
            eprintln!("lagom: {e}");
            return ExitCode::from(2);
        }
    };
    if installing && cfg!(windows) {
        eprintln!(
            "lagom: `lagom install` doesn't support Windows yet: the hooks it writes need a POSIX shell. `lagom uninstall` still works."
        );
        return ExitCode::from(2);
    }
    if installing && !dry_run && !yes && std::io::stdin().is_terminal() {
        let list: Vec<&str> = agents.iter().map(|a| a.name()).collect();
        let question = format!("Install Lagom hooks and skills into {}?", list.join(", "));
        if !confirm(
            &question,
            &mut std::io::stdin().lock(),
            &mut std::io::stdout(),
        ) {
            println!("\nCancelled. Nothing was changed.");
            return ExitCode::from(1);
        }
    }

    let bin = hook::lagom_bin();
    let mut summary: Vec<(Agent, String)> = Vec::new();
    for agent in agents.iter().copied() {
        let result = if installing {
            install::install(agent, &bin, dry_run)
        } else {
            install::uninstall(agent, dry_run)
        };
        let state = match result {
            Ok(out) => {
                println!("{}:", agent.name());
                for n in &out.notes {
                    println!("  {n}");
                }
                match (installing, dry_run, out.changed) {
                    (_, _, false) if installing => "already up to date",
                    (_, _, false) => "nothing to remove",
                    (true, true, true) => "would install",
                    (true, false, true) => "installed",
                    (false, true, true) => "would remove",
                    (false, false, true) => "removed",
                }
                .to_string()
            }
            Err(e) => format!("failed: {e}"),
        };
        summary.push((agent, state));
    }
    if names.is_empty() && installing {
        for agent in [Agent::Claude, Agent::Codex, Agent::Cursor] {
            if !agents.contains(&agent) {
                summary.push((agent, "skipped (not found)".into()));
            }
        }
    }

    println!("\nSummary:");
    for (agent, state) in &summary {
        println!("  {:<7} {state}", agent.name());
    }
    let mut failed = summary.iter().any(|(_, s)| s.starts_with("failed"));
    if purge && !failed {
        failed = !cmd_purge(dry_run, yes);
    }
    if !installing && names.is_empty() && !failed {
        if let Ok(exe) = std::env::current_exe() {
            println!(
                "To remove the program itself: {}",
                install::removal_hint(&exe)
            );
        }
    }
    if dry_run {
        println!("Dry run: nothing was changed.");
    } else if installing && !failed {
        println!("Restart the agent (or start a new session) to load Lagom.");
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Delete Lagom's data and config. Asks first in a terminal; needs `--yes` otherwise.
/// Returns false on failure or when the user declines.
fn cmd_purge(dry_run: bool, yes: bool) -> bool {
    let dirs: Vec<_> = install::purge_targets()
        .into_iter()
        .filter(|d| d.exists())
        .collect();
    if dirs.is_empty() {
        println!("purge: no Lagom data or config found");
        return true;
    }
    if !dry_run && !yes {
        let list: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
        print!(
            "Delete {} (stats, saved outputs, styles, config)? [y/N] ",
            list.join(" and ")
        );
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().read_line(&mut answer);
        if !answer.trim().to_lowercase().starts_with('y') {
            println!("purge: skipped");
            return true;
        }
    }
    match install::purge(dry_run) {
        Ok(notes) => {
            for n in notes {
                println!("purge: {n}");
            }
            true
        }
        Err(e) => {
            eprintln!("lagom: {e}");
            false
        }
    }
}

fn cmd_status() -> ExitCode {
    for agent in [Agent::Claude, Agent::Codex, Agent::Cursor] {
        let (hooks, skills) = install::status(agent);
        println!(
            "{:<7} hooks: {:<3}  skills: {}/{}",
            agent.name(),
            if hooks { "yes" } else { "no" },
            skills,
            install::SKILLS.len()
        );
    }
    print_active_styles(&cwd());
    ExitCode::SUCCESS
}

fn print_active_styles(cwd: &Path) {
    let names = style::active(cwd);
    if names.is_empty() {
        println!("styles: off");
        return;
    }
    for name in names {
        match style::load(&name, cwd) {
            Some(s) => println!(
                "style {name}: {}",
                s.path
                    .map(|p| p.display().to_string())
                    .unwrap_or(s.source.into())
            ),
            None => println!("style {name}: not found"),
        }
    }
}

fn cmd_style(args: &[String]) -> ExitCode {
    let cwd = cwd();
    match args.first().map(String::as_str) {
        None => print_active_styles(&cwd),
        Some("list") => {
            for s in style::list(&cwd) {
                println!("{:<14} {:<8} {}", s.name, s.source, s.description());
            }
        }
        Some("show") => match args.get(1).and_then(|n| style::load(n, &cwd)) {
            Some(s) => println!("{}", s.body()),
            None => {
                eprintln!("lagom: no such style (see `lagom style list`)");
                return ExitCode::from(1);
            }
        },
        Some("use") => {
            let project = args.iter().any(|a| a == "--project");
            let names: Vec<String> = style::parse_list(
                &args[1..]
                    .iter()
                    .filter(|a| *a != "--project")
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            let wants_off = args[1..].iter().any(|a| a == "off" || a == "none");
            if names.is_empty() && !wants_off {
                eprintln!("usage: lagom style use [--project] <names…|off>");
                return ExitCode::from(2);
            }
            if let Some(bad) = names.iter().find(|n| style::load(n, &cwd).is_none()) {
                eprintln!("lagom: no such style `{bad}` (see `lagom style list`)");
                return ExitCode::from(1);
            }
            match style::save_active(&names, project.then_some(cwd.as_path())) {
                Ok(path) => {
                    let shown = if names.is_empty() {
                        "off".to_string()
                    } else {
                        names.join(", ")
                    };
                    println!(
                        "styles: {shown} (saved to {}; applies to new sessions)",
                        path.display()
                    );
                }
                Err(e) => {
                    eprintln!("lagom: cannot save styles: {e}");
                    return ExitCode::from(1);
                }
            }
        }
        Some(other) => {
            eprintln!("lagom: unknown style command `{other}`");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

/// `lagom stats --usage [--days N]`: real token usage from Claude Code transcripts,
/// grouped by the styles Lagom injected.
fn cmd_usage(args: &[String]) -> ExitCode {
    let days = match args {
        [] => 30,
        [flag, n] if flag == "--days" => match n.parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("lagom: --days needs a number");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("usage: lagom stats --usage [--days N]");
            return ExitCode::from(2);
        }
    };
    let since =
        std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(days * 86_400));
    let (styles, tracked_since) = store::session_styles();
    let agents = [
        ("Claude Code", crate::gain::claude::sessions(since)),
        ("Codex", crate::gain::codex::sessions(since)),
    ];
    if agents.iter().all(|(_, s)| s.is_empty()) {
        println!("No Claude Code or Codex sessions in the last {days} days.");
        return ExitCode::SUCCESS;
    }
    for (name, sessions) in agents.iter().filter(|(_, s)| !s.is_empty()) {
        println!("{name} token usage, last {days} days (from its own logs):");
        println!(
            "{:<30} {:>8} {:>7} {:>14} {:>15} {:>11}",
            "style", "sessions", "turns", "output/turn", "input/turn", "cache read"
        );
        for g in &crate::gain::by_style(sessions, &styles, tracked_since) {
            println!(
                "{:<30} {:>8} {:>7} {:>14} {:>15} {:>10.0}%",
                g.style,
                g.sessions,
                g.turns,
                g.median_output_per_turn,
                g.median_input_per_turn,
                g.cache_read_share * 100.0
            );
        }
        println!();
    }
    println!(
        "Medians per main-conversation turn. Observational: sessions differ in their work, so this does not prove a style's effect."
    );
    ExitCode::SUCCESS
}

/// `lagom experiment start --holdout 0.2 [--days 14] | status | stop`.
fn cmd_experiment(args: &[String]) -> ExitCode {
    use crate::gain::experiment::Experiment;
    const USAGE: &str = "usage: lagom experiment start [--holdout 0.2] [--days 14] | status | stop";
    match args.first().map(String::as_str) {
        Some("start") => {
            let (mut holdout, mut days) = (0.2_f64, 14_u64);
            let mut it = args[1..].iter();
            while let Some(flag) = it.next() {
                let value = it.next();
                match (flag.as_str(), value) {
                    ("--holdout", Some(v)) => match v.parse::<f64>() {
                        Ok(h) if (0.05..=0.5).contains(&h) => holdout = h,
                        _ => {
                            eprintln!("lagom: --holdout must be between 0.05 and 0.5");
                            return ExitCode::from(2);
                        }
                    },
                    ("--days", Some(v)) => match v.parse::<u64>() {
                        Ok(d) if (1..=90).contains(&d) => days = d,
                        _ => {
                            eprintln!("lagom: --days must be between 1 and 90");
                            return ExitCode::from(2);
                        }
                    },
                    _ => {
                        eprintln!("lagom: {USAGE}");
                        return ExitCode::from(2);
                    }
                }
            }
            match Experiment::start(holdout, days) {
                Ok(_) => {
                    println!(
                        "Experiment started: about {:.0}% of new sessions run without styles for {days} days.",
                        holdout * 100.0
                    );
                    println!("See results any time with `lagom stats --experiment`.");
                    println!(
                        "Other tools that add answer rules at session start (Caveman, for example) run in both groups and blur the result: turn them off while it runs."
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("lagom: cannot save the experiment: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Some("status") => {
            match Experiment::load() {
                Some(e) if Experiment::active().is_some() => println!(
                    "Running: {:.0}% holdout, {} days left.",
                    e.holdout * 100.0,
                    e.ends.saturating_sub(store_now()) / 86_400
                ),
                Some(_) => println!("Finished. `lagom stats --experiment` shows the results."),
                None => println!("No experiment. Start one with `lagom experiment start`."),
            }
            ExitCode::SUCCESS
        }
        Some("stop") => match Experiment::stop() {
            Ok(Some(_)) => {
                println!(
                    "Experiment stopped; every session gets styles again. `lagom stats --experiment` still shows the results."
                );
                ExitCode::SUCCESS
            }
            Ok(None) => {
                println!("No experiment is running.");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("lagom: {e}");
                ExitCode::from(1)
            }
        },
        _ => {
            eprintln!("lagom: {USAGE}");
            ExitCode::from(2)
        }
    }
}

fn store_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `lagom stats --experiment`: styled vs holdout output tokens per turn.
fn cmd_experiment_report() -> ExitCode {
    use crate::gain::experiment::{Experiment, compare, per_arm, recorded_arms};
    let Some(e) = Experiment::load() else {
        println!("No experiment. Start one with `lagom experiment start --holdout 0.2`.");
        return ExitCode::SUCCESS;
    };
    let since = std::time::UNIX_EPOCH + std::time::Duration::from_secs(e.started);
    let mut sessions = crate::gain::claude::sessions(Some(since));
    sessions.extend(crate::gain::codex::sessions(Some(since)));
    let (styled, holdout) = per_arm(&sessions, &recorded_arms(e.started));
    println!(
        "Holdout experiment ({:.0}% without styles): {} styled sessions, {} holdout sessions.",
        e.holdout * 100.0,
        styled.len(),
        holdout.len()
    );
    match compare(&styled, &holdout, 10_000, e.started) {
        Some(c) => {
            println!(
                "Output tokens per turn (mean per session): styled {:.0}, holdout {:.0}.",
                c.styled_mean, c.holdout_mean
            );
            let pct = |x: f64| x * 100.0 / c.holdout_mean.max(1.0);
            println!(
                "Difference: {:+.0} tokens per turn ({:+.0}%), 95% interval {:+.0} to {:+.0} ({:+.0}% to {:+.0}%).",
                c.diff,
                pct(c.diff),
                c.ci_low,
                c.ci_high,
                pct(c.ci_low),
                pct(c.ci_high)
            );
            if c.ci_low <= 0.0 && c.ci_high >= 0.0 {
                println!("The interval includes zero: no measurable effect yet.");
            }
            if c.holdout_sessions < 20 {
                println!("Few holdout sessions so far; the interval narrows as more come in.");
            }
        }
        None => println!("Not enough sessions in both arms yet (at least 2 each)."),
    }
    ExitCode::SUCCESS
}

fn cmd_stats() -> ExitCode {
    let t = store::totals();
    let saved = t.bytes_in.saturating_sub(t.bytes_out);
    let tokens = |bytes: u64| bytes / 4;
    println!(
        "commands run through lagom: {} ({} compressed)",
        t.runs, t.compressed_runs
    );
    println!(
        "output: {} → {} bytes, saved ~{} tokens",
        t.bytes_in,
        t.bytes_out,
        tokens(saved)
    );
    println!(
        "style rules injected: {} times, ~{} tokens",
        t.injections,
        tokens(t.injected_bytes)
    );
    let net = tokens(saved) as i64 - tokens(t.injected_bytes) as i64;
    println!(
        "net: ~{net} tokens (estimate: 4 bytes ≈ 1 token; style effects on answer length are not measured)"
    );
    if !t.by_agent.is_empty() {
        println!("by agent:");
        for (agent, b) in &t.by_agent {
            println!(
                "  {agent:<8} {} runs, saved ~{} tokens; {} injections, ~{} tokens",
                b.runs,
                tokens(b.bytes_in.saturating_sub(b.bytes_out)),
                b.injections,
                tokens(b.injected_bytes)
            );
        }
    }
    if !t.by_style.is_empty() {
        println!("by style:");
        for (style, b) in &t.by_style {
            println!(
                "  {style:<20} {} injections, ~{} tokens",
                b.injections,
                tokens(b.injected_bytes)
            );
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(input: &str) -> bool {
        let mut out = Vec::new();
        let yes = confirm("Install?", &mut input.as_bytes(), &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "Install? [Y/n] ");
        yes
    }

    #[test]
    fn confirm_defaults_to_yes_and_cancels_on_no_or_end_of_input() {
        assert!(answer("\n"));
        assert!(answer("y\n"));
        assert!(answer("Yes\n"));
        assert!(!answer("n\n"));
        assert!(!answer("No\n"));
        assert!(!answer(""), "Ctrl-D at the prompt must not install");
    }
}
