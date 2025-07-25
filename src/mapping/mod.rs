use std::collections::HashMap;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use crate::canonicalizer::Canonicalizer;
use tree_sitter::Tree;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingEntry {
    pub first_line: usize,
    pub first_col: usize,
    pub last_line: usize,
    pub last_col: usize,
    pub entry_type: String,         // func, param, var
    pub original: String,           // original identifier
    pub canonical: String,          // fn_1, param_1, etc.
    pub new_name: String,           // user-editable semantic name
}

pub struct MappingGenerator {
    canonicalizer: Canonicalizer,
    source: String,
}

impl MappingGenerator {
    pub fn new(canonicalizer: Canonicalizer, source: String) -> Self {
        Self {
            canonicalizer,
            source,
        }
    }
    
    pub fn generate_mapping_file(&self, tree: &Tree) -> Result<String> {
        let mut output = String::new();
        
        // Header
        output.push_str("# FIRST LAST TYPE CANONICAL NEW\n");
        
        // Collect all identifiers and group by canonical name
        let mut identifier_groups: HashMap<String, Vec<crate::canonicalizer::IdentifierInfo>> = HashMap::new();
        let canonicalizer_identifiers = self.canonicalizer.extract_all_identifiers(tree.root_node(), &self.source);
        
        for identifier in canonicalizer_identifiers {
            if let Some(canonical_name) = self.canonicalizer.find_canonical_name(&identifier.text, &identifier.scope_id) {
                identifier_groups.entry(canonical_name).or_insert_with(Vec::new).push(identifier);
            }
        }
        
        // Create entries with first and last positions
        let mut entries = Vec::new();
        for (canonical, identifiers) in identifier_groups {
            if identifiers.is_empty() {
                continue;
            }
            
            // Find first and last positions
            let first = identifiers.iter().min_by_key(|id| id.node.start_byte()).unwrap();
            let last = identifiers.iter().max_by_key(|id| id.node.start_byte()).unwrap();
            
            // Determine type from canonical name prefix
            let entry_type = if canonical.starts_with("fn_") {
                "func".to_string()
            } else if canonical.starts_with("param_") {
                "param".to_string()
            } else {
                "var".to_string()
            };
            
            entries.push(MappingEntry {
                first_line: first.node.start_position().row + 1,
                first_col: first.node.start_position().column + 1,
                last_line: last.node.start_position().row + 1,
                last_col: last.node.start_position().column + 1,
                entry_type,
                original: identifiers[0].text.clone(),
                canonical: canonical.clone(),
                new_name: canonical,
            });
        }
        
        // Sort by first occurrence
        entries.sort_by_key(|e| (e.first_line, e.first_col));
        
        // Format entries
        for entry in entries {
            output.push_str(&format!(
                "{}:{} {}:{} {} {} {}\n",
                entry.first_line,
                entry.first_col,
                entry.last_line,
                entry.last_col,
                entry.entry_type,
                entry.canonical,
                entry.new_name
            ));
        }
        
        Ok(output)
    }
    
    pub fn parse_mapping_file(content: &str) -> Result<HashMap<String, String>> {
        let mut mappings = HashMap::new();
        
        for line in content.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                // Format: first:col last:col type canonical new
                let canonical = parts[3];
                let new_name = parts[4];
                mappings.insert(canonical.to_string(), new_name.to_string());
            }
        }
        
        Ok(mappings)
    }
    
    pub fn apply_mappings(&self, tree: &Tree, mappings: HashMap<String, String>) -> Result<String> {
        let mut output = String::new();
        let mut last_end = 0;
        
        let identifiers = self.canonicalizer.extract_all_identifiers(tree.root_node(), &self.source);
        
        for identifier in identifiers {
            let start = identifier.node.start_byte();
            let end = identifier.node.end_byte();
            
            // Skip if this identifier starts before our last position
            if start < last_end {
                continue;
            }
            
            output.push_str(&self.source[last_end..start]);
            
            if let Some(canonical_name) = self.canonicalizer.find_canonical_name(&identifier.text, &identifier.scope_id) {
                if let Some(new_name) = mappings.get(&canonical_name) {
                    output.push_str(new_name);
                } else {
                    output.push_str(&canonical_name);
                }
            } else {
                output.push_str(&self.source[start..end]);
            }
            
            last_end = end;
        }
        
        output.push_str(&self.source[last_end..]);
        
        Ok(output)
    }
    
    #[allow(dead_code)]
    fn determine_entry_type(&self, identifier: &crate::canonicalizer::IdentifierInfo) -> String {
        if let Some(parent) = identifier.node.parent() {
            match parent.kind() {
                "variable_declarator" => "var".to_string(),
                "formal_parameters" => "param".to_string(),
                "function_declaration" | "function_expression" => {
                    if parent.child_by_field_name("name") == Some(identifier.node) {
                        "func".to_string()
                    } else {
                        "var".to_string() // Default to var for other cases
                    }
                }
                _ => {
                    // Determine type from the canonical name pattern
                    if let Some(canonical) = self.canonicalizer.find_canonical_name(&identifier.text, &identifier.scope_id) {
                        if canonical.starts_with("param_") {
                            "param".to_string()
                        } else if canonical.starts_with("fn_") {
                            "func".to_string()
                        } else {
                            "var".to_string()
                        }
                    } else {
                        "var".to_string()
                    }
                }
            }
        } else {
            "var".to_string()
        }
    }
    
    
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::JsParser;
    use crate::scope::ScopeAnalyzer;
    use crate::canonicalizer::Canonicalizer;
    
    #[test]
    fn test_mapping_generation() {
        let source = "function add(a, b) { return a + b; }";
        
        let mut parser = JsParser::new().unwrap();
        let tree = parser.parse(source).unwrap();
        
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(tree.root_node(), source).unwrap();
        
        let mut canonicalizer = Canonicalizer::new(analyzer);
        canonicalizer.canonicalize(&tree, source).unwrap();
        
        let generator = MappingGenerator::new(canonicalizer, source.to_string());
        let mapping_file = generator.generate_mapping_file(&tree).unwrap();
        
        assert!(mapping_file.contains("# FIRST LAST TYPE CANONICAL NEW"));
        assert!(mapping_file.contains("fn_"));
    }
}