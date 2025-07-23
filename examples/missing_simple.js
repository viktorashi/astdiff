// Test missing identifier types
const { destructured } = { destructured: 1 };
const [arrayItem] = [2];

for (const forInKey in {}) {
  console.log(forInKey);
}
for (const forOfValue of []) {
  console.log(forOfValue);
}

try {
  throw new Error();
} catch (error) {
  console.log(error);
}

const arrow = (arrowParam) => arrowParam;
const expr = function(exprParam) { return exprParam; };

console.log(destructured, arrayItem, arrow, expr);