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
    use_parallel: bool,
}

#[derive(Debug, Clone)]
struct Declaration {
    name: String,
    kind: DeclarationKind,
    node: Node<'static>,
    line: usize,
    signature: String,
    structural_hashes: HashSet<String>,
    size: usize,
    minhash_signature: Vec<u64>,
    fingerprint: Option<FunctionFingerprint>,
}

// Thread-safe declaration data for parallel processing
#[derive(Debug, Clone)]
pub(crate) struct DeclarationData {
    name: String,
    kind: DeclarationKind,
    line: usize,
    signature: String,
    structural_hashes: HashSet<String>,
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
            signature: self.signature.clone(),
            structural_hashes: self.structural_hashes.clone(),
            size: self.size,
            minhash_signature: self.minhash_signature.clone(),
            fingerprint: self.fingerprint.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DeclarationKind {
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
pub(crate) struct Change {
    pub change_type: ChangeType,
    pub location1: Option<Location>,
    pub location2: Option<Location>,
    pub description: String,
    pub structural_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum ChangeType {
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
            use_fingerprints: true,  // Default to true for better accuracy
            generate_report: false,
            report_path: None,
            use_parallel: true,  // Default to true for better performance
        }
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
    
    pub fn set_use_parallel(&mut self, use_parallel: bool) {
        self.use_parallel = use_parallel;
    }
    
    fn calculate_line_statistics(&self, result: &DiffResult, source1: &str, source2: &str) -> (usize, usize, usize) {
        use profiling::Timer;
        let _timer = Timer::new("calculate_line_statistics");
        
        // Pre-compute lines to avoid repeated parsing
        let lines1: Vec<&str> = source1.lines().collect();
        let lines2: Vec<&str> = source2.lines().collect();
        
        let mut lines_added = 0;
        let mut lines_removed = 0;
        
        let mut processed_lines1 = HashSet::new();
        let mut processed_lines2 = HashSet::new();
        
        for change in &result.changes {
            match change.change_type {
                ChangeType::Addition => {
                    if let Some(loc) = &change.location2 {
                        let lines = self.count_declaration_lines_with_lines(loc.line, &lines2);
                        if !processed_lines2.contains(&loc.line) {
                            lines_added += lines;
                            processed_lines2.insert(loc.line);
                        }
                    }
                }
                ChangeType::Deletion => {
                    if let Some(loc) = &change.location1 {
                        let lines = self.count_declaration_lines_with_lines(loc.line, &lines1);
                        if !processed_lines1.contains(&loc.line) {
                            lines_removed += lines;
                            processed_lines1.insert(loc.line);
                        }
                    }
                }
                ChangeType::Modification => {
                    if let (Some(loc1), Some(loc2)) = (&change.location1, &change.location2) {
                        if !change.description.contains("moved from line") {
                            let lines1 = self.count_declaration_lines_with_lines(loc1.line, &lines1);
                            let lines2 = self.count_declaration_lines_with_lines(loc2.line, &lines2);
                            if !processed_lines1.contains(&loc1.line) && !processed_lines2.contains(&loc2.line) {
                                // For modifications, count as removal + addition
                                lines_removed += lines1;
                                lines_added += lines2;
                                processed_lines1.insert(loc1.line);
                                processed_lines2.insert(loc2.line);
                            }
                        }
                    }
                }
                ChangeType::Reorder => {} // Don't count reorders in line statistics
            }
        }
        
        // Total diff lines is all the lines that would appear in the AST diff output
        let total_diff_lines = lines_added + lines_removed;
        (lines_added, lines_removed, total_diff_lines)
    }
    
    fn count_declaration_lines(&self, start_line: usize, source: &str) -> usize {
        use profiling::Timer;
        let _timer = Timer::new("count_declaration_lines");
        let lines: Vec<&str> = source.lines().collect();
        self.count_declaration_lines_with_lines(start_line, &lines)
    }

    fn count_declaration_lines_with_lines(&self, start_line: usize, lines: &[&str]) -> usize {
        if start_line == 0 || start_line > lines.len() {
            return 1;
        }
        
        let first_line = lines[start_line - 1];
        
        // For simple declarations, just count as 1 line
        if first_line.trim().ends_with(',') || first_line.trim().ends_with(';') {
            return 1;
        }
        
        // For functions/classes, count until closing brace
        let mut end_line = start_line;
        let mut brace_count = 0;
        let mut found_open = false;
        
        for (i, line) in lines.iter().enumerate().skip(start_line - 1) {
            if line.contains('{') {
                brace_count += line.matches('{').count();
                found_open = true;
            }
            if line.contains('}') {
                brace_count -= line.matches('}').count();
            }
            
            if found_open && brace_count == 0 {
                end_line = i + 1;
                break;
            }
            
            // For arrow functions without braces
            if i == start_line - 1 && line.contains("=>") && !line.contains("{") {
                end_line = i + 1;
                break;
            }
        }
        
        end_line - start_line + 1
    }
    
    pub fn set_mappings1(&mut self, mappings: HashMap<String, String>) {
        self.mappings1 = Some(mappings);
    }
    
    pub fn set_mappings2(&mut self, mappings: HashMap<String, String>) {
        self.mappings2 = Some(mappings);
    }
    
    pub fn compare(&self, tree1: &Tree, source1: &str, tree2: &Tree, source2: &str) -> Result<DiffResult> {
        use profiling::Timer;
        
        // Extract global declarations from both files
        let declarations1 = {
            let _timer = Timer::new("extract_declarations_file1");
            self.extract_declarations(tree1.root_node(), source1)
        };
        let declarations2 = {
            let _timer = Timer::new("extract_declarations_file2");
            self.extract_declarations(tree2.root_node(), source2)
        };
        
        eprintln!("Extracted {} declarations from file1, {} from file2", 
                 declarations1.len(), declarations2.len());
        
        // Match declarations based on similarity
        let (matches, changes) = {
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
        })
    }
    
    fn extract_declarations<'a>(&self, root: Node<'a>, source: &str) -> Vec<Declaration> {
        let mut declarations = Vec::new();
        self.extract_declarations_recursive(root, source, &mut declarations, true);
        declarations
    }
    
    fn create_declaration(&self, name: String, kind: DeclarationKind, node: Node<'static>, 
                         line: usize, signature: String, structural_hashes: HashSet<String>, 
                         source: &str) -> Declaration {
        let size = structural_hashes.len();
        let minhash_signature = self.compute_minhash(&structural_hashes, 128);
        
        // Extract fingerprint for better matching
        let fingerprint = if self.use_fingerprints && matches!(kind, DeclarationKind::Function | DeclarationKind::Variable) {
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
        use profiling::Timer;
        
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
    
    fn compute_minhash(&self, hashes: &HashSet<String>, num_hashes: usize) -> Vec<u64> {
        let mut signature = vec![u64::MAX; num_hashes];
        
        for hash_str in hashes {
            for i in 0..num_hashes {
                let hash_value = self.hash_with_seed(hash_str, i);
                signature[i] = signature[i].min(hash_value);
            }
        }
        
        signature
    }
    
    fn hash_with_seed(&self, value: &str, seed: usize) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        seed.hash(&mut hasher);
        hasher.finish()
    }
    
    fn estimate_minhash_similarity(&self, sig1: &[u64], sig2: &[u64]) -> f64 {
        let matches = sig1.iter().zip(sig2).filter(|(a, b)| a == b).count();
        matches as f64 / sig1.len() as f64
    }
    
    fn match_declarations(&self, decls1: &[Declaration], decls2: &[Declaration], source1: &str, source2: &str) 
        -> (Vec<(usize, usize)>, Vec<Change>) {
        use profiling::Timer;
        
        if self.use_parallel && (decls1.len() > 50 || decls2.len() > 50) {
            // Use parallel matching for large files
            return self.match_declarations_parallel(decls1, decls2, source1, source2);
        }
        
        // Build rarity scorer if using fingerprints
        let scorer = if self.use_fingerprints {
            let _timer = Timer::new("build_rarity_scorer");
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
        
        // Initialize report builder if needed
        let mut report_builder = if self.generate_report {
            Some(MatchingReportBuilder::new(0.5, 2)) // Default thresholds
        } else {
            None
        };
        // Sort declarations by size while keeping original indices
        let mut sorted1: Vec<(usize, &Declaration)> = decls1.iter().enumerate()
            .map(|(i, d)| (i, d))
            .collect();
        sorted1.sort_by_key(|(_, d)| d.size);
        
        let mut sorted2: Vec<(usize, &Declaration)> = decls2.iter().enumerate()
            .map(|(i, d)| (i, d))
            .collect();
        sorted2.sort_by_key(|(_, d)| d.size);
        
        // Debug sorted arrays
        if std::env::var("ASTDIFF_DEBUG").is_ok() {
            eprintln!("\nSorted declarations from file 2:");
            for (idx, (i, d)) in sorted2.iter().enumerate() {
                eprintln!("  [{}] idx={}, '{}' size={} kind={:?}", idx, i, d.name, d.size, d.kind);
            }
        }
        
        let mut matches = Vec::new();
        let mut changes = Vec::new();
        let mut matched1 = vec![false; decls1.len()];
        let mut matched2 = vec![false; decls2.len()];
        
        let mut j_start = 0; // Track where size window starts for efficiency
        
        // Process declarations in size order
        let mut total_candidates = 0;
        let mut total_similarity_checks = 0;
        
        for (i1, decl1) in &sorted1 {
            if matched1[*i1] {
                continue;
            }
            
            let _timer = Timer::new("match_single_declaration");
            
            // Find size window in sorted2 (within 50% size difference for better matching of enhanced functions)
            let min_size = ((decl1.size as f64) * 0.5).max(1.0) as usize;
            let max_size = ((decl1.size as f64) * 1.5) as usize;
            
            // Debug size window
            if std::env::var("ASTDIFF_DEBUG").is_ok() && decl1.name == "IF" {
                eprintln!("  Size window for IF (size {}): {} - {}", decl1.size, min_size, max_size);
                eprintln!("  j_start: {}, sorted2.len: {}", j_start, sorted2.len());
            }
            
            // Move j_start forward until we're in range
            while j_start < sorted2.len() && sorted2[j_start].1.size < min_size {
                j_start += 1;
            }
            
            // Collect candidates using LSH within size window
            let mut candidates = Vec::new();
            {
                let _timer = Timer::new("candidate_collection");
                for j in j_start..sorted2.len() {
                    let (i2, decl2) = &sorted2[j];
                    
                    if decl2.size > max_size {
                        break; // Past the size window
                    }
                    
                    if matched2[*i2] || decl1.kind != decl2.kind {
                        if std::env::var("ASTDIFF_DEBUG").is_ok() && decl1.name == "IF" && decl2.name == "IF" {
                            eprintln!("  Skipping IF->IF: matched2[{}]={}, kinds match={}", 
                                *i2, matched2[*i2], decl1.kind == decl2.kind);
                        }
                        continue;
                    }
                    
                    // Quick LSH filter
                    let estimated_sim = {
                        let _timer = Timer::new("lsh_similarity");
                        self.estimate_minhash_similarity(
                            &decl1.minhash_signature,
                            &decl2.minhash_signature
                        )
                    };
                    
                    if estimated_sim >= 0.3 { // Lower threshold for LSH
                        candidates.push((*i2, estimated_sim));
                    }
                }
            }
            total_candidates += candidates.len();
            
            // Sort candidates by LSH similarity
            candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            
            // Check top candidates with full similarity
            let mut best_match = None;
            let mut best_similarity = 0.0;
            let mut best_evidence_count = 0;
            let mut second_best_similarity = 0.0;
            let mut all_similarities = Vec::new();
            let mut best_evidence_breakdown = None;
            
            for (i2, _lsh_sim) in candidates.iter().take(5) { // Only check top 5
                let decl2 = &decls2[*i2];
                total_similarity_checks += 1;
                
                // Use fingerprint matching if available
                let (full_similarity, evidence_count, evidence_breakdown) = {
                    let _timer = Timer::new("full_similarity_calculation");
                    if self.use_fingerprints {
                        if let (Some(ref fp1), Some(ref fp2), Some(ref s)) = 
                            (&decl1.fingerprint, &decl2.fingerprint, &scorer) {
                            let (fp_score, ev_count) = {
                                let _timer = Timer::new("fingerprint_similarity");
                                calculate_fingerprint_similarity(fp1, fp2, s)
                            };
                            let breakdown = {
                                let _timer = Timer::new("evidence_breakdown");
                                self.create_evidence_breakdown(decl1, decl2, fp1, fp2, s)
                            };
                            // Combine with structural similarity
                            let struct_sim = {
                                let _timer = Timer::new("structural_similarity");
                                self.calculate_declaration_similarity(decl1, decl2, source1, source2)
                            };
                            let combined = fp_score * 0.7 + struct_sim * 0.3;
                            (combined, ev_count, Some(breakdown))
                        } else {
                            (self.calculate_declaration_similarity(decl1, decl2, source1, source2), 0, None)
                        }
                    } else {
                        (self.calculate_declaration_similarity(decl1, decl2, source1, source2), 0, None)
                    }
                };
                
                all_similarities.push((decl2.name.clone(), full_similarity));
                
                if full_similarity > best_similarity {
                    second_best_similarity = best_similarity;
                    best_match = Some(*i2);
                    best_similarity = full_similarity;
                    best_evidence_count = evidence_count;
                    best_evidence_breakdown = evidence_breakdown;
                    
                    // Early termination for very good matches
                    if full_similarity >= 0.95 {
                        break;
                    }
                } else if full_similarity > second_best_similarity {
                    second_best_similarity = full_similarity;
                }
            }
            
            // Determine if we have a match using adaptive thresholds
            let should_match = if scorer.is_some() && best_evidence_count > 0 {
                // Use evidence-based matching for fingerprints
                self.should_match_with_evidence(best_similarity, best_evidence_count, decl1.size)
            } else {
                self.should_match(best_similarity, second_best_similarity, decl1.size)
            };
            
            // Debug output if enabled
            if std::env::var("ASTDIFF_DEBUG").is_ok() {
                eprintln!("Matching '{}' (size: {}, line: {}):", decl1.name, decl1.size, decl1.line);
                for (name, sim) in &all_similarities {
                    eprintln!("  -> '{}': {:.1}%", name, sim * 100.0);
                }
                if should_match && best_match.is_some() {
                    let i2 = best_match.unwrap();
                    let decl2 = &decls2[i2];
                    eprintln!("  Best match: '{}' at {:.1}% (gap: {:.1}%)", 
                             decl2.name, best_similarity * 100.0, 
                             (best_similarity - second_best_similarity) * 100.0);
                } else {
                    eprintln!("  No match found (best: {:.1}%, second: {:.1}%, gap: {:.1}%)", 
                             best_similarity * 100.0, second_best_similarity * 100.0,
                             (best_similarity - second_best_similarity) * 100.0);
                }
                eprintln!();
            }
            
            if should_match && best_match.is_some() {
                let i2 = best_match.unwrap();
                matched1[*i1] = true;
                matched2[i2] = true;
                matches.push((*i1, i2));
                
                let decl2 = &decls2[i2];
                
                // Add to report if enabled
                if let (Some(ref mut builder), Some(ref breakdown)) = 
                    (&mut report_builder, &best_evidence_breakdown) {
                    let detail = MatchDetail {
                        func1: self.create_function_info(decl1, source1),
                        func2: self.create_function_info(decl2, source2),
                        final_score: best_similarity,
                        confidence_level: if best_similarity >= 0.8 { "high" } 
                                        else if best_similarity >= 0.6 { "medium" } 
                                        else { "low" }.to_string(),
                        evidence: breakdown.clone(),
                        matching_rationale: format!("Score: {:.2}, Evidence: {} pieces", 
                                                   best_similarity, best_evidence_count),
                    };
                    builder.add_match(detail);
                }
                
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
                if best_similarity < 0.95 && decl1.signature != decl2.signature {
                    changes.push(Change {
                        change_type: ChangeType::Modification,
                        location1: Some(self.create_location(decl1.node, source1)),
                        location2: Some(self.create_location(decl2.node, source2)),
                        description: format!("{} '{}' structure changed (similarity: {:.1}%)", 
                            self.kind_to_string(&decl1.kind), decl1.name, best_similarity * 100.0),
                        structural_path: format!("global.{}", decl1.name),
                    });
                }
            }
        }
        
        // Report unmatched declarations
        for (i, decl1) in decls1.iter().enumerate() {
            if !matched1[i] {
                // Add to report if enabled
                if let Some(ref mut builder) = report_builder {
                    // Find best candidates that weren't matched
                    let mut candidates_info = Vec::new();
                    for (j, decl2) in decls2.iter().enumerate() {
                        if !matched2[j] && decl1.kind == decl2.kind {
                            let (score, evidence_count) = if self.use_fingerprints {
                                if let (Some(ref fp1), Some(ref fp2), Some(ref s)) = 
                                    (&decl1.fingerprint, &decl2.fingerprint, &scorer) {
                                    calculate_fingerprint_similarity(fp1, fp2, s)
                                } else {
                                    (self.calculate_declaration_similarity(decl1, decl2, source1, source2), 0)
                                }
                            } else {
                                (self.calculate_declaration_similarity(decl1, decl2, source1, source2), 0)
                            };
                            
                            if score > 0.3 {
                                // Calculate missing evidence
                                let mut missing_evidence = Vec::new();
                                if let (Some(ref fp1), Some(ref fp2)) = (&decl1.fingerprint, &decl2.fingerprint) {
                                    // Find unique strings in fp1 that would help matching
                                    for s in &fp1.strings {
                                        if !fp2.strings.iter().any(|s2| s2.value == s.value) && s.value.len() > 5 {
                                            missing_evidence.push(format!("string: '{}'", s.value));
                                        }
                                    }
                                }
                                
                                candidates_info.push(CandidateInfo {
                                    func: self.create_function_info(decl2, source2),
                                    score,
                                    evidence_count,
                                    missing_evidence: missing_evidence.into_iter().take(3).collect(),
                                });
                            }
                        }
                    }
                    
                    candidates_info.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
                    
                    builder.add_non_match(NonMatchDetail {
                        func: self.create_function_info(decl1, source1),
                        from_file: 1,
                        best_candidates: candidates_info.into_iter().take(3).collect(),
                        why_no_match: "No sufficient evidence for matching".to_string(),
                    });
                }
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
                // Add to report if enabled
                if let Some(ref mut builder) = report_builder {
                    builder.add_non_match(NonMatchDetail {
                        func: self.create_function_info(decl2, source2),
                        from_file: 2,
                        best_candidates: Vec::new(), // Already checked above
                        why_no_match: "New function in file 2".to_string(),
                    });
                }
                changes.push(Change {
                    change_type: ChangeType::Addition,
                    location1: None,
                    location2: Some(self.create_location(decl2.node, source2)),
                    description: format!("Added {} '{}'", self.kind_to_string(&decl2.kind), decl2.name),
                    structural_path: format!("global.{}", decl2.name),
                });
            }
        }
        
        // Generate and save report if enabled
        if let Some(builder) = report_builder {
            let report = builder.build(decls1.len(), decls2.len());
            
            if let Some(ref path) = self.report_path {
                let report_md = generate_markdown_report(&report);
                if let Err(e) = std::fs::write(path, report_md) {
                    eprintln!("Failed to write matching report: {}", e);
                }
                
                // Also save JSON for LLM analysis
                let json_path = path.replace(".md", ".json");
                if let Ok(json) = serde_json::to_string_pretty(&report) {
                    let _ = std::fs::write(json_path, json);
                }
            } else {
                // Print to stderr if no path specified
                eprintln!("\n{}", generate_markdown_report(&report));
            }
        }
        
        eprintln!("\nMatching statistics:");
        eprintln!("  Total candidates examined: {}", total_candidates);
        eprintln!("  Full similarity calculations: {}", total_similarity_checks);
        eprintln!("  Average candidates per declaration: {:.1}", 
                 total_candidates as f64 / sorted1.len() as f64);
        
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
    
    fn should_match(&self, best_similarity: f64, second_best_similarity: f64, size: usize) -> bool {
        // Always match very high similarities
        if best_similarity >= 0.85 {
            return true;
        }
        
        // Calculate the gap to the next best match
        let gap = best_similarity - second_best_similarity;
        
        // For small functions, require higher similarity or bigger gap
        if size < 10 {
            // Small functions need either high similarity or large gap
            return best_similarity >= 0.7 || (best_similarity >= 0.5 && gap >= 0.3);
        }
        
        // For medium functions, be more lenient
        if size < 50 {
            // Medium functions: accept if reasonable similarity with good gap
            return best_similarity >= 0.5 || (best_similarity >= 0.35 && gap >= 0.25);
        }
        
        // For large functions, even more lenient
        // Large functions: accept lower similarity if there's a clear gap
        best_similarity >= 0.4 || (best_similarity >= 0.3 && gap >= 0.2)
    }
    
    fn should_match_with_evidence(&self, score: f64, evidence_count: usize, _size: usize) -> bool {
        // Evidence-based matching for fingerprints
        // Lower thresholds when we have some evidence
        match evidence_count {
            0 => false,
            1 => score >= 0.6,  // Single evidence with good score
            2 => score >= 0.45,  // Two pieces of evidence (lowered from 0.5)
            3..=4 => score >= 0.4,  // Several pieces
            _ => score >= 0.35,  // Many pieces of evidence
        }
    }
    
    fn create_evidence_breakdown(&self, decl1: &Declaration, decl2: &Declaration, 
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
    
    fn create_function_info(&self, decl: &Declaration, source: &str) -> FunctionInfo {
        let lines: Vec<&str> = source.lines().collect();
        let first_line = if decl.line > 0 && decl.line <= lines.len() {
            lines[decl.line - 1].to_string()
        } else {
            String::new()
        };
        
        FunctionInfo {
            name: decl.name.clone(),
            line: decl.line,
            size: decl.size,
            signature: decl.signature.clone(),
            first_line,
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
    
    pub fn print_summary(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf, 
                         source1: &str, source2: &str) {
        println!("--- {}", file1.display());
        println!("+++ {}", file2.display());
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!("Matched declarations: {}/{} vs {}", 
                 result.matched_declarations, result.total_declarations1, result.total_declarations2);
        
        // Calculate and print line statistics
        let (lines_added, lines_removed, total_diff) = self.calculate_line_statistics(result, source1, source2);
        println!("Diff size: {} lines (+{} -{})", total_diff, lines_added, lines_removed);
        
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
        if !renames.is_empty() && std::env::var("ASTDIFF_SHOW_RENAMES").is_ok() {
            println!("=== Renamed Functions (set ASTDIFF_SHOW_RENAMES to see) ===");
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
                             canonical1: Option<&str>, canonical2: Option<&str>, source1: &str, source2: &str) -> Result<()> {
        println!("--- {}", file1.display());
        println!("+++ {}", file2.display());
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!("Matched declarations: {}/{} vs {}", 
                 result.matched_declarations, result.total_declarations1, result.total_declarations2);
        
        // Calculate and print line statistics
        let (lines_added, lines_removed, total_diff) = self.calculate_line_statistics(result, source1, source2);
        println!("Diff size: {} lines (+{} -{})", total_diff, lines_added, lines_removed);
        
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
        println!("Diff size: {} lines (+{} -{})", total_diff, lines_added, lines_removed);
        
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
    
    pub fn print_compact(&self, result: &DiffResult, file1: &std::path::PathBuf, file2: &std::path::PathBuf,
                         source1: &str, source2: &str) {
        println!("--- {}", file1.display());
        println!("+++ {}", file2.display());
        println!("Structural similarity: {:.1}%", result.similarity * 100.0);
        println!("Matched declarations: {}/{} vs {}", 
                 result.matched_declarations, result.total_declarations1, result.total_declarations2);
        
        // Calculate and print line statistics
        let (lines_added, lines_removed, total_diff) = self.calculate_line_statistics(result, source1, source2);
        println!("Diff size: {} lines (+{} -{})", total_diff, lines_added, lines_removed);
        
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
        
        // Show structural changes
        if !structural_changes.is_empty() {
            println!("\n=== Modified Functions ===");
            for change in &structural_changes {
                if let (Some(loc1), Some(loc2)) = (&change.location1, &change.location2) {
                    println!("\n@@@ {}", change.description);
                    println!("\n--- {}:{} (before)", file1.display(), loc1.line);
                    println!("+++ {}:{} (after)", file2.display(), loc2.line);
                }
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
    
    fn match_declarations_parallel(&self, decls1: &[Declaration], decls2: &[Declaration], source1: &str, source2: &str) 
        -> (Vec<(usize, usize)>, Vec<Change>) {
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
        // Different kinds = low similarity
        if decl1.kind != decl2.kind {
            return 0.0;
        }
        
        // For imports and exports, use signature similarity
        if matches!(decl1.kind, DeclarationKind::Import | DeclarationKind::Export) {
            return if decl1.signature == decl2.signature { 1.0 } else { 0.3 };
        }
        
        // Quick size check - if sizes are too different, skip expensive set operations
        let size1 = decl1.structural_hashes.len();
        let size2 = decl2.structural_hashes.len();
        
        if size1 == 0 && size2 == 0 {
            // Both are empty (e.g., simple variables with no initialization)
            return if decl1.signature == decl2.signature { 1.0 } else { 0.5 };
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
        
        // Calculate similarity based on structural hash intersection
        let intersection: HashSet<_> = decl1.structural_hashes.intersection(&decl2.structural_hashes).cloned().collect();
        let union: HashSet<_> = decl1.structural_hashes.union(&decl2.structural_hashes).cloned().collect();
        
        intersection.len() as f64 / union.len() as f64
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