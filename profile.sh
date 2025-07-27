#!/usr/bin/env bash

FILE1="${1:-cli-pretty-v1.0.60-core.js}"
FILE2="${2:-cli-pretty-v1.0.61-core.js}"

echo "=== Profiling astdiff performance ==="
echo "File 1: $FILE1 ($(wc -l < "$FILE1") lines)"
echo "File 2: $FILE2 ($(wc -l < "$FILE2") lines)"
echo

# Function to time a command and show output
time_cmd() {
    local desc="$1"
    shift
    echo "=== $desc ==="
    time "$@"
    echo
}

# Sequential with different options
time_cmd "Sequential (default)" astdiff "$FILE1" "$FILE2" --summary

time_cmd "Sequential (no fingerprints)" astdiff "$FILE1" "$FILE2" --summary --no-fingerprints

time_cmd "Sequential (compact)" astdiff "$FILE1" "$FILE2" --compact > /dev/null

# Parallel versions
time_cmd "Parallel (default)" astdiff "$FILE1" "$FILE2" --summary --parallel

time_cmd "Parallel (no fingerprints)" astdiff "$FILE1" "$FILE2" --summary --no-fingerprints --parallel

# Verbose timing for phases
echo "=== Detailed timing with verbose ==="
ASTDIFF_DEBUG=1 time astdiff "$FILE1" "$FILE2" --summary 2>&1 | grep -E "(Extracting|Building|Matching|Fingerprint)" | head -20