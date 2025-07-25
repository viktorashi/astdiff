// Global variable
const data = 100;

function process(data) {
    // Parameter shadows global 'data'
    const result = data * 2;
    return result;
}

function analyze() {
    // Local variable with same name
    const data = 50;
    const result = data + 10;
    return result;
}

console.log(data);  // Global data
console.log(process(data));
console.log(analyze());