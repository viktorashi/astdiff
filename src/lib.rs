pub mod parser;
pub mod scope;
pub mod canonicalizer;
pub mod mapping;
pub mod cli;
pub mod pretty;
pub mod diff;

use anyhow::Result;
use std::fs;

pub use cli::{Args, Mode};
use parser::JsParser;
use scope::ScopeAnalyzer;
use canonicalizer::Canonicalizer;
use mapping::MappingGenerator;
use pretty::PrettyPrinter;

pub fn run(args: Args) -> Result<()> {
    match args.mode() {
        Mode::Diff { file1, file2, map1, map2, format, export_mappings, summary, interleaved, verbose, fingerprints, report, report_path, compact, parallel } => {
            run_diff(file1, file2, map1, map2, format, export_mappings, summary, interleaved, verbose, fingerprints, report, report_path, compact, parallel)
        }
        Mode::Canonicalize { input_file, preserve_comments, pretty } => {
            run_canonicalize(&input_file, preserve_comments, pretty, args.verbose)
        }
        Mode::GenerateMapping { input_file, preserve_comments, pretty } => {
            run_generate_mapping(&input_file, preserve_comments, pretty, args.verbose)
        }
        Mode::ApplyMapping { input_file, map_file, preserve_comments, pretty } => {
            run_apply_mapping(&input_file, &map_file, preserve_comments, pretty, args.verbose)
        }
    }
}

fn run_canonicalize(input_file: &std::path::PathBuf, _preserve_comments: bool, pretty: bool, verbose: bool) -> Result<()> {
    let source = fs::read_to_string(input_file)?;
    let mut parser = JsParser::new()?;
    let tree = parser.parse(&source)?;
    
    let mut analyzer = ScopeAnalyzer::new();
    analyzer.analyze(tree.root_node(), &source)?;
    
    if verbose {
        print_scope_analysis(&analyzer);
    }
    
    let mut canonicalizer = Canonicalizer::new(analyzer);
    canonicalizer.canonicalize(&tree, &source)?;
    
    let canonical = canonicalizer.apply_canonicalization(&tree, &source)?;
    if pretty {
        let pretty_printer = PrettyPrinter::new();
        let mut parser = JsParser::new()?;
        let canonical_tree = parser.parse(&canonical)?;
        let formatted = pretty_printer.format(&canonical_tree, &canonical);
        print!("{}", formatted);
    } else {
        print!("{}", canonical);
    }
    
    Ok(())
}

fn run_generate_mapping(input_file: &std::path::PathBuf, _preserve_comments: bool, _pretty: bool, verbose: bool) -> Result<()> {
    let source = fs::read_to_string(input_file)?;
    let mut parser = JsParser::new()?;
    let tree = parser.parse(&source)?;
    
    let mut analyzer = ScopeAnalyzer::new();
    analyzer.analyze(tree.root_node(), &source)?;
    
    if verbose {
        print_scope_analysis(&analyzer);
    }
    
    let mut canonicalizer = Canonicalizer::new(analyzer);
    canonicalizer.canonicalize(&tree, &source)?;
    
    let generator = MappingGenerator::new(canonicalizer, source.clone());
    let mapping_file = generator.generate_mapping_file(&tree)?;
    print!("{}", mapping_file);
    
    Ok(())
}

fn run_apply_mapping(input_file: &std::path::PathBuf, map_file: &std::path::PathBuf, _preserve_comments: bool, pretty: bool, verbose: bool) -> Result<()> {
    let source = fs::read_to_string(input_file)?;
    let mut parser = JsParser::new()?;
    let tree = parser.parse(&source)?;
    
    let mut analyzer = ScopeAnalyzer::new();
    analyzer.analyze(tree.root_node(), &source)?;
    
    if verbose {
        print_scope_analysis(&analyzer);
    }
    
    let mut canonicalizer = Canonicalizer::new(analyzer);
    canonicalizer.canonicalize(&tree, &source)?;
    
    let mapping_content = fs::read_to_string(map_file)?;
    let mappings = MappingGenerator::parse_mapping_file(&mapping_content)?;
    let generator = MappingGenerator::new(canonicalizer, source.clone());
    let output = generator.apply_mappings(&tree, mappings)?;
    
    if pretty {
        let pretty_printer = PrettyPrinter::new();
        let mut parser = JsParser::new()?;
        let output_tree = parser.parse(&output)?;
        let formatted = pretty_printer.format(&output_tree, &output);
        print!("{}", formatted);
    } else {
        print!("{}", output);
    }
    
    Ok(())
}

fn run_diff(
    file1: std::path::PathBuf,
    file2: std::path::PathBuf,
    map1: Option<std::path::PathBuf>,
    map2: Option<std::path::PathBuf>,
    format: String,
    export_mappings: Option<std::path::PathBuf>,
    summary: bool,
    interleaved: bool,
    verbose: bool,
    fingerprints: bool,
    report: bool,
    report_path: Option<std::path::PathBuf>,
    compact: bool,
    _parallel: bool,
) -> Result<()> {
    use crate::diff::StructuralDiff;
    use crate::diff::profiling::Timer;
    
    use std::thread;
    
    // Load and parse both files in parallel
    let file1_path = file1.clone();
    let file2_path = file2.clone();
    
    let handle1 = thread::spawn(move || -> Result<(String, tree_sitter::Tree)> {
        let _timer = Timer::new("read_and_parse_file1");
        let source = fs::read_to_string(&file1_path)?;
        let mut parser = JsParser::new()?;
        let tree = parser.parse(&source)?;
        Ok((source, tree))
    });
    
    let handle2 = thread::spawn(move || -> Result<(String, tree_sitter::Tree)> {
        let _timer = Timer::new("read_and_parse_file2");
        let source = fs::read_to_string(&file2_path)?;
        let mut parser = JsParser::new()?;
        let tree = parser.parse(&source)?;
        Ok((source, tree))
    });
    
    let (source1, tree1) = handle1.join().expect("Thread 1 panicked")?;
    let (source2, tree2) = handle2.join().expect("Thread 2 panicked")?;
    
    eprintln!("Source files: {} bytes, {} bytes", source1.len(), source2.len());
    
    let mut diff = StructuralDiff::new();
    
    // Configure diff based on CLI flags
    diff.set_use_fingerprints(fingerprints);
    diff.set_generate_report(report);
    if let Some(path) = report_path {
        diff.set_report_path(path);
    }
    if verbose {
        std::env::set_var("ASTDIFF_DEBUG", "1");
    }
    
    // Load mappings if provided
    if let Some(map_path) = map1 {
        let mapping_content = fs::read_to_string(&map_path)?;
        let mappings = MappingGenerator::parse_mapping_file(&mapping_content)?;
        diff.set_mappings1(mappings);
    }
    if let Some(map_path) = map2 {
        let mapping_content = fs::read_to_string(&map_path)?;
        let mappings = MappingGenerator::parse_mapping_file(&mapping_content)?;
        diff.set_mappings2(mappings);
    }
    
    let result = {
        let _timer = Timer::new("diff_compare_total");
        diff.compare(&tree1, &source1, &tree2, &source2)?
    };
    
    // TODO: Apply existing mappings to enhance the output with semantic names
    // diff.apply_mappings_to_result(&mut result);
    
    // For detailed diff, we need to canonicalize both files
    let (canonical1, canonical2) = if !summary && interleaved {
        let _timer = Timer::new("canonicalize_for_output");
        let mut analyzer1 = ScopeAnalyzer::new();
        analyzer1.analyze(tree1.root_node(), &source1)?;
        let mut canonicalizer1 = Canonicalizer::new(analyzer1);
        canonicalizer1.canonicalize(&tree1, &source1)?;
        let canonical1 = canonicalizer1.apply_canonicalization(&tree1, &source1)?;
        
        let mut analyzer2 = ScopeAnalyzer::new();
        analyzer2.analyze(tree2.root_node(), &source2)?;
        let mut canonicalizer2 = Canonicalizer::new(analyzer2);
        canonicalizer2.canonicalize(&tree2, &source2)?;
        let canonical2 = canonicalizer2.apply_canonicalization(&tree2, &source2)?;
        
        (Some(canonical1), Some(canonical2))
    } else {
        (None, None)
    };
    
    {
        let _timer = Timer::new("generate_output");
        match format.as_str() {
            "unified" => {
                if compact {
                    diff.print_compact(&result, &file1, &file2, &source1, &source2)
                } else if summary {
                    diff.print_summary(&result, &file1, &file2, &source1, &source2)
                } else if interleaved {
                    diff.print_interleaved(&result, &file1, &file2, canonical1.as_deref(), canonical2.as_deref(), &source1, &source2)?
                } else {
                    diff.print_side_by_side_full(&result, &file1, &file2, &source1, &source2)?
                }
            }
            "side-by-side" => diff.print_side_by_side(&result, &file1, &file2, &source1, &source2),
            "json" => diff.print_json(&result)?,
            _ => anyhow::bail!("Unknown format: {}", format),
        }
    }
    
    // Export rename mappings if requested
    if let Some(export_path) = export_mappings {
        let rename_mappings = diff.generate_rename_mapping(&result);
        if !rename_mappings.is_empty() {
            let yaml = serde_yaml::to_string(&rename_mappings)?;
            fs::write(&export_path, yaml)?;
            eprintln!("Exported {} rename mappings to {}", rename_mappings.len(), export_path.display());
        }
    }
    
    // Report profiling data at the very end
    crate::diff::profiling::report_profile();
    
    Ok(())
}

fn print_scope_analysis(analyzer: &ScopeAnalyzer) {
    eprintln!("=== Scope Analysis ===");
    for (id, scope) in analyzer.get_scopes() {
        eprintln!("Scope: {} (type: {:?}, depth: {})", id, scope.scope_type, scope.depth);
        for var in &scope.variables {
            eprintln!("  Variable: {} (kind: {:?})", var.name, var.kind);
        }
    }
    eprintln!();
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