use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use serde_json::{json, Value};
use crate::traits::tool::{Tool, ToolRisk};
use crate::types::error::ToolError;

/// Default line limit before pagination is applied.
const DEFAULT_LINE_CAP: usize = 500;
/// Hard limit cap that cannot be exceeded in a single call.
const HARD_LINE_LIMIT: usize = 700;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Track inspection history per file for duplicate suppression and smart overlap deduplication.
#[derive(Debug, Clone, Default)]
pub struct FileReadHistory {
    pub mtime: Option<SystemTime>,
    pub is_fully_read: bool,
    pub read_ranges: Vec<(usize, usize)>,
}

impl FileReadHistory {
    pub fn is_range_covered(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        if self.is_fully_read {
            return Some((1, usize::MAX));
        }
        for &(s, e) in &self.read_ranges {
            if s <= start && e >= end {
                return Some((s, e));
            }
        }
        None
    }

    pub fn find_overlap_end(&self, start: usize, end: usize) -> Option<usize> {
        let mut max_covered = None;
        for &(s, e) in &self.read_ranges {
            if s <= start && e >= start && e < end {
                max_covered = Some(max_covered.map_or(e, |m: usize| m.max(e)));
            }
        }
        max_covered
    }

    pub fn record_range(&mut self, start: usize, end: usize) {
        self.read_ranges.push((start, end));
        self.read_ranges.sort_unstable_by_key(|r| r.0);
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(self.read_ranges.len());
        for (s, e) in self.read_ranges.drain(..) {
            if let Some(last) = merged.last_mut() {
                if s <= last.1 + 1 {
                    last.1 = last.1.max(e);
                    continue;
                }
            }
            merged.push((s, e));
        }
        self.read_ranges = merged;
    }
}

/// Tool for reading file contents safely, supporting ranges, symbol extraction, peek mode, and batching.
pub struct ReadFileTool {
    ast: Arc<crate::ast::DynamicAstEngine>,
    history: Arc<Mutex<HashMap<String, FileReadHistory>>>,
    workspace_root: Option<PathBuf>,
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadFileTool {
    pub fn new() -> Self {
        Self {
            ast: Arc::new(crate::ast::DynamicAstEngine::new()),
            history: Arc::new(Mutex::new(HashMap::new())),
            workspace_root: None,
        }
    }

    pub fn with_ast(mut self, ast: Arc<crate::ast::DynamicAstEngine>) -> Self {
        self.ast = ast;
        self
    }

    pub fn with_workspace(mut self, root: PathBuf) -> Self {
        self.workspace_root = Some(root);
        self
    }

    pub fn clear_cache(&self) {
        if let Ok(mut lock) = self.history.lock() {
            lock.clear();
        }
    }
}

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read file content safely. Supports line ranges (1-indexed), symbol extraction (e.g. symbol: 'fn_name'), file inspection with peek: true, and batch reading with paths: [...]."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative or absolute path to the file (optional if paths is provided)"
                },
                "paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of file paths for batch reading in a single roundtrip"
                },
                "start_line": {
                    "type": "integer",
                    "description": "Optional 1-indexed start line number"
                },
                "end_line": {
                    "type": "integer",
                    "description": "Optional 1-indexed end line number (inclusive)"
                },
                "symbol": {
                    "type": "string",
                    "description": "Optional symbol name(s) to read (e.g. 'verify_token' or 'login,logout'). Reads only the function/class body."
                },
                "peek": {
                    "type": "boolean",
                    "description": "If true, returns file stats (line count, size, modified time) and symbol outline without loading full body (default: false)"
                },
                "show_all": {
                    "type": "boolean",
                    "description": "If true, reads up to the 700-line hard cap without the 500-line default truncation (default: false)"
                }
            }
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn execute(&self, args: Value) -> Result<Value, ToolError> {
        // Handle batch multi-file reading
        if let Some(paths_val) = args.get("paths").and_then(|v| v.as_array()) {
            if !paths_val.is_empty() {
                let mut combined = String::new();
                for (idx, p_val) in paths_val.iter().enumerate() {
                    let p_str = p_val.as_str().ok_or_else(|| {
                        ToolError::InvalidArgs("Each element in 'paths' must be a string".to_string())
                    })?;
                    let mut single_args = args.clone();
                    if let Some(obj) = single_args.as_object_mut() {
                        obj.remove("paths");
                        obj.insert("path".to_string(), Value::String(p_str.to_string()));
                    }
                    let res = self.read_single_file(p_str, &single_args)?;
                    let res_str = match res {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    combined.push_str(&format!("=== [{}/{}] {} ===\n{}\n\n", idx + 1, paths_val.len(), p_str, res_str.trim()));
                }
                return Ok(Value::String(combined.trim().to_string()));
            }
        }

        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("Missing required parameter 'path' or 'paths'".to_string()))?;

        self.read_single_file(path_str, &args)
    }
}

impl ReadFileTool {
    fn read_single_file(&self, path_str: &str, args: &Value) -> Result<Value, ToolError> {
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

        let path = &resolved;
        if !path.exists() {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or(path_str);
            return Err(ToolError::ExecutionFailed(format!(
                "File not found: '{}'. Hint: Use search(query: \"{}\", in: \"files\") to locate the correct file path.",
                path_str, file_name
            )));
        }
        if path.is_dir() {
            if let Ok(entries) = fs::read_dir(path) {
                let mut list = Vec::new();
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        list.push(format!("{}/", name));
                    } else {
                        list.push(name);
                    }
                }
                list.sort();
                return Ok(Value::String(format!(
                    "[directory: {} contains {} items: {}]\nHint: Call read_file on individual files inside this directory to view their code.",
                    path_str, list.len(), list.join(", ")
                )));
            }
            return Err(ToolError::ExecutionFailed(format!(
                "Path is a directory, not a file: '{}'. Hint: Use search(path: \"{}\", in: \"files\") or list_directory to inspect its contents.",
                path_str, path_str
            )));
        }

        // 1. Binary Shield: Check extension and raw bytes
        if is_binary_extension(path) {
            let metadata = fs::metadata(path).ok();
            let size_kb = metadata.map(|m| m.len() as f64 / 1024.0).unwrap_or(0.0);
            return Ok(Value::String(format!(
                "[binary asset: {}]\n  size   : {:.1} KB\n  status : Cannot display binary file as text.",
                path_str, size_kb
            )));
        }

        let raw_bytes = fs::read(path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read '{}': {}", path_str, e)))?;

        // Null-byte check in first 512 bytes
        let sample_len = raw_bytes.len().min(512);
        if raw_bytes[..sample_len].contains(&0) {
            let size_kb = raw_bytes.len() as f64 / 1024.0;
            return Ok(Value::String(format!(
                "[binary file: {}]\n  size   : {:.1} KB\n  status : Detected binary content. Cannot display as text.",
                path_str, size_kb
            )));
        }

        let content = String::from_utf8(raw_bytes)
            .map_err(|_| ToolError::ExecutionFailed(format!("File '{}' is not valid UTF-8 text", path_str)))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let is_peek = args.get("peek").and_then(|v| v.as_bool()).unwrap_or(false);
        let symbol_param = args.get("symbol").and_then(|v| v.as_str());
        let start_line = args.get("start_line").and_then(|v| v.as_u64()).map(|v| v as usize);
        let end_line = args.get("end_line").and_then(|v| v.as_u64()).map(|v| v as usize);
        let show_all = args.get("show_all").and_then(|v| v.as_bool()).unwrap_or(false);

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        // 2. Peek Mode: Metadata + Outline
        if is_peek {
            let metadata = fs::metadata(path).ok();
            let size_kb = metadata.as_ref().map(|m| m.len() as f64 / 1024.0).unwrap_or(0.0);
            let modified_str = metadata
                .and_then(|m| m.modified().ok())
                .and_then(|m| format_system_time(m))
                .unwrap_or_else(|| "unknown".to_string());

            let mut outline_lines = Vec::new();
            for (idx, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if let Some(item) = format_outline_item(trimmed) {
                    outline_lines.push(format!("    {:4}: {}", idx + 1, item));
                }
            }

            if outline_lines.is_empty() {
                let ast_symbols = self.ast.extract_symbols(&content, ext, None);
                for sym in ast_symbols {
                    outline_lines.push(format!("    {:4}: [{}] {}", sym.start_line, sym.kind, sym.name));
                }
            }

            let mut out = format!(
                "[peek: {}]\n  lines    : {}\n  size     : {:.1} KB\n  modified : {}\n",
                path_str, total_lines, size_kb, modified_str
            );
            if !outline_lines.is_empty() {
                out.push_str("  outline  :\n");
                out.push_str(&outline_lines.join("\n"));
            } else {
                out.push_str("  outline  : (no top-level definitions detected)\n");
            }
            return Ok(Value::String(out));
        }

        // 3. Symbol / Function Extraction Mode
        if let Some(symbols_str) = symbol_param {
            let symbols: Vec<&str> = symbols_str.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            if symbols.is_empty() {
                return Err(ToolError::InvalidArgs("Parameter 'symbol' cannot be empty".to_string()));
            }

            let mut out = String::new();
            for sym in &symbols {
                let range = self.ast.locate_symbol_range(&content, ext, sym)
                    .or_else(|| extract_symbol(&lines, sym));
                match range {
                    Some((start, end)) => {
                        let count = end - start + 1;
                        if count > HARD_LINE_LIMIT {
                            out.push_str(&format!(
                                "[symbol: {} in {} (lines {}..{}, capped at {} lines)]\n",
                                sym, path_str, start, start + HARD_LINE_LIMIT - 1, HARD_LINE_LIMIT
                            ));
                            for (idx, line) in lines[start - 1..start - 1 + HARD_LINE_LIMIT].iter().enumerate() {
                                out.push_str(&format!("{:4} | {}\n", start + idx, line));
                            }
                        } else {
                            out.push_str(&format!(
                                "[symbol: {} in {} (lines {}..{})]\n",
                                sym, path_str, start, end
                            ));
                            for (idx, line) in lines[start - 1..end].iter().enumerate() {
                                out.push_str(&format!("{:4} | {}\n", start + idx, line));
                            }
                        }
                        out.push('\n');
                    }
                    None => {
                        return Err(ToolError::ExecutionFailed(format!(
                            "Symbol '{}' not found in file '{}'. Hint: Use read_file(path: \"{}\", peek: true) to inspect available symbols, or search(query: \"{}\", in: \"symbols\") to locate its definition across the codebase.",
                            sym, path_str, path_str, sym
                        )));
                    }
                }
            }
            return Ok(Value::String(out.trim_end().to_string()));
        }

        let path_key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().to_string();
        let current_mtime = fs::metadata(path).ok().and_then(|m| m.modified().ok());

        // 4. Sliced Range Mode with Scope Breadcrumb & Overlap Deduplication
        if start_line.is_some() || end_line.is_some() {
            let mut start = start_line.unwrap_or(1).max(1);
            let end = end_line.unwrap_or(total_lines).min(total_lines);

            if start > total_lines {
                return Ok(Value::String(format!(
                    "// [start_line {} exceeds total file lines ({})]",
                    start, total_lines
                )));
            }

            let window_len = (end.saturating_sub(start) + 1).min(HARD_LINE_LIMIT);
            let actual_end = (start + window_len - 1).min(total_lines);

            let mut prior_overlap_note = None;
            if let Ok(mut lock) = self.history.lock() {
                let entry = lock.entry(path_key.clone()).or_default();
                if entry.mtime != current_mtime {
                    *entry = FileReadHistory {
                        mtime: current_mtime,
                        is_fully_read: false,
                        read_ranges: Vec::new(),
                    };
                }

                // A. Check if the requested range is 100% covered already
                if let Some((cov_s, cov_e)) = entry.is_range_covered(start, actual_end) {
                    let cov_str = if entry.is_fully_read {
                        "full file read earlier in context".to_string()
                    } else {
                        format!("prior read of lines {}..{}", cov_s, cov_e)
                    };
                    return Ok(Value::String(format!(
                        "[file: {} lines {}..{} are ALREADY in your context (covered by {}).]\n\
                        Hint: Do not re-read lines already provided. Use the code above to formulate your response or plan edits.",
                        path_str, start, actual_end, cov_str
                    )));
                }

                // B. Check for partial overlap at the beginning (e.g. read 1..100, now requested 50..130)
                if let Some(overlap_end) = entry.find_overlap_end(start, actual_end) {
                    let old_start = start;
                    start = overlap_end + 1;
                    prior_overlap_note = Some((old_start, overlap_end));
                }

                entry.record_range(start, actual_end);
            }

            let enclosing = self.ast.enclosing_scope(&content, ext, start)
                .or_else(|| find_enclosing_scope(&lines, start));
            let mut formatted = if let Some((old_s, old_e)) = prior_overlap_note {
                format!(
                    "[file: {} | lines {}..{} of {} (lines {}..{} were already provided in prior context)]\n",
                    path_str, start, actual_end, total_lines, old_s, old_e
                )
            } else if let Some(ref enc) = enclosing {
                format!(
                    "[file: {} | lines {}..{} of {} | inside: {}]\n",
                    path_str, start, actual_end, total_lines, enc
                )
            } else {
                format!(
                    "[file: {} | lines {}..{} of {}]\n",
                    path_str, start, actual_end, total_lines
                )
            };

            for (idx, line) in lines[start - 1..actual_end].iter().enumerate() {
                formatted.push_str(&format!("{:4} | {}\n", start + idx, line));
            }

            if end > actual_end {
                formatted.push_str(&format!(
                    "\n// ... (hard limit: remaining {} lines omitted. Use start_line: {} to read further)\n",
                    end - actual_end, actual_end + 1
                ));
            }

            return Ok(Value::String(formatted));
        }

        // 5. Full / Default Mode with Read Deduplication Guard
        if let Ok(mut lock) = self.history.lock() {
            let entry = lock.entry(path_key.clone()).or_default();
            if entry.mtime != current_mtime {
                *entry = FileReadHistory {
                    mtime: current_mtime,
                    is_fully_read: false,
                    read_ranges: Vec::new(),
                };
            }

            if entry.is_fully_read {
                return Ok(Value::String(format!(
                    "[file: {} ({} lines) has ALREADY been read in full and is present in your conversation context.]\n\
                    Hint: Do not re-read this file. Its complete source is already available above. Please proceed to answer the user's question, formulate your plan, or make edits.",
                    path_str, total_lines
                )));
            }

            if total_lines <= DEFAULT_LINE_CAP {
                entry.is_fully_read = true;
                entry.record_range(1, total_lines);
            } else {
                let limit = if show_all { total_lines.min(HARD_LINE_LIMIT) } else { DEFAULT_LINE_CAP };
                entry.record_range(1, limit);
            }
        }

        if total_lines <= DEFAULT_LINE_CAP {
            let mut formatted = format!("[file: {} ({} lines)]\n", path_str, total_lines);
            for (idx, line) in lines.iter().enumerate() {
                formatted.push_str(&format!("{:4} | {}\n", idx + 1, line));
            }
            Ok(Value::String(formatted))
        } else if show_all {
            let limit = total_lines.min(HARD_LINE_LIMIT);
            let mut formatted = format!(
                "[file: {} | showing lines 1..{} of {} (hard limit cap)]\n",
                path_str, limit, total_lines
            );
            for (idx, line) in lines[..limit].iter().enumerate() {
                formatted.push_str(&format!("{:4} | {}\n", idx + 1, line));
            }
            if total_lines > HARD_LINE_LIMIT {
                formatted.push_str(&format!(
                    "\n// [Hard cap: {} more lines omitted. Use start_line: 701 to read further]\n",
                    total_lines - HARD_LINE_LIMIT
                ));
            }
            Ok(Value::String(formatted))
        } else {
            // Default cap at 500 with actionable copy-paste pagination hint
            let mut formatted = format!(
                "[file: {} | showing lines 1..{} of {}]\n",
                path_str, DEFAULT_LINE_CAP, total_lines
            );
            for (idx, line) in lines[..DEFAULT_LINE_CAP].iter().enumerate() {
                formatted.push_str(&format!("{:4} | {}\n", idx + 1, line));
            }
            formatted.push_str(&format!(
                "\n// [Showing 1..500 of {} lines. Next chunk: read_file(path: \"{}\", start_line: 501, end_line: {})]\n// [Or use peek: true to view symbol outline, or symbol: \"name\" to read specific functions]\n",
                total_lines,
                path_str,
                (500 + DEFAULT_LINE_CAP).min(total_lines)
            ));
            Ok(Value::String(formatted))
        }
    }
}

/// Helper identifying binary extensions.
fn is_binary_extension(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" | "tiff"
            | "wasm" | "sqlite" | "sqlite3" | "db" | "bin" | "dat"
            | "pdf" | "zip" | "tar" | "gz" | "bz2" | "xz" | "7z"
            | "lockb" | "exe" | "so" | "dylib" | "dll" | "class" | "o" | "a"
            | "woff" | "woff2" | "ttf" | "eot" | "otf"
            | "mp4" | "mp3" | "wav" | "flac" | "ogg" | "webm"
    )
}

/// Helper formatting outline item for peek mode.
fn format_outline_item(line: &str) -> Option<String> {
    if line.starts_with("pub fn ") || line.starts_with("fn ") || line.starts_with("pub async fn ") || line.starts_with("async fn ") {
        let sig = line.trim_end_matches('{').trim();
        Some(sig.to_string())
    } else if line.starts_with("pub struct ") || line.starts_with("struct ") {
        let sig = line.trim_end_matches('{').trim();
        Some(sig.to_string())
    } else if line.starts_with("pub enum ") || line.starts_with("enum ") {
        let sig = line.trim_end_matches('{').trim();
        Some(sig.to_string())
    } else if line.starts_with("pub trait ") || line.starts_with("trait ") {
        let sig = line.trim_end_matches('{').trim();
        Some(sig.to_string())
    } else if line.starts_with("impl ") {
        let sig = line.trim_end_matches('{').trim();
        Some(sig.to_string())
    } else if line.starts_with("def ") || line.starts_with("async def ") || line.starts_with("class ") {
        let sig = line.trim_end_matches(':').trim();
        Some(sig.to_string())
    } else if line.starts_with("function ") || line.starts_with("export function ") {
        let sig = line.trim_end_matches('{').trim();
        Some(sig.to_string())
    } else {
        None
    }
}

/// Helper finding enclosing function/class for a given line number (1-indexed).
fn find_enclosing_scope(lines: &[&str], target_line: usize) -> Option<String> {
    if target_line == 0 || target_line > lines.len() {
        return None;
    }
    // Scan upward from target_line - 1
    for idx in (0..target_line).rev() {
        let trimmed = lines[idx].trim();
        if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") || trimmed.starts_with("pub async fn ") || trimmed.starts_with("async fn ") {
            let name = trimmed.split('(').next()?.trim();
            let fn_name = name.split_whitespace().last()?;
            return Some(format!("fn {}()", fn_name));
        } else if trimmed.starts_with("def ") || trimmed.starts_with("async def ") {
            let name = trimmed.split('(').next()?.trim();
            let fn_name = name.split_whitespace().last()?;
            return Some(format!("def {}()", fn_name));
        } else if trimmed.starts_with("class ") {
            let name = trimmed.split('(').next()?.split(':').next()?.trim();
            return Some(name.to_string());
        }
    }
    None
}

/// Helper extracting function or symbol body (returns 1-indexed (start_line, end_line)).
fn extract_symbol(lines: &[&str], symbol_name: &str) -> Option<(usize, usize)> {
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Check if line matches symbol definition
        let is_match = (trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") || trimmed.starts_with("pub async fn ") || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub struct ") || trimmed.starts_with("struct ")
            || trimmed.starts_with("pub enum ") || trimmed.starts_with("enum ")
            || trimmed.starts_with("def ") || trimmed.starts_with("async def ")
            || trimmed.starts_with("class ") || trimmed.starts_with("function ") || trimmed.starts_with("export function "))
            && trimmed.contains(symbol_name);

        if is_match {
            let mut start_idx = idx;

            // Check if there are decorators above this line (Python decorators @...)
            while start_idx > 0 && lines[start_idx - 1].trim().starts_with('@') {
                start_idx -= 1;
            }

            // Detect ending boundary
            let is_python = trimmed.starts_with("def ") || trimmed.starts_with("async def ") || trimmed.starts_with("class ");
            let end_idx = if is_python {
                find_python_end(lines, idx)
            } else {
                find_braced_end(lines, idx)
            };

            return Some((start_idx + 1, end_idx + 1));
        }
    }
    None
}

/// Braced boundary scanner: matches opening { to closing } tracking depth, skipping comments and string literals.
fn find_braced_end(lines: &[&str], start_idx: usize) -> usize {
    let mut depth: isize = 0;
    let mut started = false;

    for (idx, line) in lines.iter().enumerate().skip(start_idx) {
        let mut in_str = false;
        let mut str_char = ' ';
        let mut prev_char = ' ';

        for c in line.chars() {
            if in_str {
                if c == str_char && prev_char != '\\' {
                    in_str = false;
                }
            } else if c == '"' || c == '\'' {
                in_str = true;
                str_char = c;
            } else if c == '/' && prev_char == '/' {
                // Line comment starts, ignore rest of line
                break;
            } else if c == '{' {
                depth += 1;
                started = true;
            } else if c == '}' {
                depth -= 1;
                if started && depth <= 0 {
                    return idx;
                }
            }
            prev_char = c;
        }

        if started && depth <= 0 {
            return idx;
        }
    }

    lines.len().saturating_sub(1)
}

/// Python boundary scanner: matches leading indentation of def/class.
fn find_python_end(lines: &[&str], def_idx: usize) -> usize {
    let base_indent = lines[def_idx].len() - lines[def_idx].trim_start().len();
    let mut last_content_idx = def_idx;

    for (idx, line) in lines.iter().enumerate().skip(def_idx + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        if indent <= base_indent {
            return last_content_idx;
        }
        last_content_idx = idx;
    }

    last_content_idx
}

fn format_system_time(time: SystemTime) -> Option<String> {
    let dur = SystemTime::now().duration_since(time).ok()?;
    let secs = dur.as_secs();
    if secs < 60 {
        Some("just now".to_string())
    } else if secs < 3600 {
        Some(format!("{} minutes ago", secs / 60))
    } else if secs < 86400 {
        Some(format!("{} hours ago", secs / 3600))
    } else {
        Some(format!("{} days ago", secs / 86400))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_braced_symbol_extraction() {
        let code = r#"
pub fn calculate_total(a: i32, b: i32) -> i32 {
    let sum = a + b;
    if sum > 10 {
        println!("large: {}", sum);
    }
    sum
}

pub fn other_fn() {}
"#;
        let lines: Vec<&str> = code.lines().collect();
        let (start, end) = extract_symbol(&lines, "calculate_total").expect("Should find calculate_total");
        assert_eq!(start, 2); // 1-indexed
        assert_eq!(end, 8);
    }

    #[test]
    fn test_python_symbol_extraction() {
        let code = r#"
@router.post("/auth")
@limiter.limit("5/min")
def verify_session(req):
    """Docstring here."""
    if not req:
        return False
    return True

def next_function():
    pass
"#;
        let lines: Vec<&str> = code.lines().collect();
        let (start, end) = extract_symbol(&lines, "verify_session").expect("Should find verify_session");
        assert_eq!(start, 2); // includes @router.post decorator
        assert_eq!(end, 8); // ends at return True
    }

    #[test]
    fn test_binary_extension_detection() {
        assert!(is_binary_extension(Path::new("image.png")));
        assert!(is_binary_extension(Path::new("app.wasm")));
        assert!(is_binary_extension(Path::new("db.sqlite")));
        assert!(!is_binary_extension(Path::new("main.rs")));
        assert!(!is_binary_extension(Path::new("script.py")));
    }

    #[test]
    fn test_read_file_deduplicates_full_reads() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_str().unwrap().to_string();
        std::fs::write(&path, "line 1\nline 2\nline 3\nline 4\n").unwrap();

        let tool = ReadFileTool::new();

        // Turn 1: Initial full read
        let res1 = tool.execute(json!({ "path": path })).unwrap();
        let s1 = res1.as_str().unwrap();
        assert!(s1.contains("line 1"));
        assert!(s1.contains("line 4"));

        // Turn 2: Duplicate full read attempt
        let res2 = tool.execute(json!({ "path": path })).unwrap();
        let s2 = res2.as_str().unwrap();
        assert!(s2.contains("ALREADY been read in full"));
    }

    #[test]
    fn test_read_file_overlap_trimming() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_str().unwrap().to_string();
        let content: String = (1..=50).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(&path, content).unwrap();

        let tool = ReadFileTool::new();

        // Turn 1: Read lines 1 to 20
        let res1 = tool.execute(json!({ "path": path, "start_line": 1, "end_line": 20 })).unwrap();
        let s1 = res1.as_str().unwrap();
        assert!(s1.contains("   1 | line 1"));
        assert!(s1.contains("  20 | line 20"));

        // Turn 2: Read lines 10 to 30 (lines 10..20 overlap with turn 1)
        let res2 = tool.execute(json!({ "path": path, "start_line": 10, "end_line": 30 })).unwrap();
        let s2 = res2.as_str().unwrap();
        // Should trim to lines 21..30 and display notice
        assert!(s2.contains("lines 10..20 were already provided in prior context"));
        assert!(s2.contains("  21 | line 21"));
        assert!(s2.contains("  30 | line 30"));
        assert!(!s2.contains("  15 | line 15")); // lines 10..20 omitted to save context

        // Turn 3: Read lines 5 to 15 (completely covered by prior reads)
        let res3 = tool.execute(json!({ "path": path, "start_line": 5, "end_line": 15 })).unwrap();
        let s3 = res3.as_str().unwrap();
        assert!(s3.contains("are ALREADY in your context"));
    }
}
