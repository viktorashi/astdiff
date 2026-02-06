use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StringFingerprint {
    pub value: String,
    pub context: StringContext,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StringContext {
    ErrorMessage,    // Contains "error", "fail", "exception"
    ConfigKey,       // Common config patterns
    ApiEndpoint,     // URLs, paths with /
    FilePath,        // Contains ~, /, \, .ext
    CommandName,     // Kebab-case strings
    Regular,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConstantValue {
    Number(i64),          // Integers only for now
    Float(String),        // Store as string to avoid precision issues
    Regex(String),        // Regex patterns
    Duration(u64),        // setTimeout/setInterval values
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConstantFingerprint {
    pub value: ConstantValue,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ApiCallFingerprint {
    pub object: Option<String>,   // "process.env", "fs", etc
    pub method: String,           // "existsSync", "readFileSync"
    pub first_arg: Option<String>, // If it's a literal
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FunctionFingerprint {
    pub strings: Vec<StringFingerprint>,
    pub constants: Vec<ConstantFingerprint>,
    pub api_calls: Vec<ApiCallFingerprint>,
    pub size: usize,
}

/// Represents a change to a string constant between two versions
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StringChange {
    Added(StringFingerprint),
    Removed(StringFingerprint),
    Modified {
        old: StringFingerprint,
        new: StringFingerprint,
        similarity: f64,
    },
}

/// Categorizes string importance based on length
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StringImportance {
    SystemPrompt,  // > 500 chars - very likely a prompt
    LongText,      // 100-500 chars - important
    Medium,        // 20-100 chars
    Short,         // < 20 chars
}

impl StringFingerprint {
    pub fn importance(&self) -> StringImportance {
        match self.value.len() {
            0..=19 => StringImportance::Short,
            20..=99 => StringImportance::Medium,
            100..=499 => StringImportance::LongText,
            _ => StringImportance::SystemPrompt,
        }
    }
}

/// Summary of string changes between two matched functions
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StringDiff {
    pub changes: Vec<StringChange>,
    pub important_changes: Vec<StringChange>,  // Strings > 100 chars
    pub added_count: usize,
    pub removed_count: usize,
    pub modified_count: usize,
}

impl StringDiff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn has_important_changes(&self) -> bool {
        !self.important_changes.is_empty()
    }
}

/// Compute the diff between strings in two function fingerprints
pub fn compute_string_diff(fp1: &FunctionFingerprint, fp2: &FunctionFingerprint) -> StringDiff {
    use strsim::normalized_levenshtein;

    let mut changes = Vec::new();
    let mut used_from_fp2: HashSet<usize> = HashSet::new();

    // Find exact matches first (these are not changes)
    let exact_matches: HashSet<String> = fp1.strings.iter()
        .filter(|s1| fp2.strings.iter().any(|s2| s1.value == s2.value))
        .map(|s| s.value.clone())
        .collect();

    // For each string in fp1 not in exact matches, look for fuzzy match or mark as removed
    for s1 in &fp1.strings {
        if exact_matches.contains(&s1.value) {
            continue;
        }

        // Try to find a fuzzy match in fp2
        // Only consider strings of similar length (within 2x) for fuzzy matching
        let best_match = fp2.strings.iter()
            .enumerate()
            .filter(|(i, _)| !used_from_fp2.contains(i))
            .filter(|(_, s2)| !exact_matches.contains(&s2.value))
            .filter(|(_, s2)| {
                let len_ratio = s1.value.len().max(s2.value.len()) as f64
                    / s1.value.len().min(s2.value.len()).max(1) as f64;
                len_ratio < 2.0  // Within 2x length
            })
            .map(|(i, s2)| {
                let similarity = normalized_levenshtein(&s1.value, &s2.value);
                (i, s2, similarity)
            })
            .filter(|(_, _, sim)| *sim > 0.6)  // Minimum 60% similarity
            .max_by(|(_, _, sim1), (_, _, sim2)| sim1.partial_cmp(sim2).unwrap());

        if let Some((idx, s2, similarity)) = best_match {
            used_from_fp2.insert(idx);
            changes.push(StringChange::Modified {
                old: s1.clone(),
                new: s2.clone(),
                similarity,
            });
        } else {
            changes.push(StringChange::Removed(s1.clone()));
        }
    }

    // Remaining strings in fp2 are additions
    for (i, s2) in fp2.strings.iter().enumerate() {
        if !exact_matches.contains(&s2.value) && !used_from_fp2.contains(&i) {
            changes.push(StringChange::Added(s2.clone()));
        }
    }

    // Separate important changes (strings > 100 chars)
    let important_changes: Vec<StringChange> = changes.iter()
        .filter(|c| match c {
            StringChange::Added(s) => s.value.len() >= 100,
            StringChange::Removed(s) => s.value.len() >= 100,
            StringChange::Modified { old, new, .. } =>
                old.value.len() >= 100 || new.value.len() >= 100,
        })
        .cloned()
        .collect();

    // Count by type
    let (added_count, removed_count, modified_count) = changes.iter()
        .fold((0, 0, 0), |(a, r, m), c| {
            match c {
                StringChange::Added(_) => (a + 1, r, m),
                StringChange::Removed(_) => (a, r + 1, m),
                StringChange::Modified { .. } => (a, r, m + 1),
            }
        });

    StringDiff {
        changes,
        important_changes,
        added_count,
        removed_count,
        modified_count,
    }
}

/// Like compute_string_diff, but normalizes fp2's strings using the rename map first.
/// This eliminates false positives from minifier variable name changes in template literals.
pub fn compute_string_diff_normalized(
    fp1: &FunctionFingerprint,
    fp2: &FunctionFingerprint,
    rename_map: &HashMap<String, String>,
) -> StringDiff {
    if rename_map.is_empty() {
        return compute_string_diff(fp1, fp2);
    }

    // Normalize fp2's string values using the rename map
    let normalized_fp2 = FunctionFingerprint {
        strings: fp2.strings.iter().map(|s| {
            StringFingerprint {
                value: normalize_string_with_renames(&s.value, rename_map),
                context: s.context.clone(),
            }
        }).collect(),
        constants: fp2.constants.clone(),
        api_calls: fp2.api_calls.clone(),
        size: fp2.size,
    };

    compute_string_diff(fp1, &normalized_fp2)
}

/// Replace renamed identifiers in a string value (e.g., template literal text).
/// Uses scan-and-lookup: extracts identifiers from the string and looks each up in the map.
/// This is O(string_length) instead of O(string_length * map_size).
pub fn normalize_string_with_renames(s: &str, rename_map: &HashMap<String, String>) -> String {
    let mut output = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' {
            // Start of potential identifier
            let start = i;
            let mut end = i + ch.len_utf8();
            while let Some(&(j, next_ch)) = chars.peek() {
                if next_ch.is_ascii_alphanumeric() || next_ch == '_' || next_ch == '$' {
                    end = j + next_ch.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let ident = &s[start..end];
            if let Some(old_name) = rename_map.get(ident) {
                output.push_str(old_name);
            } else {
                output.push_str(ident);
            }
        } else {
            output.push(ch);
        }
    }

    output
}

/// Normalize all short minified identifiers in a string to a canonical placeholder "_".
/// This catches local variables, function references, and module names that change between builds
/// but aren't in the top-level rename map.
pub fn normalize_minified_identifiers(s: &str) -> String {
    let mut output = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' {
            // Start of potential identifier — collect it
            let start = i;
            let mut end = i + ch.len_utf8();
            while let Some(&(j, next_ch)) = chars.peek() {
                if next_ch.is_ascii_alphanumeric() || next_ch == '_' || next_ch == '$' {
                    end = j + next_ch.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let ident = &s[start..end];
            if looks_minified(ident) {
                output.push('_');
            } else {
                output.push_str(ident);
            }
        } else {
            output.push(ch);
        }
    }

    output
}

/// All identifiers ≤4 chars are normalized. Since we apply this to BOTH old and new
/// strings symmetrically, real English words that are the same in both versions cancel
/// out (both become "_"). Only structural differences in longer identifiers survive.
fn looks_minified(s: &str) -> bool {
    s.len() <= 4
}

pub struct FingerprintExtractor<'a> {
    source: &'a str,
}

impl<'a> FingerprintExtractor<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { source }
    }
    
    pub fn extract_function_fingerprint(&self, node: Node) -> FunctionFingerprint {
        let mut strings = Vec::new();
        let mut constants = Vec::new();
        let mut api_calls = Vec::new();
        let mut node_count = 0;
        
        self.extract_fingerprints_recursive(node, &mut strings, &mut constants, &mut api_calls, &mut node_count);
        
        FunctionFingerprint {
            strings,
            constants,
            api_calls,
            size: node_count,
        }
    }
    
    fn extract_fingerprints_recursive(
        &self,
        node: Node,
        strings: &mut Vec<StringFingerprint>,
        constants: &mut Vec<ConstantFingerprint>,
        api_calls: &mut Vec<ApiCallFingerprint>,
        node_count: &mut usize,
    ) {
        *node_count += 1;
        
        match node.kind() {
            "string" | "template_string" => {
                if let Some(value) = self.extract_string_value(node) {
                    if value.len() > 2 && !value.chars().all(|c| c.is_whitespace()) {
                        let context = self.classify_string(&value);
                        strings.push(StringFingerprint { value, context });
                    }
                }
            }
            "number" => {
                if let Ok(num) = self.source[node.byte_range()].parse::<i64>() {
                    // Special handling for common timer values
                    if matches!(num, 100 | 200 | 500 | 1000 | 2000 | 5000) {
                        constants.push(ConstantFingerprint {
                            value: ConstantValue::Duration(num as u64),
                        });
                    } else if num > 100 || num < -100 {
                        // Only track non-trivial numbers
                        constants.push(ConstantFingerprint {
                            value: ConstantValue::Number(num),
                        });
                    }
                } else if let Ok(_) = self.source[node.byte_range()].parse::<f64>() {
                    constants.push(ConstantFingerprint {
                        value: ConstantValue::Float(self.source[node.byte_range()].to_string()),
                    });
                }
            }
            "regex" => {
                let regex_str = &self.source[node.byte_range()];
                constants.push(ConstantFingerprint {
                    value: ConstantValue::Regex(regex_str.to_string()),
                });
            }
            "call_expression" => {
                if let Some(api_call) = self.extract_api_call(node) {
                    api_calls.push(api_call);
                }
            }
            _ => {}
        }
        
        // Recurse into children
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if !matches!(child.kind(), "comment") {
                    self.extract_fingerprints_recursive(child, strings, constants, api_calls, node_count);
                }
                
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    
    fn extract_string_value(&self, node: Node) -> Option<String> {
        let text = &self.source[node.byte_range()];
        // Remove quotes
        if text.len() >= 2 {
            let inner = &text[1..text.len()-1];
            // Basic unescape for common cases
            Some(inner.replace("\\\"", "\"").replace("\\'", "'").replace("\\\\", "\\"))
        } else {
            None
        }
    }
    
    fn classify_string(&self, s: &str) -> StringContext {
        let lower = s.to_lowercase();
        
        // Error messages
        if lower.contains("error") || lower.contains("fail") || lower.contains("exception") 
            || lower.contains("invalid") || lower.contains("unable") {
            return StringContext::ErrorMessage;
        }
        
        // File paths
        if s.contains("~/") || s.contains("/.") || s.contains("\\") 
            || s.ends_with(".js") || s.ends_with(".json") || s.ends_with(".md") {
            return StringContext::FilePath;
        }
        
        // API endpoints
        if s.starts_with("/") && (s.contains("/api") || s.chars().filter(|&c| c == '/').count() > 1) {
            return StringContext::ApiEndpoint;
        }
        
        // Command names (kebab-case)
        if s.contains("-") && s.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return StringContext::CommandName;
        }
        
        // Config keys
        if s.chars().all(|c| c.is_alphanumeric() || c == '_') && s.len() > 5 {
            return StringContext::ConfigKey;
        }
        
        StringContext::Regular
    }
    
    fn extract_api_call(&self, node: Node) -> Option<ApiCallFingerprint> {
        let func_node = node.child_by_field_name("function")?;
        
        let (object, method) = match func_node.kind() {
            "member_expression" => {
                let obj = func_node.child_by_field_name("object")?;
                let prop = func_node.child_by_field_name("property")?;
                
                let obj_text = &self.source[obj.byte_range()];
                let method_text = &self.source[prop.byte_range()];
                
                // Special handling for nested member expressions like process.env
                let full_obj = if obj.kind() == "member_expression" {
                    self.get_full_member_path(obj)
                } else {
                    obj_text.to_string()
                };
                
                (Some(full_obj), method_text.to_string())
            }
            "identifier" => {
                let method = &self.source[func_node.byte_range()];
                (None, method.to_string())
            }
            _ => return None,
        };
        
        // Skip minified method names
        if method.len() <= 2 && !matches!(method.as_str(), "fs" | "os") {
            return None;
        }
        
        // Extract first argument if it's a literal
        let first_arg = node.child_by_field_name("arguments")
            .and_then(|args| {
                let mut cursor = args.walk();
                cursor.goto_first_child();
                loop {
                    let child = cursor.node();
                    match child.kind() {
                        "string" => return self.extract_string_value(child),
                        "number" => return Some(self.source[child.byte_range()].to_string()),
                        "," | "(" | ")" => {},
                        _ => return None,
                    }
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
                None
            });
        
        Some(ApiCallFingerprint { object, method, first_arg })
    }
    
    fn get_full_member_path(&self, node: Node) -> String {
        let mut parts = Vec::new();
        let mut current = node;
        
        loop {
            if current.kind() == "member_expression" {
                if let Some(prop) = current.child_by_field_name("property") {
                    parts.push(self.source[prop.byte_range()].to_string());
                }
                if let Some(obj) = current.child_by_field_name("object") {
                    if obj.kind() == "member_expression" {
                        current = obj;
                        continue;
                    } else {
                        parts.push(self.source[obj.byte_range()].to_string());
                        break;
                    }
                }
            } else {
                parts.push(self.source[current.byte_range()].to_string());
                break;
            }
        }
        
        parts.reverse();
        parts.join(".")
    }
}

#[derive(Debug)]
pub struct RarityScorer {
    string_counts: HashMap<String, usize>,
    constant_counts: HashMap<ConstantValue, usize>,
    api_counts: HashMap<String, usize>,
}

impl RarityScorer {
    pub fn new() -> Self {
        Self {
            string_counts: HashMap::new(),
            constant_counts: HashMap::new(),
            api_counts: HashMap::new(),
        }
    }
    
    pub fn add_fingerprint(&mut self, fp: &FunctionFingerprint) {
        for s in &fp.strings {
            *self.string_counts.entry(s.value.clone()).or_insert(0) += 1;
        }
        
        for c in &fp.constants {
            *self.constant_counts.entry(c.value.clone()).or_insert(0) += 1;
        }
        
        for api in &fp.api_calls {
            let key = format!("{:?}::{}", api.object, api.method);
            *self.api_counts.entry(key).or_insert(0) += 1;
        }
    }
    
    pub fn score_string(&self, s: &str) -> f64 {
        match self.string_counts.get(s) {
            Some(1) => 1.0,
            Some(2) => 0.7,
            Some(3..=5) => 0.4,
            Some(_) => 0.1,
            None => 0.0,
        }
    }
    
    pub fn score_constant(&self, c: &ConstantValue) -> f64 {
        match self.constant_counts.get(c) {
            Some(1) => 1.0,
            Some(2) => 0.6,
            Some(3..=5) => 0.3,
            Some(_) => 0.1,
            None => 0.0,
        }
    }
    
    pub fn score_api_call(&self, api: &ApiCallFingerprint) -> f64 {
        let key = format!("{:?}::{}", api.object, api.method);
        match self.api_counts.get(&key) {
            Some(1..=3) => 0.8,
            Some(4..=10) => 0.5,
            Some(_) => 0.2,
            None => 0.0,
        }
    }
}

pub fn calculate_fingerprint_similarity(
    fp1: &FunctionFingerprint,
    fp2: &FunctionFingerprint,
    scorer: &RarityScorer,
) -> (f64, usize) {
    let mut total_score = 0.0;
    let mut evidence_count = 0;
    let mut matched_strings = HashSet::new();
    
    // Match strings (highest weight)
    for s1 in &fp1.strings {
        for s2 in &fp2.strings {
            if s1.value == s2.value && !matched_strings.contains(&s1.value) {
                matched_strings.insert(s1.value.clone());
                let rarity = scorer.score_string(&s1.value);
                let context_weight = match s1.context {
                    StringContext::ErrorMessage => 1.2,
                    StringContext::FilePath => 1.1,
                    StringContext::CommandName => 1.0,
                    StringContext::ConfigKey => 0.9,
                    StringContext::ApiEndpoint => 1.0,
                    StringContext::Regular => 0.7,
                };
                total_score += rarity * context_weight;
                evidence_count += 1;
            }
        }
    }
    
    // Match constants
    let mut matched_constants = HashSet::new();
    for c1 in &fp1.constants {
        for c2 in &fp2.constants {
            if c1.value == c2.value && !matched_constants.contains(&c1.value) {
                matched_constants.insert(c1.value.clone());
                let rarity = scorer.score_constant(&c1.value);
                total_score += rarity * 0.8;
                evidence_count += 1;
            }
        }
    }
    
    // Match API calls
    for api1 in &fp1.api_calls {
        for api2 in &fp2.api_calls {
            if api1.object == api2.object && api1.method == api2.method {
                let rarity = scorer.score_api_call(api1);
                // Bonus if first argument also matches
                let arg_bonus = if api1.first_arg == api2.first_arg && api1.first_arg.is_some() {
                    0.3
                } else {
                    0.0
                };
                total_score += rarity * 0.6 + arg_bonus;
                evidence_count += 1;
            }
        }
    }
    
    // Size compatibility factor
    let size_ratio = fp1.size.min(fp2.size) as f64 / fp1.size.max(fp2.size) as f64;
    let size_factor = if size_ratio > 0.7 { 1.0 } else { 0.8 + 0.2 * size_ratio };
    
    let final_score = if evidence_count > 0 {
        (total_score / evidence_count as f64) * size_factor
    } else {
        0.0
    };
    
    (final_score, evidence_count)
}