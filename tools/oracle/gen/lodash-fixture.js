// Generates crates/engine/tests/fixtures/lodash/cases.ndjson by running the real
// reference/lodash-3.10.1 on random graphs (shared and cyclic references, undefined,
// NaN, -0, index-like keys, arrays merged into objects and strings, a numeric `length`
// key, sparse arrays), hand-picked edges (self-merge, a source reaching the destination,
// arrays of shared objects, cycles of different shapes, array `length` writes), plus JS
// value semantics (ToBoolean, ToNumber, ""+v, parseInt, JSON.stringify). Holes are
// encoded as undefined: none of these functions tells the two apart.
// Each line: {"op", "in": graph, "out": graph} in the codec of ../lib/common.js; the
// input and output tuples are single graphs, so identity between their parts is kept.
// Usage: node tools/oracle/gen/lodash-fixture.js
'use strict';
const { reference, prng, writeFixture, encode } = require('../lib/common');
const _ = require(reference('lodash-3.10.1/lodash.js'));

const g = prng(0x10da5);
const KEYS = ['a', 'b', 'c', 'x', 'store', 'energy', '0', '1', '2', '10', '_h', '_id', '$loki', 'e f', 'é', 'length'];
const STRINGS = ['', 's', 'abc', '12', '0x1f', ' 7 ', '-0', '1e3', 'Infinity', 'é€', 'true', 'null', '  +5.5e2xyz', ' 12', '﻿7',
  '\t-0x1F', '0b101', '0o17', '1e400', '.5', '5.', '0x', '1_000', '-', '00012', '9007199254740993', '0x1fffffffffffff1',
  '\u2028 5', '\u180e5', '+0x10', '-0x', '0X1g', '   -12.9e3', '\u00a0\u3000 8', '4294967295', '-Infinity', 'infinity', '0.0000001'];
const NUMBERS = [-0, NaN, Infinity, -Infinity, 1.5, -2.25, 1e21, 5e-7, 0.1, 2 ** 53, 123456789.125, -1e-7];

function prim() {
  switch (g.int(10)) {
    case 0: return undefined;
    case 1: return null;
    case 2: return g.chance(0.5);
    case 3: case 4: return g.int(100) - 20;
    case 5: return g.pick(NUMBERS);
    default: return g.pick(STRINGS);
  }
}

// Random graph; `pool` holds earlier nodes for shared (and, when `cycles`, cyclic) references.
function value(depth, pool, opts) {
  if (pool.length && g.chance(opts.share)) {
    const n = g.pick(pool);
    if (opts.cycles || n.done) return n.v;
  }
  if (depth >= opts.depth || g.chance(0.4)) return prim();
  const node = { v: g.chance(0.35) ? [] : {}, done: false };
  pool.push(node);
  const n = g.int(5);
  for (let i = 0; i < n; i++) {
    const x = value(depth + 1, pool, opts);
    if (Array.isArray(node.v)) node.v.push(x);
    else {
      const k = g.pick(KEYS);
      // A numeric `length` makes lodash treat the object as array-like.
      node.v[k] = k === 'length' ? (opts.length ? g.int(4) : 'len') : x;
    }
  }
  // Sparse arrays: a deleted slot, or a length past the last element.
  if (Array.isArray(node.v) && g.chance(0.12)) {
    if (node.v.length && g.chance(0.5)) delete node.v[g.int(node.v.length)];
    else node.v.length += 1 + g.int(2);
  }
  node.done = true;
  return node.v;
}

const obj = (pool, opts) => {
  let v;
  do v = value(0, pool, opts); while (!v || typeof v !== 'object' || Array.isArray(v));
  return v;
};

const lines = [];
const tries = { merge: 0, ok: 0 };

function emit(op, input, run) {
  let enc;
  try {
    enc = encode(input, { holes: true });
  } catch (e) {
    return false;
  }
  try {
    const output = run();
    lines.push({ op, in: enc, out: encode(output, { holes: true }) });
    return true;
  } catch (e) {
    return false; // outside the Rust model (holes, named array properties)
  }
}

// _.merge: hand-picked edges, then random graphs.
const cyclic = { a: 1 };
cyclic.self = cyclic;
for (const [dst, src] of [
  [{ k: 'abc' }, { k: [1] }], // array merged over a string: arrayCopy of its characters
  [{ k: { length: 2, 0: 'x' } }, { k: [undefined, 5] }], // array-like object copied
  [{}, 'ab'], [[1, 2, 3], [undefined]], [{ a: 1 }, { a: undefined }], [{}, { a: undefined }], [[], [undefined, null]],
  [{}, cyclic], [{ a: [1, 2] }, { a: { x: 1 } }], [{ a: { x: 1 } }, { a: [5] }], [{ a: [1, 2, 3] }, { a: [9] }],
  [{ k: 5 }, { k: { a: 1 } }], [null, { a: 1 }], [5, { a: 1 }], [{ a: 1 }, 0], [{ s: { e: 1 } }, { s: { e: 2, f: [1, { g: 1 }] } }],
]) emit('merge', [dst, src], () => [dst, src, _.merge(dst, src)]);
const mergeCase = (dst, src) => emit('merge', [dst, src], () => [dst, src, _.merge(dst, src)]);
{
  const self = { a: { b: [1, { c: 2 }] }, d: [3] };
  mergeCase(self, self);
  const inner = { x: 1, y: { z: [1] } };
  mergeCase({ s: inner, t: 2 }, inner); // the destination reaches the source
  const e = { k: 1, arr: [] };
  mergeCase(e, { self: e, again: e, arr: [e] }); // the source reaches the destination, twice
  const ca = [1];
  ca.push(ca);
  mergeCase({ a: [], b: { c: ca } }, { a: ca, b: { c: [ca, 2] } }); // a cyclic array source
  const part = { type: 'move' };
  const hits = { hits: 50 };
  // Arrays of objects: one destination part twice, one source object three times.
  mergeCase({ body: [part, part, { type: 'work' }] }, { body: [hits, { boost: 'UH' }, hits, hits] });
  mergeCase({ a: [[1, 2], [3], { k: [4] }] }, { a: [[9], [undefined, 4], [5], { k: [undefined, 6] }] });
  const sparse = [, 1, , { x: 1 }];
  sparse.length = 6;
  mergeCase({ a: [7, 8, 9] }, { a: sparse });
  mergeCase([1, , 3], [undefined, undefined, undefined, undefined]);
  for (const [d, s] of [
    [[1, 2, 3], { length: 1 }], [[1, 2], { length: 4 }], [[1, 2], { length: 0, 0: 'z' }], [[1], { 3: 'far' }],
    [{ a: { length: 3, 1: 'y' } }, { a: [undefined, undefined, 'z'] }], [{ a: { length: -0 } }, { a: [1] }],
    [{ a: { length: 1.5, 0: 1 } }, { a: [2] }], [{ a: { length: '2', 0: 1 } }, { a: [2] }], [{ a: { 0: 'x', b: 1 } }, { a: [1] }],
    [{ a: NaN, b: 0, c: [NaN] }, { a: -0, b: NaN, c: [-0, NaN] }], [{ a: 'str' }, { a: { 0: 'o' } }], [{ a: true }, { a: [1] }],
    [{ a: [{ x: 1 }, { y: 2 }] }, { a: [undefined, null] }], [{ a: [1, 2, 3] }, { a: [] }], [{}, { a: [[], [[]], {}] }],
  ]) mergeCase(d, s);
}
while (lines.length < 1500) {
  tries.merge++;
  const pool = [];
  const opts = { depth: 4, share: 0.12, cycles: g.chance(0.3), length: g.chance(0.3) };
  const r = g.rand();
  const dst = r < 0.7 ? obj(pool, opts) : r < 0.85 ? (opts.cycles ? [] : value(1, pool, opts)) : prim();
  const s = g.rand();
  const src = s < 0.15 ? g.pick([...STRINGS.slice(1, 4), 5, null, undefined, true]) : Array.isArray(dst) && g.chance(0.6) ? [value(1, pool, opts), value(1, pool, opts)] : obj(pool, opts);
  if (emit('merge', [dst, src], () => {
    const ret = _.merge(dst, src);
    return [dst, src, ret];
  })) tries.ok++;
}

// _.cloneDeep
for (let i = 0; i < 400;) {
  const pool = [];
  const x = value(0, pool, { depth: 4, share: 0.15, cycles: g.chance(0.4), length: true });
  if (emit('cloneDeep', [x], () => [x, _.cloneDeep(x)])) i++;
}

// _.isEqual
function mutate(x) {
  // One change somewhere in an acyclic copy.
  if (!x || typeof x !== 'object') return prim();
  const keys = Object.keys(x);
  if (!keys.length || g.chance(0.2)) {
    if (Array.isArray(x)) x.push(prim());
    else x[g.pick(KEYS.filter((k) => k !== 'length'))] = prim();
  } else {
    const k = g.pick(keys);
    if (g.chance(0.3) && !Array.isArray(x)) delete x[k];
    else x[k] = mutate(x[k]);
  }
  return x;
}
for (let i = 0; i < 500;) {
  const pool = [];
  const cycles = g.chance(0.3);
  const a = value(0, pool, { depth: 4, share: 0.12, cycles, length: false });
  let b;
  const c = g.rand();
  if (c < 0.35) b = _.cloneDeep(a);
  else if (c < 0.6 && !cycles) b = mutate(_.cloneDeep(a));
  else if (c < 0.7) b = a;
  else if (c < 0.8 && a && typeof a === 'object' && !Array.isArray(a)) {
    b = {};
    const keys = Object.keys(a);
    for (let j = keys.length - 1; j > 0; j--) {
      const r = g.int(j + 1);
      [keys[j], keys[r]] = [keys[r], keys[j]];
    }
    for (const k of keys) b[k] = _.cloneDeep(a[k]);
  } else b = value(0, pool, { depth: 4, share: 0.12, cycles, length: false });
  if (emit('isEqual', [a, b], () => [_.isEqual(a, b)])) i++;
}
for (const [a, b] of [[NaN, NaN], [0, -0], [1, '1'], [null, undefined], [{}, []], [[1, [2]], [1, [2]]], ['a', 'a'], [true, 1]]) {
  emit('isEqual', [a, b], () => [_.isEqual(a, b)]);
}
{
  const x = {};
  x.x = x;
  const y = {};
  y.x = { x: y }; // the same infinite unfolding as x, with a longer cycle
  const z = { x: {} };
  z.x.x = z.x;
  const p = [];
  p.push(p);
  const q = [];
  q.push(q);
  const r = [[]];
  r[0].push(r);
  const s = { v: 1 };
  const t = { v: 1 };
  for (const [a, b] of [
    [x, y], [x, z], [p, q], [p, r], [{ a: s, b: s }, { a: s, b: t }], [{ a: s, b: s }, { a: t, b: { v: 2 } }],
    [{ a: undefined }, { b: undefined }], [{ a: undefined }, {}], [[NaN, { k: -0 }], [NaN, { k: 0 }]], [[1, undefined], [1, , ]],
    [{ constructor: 'x' }, { constructor: 'x' }], [{ constructor: 1 }, {}], [{ length: 1, 0: 'a' }, ['a']], [[[]], [{}]],
    [{ a: [1, 2] }, { a: { 0: 1, 1: 2 } }], [{ 1: 'a', b: 2 }, { b: 2, 1: 'a' }],
  ]) emit('isEqual', [a, b], () => [_.isEqual(a, b)]);
  const cy = [1];
  cy.push(cy, { back: cy });
  const sh = { k: [1] };
  const me = { a: 1 };
  me.self = me;
  for (const v of [cy, { a: sh, b: [sh, sh], c: { d: sh } }, [1, , { x: [, 2] }], me]) emit('cloneDeep', [v], () => [v, _.cloneDeep(v)]);
  const ad = { 2: 'two', a: 1, 1: 'one' };
  emit('assign', [ad, ad], () => [ad, ad, _.assign(ad, ad)]);
  const as = { 10: 'x', b: undefined, 9: [ad], c: { d: ad } };
  emit('assign', [ad, as], () => [ad, as, _.assign(ad, as)]);
}

// _.assign
for (let i = 0; i < 300;) {
  const pool = [];
  const opts = { depth: 3, share: 0.15, cycles: false, length: true };
  const dst = obj(pool, opts);
  const src = g.chance(0.1) ? dst : obj(pool, opts);
  if (emit('assign', [dst, src], () => [dst, src, _.assign(dst, src)])) i++;
}

// _.forEach over an object that the callback mutates: keys are a snapshot, values live.
for (let i = 0; i < 200;) {
  const pool = [];
  const o = obj(pool, { depth: 2, share: 0, cycles: false, length: false });
  const keys = Object.keys(o);
  const actions = keys.map(() => (g.chance(0.5) ? [] : [[g.pick(['del', 'add', 'set']), g.pick(keys.concat(['new1', '7', 'z']))]]));
  if (emit('forEach', [o, actions], () => {
    const visits = [];
    let n = 0;
    _.forEach(o, (v, k) => {
      visits.push([k, v]);
      for (const [what, key] of actions[n++] || []) {
        if (what === 'del') delete o[key];
        else o[key] = what === 'add' ? 'added' : 42;
      }
    });
    return [o, visits];
  })) i++;
}

// JS value semantics.
const jsCase = (v) =>
  emit('js', [v], () => {
    let json;
    try {
      json = JSON.stringify(v);
    } catch (e) {
      json = ['throws'];
    }
    return [!!v, Number(v), '' + v, parseInt(v), json];
  });
for (let i = 0; i < 600;) {
  const pool = [];
  let v = g.chance(0.4) ? prim() : value(0, pool, { depth: 3, share: 0.1, cycles: false, length: false });
  if (g.chance(0.03)) {
    v = [1, [2]];
    v.push(v); // a cyclic array joins as ""
  }
  if (jsCase(v)) i++;
}
{
  const holes = [1, , 3];
  holes.length = 5;
  const cyc = [1, [2]];
  cyc[1].push(cyc);
  const shared = [7];
  const obj = { a: 1 };
  obj.me = obj; // JSON.stringify throws, the rest does not look inside
  for (const v of [holes, [, ], [[, ], [null, [undefined]]], { length: 2 }, [cyc, cyc], [shared, shared], [-0, [-0]], [' 0x10 '], obj]) jsCase(v);
}

writeFixture('lodash/cases.ndjson', lines);
console.log(`merge: ${tries.ok} kept of ${tries.merge}`);
