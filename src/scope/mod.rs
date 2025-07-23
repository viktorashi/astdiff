use std::collections::HashMap;
use anyhow::Result;
use tree_sitter::{Node, TreeCursor};

#[derive(Debug, Clone, PartialEq)]
pub enum ScopeType {
    Global,
    Function,
    Block,
    Module,
    Class,
    Method,
}

#[derive(Debug, Clone)]
pub struct Scope {
    pub id: String,
    pub scope_type: ScopeType,
    pub parent: Option<String>,
    pub depth: usize,
    pub variables: Vec<Variable>,
    pub functions: Vec<String>,
    pub children: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Variable {
    pub name: String,
    pub canonical_name: Option<String>,
    pub kind: VariableKind,
    pub declaration_line: usize,
    pub declaration_column: usize,
    pub is_hoisted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VariableKind {
    Var,
    Let,
    Const,
    Parameter,
    FunctionDeclaration,
    ClassDeclaration,
}

pub struct ScopeAnalyzer {
    scopes: HashMap<String, Scope>,
    current_scope_id: String,
    scope_counter: usize,
}

impl ScopeAnalyzer {
    pub fn new() -> Self {
        let mut scopes = HashMap::new();
        let global_scope = Scope {
            id: "global".to_string(),
            scope_type: ScopeType::Global,
            parent: None,
            depth: 0,
            variables: Vec::new(),
            functions: Vec::new(),
            children: Vec::new(),
        };
        scopes.insert("global".to_string(), global_scope);
        
        Self {
            scopes,
            current_scope_id: "global".to_string(),
            scope_counter: 0,
        }
    }
    
    pub fn analyze(&mut self, root: Node, source: &str) -> Result<()> {
        let mut cursor = root.walk();
        self.visit_node(&mut cursor, source)?;
        Ok(())
    }
    
    fn visit_node(&mut self, cursor: &mut TreeCursor, source: &str) -> Result<()> {
        let node = cursor.node();
        let mut scope_changed = false;
        let old_scope = self.current_scope_id.clone();
        
        
        match node.kind() {
            "function_declaration" | "function_expression" => {
                self.handle_function(node, source)?;
                scope_changed = true;
            }
            "arrow_function" => {
                self.handle_arrow_function(node, source)?;
                scope_changed = true;
            }
            "block_statement" => {
                if self.should_create_block_scope(node) {
                    self.handle_block_scope(node, source)?;
                    scope_changed = true;
                }
            }
            "class_declaration" => {
                self.handle_class(node, source)?;
                scope_changed = true;
            }
            "variable_declaration" | "lexical_declaration" => {
                self.handle_variable_declaration(node, source)?;
            }
            "for_in_statement" => {
                self.handle_for_in_statement(node, source)?;
                scope_changed = true;
            }
            "for_of_statement" => {
                self.handle_for_of_statement(node, source)?;
                scope_changed = true;
            }
            "catch_clause" => {
                self.handle_catch_clause(node, source)?;
                scope_changed = true;
            }
            "import_statement" => {
                self.handle_import_statement(node, source)?;
            }
            _ => {}
        }
        
        if cursor.goto_first_child() {
            loop {
                self.visit_node(cursor, source)?;
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
        
        if scope_changed {
            self.current_scope_id = old_scope;
        }
        
        Ok(())
    }
    
    fn handle_function(&mut self, node: Node, source: &str) -> Result<()> {
        let name_node = node.child_by_field_name("name");
        let name = name_node
            .map(|n| &source[n.byte_range()])
            .unwrap_or("anonymous");
        
        let function_scope_id = self.create_scope(ScopeType::Function, Some(name.to_string()));
        
        if let Some(name_node) = name_node {
            self.add_variable_to_current_scope(
                name.to_string(),
                VariableKind::FunctionDeclaration,
                name_node.start_position(),
                true,
            );
        }
        
        self.current_scope_id = function_scope_id;
        
        if let Some(params) = node.child_by_field_name("parameters") {
            self.handle_parameters(params, source)?;
        }
        
        Ok(())
    }
    
    fn handle_arrow_function(&mut self, node: Node, source: &str) -> Result<()> {
        let function_scope_id = self.create_scope(ScopeType::Function, None);
        self.current_scope_id = function_scope_id;
        
        if let Some(params) = node.child_by_field_name("parameters") {
            self.handle_parameters(params, source)?;
        }
        
        Ok(())
    }
    
    fn handle_parameters(&mut self, params_node: Node, source: &str) -> Result<()> {
        let mut cursor = params_node.walk();
        if cursor.goto_first_child() {
            loop {
                let node = cursor.node();
                match node.kind() {
                    "identifier" => {
                        let param_name = &source[node.byte_range()];
                        self.add_variable_to_current_scope(
                            param_name.to_string(),
                            VariableKind::Parameter,
                            node.start_position(),
                            false,
                        );
                    }
                    "object_pattern" | "array_pattern" | "assignment_pattern" => {
                        // Handle destructuring in parameters
                        self.handle_pattern(node, source, VariableKind::Parameter)?;
                    }
                    "rest_pattern" => {
                        // Handle rest parameters
                        if let Some(ident) = node.child(1) {
                            self.handle_pattern(ident, source, VariableKind::Parameter)?;
                        }
                    }
                    _ => {}
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        Ok(())
    }
    
    fn handle_block_scope(&mut self, _node: Node, _source: &str) -> Result<()> {
        let block_scope_id = self.create_scope(ScopeType::Block, None);
        self.current_scope_id = block_scope_id;
        Ok(())
    }
    
    fn handle_class(&mut self, node: Node, source: &str) -> Result<()> {
        let name_node = node.child_by_field_name("name");
        let name = name_node
            .map(|n| &source[n.byte_range()])
            .unwrap_or("anonymous");
        
        self.add_variable_to_current_scope(
            name.to_string(),
            VariableKind::ClassDeclaration,
            node.start_position(),
            false,
        );
        
        let class_scope_id = self.create_scope(ScopeType::Class, Some(name.to_string()));
        self.current_scope_id = class_scope_id;
        Ok(())
    }
    
    fn handle_variable_declaration(&mut self, node: Node, source: &str) -> Result<()> {
        let kind_str = node.child(0).map(|n| n.kind()).unwrap_or("");
        let kind = match kind_str {
            "var" => VariableKind::Var,
            "let" => VariableKind::Let,
            "const" => VariableKind::Const,
            _ => return Ok(()),
        };
        
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.kind() == "variable_declarator" {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        self.handle_pattern(name_node, source, kind)?;
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        
        Ok(())
    }
    
    fn handle_pattern(&mut self, node: Node, source: &str, kind: VariableKind) -> Result<()> {
        match node.kind() {
            "identifier" => {
                let var_name = &source[node.byte_range()];
                self.add_variable_to_current_scope(
                    var_name.to_string(),
                    kind,
                    node.start_position(),
                    kind == VariableKind::Var,
                );
            }
            "object_pattern" => {
                // Handle object destructuring: { a, b: c, ...rest }
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        match child.kind() {
                            "shorthand_property_identifier_pattern" => {
                                // { prop } - shorthand
                                // The identifier is the child itself in shorthand pattern
                                let var_name = &source[child.byte_range()];
                                self.add_variable_to_current_scope(
                                    var_name.to_string(),
                                    kind,
                                    child.start_position(),
                                    kind == VariableKind::Var,
                                );
                            }
                            "pair_pattern" => {
                                // { key: value } - renamed destructuring
                                if let Some(value) = child.child_by_field_name("value") {
                                    self.handle_pattern(value, source, kind)?;
                                }
                            }
                            "rest_pattern" => {
                                // { ...rest }
                                if let Some(ident) = child.child(1) {
                                    self.handle_pattern(ident, source, kind)?;
                                }
                            }
                            "{" | "}" | "," => {
                                // Skip punctuation
                            }
                            "object_pattern" | "array_pattern" => {
                                // Handle nested patterns
                                self.handle_pattern(child, source, kind)?;
                            }
                            _ => {}
                        }
                    }
                }
            }
            "array_pattern" => {
                // Handle array destructuring: [a, b, ...rest]
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        match child.kind() {
                            "identifier" => {
                                let var_name = &source[child.byte_range()];
                                self.add_variable_to_current_scope(
                                    var_name.to_string(),
                                    kind,
                                    child.start_position(),
                                    kind == VariableKind::Var,
                                );
                            }
                            "rest_pattern" => {
                                // [...rest]
                                if let Some(ident) = child.child(1) {
                                    self.handle_pattern(ident, source, kind)?;
                                }
                            }
                            "[" | "]" | "," => {
                                // Skip punctuation
                            }
                            _ => {
                                // Could be nested patterns
                                self.handle_pattern(child, source, kind)?;
                            }
                        }
                    }
                }
            }
            "assignment_pattern" => {
                // Handle default values: { a = 5 }
                if let Some(left) = node.child_by_field_name("left") {
                    self.handle_pattern(left, source, kind)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    
    fn handle_for_in_statement(&mut self, node: Node, source: &str) -> Result<()> {
        let block_scope_id = self.create_scope(ScopeType::Block, None);
        self.current_scope_id = block_scope_id;
        
        // Extract the loop variable from "left" field
        if let Some(left) = node.child_by_field_name("left") {
            self.handle_loop_variable(left, source)?;
        }
        
        Ok(())
    }
    
    fn handle_for_of_statement(&mut self, node: Node, source: &str) -> Result<()> {
        let block_scope_id = self.create_scope(ScopeType::Block, None);
        self.current_scope_id = block_scope_id;
        
        // Extract the loop variable from "left" field
        if let Some(left) = node.child_by_field_name("left") {
            self.handle_loop_variable(left, source)?;
        }
        
        Ok(())
    }
    
    fn handle_loop_variable(&mut self, node: Node, source: &str) -> Result<()> {
        match node.kind() {
            "variable_declaration" | "lexical_declaration" => {
                self.handle_variable_declaration(node, source)?;
            }
            "identifier" => {
                // Simple identifier in for-in/for-of
                let var_name = &source[node.byte_range()];
                self.add_variable_to_current_scope(
                    var_name.to_string(),
                    VariableKind::Let, // for-in/for-of variables are block-scoped
                    node.start_position(),
                    false,
                );
            }
            _ => {}
        }
        Ok(())
    }
    
    fn handle_catch_clause(&mut self, node: Node, source: &str) -> Result<()> {
        let catch_scope_id = self.create_scope(ScopeType::Block, None);
        self.current_scope_id = catch_scope_id;
        
        // Extract the parameter from catch clause
        if let Some(param) = node.child_by_field_name("parameter") {
            if param.kind() == "identifier" {
                let param_name = &source[param.byte_range()];
                self.add_variable_to_current_scope(
                    param_name.to_string(),
                    VariableKind::Parameter,
                    param.start_position(),
                    false,
                );
            }
            // TODO: Handle destructuring in catch parameters
        }
        
        Ok(())
    }
    
    fn handle_import_statement(&mut self, node: Node, source: &str) -> Result<()> {
        // Handle various import types
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                match child.kind() {
                    "import_clause" => {
                        self.handle_import_clause(child, source)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
    
    fn handle_import_clause(&mut self, node: Node, source: &str) -> Result<()> {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                match child.kind() {
                    "identifier" => {
                        // Default import
                        let import_name = &source[child.byte_range()];
                        self.add_variable_to_current_scope(
                            import_name.to_string(),
                            VariableKind::Const,
                            child.start_position(),
                            false,
                        );
                    }
                    "namespace_import" => {
                        // import * as name
                        if let Some(alias) = child.child_by_field_name("alias") {
                            let import_name = &source[alias.byte_range()];
                            self.add_variable_to_current_scope(
                                import_name.to_string(),
                                VariableKind::Const,
                                alias.start_position(),
                                false,
                            );
                        }
                    }
                    "named_imports" => {
                        // import { a, b as c }
                        for j in 0..child.child_count() {
                            if let Some(import_spec) = child.child(j) {
                                if import_spec.kind() == "import_specifier" {
                                    let name = if let Some(alias) = import_spec.child_by_field_name("alias") {
                                        &source[alias.byte_range()]
                                    } else if let Some(name) = import_spec.child_by_field_name("name") {
                                        &source[name.byte_range()]
                                    } else {
                                        continue;
                                    };
                                    
                                    self.add_variable_to_current_scope(
                                        name.to_string(),
                                        VariableKind::Const,
                                        import_spec.start_position(),
                                        false,
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
    
    fn should_create_block_scope(&self, node: Node) -> bool {
        if let Some(parent) = node.parent() {
            !matches!(
                parent.kind(),
                "function_declaration" | "function_expression" | "arrow_function" | "method_definition"
            )
        } else {
            true
        }
    }
    
    fn create_scope(&mut self, scope_type: ScopeType, name: Option<String>) -> String {
        self.scope_counter += 1;
        let scope_id = match (&scope_type, name.as_ref()) {
            (ScopeType::Function, Some(n)) => format!("fn_{}", n),
            (ScopeType::Class, Some(n)) => format!("class_{}", n),
            _ => format!("scope_{}", self.scope_counter),
        };
        
        let current_scope = self.scopes.get(&self.current_scope_id).unwrap();
        let depth = current_scope.depth + 1;
        
        let new_scope = Scope {
            id: scope_id.clone(),
            scope_type,
            parent: Some(self.current_scope_id.clone()),
            depth,
            variables: Vec::new(),
            functions: Vec::new(),
            children: Vec::new(),
        };
        
        self.scopes.insert(scope_id.clone(), new_scope);
        self.scopes
            .get_mut(&self.current_scope_id)
            .unwrap()
            .children
            .push(scope_id.clone());
        
        scope_id
    }
    
    fn add_variable_to_current_scope(
        &mut self,
        name: String,
        kind: VariableKind,
        position: tree_sitter::Point,
        is_hoisted: bool,
    ) {
        let variable = Variable {
            name: name.clone(),
            canonical_name: None,
            kind: kind,
            declaration_line: position.row,
            declaration_column: position.column,
            is_hoisted,
        };
        
        let scope = self.scopes.get_mut(&self.current_scope_id).unwrap();
        
        if kind == VariableKind::FunctionDeclaration {
            scope.functions.push(name);
        }
        
        scope.variables.push(variable);
    }
    
    pub fn get_scopes(&self) -> &HashMap<String, Scope> {
        &self.scopes
    }
    
    pub fn resolve_identifier(&self, identifier: &str, from_scope_id: &str) -> Option<&Variable> {
        let mut current_scope_id = from_scope_id;
        
        loop {
            if let Some(scope) = self.scopes.get(current_scope_id) {
                if let Some(var) = scope.variables.iter().find(|v| v.name == identifier) {
                    return Some(var);
                }
                
                if let Some(parent) = &scope.parent {
                    current_scope_id = parent;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::JsParser;
    
    #[test]
    fn test_simple_scope_analysis() {
        let source = r#"
        var globalVar = 1;
        function outer(x) {
            let innerVar = 2;
            function inner(y) {
                const z = x + y;
                return z;
            }
            return inner;
        }
        "#;
        
        let mut parser = JsParser::new().unwrap();
        let tree = parser.parse(source).unwrap();
        
        let mut analyzer = ScopeAnalyzer::new();
        analyzer.analyze(tree.root_node(), source).unwrap();
        
        let scopes = analyzer.get_scopes();
        assert!(scopes.contains_key("global"));
        assert!(scopes.values().any(|s| s.scope_type == ScopeType::Function));
    }
}