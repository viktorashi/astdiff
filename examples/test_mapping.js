function processItems(items) {
    const results = [];
    for (const item of items) {
        if (item > 0) {
            results.push(item * 2);
        }
    }
    return results;
}

const data = [1, 2, 3];
const output = processItems(data);
console.log(output);