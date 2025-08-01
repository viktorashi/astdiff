# astdiff

A high-performance AST-based structural diff tool for JavaScript that intelligently matches renamed functions and variables in minified or obfuscated code.

## Overview

`astdiff` analyzes JavaScript files at the Abstract Syntax Tree (AST) level to identify structural changes between versions, even when functions and variables have been renamed. Unlike traditional text-based diffs, it understands code structure and can match semantically equivalent code blocks.

### Key Features

- **Intelligent Matching**: Uses MinHash signatures and structural fingerprinting to match renamed functions
- **Minified Code Support**: Designed to work with heavily minified/obfuscated JavaScript
- **Fast Performance**: Parallel processing and optimized algorithms handle large files efficiently
- **Multiple Output Formats**: Summary, detailed, compact, side-by-side, and JSON outputs
- **Source Map Support**: Can utilize source maps to show original names when available
- **Comprehensive Dumps**: Save and reload analysis results for faster repeated comparisons

## Installation

```bash
cargo install --path .
```

Or build from source:

```bash
cargo build --release
./target/release/astdiff --help
```

## Usage

### Basic Diff

Compare two JavaScript files:

```bash
astdiff file1.js file2.js
```

### Output Formats

Choose different output styles with `--format`:

```bash
# Summary view (default)
astdiff old.js new.js

# Detailed view with full function bodies
astdiff old.js new.js --format detailed

# Compact view (just locations)
astdiff old.js new.js --format compact

# Side-by-side comparison
astdiff old.js new.js --format side-by-side

# JSON output for programmatic use
astdiff old.js new.js --format json
```

### Advanced Options

```bash
# Use source maps for better names
astdiff old.js new.js --map1 old.js.map --map2 new.js.map

# Show renamed functions (hidden by default)
ASTDIFF_SHOW_RENAMES=1 astdiff old.js new.js

# Save analysis for faster re-runs
astdiff old.js new.js --dump analysis.astdump

# Generate detailed matching report
astdiff old.js new.js --report-path report.html
```

### Working with Dumps

Save analysis results for faster repeated comparisons:

```bash
# Create a comprehensive dump
astdiff v1.js v2.js --dump comparison.astdump

# Query the dump
astdiff query comparison.astdump find functionName
astdiff query comparison.astdump summary
astdiff query comparison.astdump validate

# Load and display the dump
astdiff load comparison.astdump
```

### Other Commands

```bash
# Canonicalize JavaScript (normalize variable names)
astdiff canonicalize input.js

# Inspect a specific declaration
astdiff inspect file.js functionName
astdiff inspect file.js functionName --compare-file other.js

# Apply source map to canonicalized code
astdiff apply-mapping canonical.js mapping.json
```

## How It Works

1. **Parsing**: Uses tree-sitter to parse JavaScript into ASTs
2. **Declaration Extraction**: Identifies all functions, variables, classes, imports, and exports
3. **Structural Hashing**: Creates hash signatures for each declaration's AST structure
4. **MinHash Signatures**: Generates compact signatures for efficient similarity estimation
5. **Fingerprinting**: Extracts semantic features (strings, constants, API calls) for better matching
6. **Parallel Matching**: Uses parallel algorithms to find best matches between declarations
7. **Change Detection**: Identifies additions, deletions, modifications, and renames

## Performance

Optimized for large minified files:
- Processes files with 7,500+ declarations in ~20 seconds
- Parallel extraction and matching algorithms
- Efficient u64-based structural hashing
- MinHash filtering reduces comparison complexity from O(n²) to manageable levels

## Output Interpretation

The tool reports several types of changes:

- **Added/Removed Functions**: New or deleted declarations
- **Modified Functions**: Structurally changed but matched declarations
- **Renamed Functions**: High-confidence matches with different names (hidden by default)
- **Structural Similarity**: Overall percentage of matched declarations

Example output:
```
Structural similarity: 98.5%
Matched declarations: 7483/7490 vs 7501
Changes: 18 additions, 7 deletions, 10 modifications (+ 7206 renames)
```

## Environment Variables

- `ASTDIFF_SHOW_RENAMES`: Show renamed functions in output
- `ASTDIFF_DEBUG`: Enable debug output for fingerprint extraction
- `ASTDIFF_PROFILE`: Show performance profiling information

## Building from Source

Requirements:
- Rust 1.70+
- C++ compiler (for tree-sitter)

```bash
git clone https://github.com/yourusername/astdiff
cd astdiff
cargo build --release
```

## License

MIT License - see LICENSE file for details