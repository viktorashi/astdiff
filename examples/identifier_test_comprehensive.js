// Test file for comprehensive identifier coverage in JavaScript

// 1. Variable declarations
var varVariable = 1;
let letVariable = 2;
const constVariable = 3;

// 2. Function declarations and expressions
function namedFunction(param1, param2) {
    return param1 + param2;
}

const arrowFunction = (arrowParam1, arrowParam2) => {
    return arrowParam1 * arrowParam2;
};

const functionExpression = function(exprParam) {
    return exprParam;
};

// 3. Object property names (declaration and access)
const objectWithProperties = {
    property1: 'value1',
    property2: 42,
    'string-property': 'string-value',
    123: 'numeric-key',
    methodName(methodParam) {
        return methodParam;
    },
    get getterName() {
        return this.property1;
    },
    set setterName(value) {
        this.property1 = value;
    }
};

// Accessing object properties
const prop1 = objectWithProperties.property1;
const prop2 = objectWithProperties['property2'];
const computed = 'property1';
const prop3 = objectWithProperties[computed];

// 4. Destructuring patterns
const { destructuredProp1, destructuredProp2 } = objectWithProperties;
const { property1: renamedProp } = objectWithProperties;
const { nested: { deepProp } = {} } = { nested: { deepProp: 'deep' } };

const [arrayElem1, arrayElem2, ...restElements] = [1, 2, 3, 4, 5];
const [, , thirdElement] = [1, 2, 3];

// Function parameter destructuring
function destructuringParams({ paramProp1, paramProp2 }, [arrParam1]) {
    return paramProp1 + paramProp2 + arrParam1;
}

// 5. Classes
class MyClass {
    constructor(constructorParam) {
        this.instanceProperty = constructorParam;
    }
    
    instanceMethod(methodParam) {
        return this.instanceProperty + methodParam;
    }
    
    static staticMethod(staticParam) {
        return staticParam;
    }
    
    get classGetter() {
        return this.instanceProperty;
    }
    
    set classSetter(setterParam) {
        this.instanceProperty = setterParam;
    }
}

class ExtendedClass extends MyClass {
    constructor(extendedParam) {
        super(extendedParam);
        this.extendedProperty = extendedParam;
    }
}

// 6. For loop variables
for (let forLoopVar = 0; forLoopVar < 10; forLoopVar++) {
    console.log(forLoopVar);
}

for (const forInVar in objectWithProperties) {
    console.log(forInVar);
}

for (const forOfVar of [1, 2, 3]) {
    console.log(forOfVar);
}

// 7. Import/Export statements
import { importedFunction, importedVariable } from './module';
import * as namespace from './namespace';
import defaultExport from './default';
import defaultWithNamed, { namedImport } from './mixed';

export { exportedVariable, exportedFunction };
export const exportedConst = 'exported';
export function exportedFunc(exportParam) {
    return exportParam;
}
export default function defaultExportFunc() {}

// 8. Try-catch
try {
    throw new Error('test');
} catch (errorVariable) {
    console.log(errorVariable);
} finally {
    console.log('finally');
}

// 9. Object method shorthand
const shorthandObject = {
    shorthandMethod() {
        return 'method';
    },
    property1,
    property2
};

// 10. Computed property names
const computedKey = 'dynamic';
const computedObject = {
    [computedKey]: 'value',
    [`prefix_${computedKey}`]: 'prefixed'
};

// 11. Labels
outerLabel: for (let i = 0; i < 3; i++) {
    innerLabel: for (let j = 0; j < 3; j++) {
        if (i === 1 && j === 1) {
            break outerLabel;
        }
    }
}

// 12. Template literal tags
function taggedTemplate(strings, ...values) {
    return strings.join('');
}

const taggedResult = taggedTemplate`Hello ${letVariable} world`;

// 13. this and super references
const contextObject = {
    contextMethod() {
        return this.property1;
    }
};

// 14. Global identifiers
console.log(window);
console.log(global);
console.log(globalThis);

// 15. Reserved words as property names
const reservedAsProps = {
    class: 'class-value',
    const: 'const-value',
    function: 'function-value'
};

// 16. Private class fields (if supported)
class PrivateClass {
    #privateField = 'private';
    
    getPrivate() {
        return this.#privateField;
    }
}

// 17. Async/await
async function asyncFunction(asyncParam) {
    const result = await Promise.resolve(asyncParam);
    return result;
}

// 18. Generator functions
function* generatorFunction(genParam) {
    yield genParam;
    yield* [1, 2, 3];
}

// 19. Dynamic import
const dynamicImport = import('./dynamic-module');

// 20. new.target
function ConstructorFunction() {
    if (new.target) {
        this.wasCalledWithNew = true;
    }
}

// 21. Rest parameters
function restParamFunction(firstParam, ...restParams) {
    return restParams;
}

// 22. Default parameters
function defaultParamFunction(defaultParam = 'default') {
    return defaultParam;
}

// 23. Nested destructuring with defaults
const { 
    outer: { 
        inner = 'defaultInner' 
    } = {} 
} = { outer: {} };

// 24. Symbol usage
const symbolKey = Symbol('description');
const symbolObject = {
    [symbolKey]: 'symbol-value'
};

// 25. Proxy and Reflect
const proxyTarget = { proxyProp: 'value' };
const proxyHandler = {
    get(target, property) {
        return target[property];
    }
};
const proxy = new Proxy(proxyTarget, proxyHandler);

// 26. with statement (discouraged but valid)
with (objectWithProperties) {
    console.log(property1);
}

// 27. eval and Function constructor
eval('var evalVar = 1;');
const FunctionConstructor = new Function('funcParam', 'return funcParam;');

// 28. Decorators (if supported)
// @decoratorFunction
// class DecoratedClass {}

// 29. Optional chaining and nullish coalescing
const optionalResult = objectWithProperties?.property1?.nested;
const nullishResult = optionalResult ?? 'default';

// 30. Logical assignment operators
let logicalVar = null;
logicalVar ||= 'default';
logicalVar &&= 'updated';
logicalVar ??= 'nullish';
