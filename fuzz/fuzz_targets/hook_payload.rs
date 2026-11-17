//! Any bytes on a hook's stdin: parsing never panics, and neither does any agent's hook.
#![no_main]

use std::sync::OnceLock;

use lagom::hook::{self, Agent};
use libfuzzer_sys::fuzz_target;
use serde_json::json;

/// A throwaway HOME: the hooks read no real settings and write their stats here.
fn sandbox() -> &'static str {
    static CWD: OnceLock<String> = OnceLock::new();
    CWD.get_or_init(|| {
        let home = std::env::temp_dir().join(format!("lagom-fuzz-{}", std::process::id()));
        std::fs::create_dir_all(home.join("repo")).expect("create the sandbox");
        // SAFETY: set once, before any hook code runs, while libFuzzer runs one thread.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("LAGOM_HOME", home.join("lagom-home"));
            for var in [
                "CLAUDE_CONFIG_DIR",
                "CLAUDE_PROJECT_DIR",
                "CODEX_HOME",
                "XDG_CONFIG_HOME",
                "LAGOM_STYLES",
                "LAGOM_DISABLE",
                "LAGOM_REWRITE",
                "LAGOM_DEBUG",
            ] {
                std::env::remove_var(var);
            }
        }
        home.join("repo").display().to_string()
    })
}

fuzz_target!(|data: &[u8]| {
    let cwd = sandbox();
    let Ok(mut payload) = hook::read_payload(data) else {
        return;
    };
    // The folder fields send hooks to real folders on disk: keep them in the sandbox.
    payload["cwd"] = json!(cwd);
    if payload.get("workspace_roots").is_some() {
        payload["workspace_roots"] = json!([cwd]);
    }
    for (agent, events) in [
        (
            Agent::Claude,
            &["session-start", "subagent-start", "pre-tool", "other"][..],
        ),
        (Agent::Codex, &["session-start", "pre-tool"]),
        (Agent::Cursor, &["session-start", "pre-tool"]),
    ] {
        for event in events {
            let _ = hook::respond(agent, event, &payload, "/usr/local/bin/lagom");
        }
    }
});
