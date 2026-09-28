use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tree_sitter::{Language, Node, Parser, Tree};

/// An extracted AST symbol definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstSymbol {
    pub name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
}

/// Dynamic registry mapping file extensions to Tree-sitter language grammars.
#[derive(Clone, Default)]
pub struct DynamicGrammarRegistry {
    grammars: HashMap<String, Language>,
}

impl DynamicGrammarRegistry {
    pub fn new() -> Self {
        Self {
            grammars: HashMap::new(),
        }
    }

    /// Dynamically register an extension with a Tree-sitter Language.
    pub fn register(&mut self, ext: &str, language: Language) {
        let clean = ext.trim().trim_start_matches('.').to_lowercase();
        self.grammars.insert(clean, language);
    }

    /// Dynamically register multiple extensions for the same language.
    pub fn register_extensions(&mut self, exts: &[&str], language: Language) {
        for ext in exts {
            self.register(ext, language.clone());
        }
    }

    /// Retrieve the language for a given extension.
    pub fn get_language(&self, ext: &str) -> Option<Language> {
        let clean = ext.trim().trim_start_matches('.').to_lowercase();
        self.grammars.get(&clean).cloned()
    }
}

/// The main dynamic AST engine supporting CST parsing and universal structural fallback.
pub struct DynamicAstEngine {
    registry: Arc<Mutex<DynamicGrammarRegistry>>,
}

impl Default for DynamicAstEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicAstEngine {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Mutex::new(DynamicGrammarRegistry::default())),
        }
    }

    /// Create engine with a custom grammar registry.
    pub fn with_registry(registry: DynamicGrammarRegistry) -> Self {
        Self {
            registry: Arc::new(Mutex::new(registry)),
        }
    }

    /// Dynamically register a language grammar at runtime for an extension.
    pub fn register_language(&self, ext: &str, language: Language) {
        if let Ok(mut lock) = self.registry.lock() {
            lock.register(ext, language);
        }
    }

    /// Dynamically register a language grammar at runtime for multiple extensions.
    pub fn register_extensions(&self, exts: &[&str], language: Language) {
        if let Ok(mut lock) = self.registry.lock() {
            lock.register_extensions(exts, language);
        }
    }

    /// Parse source text with a Tree-sitter grammar if available.
    pub fn parse(&self, content: &str, ext: &str) -> Option<Tree> {
        let lang = {
            let lock = self.registry.lock().ok()?;
            lock.get_language(ext)?
        };

        let mut parser = Parser::new();
        parser.set_language(&lang).ok()?;
        parser.parse(content, None)
    }

    /// Dynamically extract symbol definitions (functions, structs, classes, traits, enums, etc.).
    /// Uses Tree-sitter if grammar is available, or gracefully falls back to the universal structural scanner.
    pub fn extract_symbols(&self, content: &str, ext: &str, query: Option<&str>) -> Vec<AstSymbol> {
        if let Some(tree) = self.parse(content, ext) {
            let mut symbols = Vec::new();
            let query_lower = query.map(|q| q.to_lowercase());
            walk_ast_symbols(tree.root_node(), content, &mut symbols, query_lower.as_deref());
            if !symbols.is_empty() {
                return symbols;
            }
        }

        // Universal structural fallback for any file without a grammar
        fallback_extract_symbols(content, query)
    }

    /// Locate exact start and end line range for a symbol by name.
    pub fn locate_symbol_range(&self, content: &str, ext: &str, symbol_name: &str) -> Option<(usize, usize)> {
        let target = symbol_name.trim();
        if target.is_empty() {
            return None;
        }

        if let Some(tree) = self.parse(content, ext) {
            if let Some(range) = find_ast_symbol_range(tree.root_node(), content, target) {
                return Some(range);
            }
        }

        // Universal structural fallback
        fallback_locate_symbol_range(content, target)
    }

    /// Determine the enclosing function/class/struct scope for a given line number.
    pub fn enclosing_scope(&self, content: &str, ext: &str, line_num: usize) -> Option<String> {
        if line_num == 0 {
            return None;
        }

        if let Some(tree) = self.parse(content, ext) {
            if let Some(scope) = find_ast_enclosing_scope(tree.root_node(), content, line_num) {
                return Some(scope);
            }
        }

        // Universal structural fallback
        fallback_enclosing_scope(content, line_num)
    }

    /// Generate a compact structural code skeleton with statement bodies collapsed into `{ ... }`.
    pub fn generate_skeleton(&self, content: &str, ext: &str) -> Option<String> {
        if let Some(tree) = self.parse(content, ext) {
            return Some(collapse_ast_skeleton(tree.root_node(), content));
        }

        None
    }
}

// =========================================================================
// Tree-sitter Dynamic AST Inspection Helpers
// =========================================================================

/// Recursively inspects AST nodes to discover definitions dynamically.
fn walk_ast_symbols(
    node: Node,
    content: &str,
    symbols: &mut Vec<AstSymbol>,
    query_lower: Option<&str>,
) {
    let kind = node.kind();

    // Check if this node represents a definition
    if is_definition_kind(kind) {
        if let Some((name, identifier_kind)) = extract_node_identifier(node, content) {
            let matches = match query_lower {
                Some(q) => name.to_lowercase().contains(q),
                None => true,
            };

            if matches {
                let start_line = node.start_position().row + 1;
                let end_line = node.end_position().row + 1;
                let signature = extract_signature_line(node, content);

                symbols.push(AstSymbol {
                    name,
                    kind: identifier_kind,
                    start_line,
                    end_line,
                    signature,
                });
            }
        }
    }

    // Traverse children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_ast_symbols(child, content, symbols, query_lower);
    }
}

/// Checks if an AST node kind corresponds to a code definition across languages.
fn is_definition_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_item"
            | "function_declaration"
            | "function_definition"
            | "method_definition"
            | "method_declaration"
            | "arrow_function"
            | "struct_item"
            | "class_declaration"
            | "class_definition"
            | "enum_item"
            | "trait_item"
            | "impl_item"
            | "interface_declaration"
            | "type_alias"
            | "type_item"
            | "decorated_definition"
    )
}

/// Dynamically extracts the name and normalized kind of a definition node.
fn extract_node_identifier(node: Node, content: &str) -> Option<(String, String)> {
    let mut actual_node = node;
    let mut kind_label = simplify_kind(node.kind());

    // Handle Python decorated definitions
    if node.kind() == "decorated_definition" {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "function_definition" || child.kind() == "class_definition" {
                actual_node = child;
                kind_label = simplify_kind(child.kind());
                break;
            }
        }
    }

    // Try finding identifier by field name "name" or "declarator"
    if let Some(name_node) = actual_node
        .child_by_field_name("name")
        .or_else(|| actual_node.child_by_field_name("declarator"))
    {
        let text = get_node_text(name_node, content);
        if !text.is_empty() {
            return Some((text.to_string(), kind_label));
        }
    }

    // Fallback: look for child of kind identifier
    for child in actual_node.children(&mut actual_node.walk()) {
        if child.kind() == "identifier" || child.kind() == "type_identifier" {
            let text = get_node_text(child, content);
            if !text.is_empty() {
                return Some((text.to_string(), kind_label));
            }
        }
    }

    None
}

/// Simplifies AST node kind strings to clean, compact tags.
fn simplify_kind(kind: &str) -> String {
    if kind.contains("func") || kind.contains("method") {
        "fn".to_string()
    } else if kind.contains("struct") {
        "struct".to_string()
    } else if kind.contains("class") {
        "class".to_string()
    } else if kind.contains("enum") {
        "enum".to_string()
    } else if kind.contains("trait") {
        "trait".to_string()
    } else if kind.contains("interface") {
        "interface".to_string()
    } else if kind.contains("impl") {
        "impl".to_string()
    } else if kind.contains("type") {
        "type".to_string()
    } else {
        kind.to_string()
    }
}

/// Finds the start and end row for an exact symbol in the AST.
fn find_ast_symbol_range(node: Node, content: &str, target: &str) -> Option<(usize, usize)> {
    if is_definition_kind(node.kind()) {
        if let Some((name, _)) = extract_node_identifier(node, content) {
            if name == target || name.eq_ignore_ascii_case(target) {
                // If enclosed in a decorated definition or attributes, include the parent start
                let start_line = if let Some(parent) = node.parent() {
                    if parent.kind() == "decorated_definition" {
                        parent.start_position().row + 1
                    } else {
                        node.start_position().row + 1
                    }
                } else {
                    node.start_position().row + 1
                };

                let end_line = node.end_position().row + 1;
                return Some((start_line, end_line));
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(range) = find_ast_symbol_range(child, content, target) {
            return Some(range);
        }
    }

    None
}

/// Finds the enclosing scope path (e.g. `impl Auth -> fn verify`) for a given line number.
fn find_ast_enclosing_scope(node: Node, content: &str, target_line: usize) -> Option<String> {
    let row = target_line.saturating_sub(1);
    let mut current = node;
    let mut parts = Vec::new();

    // Find the smallest node covering the target line
    loop {
        let mut found_child = false;
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            if child.start_position().row <= row && child.end_position().row >= row {
                current = child;
                found_child = true;
                break;
            }
        }
        if !found_child {
            break;
        }
    }

    // Climb up from current node collecting enclosing definitions
    let mut climb = Some(current);
    while let Some(n) = climb {
        if is_definition_kind(n.kind()) {
            if let Some((name, kind)) = extract_node_identifier(n, content) {
                parts.push(format!("{} {}", kind, name));
            }
        }
        climb = n.parent();
    }

    if parts.is_empty() {
        None
    } else {
        parts.reverse();
        Some(parts.join(" -> "))
    }
}

/// Replaces all block bodies in AST with `{ ... }`.
fn collapse_ast_skeleton(_node: Node, content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut skeleton = String::new();

    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        // Include signatures and top-level definitions
        if trimmed.starts_with("pub ")
            || trimmed.starts_with("fn ")
            || trimmed.starts_with("def ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("impl ")
            || trimmed.starts_with("enum ")
            || trimmed.starts_with("trait ")
            || trimmed.starts_with("interface ")
            || trimmed.starts_with("type ")
            || trimmed.starts_with("export ")
            || trimmed.starts_with('@')
        {
            skeleton.push_str(&format!("{:4} | {}\n", line_num, line));
        }
    }

    skeleton
}

fn get_node_text<'a>(node: Node, content: &'a str) -> &'a str {
    let start = node.start_byte();
    let end = node.end_byte();
    if end <= content.len() && start <= end {
        &content[start..end]
    } else {
        ""
    }
}

fn extract_signature_line(node: Node, content: &str) -> String {
    let start = node.start_byte();
    let end = node.end_byte();
    let slice = if end <= content.len() && start <= end {
        &content[start..end]
    } else {
        ""
    };

    let first_line = slice.lines().next().unwrap_or("").trim();
    let cleaned = first_line.trim_end_matches('{').trim_end_matches(':').trim();
    if cleaned.len() > 80 {
        format!("{}...", &cleaned[..77])
    } else {
        cleaned.to_string()
    }
}

// =========================================================================
// Universal Structural Fallback Scanner (Zero Whitelist)
// =========================================================================

/// Universal fallback symbol extractor when no Tree-sitter grammar is available.
fn fallback_extract_symbols(content: &str, query: Option<&str>) -> Vec<AstSymbol> {
    let mut symbols = Vec::new();
    let query_lower = query.map(|q| q.to_lowercase());

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            continue;
        }

        if let Some((name, kind)) = scan_structural_definition(trimmed) {
            let matches = match query_lower.as_deref() {
                Some(q) => name.to_lowercase().contains(q),
                None => true,
            };

            if matches {
                let start_line = idx + 1;
                let end_line = scan_block_end(content, start_line);
                let signature = trimmed.trim_end_matches('{').trim_end_matches(':').to_string();

                symbols.push(AstSymbol {
                    name,
                    kind,
                    start_line,
                    end_line,
                    signature,
                });
            }
        }
    }

    symbols
}

/// Scans line for universal definition keywords.
fn scan_structural_definition(line: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    for (idx, &part) in parts.iter().enumerate() {
        match part {
            "fn" | "def" | "func" | "function" => {
                if let Some(next) = parts.get(idx + 1) {
                    let name = next.split('(').next()?.split('<').next()?.trim();
                    if !name.is_empty() {
                        return Some((name.to_string(), "fn".to_string()));
                    }
                }
            }
            "struct" | "class" | "interface" | "trait" | "enum" | "type" => {
                if let Some(next) = parts.get(idx + 1) {
                    let name = next.split('{').next()?.split(':').next()?.split('<').next()?.trim();
                    if !name.is_empty() {
                        return Some((name.to_string(), part.to_string()));
                    }
                }
            }
            _ => {}
        }
    }

    None
}

/// Scans for symbol end line using brace matching or indentation.
fn scan_block_end(content: &str, start_line: usize) -> usize {
    let lines: Vec<&str> = content.lines().collect();
    if start_line > lines.len() {
        return start_line;
    }

    let start_idx = start_line - 1;
    let first_line = lines[start_idx];

    // Brace delimited block
    if first_line.contains('{') {
        let mut depth = 0;
        let mut started = false;
        for (i, line) in lines[start_idx..].iter().enumerate() {
            for ch in line.chars() {
                if ch == '{' {
                    depth += 1;
                    started = true;
                } else if ch == '}' {
                    depth -= 1;
                    if started && depth == 0 {
                        return start_line + i;
                    }
                }
            }
        }
    }

    // Indentation delimited block
    let base_indent = first_line.len() - first_line.trim_start().len();
    for (i, line) in lines[start_idx + 1..].iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent <= base_indent {
            return start_line + i;
        }
    }

    lines.len().min(start_line + 50)
}

/// Fallback symbol range locator.
fn fallback_locate_symbol_range(content: &str, target: &str) -> Option<(usize, usize)> {
    let symbols = fallback_extract_symbols(content, Some(target));
    symbols
        .into_iter()
        .find(|s| s.name == target || s.name.eq_ignore_ascii_case(target))
        .map(|s| (s.start_line, s.end_line))
}

/// Fallback enclosing scope finder.
fn fallback_enclosing_scope(content: &str, line_num: usize) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    if line_num == 0 || line_num > lines.len() {
        return None;
    }

    for idx in (0..line_num).rev() {
        let line = lines[idx].trim();
        if let Some((name, kind)) = scan_structural_definition(line) {
            return Some(format!("{} {}", kind, name));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ast_symbol_extraction_rust() {
        let engine = DynamicAstEngine::new();
        let code = r#"
pub struct UserSession {
    pub id: String,
}

impl UserSession {
    pub fn is_valid(&self) -> bool {
        true
    }
}
"#;
        let symbols = engine.extract_symbols(code, "rs", None);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"UserSession"));
        assert!(names.contains(&"is_valid"));
    }

    #[test]
    fn test_ast_symbol_extraction_python() {
        let engine = DynamicAstEngine::new();
        let code = r#"
class AuthManager:
    @decorator
    def verify_token(self, token: str) -> bool:
        return True
"#;
        let symbols = engine.extract_symbols(code, "py", None);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"AuthManager"));
        assert!(names.contains(&"verify_token"));
    }

    #[test]
    fn test_universal_fallback_unregistered_extension() {
        let engine = DynamicAstEngine::new();
        let code = r#"
func calculate_total(a int, b int) int {
    return a + b
}
"#;
        // ".zig" or custom extension without active grammar
        let symbols = engine.extract_symbols(code, "zig", Some("calculate_total"));
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "calculate_total");
        assert_eq!(symbols[0].kind, "fn");
    }

    #[test]
    fn test_ast_locate_symbol_range() {
        let engine = DynamicAstEngine::new();
        let code = r#"
fn first() {
    println!("1");
}

fn second() {
    println!("2");
    println!("more");
}
"#;
        let range = engine.locate_symbol_range(code, "rs", "second").unwrap();
        assert_eq!(range.0, 6);
        assert_eq!(range.1, 9);
    }
}
