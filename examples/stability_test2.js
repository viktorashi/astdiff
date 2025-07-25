// Test file 2 - added function and renamed variables
function newFunction() {
    return "hello";
}

function add(x, y) {  // renamed from calculateSum
    const sum = x + y;  // renamed from result
    return sum;
}

function multiply(input) {  // renamed from processData
    let output = input * 2;  // renamed from temp
    return output;
}

// Call the functions
console.log(add(1, 2));
console.log(multiply(5));