# VarMap: JavaScript Variable Mapping and Canonicalization Tool

## Overview

VarMap is a tool that analyzes JavaScript code to create canonical variable mappings, enabling semantic comparison of code where identifiers have been renamed (e.g., minified JavaScript with randomized variable names).

## Core Functionality

### Primary Features
1. **Scope Analysis**: Parse JavaScript using tree-sitter to build a complete scope tree
2. **Canonicalization**: Rename all identifiers to canonical forms based on scope and declaration order
3. **Mapping Generation**: Create bidirectional mappings between original and canonical names
4. **Multiple Output Formats**: Support JSON, YAML, and custom formats for mappings

### Use Cases
- Comparing different versions of minified JavaScript
- Analyzing obfuscated code changes over time
- Creating semantic diffs that ignore variable name randomization
- Security research on minified/obfuscated JavaScript

## Technical Architecture

### Dependencies
- **tree-sitter**: Core parsing engine
- **tree-sitter-javascript**: JavaScript grammar
- **Language**: Rust (recommended) or Go for performance and tree-sitter integration

### Core Components

#### 1. Parser Module
- Initialize tree-sitter with JavaScript grammar
- Parse input JavaScript into concrete syntax tree
- Handle syntax errors gracefully (important for minified code)

#### 2. Scope Analyzer
- Traverse AST to identify scope boundaries:
  - Global scope
  - Function scopes
  - Block scopes (let/const)
  - Module scopes (import/export)
- Track variable bindings and their relationships
- Handle JavaScript-specific scoping rules:
  - Hoisting (var, function declarations)
  - Temporal dead zones (let/const)
  - Closure captures

#### 3. Canonicalizer
- Rename identifiers using consistent naming scheme:
  - Functions: `fn_1`, `fn_2`, `fn_3` (by declaration order)
  - Function parameters: `param_1`, `param_2` (within each function)
  - Variables: `var_1`, `var_2` (within each scope)
  - Class names: `class_1`, `class_2`
  - Method names: `method_1`, `method_2`
- Preserve semantic meaning through consistent ordering
- Handle edge cases:
  - Anonymous functions
  - Destructuring assignments
  - Property access chains

#### 4. Mapping Generator
- Create bidirectional identifier mappings
- Track scope relationships
- Generate mapping metadata (declaration positions, scope depth, etc.)

## Tree-sitter Queries

### Key Node Types to Extract
```scheme
; Function declarations and expressions
(function_declaration name: (identifier) @function-name)
(function_expression name: (identifier) @function-name)
(arrow_function)

; Variable declarations
(variable_declarator name: (identifier) @variable-name)
(formal_parameters (identifier) @parameter-name)

; All identifier references
(identifier) @identifier

; Class declarations
(class_declaration name: (identifier) @class-name)
(method_definition name: (property_name) @method-name)
```

## Input/Output Specification

### Command Line Interface
```bash
# Canonicalize JavaScript (rename to fn_1, param_1, etc.)
varmap <input-file> > <canonical-file>

# Generate mapping template for editing
varmap <input-file> --map > <map-file>

# Apply edited mappings to create semantic version
varmap <input-file> --map <map-file> > <semantic-file>

OPTIONS:
  --map [file]           Generate mapping template (no file) or apply mappings (with file)
  --preserve-comments    Keep comments in output
  --verbose             Show detailed scope analysis to stderr
  -h, --help            Show help
```

### Input
- JavaScript files (minified or regular)
- Supports ES5, ES6+, CommonJS, ES modules
- Handle syntax errors gracefully

### Output Formats

#### 1. Canonicalized JavaScript
When run without `--map`, outputs JavaScript with canonical identifiers:
```javascript
// Input: function a(b,c){var d=b+c;return d;}
// Output: function fn_1(param_1,param_2){var var_1=param_1+param_2;return var_1;}
```

#### 2. Mapping File Format
When run with `--map` (no file), outputs line-based mapping format:
```
# LINE:COL TYPE SCOPE ORIGINAL CANONICAL NEW_NAME CONTEXT
1:10 func global a fn_1 fn_1 "function a(b,c){var d=b+c;return d;}"
1:12 param func_a b param_1 param_1 "parameter used in addition: b+c"
1:14 param func_a c param_2 param_2 "parameter used in addition: b+c"
1:20 var func_a d var_1 var_1 "stores result of b+c, then returned"
1:32 ref func_a d var_1 var_1 "return value"
```

**Field Descriptions:**
- **LINE:COL**: Position in source file
- **TYPE**: func, param, var, ref (function, parameter, variable declaration, reference)
- **SCOPE**: Scope identifier (global, func_1, etc.)
- **ORIGINAL**: Original identifier from source
- **CANONICAL**: Generated canonical name (fn_1, param_1, etc.)
- **NEW_NAME**: User-editable semantic name (initially same as CANONICAL)
- **CONTEXT**: Code context for human/LLM understanding

#### 3. Semantic JavaScript
When run with `--map <file>`, outputs JavaScript using NEW_NAME mappings:
```javascript
// After editing map file NEW_NAME column:
// fn_1 → addNumbers, param_1 → firstNumber, etc.
// Output: function addNumbers(firstNumber,secondNumber){var sum=firstNumber+secondNumber;return sum;}
```

## Implementation Plan

### Phase 1: Core Parser
- [ ] Set up tree-sitter with JavaScript grammar
- [ ] Basic AST traversal and node identification
- [ ] Simple identifier extraction
- [ ] Basic scope boundary detection

### Phase 2: Scope Analysis
- [ ] Complete scope tree construction
- [ ] Variable binding resolution
- [ ] Handle JavaScript scoping edge cases
- [ ] Closure capture detection

### Phase 3: Canonicalization
- [ ] Implement naming scheme
- [ ] Consistent identifier replacement
- [ ] Preserve code structure and formatting
- [ ] Handle destructuring and complex assignments

### Phase 4: Mapping Generation
- [ ] Bidirectional mapping creation
- [ ] JSON output format
- [ ] Position tracking
- [ ] Metadata collection

### Phase 5: CLI and Output
- [ ] Command-line interface
- [ ] Multiple output formats
- [ ] Error handling and reporting
- [ ] Documentation and examples

## Usage Examples

### Basic Canonicalization for Diffing
```bash
# Compare two versions of minified code structurally
varmap version1.min.js > canonical1.js
varmap version2.min.js > canonical2.js
diff canonical1.js canonical2.js
```

### Human-Readable Renaming Workflow
```bash
# Generate canonical version
varmap minified.js > canonical.js

# Create mapping template for editing
varmap minified.js --map > minified.map

# Edit minified.map (change NEW_NAME column to semantic names)
# Then apply the mappings
varmap minified.js --map minified.map > readable.js
```

### LLM-Assisted Renaming
```bash
# Generate mapping template and process with LLM
varmap input.js --map | llm-rename-variables > edited.map
varmap input.js --map edited.map > semantic.js

# Or in a pipeline
varmap input.js --map | llm process | varmap input.js --map /dev/stdin > output.js
```

### Nested Scopes
```javascript
// Input
function outer(x) {
  var y = 1;
  function inner(z) {
    return x + y + z;
  }
  return inner;
}

// Expected canonical  
function fn_1(param_1) {
  var var_1 = 1;
  function fn_2(param_1) {
    return param_1 + var_1 + param_1;
  }
  return fn_2;
}
```

### Edge Cases to Handle
- Hoisted variables and functions
- let/const temporal dead zones
- Arrow functions and implicit returns
- Destructuring assignments
- Class methods and constructors
- Import/export statements
- Anonymous functions
- IIFE patterns

## Success Criteria

1. **Accuracy**: Correctly identifies and maps 99%+ of identifiers in well-formed JavaScript
2. **Performance**: Processes typical minified libraries (1MB+) in under 5 seconds
3. **Robustness**: Handles syntax errors and malformed input gracefully
4. **Consistency**: Same input always produces identical canonical output
5. **Completeness**: Supports all modern JavaScript features (ES2023+)

## Future Enhancements

- TypeScript support
- Source map integration
- Semantic hints (detect common patterns like event handlers)
- Integration with existing diff tools
- VSCode extension for live mapping
- API mode for programmatic usage
