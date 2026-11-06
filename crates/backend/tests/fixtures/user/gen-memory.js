// Regenerates memory.json: memory path semantics of socket/user.js (watch) and api/user.js GET /memory.
//   node gen-memory.js
const fs = require('fs');
const path = require('path');
const memory = JSON.stringify({a: {b: [1, {c: null}, [2, [3, null]], 'x'], s: 'str😀', z: 0, t: true, e: {}}, n: 5.5, big: 1e21, neg: -0, '7': 'seven', '': {'': 'empty'}});
const paths = ['a', 'a.b', 'a.b.0', 'a.b.1', 'a.b.1.c', 'a.b.1.c.d', 'a.b.2', 'a.b.length', 'a.s', 'a.s.0', 'a.s.3', 'a.s.length', 'a.z', 'a.t',
  'a.e', 'a.e.x', 'a.e.x.y', 'n', 'n.x', 'big', 'neg', '7', '', '.', 'missing', 'missing.deeper', 'a..b', 'a.b.9', 'a.b.01'];
// socket/user.js: do { cur = cur[parts.shift()] } while (parts.length); result = "" + cur
const watch = paths.map(p => {
  let result;
  try {
    let cur = JSON.parse(memory), parts = p.split(/\./);
    do { cur = cur[parts.shift()]; } while (parts.length > 0);
    result = '' + cur;
  } catch (e) {
    result = 'Incorrect memory path';
  }
  return [p, result];
});
// api/user.js GET: if (path) while (parts.length) cur = cur[parts.shift()]; undefined → no data; else JSON.stringify
const get = paths.map(p => {
  try {
    let cur = JSON.parse(memory);
    if (p) {
      const parts = p.split(/\./);
      while (parts.length > 0) cur = cur[parts.shift()];
    }
    return [p, cur === undefined ? null : JSON.stringify(cur)];
  } catch (e) {
    return [p, 'Incorrect memory path'];
  }
});
fs.writeFileSync(path.join(__dirname, 'memory.json'), JSON.stringify({memory, watch, get}, null, 1) + '\n');
