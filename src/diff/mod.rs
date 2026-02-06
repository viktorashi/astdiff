use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use anyhow::Result;
use tree_sitter::{Node, Tree};
use serde::{Serialize, Deserialize};

pub mod fingerprint;
pub mod matching_report;
pub mod threshold_learning;
pub mod parallel_matching;
pub mod parallel_matching_v2;
pub mod profiling;

use fingerprint::*;
use matching_report::*;

/// Represents a structural diff between two JavaScript ASTs
pub struct StructuralDiff {
    mappings1: Option<HashMap<String, String>>,
    mappings2: Option<HashMap<String, String>>,
    use_fingerprints: bool,
    generate_report: bool,
    report_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Declaration {
    pub name: String,
    pub kind: DeclarationKind,
    pub node: Node<'static>,
    pub line: usize,
    pub signature: String,
    pub structural_hashes: HashSet<u64>,
    pub size: usize,
    pub minhash_signature: Vec<u64>,
    pub fingerprint: Option<FunctionFingerprint>,
}

// Serializable version without the node
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SerializableDeclaration {
    pub name: String,
    pub kind: DeclarationKind,
    pub line: usize,
    pub signature: String,
    pub structural_hashes: HashSet<u64>,
    pub size: usize,
    pub minhash_signature: Vec<u64>,
    pub fingerprint: Option<FunctionFingerprint>,
}

impl From<&Declaration> for SerializableDeclaration {
    fn from(decl: &Declaration) -> Self {
        SerializableDeclaration {
            name: decl.name.clone(),
            kind: decl.kind.clone(),
            line: decl.line,
            signature: decl.signature.clone(),
            structural_hashes: decl.structural_hashes.clone(),
            size: decl.size,
            minhash_signature: decl.minhash_signature.clone(),
            fingerprint: decl.fingerprint.clone(),
        }
    }
}

impl SerializableDeclaration {
    pub fn to_declaration(self) -> Declaration {
        Declaration {
            name: self.name,
            kind: self.kind,
            node: unsafe { std::mem::zeroed() }, // Placeholder node
            line: self.line,
            signature: self.signature,
            structural_hashes: self.structural_hashes,
            size: self.size,
            minhash_signature: self.minhash_signature,
            fingerprint: self.fingerprint,
        }
    }
}

// Thread-safe declaration data for parallel processing
#[derive(Debug, Clone)]
pub struct DeclarationData {
    name: String,
    kind: DeclarationKind,
    line: usize,
    end_line: usize,
    signature: String,
    structural_hashes: HashSet<u64>,
    size: usize,
    minhash_signature: Vec<u64>,
    fingerprint: Option<FunctionFingerprint>,
}

impl Declaration {
    fn to_data(&self) -> DeclarationData {
        DeclarationData {
            name: self.name.clone(),
            kind: self.kind.clone(),
            line: self.line,
            end_line: self.node.end_position().row + 1,
            signature: self.signature.clone(),
            structural_hashes: self.structural_hashes.clone(),
            size: self.size,
            minhash_signature: self.minhash_signature.clone(),
            fingerprint: self.fingerprint.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DeclarationKind {
    Function,
    Variable,
    Class,
    Import,
    Export,
}

impl std::fmt::Display for DeclarationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeclarationKind::Function => write!(f, "function"),
            DeclarationKind::Variable => write!(f, "variable"),
            DeclarationKind::Class => write!(f, "class"),
            DeclarationKind::Import => write!(f, "import"),
            DeclarationKind::Export => write!(f, "export"),
        }
    }
}

/// Classification of a matched declaration pair based on normalized diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffClassification {
    /// Empty normalized diff — pure rename or identical
    Unchanged,
    /// Only string literal values changed (code skeleton identical)
    StringOnly,
    /// Code logic changed (structural differences beyond strings)
    Structural,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    pub identical: bool,
    pub similarity: f64,
    pub changes: Vec<Change>,
    pub matched_declarations: usize,
    pub total_declarations1: usize,
    pub total_declarations2: usize,
    /// Rename map: new_name → old_name (file2 → file1) for normalizing source2 references
    #[serde(skip)]
    pub rename_map: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub change_type: ChangeType,
    pub location1: Option<Location>,
    pub location2: Option<Location>,
    pub description: String,
    pub structural_path: String,
    /// String constant changes for modified functions (text diffs like system prompts)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_diff: Option<StringDiff>,
    /// Classification derived from normalized diff (None for Add/Delete)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification: Option<DiffClassification>,
    /// The display diff (original text, normalized comparison). Empty if Unchanged.
    #[serde(skip)]
    pub display_diff: String,
    /// Similarity score for matched pairs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChangeType {
    Addition,
    Deletion,
    Modification,
    Reorder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub line: usize,
    pub column: usize,
    pub code_snippet: String,
    pub end_line: Option<usize>,  // Optional end line for line ranges
}

/// Extract source text by line range. O(1) with pre-built line vector.
/// Lines are 1-indexed (matching tree-sitter convention).
pub fn extract_source_range(lines: &[&str], start_line: usize, end_line: usize) -> String {
    if start_line == 0 || start_line > lines.len() {
        return String::new();
    }
    let start = start_line - 1; // Convert to 0-indexed
    let end = end_line.min(lines.len());
    if start >= end {
        return String::new();
    }
    lines[start..end].join("\n")
}

impl StructuralDiff {
    pub fn new() -> Self {
        Self {
            mappings1: None,
            mappings2: None,
            use_fingerprints: true,  // Default to true for better accuracy
            generate_report: false,
            report_path: None,
        }
    }
    
    pub fn extract_declarations_for_inspection<'a>(&self, root: Node<'a>, source: &str) -> Vec<Declaration> {
        self.extract_declarations(root, source)
    }
    
    
    pub fn set_use_fingerprints(&mut self, use_fingerprints: bool) {
        self.use_fingerprints = use_fingerprints;
    }
    
    pub fn set_generate_report(&mut self, generate_report: bool) {
        self.generate_report = generate_report;
    }
    
    pub fn set_report_path(&mut self, path: std::path::PathBuf) {
        self.report_path = Some(path.to_string_lossy().to_string());
        self.generate_report = true;  // Automatically enable report if path is set
    }

    /// Format string diff for display, highlighting important changes
    fn format_string_diff(&self, diff: &StringDiff) -> String {
        let mut output = String::new();

        // Show important changes first (strings > 100 chars, like system prompts)
        if !diff.important_changes.is_empty() {
            output.push_str("    [IMPORTANT] Long text changes:\n");
            for change in &diff.important_changes {
                match change {
                    StringChange::Added(s) => {
                        output.push_str(&format!("      + ADDED ({} chars): \"{}\"\n",
                            s.value.len(),
                            Self::truncate_with_ellipsis(&s.value, 120)));
                    }
                    StringChange::Removed(s) => {
                        output.push_str(&format!("      - REMOVED ({} chars): \"{}\"\n",
                            s.value.len(),
                            Self::truncate_with_ellipsis(&s.value, 120)));
                    }
                    StringChange::Modified { old, new, similarity } => {
                        output.push_str(&format!("      ~ MODIFIED ({:.0}% similar, {} -> {} chars):\n",
                            similarity * 100.0, old.value.len(), new.value.len()));
                        output.push_str(&format!("        - \"{}\"\n",
                            Self::truncate_with_ellipsis(&old.value, 100)));
                        output.push_str(&format!("        + \"{}\"\n",
                            Self::truncate_with_ellipsis(&new.value, 100)));
                    }
                }
            }
        }

        // Summary of other (shorter) string changes
        let other_added = diff.added_count - diff.important_changes.iter()
            .filter(|c| matches!(c, StringChange::Added(_))).count();
        let other_removed = diff.removed_count - diff.important_changes.iter()
            .filter(|c| matches!(c, StringChange::Removed(_))).count();
        let other_modified = diff.modified_count - diff.important_changes.iter()
            .filter(|c| matches!(c, StringChange::Modified { .. })).count();

        if other_added > 0 || other_removed > 0 || other_modified > 0 {
            output.push_str(&format!("    Other string changes: +{} added, -{} removed, ~{} modified\n",
                other_added, other_removed, other_modified));
        }

        output
    }

    fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
        // Replace newlines with visible markers
        let cleaned: String = s.chars()
            .map(|c| if c == '\n' { '↵' } else { c })
            .collect();

        if cleaned.len() <= max_len {
            cleaned
        } else {
            // Find a valid char boundary at or before max_len
            let mut end = max_len;
            while end > 0 && !cleaned.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...", &cleaned[..end])
        }
    }

    /// Normalize source text by replacing renamed identifiers with their old names.
    /// Uses scan-and-lookup: extracts identifiers from the source and looks each up in the map.
    #[allow(dead_code)]
    fn normalize_source_with_renames(source: &str, rename_map: &HashMap<String, String>) -> String {
        if rename_map.is_empty() {
            return source.to_string();
        }
        fingerprint::normalize_string_with_renames(source, rename_map)
    }

    fn calculate_line_statistics(&self, result: &DiffResult, _source1: &str, _source2: &str) -> (usize, usize, usize) {
        let mut declarations_added = 0;
        let mut declarations_removed = 0;
        let mut declarations_modified = 0;

        for change in &result.changes {
            match change.change_type {
                ChangeType::Addition => declarations_added += 1,
                ChangeType::Deletion => declarations_removed += 1,
                ChangeType::Modification => {
                    match change.classification.as_ref() {
                        Some(DiffClassification::Structural) | Some(DiffClassification::StringOnly) => {
                            declarations_modified += 1;
                        }
                        _ => {}
                    }
                }
                ChangeType::Reorder => {}
            }
        }

        (declarations_added, declarations_removed, declarations_added + declarations_removed + declarations_modified)
    }
    
    
    pub fn set_mappings1(&mut self, mappings: HashMap<String, String>) {
        self.mappings1 = Some(mappings);
    }
    
    pub fn set_mappings2(&mut self, mappings: HashMap<String, String>) {
        self.mappings2 = Some(mappings);
    }
    
    
    pub fn compare(&self, source1: &str, source2: &str, 
                  tree1: &Tree, tree2: &Tree,
                  dump: Option<&std::path::Path>,
                  file1_path: &std::path::Path, file2_path: &std::path::Path) -> Result<DiffResult> {
        use profiling::Timer;
        use crate::dump::{AstDiffDump, DiffConfig};
        
        // Extract declarations
        let declarations1 = {
            let _timer = Timer::new("extract_declarations_file1");
            self.extract_declarations(tree1.root_node(), source1)
        };
        
        let declarations2 = {
            let _timer = Timer::new("extract_declarations_file2");
            self.extract_declarations(tree2.root_node(), source2)
        };
        
        // Store declarations for dump
        let declarations1_clone = declarations1.clone();
        let declarations2_clone = declarations2.clone();
        
        // Compare declarations
        let result = self.compare_declarations(declarations1, declarations2, source1, source2)?;
        
        // Dump if requested
        if let Some(dump_path) = dump {
            eprintln!("Creating comprehensive dump at {}", dump_path.display());
                
                // Convert declarations to serializable format
                let decls1: Vec<SerializableDeclaration> = declarations1_clone.iter().map(|d| d.into()).collect();
                let decls2: Vec<SerializableDeclaration> = declarations2_clone.iter().map(|d| d.into()).collect();
                
                // Get matches from the result
                let matches = self.get_matches_from_result(&declarations1_clone, &declarations2_clone, &result);
                
                // Create config
                let config = DiffConfig {
                    use_fingerprints: self.use_fingerprints,
                    parallel_matching: true, // Assume we're using parallel matching
                    threshold: 0.5, // Default threshold
                };
                
                // Use the provided file paths
                
                let dump = AstDiffDump::new(
                    file1_path.to_path_buf(),
                    file2_path.to_path_buf(),
                    decls1,
                    decls2,
                    matches,
                    result.clone(),
                    config,
                )?;
                
                dump.save(dump_path)?;
        }
        
        Ok(result)
    }
    
    pub fn compare_declarations(&self, declarations1: Vec<Declaration>, declarations2: Vec<Declaration>,
                              source1: &str, source2: &str) -> Result<DiffResult> {
        use profiling::Timer;

        eprintln!("Extracted {} declarations from file1, {} from file2",
                 declarations1.len(), declarations2.len());

        // Match declarations — now returns rename map and pre-classified changes
        let (matches, changes, rename_map) = {
            let _timer = Timer::new("match_declarations_total");
            self.match_declarations(&declarations1, &declarations2, source1, source2)
        };

        let matched_declarations = matches.len();
        let total_declarations1 = declarations1.len();
        let total_declarations2 = declarations2.len();

        let similarity = if total_declarations1 == 0 && total_declarations2 == 0 {
            1.0
        } else {
            matched_declarations as f64 / total_declarations1.max(total_declarations2) as f64
        };

        Ok(DiffResult {
            identical: changes.is_empty(),
            similarity,
            changes,
            matched_declarations,
            total_declarations1,
            total_declarations2,
            rename_map,
        })
    }

    fn extract_declarations<'a>(&self, root: Node<'a>, source: &str) -> Vec<Declaration> {
        let mut declarations = Vec::new();
        self.extract_declarations_recursive(root, source, &mut declarations, true);
        declarations
    }
    
    fn create_declaration(&self, name: String, kind: DeclarationKind, node: Node<'static>, 
                         line: usize, signature: String, structural_hashes: HashSet<u64>, 
                         source: &str) -> Declaration {
        let size = structural_hashes.len();
        let minhash_signature = self.compute_minhash(&structural_hashes, 128);
        
        // Extract fingerprint for string diffing (always) and matching (when enabled)
        // We always extract fingerprints so we can detect string content changes
        // even when fingerprint-based matching is disabled
        let fingerprint = if matches!(kind, DeclarationKind::Function | DeclarationKind::Variable) {
            let _timer = profiling::Timer::new("extract_fingerprint");
            let extractor = FingerprintExtractor::new(source);
            let fp = extractor.extract_function_fingerprint(node);

            // Debug fingerprints
            if std::env::var("ASTDIFF_DEBUG").is_ok() && !fp.strings.is_empty() {
                eprintln!("Fingerprint for {} '{}': {} strings, {} constants, {} API calls",
                    match kind {
                        DeclarationKind::Function => "function",
                        DeclarationKind::Variable => "variable",
                        _ => "other",
                    },
                    name, fp.strings.len(), fp.constants.len(), fp.api_calls.len());
                for s in &fp.strings {
                    eprintln!("  String: '{}' ({:?})", s.value, s.context);
                }
            }

            Some(fp)
        } else {
            None
        };
        
        Declaration {
            name,
            kind,
            node,
            line,
            signature,
            structural_hashes,
            size,
            minhash_signature,
            fingerprint,
        }
    }
    
    fn extract_declarations_recursive<'a>(&self, node: Node<'a>, source: &str, declarations: &mut Vec<Declaration>, is_global: bool) {
        match node.kind() {
            "function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &source[name_node.byte_range()];
                    let signature = self.get_function_signature(node, source);
                    let structural_hashes = self.collect_structural_hashes(node, source);
                    declarations.push(self.create_declaration(
                        name.to_string(),
                        DeclarationKind::Function,
                        unsafe { std::mem::transmute(node) },
                        node.start_position().row + 1,
                        signature,
                        structural_hashes,
                        source,
                    ));
                }
            }
            "variable_declaration" if is_global => {
                // Extract all variable declarators
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "variable_declarator" {
                        // Skip variables without initialization - they don't provide meaningful information
                        if child.child_by_field_name("value").is_none() {
                            continue;
                        }
                        
                        if let Some(name_node) = child.child_by_field_name("name") {
                            if name_node.kind() == "identifier" {
                                let name = &source[name_node.byte_range()];
                                let signature = self.get_variable_signature(child, source);
                                let structural_hashes = if let Some(value_node) = child.child_by_field_name("value") {
                                    self.collect_structural_hashes(value_node, source)
                                } else {
                                    HashSet::new()
                                };
                                declarations.push(self.create_declaration(
                                    name.to_string(),
                                    DeclarationKind::Variable,
                                    unsafe { std::mem::transmute(child) },
                                    child.start_position().row + 1,
                                    signature,
                                    structural_hashes,
                                    source,
                                ));
                            }
                        }
                    }
                }
            }
            "class_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &source[name_node.byte_range()];
                    let signature = self.get_class_signature(node, source);
                    let structural_hashes = self.collect_structural_hashes(node, source);
                    declarations.push(self.create_declaration(
                        name.to_string(),
                        DeclarationKind::Class,
                        unsafe { std::mem::transmute(node) },
                        node.start_position().row + 1,
                        signature,
                        structural_hashes,
                        source,
                    ));
                }
            }
            "import_statement" => {
                let signature = self.get_import_signature(node, source);
                let structural_hashes = self.collect_structural_hashes(node, source);
                declarations.push(self.create_declaration(
                    format!("import@{}", node.start_position().row),
                    DeclarationKind::Import,
                    unsafe { std::mem::transmute(node) },
                    node.start_position().row + 1,
                    signature,
                    structural_hashes,
                    source,
                ));
            }
            "export_statement" => {
                if let Some(decl) = node.child_by_field_name("declaration") {
                    self.extract_declarations_recursive(decl, source, declarations, is_global);
                } else {
                    let signature = self.get_export_signature(node, source);
                    let structural_hashes = self.collect_structural_hashes(node, source);
                    declarations.push(self.create_declaration(
                        format!("export@{}", node.start_position().row),
                        DeclarationKind::Export,
                        unsafe { std::mem::transmute(node) },
                        node.start_position().row + 1,
                        signature,
                        structural_hashes,
                        source,
                    ));
                }
            }
            _ => {
                // Only look for global declarations at the top level
                if is_global && node == node.parent().map(|p| p.child(0)).flatten().unwrap_or(node) {
                    for child in node.children(&mut node.walk()) {
                        self.extract_declarations_recursive(child, source, declarations, 
                            child.kind() != "function_declaration" && 
                            child.kind() != "class_declaration");
                    }
                }
            }
        }
    }
    
    fn collect_structural_hashes(&self, node: Node, source: &str) -> HashSet<u64> {
        let mut hashes = HashSet::new();
        self.collect_structural_hashes_recursive(node, source, &mut hashes);
        hashes
    }
    
    fn collect_structural_hashes_recursive(&self, node: Node, source: &str, hashes: &mut HashSet<u64>) {
        // Compute hash for this node
        let hash = self.compute_structural_hash(node, source);
        hashes.insert(hash);
        
        // Recursively collect hashes from children
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if !matches!(child.kind(), "comment") {
                    self.collect_structural_hashes_recursive(child, source, hashes);
                }
                
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    
    fn compute_structural_hash(&self, node: Node, source: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        
        let mut hasher = DefaultHasher::new();
        
        // Hash node type
        node.kind().hash(&mut hasher);
        
        // For literals, include the value
        if self.is_literal(node) {
            source[node.byte_range()].hash(&mut hasher);
        } else if node.kind() == "identifier" {
            // For identifiers, just use a placeholder
            "<ID>".hash(&mut hasher);
        } else {
            // For other nodes, hash the child structure
            let mut cursor = node.walk();
            if cursor.goto_first_child() {
                let mut child_hashes = Vec::new();
                loop {
                    let child = cursor.node();
                    if !matches!(child.kind(), "comment" | ";" | "," | "(" | ")" | "{" | "}" | "[" | "]") {
                        child_hashes.push(self.compute_structural_hash(child, source));
                    }
                    
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
                // Sort child hashes for order-independent nodes
                if self.is_order_independent(node) {
                    child_hashes.sort();
                }
                for hash in child_hashes {
                    hash.hash(&mut hasher);
                }
            }
        }
        
        hasher.finish()
    }
    
    fn get_function_signature(&self, node: Node, _source: &str) -> String {
        let params = if let Some(params_node) = node.child_by_field_name("parameters") {
            let param_count = params_node.children(&mut params_node.walk())
                .filter(|n| n.kind() == "identifier" || n.kind() == "formal_parameters")
                .count();
            format!("params:{}", param_count)
        } else {
            "params:0".to_string()
        };
        
        let body = if let Some(body_node) = node.child_by_field_name("body") {
            let statement_count = body_node.children(&mut body_node.walk())
                .filter(|n| !matches!(n.kind(), "{" | "}" | ";"))
                .count();
            format!("stmts:{}", statement_count)
        } else {
            "stmts:0".to_string()
        };
        
        format!("function({},{})", params, body)
    }
    
    fn get_variable_signature(&self, node: Node, source: &str) -> String {
        if let Some(init) = node.child_by_field_name("value") {
            match init.kind() {
                "number" => format!("var=number:{}", &source[init.byte_range()]),
                "string" => format!("var=string:len{}", source[init.byte_range()].len()),
                "true" | "false" => format!("var=bool:{}", init.kind()),
                "array" => format!("var=array:len{}", init.children(&mut init.walk()).count()),
                "object" => format!("var=object:props{}", init.children(&mut init.walk())
                    .filter(|n| n.kind() == "pair").count()),
                "arrow_function" | "function" => {
                    let param_count = if let Some(params) = init.child_by_field_name("parameters") {
                        params.children(&mut params.walk()).count()
                    } else if init.child_by_field_name("parameter").is_some() {
                        1
                    } else {
                        0
                    };
                    format!("var=function:params{}", param_count)
                }
                _ => format!("var={}", init.kind()),
            }
        } else {
            "var=undefined".to_string()
        }
    }
    
    fn get_class_signature(&self, node: Node, _source: &str) -> String {
        if let Some(body) = node.child_by_field_name("body") {
            let method_count = body.children(&mut body.walk())
                .filter(|n| n.kind() == "method_definition")
                .count();
            let field_count = body.children(&mut body.walk())
                .filter(|n| n.kind() == "field_definition")
                .count();
            format!("class(methods:{},fields:{})", method_count, field_count)
        } else {
            "class()".to_string()
        }
    }
    
    fn get_import_signature(&self, node: Node, source: &str) -> String {
        let source_path = node.children(&mut node.walk())
            .find(|n| n.kind() == "string")
            .map(|n| &source[n.byte_range()])
            .unwrap_or("");
        format!("import from {}", source_path)
    }
    
    fn get_export_signature(&self, node: Node, _source: &str) -> String {
        if node.child_by_field_name("declaration").is_some() {
            "export declaration".to_string()
        } else if let Some(clause) = node.child_by_field_name("clause") {
            let export_count = clause.children(&mut clause.walk())
                .filter(|n| n.kind() == "export_specifier")
                .count();
            format!("export {} items", export_count)
        } else {
            "export".to_string()
        }
    }
    
    fn compute_minhash(&self, hashes: &HashSet<u64>, num_hashes: usize) -> Vec<u64> {
        let mut signature = vec![u64::MAX; num_hashes];
        
        for &hash in hashes {
            for i in 0..num_hashes {
                let hash_value = self.hash_with_seed_u64(hash, i);
                signature[i] = signature[i].min(hash_value);
            }
        }
        
        signature
    }
    
    fn hash_with_seed_u64(&self, value: u64, seed: usize) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        seed.hash(&mut hasher);
        hasher.finish()
    }
    
    
    pub fn calculate_declaration_similarity(&self, decl1: &Declaration, decl2: &Declaration, _source1: &str, _source2: &str) -> f64 {
        // For imports and exports, use signature similarity regardless of kind
        if matches!(decl1.kind, DeclarationKind::Import | DeclarationKind::Export) 
            || matches!(decl2.kind, DeclarationKind::Import | DeclarationKind::Export) {
            return if decl1.signature == decl2.signature { 1.0 } else { 0.3 };
        }
        
        // Calculate base similarity from structural hash intersection
        let intersection: HashSet<_> = decl1.structural_hashes.intersection(&decl2.structural_hashes).cloned().collect();
        let union: HashSet<_> = decl1.structural_hashes.union(&decl2.structural_hashes).cloned().collect();
        
        let base_similarity = if union.is_empty() {
            // Both are empty (e.g., simple variables with no initialization)
            if decl1.signature == decl2.signature { 
                1.0 
            } else {
                0.5
            }
        } else {
            let jaccard = intersection.len() as f64 / union.len() as f64;
            
            // Boost similarity for variables with the same initialization type
            if matches!(decl1.kind, DeclarationKind::Variable) && matches!(decl2.kind, DeclarationKind::Variable) {
                if decl1.signature.starts_with("var=") && decl2.signature.starts_with("var=") {
                    let type1 = decl1.signature.strip_prefix("var=").unwrap_or("");
                    let type2 = decl2.signature.strip_prefix("var=").unwrap_or("");
                    
                    // Debug specific cases
                    if (decl1.name == "QhB" || decl1.name == "EhB") && (decl2.name == "QhB" || decl2.name == "EhB") {
                        eprintln!("DEBUG MATCH: {} vs {} - sig1: {}, sig2: {}, jaccard: {:.3}", 
                            decl1.name, decl2.name, decl1.signature, decl2.signature, jaccard);
                    }
                    
                    // If they have the same type and low jaccard, boost similarity
                    if type1 == type2 && jaccard < 0.5 && !type1.is_empty() {
                        // Same type of initialization (e.g., both "member_expression")
                        jaccard + 0.3  // Boost by 0.3
                    } else {
                        jaccard
                    }
                } else {
                    jaccard
                }
            } else {
                jaccard
            }
        };
        
        // Apply a penalty for different kinds, but allow matching
        if decl1.kind != decl2.kind {
            // Function <-> Variable is common in minified code, apply small penalty
            if (matches!(decl1.kind, DeclarationKind::Function) && matches!(decl2.kind, DeclarationKind::Variable))
                || (matches!(decl1.kind, DeclarationKind::Variable) && matches!(decl2.kind, DeclarationKind::Function)) {
                base_similarity * 0.9  // 10% penalty
            } else {
                base_similarity * 0.7  // 30% penalty for other mismatches
            }
        } else {
            base_similarity
        }
    }
    
    
    
    fn is_literal(&self, node: Node) -> bool {
        matches!(node.kind(), 
            "string" | "number" | "true" | "false" | "null" | "undefined" | "regex" | "template_string"
        )
    }
    
    fn is_order_independent(&self, node: Node) -> bool {
        matches!(node.kind(), 
            "object" | "object_pattern" | "named_imports" | "export_clause"
        )
    }
    
    
        
    pub fn print_summary(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf,
                         source1: &str, source2: &str) {
        println!("--- {}", file1.display());
        println!("+++ {}", file2.display());
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!("Matched declarations: {}/{} vs {}",
                 result.matched_declarations, result.total_declarations1, result.total_declarations2);

        // Calculate and print line statistics
        let (lines_added, lines_removed, total_diff) = self.calculate_line_statistics(result, source1, source2);
        println!("Diff size: {} declarations (+{} added, -{} removed)", total_diff, lines_added, lines_removed);

        // Group changes by type using classification
        let mut additions = Vec::new();
        let mut deletions = Vec::new();
        let mut structural_changes = Vec::new();
        let mut string_changes = Vec::new();

        for change in &result.changes {
            match change.change_type {
                ChangeType::Addition => additions.push(change),
                ChangeType::Deletion => deletions.push(change),
                ChangeType::Modification => {
                    match change.classification.as_ref() {
                        Some(DiffClassification::Structural) => structural_changes.push(change),
                        Some(DiffClassification::StringOnly) => string_changes.push(change),
                        _ => {} // Unchanged — not shown
                    }
                }
                ChangeType::Reorder => {}
            }
        }

        let total_unchanged = result.matched_declarations
            .saturating_sub(structural_changes.len())
            .saturating_sub(string_changes.len());

        println!("Changes: {} added, {} removed, {} structural, {} string-only ({} unchanged)",
            additions.len(), deletions.len(), structural_changes.len(),
            string_changes.len(), total_unchanged);
        println!();

        // Show deletions
        if !deletions.is_empty() {
            println!("=== Removed ===");
            for change in &deletions {
                println!("--- {}", change.description);
                if let Some(loc) = &change.location1 {
                    println!("    at line {}: {}", loc.line, loc.code_snippet);
                }
            }
            println!();
        }

        // Show additions
        if !additions.is_empty() {
            println!("=== Added ===");
            for change in &additions {
                println!("+++ {}", change.description);
                if let Some(loc) = &change.location2 {
                    println!("    at line {}: {}", loc.line, loc.code_snippet);
                }
            }
            println!();
        }

        // Show structural changes
        if !structural_changes.is_empty() {
            println!("=== Structural Changes ===");
            for change in &structural_changes {
                println!("@@@ {}", change.description);
                if let Some(loc) = &change.location1 {
                    println!("  - at line {}: {}", loc.line, loc.code_snippet);
                }
                if let Some(loc) = &change.location2 {
                    println!("  + at line {}: {}", loc.line, loc.code_snippet);
                }
            }
            println!();
        }

        // Show string changes
        if !string_changes.is_empty() {
            println!("=== String Changes ===");
            for change in &string_changes {
                println!("@@@ {}", change.description);
                if let Some(loc) = &change.location1 {
                    println!("  - at line {}: {}", loc.line, loc.code_snippet);
                }
                if let Some(loc) = &change.location2 {
                    println!("  + at line {}: {}", loc.line, loc.code_snippet);
                }
            }
            println!();
        }
    }
    
    pub fn print_interleaved(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf, 
                             canonical1: Option<&str>, canonical2: Option<&str>, source1: &str, source2: &str) -> Result<()> {
        println!("--- {}", file1.display());
        println!("+++ {}", file2.display());
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!("Matched declarations: {}/{} vs {}", 
                 result.matched_declarations, result.total_declarations1, result.total_declarations2);
        
        // Calculate and print line statistics
        let (lines_added, lines_removed, total_diff) = self.calculate_line_statistics(result, source1, source2);
        println!("Diff size: {} declarations (+{} added, -{} removed)", total_diff, lines_added, lines_removed);
        
        // Group changes by type
        let mut additions = Vec::new();
        let mut deletions = Vec::new();
        let mut structural_changes = Vec::new();
        let mut renames = Vec::new();
        
        for change in &result.changes {
            match change.change_type {
                ChangeType::Addition => additions.push(change),
                ChangeType::Deletion => deletions.push(change),
                ChangeType::Modification => {
                    if change.description.contains("structure changed") {
                        structural_changes.push(change);
                    } else if change.description.contains("matched with") {
                        renames.push(change);
                    }
                }
                ChangeType::Reorder => {} // Ignore reorders
            }
        }
        
        let _meaningful_changes = additions.len() + deletions.len() + structural_changes.len();
        println!("Changes: {} additions, {} deletions, {} modifications (+ {} renames)", 
                 additions.len(), deletions.len(), structural_changes.len(), renames.len());
        println!();
        
        // Show deletions with their canonical form
        if !deletions.is_empty() {
            println!("=== Removed Functions ===");
            for change in deletions {
                println!("\n--- {}", change.description);
                if let Some(loc) = &change.location1 {
                    // Extract and show the canonical version of this function
                    if let (Some(can1), Some(line)) = (canonical1, Some(loc.line)) {
                        self.print_canonical_snippet(can1, line, "-");
                    }
                }
            }
            println!();
        }
        
        // Show additions
        if !additions.is_empty() {
            println!("=== Added Functions ===");
            for change in additions {
                println!("\n+++ {}", change.description);
                if let Some(loc) = &change.location2 {
                    if let (Some(can2), Some(line)) = (canonical2, Some(loc.line)) {
                        self.print_canonical_snippet(can2, line, "+");
                    }
                }
            }
            println!();
        }
        
        // Show structural changes with unified diff
        if !structural_changes.is_empty() {
            println!("=== Modified Functions ===");
            for change in structural_changes {
                println!("\n@@@ {}", change.description);

                // Show string diff if present
                if let Some(ref string_diff) = change.string_diff {
                    print!("{}", self.format_string_diff(string_diff));
                }

                // Show unified diff of the canonical versions
                if let (Some(loc1), Some(loc2), Some(can1), Some(can2)) =
                    (&change.location1, &change.location2, canonical1, canonical2) {

                    // Extract the function from both canonical versions
                    let func1 = self.extract_function_at_line(can1, loc1.line);
                    let func2 = self.extract_function_at_line(can2, loc2.line);

                    if let (Some(f1), Some(f2)) = (func1, func2) {
                        self.print_unified_diff(&f1, &f2);
                    }
                }
            }
            println!();
        }
        
        // Optionally show renames
        if !renames.is_empty() && std::env::var("ASTDIFF_SHOW_RENAMES").is_ok() {
            println!("=== Renamed Functions ===");
            for change in renames {
                if let Some(path) = change.structural_path.split("->").nth(1) {
                    println!("  {} -> {}", 
                        change.structural_path.split("->").next().unwrap_or("").replace("global.", ""),
                        path);
                }
            }
            println!();
        }
        
        Ok(())
    }
    
    fn print_canonical_snippet(&self, source: &str, start_line: usize, prefix: &str) {
        let lines: Vec<&str> = source.lines().collect();
        if start_line > 0 && start_line <= lines.len() {
            // Find the function boundaries
            let mut end_line = start_line;
            let mut brace_count = 0;
            let mut in_function = false;
            
            for (i, line) in lines.iter().enumerate().skip(start_line - 1) {
                if line.contains('{') {
                    brace_count += line.matches('{').count();
                    in_function = true;
                }
                if line.contains('}') {
                    brace_count -= line.matches('}').count();
                }
                if in_function && brace_count == 0 {
                    end_line = i + 1;
                    break;
                }
            }
            
            // Print the function
            for i in (start_line - 1)..=end_line.min(lines.len() - 1) {
                println!("{} {}", prefix, lines[i]);
            }
        }
    }
    
    fn extract_function_at_line(&self, source: &str, start_line: usize) -> Option<String> {
        let lines: Vec<&str> = source.lines().collect();
        if start_line == 0 || start_line > lines.len() {
            return None;
        }
        
        let mut result = Vec::new();
        let mut brace_count = 0;
        let mut in_function = false;
        
        for (_i, line) in lines.iter().enumerate().skip(start_line - 1) {
            result.push(line.to_string());
            
            if line.contains('{') {
                brace_count += line.matches('{').count();
                in_function = true;
            }
            if line.contains('}') {
                brace_count -= line.matches('}').count();
            }
            if in_function && brace_count == 0 {
                break;
            }
        }
        
        Some(result.join("\n"))
    }
    
    /// Generate a unified diff using normalized text for comparison but original text
    /// for display. This eliminates rename noise from the diff while keeping the output
    /// readable. The normalized texts must have the same line count as the originals
    /// (normalization only substitutes within lines, never adds/removes lines).
    pub fn generate_normalized_display_diff(
        orig1: &str, orig2: &str,
        norm1: &str, norm2: &str,
        context_lines: usize,
    ) -> String {
        use similar::{ChangeTag, TextDiff};

        let diff = TextDiff::from_lines(norm1, norm2);
        let orig_lines1: Vec<&str> = orig1.lines().collect();
        let orig_lines2: Vec<&str> = orig2.lines().collect();

        let mut output = String::new();
        let mut has_changes = false;

        for hunk in diff.unified_diff().context_radius(context_lines).iter_hunks() {
            has_changes = true;
            output.push_str(&format!("{}\n", hunk.header()));
            for change in hunk.iter_changes() {
                let sign = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                // Look up the original (non-normalized) line at the same index
                let orig_line = match change.tag() {
                    ChangeTag::Delete | ChangeTag::Equal => {
                        change.old_index()
                            .and_then(|i| orig_lines1.get(i))
                            .copied()
                            .unwrap_or("")
                    }
                    ChangeTag::Insert => {
                        change.new_index()
                            .and_then(|i| orig_lines2.get(i))
                            .copied()
                            .unwrap_or("")
                    }
                };
                output.push_str(sign);
                output.push_str(orig_line);
                if !orig_line.ends_with('\n') {
                    output.push('\n');
                }
            }
        }

        if has_changes { output } else { String::new() }
    }

    fn print_unified_diff(&self, text1: &str, text2: &str) {
        let lines1: Vec<&str> = text1.lines().collect();
        let lines2: Vec<&str> = text2.lines().collect();
        
        // Simple line-by-line diff
        let max_lines = lines1.len().max(lines2.len());
        let mut shown_context = false;
        
        for i in 0..max_lines {
            match (lines1.get(i), lines2.get(i)) {
                (Some(l1), Some(l2)) if l1 == l2 => {
                    if !shown_context {
                        println!("  {}", l1);
                    }
                }
                (Some(l1), Some(l2)) => {
                    shown_context = true;
                    println!("- {}", l1);
                    println!("+ {}", l2);
                }
                (Some(l1), None) => {
                    shown_context = true;
                    println!("- {}", l1);
                }
                (None, Some(l2)) => {
                    shown_context = true;
                    println!("+ {}", l2);
                }
                (None, None) => {}
            }
        }
    }
    
    pub fn print_side_by_side_full(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf,
                                   source1: &str, source2: &str) -> Result<()> {
        println!("--- {}", file1.display());
        println!("+++ {}", file2.display());
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!("Matched declarations: {}/{} vs {}", 
                 result.matched_declarations, result.total_declarations1, result.total_declarations2);
        
        // Calculate and print line statistics
        let (lines_added, lines_removed, total_diff) = self.calculate_line_statistics(result, source1, source2);
        println!("Diff size: {} declarations (+{} added, -{} removed)", total_diff, lines_added, lines_removed);
        
        // Group changes by type
        let mut additions = Vec::new();
        let mut deletions = Vec::new();
        let mut structural_changes = Vec::new();
        let mut renames = Vec::new();
        
        for change in &result.changes {
            match change.change_type {
                ChangeType::Addition => additions.push(change),
                ChangeType::Deletion => deletions.push(change),
                ChangeType::Modification => {
                    // Separate structural/text changes from simple renames
                    if change.description.contains("structure changed") || change.description.contains("text constant changes") {
                        structural_changes.push(change);
                    } else if change.description.contains("matched with") {
                        renames.push(change);
                    }
                }
                ChangeType::Reorder => {} // Ignore reorders
            }
        }

        println!("Changes: {} additions, {} deletions, {} modifications (+ {} renames)", 
                 additions.len(), deletions.len(), structural_changes.len(), renames.len());
        println!();
        
        // Show deletions
        if !deletions.is_empty() {
            println!("=== Removed Functions ===");
            for change in deletions {
                println!("\n--- {}", change.description);
                if let Some(loc) = &change.location1 {
                    println!("{}:{}", file1.display(), loc.line);
                    self.print_original_function(source1, loc.line, "- ");
                }
            }
            println!();
        }
        
        // Show additions
        if !additions.is_empty() {
            println!("=== Added Functions ===");
            for change in additions {
                println!("\n+++ {}", change.description);
                if let Some(loc) = &change.location2 {
                    println!("{}:{}", file2.display(), loc.line);
                    self.print_original_function(source2, loc.line, "+ ");
                }
            }
            println!();
        }
        
        // Show structural changes side by side
        if !structural_changes.is_empty() {
            println!("=== Modified Functions ===");
            for change in structural_changes {
                println!("\n@@@ {}", change.description);

                // Show string diff if present
                if let Some(ref string_diff) = change.string_diff {
                    print!("{}", self.format_string_diff(string_diff));
                }

                if let (Some(loc1), Some(loc2)) = (&change.location1, &change.location2) {
                    println!("\n--- {}:{} (before)", file1.display(), loc1.line);
                    self.print_original_function(source1, loc1.line, "- ");

                    println!("\n+++ {}:{} (after)", file2.display(), loc2.line);
                    self.print_original_function(source2, loc2.line, "+ ");
                }
            }
            println!();
        }
        
        // Optionally show renames
        if !renames.is_empty() && std::env::var("ASTDIFF_SHOW_RENAMES").is_ok() {
            println!("=== Renamed Functions ===");
            for change in renames {
                if let Some(path) = change.structural_path.split("->").nth(1) {
                    println!("  {} -> {}", 
                        change.structural_path.split("->").next().unwrap_or("").replace("global.", ""),
                        path);
                }
            }
            println!();
        }
        
        Ok(())
    }
    
    fn print_original_function(&self, source: &str, start_line: usize, prefix: &str) {
        let lines: Vec<&str> = source.lines().collect();
        if start_line > 0 && start_line <= lines.len() {
            // For simple variable declarations without initialization, just print the line
            let first_line = lines[start_line - 1];
            
            // Check if this is a simple variable declaration (contains var/let/const but no '=' or arrow function)
            let is_simple_var_decl = (first_line.trim_start().starts_with("var ") || 
                                      first_line.trim_start().starts_with("let ") || 
                                      first_line.trim_start().starts_with("const ")) &&
                                     !first_line.contains('=') && 
                                     !first_line.contains("=>");
            
            if is_simple_var_decl || first_line.trim().ends_with(',') || first_line.trim().ends_with(';') {
                // For simple variable declarations, just print the single line
                println!("{} {}", prefix, first_line);
                return;
            }
            
            // Find the function boundaries
            let mut end_line = start_line;
            let mut brace_count = 0;
            let mut in_function = false;
            
            for (i, line) in lines.iter().enumerate().skip(start_line - 1) {
                if line.contains('{') {
                    brace_count += line.matches('{').count();
                    in_function = true;
                }
                if line.contains('}') {
                    brace_count -= line.matches('}').count();
                }
                
                // Also check for single-line arrow functions
                let is_arrow = line.contains("=>") && !line.contains("{");
                
                if (in_function && brace_count == 0) || (i == start_line - 1 && is_arrow) {
                    end_line = i + 1;
                    if !is_arrow {
                        break;
                    }
                }
            }
            
            // Print the function without line numbers
            for i in (start_line - 1)..end_line.min(lines.len()) {
                println!("{} {}", prefix, lines[i]);
            }
        }
    }
    
    /// New default output: grouped by classification, using pre-computed display_diff.
    pub fn print_default(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf,
                         source1: &str, source2: &str) -> Result<()> {
        let file1_name = file1.file_name().unwrap_or(file1.as_os_str()).to_string_lossy();
        let file2_name = file2.file_name().unwrap_or(file2.as_os_str()).to_string_lossy();

        println!("--- {}", file1.display());
        println!("+++ {}", file2.display());
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!("Matched: {}/{} vs {}",
            result.matched_declarations, result.total_declarations1, result.total_declarations2);

        // Classify changes
        let mut additions = Vec::new();
        let mut deletions = Vec::new();
        let mut structural = Vec::new();
        let mut string_only = Vec::new();
        for change in &result.changes {
            match change.change_type {
                ChangeType::Addition => additions.push(change),
                ChangeType::Deletion => deletions.push(change),
                ChangeType::Modification => {
                    match change.classification.as_ref() {
                        Some(DiffClassification::Structural) => structural.push(change),
                        Some(DiffClassification::StringOnly) => string_only.push(change),
                        Some(DiffClassification::Unchanged) | None => {}
                    }
                }
                ChangeType::Reorder => {} // Implicit in location
            }
        }

        // Count unchanged as: matched - structural - string_only
        let total_unchanged = result.matched_declarations
            .saturating_sub(structural.len())
            .saturating_sub(string_only.len());

        println!("Changes: {} added, {} removed, {} structural, {} string-only ({} unchanged)",
            additions.len(), deletions.len(), structural.len(), string_only.len(), total_unchanged);
        println!();

        // === Removed ===
        if !deletions.is_empty() {
            println!("=== Removed ===");
            let lines1: Vec<&str> = source1.lines().collect();
            for change in &deletions {
                if let Some(loc) = &change.location1 {
                    let end = loc.end_line.unwrap_or(loc.line);
                    println!("\n--- Removed {} ({}:{}-{})",
                        Self::extract_name_from_desc(&change.description),
                        file1_name, loc.line, end);
                    let body = extract_source_range(&lines1, loc.line, end);
                    for line in body.lines() {
                        println!("- {}", line);
                    }
                }
            }
            println!();
        }

        // === Added ===
        if !additions.is_empty() {
            println!("=== Added ===");
            let lines2: Vec<&str> = source2.lines().collect();
            for change in &additions {
                if let Some(loc) = &change.location2 {
                    let end = loc.end_line.unwrap_or(loc.line);
                    println!("\n+++ Added {} ({}:{}-{})",
                        Self::extract_name_from_desc(&change.description),
                        file2_name, loc.line, end);
                    let body = extract_source_range(&lines2, loc.line, end);
                    for line in body.lines() {
                        println!("+ {}", line);
                    }
                }
            }
            println!();
        }

        // === Structural Changes ===
        if !structural.is_empty() {
            println!("=== Structural Changes ===");
            for change in &structural {
                if let (Some(loc1), Some(loc2)) = (&change.location1, &change.location2) {
                    println!("\n@@@ {}", change.description);
                    println!("--- {}:{}", file1_name, loc1.line);
                    println!("+++ {}:{}", file2_name, loc2.line);
                    if !change.display_diff.is_empty() {
                        print!("{}", change.display_diff);
                    }
                }
            }
            println!();
        }

        // === String Changes ===
        if !string_only.is_empty() {
            println!("=== String Changes ===");
            for change in &string_only {
                if let (Some(loc1), Some(loc2)) = (&change.location1, &change.location2) {
                    println!("\n@@@ {}", change.description);
                    println!("--- {}:{}", file1_name, loc1.line);
                    println!("+++ {}:{}", file2_name, loc2.line);
                    if !change.display_diff.is_empty() {
                        print!("{}", change.display_diff);
                    }
                }
            }
            println!();
        }

        Ok(())
    }

    /// Compact output: location-only summary grouped by classification.
    pub fn print_compact_locations(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf) {
        let file1_name = file1.file_name().unwrap_or(file1.as_os_str()).to_string_lossy();
        let file2_name = file2.file_name().unwrap_or(file2.as_os_str()).to_string_lossy();

        // Classify changes
        let mut additions = Vec::new();
        let mut deletions = Vec::new();
        let mut structural = Vec::new();
        let mut string_only = Vec::new();

        for change in &result.changes {
            match change.change_type {
                ChangeType::Addition => additions.push(change),
                ChangeType::Deletion => deletions.push(change),
                ChangeType::Modification => {
                    match change.classification.as_ref() {
                        Some(DiffClassification::Structural) => structural.push(change),
                        Some(DiffClassification::StringOnly) => string_only.push(change),
                        _ => {} // Unchanged — not shown
                    }
                }
                ChangeType::Reorder => {}
            }
        }

        let total_unchanged = result.matched_declarations
            .saturating_sub(structural.len())
            .saturating_sub(string_only.len());

        // Removed
        if !deletions.is_empty() {
            println!("Removed: {}", deletions.len());
            for change in &deletions {
                if let Some(loc) = &change.location1 {
                    let end = loc.end_line.unwrap_or(loc.line);
                    let name = Self::extract_name_from_desc(&change.description);
                    println!("  {} ({}:{}-{})", name, file1_name, loc.line, end);
                }
            }
            println!();
        }

        // Added
        if !additions.is_empty() {
            println!("Added: {}", additions.len());
            for change in &additions {
                if let Some(loc) = &change.location2 {
                    let end = loc.end_line.unwrap_or(loc.line);
                    let name = Self::extract_name_from_desc(&change.description);
                    println!("  {} ({}:{}-{})", name, file2_name, loc.line, end);
                }
            }
            println!();
        }

        // Structural
        if !structural.is_empty() {
            println!("Structural: {}", structural.len());
            for change in &structural {
                if let (Some(loc1), Some(loc2)) = (&change.location1, &change.location2) {
                    let name = Self::extract_name_from_desc(&change.description);
                    let sim = change.similarity_score.map(|s| format!(" {:.1}%", s * 100.0)).unwrap_or_default();
                    println!("  {} ({}:{} -> {}:{}){}",
                        name, file1_name, loc1.line, file2_name, loc2.line, sim);
                }
            }
            println!();
        }

        // String-only
        if !string_only.is_empty() {
            println!("String-only: {}", string_only.len());
            for change in &string_only {
                if let (Some(loc1), Some(loc2)) = (&change.location1, &change.location2) {
                    let name = Self::extract_name_from_desc(&change.description);
                    println!("  {} ({}:{} -> {}:{})",
                        name, file1_name, loc1.line, file2_name, loc2.line);
                }
            }
            println!();
        }

        println!("Unchanged: {} (not shown)", total_unchanged);
    }

    /// Extract declaration name from a description string.
    fn extract_name_from_desc(desc: &str) -> &str {
        // Try patterns like "function 'foo' ..." or "Removed function 'foo'"
        if let Some(start) = desc.find('\'') {
            if let Some(end) = desc[start+1..].find('\'') {
                return &desc[start+1..start+1+end];
            }
        }
        desc
    }

    pub fn print_side_by_side(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf,
                               source1: &str, source2: &str) {
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!();
        // Simplified implementation
        self.print_summary(result, file1, file2, source1, source2);
    }
    
    pub fn print_json(&self, result: &DiffResult) -> Result<()> {
        let json = serde_json::to_string_pretty(result)?;
        println!("{}", json);
        Ok(())
    }
    
    pub fn print_lite(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf) {
        // Get just the filenames for display
        let file1_name = file1.file_name().unwrap_or(file1.as_os_str()).to_string_lossy();
        let file2_name = file2.file_name().unwrap_or(file2.as_os_str()).to_string_lossy();
        
        // Group changes by type
        let mut additions = Vec::new();
        let mut deletions = Vec::new();
        let mut modifications = Vec::new();
        
        for change in &result.changes {
            match change.change_type {
                ChangeType::Addition => additions.push(change),
                ChangeType::Deletion => deletions.push(change),
                ChangeType::Modification => modifications.push(change),
                ChangeType::Reorder => {} // Ignore reorders in lite view
            }
        }
        
        // Show deletions
        if !deletions.is_empty() {
            println!("=== Removed ===");
            for change in &deletions {
                if let Some(loc) = &change.location1 {
                    // Extract just the name from the description
                    let name = change.description
                        .strip_prefix("Removed ")
                        .and_then(|s| s.split(" '").nth(1))
                        .and_then(|s| s.strip_suffix("'"))
                        .unwrap_or(&change.description);
                    let line_range = if let Some(end) = loc.end_line {
                        if end > loc.line {
                            format!("{}-{}", loc.line, end)
                        } else {
                            loc.line.to_string()
                        }
                    } else {
                        loc.line.to_string()
                    };
                    println!("- {} ({}:{})", name, file1_name, line_range);
                }
            }
        }
        
        // Show additions  
        if !additions.is_empty() {
            if !deletions.is_empty() {
                println!();
            }
            println!("=== Added ===");
            for change in &additions {
                if let Some(loc) = &change.location2 {
                    // Extract just the name from the description
                    let name = change.description
                        .strip_prefix("Added ")
                        .and_then(|s| s.split(" '").nth(1))
                        .and_then(|s| s.strip_suffix("'"))
                        .unwrap_or(&change.description);
                    let line_range = if let Some(end) = loc.end_line {
                        if end > loc.line {
                            format!("{}-{}", loc.line, end)
                        } else {
                            loc.line.to_string()
                        }
                    } else {
                        loc.line.to_string()
                    };
                    println!("+ {} ({}:{})", name, file2_name, line_range);
                }
            }
        }
        
        // Show modifications with separation between pairs
        if !modifications.is_empty() {
            if !deletions.is_empty() || !additions.is_empty() {
                println!();
            }
            println!("=== Modified ===");
            for (i, change) in modifications.iter().enumerate() {
                if i > 0 {
                    println!(); // Add blank line between modification pairs
                }
                if let (Some(loc1), Some(loc2)) = (&change.location1, &change.location2) {
                    // Extract names from descriptions like "function 'foo' matched with 'bar'" or "function 'foo' structure changed"
                    if change.description.contains("matched with") {
                        // This is a rename
                        let parts: Vec<&str> = change.description.split("'").collect();
                        if parts.len() >= 4 {
                            let old_name = parts[1];
                            let new_name = parts[3];
                            let line_range1 = if let Some(end) = loc1.end_line {
                                if end > loc1.line {
                                    format!("{}-{}", loc1.line, end)
                                } else {
                                    loc1.line.to_string()
                                }
                            } else {
                                loc1.line.to_string()
                            };
                            let line_range2 = if let Some(end) = loc2.end_line {
                                if end > loc2.line {
                                    format!("{}-{}", loc2.line, end)
                                } else {
                                    loc2.line.to_string()
                                }
                            } else {
                                loc2.line.to_string()
                            };
                            println!("- {} ({}:{})", old_name, file1_name, line_range1);
                            println!("+ {} ({}:{})", new_name, file2_name, line_range2);
                        }
                    } else if change.description.contains("structure changed") {
                        // This is a structural change
                        let name = change.description
                            .split(" '")
                            .nth(1)
                            .and_then(|s| s.split("'").next())
                            .unwrap_or(&change.description);
                        let line_range1 = if let Some(end) = loc1.end_line {
                            if end > loc1.line {
                                format!("{}-{}", loc1.line, end)
                            } else {
                                loc1.line.to_string()
                            }
                        } else {
                            loc1.line.to_string()
                        };
                        let line_range2 = if let Some(end) = loc2.end_line {
                            if end > loc2.line {
                                format!("{}-{}", loc2.line, end)
                            } else {
                                loc2.line.to_string()
                            }
                        } else {
                            loc2.line.to_string()
                        };
                        println!("- {} ({}:{})", name, file1_name, line_range1);
                        println!("+ {} ({}:{})", name, file2_name, line_range2);
                    }
                }
            }
        }
    }
    
    pub fn print_compact(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf,
                         source1: &str, source2: &str) {
        println!("--- {}", file1.display());
        println!("+++ {}", file2.display());
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!("Matched declarations: {}/{} vs {}", 
                 result.matched_declarations, result.total_declarations1, result.total_declarations2);
        
        // Calculate and print line statistics
        let (lines_added, lines_removed, total_diff) = self.calculate_line_statistics(result, source1, source2);
        println!("Diff size: {} declarations (+{} added, -{} removed)", total_diff, lines_added, lines_removed);
        
        // Group changes by type
        let mut additions = Vec::new();
        let mut deletions = Vec::new();
        let mut structural_changes = Vec::new();
        let mut renames = Vec::new();
        
        for change in &result.changes {
            match change.change_type {
                ChangeType::Addition => additions.push(change),
                ChangeType::Deletion => deletions.push(change),
                ChangeType::Modification => {
                    // Separate structural/text changes from simple renames
                    if change.description.contains("structure changed") || change.description.contains("text constant changes") {
                        structural_changes.push(change);
                    } else if change.description.contains("matched with") {
                        renames.push(change);
                    }
                }
                ChangeType::Reorder => {} // Ignore reorders in compact view
            }
        }
        
        // Show deletions
        if !deletions.is_empty() {
            println!("\n=== Removed Functions ===");
            for change in &deletions {
                if let Some(loc) = &change.location1 {
                    println!("\n--- {}", change.description);
                    println!("{}:{}", file1.display(), loc.line);
                }
            }
        }
        
        // Show additions
        if !additions.is_empty() {
            println!("\n=== Added Functions ===");
            for change in &additions {
                if let Some(loc) = &change.location2 {
                    println!("\n+++ {}", change.description);
                    println!("{}:{}", file2.display(), loc.line);
                }
            }
        }
        
        // Show structural changes with normalized intra-declaration diffs
        if !structural_changes.is_empty() {
            println!("\n=== Modified Functions ===");
            let mut skipped = 0usize;
            for change in &structural_changes {
                if let (Some(loc1), Some(loc2)) = (&change.location1, &change.location2) {
                    let src1 = self.extract_function_at_line(source1, loc1.line);
                    let src2 = self.extract_function_at_line(source2, loc2.line);

                    if let (Some(ref s1), Some(ref s2)) = (&src1, &src2) {
                        // Fully normalize both sides: rename map + minified ident blanking.
                        // This is used for diffing (to eliminate rename noise) but NOT for display.
                        let (norm_s1, norm_s2) = if !result.rename_map.is_empty() {
                            let renamed = fingerprint::normalize_string_with_renames(s2, &result.rename_map);
                            (
                                fingerprint::normalize_minified_identifiers(s1),
                                fingerprint::normalize_minified_identifiers(&renamed),
                            )
                        } else {
                            (
                                fingerprint::normalize_minified_identifiers(s1),
                                fingerprint::normalize_minified_identifiers(s2),
                            )
                        };

                        // Skip if identical after normalization (pure rename)
                        if norm_s2 == norm_s1 {
                            skipped += 1;
                            continue;
                        }

                        // Two-pass diff: use normalized text for comparison (finds
                        // real changes only), but display original source text
                        // (readable output with actual identifier names).
                        let diff_output = Self::generate_normalized_display_diff(
                            s1, s2, &norm_s1, &norm_s2, 3,
                        );
                        if diff_output.is_empty() {
                            skipped += 1;
                            continue;
                        }

                        println!("\n@@@ {}", change.description);
                        println!("--- {}:{}", file1.display(), loc1.line);
                        println!("+++ {}:{}", file2.display(), loc2.line);
                        print!("{}", diff_output);
                    } else {
                        // Couldn't extract source — fall back to description only
                        println!("\n@@@ {}", change.description);
                        if let Some(ref string_diff) = change.string_diff {
                            print!("{}", self.format_string_diff(string_diff));
                        }
                        println!("--- {}:{} (before)", file1.display(), loc1.line);
                        println!("+++ {}:{} (after)", file2.display(), loc2.line);
                    }
                }
            }
            if skipped > 0 {
                println!("\n({} modifications skipped — only minifier renames)", skipped);
            }
        }
        
        // Show renames
        if !renames.is_empty() && std::env::var("ASTDIFF_SHOW_RENAMES").is_ok() {
            println!("\n=== Renamed Functions ===");
            for change in &renames {
                if let Some(path) = change.structural_path.split("->").nth(1) {
                    println!("    {} -> {}", 
                        change.structural_path.split("->").next().unwrap_or("").replace("global.", ""),
                        path);
                }
            }
        }
        
        println!("\nChanges: {} additions, {} deletions, {} modifications (+ {} renames)", 
                 additions.len(), deletions.len(), structural_changes.len(), renames.len());
    }
    
    /// Generate a mapping file that captures the rename relationships found during diff
    fn get_matches_from_result(&self, declarations1: &[Declaration], declarations2: &[Declaration], result: &DiffResult) -> Vec<(usize, usize, f64)> {
        let mut matches = Vec::new();
        
        // Extract matches from modifications in the result
        for change in &result.changes {
            if let ChangeType::Modification = change.change_type {
                // Try to find the declarations based on the change description
                if let (Some(loc1), Some(loc2)) = (&change.location1, &change.location2) {
                    // Find declaration at location1
                    let idx1_opt = declarations1.iter().position(|d| d.line == loc1.line);
                    let idx2_opt = declarations2.iter().position(|d| d.line == loc2.line);
                    
                    if let (Some(idx1), Some(idx2)) = (idx1_opt, idx2_opt) {
                        // Calculate similarity if not already stored
                        let similarity = self.calculate_declaration_similarity(&declarations1[idx1], &declarations2[idx2], "", "");
                        matches.push((idx1, idx2, similarity));
                    }
                }
            }
        }
        
        matches
    }
    
    pub fn generate_rename_mapping(&self, result: &DiffResult) -> HashMap<String, String> {
        let mut mappings = HashMap::new();
        
        for change in &result.changes {
            if let ChangeType::Modification = change.change_type {
                if change.description.contains("matched with") {
                    // Extract the rename relationship from the structural_path
                    if let Some((from, to)) = change.structural_path
                        .strip_prefix("global.")
                        .and_then(|s| s.split_once("->")) {
                        mappings.insert(from.to_string(), to.to_string());
                    }
                }
            }
        }
        
        mappings
    }
    
    /// Apply existing mappings to resolve names in the diff
    pub fn apply_mappings_to_result(&self, result: &mut DiffResult) {
        if let (Some(map1), Some(map2)) = (&self.mappings1, &self.mappings2) {
            for change in &mut result.changes {
                // Apply mappings to resolve semantic names
                if let Some(_loc1) = &mut change.location1 {
                    // Look up semantic name from canonical name
                    if let Some(parts) = change.structural_path.strip_prefix("global.") {
                        if let Some(canonical_name) = parts.split("->").next() {
                            if let Some(semantic_name) = map1.get(canonical_name) {
                                change.description = change.description.replace(
                                    &format!("'{}'", canonical_name),
                                    &format!("'{}' ({})", semantic_name, canonical_name)
                                );
                            }
                        }
                    }
                }
                
                if let Some(_loc2) = &mut change.location2 {
                    // Look up semantic name from canonical name for second file
                    if let Some(parts) = change.structural_path.split("->").nth(1) {
                        if let Some(semantic_name) = map2.get(parts) {
                            change.description = change.description.replace(
                                &format!("'{}'", parts),
                                &format!("'{}' ({})", semantic_name, parts)
                            );
                        }
                    }
                }
            }
        }
    }
    
    pub fn match_declarations(&self, decls1: &[Declaration], decls2: &[Declaration], source1: &str, source2: &str)
        -> (Vec<(usize, usize)>, Vec<Change>, HashMap<String, String>) {
        use parallel_matching_v2::ParallelMatcherV2;
        use profiling::Timer;

        eprintln!("Using parallel matching v2 for {} x {} declarations", decls1.len(), decls2.len());

        // Convert to thread-safe data structures
        let data1: Vec<DeclarationData> = {
            let _timer = Timer::new("convert_to_data_structures");
            decls1.iter().map(|d| d.to_data()).collect()
        };
        let data2: Vec<DeclarationData> = decls2.iter().map(|d| d.to_data()).collect();

        // Build rarity scorer if using fingerprints
        let scorer = if self.use_fingerprints {
            let _timer = Timer::new("build_rarity_scorer_parallel");
            let mut scorer = RarityScorer::new();
            for decl in decls1.iter().chain(decls2.iter()) {
                if let Some(ref fp) = decl.fingerprint {
                    scorer.add_fingerprint(fp);
                }
            }
            Some(scorer)
        } else {
            None
        };

        let matcher = ParallelMatcherV2::new(self.use_fingerprints);

        matcher.match_declarations(
            &data1,
            &data2,
            source1,
            source2,
            scorer.as_ref(),
            |d1, d2, s1, s2| self.calculate_declaration_similarity_data(d1, d2, s1, s2),
            |d1, d2, fp1, fp2, s| self.create_evidence_breakdown_data(d1, d2, fp1, fp2, s),
        )
    }
    
    fn calculate_declaration_similarity_data(&self, decl1: &DeclarationData, decl2: &DeclarationData, _source1: &str, _source2: &str) -> f64 {
        // For imports and exports, use signature similarity regardless of kind
        if matches!(decl1.kind, DeclarationKind::Import | DeclarationKind::Export) 
            || matches!(decl2.kind, DeclarationKind::Import | DeclarationKind::Export) {
            return if decl1.signature == decl2.signature { 1.0 } else { 0.3 };
        }
        
        // Quick size check - if sizes are too different, skip expensive set operations
        let size1 = decl1.structural_hashes.len();
        let size2 = decl2.structural_hashes.len();
        
        if size1 == 0 && size2 == 0 {
            // Both are empty (e.g., simple variables with no initialization)
            if decl1.signature == decl2.signature {
                return 1.0;
            } else {
                let base_sim = 0.5;
                
                // Apply cross-kind penalty if needed
                if decl1.kind != decl2.kind {
                    if (matches!(decl1.kind, DeclarationKind::Function) && matches!(decl2.kind, DeclarationKind::Variable))
                        || (matches!(decl1.kind, DeclarationKind::Variable) && matches!(decl2.kind, DeclarationKind::Function)) {
                        return base_sim * 0.9;
                    } else {
                        return base_sim * 0.7;
                    }
                }
                return base_sim;
            }
        }
        
        // If one is much larger than the other, they can't be similar enough
        let size_ratio = if size1 > size2 {
            size2 as f64 / size1 as f64
        } else {
            size1 as f64 / size2 as f64
        };
        
        if size_ratio < 0.3 {
            return 0.2; // Too different in size
        }
        
        // Calculate base similarity from structural hash intersection
        let intersection: HashSet<_> = decl1.structural_hashes.intersection(&decl2.structural_hashes).cloned().collect();
        let union: HashSet<_> = decl1.structural_hashes.union(&decl2.structural_hashes).cloned().collect();
        
        let base_similarity = intersection.len() as f64 / union.len() as f64;
        
        // Apply a penalty for different kinds, but allow matching
        if decl1.kind != decl2.kind {
            // Function <-> Variable is common in minified code, apply small penalty
            if (matches!(decl1.kind, DeclarationKind::Function) && matches!(decl2.kind, DeclarationKind::Variable))
                || (matches!(decl1.kind, DeclarationKind::Variable) && matches!(decl2.kind, DeclarationKind::Function)) {
                base_similarity * 0.9  // 10% penalty
            } else {
                base_similarity * 0.7  // 30% penalty for other mismatches
            }
        } else {
            base_similarity
        }
    }
    
    fn create_evidence_breakdown_data(&self, decl1: &DeclarationData, decl2: &DeclarationData, 
                                     fp1: &FunctionFingerprint, fp2: &FunctionFingerprint, 
                                     scorer: &RarityScorer) -> EvidenceBreakdown {
        let mut string_matches = Vec::new();
        let mut constant_matches = Vec::new();
        let mut api_matches = Vec::new();
        
        // Match strings
        for s1 in &fp1.strings {
            for s2 in &fp2.strings {
                if s1.value == s2.value {
                    let rarity = scorer.score_string(&s1.value);
                    let context_weight = match s1.context {
                        StringContext::ErrorMessage => 1.2,
                        StringContext::FilePath => 1.1,
                        StringContext::CommandName => 1.0,
                        StringContext::ConfigKey => 0.9,
                        StringContext::ApiEndpoint => 1.0,
                        StringContext::Regular => 0.7,
                    };
                    string_matches.push(StringMatch {
                        value: s1.value.clone(),
                        context: format!("{:?}", s1.context),
                        rarity_score: rarity,
                        contribution: rarity * context_weight,
                    });
                    break;
                }
            }
        }
        
        // Match constants
        for c1 in &fp1.constants {
            for c2 in &fp2.constants {
                if c1.value == c2.value {
                    let rarity = scorer.score_constant(&c1.value);
                    constant_matches.push(ConstantMatch {
                        value: format!("{:?}", c1.value),
                        type_: match &c1.value {
                            ConstantValue::Number(_) => "number",
                            ConstantValue::Float(_) => "float",
                            ConstantValue::Regex(_) => "regex",
                            ConstantValue::Duration(_) => "duration",
                        }.to_string(),
                        rarity_score: rarity,
                        contribution: rarity * 0.8,
                    });
                    break;
                }
            }
        }
        
        // Match API calls
        for api1 in &fp1.api_calls {
            for api2 in &fp2.api_calls {
                if api1.object == api2.object && api1.method == api2.method {
                    let rarity = scorer.score_api_call(api1);
                    api_matches.push(ApiMatch {
                        call: format!("{}.{}", 
                            api1.object.as_deref().unwrap_or("global"), 
                            api1.method),
                        first_arg: api1.first_arg.clone(),
                        rarity_score: rarity,
                        contribution: rarity * 0.6,
                    });
                    break;
                }
            }
        }
        
        // Calculate unique elements
        let unique_strings1: Vec<_> = fp1.strings.iter()
            .filter(|s| !fp2.strings.iter().any(|s2| s2.value == s.value))
            .map(|s| (s.value.clone(), format!("{:?}", s.context)))
            .collect();
            
        let unique_strings2: Vec<_> = fp2.strings.iter()
            .filter(|s| !fp1.strings.iter().any(|s1| s1.value == s.value))
            .map(|s| (s.value.clone(), format!("{:?}", s.context)))
            .collect();
        
        let unique_to_func1 = UniqueElements {
            strings: unique_strings1,
            constants: Vec::new(), // TODO: implement
            api_calls: Vec::new(), // TODO: implement
        };
        
        let unique_to_func2 = UniqueElements {
            strings: unique_strings2,
            constants: Vec::new(), // TODO: implement
            api_calls: Vec::new(), // TODO: implement
        };
        
        // Size analysis
        let size_ratio = decl2.size as f64 / decl1.size as f64;
        let interpretation = if size_ratio > 1.2 {
            "likely enhanced"
        } else if size_ratio < 0.8 {
            "significantly reduced"
        } else {
            "similar size"
        }.to_string();
        
        let total_score = string_matches.iter().map(|s| s.contribution).sum::<f64>()
            + constant_matches.iter().map(|c| c.contribution).sum::<f64>()
            + api_matches.iter().map(|a| a.contribution).sum::<f64>();
            
        let evidence_count = string_matches.len() + constant_matches.len() + api_matches.len();
        
        EvidenceBreakdown {
            total_score,
            evidence_count,
            string_matches,
            constant_matches,
            api_matches,
            unique_to_func1,
            unique_to_func2,
            size_analysis: SizeAnalysis {
                size1: decl1.size,
                size2: decl2.size,
                ratio: size_ratio,
                size_penalty: if size_ratio < 0.7 { 0.2 } else { 0.0 },
                interpretation,
            },
        }
    }
}