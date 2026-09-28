use serde_json::{json, Value};
use crate::types::error::ToolError;

/// Risk classification drives whether a tool can run automatically or requires user approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRisk {
    /// Safe to execute automatically (read_file, search_codebase, list_directory).
    ReadOnly,
    /// Mutates files, generates diff previews (edit_file, write_file).
    Mutating,
    /// High-impact or destructive actions (run_terminal, git_commit, delete_file).
    Destructive,
}

/// Unified Tool interface for agent capabilities.
pub trait Tool: Send + Sync {
    /// Name the LLM uses to invoke this tool.
    fn name(&self) -> &str;

    /// Clear, concise description of what the tool does.
    fn description(&self) -> &str;

    /// JSON Schema object describing the tool's parameters.
    fn schema(&self) -> Value;

    /// Risk level of executing this tool.
    fn risk(&self) -> ToolRisk;

    /// Execute the tool with given arguments.
    fn execute(&self, args: Value) -> Result<Value, ToolError>;

    /// Convert to OpenAI / Groq tool schema.
    fn to_openai_schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name(),
                "description": self.description(),
                "parameters": self.schema()
            }
        })
    }

    /// Convert to Google Gemini function declaration format.
    fn to_gemini_schema(&self) -> Value {
        json!({
            "name": self.name(),
            "description": self.description(),
            "parameters": self.schema()
        })
    }
}
