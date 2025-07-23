const { simple } = { simple: 1 };
const { renamed: newName } = { renamed: 2 };
const { nested: { deep } } = { nested: { deep: 3 } };
const { a, b, c } = { a: 1, b: 2, c: 3 };

console.log(simple, newName, deep, a, b, c);