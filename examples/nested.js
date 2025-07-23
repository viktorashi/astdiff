function outer(x) {
  var y = 1;
  function inner(z) {
    return x + y + z;
  }
  return inner;
}