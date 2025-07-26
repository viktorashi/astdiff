use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use anyhow::Result;
use tree_sitter::{Node, Tree};
use serde::{Serialize, Deserialize};

/// Represents a structural diff between two JavaScript ASTs
pub struct StructuralDiff {
    mappings1: Option<HashMap<String, String>>,
    mappings2: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone)]
struct Declaration {
    name: String,
    kind: DeclarationKind,
    node: Node<'static>,
    line: usize,
    signature: String,
    structural_hashes: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum DeclarationKind {
    Function,
    Variable,
    Class,
    Import,
    Export,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiffResult {
    pub identical: bool,
    pub similarity: f64,
    pub changes: Vec<Change>,
    pub matched_declarations: usize,
    pub total_declarations1: usize,
    pub total_declarations2: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Change {
    pub change_type: ChangeType,
    pub location1: Option<Location>,
    pub location2: Option<Location>,
    pub description: String,
    pub structural_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ChangeType {
    Addition,
    Deletion,
    Modification,
    Reorder,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Location {
    pub line: usize,
    pub column: usize,
    pub code_snippet: String,
}

impl StructuralDiff {
    pub fn new() -> Self {
        Self {
            mappings1: None,
            mappings2: None,
        }
    }
    
    pub fn set_mappings1(&mut self, mappings: HashMap<String, String>) {
        self.mappings1 = Some(mappings);
    }
    
    pub fn set_mappings2(&mut self, mappings: HashMap<String, String>) {
        self.mappings2 = Some(mappings);
    }
    
    pub fn compare(&self, tree1: &Tree, source1: &str, tree2: &Tree, source2: &str) -> Result<DiffResult> {
        // Extract global declarations from both files
        let declarations1 = self.extract_declarations(tree1.root_node(), source1);
        let declarations2 = self.extract_declarations(tree2.root_node(), source2);
        
        // Match declarations based on similarity
        let (matches, changes) = self.match_declarations(&declarations1, &declarations2, source1, source2);
        
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
        })
    }
    
    fn extract_declarations<'a>(&self, root: Node<'a>, source: &str) -> Vec<Declaration> {
        let mut declarations = Vec::new();
        self.extract_declarations_recursive(root, source, &mut declarations, true);
        declarations
    }
    
    fn extract_declarations_recursive<'a>(&self, node: Node<'a>, source: &str, declarations: &mut Vec<Declaration>, is_global: bool) {
        match node.kind() {
            "function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = &source[name_node.byte_range()];
                    let signature = self.get_function_signature(node, source);
                    let structural_hashes = self.collect_structural_hashes(node, source);
                    declarations.push(Declaration {
                        name: name.to_string(),
                        kind: DeclarationKind::Function,
                        node: unsafe { std::mem::transmute(node) },
                        line: node.start_position().row + 1,
                        signature,
                        structural_hashes,
                    });
                }
            }
            "variable_declaration" if is_global => {
                // Extract all variable declarators
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "variable_declarator" {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            if name_node.kind() == "identifier" {
                                let name = &source[name_node.byte_range()];
                                let signature = self.get_variable_signature(child, source);
                                let structural_hashes = if let Some(value_node) = child.child_by_field_name("value") {
                                    self.collect_structural_hashes(value_node, source)
                                } else {
                                    HashSet::new()
                                };
                                declarations.push(Declaration {
                                    name: name.to_string(),
                                    kind: DeclarationKind::Variable,
                                    node: unsafe { std::mem::transmute(child) },
                                    line: child.start_position().row + 1,
                                    signature,
                                    structural_hashes,
                                });
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
                    declarations.push(Declaration {
                        name: name.to_string(),
                        kind: DeclarationKind::Class,
                        node: unsafe { std::mem::transmute(node) },
                        line: node.start_position().row + 1,
                        signature,
                        structural_hashes,
                    });
                }
            }
            "import_statement" => {
                let signature = self.get_import_signature(node, source);
                let structural_hashes = self.collect_structural_hashes(node, source);
                declarations.push(Declaration {
                    name: format!("import@{}", node.start_position().row),
                    kind: DeclarationKind::Import,
                    node: unsafe { std::mem::transmute(node) },
                    line: node.start_position().row + 1,
                    signature,
                    structural_hashes,
                });
            }
            "export_statement" => {
                if let Some(decl) = node.child_by_field_name("declaration") {
                    self.extract_declarations_recursive(decl, source, declarations, is_global);
                } else {
                    let signature = self.get_export_signature(node, source);
                    let structural_hashes = self.collect_structural_hashes(node, source);
                    declarations.push(Declaration {
                        name: format!("export@{}", node.start_position().row),
                        kind: DeclarationKind::Export,
                        node: unsafe { std::mem::transmute(node) },
                        line: node.start_position().row + 1,
                        signature,
                        structural_hashes,
                    });
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
    
    fn collect_structural_hashes(&self, node: Node, source: &str) -> HashSet<String> {
        let mut hashes = HashSet::new();
        self.collect_structural_hashes_recursive(node, source, &mut hashes);
        hashes
    }
    
    fn collect_structural_hashes_recursive(&self, node: Node, source: &str, hashes: &mut HashSet<String>) {
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
    
    fn compute_structural_hash(&self, node: Node, source: &str) -> String {
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
        
        format!("{:016x}", hasher.finish())
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
    
    fn match_declarations(&self, decls1: &[Declaration], decls2: &[Declaration], source1: &str, source2: &str) 
        -> (Vec<(usize, usize)>, Vec<Change>) {
        let mut matches = Vec::new();
        let mut changes = Vec::new();
        let mut matched1 = vec![false; decls1.len()];
        let mut matched2 = vec![false; decls2.len()];
        
        // Calculate similarity scores for all pairs
        let mut similarities = Vec::new();
        for (i, decl1) in decls1.iter().enumerate() {
            for (j, decl2) in decls2.iter().enumerate() {
                let similarity = self.calculate_declaration_similarity(decl1, decl2, source1, source2);
                if similarity > 0.0 {
                    similarities.push((i, j, similarity));
                }
            }
        }
        
        // Sort by similarity (highest first)
        similarities.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
        
        // Greedy matching
        for (i, j, similarity) in similarities {
            if !matched1[i] && !matched2[j] && similarity >= 0.5 {
                matched1[i] = true;
                matched2[j] = true;
                matches.push((i, j));
                
                let decl1 = &decls1[i];
                let decl2 = &decls2[j];
                
                // Check if names differ (indicates matching different functions)
                if decl1.name != decl2.name {
                    changes.push(Change {
                        change_type: ChangeType::Modification,
                        location1: Some(self.create_location(decl1.node, source1)),
                        location2: Some(self.create_location(decl2.node, source2)),
                        description: format!("{} '{}' matched with '{}'", 
                            self.kind_to_string(&decl1.kind), decl1.name, decl2.name),
                        structural_path: format!("global.{}->{}", decl1.name, decl2.name),
                    });
                }
                
                // Check if it's a reorder
                if decl1.line != decl2.line {
                    changes.push(Change {
                        change_type: ChangeType::Reorder,
                        location1: Some(self.create_location(decl1.node, source1)),
                        location2: Some(self.create_location(decl2.node, source2)),
                        description: format!("{} '{}' moved from line {} to line {}", 
                            self.kind_to_string(&decl1.kind), decl1.name, decl1.line, decl2.line),
                        structural_path: format!("global.{}", decl1.name),
                    });
                }
                
                // Check for signature changes (only if similarity is not perfect)
                if similarity < 0.95 && decl1.signature != decl2.signature {
                    changes.push(Change {
                        change_type: ChangeType::Modification,
                        location1: Some(self.create_location(decl1.node, source1)),
                        location2: Some(self.create_location(decl2.node, source2)),
                        description: format!("{} '{}' structure changed (similarity: {:.1}%)", 
                            self.kind_to_string(&decl1.kind), decl1.name, similarity * 100.0),
                        structural_path: format!("global.{}", decl1.name),
                    });
                }
            }
        }
        
        // Report unmatched declarations
        for (i, decl1) in decls1.iter().enumerate() {
            if !matched1[i] {
                changes.push(Change {
                    change_type: ChangeType::Deletion,
                    location1: Some(self.create_location(decl1.node, source1)),
                    location2: None,
                    description: format!("Removed {} '{}'", self.kind_to_string(&decl1.kind), decl1.name),
                    structural_path: format!("global.{}", decl1.name),
                });
            }
        }
        
        for (j, decl2) in decls2.iter().enumerate() {
            if !matched2[j] {
                changes.push(Change {
                    change_type: ChangeType::Addition,
                    location1: None,
                    location2: Some(self.create_location(decl2.node, source2)),
                    description: format!("Added {} '{}'", self.kind_to_string(&decl2.kind), decl2.name),
                    structural_path: format!("global.{}", decl2.name),
                });
            }
        }
        
        (matches, changes)
    }
    
    fn calculate_declaration_similarity(&self, decl1: &Declaration, decl2: &Declaration, _source1: &str, _source2: &str) -> f64 {
        // Different kinds = low similarity
        if decl1.kind != decl2.kind {
            return 0.0;
        }
        
        // For imports and exports, use signature similarity
        if matches!(decl1.kind, DeclarationKind::Import | DeclarationKind::Export) {
            return if decl1.signature == decl2.signature { 1.0 } else { 0.3 };
        }
        
        // Calculate similarity based on structural hash intersection
        let intersection: HashSet<_> = decl1.structural_hashes.intersection(&decl2.structural_hashes).cloned().collect();
        let union: HashSet<_> = decl1.structural_hashes.union(&decl2.structural_hashes).cloned().collect();
        
        if union.is_empty() {
            // Both are empty (e.g., simple variables with no initialization)
            return if decl1.signature == decl2.signature { 1.0 } else { 0.5 };
        }
        
        intersection.len() as f64 / union.len() as f64
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
    
    fn kind_to_string(&self, kind: &DeclarationKind) -> &'static str {
        match kind {
            DeclarationKind::Function => "function",
            DeclarationKind::Variable => "variable",
            DeclarationKind::Class => "class",
            DeclarationKind::Import => "import",
            DeclarationKind::Export => "export",
        }
    }
    
    fn create_location(&self, node: Node, source: &str) -> Location {
        let start = node.start_position();
        Location {
            line: start.row + 1,
            column: start.column + 1,
            code_snippet: self.get_snippet(source, node),
        }
    }
    
    fn get_snippet(&self, source: &str, node: Node) -> String {
        let text = &source[node.byte_range()];
        let max_chars = 60;
        
        // Handle UTF-8 properly by iterating over chars
        if text.chars().count() > max_chars {
            let truncated: String = text.chars().take(max_chars).collect();
            format!("{}...", truncated)
        } else {
            text.to_string()
        }
    }
    
    pub fn print_summary(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf) {
        println!("--- {}", file1.display());
        println!("+++ {}", file2.display());
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!("Matched declarations: {}/{} vs {}", 
                 result.matched_declarations, result.total_declarations1, result.total_declarations2);
        
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
                    // Separate structural changes from simple renames
                    if change.description.contains("structure changed") {
                        structural_changes.push(change);
                    } else if change.description.contains("matched with") {
                        renames.push(change);
                    }
                }
                ChangeType::Reorder => {} // Ignore reorders
            }
        }
        
        let meaningful_changes = additions.len() + deletions.len() + structural_changes.len();
        println!("Meaningful changes: {} (+ {} renames)", meaningful_changes, renames.len());
        println!();
        
        // Show deletions first
        if !deletions.is_empty() {
            println!("=== Removed Functions ===");
            for change in deletions {
                println!("--- {}", change.description);
                if let Some(loc) = &change.location1 {
                    println!("    at line {}: {}", loc.line, loc.code_snippet);
                }
            }
            println!();
        }
        
        // Show additions
        if !additions.is_empty() {
            println!("=== Added Functions ===");
            for change in additions {
                println!("+++ {}", change.description);
                if let Some(loc) = &change.location2 {
                    println!("    at line {}: {}", loc.line, loc.code_snippet);
                }
            }
            println!();
        }
        
        // Show structural changes
        if !structural_changes.is_empty() {
            println!("=== Structurally Modified Functions ===");
            for change in structural_changes {
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
        
        // Optionally show renames in verbose mode
        if !renames.is_empty() && std::env::var("VARMAP_SHOW_RENAMES").is_ok() {
            println!("=== Renamed Functions (set VARMAP_SHOW_RENAMES to see) ===");
            for change in renames {
                if let Some(path) = change.structural_path.split("->").nth(1) {
                    println!("  {} -> {}", 
                        change.structural_path.split("->").next().unwrap_or("").replace("global.", ""),
                        path);
                }
            }
            println!();
        }
    }
    
    pub fn print_interleaved(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf, 
                             canonical1: Option<&str>, canonical2: Option<&str>) -> Result<()> {
        println!("--- {}", file1.display());
        println!("+++ {}", file2.display());
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!("Matched declarations: {}/{} vs {}", 
                 result.matched_declarations, result.total_declarations1, result.total_declarations2);
        
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
        
        let meaningful_changes = additions.len() + deletions.len() + structural_changes.len();
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
        if !renames.is_empty() && std::env::var("VARMAP_SHOW_RENAMES").is_ok() {
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
        
        for (i, line) in lines.iter().enumerate().skip(start_line - 1) {
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
        if !renames.is_empty() && std::env::var("VARMAP_SHOW_RENAMES").is_ok() {
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
            
            // Print the function with line numbers
            for i in (start_line - 1)..end_line.min(lines.len()) {
                println!("{}{:4} {}", prefix, i + 1, lines[i]);
            }
        }
    }
    
    pub fn print_side_by_side(&self, result: &DiffResult) {
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!();
        // Simplified implementation
        self.print_summary(result, &std::path::PathBuf::from("file1"), &std::path::PathBuf::from("file2"));
    }
    
    pub fn print_json(&self, result: &DiffResult) -> Result<()> {
        let json = serde_json::to_string_pretty(result)?;
        println!("{}", json);
        Ok(())
    }
    
    /// Generate a mapping file that captures the rename relationships found during diff
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
}