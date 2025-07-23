use std::collections::HashMap;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use crate::canonicalizer::Canonicalizer;
use tree_sitter::Tree;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingEntry {
    pub position: String,           // LINE:COL
    pub entry_type: String,         // func, param, var, ref
    pub scope: String,              // global, func_a, etc.
    pub original: String,           // original identifier
    pub canonical: String,          // fn_1, param_1, etc.
    pub new_name: String,           // user-editable semantic name
    pub context: String,            // code context for understanding
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
        output.push_str("# LINE:COL TYPE SCOPE ORIGINAL CANONICAL NEW_NAME CONTEXT\n");
        
        // Collect all mappings with context
        let entries = self.collect_mapping_entries(tree)?;
        
        // Sort by position and deduplicate identical entries
        let mut sorted_entries = entries;
        sorted_entries.sort_by(|a, b| {
            let a_parts: Vec<_> = a.position.split(':').collect();
            let b_parts: Vec<_> = b.position.split(':').collect();
            
            let a_line: usize = a_parts[0].parse().unwrap_or(0);
            let a_col: usize = a_parts[1].parse().unwrap_or(0);
            let b_line: usize = b_parts[0].parse().unwrap_or(0);
            let b_col: usize = b_parts[1].parse().unwrap_or(0);
            
            (a_line, a_col).cmp(&(b_line, b_col))
        });
        
        // Remove exact duplicates (same position and content)
        sorted_entries.dedup_by(|a, b| {
            a.position == b.position && a.original == b.original && a.canonical == b.canonical
        });
        
        // Format entries
        for entry in sorted_entries {
            output.push_str(&format!(
                "{} {} {} {} {} {} \"{}\"\n",
                entry.position,
                entry.entry_type,
                entry.scope,
                entry.original,
                entry.canonical,
                entry.new_name,
                entry.context
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
            if parts.len() >= 6 {
                let canonical = parts[4];
                let new_name = parts[5];
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
    
    fn collect_mapping_entries(&self, tree: &Tree) -> Result<Vec<MappingEntry>> {
        let mut entries = Vec::new();
        
        // Reuse canonicalizer's identifier extraction to avoid duplicate traversal
        let canonicalizer_identifiers = self.canonicalizer.extract_all_identifiers(tree.root_node(), &self.source);
        
        // Convert canonicalizer identifiers to mapping entries
        for canonical_id in canonicalizer_identifiers {
            if let Some(canonical_name) = self.canonicalizer.find_canonical_name(&canonical_id.text, &canonical_id.scope_id) {
                let position = format!("{}:{}", canonical_id.node.start_position().row + 1, canonical_id.node.start_position().column + 1);
                
                let entry_type = self.determine_entry_type_from_canonical_identifier(&canonical_id);
                let context = self.extract_context_from_canonical_identifier(&canonical_id);
                
                entries.push(MappingEntry {
                    position,
                    entry_type,
                    scope: canonical_id.scope_id.clone(),
                    original: canonical_id.text.clone(),
                    canonical: canonical_name.clone(),
                    new_name: canonical_name, // Initially same as canonical
                    context,
                });
            }
        }
        
        Ok(entries)
    }
    
    
    fn determine_entry_type_from_canonical_identifier(&self, identifier: &crate::canonicalizer::IdentifierInfo) -> String {
        if let Some(parent) = identifier.node.parent() {
            match parent.kind() {
                "variable_declarator" => {
                    if parent.child_by_field_name("name") == Some(identifier.node) {
                        "var".to_string()
                    } else {
                        "ref".to_string()
                    }
                }
                "formal_parameters" => "param".to_string(),
                "function_declaration" | "function_expression" => {
                    if parent.child_by_field_name("name") == Some(identifier.node) {
                        "func".to_string()
                    } else {
                        "ref".to_string()
                    }
                }
                _ => "ref".to_string(),
            }
        } else {
            "ref".to_string()
        }
    }
    
    fn extract_context_from_canonical_identifier(&self, identifier: &crate::canonicalizer::IdentifierInfo) -> String {
        let lines: Vec<&str> = self.source.lines().collect();
        let line_num = identifier.node.start_position().row;
        if line_num < lines.len() {
            let line = lines[line_num];
            line.trim().to_string()
        } else {
            String::new()
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
        canonicalizer.canonicalize().unwrap();
        
        let generator = MappingGenerator::new(canonicalizer, source.to_string());
        let mapping_file = generator.generate_mapping_file(&tree).unwrap();
        
        assert!(mapping_file.contains("LINE:COL TYPE SCOPE ORIGINAL CANONICAL NEW_NAME CONTEXT"));
        assert!(mapping_file.contains("add"));
        assert!(mapping_file.contains("fn_1"));
    }
}