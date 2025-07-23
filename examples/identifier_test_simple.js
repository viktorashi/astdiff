// Simple test for identifier contexts

// 1. Object property access patterns
const obj = {
    prop: 'value',
    method() { return this.prop; }
};

// These should NOT be canonicalized:
obj.prop;           // 'prop' is a property, not a variable
obj['prop'];        // 'prop' is a string literal
obj.method();       // 'method' is a property

// 2. Destructuring
const { prop: renamed } = obj;  // 'prop' should NOT be canonicalized, 'renamed' should

// 3. Import/export names
export { obj as exportedObj };  // 'exportedObj' is export name, shouldn't be canonicalized

// 4. Labels
labelName: for (let i = 0; i < 5; i++) {  // 'labelName' shouldn't be canonicalized
    if (i === 3) break labelName;
}

// 5. Property shorthand
const prop = 'value';
const shorthand = { prop };  // First 'prop' is variable (canonicalize), second is property (don't)
