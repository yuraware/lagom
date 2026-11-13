//! Lagom: a lightweight token saver for coding agents.
//!
//! - [`style`]: user-definable answer/code styles injected at session start.
//! - [`compress`]: deterministic command-output compressor (library API).
//! - [`rewrite`] + [`hook`]: agent hooks that route simple shell commands through `lagom run`.
//! - [`install`]: hook and skill installation for Claude Code, Codex and Cursor.
//! - [`json_splice`]: byte-exact edits of JSON string values, for the proxy.

pub mod cli;
pub mod compress;
pub mod hook;
pub mod install;
pub mod json_splice;
pub mod rewrite;
pub mod run;
pub mod store;
pub mod style;
