use serde::{Deserialize, Serialize};
use crate::types::message::Message;

/// Serializable session state enabling session resume after IDE restarts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSession {
    pub session_id: String,
    pub conversation: Vec<Message>,
}

impl AgentSession {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            conversation: Vec::new(),
        }
    }

    /// Serialize session to pretty JSON string for disk storage.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Restore session from JSON string.
    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json_str)
    }
}
