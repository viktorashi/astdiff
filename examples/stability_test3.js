// Test file 3 - same functions in different order with extra code
function multiply(input) {  // Same structure as processData
    let output = input * 2;
    return output;
}

// Extra function that wasn't in original
function divide(x, y) {
    if (y === 0) return null;
    const ratio = x / y;
    return ratio;
}

// Some random code
const values = [1, 2, 3];
for (const v of values) {
    console.log(v);
}

function add(x, y) {  // Same structure as calculateSum
    const sum = x + y;
    return sum;
}

// Extra function with same structure as divide
function subtract(a, b) {
    if (b === 0) return null;
    const diff = a - b;
    return diff;
}