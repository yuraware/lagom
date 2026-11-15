//! Real token usage from agents' own session logs (for `lagom stats`).

pub mod claude;

/// Token counts as the provider reported them.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    /// Assistant messages counted (after removing streaming duplicates).
    pub messages: u64,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_write += other.cache_write;
        self.cache_read += other.cache_read;
        self.messages += other.messages;
    }
}

/// One agent session's usage.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SessionUsage {
    pub session_id: String,
    /// Model of the last counted message.
    pub model: Option<String>,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
    /// The main conversation.
    pub main: Usage,
    /// Subagent (sidechain) messages.
    pub subagents: Usage,
}

impl SessionUsage {
    pub fn total(&self) -> Usage {
        let mut t = self.main;
        t.add(&self.subagents);
        t
    }
}
