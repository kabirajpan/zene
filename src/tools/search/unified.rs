use std::fs;
use std::path::Path;
use std::sync::Arc;
use regex::Regex;
use serde_json::{json, Value};

use crate::skills::SkillRegistry;
use crate::traits::tool::{Tool, ToolRisk};
use crate::types::error::ToolError;

/// Type alias for a provider function that supplies tool names and descriptions.
pub type ToolListProvider = Arc<dyn Fn() -> Vec<(String, String)> + Send + Sync>;

/// Universal search tool supporting 6 scopes: tools, skills, folders, files, symbols, content.
pub struct SearchTool {
    skills: Option<Arc<SkillRegistry>>,
    tools_provider: Option<ToolListProvider>,
    ast: Arc<crate::ast::DynamicAstEngine>,
}

impl Default for SearchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchTool {
    /// Create a new SearchTool without external registries.
    pub fn new() -> Self {
        Self {
            skills: None,
            tools_provider: None,
            ast: Arc::new(crate::ast::DynamicAstEngine::new()),
        }
    }

    /// Attach a custom AST engine.
    pub fn with_ast(mut self, ast: Arc<crate::ast::DynamicAstEngine>) -> Self {
        self.ast = ast;
        self
    }

    /// Attach a skills registry for `in: "skills"` scope.
    pub fn with_skills(mut self, skills: Arc<SkillRegistry>) -> Self {
        self.skills = Some(skills);
        self
    }

    /// Attach a tool descriptions provider for `in: "tools"` scope.
    pub fn with_tools_provider(mut self, provider: ToolListProvider) -> Self {
        self.tools_provider = Some(provider);
        self
    }
}

#[derive(Default)]
struct Scopes {
    tools: bool,
    skills: bool,
    folders: bool,
    files: bool,
    symbols: bool,
    content: bool,
}

impl Scopes {
    fn parse(in_str: &str) -> Self {
        let trimmed = in_str.trim().to_lowercase();
        if trimmed == "all" || trimmed.is_empty() {
            return Self {
                tools: true,
                skills: true,
                folders: true,
                files: true,
                symbols: true,
                content: true,
            };
        }

        let mut scopes = Self::default();
        for part in trimmed.split('+') {
            match part.trim() {
                "tools" => scopes.tools = true,
                "skills" => scopes.skills = true,
                "folders" => scopes.folders = true,
                "files" => scopes.files = true,
                "symbols" => scopes.symbols = true,
                "content" => scopes.content = true,
                "all" => {
                    return Self {
                        tools: true,
                        skills: true,
                        folders: true,
                        files: true,
                        symbols: true,
                        content: true,
                    };
                }
                _ => {}
            }
        }
        scopes
    }
}

struct MatchLine {
    enclosing: Option<String>,
    line_num: usize,
    text: String,
}

struct FileMatches {
    path: String,
    total_lines: usize,
    matches: Vec<MatchLine>,
}

struct SymbolMatch {
    path: String,
    line_num: usize,
    kind: String,
    signature: String,
}

impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Universal search across tools, skills, folders, files, symbols (definitions), or content. Combine scopes with '+' (e.g. 'symbols', 'tools+skills', 'folders+files', 'symbols+content', or 'all')."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What to look for (symbol name, keyword, or regex pattern)"
                },
                "in": {
                    "type": "string",
                    "description": "Combinator of scopes to search in: tools | skills | folders | files | symbols | content (default: 'all', combine with '+', e.g. 'symbols+content')"
                },
                "path": {
                    "type": "string",
                    "description": "Root directory path for filesystem searches (default: '.')"
                },
                "ext": {
                    "type": "string",
                    "description": "File extension filter, e.g. 'rs' or 'rs,toml,md'"
                },
                "regex": {
                    "type": "boolean",
                    "description": "Treat query as a regular expression (default: false)"
                },
                "case": {
                    "type": "boolean",
                    "description": "Whether search is case-sensitive (default: false)"
                }
            },
            "required": ["query"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'query'".to_string()))?;

        let in_param = args.get("in").and_then(|v| v.as_str()).unwrap_or("all");
        let path_param = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let ext_param = args.get("ext").and_then(|v| v.as_str());
        let is_regex = args.get("regex").and_then(|v| v.as_bool()).unwrap_or(false);
        let is_case = args.get("case").and_then(|v| v.as_bool()).unwrap_or(false);

        let scopes = Scopes::parse(in_param);

        // Prepare extensions filter set
        let extensions: Option<Vec<String>> = ext_param.map(|exts| {
            exts.split(',')
                .map(|e| e.trim().trim_start_matches('.').to_lowercase())
                .filter(|e| !e.is_empty())
                .collect()
        });

        // Compile regex matcher or plain matcher
        let regex_matcher: Option<Regex> = if is_regex {
            let pattern = if is_case {
                query.to_string()
            } else {
                format!("(?i){}", query)
            };
            Some(Regex::new(&pattern).map_err(|e| ToolError::InvalidArgs(format!("Invalid regex '{}': {}", query, e)))?)
        } else {
            None
        };

        let query_lower = query.to_lowercase();
        let matches_text = |text: &str| -> bool {
            if let Some(ref re) = regex_matcher {
                re.is_match(text)
            } else if query_lower.starts_with('*') {
                text.to_lowercase().ends_with(&query_lower[1..])
            } else if query_lower.contains('*') {
                let parts: Vec<&str> = query_lower.split('*').collect();
                if parts.len() == 2 {
                    let tl = text.to_lowercase();
                    tl.starts_with(parts[0]) && tl.ends_with(parts[1])
                } else {
                    text.to_lowercase().contains(&query_lower)
                }
            } else if is_case {
                text.contains(query)
            } else {
                text.to_lowercase().contains(&query_lower)
            }
        };

        let mut out_tools: Vec<(String, String)> = Vec::new();
        let mut out_skills: Vec<(String, String)> = Vec::new();
        let mut out_folders: Vec<String> = Vec::new();
        let mut out_files: Vec<(String, usize)> = Vec::new();
        let mut out_symbols: Vec<SymbolMatch> = Vec::new();
        let mut out_matches: Vec<FileMatches> = Vec::new();

        // 1. Scope: Tools
        if scopes.tools {
            let tools_list = if let Some(ref provider) = self.tools_provider {
                provider()
            } else {
                default_known_tools()
            };

            for (name, desc) in tools_list {
                if matches_text(&name) || matches_text(&desc) {
                    out_tools.push((name, desc));
                }
            }
        }

        // 2. Scope: Skills
        if scopes.skills {
            if let Some(ref skills_reg) = self.skills {
                for skill in skills_reg.list() {
                    let desc = if skill.description.is_empty() {
                        skill.scope.to_string()
                    } else {
                        skill.description.clone()
                    };
                    if matches_text(&skill.name) || matches_text(&desc) {
                        out_skills.push((skill.name.clone(), desc));
                    }
                }
            }
        }

        // 3. Filesystem scopes: Folders, Files, Symbols, Content
        let needs_fs = scopes.folders || scopes.files || scopes.symbols || scopes.content;
        if needs_fs {
            let root = Path::new(path_param);
            if !root.exists() {
                return Err(ToolError::ExecutionFailed(format!("Path does not exist: {}", path_param)));
            }

            let mut stack = vec![root.to_path_buf()];
            let mut total_content_matches = 0;
            const MAX_CONTENT_MATCHES: usize = 60;
            const MAX_SYMBOL_MATCHES: usize = 40;

            while let Some(current) = stack.pop() {
                let file_name_str = current.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if current.is_dir() {
                    // Skip ignored directories
                    if file_name_str.starts_with('.') || file_name_str == "target" || file_name_str == "node_modules" || file_name_str == "dist" || file_name_str == "build" {
                        continue;
                    }

                    if scopes.folders && current != root && matches_text(file_name_str) {
                        let rel_path = current.strip_prefix(root).unwrap_or(&current);
                        let display = format!("{}/", rel_path.display());
                        out_folders.push(display);
                    }

                    // Traverse directory contents
                    if let Ok(entries) = fs::read_dir(&current) {
                        for entry in entries.flatten() {
                            stack.push(entry.path());
                        }
                    }
                } else if current.is_file() {
                    // Check extension filter
                    if let Some(ref allowed_exts) = extensions {
                        let file_ext = current
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.to_lowercase())
                            .unwrap_or_default();
                        if !allowed_exts.contains(&file_ext) {
                            continue;
                        }
                    }

                    // Scope: Files
                    let file_matched_name = scopes.files && matches_text(file_name_str);

                    let should_read_content = scopes.symbols || scopes.content || file_matched_name;
                    if should_read_content {
                        if let Ok(content) = fs::read_to_string(&current) {
                            let line_count = content.lines().count();
                            let rel_path = current.strip_prefix(root).unwrap_or(&current).display().to_string();

                            if file_matched_name {
                                out_files.push((rel_path.clone(), line_count));
                            }

                            let ext = current.extension().and_then(|e| e.to_str()).unwrap_or("");

                            // Scope: Symbols (powered by DynamicAstEngine)
                            if scopes.symbols && out_symbols.len() < MAX_SYMBOL_MATCHES {
                                let ast_matches = self.ast.extract_symbols(&content, ext, Some(query));
                                for sym in ast_matches {
                                    if out_symbols.len() >= MAX_SYMBOL_MATCHES {
                                        break;
                                    }
                                    out_symbols.push(SymbolMatch {
                                        path: rel_path.clone(),
                                        line_num: sym.start_line,
                                        kind: sym.kind,
                                        signature: sym.signature,
                                    });
                                }
                            }

                            // Scope: Content (with AST enclosing scope breadcrumbs)
                            if scopes.content && total_content_matches < MAX_CONTENT_MATCHES {
                                let mut file_match_lines = Vec::new();

                                for (idx, line) in content.lines().enumerate() {
                                    let trimmed_line = line.trim();

                                    if matches_text(line) {
                                        let enclosing = self.ast.enclosing_scope(&content, ext, idx + 1);
                                        file_match_lines.push(MatchLine {
                                            enclosing,
                                            line_num: idx + 1,
                                            text: trimmed_line.to_string(),
                                        });
                                        total_content_matches += 1;
                                        if total_content_matches >= MAX_CONTENT_MATCHES {
                                            break;
                                        }
                                    }
                                }

                                if !file_match_lines.is_empty() {
                                    out_matches.push(FileMatches {
                                        path: rel_path,
                                        total_lines: line_count,
                                        matches: file_match_lines,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // Format Output
        let mut output = String::new();
        let mut summary_parts = Vec::new();

        // 1. [tools]
        if !out_tools.is_empty() {
            output.push_str("[tools]\n");
            for (name, desc) in &out_tools {
                output.push_str(&format!("  {:14} : {}\n", name, desc));
            }
            output.push('\n');
            summary_parts.push(format!("{} tool{}", out_tools.len(), if out_tools.len() == 1 { "" } else { "s" }));
        }

        // 2. [skills]
        if !out_skills.is_empty() {
            output.push_str("[skills]\n");
            for (name, desc) in &out_skills {
                output.push_str(&format!("  {:14} : {}\n", name, desc));
            }
            output.push('\n');
            summary_parts.push(format!("{} skill{}", out_skills.len(), if out_skills.len() == 1 { "" } else { "s" }));
        }

        // 3. [folders]
        if !out_folders.is_empty() {
            output.push_str("[folders]\n");
            for folder in &out_folders {
                output.push_str(&format!("  {}\n", folder));
            }
            output.push('\n');
            summary_parts.push(format!("{} folder{}", out_folders.len(), if out_folders.len() == 1 { "" } else { "s" }));
        }

        // 4. [files]
        if !out_files.is_empty() {
            output.push_str("[files]\n");
            for (file, lines) in &out_files {
                output.push_str(&format!("  {}  [{} lines]\n", file, lines));
            }
            output.push('\n');
            summary_parts.push(format!("{} file{}", out_files.len(), if out_files.len() == 1 { "" } else { "s" }));
        }

        // 5. [symbols]
        if !out_symbols.is_empty() {
            output.push_str("[symbols]\n");
            for sym in &out_symbols {
                output.push_str(&format!(
                    "  {}:{}  [{}]  {}\n",
                    sym.path, sym.line_num, sym.kind, sym.signature
                ));
            }
            output.push('\n');
            summary_parts.push(format!("{} symbol{}", out_symbols.len(), if out_symbols.len() == 1 { "" } else { "s" }));
        }

        // 6. [matches]
        if !out_matches.is_empty() {
            output.push_str("[matches]\n");
            let mut total_m = 0;
            let total_f = out_matches.len();

            for fm in &out_matches {
                total_m += fm.matches.len();
                output.push_str(&format!("  {}  [{} lines]  ({}x)\n", fm.path, fm.total_lines, fm.matches.len()));

                let mut current_enc: Option<String> = None;
                for m in &fm.matches {
                    if m.enclosing != current_enc {
                        current_enc = m.enclosing.clone();
                        if let Some(ref enc) = current_enc {
                            output.push_str(&format!("    in {}:\n", enc));
                        }
                    }
                    let indent = if current_enc.is_some() { "      " } else { "    " };
                    output.push_str(&format!("{}{}: {}\n", indent, m.line_num, m.text));
                }
                output.push('\n');
            }
            summary_parts.push(format!("{} match{} in {} file{}", total_m, if total_m == 1 { "" } else { "es" }, total_f, if total_f == 1 { "" } else { "s" }));
        }

        if summary_parts.is_empty() {
            return Ok(Value::String(format!(
                "No results found for query: '{}' (scope: '{}'). Hint: Try broadening the query or search with in: \"all\".",
                query, in_param
            )));
        }

        output.push_str(&summary_parts.join("  |  "));
        Ok(Value::String(output))
    }
}

fn default_known_tools() -> Vec<(String, String)> {
    vec![
        ("search".into(), "Universal search — tools, skills, folders, files, symbols, or content".into()),
        ("read_file".into(), "Read file content (by line range, symbol/function, or peek)".into()),
        ("edit_file".into(), "Make a surgical find-and-replace edit in a file".into()),
        ("write_file".into(), "Write or create a file with full content".into()),
        ("list_directory".into(), "List files and folders in a directory".into()),
        ("get_project_structure".into(), "Show directory tree of the project".into()),
        ("get_diagnostics".into(), "Get compiler and linter errors and warnings".into()),
        ("run_terminal".into(), "Execute a shell command".into()),
        ("create_plan".into(), "Create a step-by-step execution plan".into()),
        ("update_plan_step".into(), "Mark a plan step as done or in-progress".into()),
        ("git_status".into(), "Show changed, staged, and untracked files".into()),
        ("git_diff".into(), "Show the diff of current changes".into()),
        ("delete_file".into(), "Delete a file from disk".into()),
        ("rename_file".into(), "Rename or move a file".into()),
        ("activate_skill".into(), "Load a workflow skill by name for step-by-step guidance".into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_tools_scope() {
        let tool = SearchTool::new();
        let res = tool.execute(json!({
            "query": "read",
            "in": "tools"
        })).unwrap();

        let output = res.as_str().unwrap();
        assert!(output.contains("[tools]"));
        assert!(output.contains("read_file"));
        assert!(output.contains("tool"));
    }

    #[test]
    fn test_search_combinator_parsing() {
        let s = Scopes::parse("symbols+files");
        assert!(s.symbols);
        assert!(s.files);
        assert!(!s.tools);
        assert!(!s.content);

        let all = Scopes::parse("all");
        assert!(all.tools);
        assert!(all.skills);
        assert!(all.folders);
        assert!(all.files);
        assert!(all.symbols);
        assert!(all.content);
    }

    #[test]
    fn test_ast_symbol_search_and_enclosing() {
        let ast = crate::ast::DynamicAstEngine::new();
        let code = r#"
pub struct UserAuth {
    token: String,
}

impl UserAuth {
    pub fn verify_token(&self) -> bool {
        let valid = true;
        valid
    }
}
"#;
        let symbols = ast.extract_symbols(code, "rs", Some("verify_token"));
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].kind, "fn");
        assert_eq!(symbols[0].name, "verify_token");

        let enc = ast.enclosing_scope(code, "rs", 8);
        assert!(enc.is_some());
        let enc_str = enc.unwrap();
        assert!(enc_str.contains("verify_token"));
    }
}

