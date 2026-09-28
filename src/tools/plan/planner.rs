use std::sync::{Arc, Mutex};
use serde_json::{json, Value};
use crate::traits::tool::{Tool, ToolRisk};
use crate::types::error::ToolError;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PlanStep {
    pub id: usize,
    pub description: String,
    pub status: String,
}

#[derive(Clone)]
pub struct PlanState {
    pub output_path: String,
    pub steps: Vec<PlanStep>,
}

impl Default for PlanState {
    fn default() -> Self {
        Self {
            output_path: "implementation_plan.md".to_string(),
            steps: Vec::new(),
        }
    }
}

/// Tool for externalizing a structured multi-step plan before execution.
pub struct CreatePlanTool {
    state: Arc<Mutex<PlanState>>,
}

impl CreatePlanTool {
    pub fn new(state: Arc<Mutex<PlanState>>) -> Self {
        Self { state }
    }
}

impl Tool for CreatePlanTool {
    fn name(&self) -> &str {
        "create_plan"
    }

    fn description(&self) -> &str {
        "Create an explicit multi-step implementation plan before performing modifications or actions. Call this first whenever a task requires code changes or execution."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Short title describing the implementation plan (e.g. 'Refactor Database Queries')"
                },
                "overview": {
                    "type": "string",
                    "description": "Executive summary and architectural intent of the changes"
                },
                "files": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Relative file path" },
                            "action": { "type": "string", "description": "Action: 'Modify', 'Create', or 'Delete'" },
                            "purpose": { "type": "string", "description": "Why this file is changing" }
                        },
                        "required": ["path", "action", "purpose"]
                    },
                    "description": "List of files that will be created, modified, or deleted"
                },
                "diffs": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "file": { "type": "string", "description": "File path" },
                            "before": { "type": "string", "description": "Code snippet before modification" },
                            "after": { "type": "string", "description": "Code snippet after modification" }
                        },
                        "required": ["file", "before", "after"]
                    },
                    "description": "Before and after code comparison snippets"
                },
                "steps": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Sequential action steps required to complete the task"
                },
                "tests": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "target": { "type": "string", "description": "Target component or test suite" },
                            "command": { "type": "string", "description": "Verification command line (e.g. 'cargo test')" },
                            "expected": { "type": "string", "description": "Expected outcome" }
                        },
                        "required": ["command"]
                    },
                    "description": "Verification and testing commands to run after implementation"
                },
                "output_path": {
                    "type": "string",
                    "description": "File path where markdown plan should be saved (default: 'implementation_plan.md')"
                }
            },
            "required": ["steps"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let steps_arr = args.get("steps")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'steps' (must be an array of strings)".to_string()))?;

        let title = args.get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Implementation Plan");

        let overview = args.get("overview").and_then(|v| v.as_str());
        let files = args.get("files").and_then(|v| v.as_array());
        let diffs = args.get("diffs").and_then(|v| v.as_array());
        let tests = args.get("tests").and_then(|v| v.as_array());
        let output_path = args.get("output_path")
            .and_then(|v| v.as_str())
            .unwrap_or("implementation_plan.md");

        let mut state = self.state.lock().map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        state.steps.clear();
        state.output_path = output_path.to_string();

        // 1. Title & Header
        let mut md = format!("# Implementation Plan: {}\n\n", title);

        // 2. Overview & Architectural Intent
        md.push_str("## 1. Overview & Architectural Intent\n");
        if let Some(ov) = overview {
            md.push_str(ov);
            md.push_str("\n\n");
        } else {
            md.push_str("Detailed implementation plan for requested changes.\n\n");
        }
        md.push_str("---\n\n");

        // 3. Files to Change
        md.push_str("## 2. Files to Change\n\n");
        if let Some(files_list) = files {
            if !files_list.is_empty() {
                md.push_str("| File Path | Action | Purpose |\n");
                md.push_str("| :--- | :---: | :--- |\n");
                for f in files_list {
                    if let Some(obj) = f.as_object() {
                        let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        let action = obj.get("action").and_then(|v| v.as_str()).unwrap_or("Modify");
                        let purpose = obj.get("purpose").and_then(|v| v.as_str()).unwrap_or("");
                        md.push_str(&format!("| `{}` | **{}** | {} |\n", path, action, purpose));
                    } else if let Some(s) = f.as_str() {
                        md.push_str(&format!("| `{}` | **Modify** | Targeted update |\n", s));
                    }
                }
                md.push_str("\n");
            } else {
                md.push_str("*Files will be identified dynamically during execution phases.*\n\n");
            }
        } else {
            md.push_str("*Files will be identified dynamically during execution phases.*\n\n");
        }
        md.push_str("---\n\n");

        // 4. Before & After Code Changes
        md.push_str("## 3. Before & After Code Changes\n\n");
        if let Some(diff_list) = diffs {
            if !diff_list.is_empty() {
                for d in diff_list {
                    if let Some(obj) = d.as_object() {
                        let file = obj.get("file").and_then(|v| v.as_str()).unwrap_or("target_file");
                        let before = obj.get("before").and_then(|v| v.as_str()).unwrap_or("// previous code");
                        let after = obj.get("after").and_then(|v| v.as_str()).unwrap_or("// modified code");

                        let ext = std::path::Path::new(file)
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("text");

                        md.push_str(&format!("### `{}`\n\n", file));
                        md.push_str("#### Before\n");
                        md.push_str(&format!("```{}\n{}\n```\n\n", ext, before));
                        md.push_str("#### After\n");
                        md.push_str(&format!("```{}\n{}\n```\n\n", ext, after));
                    }
                }
            } else {
                md.push_str("*Code changes will follow the sequential phases below.*\n\n");
            }
        } else {
            md.push_str("*Code changes will follow the sequential phases below.*\n\n");
        }
        md.push_str("---\n\n");

        // 5. Sequential Execution Phases
        md.push_str("## 4. Sequential Execution Phases\n\n");
        for (i, step_val) in steps_arr.iter().enumerate() {
            let desc = step_val.as_str().unwrap_or("").to_string();
            state.steps.push(PlanStep {
                id: i + 1,
                description: desc.clone(),
                status: "pending".to_string(),
            });
            md.push_str(&format!("- [ ] Step {}: {}\n", i + 1, desc));
        }
        md.push_str("\n---\n\n");

        // 6. Testing & Verification Suite
        md.push_str("## 5. Testing & Verification Suite\n\n");
        if let Some(test_list) = tests {
            if !test_list.is_empty() {
                md.push_str("| Test Target | Command | Expected Result |\n");
                md.push_str("| :--- | :--- | :--- |\n");
                for t in test_list {
                    if let Some(obj) = t.as_object() {
                        let target = obj.get("target").and_then(|v| v.as_str()).unwrap_or("Verification");
                        let command = obj.get("command").and_then(|v| v.as_str()).unwrap_or("cargo check");
                        let expected = obj.get("expected").and_then(|v| v.as_str()).unwrap_or("Passes cleanly");
                        md.push_str(&format!("| **{}** | `{}` | {} |\n", target, command, expected));
                    } else if let Some(cmd) = t.as_str() {
                        md.push_str(&format!("| **Test** | `{}` | Passes cleanly |\n", cmd));
                    }
                }
                md.push_str("\n");
            } else {
                md.push_str("| Test Target | Command | Expected Result |\n");
                md.push_str("| :--- | :--- | :--- |\n");
                md.push_str("| **Build Verification** | `cargo check` | Zero compiler errors or warnings |\n\n");
            }
        } else {
            md.push_str("| Test Target | Command | Expected Result |\n");
            md.push_str("| :--- | :--- | :--- |\n");
            md.push_str("| **Build Verification** | `cargo check` | Zero compiler errors or warnings |\n\n");
        }
        md.push_str("---\n\n");

        // 7. Approval Gate
        md.push_str("## 6. Approval Gate\n\n");
        md.push_str("> [!IMPORTANT]\n");
        md.push_str("> **Action Required Before Execution:**\n");
        md.push_str("> - Click the **`[ ▶ Proceed with Plan ]`** button in the AI plan card, or\n");
        md.push_str("> - Type **`proceed`** / **`go`** in chat to execute this plan.\n");
        md.push_str(">\n");
        md.push_str("> *No files will be modified until approval is given.*\n");

        // Persist implementation_plan.md to disk
        if let Err(e) = std::fs::write(output_path, &md) {
            eprintln!("[planner] Notice: could not write to {}: {}", output_path, e);
        }

        Ok(Value::String(md))
    }
}

/// Tool for updating the status of an ongoing plan step.
pub struct UpdatePlanStepTool {
    state: Arc<Mutex<PlanState>>,
}

impl UpdatePlanStepTool {
    pub fn new(state: Arc<Mutex<PlanState>>) -> Self {
        Self { state }
    }
}

impl Tool for UpdatePlanStepTool {
    fn name(&self) -> &str {
        "update_plan_step"
    }

    fn description(&self) -> &str {
        "Update the status of a specific step in the current plan ('pending', 'in_progress', 'completed', 'failed')."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "step_id": {
                    "type": "integer",
                    "description": "1-indexed step number"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "failed"],
                    "description": "Current status of this step"
                }
            },
            "required": ["step_id", "status"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let step_id = args.get("step_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'step_id'".to_string()))?;

        let status = args.get("status")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'status'".to_string()))?;

        let mut state = self.state.lock().map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if step_id == 0 || step_id > state.steps.len() {
            return Err(ToolError::ExecutionFailed(format!("Step {} does not exist (total steps: {})", step_id, state.steps.len())));
        }

        let step_desc = {
            let step = &mut state.steps[step_id - 1];
            step.status = status.to_string();
            step.description.clone()
        };

        let mark = match status {
            "completed" => "[x]",
            "in_progress" => "[>]",
            "failed" => "[!]",
            _ => "[ ]",
        };

        // Live-sync checkmarks to implementation_plan.md on disk
        let output_path = state.output_path.clone();
        if let Ok(content) = std::fs::read_to_string(&output_path) {
            let target_pending = format!("- [ ] Step {}:", step_id);
            let target_progress = format!("- [>] Step {}:", step_id);
            let target_completed = format!("- [x] Step {}:", step_id);
            let replacement = format!("- {} Step {}:", mark, step_id);

            let new_content = content
                .replace(&target_pending, &replacement)
                .replace(&target_progress, &replacement)
                .replace(&target_completed, &replacement);

            let _ = std::fs::write(&output_path, new_content);
        }

        let completed_count = state.steps.iter().filter(|s| s.status == "completed").count();
        let total_count = state.steps.len();
        let all_complete = completed_count == total_count;

        let summary = if all_complete {
            format!("{} Step {}: {} -> {} (ALL {}/{} STEPS COMPLETED! Run verification tests to finish task)", mark, step_id, step_desc, status, completed_count, total_count)
        } else {
            format!("{} Step {}: {} -> {} ({}/{} steps completed)", mark, step_id, step_desc, status, completed_count, total_count)
        };

        Ok(Value::String(summary))
    }
}
