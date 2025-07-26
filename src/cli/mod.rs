use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(author, version, about = "JavaScript Variable Mapping and Canonicalization Tool", long_about = None)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Option<Command>,
    
    // Default mode arguments (when no subcommand is used)
    /// Input JavaScript file (for default canonicalize mode)
    pub input_file: Option<PathBuf>,
    
    /// Generate mapping template (no file) or apply mappings (with file)
    #[clap(long, value_name = "FILE")]
    pub map: Option<Option<PathBuf>>,
    
    /// Keep comments in output
    #[clap(long)]
    pub preserve_comments: bool,
    
    /// Pretty print the output with proper indentation
    #[clap(long)]
    pub pretty: bool,
    
    /// Show detailed scope analysis to stderr
    #[clap(long)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Compare two JavaScript files structurally
    Diff {
        /// First JavaScript file
        file1: PathBuf,
        /// Second JavaScript file
        file2: PathBuf,
        /// Mapping file for first file
        #[clap(long)]
        map1: Option<PathBuf>,
        /// Mapping file for second file
        #[clap(long)]
        map2: Option<PathBuf>,
        /// Output format: unified (default), side-by-side, or json
        #[clap(long, default_value = "unified")]
        format: String,
        /// Export rename mappings to a file
        #[clap(long)]
        export_mappings: Option<PathBuf>,
        /// Show only summary of changes (no detailed diffs)
        #[clap(long)]
        summary: bool,
        /// Show interleaved line-by-line diff
        #[clap(long)]
        interleaved: bool,
    },
}

impl Args {
    pub fn mode(&self) -> Mode {
        if let Some(Command::Diff { file1, file2, map1, map2, format, export_mappings, summary, interleaved }) = &self.command {
            return Mode::Diff {
                file1: file1.clone(),
                file2: file2.clone(),
                map1: map1.clone(),
                map2: map2.clone(),
                format: format.clone(),
                export_mappings: export_mappings.clone(),
                summary: *summary,
                interleaved: *interleaved,
            };
        }
        
        // Default behavior for backward compatibility
        match &self.input_file {
            Some(input_file) => match &self.map {
                None => Mode::Canonicalize(input_file.clone()),
                Some(None) => Mode::GenerateMapping(input_file.clone()),
                Some(Some(path)) => Mode::ApplyMapping(input_file.clone(), path.clone()),
            },
            None => {
                eprintln!("Error: No input file provided");
                eprintln!("\nUsage:");
                eprintln!("  varmap <INPUT_FILE>                # Canonicalize JavaScript");
                eprintln!("  varmap <INPUT_FILE> --map          # Generate mapping template");
                eprintln!("  varmap <INPUT_FILE> --map MAP_FILE # Apply mappings");
                eprintln!("  varmap diff FILE1 FILE2            # Compare two JavaScript files");
                eprintln!("\nFor more information, run: varmap --help");
                std::process::exit(1);
            }
        }
    }
}

#[derive(Debug)]
pub enum Mode {
    /// Default mode: output canonicalized JavaScript
    Canonicalize(PathBuf),
    /// Generate mapping template for editing
    GenerateMapping(PathBuf),
    /// Apply edited mappings to create semantic version
    ApplyMapping(PathBuf, PathBuf),
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
    },
}