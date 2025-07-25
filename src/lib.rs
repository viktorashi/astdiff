pub mod parser;
pub mod scope;
pub mod canonicalizer;
pub mod mapping;
pub mod cli;
pub mod pretty;

use anyhow::Result;
use std::fs;

pub use cli::{Args, Mode};
use parser::JsParser;
use scope::ScopeAnalyzer;
use canonicalizer::Canonicalizer;
use mapping::MappingGenerator;
use pretty::PrettyPrinter;

pub fn run(args: Args) -> Result<()> {
    // Read input file
    let source = fs::read_to_string(&args.input_file)?;
    
    // Parse JavaScript
    let mut parser = JsParser::new()?;
    let tree = parser.parse(&source)?;
    
    // Analyze scopes
    let mut analyzer = ScopeAnalyzer::new();
    analyzer.analyze(tree.root_node(), &source)?;
    
    if args.verbose {
        eprintln!("=== Scope Analysis ===");
        for (id, scope) in analyzer.get_scopes() {
            eprintln!("Scope: {} (type: {:?}, depth: {})", id, scope.scope_type, scope.depth);
            for var in &scope.variables {
                eprintln!("  Variable: {} (kind: {:?})", var.name, var.kind);
            }
        }
        eprintln!();
    }
    
    // Canonicalize
    let mut canonicalizer = Canonicalizer::new(analyzer);
    canonicalizer.canonicalize(&tree, &source)?;
    
    // Generate output based on mode
    match args.mode() {
        Mode::Canonicalize => {
            let canonical = canonicalizer.apply_canonicalization(&tree, &source)?;
            if args.pretty {
                let pretty_printer = PrettyPrinter::new();
                let mut parser = JsParser::new()?;
                let canonical_tree = parser.parse(&canonical)?;
                let formatted = pretty_printer.format(&canonical_tree, &canonical);
                print!("{}", formatted);
            } else {
                print!("{}", canonical);
            }
        }
        Mode::GenerateMapping => {
            let generator = MappingGenerator::new(canonicalizer, source.clone());
            let mapping_file = generator.generate_mapping_file(&tree)?;
            print!("{}", mapping_file);
        }
        Mode::ApplyMapping(map_file) => {
            let mapping_content = fs::read_to_string(&map_file)?;
            let mappings = MappingGenerator::parse_mapping_file(&mapping_content)?;
            let generator = MappingGenerator::new(canonicalizer, source.clone());
            let output = generator.apply_mappings(&tree, mappings)?;
            if args.pretty {
                let pretty_printer = PrettyPrinter::new();
                let mut parser = JsParser::new()?;
                let output_tree = parser.parse(&output)?;
                let formatted = pretty_printer.format(&output_tree, &output);
                print!("{}", formatted);
            } else {
                print!("{}", output);
            }
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    
    #[test]
    fn test_basic_functionality() {
        // This is a placeholder test - in a real implementation,
        // we'd have comprehensive tests for each component
        assert!(true);
    }
}