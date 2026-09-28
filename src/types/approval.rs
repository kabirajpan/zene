//! Safety Approval Types
//!
//! NOTE (Development Mode):
//! Full control of tools is granted during the current development phase.
//! In development, mutating and destructive tools execute directly without
//! blocking on manual approval dialogs.
//! Strict user approval gating (preview diffs, confirmation prompts) will be enforced
//! in production mode.

use serde_json::Value;

/// Context provided to the UI/developer when a mutating tool call is prepared.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool_name: String,
    pub args: Value,
    pub preview: String,
}

/// User's choice regarding a pending approval request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Proceed with execution as-is.
    Approve,
    /// Cancel this tool invocation.
    Reject,
    /// Proceed with user-modified arguments (e.g. tweaked terminal command or diff).
    ApproveWithEdits(Value),
}
