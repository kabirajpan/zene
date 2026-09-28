use std::fs;
use std::path::Path;
use serde_json::{json, Value};
use crate::traits::tool::{Tool, ToolRisk};
use crate::types::error::ToolError;

/// Tool to delete a file.
pub struct DeleteFileTool;

impl Tool for DeleteFileTool {
    fn name(&self) -> &str {
        "delete_file"
    }

    fn description(&self) -> &str {
        "Delete a file from the filesystem."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to remove"
                }
            },
            "required": ["path"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::Destructive
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let path_str = args.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'path'".to_string()))?;

        let trimmed = path_str.trim();
        if trimmed == "." || trimmed == "./" || trimmed == ".." || trimmed.is_empty() || trimmed == "/" {
            return Err(ToolError::ExecutionFailed("Refusing to delete project root or parent directory".to_string()));
        }

        let path = Path::new(trimmed);
        if !path.exists() {
            return Err(ToolError::ExecutionFailed(format!("File does not exist: {}", path_str)));
        }

        if path.is_dir() {
            fs::remove_dir_all(path)
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to delete directory '{}': {}", path_str, e)))?;
            Ok(Value::String(format!("Successfully deleted directory '{}'", path_str)))
        } else {
            fs::remove_file(path)
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to delete file '{}': {}", path_str, e)))?;
            Ok(Value::String(format!("Successfully deleted file '{}'", path_str)))
        }
    }
}

/// Tool to rename or move a file.
pub struct RenameFileTool;

impl Tool for RenameFileTool {
    fn name(&self) -> &str {
        "rename_file"
    }

    fn description(&self) -> &str {
        "Rename or move a file from old_path to new_path."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "old_path": {
                    "type": "string",
                    "description": "Current file path"
                },
                "new_path": {
                    "type": "string",
                    "description": "Destination file path"
                }
            },
            "required": ["old_path", "new_path"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::Mutating
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let old_path = args.get("old_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'old_path'".to_string()))?;

        let new_path = args.get("new_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'new_path'".to_string()))?;

        let old = Path::new(old_path);
        if !old.exists() {
            return Err(ToolError::ExecutionFailed(format!("Source path '{}' does not exist", old_path)));
        }

        let new = Path::new(new_path);
        if let Some(parent) = new.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)
                    .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create destination directories for '{}': {}", new_path, e)))?;
            }
        }

        fs::rename(old, new)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to rename '{}' to '{}': {}", old_path, new_path, e)))?;

        Ok(Value::String(format!("Successfully renamed '{}' to '{}'", old_path, new_path)))
    }
}
