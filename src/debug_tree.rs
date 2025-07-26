use tree_sitter::{Node, Parser};

fn print_tree(node: Node, source: &str, indent: usize) {
    let node_text = if node.child_count() == 0 && node.byte_range().len() < 50 {
        format!(" \"{}\"", &source[node.byte_range()])
    } else {
        String::new()
    };
    
    println!("{}{} [{}:{}]{}",
        " ".repeat(indent),
        node.kind(),
        node.start_position().row,
        node.start_position().column,
        node_text
    );
    
    // Print field names if any
    let mut cursor = node.walk();
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if let Some(field_name) = node.field_name_for_child(i) {
                print!("{}  ({}=", " ".repeat(indent), field_name);
                print_tree(child, source, 0);
                print!(")");
            } else {
                print_tree(child, source, indent + 2);
            }
        }
    }
}

fn main() {
    let source = r#"
import * as WbB from "path";
const filter = hA => hA.status === "running";
"#;

    let mut parser = Parser::new();
    parser.set_language(tree_sitter_javascript::language()).unwrap();
    
    let tree = parser.parse(source, None).unwrap();
    print_tree(tree.root_node(), source, 0);
}