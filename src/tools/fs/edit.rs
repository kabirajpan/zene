use std::fs;
use std::path::Path;
use serde_json::{json, Value};
use crate::traits::tool::{Tool, ToolRisk};
use crate::types::error::ToolError;

/// Tool for applying targeted patches/edits to files safely.
pub struct EditFileTool;

impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Apply a targeted patch to a file by replacing unique target text with replacement text. Safer and cheaper than rewriting the whole file."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "target": {
                    "type": "string",
                    "description": "Unique text snippet in the file to replace (include surrounding context if needed to ensure uniqueness)"
                },
                "replacement": {
                    "type": "string",
                    "description": "New text to substitute in place of target"
                }
            },
            "required": ["path", "target", "replacement"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::Mutating
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let path_str = args.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'path'".to_string()))?;

        let target = args.get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'target'".to_string()))?;

        let replacement = args.get("replacement")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'replacement'".to_string()))?;

        let path = Path::new(path_str);
        if !path.exists() {
            return Err(ToolError::ExecutionFailed(format!("File not found: {}", path_str)));
        }

        let content = fs::read_to_string(path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read '{}': {}", path_str, e)))?;

        if !content.contains(target) {
            return Err(ToolError::ExecutionFailed(format!(
                "Target snippet not found in '{}'. Ensure whitespace and lines match exactly.",
                path_str
            )));
        }

        let occurrences = content.matches(target).count();
        if occurrences > 1 {
            return Err(ToolError::ExecutionFailed(format!(
                "Target snippet matched {} times in '{}'. Provide additional surrounding context to make it unique.",
                occurrences, path_str
            )));
        }

        let patched = content.replacen(target, replacement, 1);
        fs::write(path, patched)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write patched file '{}': {}", path_str, e)))?;

        Ok(Value::String(format!("Successfully patched '{}'", path_str)))
    }
}
