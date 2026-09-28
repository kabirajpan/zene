use serde::{Deserialize, Serialize};

/// Categorizes the user's intent to decide tool provisioning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentIntent {
    /// Pure conversational exchange, brainstorming, advice, architecture Q&A.
    /// Loaded tools: 0 (saves ~3,750 tokens per message).
    Discussion,

    /// Code exploration, discovery, finding where logic lives.
    /// Loaded tools: search, read_file.
    Exploration,

    /// Concrete code modification, bug fixing, terminal execution.
    /// Loaded tools: targeted 3-5 action tools on-demand.
    Action(ActionCategory),
}

/// Action sub-categories defining targeted tool packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionCategory {
    /// Bug fixing, error diagnostics, runtime failure investigation.
    Debug,
    /// Adding new features, writing new code, implementing specifications.
    Implement,
    /// Restructuring code, renaming symbols, improving cleanliness.
    Refactor,
    /// Running tests, builds, compiler checks, verification.
    Verify,
    /// Diff inspection, git status, code quality review.
    Review,
    /// General multi-step execution.
    General,
}

impl AgentIntent {
    /// Returns the initial set of tool names needed for this intent.
    pub fn initial_tools(&self) -> Vec<&'static str> {
        match self {
            AgentIntent::Discussion => vec![
                "search",
                "read_file",
                "list_directory",
                "activate_skill",
            ],
            AgentIntent::Exploration => vec![
                "search",
                "read_file",
                "list_directory",
                "activate_skill",
            ],
            AgentIntent::Action(cat) => match cat {
                ActionCategory::Debug => vec![
                    "create_plan",
                    "update_plan_step",
                    "search",
                    "read_file",
                    "edit_file",
                    "get_diagnostics",
                    "run_terminal",
                    "activate_skill",
                ],
                ActionCategory::Implement => vec![
                    "create_plan",
                    "update_plan_step",
                    "search",
                    "read_file",
                    "list_directory",
                    "edit_file",
                    "write_file",
                    "run_terminal",
                    "get_diagnostics",
                    "activate_skill",
                ],
                ActionCategory::Refactor => vec![
                    "create_plan",
                    "update_plan_step",
                    "search",
                    "read_file",
                    "edit_file",
                    "rename_file",
                    "get_diagnostics",
                    "activate_skill",
                ],
                ActionCategory::Verify => vec![
                    "update_plan_step",
                    "run_terminal",
                    "get_diagnostics",
                    "read_file",
                    "activate_skill",
                ],
                ActionCategory::Review => vec![
                    "git_status",
                    "git_diff",
                    "get_diagnostics",
                    "read_file",
                    "activate_skill",
                ],
                ActionCategory::General => vec![
                    "create_plan",
                    "update_plan_step",
                    "search",
                    "read_file",
                    "list_directory",
                    "edit_file",
                    "write_file",
                    "run_terminal",
                    "get_diagnostics",
                    "activate_skill",
                ],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolRegistry;

    #[test]
    fn test_discussion_intent_has_safe_readonly_tools() {
        let intent = AgentIntent::Discussion;
        assert!(intent.initial_tools().contains(&"search"));
        assert!(intent.initial_tools().contains(&"read_file"));
        assert!(intent.initial_tools().contains(&"list_directory"));
        assert!(intent.initial_tools().contains(&"activate_skill"));
        assert!(!intent.initial_tools().contains(&"edit_file"));
    }

    #[test]
    fn test_exploration_intent_tools() {
        let intent = AgentIntent::Exploration;
        assert!(intent.initial_tools().contains(&"search"));
        assert!(intent.initial_tools().contains(&"read_file"));
        assert!(intent.initial_tools().contains(&"list_directory"));
        assert!(intent.initial_tools().contains(&"activate_skill"));
    }

    #[test]
    fn test_action_debug_intent_tools() {
        let intent = AgentIntent::Action(ActionCategory::Debug);
        assert!(intent.initial_tools().contains(&"edit_file"));
        assert!(intent.initial_tools().contains(&"get_diagnostics"));
    }

    #[test]
    fn test_selective_schemas_and_manifest() {
        let registry = ToolRegistry::all();
        let manifest = registry.manifest();
        assert!(manifest.contains("read_file"));
        assert!(manifest.contains("edit_file"));

        // Discussion has 0 schemas
        let empty_schemas = registry.openai_schemas_for(&[]);
        assert!(empty_schemas.is_empty());

        // Selective schemas only return requested tools
        let selected = registry.openai_schemas_for(&["read_file", "edit_file"]);
        assert_eq!(selected.len(), 2);
    }
}

