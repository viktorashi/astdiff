// Test missing identifier types
const { destructured } = { destructured: 1 };
const [arrayItem] = [2];

for (const forInKey in {}) {}
for (const forOfValue of []) {}

try {} catch (error) {}

const arrow = (arrowParam) => arrowParam;
const expr = function(exprParam) { return exprParam; };

import { imported } from './module';

console.log(destructured, arrayItem, forInKey, forOfValue, error, arrowParam, exprParam, imported);