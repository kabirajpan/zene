use std::fs;
use std::path::{Path, PathBuf};
use serde_json::{json, Value};
use crate::traits::tool::{Tool, ToolRisk};
use crate::types::error::ToolError;

/// Tool for exploring directories and project structure safely.
/// In flat mode (depth = 1, default), it lists direct folder contents with item sizes
/// and subitem counts for subdirectories.
/// In recursive tree mode (depth >= 2), it renders a compact, visual ASCII hierarchy tree
/// with automatic folding of large directories (e.g. assets) to prevent context blowouts.
#[derive(Default)]
pub struct ListDirectoryTool {
    workspace_root: Option<PathBuf>,
}

impl ListDirectoryTool {
    pub fn new() -> Self {
        Self { workspace_root: None }
    }

    pub fn with_workspace(mut self, root: PathBuf) -> Self {
        self.workspace_root = Some(root);
        self
    }
}

impl Tool for ListDirectoryTool {
    fn name(&self) -> &str {
        "list_directory"
    }

    fn description(&self) -> &str {
        "List files and subdirectories in a directory path. Depth 1 (default) provides a flat listing with file sizes and subdirectory child counts. Depth 2 or higher provides a visual hierarchy tree (like 'tree'). Set show_all: true (or no_cap: true) to completely disable capping and display all items."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to directory (defaults to current directory '.' if omitted)"
                },
                "depth": {
                    "type": "integer",
                    "description": "Traversal depth. 1 = flat listing of direct contents (default), 2+ = recursive visual hierarchy tree up to specified depth."
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Alias for depth (for backward compatibility with tree inspection)"
                },
                "max_items": {
                    "type": "integer",
                    "description": "Maximum entries to display per directory before capping (defaults to 50 for flat listing, 20 for tree view). Set to 0 for unlimited."
                },
                "show_all": {
                    "type": "boolean",
                    "description": "If true, completely disables capping and displays every file/folder without truncation (no cap). Default is false."
                },
                "no_cap": {
                    "type": "boolean",
                    "description": "Alias for show_all. Set to true to view all items without any truncation or capping."
                },
                "show_hidden": {
                    "type": "boolean",
                    "description": "If true, includes hidden/build directories like .git, target, node_modules (defaults to false)"
                }
            }
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let path_str = args.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        let depth = args.get("depth")
            .and_then(|v| v.as_u64())
            .or_else(|| args.get("max_depth").and_then(|v| v.as_u64()))
            .unwrap_or(1) as usize;

        let no_cap = args.get("show_all")
            .and_then(|v| v.as_bool())
            .or_else(|| args.get("no_cap").and_then(|v| v.as_bool()))
            .or_else(|| args.get("unlimited").and_then(|v| v.as_bool()))
            .unwrap_or(false)
            || args.get("max_items").and_then(|v| v.as_u64()) == Some(0);

        let max_items = args.get("max_items")
            .and_then(|v| v.as_u64())
            .map(|v| if v == 0 { usize::MAX } else { v as usize })
            .unwrap_or_else(|| if depth > 1 { 20 } else { 50 });

        let show_hidden = args.get("show_hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let path_obj = Path::new(path_str);
        let resolved = if path_obj.is_absolute() {
            path_obj.to_path_buf()
        } else if let Some(ref ws) = self.workspace_root {
            let ws_name = ws.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let clean_rel = if !ws_name.is_empty() {
                if path_obj == Path::new(ws_name) {
                    Path::new(".")
                } else if let Ok(stripped) = path_obj.strip_prefix(ws_name) {
                    stripped
                } else {
                    path_obj
                }
            } else {
                path_obj
            };
            ws.join(clean_rel)
        } else {
            path_obj.to_path_buf()
        };

        if let Some(ref ws) = self.workspace_root {
            let canon_ws = ws.canonicalize().unwrap_or_else(|_| ws.clone());
            let canon_res = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
            if !canon_res.starts_with(&canon_ws) {
                return Err(ToolError::ExecutionFailed(format!(
                    "Access denied: Path '{}' is outside the project workspace boundary ('{}'). You are strictly forbidden from navigating to parent directories.",
                    path_str,
                    ws.display()
                )));
            }
        }

        if !resolved.exists() {
            return Err(ToolError::ExecutionFailed(format!("Directory not found: {}", path_str)));
        }
        if !resolved.is_dir() {
            return Err(ToolError::ExecutionFailed(format!("Path is not a directory: {}", path_str)));
        }

        // Recursive tree mode (depth >= 2)
        if depth > 1 {
            let mut output = String::new();
            output.push_str(&format!("{}/\n", path_str));
            let tree_cap = if no_cap { usize::MAX } else { max_items };
            render_tree(&resolved, "", 1, depth, tree_cap, &mut output);
            return Ok(Value::String(output));
        }

        // Flat listing mode (depth = 1)
        let read_dir = fs::read_dir(&resolved)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read directory '{}': {}", path_str, e)))?;

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !show_hidden && is_ignored(&name) {
                continue;
            }

            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);

            if is_dir {
                let (subdirs, subfiles) = count_children(&entry.path());
                let summary = match (subdirs, subfiles) {
                    (d, f) if d > 0 && f > 0 => format!(" ({} subdirs, {} files)", d, f),
                    (d, 0) if d > 0 => format!(" ({} subdirs)", d),
                    (0, f) if f > 0 => format!(" ({} files)", f),
                    _ => " (empty)".to_string(),
                };
                dirs.push(format!("[DIR]  {}/{}", name, summary));
            } else {
                let size_str = format_size(size);
                files.push(format!("[FILE] {} ({})", name, size_str));
            }
        }

        dirs.sort();
        files.sort();

        let total_dirs = dirs.len();
        let total_files = files.len();
        let total_entries = total_dirs + total_files;

        let mut all_lines = Vec::with_capacity(total_entries);
        all_lines.extend(dirs);
        all_lines.extend(files);

        if all_lines.is_empty() {
            return Ok(Value::String(format!("Directory '{}' is empty (or contains only hidden/ignored files).", path_str)));
        }

        let limit = if no_cap { total_entries } else { max_items.min(total_entries) };
        let mut output_lines: Vec<String> = all_lines.into_iter().take(limit).collect();

        if !no_cap && total_entries > limit {
            output_lines.push(format!("... ({} more items capped. Set show_all: true or no_cap: true to view everything without truncation)", total_entries - limit));
        }

        output_lines.push(format!("Total: {} directories, {} files", total_dirs, total_files));
        Ok(Value::String(output_lines.join("\n")))
    }
}

fn is_ignored(name: &str) -> bool {
    name.starts_with('.') || name == "target" || name == "node_modules" || name == "dist" || name == ".turbo"
}

fn format_size(size: u64) -> String {
    if size > 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else if size > 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{} B", size)
    }
}

fn count_children(dir: &Path) -> (usize, usize) {
    let mut files = 0;
    let mut dirs = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_ignored(&name) {
                continue;
            }
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    dirs += 1;
                } else {
                    files += 1;
                }
            }
        }
    }
    (dirs, files)
}

fn render_tree(dir: &Path, prefix: &str, current_depth: usize, max_depth: usize, max_items_per_dir: usize, output: &mut String) {
    let mut entries: Vec<PathBuf> = Vec::new();
    if let Ok(read_dir) = fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_ignored(&name) {
                continue;
            }
            entries.push(entry.path());
        }
    }

    // Sort: directories first, then files alphabetically
    entries.sort_by(|a, b| {
        let a_is_dir = a.is_dir();
        let b_is_dir = b.is_dir();
        if a_is_dir == b_is_dir {
            a.file_name().cmp(&b.file_name())
        } else if a_is_dir {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });

    let total = entries.len();
    let display_count = total.min(max_items_per_dir);

    for (idx, path) in entries.iter().take(display_count).enumerate() {
        let is_last = idx == display_count - 1 && total <= max_items_per_dir;
        let branch = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if path.is_dir() {
            let (child_dirs, child_files) = count_children(path);
            let summary = match (child_dirs, child_files) {
                (d, f) if d > 0 && f > 0 => format!("{} subdirs, {} files", d, f),
                (d, 0) if d > 0 => format!("{} subdirs", d),
                (0, f) if f > 0 => format!("{} files", f),
                _ => "empty".to_string(),
            };

            if current_depth < max_depth && (child_dirs > 0 || child_files > 0) {
                output.push_str(&format!("{}{}{}/ ({})\n", prefix, branch, name, summary));
                let next_prefix = format!("{}{}", prefix, child_prefix);
                render_tree(path, &next_prefix, current_depth + 1, max_depth, max_items_per_dir, output);
            } else {
                output.push_str(&format!("{}{}{}/ [{}]\n", prefix, branch, name, summary));
            }
        } else {
            let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            let size_str = format_size(size);
            output.push_str(&format!("{}{}{} ({})\n", prefix, branch, name, size_str));
        }
    }

    if total > max_items_per_dir {
        let remaining = total - max_items_per_dir;
        output.push_str(&format!("{}└── ... ({} more items capped. Set show_all: true or no_cap: true to view all)\n", prefix, remaining));
    }
}
