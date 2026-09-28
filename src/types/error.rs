use std::fmt;

/// Errors originating from provider calls.
#[derive(Debug, Clone)]
pub enum ProviderError {
    Network(String),
    RateLimited(Option<std::time::Duration>),
    Malformed(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(msg) => write!(f, "Network error: {}", msg),
            Self::RateLimited(d) => write!(f, "Rate limited (retry after {:?}):", d),
            Self::Malformed(msg) => write!(f, "Malformed provider response: {}", msg),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Errors occurring during tool execution or validation.
#[derive(Debug, Clone)]
pub enum ToolError {
    InvalidArgs(String),
    ExecutionFailed(String),
    Rejected,
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgs(msg) => write!(f, "Invalid arguments: {}", msg),
            Self::ExecutionFailed(msg) => write!(f, "Execution failed: {}", msg),
            Self::Rejected => write!(f, "Tool execution was rejected by user"),
        }
    }
}

impl std::error::Error for ToolError {}
