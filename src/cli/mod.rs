use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(author, version, about = "AST-based JavaScript Diff and Code Analysis Tool", long_about = None)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Option<Command>,
    
    // Default diff mode arguments (when no subcommand is used)
    /// First JavaScript file to compare
    pub file1: Option<PathBuf>,
    
    /// Second JavaScript file to compare
    pub file2: Option<PathBuf>,
    
    /// Mapping file for first file
    #[clap(long)]
    pub map1: Option<PathBuf>,
    
    /// Mapping file for second file
    #[clap(long)]
    pub map2: Option<PathBuf>,
    
    /// Output format: unified (default), side-by-side, or json
    #[clap(long, default_value = "unified")]
    pub format: String,
    
    /// Export rename mappings to a file
    #[clap(long)]
    pub export_mappings: Option<PathBuf>,
    
    /// Show only summary of changes (no detailed diffs)
    #[clap(long)]
    pub summary: bool,
    
    /// Show interleaved line-by-line diff
    #[clap(long)]
    pub interleaved: bool,
    
    /// Show detailed analysis to stderr
    #[clap(long)]
    pub verbose: bool,
    
    /// Enable fingerprint-based matching (disabled by default due to accuracy issues)
    #[clap(long)]
    pub fingerprints: bool,
    
    /// Generate a detailed matching report
    #[clap(long)]
    pub report: bool,
    
    /// Path to save the matching report (implies --report)
    #[clap(long, value_name = "PATH")]
    pub report_path: Option<PathBuf>,
    
    /// Compact output showing only function names and line ranges
    #[clap(long)]
    pub compact: bool,
    
    /// Disable parallel matching (enabled by default for better performance)
    #[clap(long = "no-parallel")]
    pub no_parallel: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Canonicalize JavaScript code (normalize variable names)
    Canon {
        /// Input JavaScript file
        input_file: PathBuf,
        
        /// Generate mapping template (no file) or apply mappings (with file)
        #[clap(long, value_name = "FILE")]
        map: Option<Option<PathBuf>>,
        
        /// Keep comments in output
        #[clap(long)]
        preserve_comments: bool,
        
        /// Pretty print the output with proper indentation
        #[clap(long)]
        pretty: bool,
    },
}

impl Args {
    pub fn mode(&self) -> Mode {
        match &self.command {
            Some(Command::Canon { input_file, map, preserve_comments, pretty }) => {
                match map {
                    None => Mode::Canonicalize {
                        input_file: input_file.clone(),
                        preserve_comments: *preserve_comments,
                        pretty: *pretty,
                    },
                    Some(None) => Mode::GenerateMapping {
                        input_file: input_file.clone(),
                        preserve_comments: *preserve_comments,
                        pretty: *pretty,
                    },
                    Some(Some(path)) => Mode::ApplyMapping {
                        input_file: input_file.clone(),
                        map_file: path.clone(),
                        preserve_comments: *preserve_comments,
                        pretty: *pretty,
                    },
                }
            },
            None => {
                // Default is diff mode
                match (&self.file1, &self.file2) {
                    (Some(file1), Some(file2)) => Mode::Diff {
                        file1: file1.clone(),
                        file2: file2.clone(),
                        map1: self.map1.clone(),
                        map2: self.map2.clone(),
                        format: self.format.clone(),
                        export_mappings: self.export_mappings.clone(),
                        summary: self.summary,
                        interleaved: self.interleaved,
                        verbose: self.verbose,
                        fingerprints: self.fingerprints,
                        report: self.report || self.report_path.is_some(),
                        report_path: self.report_path.clone(),
                        compact: self.compact,
                        parallel: !self.no_parallel,
                    },
                    _ => {
                        eprintln!("Error: Two files required for diff");
                        eprintln!("\nUsage:");
                        eprintln!("  astdiff FILE1 FILE2                    # Compare two JavaScript files");
                        eprintln!("  astdiff FILE1 FILE2 --summary          # Show only summary of changes");
                        eprintln!("  astdiff FILE1 FILE2 --interleaved     # Show interleaved diff");
                        eprintln!("  astdiff canon INPUT_FILE               # Canonicalize JavaScript");
                        eprintln!("  astdiff canon INPUT_FILE --map         # Generate mapping template");
                        eprintln!("  astdiff canon INPUT_FILE --map MAP.yaml # Apply mappings");
                        eprintln!("\nFor more information, run: astdiff --help");
                        std::process::exit(1);
                    }
                }
            }
        }
    }
    
    pub fn preserve_comments(&self) -> bool {
        match &self.command {
            Some(Command::Canon { preserve_comments, .. }) => *preserve_comments,
            None => false,
        }
    }
    
    pub fn pretty(&self) -> bool {
        match &self.command {
            Some(Command::Canon { pretty, .. }) => *pretty,
            None => false,
        }
    }
}

#[derive(Debug)]
pub enum Mode {
    /// Canonicalize JavaScript (normalize variable names)
    Canonicalize {
        input_file: PathBuf,
        preserve_comments: bool,
        pretty: bool,
    },
    /// Generate mapping template for editing
    GenerateMapping {
        input_file: PathBuf,
        preserve_comments: bool,
        pretty: bool,
    },
    /// Apply edited mappings to create semantic version
    ApplyMapping {
        input_file: PathBuf,
        map_file: PathBuf,
        preserve_comments: bool,
        pretty: bool,
    },
    /// Diff two JavaScript files structurally
    Diff {
        file1: PathBuf,
        file2: PathBuf,
        map1: Option<PathBuf>,
        map2: Option<PathBuf>,
        format: String,
        export_mappings: Option<PathBuf>,
        summary: bool,
        interleaved: bool,
        verbose: bool,
        fingerprints: bool,
        report: bool,
        report_path: Option<PathBuf>,
        compact: bool,
        parallel: bool,
    },
}