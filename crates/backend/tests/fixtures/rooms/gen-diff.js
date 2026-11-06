// Regenerates diff.json with the official getDiff:
//   node gen-diff.js <SCREEPS_ROOT, e.g. E:\screeps\server>   (node 22)
const path = require('path');
const fs = require('fs');
const common = require(path.join(process.argv[2], 'node_modules', '@screeps', 'common'));
const o = (id, f) => Object.assign({_id: id}, f);
const cases = [
  ['snapshot', [], [o('a', {type: 'spawn', x: 1, store: {energy: 300}}), o('b', {type: 'creep', name: 'Zoë'})]],
  ['removed object', [o('a', {x: 1}), o('b', {x: 2})], [o('b', {x: 2})]],
  ['top-level key removed and added', [o('a', {x: 1, gone: true})], [o('a', {x: 1, added: 'y'})]],
  ['scalar changes', [o('a', {x: 1, s: 'p', b: false})], [o('a', {x: 2, s: 'q', b: true})]],
  ['nested change carries the new value', [o('a', {store: {energy: 50, H: 1}})], [o('a', {store: {energy: 40, H: 1}})]],
  ['nested removal is dropped', [o('a', {store: {energy: 50, H: 1}})], [o('a', {store: {energy: 50}})]],
  ['nested addition', [o('a', {store: {energy: 50}})], [o('a', {store: {energy: 50, O: 3}})]],
  ['truthy to falsy', [o('a', {p: {k: 1}, n: 5, s: 'x', t: true})], [o('a', {p: null, n: 0, s: '', t: false})]],
  ['null to object', [o('a', {p: null})], [o('a', {p: {k: 1}})]],
  ['type change', [o('a', {p: 1})], [o('a', {p: '1'})]],
  ['array element', [o('a', {body: [{type: 'move', hits: 100}, {type: 'work', hits: 100}]})], [o('a', {body: [{type: 'move', hits: 100}, {type: 'work', hits: 50}]})]],
  ['array shrink and grow', [o('a', {s: [1, 2, 3], g: [1]})], [o('a', {s: [1, 2], g: [1, 2]})]],
  ['unchanged and reordered nested', [o('a', {p: {x: {m: 1, n: 2}}, q: 1})], [o('a', {p: {x: {n: 2, m: 1}}, q: 1})]],
  ['deep change', [o('a', {p: {x: {m: 1}, y: 2}})], [o('a', {p: {x: {m: 2}, y: 2}})]],
  ['array vs object', [o('a', {p: [1]})], [o('a', {p: {'0': 1}})]],
  ['integer keys order', [o('a', {store: {b: 1, '7': 1}})], [o('a', {store: {b: 2, '7': 2, '3': 1}})]],
  ['mixed list', [o('a', {x: 1}), o('b', {x: 1}), o('c', {x: 1})], [o('d', {x: 9}), o('c', {x: 2}), o('a', {x: 1})]],
  ['empty diffs everywhere', [o('a', {x: [1, {y: 2}]})], [o('a', {x: [1, {y: 2}]})]],
];
const out = cases.map(([name, oldL, newL]) => ({name, old: oldL, new: newL, diff: JSON.stringify(common.getDiff(oldL, newL))}));
fs.writeFileSync(path.join(__dirname, 'diff.json'), JSON.stringify(out, null, 1) + '\n');
