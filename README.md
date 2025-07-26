# ASTDiff: AST-based JavaScript Diff and Code Analysis Tool

A Rust-based tool that compares JavaScript files using Abstract Syntax Tree (AST) analysis, enabling meaningful comparison of code regardless of variable names, formatting, or code reordering. Perfect for analyzing minified, obfuscated, or refactored JavaScript.

## Features

- **Structural Diff**: Compare JavaScript files based on AST structure, not text
- **Smart Matching**: Automatically matches renamed functions and variables
- **Flexible Output**: Side-by-side, summary, or interleaved diff formats
- **Canonicalization**: Normalize variable names for consistent comparison
- **Mapping Support**: Generate and apply custom variable name mappings
- **Performance**: Optimized with size-based sorting and MinHash LSH

## Installation

```bash
cargo build --release
```

## Usage

### Basic Structural Diff (Default)

Compare two JavaScript files to see what actually changed:

```bash
# Show side-by-side comparison of modified functions
astdiff file1.js file2.js

# Show only summary of changes
astdiff file1.js file2.js --summary

# Show interleaved line-by-line diff
astdiff file1.js file2.js --interleaved

# Export rename mappings
astdiff file1.js file2.js --export-mappings renames.yaml
```

### Canonicalization

Convert JavaScript to canonical form with normalized variable names:

```bash
# Basic canonicalization
astdiff canon input.js > canonical.js

# Pretty print the output
astdiff canon input.js --pretty

# Generate mapping template
astdiff canon input.js --map > mappings.yaml

# Apply custom mappings
astdiff canon input.js --map mappings.yaml > semantic.js
```

## Examples

### Structural Diff Example

Given two files where functions are renamed and reordered:

**file1.js:**
```javascript
function getData() {
    return fetch('/api/data');
}

function processData(data) {
    return data.map(item => item.value * 2);
}
```

**file2.js:**
```javascript
function b(x) {
    return x.map(y => y.value * 2);
}

function a() {
    return fetch('/api/v2/data'); // Changed!
}
```

Running `astdiff file1.js file2.js` will show:

- `getData` → `a` with structural modification (API endpoint changed)
- `processData` → `b` with no structural changes (just renamed)
- Reordering is ignored as non-meaningful

### Canonicalization Example

**Input (minified.js):**
```javascript
function a(b,c){var d=b+c;return d;}
```

**Canonical Output:**
```bash
astdiff canon minified.js
```
```javascript
function fn_1(param_1,param_2){var var_1=param_1+param_2;return var_1;}
```

**Pretty Printed:**
```bash
astdiff canon minified.js --pretty
```
```javascript
function fn_1(param_1, param_2) {
  var var_1 = param_1 + param_2;
  return var_1;
}
```

## Command Line Options

### Diff Mode (Default)
```
astdiff [OPTIONS] <file1> <file2>

OPTIONS:
  --map1 <FILE>           Mapping file for first file
  --map2 <FILE>           Mapping file for second file  
  --format <FORMAT>       Output format: unified (default), side-by-side, json
  --export-mappings <FILE> Export rename mappings to file
  --summary              Show only summary of changes
  --interleaved          Show interleaved line-by-line diff
  --verbose              Show detailed analysis
  -h, --help             Show help
```

### Canon Subcommand
```
astdiff canon [OPTIONS] <input-file>

OPTIONS:
  --map [FILE]           Generate mapping template (no file) or apply mappings (with file)
  --preserve-comments    Keep comments in output
  --pretty              Pretty print the output
  -h, --help            Show help
```

## Use Cases

- **Security Analysis**: Compare obfuscated malware samples to identify changes
- **Build Verification**: Ensure minified code matches the source
- **Code Review**: Focus on actual logic changes, not formatting
- **Reverse Engineering**: Track changes between obfuscated versions
- **Refactoring**: Verify that refactoring preserved functionality

## How It Works

1. **Structural Hashing**: Computes hashes of AST nodes while ignoring identifiers
2. **Declaration Extraction**: Identifies functions, variables, classes, imports, and exports
3. **Similarity Matching**: Uses Jaccard index to find matching declarations
4. **Optimized Search**: Size-based sorting and MinHash LSH for O(n log n) performance

## Supported JavaScript Features

- Function declarations and expressions
- Arrow functions  
- Variable declarations (var, let, const)
- Classes and methods
- Import/export statements
- Destructuring patterns
- Object and array literals
- All standard JavaScript syntax

## Contributing

Contributions welcome for:

- TypeScript support
- Additional output formats
- More sophisticated matching algorithms
- Performance improvements

## License

MIT License