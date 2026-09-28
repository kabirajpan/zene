use std::path::Path;
use std::process::Command;
use serde_json::{json, Value};
use crate::traits::tool::{Tool, ToolRisk};
use crate::types::error::ToolError;

/// Tool for querying compiler diagnostics (errors and warnings) across the project or for a specific file.
pub struct GetDiagnosticsTool;

impl Tool for GetDiagnosticsTool {
    fn name(&self) -> &str {
        "get_diagnostics"
    }

    fn description(&self) -> &str {
        "Fetch current compiler errors and warnings for the project or a specific file to detect bugs and verify code fixes."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional file or crate path to filter diagnostics for"
                }
            }
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let filter_path = args.get("path").and_then(|v| v.as_str());

        // Run cargo check with JSON message format to extract precise diagnostics
        let output = Command::new("cargo")
            .args(["check", "--message-format=json", "--offline"])
            .output()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to run cargo check: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut diagnostics = Vec::new();

        for line in stdout.lines() {
            if let Ok(val) = serde_json::from_str::<Value>(line) {
                if val.get("reason").and_then(|r| r.as_str()) == Some("compiler-message") {
                    if let Some(msg) = val.get("message") {
                        let level = msg.get("level").and_then(|l| l.as_str()).unwrap_or("info");
                        if level == "error" || level == "warning" {
                            let rendered = msg.get("rendered").and_then(|r| r.as_str()).unwrap_or("");
                            
                            // If a filter path is provided, check if the diagnostic relates to it
                            let should_include = if let Some(target) = filter_path {
                                let norm_target = Path::new(target);
                                if let Some(spans) = msg.get("spans").and_then(|s| s.as_array()) {
                                    spans.iter().any(|span| {
                                        span.get("file_name")
                                            .and_then(|f| f.as_str())
                                            .map(|f| Path::new(f).ends_with(norm_target) || norm_target.ends_with(Path::new(f)))
                                            .unwrap_or(false)
                                    })
                                } else {
                                    rendered.contains(target)
                                }
                            } else {
                                true
                            };

                            if should_include && !rendered.is_empty() {
                                diagnostics.push(rendered.trim_end().to_string());
                            }
                        }
                    }
                }
            }
        }

        if diagnostics.is_empty() {
            let res = if let Some(p) = filter_path {
                format!("No diagnostics (0 errors, 0 warnings) found for '{}'.", p)
            } else {
                "No diagnostics (0 errors, 0 warnings) found in project.".to_string()
            };
            Ok(Value::String(res))
        } else {
            let count = diagnostics.len();
            let summary = format!("Found {} diagnostic message(s):\n\n{}", count, diagnostics.join("\n\n"));
            Ok(Value::String(summary))
        }
    }
}
