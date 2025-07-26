# VarMap Diff - Structural JavaScript Comparison

The `varmap diff` command compares JavaScript files based on their Abstract Syntax Tree (AST) structure rather than text, making it ideal for comparing obfuscated or minified code.

## How It Works

1. **Structural Hashing**: Computes hashes of AST nodes while replacing all identifiers with placeholders
2. **Declaration Matching**: Extracts functions, variables, classes, imports, and exports from both files
3. **Similarity Scoring**: Uses Jaccard index of structural hash sets to find matching declarations
4. **Smart Reporting**: Shows only meaningful changes, ignoring renames and reordering

## Usage

### Basic Diff (Default - Side by Side)
Shows full functions before and after with line numbers:
```bash
varmap diff file1.js file2.js
```

### Summary Mode
Shows only what changed without full code:
```bash
varmap diff file1.js file2.js --summary
```

### Interleaved Mode
Shows line-by-line diff of canonicalized code:
```bash
varmap diff file1.js file2.js --interleaved
```

### Export Rename Mappings
Capture how functions were renamed between versions:
```bash
varmap diff file1.js file2.js --export-mappings renames.yaml
```

### Show Renames
By default, simple renames are hidden. To see them:
```bash
VARMAP_SHOW_RENAMES=1 varmap diff file1.js file2.js
```

## Example

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

VarMap diff will:
- Recognize that `getData` → `a` but the API endpoint changed (structural modification)
- Recognize that `processData` → `b` with identical logic (just a rename)
- Ignore the reordering as non-meaningful

## Similarity Threshold

Declarations are matched if they have ≥50% structural similarity. This prevents false matches while allowing for minor variations.

## Use Cases

- **Security Analysis**: Compare obfuscated malware samples to identify changes
- **Build Verification**: Ensure minified code matches the source
- **Code Review**: Focus on actual logic changes, not formatting
- **Reverse Engineering**: Track changes between obfuscated versions