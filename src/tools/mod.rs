pub mod diagnostics;
pub mod fs;
pub mod git;
pub mod plan;
pub mod registry;
pub mod search;
pub mod skill;
pub mod terminal;
pub mod traits;

// Top-level re-exports for easy consumption
pub use diagnostics::GetDiagnosticsTool;
pub use fs::{DeleteFileTool, EditFileTool, ListDirectoryTool, ReadFileTool, RenameFileTool, WriteFileTool};
pub use git::{GitDiffTool, GitStatusTool};
pub use plan::{CreatePlanTool, PlanState, PlanStep, UpdatePlanStepTool};
pub use registry::ToolRegistry;
pub use search::SearchTool;
pub use skill::ActivateSkillTool;
pub use terminal::RunTerminalTool;
pub use traits::Tool;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_minimal_registry() {
        let reg = ToolRegistry::minimal();
        assert_eq!(reg.len(), 2);
        let names = reg.list_names();
        assert_eq!(names, vec!["get_diagnostics", "read_file"]);
    }

    #[test]
    fn test_essentials_registry() {
        let reg = ToolRegistry::essentials();
        assert_eq!(reg.len(), 9);
        let names = reg.list_names();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"get_diagnostics"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"edit_file"));
        assert!(names.contains(&"list_directory"));
        assert!(names.contains(&"search"));
        assert!(names.contains(&"run_terminal"));
        assert!(names.contains(&"create_plan"));
        assert!(names.contains(&"update_plan_step"));
        // get_project_structure is transparently available via alias
        assert!(reg.find("get_project_structure").is_some());
    }

    #[test]
    fn test_all_registry() {
        let reg = ToolRegistry::all();
        assert_eq!(reg.len(), 13);
        let names = reg.list_names();
        assert!(names.contains(&"search"));
        assert!(names.contains(&"create_plan"));
        assert!(names.contains(&"delete_file"));
        assert!(names.contains(&"get_diagnostics"));
        assert!(names.contains(&"git_status"));
        assert!(names.contains(&"git_diff"));
        assert!(names.contains(&"rename_file"));
        assert!(names.contains(&"update_plan_step"));
        // get_project_structure is available via alias
        assert!(reg.find("get_project_structure").is_some());
    }

    #[test]
    fn test_file_operations_lifecycle() {
        let reg = ToolRegistry::all();
        let test_file = "target/test_modular_lifecycle.txt";
        let renamed_file = "target/test_modular_renamed.txt";

        // 1. Write file
        let write_res = reg.execute(
            "write_file",
            json!({
                "path": test_file,
                "content": "line 1\nline 2\nline 3"
            }),
        );
        assert!(write_res.is_ok());

        // 2. Read file with range
        let read_res = reg.execute(
            "read_file",
            json!({
                "path": test_file,
                "start_line": 2,
                "end_line": 2
            }),
        );
        assert!(read_res.is_ok());
        assert!(read_res.unwrap().as_str().unwrap().contains("line 2"));

        // 3. Edit file
        let edit_res = reg.execute(
            "edit_file",
            json!({
                "path": test_file,
                "target": "line 2",
                "replacement": "line 2 patched"
            }),
        );
        assert!(edit_res.is_ok());

        // 4. Search codebase for replacement
        let search_res = reg.execute(
            "search_codebase",
            json!({
                "path": test_file,
                "query": "patched"
            }),
        );
        assert!(search_res.is_ok());
        assert!(search_res.unwrap().as_str().unwrap().contains("line 2 patched"));

        // 5. Rename file
        let rename_res = reg.execute(
            "rename_file",
            json!({
                "old_path": test_file,
                "new_path": renamed_file
            }),
        );
        assert!(rename_res.is_ok());

        // 6. Delete file
        let del_res = reg.execute(
            "delete_file",
            json!({
                "path": renamed_file
            }),
        );
        assert!(del_res.is_ok());
    }

    #[test]
    fn test_planning_tool() {
        let reg = ToolRegistry::all();

        let plan_res = reg.execute(
            "create_plan",
            json!({
                "steps": [
                    "Inspect code",
                    "Apply changes",
                    "Run tests"
                ]
            }),
        );
        assert!(plan_res.is_ok());
        assert!(plan_res.unwrap().as_str().unwrap().contains("Step 1: Inspect code"));

        let update_res = reg.execute(
            "update_plan_step",
            json!({
                "step_id": 1,
                "status": "completed"
            }),
        );
        assert!(update_res.is_ok());
        assert!(update_res.unwrap().as_str().unwrap().contains("[x] Step 1"));
    }

    #[test]
    fn test_git_tools() {
        let reg = ToolRegistry::all();
        let status_res = reg.execute("git_status", json!({}));
        assert!(status_res.is_ok());

        let diff_res = reg.execute("git_diff", json!({}));
        assert!(diff_res.is_ok());
    }

    #[test]
    fn test_terminal_exec() {
        let reg = ToolRegistry::essentials();
        let term_res = reg.execute(
            "run_terminal",
            json!({
                "command": "echo 'running terminal tool'"
            }),
        );
        assert!(term_res.is_ok());
        let out = term_res.unwrap();
        let out_str = out.as_str().unwrap();
        assert!(out_str.contains("running terminal tool"));
        assert!(out_str.contains("Exit code: 0"));
    }

    #[test]
    fn test_schemas_conformance() {
        let minimal = ToolRegistry::minimal();
        assert_eq!(minimal.to_openai_tools().as_array().unwrap().len(), 2);
        assert_eq!(minimal.to_gemini_tools().as_array().unwrap().len(), 2);

        let reg = ToolRegistry::essentials();
        let openai = reg.to_openai_tools();
        assert_eq!(openai.as_array().unwrap().len(), 9);

        let gemini = reg.to_gemini_tools();
        assert_eq!(gemini.as_array().unwrap().len(), 9);
    }

    #[test]
    fn test_project_structure_and_find_files() {
        let reg = ToolRegistry::essentials();

        // Test compact project structure
        let struct_res = reg.execute("get_project_structure", json!({ "path": ".", "max_depth": 2 }));
        assert!(struct_res.is_ok());
        let tree_str = struct_res.unwrap();
        let tree_val = tree_str.as_str().unwrap();
        assert!(tree_val.contains("src/") || tree_val.contains("crates/"));
        assert!(tree_val.contains("Cargo.toml"));

        // Test file pattern search
        let find_res = reg.execute("find_files", json!({ "pattern": "*.toml" }));
        assert!(find_res.is_ok());
        let files_str = find_res.unwrap();
        let files_val = files_str.as_str().unwrap();
        assert!(files_val.contains("Cargo.toml"));

        // Test compact list directory
        let list_res = reg.execute("list_directory", json!({ "path": ".", "max_items": 5 }));
        assert!(list_res.is_ok());
        let list_val = list_res.unwrap();
        let list_str = list_val.as_str().unwrap();
        assert!(list_str.contains("Total:"));

        // Test list_directory in recursive tree mode (depth: 2)
        let list_tree_res = reg.execute("list_directory", json!({ "path": ".", "depth": 2 }));
        assert!(list_tree_res.is_ok());
        let list_tree_str = list_tree_res.unwrap();
        let list_tree_val = list_tree_str.as_str().unwrap();
        assert!(list_tree_val.contains("├── ") || list_tree_val.contains("└── "));

        // Test no_cap / show_all mode
        let no_cap_res = reg.execute("list_directory", json!({ "path": ".", "no_cap": true }));
        assert!(no_cap_res.is_ok());
        let no_cap_val = no_cap_res.unwrap();
        assert!(!no_cap_val.as_str().unwrap().contains("items capped"));
    }

    #[test]
    fn test_namespaced_tool_lookup_and_execution() {
        let reg = ToolRegistry::essentials();
        assert!(reg.find("repo_browser.get_project_structure").is_some());
        assert!(reg.find("functions.read_file").is_some());
        assert!(reg.find("tools.search").is_some());

        let res = reg.execute("repo_browser.get_project_structure", json!({ "path": ".", "max_depth": 1 }));
        assert!(res.is_ok());
    }
}
