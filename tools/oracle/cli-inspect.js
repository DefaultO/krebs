// CLI-OUTPUT oracle: crates/runtime/js/cli/inspect.js against node's own util.inspect (node 22, the defaults
// the CLI uses) on random values: nested objects and arrays past the depth limit, long and quoted strings,
// numbers (-0, NaN, ±Infinity), number arrays that group into columns, sparse arrays, functions with
// properties, classes, Date, RegExp, Map, Set, boxed primitives, null-prototype objects, circular references.
//   node tools/oracle/cli-inspect.js <seed> <n>
'use strict';
const util = require('util');
const path = require('path');
const { prng } = require('./lib/common');
const { inspect } = require(path.join(__dirname, '../../crates/runtime/js/cli/inspect.js'));

class Foo { constructor() { this.a = 1; } }
class Bar extends Foo {}

function random(seed, i) {
  const g = prng((seed * 2654435761 + i) >>> 0);
  const pick = (a) => a[g.int(a.length)];
  const str = () => pick(['', 'a', 'W1N1', "it's", 'say "hi"', 'both \' and "', 'tab\there', 'line\nbreak', 'ü€😀', '\u0000\u001f',
    'x'.repeat(g.int(120)), 'a long line of text with spaces that goes on and on '.repeat(1 + g.int(3)), '\\', 'a`b${c}\'"', '\ud800']);
  const num = () => pick([0, -0, 1, -1, 3.14, 1e21, 1e-7, NaN, Infinity, -Infinity, 123456789, g.int(1000)]);
  const prim = () => pick([str, num, () => true, () => false, () => null, () => undefined, () => 10n])();
  const key = () => pick(['a', 'room', 'x_1', '1', '0', 'with space', 'it\'s', 'é', '__proto__x', 'type', '$loki', '12abc']);
  const value = (d) => {
    if (d <= 0) return prim();
    switch (g.int(17)) {
      case 0: case 1: case 2: return prim();
      case 3: case 4: { const o = {}; for (let k = g.int(6); k > 0; k--) o[key()] = value(d - 1); return o; }
      case 5: case 6: return Array.from({ length: g.int(9) }, () => value(d - 1));
      case 7: return Array.from({ length: 5 + g.int(40) }, () => (g.chance(0.8) ? g.int(g.chance(0.5) ? 100 : 100000) : -g.int(50)));
      case 8: return Array.from({ length: 5 + g.int(30) }, () => pick(['a', 'bb', 'ccc', 'W1N1', 'spawn']));
      case 9: { const f = function namedFn() {}; if (g.chance(0.5)) f._help = str(); return g.chance(0.5) ? f : () => {}; }
      case 10: return pick([new Date(g.int(2e12)), new Date(NaN), /ab+c/gi, new Foo(), new Bar(), Foo, class {}]);
      case 11: return new Map([[str(), value(d - 1)], [num(), value(d - 1)]]);
      case 12: return new Set([prim(), prim(), value(d - 1)]);
      case 13: { const o = Object.create(null); o[key()] = value(d - 1); return o; }
      case 14: { const a = [1, 2]; a[5 + g.int(5)] = value(d - 1); return a; }
      case 15: return pick([new Number(5), new String('ab'), new Boolean(false)]);
      default: { const o = { name: str() }; o.self = o; if (g.chance(0.5)) o.list = [o, { back: o }]; return o; }
    }
  };
  return value(1 + g.int(5));
}

const [seed, n] = [Number(process.argv[2] || 1), Number(process.argv[3] || 1000)];
let bad = 0;
for (let i = 0; i < n; i++) {
  const v = random(seed, i);
  const a = util.inspect(v);
  const b = inspect(v);
  if (a !== b) {
    if (++bad <= 5) console.log(`case ${i}:\n--- node\n${a}\n--- chitin\n${b}`);
  }
}
console.log(`${n - bad}/${n} equal`);
process.exit(bad ? 1 : 0);
