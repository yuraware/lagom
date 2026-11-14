//! Lagom: a lightweight token saver for coding agents.
//!
//! - [`style`]: user-definable answer/code styles injected at session start.
//! - [`compress`]: deterministic command-output compressor (library API).
//! - [`rewrite`] + [`hook`]: agent hooks that route simple shell commands through `lagom run`.
//! - [`install`]: hook and skill installation for Claude Code, Codex and Cursor.

pub mod bench;
pub mod cli;
pub mod compress;
pub mod gain;
pub mod hook;
pub mod install;
pub mod lang;
pub mod rewrite;
pub mod run;
pub mod store;
pub mod style;
