use std::fs;
use std::path::Path;
use serde_json::{json, Value};
use crate::traits::tool::{Tool, ToolRisk};
use crate::types::error::ToolError;

/// Tool for creating or overwriting a file.
pub struct WriteFileTool;

impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Create a new file or completely overwrite an existing file with content. Automatically creates parent directories."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to create or overwrite"
                },
                "content": {
                    "type": "string",
                    "description": "Full file content"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::Mutating
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let path_str = args.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'path'".to_string()))?;

        let content = args.get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'content'".to_string()))?;

        let path = Path::new(path_str);

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)
                    .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create directories for '{}': {}", path_str, e)))?;
            }
        }

        fs::write(path, content)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write to '{}': {}", path_str, e)))?;

        Ok(Value::String(format!("Successfully wrote {} bytes to '{}'", content.len(), path_str)))
    }
}
