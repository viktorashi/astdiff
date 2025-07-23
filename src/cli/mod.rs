use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(author, version, about = "JavaScript Variable Mapping and Canonicalization Tool", long_about = None)]
pub struct Args {
    /// Input JavaScript file
    pub input_file: PathBuf,
    
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

impl Args {
    pub fn mode(&self) -> Mode {
        match &self.map {
            None => Mode::Canonicalize,
            Some(None) => Mode::GenerateMapping,
            Some(Some(path)) => Mode::ApplyMapping(path.clone()),
        }
    }
}

#[derive(Debug)]
pub enum Mode {
    /// Default mode: output canonicalized JavaScript
    Canonicalize,
    /// Generate mapping template for editing
    GenerateMapping,
    /// Apply edited mappings to create semantic version
    ApplyMapping(PathBuf),
}