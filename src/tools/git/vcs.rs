use std::process::Command;
use serde_json::{json, Value};
use crate::traits::tool::{Tool, ToolRisk};
use crate::types::error::ToolError;

/// Tool for checking git repository status.
pub struct GitStatusTool;

impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }

    fn description(&self) -> &str {
        "View current working tree and index status (staged, unstaged, untracked files)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "cwd": {
                    "type": "string",
                    "description": "Optional repository path (defaults to current directory)"
                }
            }
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let mut cmd = Command::new("git");
        cmd.args(["status", "--short"]);

        if let Some(cwd) = args.get("cwd").and_then(|v| v.as_str()) {
            cmd.current_dir(cwd);
        }

        let output = cmd.output()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to execute 'git status': {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Err(ToolError::ExecutionFailed(format!("git status failed: {}", stderr.trim())));
        }

        if stdout.trim().is_empty() {
            Ok(Value::String("Working tree is clean (no changes).".to_string()))
        } else {
            Ok(Value::String(stdout.to_string()))
        }
    }
}

/// Tool for viewing uncommitted git diffs.
pub struct GitDiffTool;

impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn description(&self) -> &str {
        "Inspect uncommitted git diffs in working tree or staged changes."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "staged": {
                    "type": "boolean",
                    "description": "Whether to view staged diff (--cached)"
                },
                "path": {
                    "type": "string",
                    "description": "Optional specific file or path to diff"
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional repository working directory"
                }
            }
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let mut cmd = Command::new("git");
        cmd.arg("diff");

        let staged = args.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);
        if staged {
            cmd.arg("--cached");
        }

        if let Some(cwd) = args.get("cwd").and_then(|v| v.as_str()) {
            cmd.current_dir(cwd);
        }

        if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
            cmd.arg("--").arg(path);
        }

        let output = cmd.output()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to execute 'git diff': {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Err(ToolError::ExecutionFailed(format!("git diff failed: {}", stderr.trim())));
        }

        if stdout.trim().is_empty() {
            Ok(Value::String("No changes detected in diff.".to_string()))
        } else {
            Ok(Value::String(stdout.to_string()))
        }
    }
}
