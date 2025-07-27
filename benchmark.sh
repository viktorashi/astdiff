#!/usr/bin/env bash

# Test files
FILE1="${1:-archive/cli-pretty-v1.0.60-core.js}"
FILE2="${2:-cli-pretty-v1.0.61-core.js}"

if [ ! -f "$FILE1" ] || [ ! -f "$FILE2" ]; then
    echo "Error: Test files not found. Usage: $0 [file1] [file2]"
    exit 1
fi

echo "=== Performance Benchmark: astdiff ==="
echo "File 1: $FILE1"
echo "File 2: $FILE2"
echo

# Count declarations in files for context
DECLS1=$(grep -E '^\s*(function|const|let|var|class)\s+\w+' "$FILE1" | wc -l)
DECLS2=$(grep -E '^\s*(function|const|let|var|class)\s+\w+' "$FILE2" | wc -l)
echo "Approximate declarations: $DECLS1 in file1, $DECLS2 in file2"
echo

# Sequential matching (default)
echo "=== Sequential Matching (default) ==="
time astdiff "$FILE1" "$FILE2" --summary > /dev/null 2>&1
echo

# Parallel matching
echo "=== Parallel Matching ==="
time astdiff "$FILE1" "$FILE2" --summary --parallel > /dev/null 2>&1
echo

# Detailed comparison with compact output
echo "=== Comparing output consistency ==="
astdiff "$FILE1" "$FILE2" --compact > sequential.diff 2>&1
astdiff "$FILE1" "$FILE2" --compact --parallel > parallel.diff 2>&1

if diff -q sequential.diff parallel.diff > /dev/null; then
    echo "✓ Output is consistent between sequential and parallel modes"
else
    echo "✗ Warning: Output differs between modes"
    echo "Differences:"
    diff sequential.diff parallel.diff | head -20
fi

rm -f sequential.diff parallel.diff

echo
echo "=== Testing with fingerprints disabled ==="
echo "Sequential (no fingerprints):"
time astdiff "$FILE1" "$FILE2" --summary --no-fingerprints > /dev/null 2>&1
echo
echo "Parallel (no fingerprints):"
time astdiff "$FILE1" "$FILE2" --summary --no-fingerprints --parallel > /dev/null 2>&1