function outer(x, y) {
    let localVar = x + y;
    const CONSTANT = 42;
    
    function inner(a, b) {
        let innerVar = a * b;
        return innerVar + localVar + CONSTANT;
    }
    
    var oldStyleVar = "test";
    return inner(x, y) + oldStyleVar.length;
}