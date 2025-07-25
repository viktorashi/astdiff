# VarMap: JavaScript Variable Mapping and Canonicalization Tool

A Rust-based tool that analyzes JavaScript code to create canonical variable mappings, enabling semantic comparison of code where identifiers have been renamed (e.g., minified JavaScript with randomized variable names).

## Features

- **Scope Analysis**: Parse JavaScript using tree-sitter to build a complete scope tree
- **Canonicalization**: Rename all identifiers to canonical forms based on scope and declaration order  
- **Mapping Generation**: Create bidirectional mappings between original and canonical names
- **Multiple Modes**: Support canonical output, mapping generation, and semantic renaming

## Installation

```bash
cargo build --release
```

## Usage

### Basic Canonicalization

Convert JavaScript to canonical form for structural comparison:

```bash
# Canonicalize for diffing
./target/release/varmap input.js > canonical.js

# Compare two versions structurally  
./target/release/varmap version1.min.js > canonical1.js
./target/release/varmap version2.min.js > canonical2.js
diff canonical1.js canonical2.js
```

### Generate Mapping Template

Create a mapping file for manual editing:

```bash
./target/release/varmap input.js --map > mappings.map
```

### Apply Custom Mappings

Use edited mapping file to create semantic JavaScript:

```bash
# Edit mappings.map to change NEW_NAME column to semantic names
./target/release/varmap input.js --map mappings.map > readable.js
```

## Examples

### Input (minified.js)
```javascript
function a(b,c){var d=b+c;return d;}
```

### Canonical Output
```bash
./target/release/varmap minified.js
```
```javascript
function fn_1(param_1,param_2){var var_1=param_1+param_2;return var_1;}
```

### Pretty Printed Output
```bash
./target/release/varmap minified.js --pretty
```
```javascript
function fn_1(param_1, param_2) {
  var var_1 = param_1 + param_2;
  return var_1;
}
```

### Mapping Template
```bash
./target/release/varmap minified.js --map
```
```
# FIRST LAST TYPE SCOPE CANONICAL NEW
1:10 1:10 func global fn_7823 fn_7823
1:12 1:25 param fn_add param_1 param_1
1:14 1:29 param fn_add param_2 param_2
1:21 1:35 var fn_add var_1 var_1
```

### Semantic Output (after editing mappings)
After editing the NEW column in the mapping file:
```javascript
function addNumbers(firstNumber,secondNumber){var sum=firstNumber+secondNumber;return sum;}
```

### Pretty Printed Semantic Output
```bash
./target/release/varmap minified.js --map mappings.map --pretty
```
```javascript
function addNumbers(firstNumber, secondNumber) {
  var sum = firstNumber + secondNumber;
  return sum;
}
```

## Command Line Options

```
varmap [OPTIONS] <input-file>

OPTIONS:
  --map [file]           Generate mapping template (no file) or apply mappings (with file)
  --preserve-comments    Keep comments in output (not yet implemented)
  --pretty              Pretty print the output with proper indentation
  --verbose             Show detailed scope analysis to stderr
  -h, --help            Show help
```

## Use Cases

- **Code Analysis**: Compare different versions of minified JavaScript
- **Reverse Engineering**: Analyze obfuscated code changes over time  
- **Security Research**: Create semantic diffs that ignore variable name randomization
- **LLM Integration**: Generate mapping templates for AI-assisted variable renaming

## Architecture

The tool consists of four main components:

1. **Parser Module**: Uses tree-sitter-javascript for robust JavaScript parsing
2. **Scope Analyzer**: Tracks variable bindings and scope relationships
3. **Canonicalizer**: Applies structural hashing for functions and scope-local naming
4. **Mapping Generator**: Creates and applies variable mappings

## Supported JavaScript Features

- Function declarations and expressions
- Arrow functions  
- Variable declarations (var, let, const)
- Function parameters
- Destructuring patterns (object and array)
- Loop variables (for-in, for-of)
- Catch clause parameters
- Import/export statements
- Nested scopes
- Block scoping
- Classes (basic support)

## Contributing

This is a research/educational implementation. Contributions welcome for:

- TypeScript support
- Improved error handling
- More JavaScript features
- Performance optimizations

## License

MIT License