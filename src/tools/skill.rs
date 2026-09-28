use std::sync::Arc;
use serde_json::{json, Value};
use crate::skills::SkillRegistry;
use crate::traits::tool::{Tool, ToolRisk};
use crate::types::error::ToolError;

/// Tool for loading and activating workflow instructions for a specific skill on demand.
pub struct ActivateSkillTool {
    registry: Arc<SkillRegistry>,
}

impl ActivateSkillTool {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry }
    }
}

impl Tool for ActivateSkillTool {
    fn name(&self) -> &str {
        "activate_skill"
    }

    fn description(&self) -> &str {
        "Activate and load the detailed step-by-step instructions for a specific workflow skill (e.g. 'explore', 'plan', 'implement', 'debug', 'verify', 'review'). Call this when beginning a phase of work to follow best practices and tool guidance."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The exact name of the skill to activate (e.g. 'debug', 'verify', 'explore', 'implement', 'plan', 'review')"
                }
            },
            "required": ["name"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'name'".to_string()))?;

        if let Some(skill) = self.registry.get(name) {
            let res = format!(
                "Skill '{}' (v{}, scope: {}) activated successfully.\n\n{}",
                skill.name,
                skill.version.as_deref().unwrap_or("1.0.0"),
                skill.scope,
                skill.instructions
            );
            Ok(Value::String(res))
        } else {
            let available: Vec<&str> = self.registry.list().map(|s| s.name.as_str()).collect();
            Err(ToolError::ExecutionFailed(format!(
                "Skill '{}' not found. Available skills: [{}]",
                name,
                available.join(", ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activate_skill_tool() {
        let mut reg = SkillRegistry::default();
        for skill in crate::skills::builtin_skills() {
            let _ = reg.register(skill);
        }
        let tool = ActivateSkillTool::new(Arc::new(reg));
        assert_eq!(tool.name(), "activate_skill");
        assert_eq!(tool.risk(), ToolRisk::ReadOnly);

        let res = tool.execute(json!({"name": "debug"})).unwrap();
        let text = res.as_str().unwrap();
        assert!(text.contains("debug"));
        assert!(text.contains("Debugging"));

        let err = tool.execute(json!({"name": "non_existent"})).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }
}
