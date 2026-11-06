// Shared helpers of the oracle generators: locating reference sources, a seeded PRNG,
// fixture output, and the graph codec the Rust fixture tests decode.
'use strict';
const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..', '..', '..');

// Path of `rel` inside the git-ignored reference/ tree: $CHITIN_REFERENCE, else the
// nearest reference/ above this checkout (worktrees live inside the main checkout).
function reference(rel) {
  if (process.env.CHITIN_REFERENCE) return path.join(process.env.CHITIN_REFERENCE, rel);
  for (let dir = ROOT; ; dir = path.dirname(dir)) {
    const p = path.join(dir, 'reference', rel);
    if (fs.existsSync(p)) return p;
    if (path.dirname(dir) === dir) throw new Error(`reference/${rel} not found; set CHITIN_REFERENCE`);
  }
}

// mulberry32: deterministic fixtures.
function prng(seed) {
  let a = seed >>> 0;
  const rand = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const int = (n) => Math.floor(rand() * n);
  return { rand, int, pick: (a) => a[int(a.length)], chance: (p) => rand() < p };
}

// Writes NDJSON lines to crates/engine/tests/fixtures/<rel>.
function writeFixture(rel, lines) {
  const out = path.join(ROOT, 'crates', 'engine', 'tests', 'fixtures', rel);
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  console.log(`wrote ${path.relative(ROOT, out)} (${lines.length} lines)`);
}

// Graph codec (identity, cycles, undefined, NaN/±Infinity/-0 survive):
//   primitive JSON value as is; ["u"] undefined; ["n","NaN"|"Infinity"|"-Infinity"|"-0"];
//   ["o", id, [[key, value], …]] / ["a", id, [value, …]] at the first visit (depth-first,
//   ids from 0 in visit order); ["r", id] afterwards. Arrays with named properties are
//   outside the Rust model and throw; so do holes, unless `opts.holes` encodes them as
//   undefined (sound where nothing tells the two apart, as in the lodash functions).
function encode(value, opts = {}) {
  const ids = new Map();
  const enc = (v) => {
    if (v === undefined) return ['u'];
    if (v === null || typeof v === 'boolean' || typeof v === 'string') return v;
    if (typeof v === 'number') {
      if (Number.isNaN(v)) return ['n', 'NaN'];
      if (v === Infinity || v === -Infinity) return ['n', String(v)];
      if (Object.is(v, -0)) return ['n', '-0'];
      return v;
    }
    if (typeof v !== 'object') throw new Error(`cannot encode ${typeof v}`);
    if (ids.has(v)) return ['r', ids.get(v)];
    const id = ids.size;
    ids.set(v, id);
    if (Array.isArray(v)) {
      const own = Object.keys(v);
      if (own.some((k) => !/^(0|[1-9]\d*)$/.test(k))) throw new Error('array with named properties');
      if (own.length !== v.length && !opts.holes) throw new Error('array with holes');
      return ['a', id, Array.from({ length: v.length }, (_x, i) => enc(v[i]))];
    }
    return ['o', id, Object.keys(v).map((k) => [k, enc(v[k])])];
  };
  return enc(value);
}

module.exports = { ROOT, reference, prng, writeFixture, encode };
