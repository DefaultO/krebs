// Generates crates/engine/tests/fixtures/bulk/cases.ndjson: random op sequences on a
// room's live objects through the REAL reference/driver/lib/bulk.js, with execute()
// captured as the storage RPC sees it (JSON round trip) and applied by ../lib/loki-update.js.
// Covers the alias / absent-key / truthy-scalar rules for object-valued keys, hidden keys,
// temp ids of inserts (registered in roomObjects like _create-energy), op order,
// undefined vs null, index-wise array merge, unrecorded live mutations and cross-object
// aliases, shared and cyclic update data, inc/addToSet/pull, and the throws (object diff by
// id, removeHidden on null, removeHidden overflowing on a cycle). Hand-written SCRIPTS pin
// the nastiest paths (temp-id folding, remove after insert, nested hidden keys, id string vs
// object, arrays of objects, `$set` order with integer-like ids, `_id` in the data).
// Skipped as F rows or outside the Rust model: inserts after cancelling a pending insert
// (BUG-INSERT-COUNTER), dotted inc keys (BUG-DOTKEY-INC), `length` keys, and scenarios
// whose dbBulk aborts (Loki throwing on a non-array $addToSet/$pull target).
// Line: {objects: graph (the DB holds their JSON, in key order), ops, steps: [{ret, crash, state
// only when it changed}], docs: the documents after dbBulk, or null with executeError when
// execute() itself threw}.
// Usage: node tools/oracle/gen/bulk-fixture.js
'use strict';
const Module = require('module');
const { reference, prng, writeFixture, encode } = require('../lib/common');
const { dbBulk } = require('../lib/loki-update');

const LODASH = reference('lodash-3.10.1/lodash.js');
let captured = null;
const stubs = {
  q: { when: (x) => Promise.resolve(x) },
  '@screeps/common': {
    storage: {
      db: new Proxy({}, {
        get: () => ({
          bulk(ops) {
            captured = JSON.parse(JSON.stringify(ops)); // the storage RPC
            return Promise.resolve();
          },
        }),
      }),
    },
  },
};
const resolve = Module._resolveFilename;
Module._resolveFilename = function (request, ...rest) {
  if (request === 'lodash') return LODASH;
  if (request in stubs) return `chitin-stub:${request}`;
  return resolve.call(this, request, ...rest);
};
for (const [k, v] of Object.entries(stubs)) require.cache[`chitin-stub:${k}`] = { id: k, filename: k, loaded: true, exports: v, children: [] };
const makeBulk = require(reference('driver/lib/bulk.js'));
const _ = require(LODASH);

const g = prng(0xb01c);
const SCALARS = [undefined, null, 0, 1, 7, -3, 1.5, NaN, Infinity, -0, '', 'x', 'W2N2', true, false];
const DKEYS = ['energy', 'power', 'x', 'y', 'a', 'b', '0', '1', '2', '_tmp', '_x', '$loki', 'level', 'path'];
const TKEYS = ['store', 'energy', 'x', 'hits', 'effects', 'body', 'nested', 'list', 'flag', 'n', 's', 'memory_move', 'empty', '_tmp', '$loki', '_id', 'send', 'fresh'];

function genVal(depth) {
  const r = g.rand();
  if (depth > 2 || r < 0.5) return g.pick(SCALARS);
  if (r < 0.7) return Array.from({ length: g.int(4) }, () => genVal(depth + 1));
  const o = {};
  for (let i = g.int(4); i > 0; i--) o[g.pick(DKEYS)] = genVal(depth + 1);
  return o;
}

function genDoc(id) {
  const o = { _id: id, type: g.pick(['creep', 'energy', 'spawn', 'tower']), room: 'W1N1', x: g.int(50), y: g.int(50) };
  if (g.chance(0.7)) o.store = g.chance(0.3) ? { energy: g.int(100), power: 0 } : { energy: g.int(100) };
  if (g.chance(0.3)) o.body = [{ type: 'move', hits: 100 }, { type: 'work', hits: 100, boost: null }];
  if (g.chance(0.3)) o.effects = [{ power: 1, level: 2, endTime: 100 }];
  if (g.chance(0.3)) o.hits = g.int(1000);
  if (g.chance(0.2)) o.memory_move = { path: '1234', dest: { x: 1, y: 2 } };
  if (g.chance(0.3)) o.list = ['a', 'b'];
  if (g.chance(0.3)) o.flag = g.pick([true, false, 0, '', 'str', 5]);
  if (g.chance(0.2)) o.nested = { a: { b: { c: 1 } }, arr: [[1, 2], [3]] };
  if (g.chance(0.15)) o.empty = g.pick([[], {}]);
  return o;
}

// Update data shaped after the target: objects for object keys, arrays for array keys.
function genData(target) {
  const d = {};
  for (let i = 1 + g.int(4); i > 0; i--) {
    const k = g.pick(TKEYS);
    const cur = target && target[k];
    if (k === '_id') d[k] = g.pick([null, 'renamed']);
    else if (cur && typeof cur === 'object' && g.chance(0.6)) {
      if (Array.isArray(cur)) d[k] = Array.from({ length: g.int(4) }, () => genVal(1));
      else {
        d[k] = {};
        for (let j = 1 + g.int(3); j > 0; j--) d[k][g.pick(DKEYS)] = genVal(1);
      }
    } else d[k] = genVal(0);
  }
  return d;
}

// Every (container, path) in the live graph, for live mutations.
function containers(roomObjects) {
  const out = [];
  const walk = (v, path, depth) => {
    if (!v || typeof v !== 'object' || depth > 3) return;
    out.push([v, path]);
    for (const k of Object.keys(v)) walk(v[k], path.concat(Array.isArray(v) ? Number(k) : k), depth + 1);
  };
  for (const k of Object.keys(roomObjects)) walk(roomObjects[k], [k], 0);
  return out;
}

function reaches(from, target) {
  const seen = new Set();
  const go = (v) => {
    if (!v || typeof v !== 'object' || seen.has(v)) return false;
    if (v === target) return true;
    seen.add(v);
    return Object.keys(v).some((k) => go(v[k]));
  };
  return go(from);
}

const at = (roomObjects, path) => path.reduce((v, k) => v[k], roomObjects);

// One scenario: random ops, or `script` = {objects: () => roomObjects, steps: [(h) => [op, run]]}
// built from the same step helpers, so the Rust replay sees one vocabulary.
function scenario(script) {
  const roomObjects = script ? script.objects() : {};
  if (!script) {
    for (let i = g.int(3); i >= 0; i--) {
      const id = `id${i}${g.pick(['', 'a', 'f00'])}`;
      roomObjects[id] = genDoc(id);
    }
  }
  const objectsEnc = encode(roomObjects);
  let last = JSON.stringify(objectsEnc);
  const db = JSON.parse(JSON.stringify(Object.values(roomObjects)));
  const bulk = makeBulk('rooms.objects');
  const ops = [];
  const steps = [];
  const pending = new Set();
  let noMoreInserts = false;

  const target = () => {
    const keys = Object.keys(roomObjects);
    if (keys.length && g.chance(0.75)) return ['o', g.pick(keys)];
    return ['s', g.pick(['', 'nope', 'id0', 'id1', ...pending, ...keys.map((k) => String(roomObjects[k]._id))])];
  };
  const resolveT = (t) => (t[0] === 'o' ? roomObjects[t[1]] : t[1]);
  // Step helpers: each returns [op, run], or null when the step is not possible.
  const h = {
    update: (t, data) => [['update', t, encode(data)], () => bulk.update(resolveT(t), data)],
    insert(data, id, tmp) {
      if (noMoreInserts) return null;
      try {
        // A throwing insert (removeHidden on a null element) changes nothing; drop it.
        makeBulk('probe').insert(data, id);
      } catch (e) {
        return null;
      }
      const run = () => {
        if (!tmp) return bulk.insert(data, id || undefined);
        // _create-energy: `obj._id = "" + bulk.insert(obj); roomObjects[obj._id] = obj`
        const obj = _.cloneDeep(data);
        const c = bulk.insert(obj);
        obj._id = '' + c;
        roomObjects[obj._id] = obj;
        return c;
      };
      return [['insert', encode(data), id, tmp ? 'tmp' : null], run];
    },
    remove(t, del) {
      const id = t[0] === 'o' ? roomObjects[t[1]]._id : t[1];
      const run = () => {
        if (pending.has(String(id))) {
          pending.delete(String(id));
          noMoreInserts = true; // later inserts would hit BUG-INSERT-COUNTER (F)
        }
        bulk.remove(id);
        if (del) delete roomObjects[t[1]];
      };
      return [['remove', t, del], run];
    },
    inc: (t, key, amount) => [['inc', t, key, encode(amount)], () => bulk.inc(resolveT(t), key, amount)],
    listOp: (what, t, key, value) => [[what, t, key, encode(value)], () => bulk[what](resolveT(t), key, value)],
    mutSet: (p, v) => [['set', p, encode(v)], () => (at(roomObjects, p.slice(0, -1))[p[p.length - 1]] = _.cloneDeep(v))],
    mutAlias: (p, srcPath) => [['alias', p, srcPath], () => (at(roomObjects, p.slice(0, -1))[p[p.length - 1]] = at(roomObjects, srcPath))],
    mutDel: (p) => [['del', p], () => delete at(roomObjects, p.slice(0, -1))[p[p.length - 1]]],
  };

  const count = script ? script.steps.length : 3 + g.int(15);
  for (let n = 0; n < count; n++) {
    const step = script ? script.steps[n](h) : randomStep();
    if (!step) {
      if (script) throw new Error(`script step ${n} is not possible`);
      continue;
    }
    const [op, run] = step;
    let ret = null;
    let crash = null;
    try {
      const x = run();
      if (op[0] === 'insert') {
        ret = x;
        pending.add(String(x));
      }
    } catch (e) {
      crash = e.message;
    }
    ops.push(op);
    try {
      // `state` only when it changed, to keep the fixture small.
      const state = encode(roomObjects);
      const text = JSON.stringify(state);
      steps.push(text === last ? { ret, crash } : { ret, crash, state });
      last = text;
    } catch (e) {
      // An object merged into a live array gave it named properties (or holes). JSON drops
      // those, so persistence is unaffected, but the Rust live model has no place for them.
      if (script) throw new Error(`script outside the model: ${e.message}`);
      dropped.model++;
      return null;
    }
  }
  captured = null;
  try {
    bulk.execute();
  } catch (e) {
    // A payload aliases a live object that a later, crashed update made cyclic: the storage
    // RPC's JSON.stringify throws (in vanilla the crash had already ended the room).
    return { objects: objectsEnc, ops, steps, docs: null, executeError: e.message.split('\n')[0] };
  }
  const aborted = dbBulk(db, captured || []);
  if (aborted) {
    if (script) throw new Error(`script dbBulk aborted: ${aborted}`);
    dropped.aborted++;
    return null;
  }
  return { objects: objectsEnc, ops, steps, docs: db };

  function randomStep() {
    const r = g.rand();
    if (r < 0.32 && Object.keys(roomObjects).length) {
      const t = target();
      let data;
      if (g.chance(0.03)) data = g.pick([null, undefined]);
      else if (t[0] === 's' && g.chance(0.6)) data = { energy: g.pick(SCALARS), x: 1 };
      else {
        data = genData(resolveT(t));
        const keys = Object.keys(data);
        const s = g.rand();
        if (s < 0.06 && keys.length > 1) {
          // One sub-object under two keys: cloneDeep keeps the sharing.
          const shared = { energy: g.int(9), _x: 1 };
          data[keys[0]] = shared;
          data[keys[1]] = shared;
        } else if (s < 0.09) {
          // A cycle: merged into the live object, then removeHidden recurses forever.
          const c = { k: 1 };
          c.self = g.chance(0.5) ? c : [c];
          data[g.pick(['store', 'fresh', 'nested'])] = c;
        }
      }
      return h.update(t, data);
    }
    if (r < 0.42) {
      // genDoc(undefined) starts with `_id: undefined`, _create-energy's literal.
      const data = Object.assign(genDoc(undefined), g.chance(0.3) ? { _hidden: 1, nested: { _x: 1, a: [{ _y: 2 }] } } : { extra: genVal(0) });
      if (g.chance(0.5)) delete data._id;
      const tmp = g.chance(0.5);
      return h.insert(data, !tmp && g.chance(0.3) ? g.pick(['given1', '']) : null, tmp);
    }
    if (r < 0.5) {
      const keys = Object.keys(roomObjects);
      const t = keys.length && g.chance(0.6) ? ['o', g.pick(keys)] : ['s', g.pick(['', 'nope', ...pending])];
      return h.remove(t, t[0] === 'o' && g.chance(0.6));
    }
    if (r < 0.6) {
      const key = g.pick(['energy', 'hits', 'x', 'n', 's', 'flag', 'store', 'list', 'newk', 'harvested']);
      const amount = g.chance(0.1) ? g.pick([NaN, Infinity, -0]) : g.chance(0.5) ? g.int(20) - 10 : g.pick([0.5, 1e21, -2.25]);
      return h.inc(target(), key, amount);
    }
    if (r < 0.68) {
      const key = g.pick(['list', 'rooms', 'body', 'x', 'newl', 'flag']);
      return h.listOp(g.pick(['addToSet', 'pull']), target(), key, g.pick(['a', 'b', 'W1N1', 1, 2, null, true, { o: 1 }, undefined, [1]]));
    }
    // Live mutations the bulk never hears about (or hears about later).
    const cs = containers(roomObjects);
    if (!cs.length) return null;
    const [c, path] = g.pick(cs);
    const key = Array.isArray(c) ? g.int(c.length + 1) : g.pick(['energy', 'x', 'a', 'store', 'list', 'fresh', '_t', 'path', '0', 'n']);
    if (path.length === 1 && key === '_id') return null;
    const p = path.concat(key);
    const s = g.rand();
    if (s < 0.15 && !Array.isArray(c) && key in c) return h.mutDel(p);
    if (s < 0.35) {
      const [src, srcPath] = g.pick(cs);
      if (src === c || reaches(src, c)) return null;
      return h.mutAlias(p, srcPath);
    }
    return h.mutSet(p, g.chance(0.2) ? [1, null] : genVal(0));
  }
}

// Hand-written scenarios for the nastiest paths.
const creep = (id, extra) => ({
  _id: id, type: 'creep', room: 'W1N1', x: 5, y: 5, store: { energy: 10 }, hits: 300,
  body: [{ type: 'move', hits: 100 }, { type: 'work', hits: 100, boost: null }, { type: 'carry', hits: 100 }],
  list: ['a', 'b'], ...extra,
});
const SCRIPTS = [
  // Temp-id folding: updates of a pending insert by object and by id string fold into it;
  // inc/addToSet on the temp id reach no document; a later insert cancelled, then updated.
  {
    objects: () => ({ id0: creep('id0') }),
    steps: [
      (h) => h.insert({ _id: undefined, type: 'energy', energy: 10, _x: 1 }, null, true),
      (h) => h.update(['o', '1'], { energy: 7, _tmp: { a: 1 }, store: { energy: 7 } }),
      (h) => h.update(['s', '1'], { amount: 3, resourceType: 'energy' }),
      (h) => h.inc(['o', '1'], 'energy', 2),
      (h) => h.listOp('addToSet', ['s', '1'], 'list', 'a'),
      (h) => h.insert({ type: 'tombstone', creepBody: [{ type: 'move' }] }, null, false),
      (h) => h.update(['s', '2'], { deathTime: 5 }),
      (h) => h.mutSet(['1', 'store', 'energy'], 99), // the key was absent: the folded store is a snapshot
      (h) => h.remove(['s', '2'], false),
      (h) => h.update(['s', '2'], { x: 1 }),
      (h) => h.update(['o', 'id0'], { store: { energy: 1 } }),
    ],
  },
  // Remove after insert: the temp object stays in roomObjects; updates of it go to `updates`.
  {
    objects: () => ({ id0: creep('id0') }),
    steps: [
      (h) => h.insert({ type: 'energy', energy: 1 }, null, true),
      (h) => h.update(['o', '1'], { energy: 0 }),
      (h) => h.remove(['o', '1'], false),
      (h) => h.update(['o', '1'], { energy: 5, store: { energy: 5 } }),
      (h) => h.update(['s', '1'], { a: 1 }),
      (h) => h.inc(['o', '1'], 'energy', 1),
      (h) => h.update(['o', 'id0'], { x: 6 }),
    ],
  },
  // Nested hidden keys: arrays of objects, deep objects, `$loki`, and `_` keys added later to
  // an aliased live sub-object (they persist, as vanilla).
  {
    objects: () => ({ id0: creep('id0'), id1: creep('id1') }),
    steps: [
      (h) => h.update(['o', 'id0'], {
        body: [{ type: 'move', _x: 1, nested: { _y: 2, k: 1 } }, { hits: 5, _h: {} }],
        store: { _z: 1, energy: 2, deep: { _q: [1], ok: [{ _r: 1, s: 2 }, [{ _t: 3, u: 4 }]] } },
        $loki: 5, nest: { $loki: 1, a: 1 }, zero: { $loki: 0 },
      }),
      (h) => h.mutSet(['id0', 'store', '_late'], 1),
      (h) => h.mutSet(['id0', 'body', 0, '_late'], { _x: 1 }),
      (h) => h.update(['o', 'id1'], { store: { energy: 3 } }),
      (h) => h.mutSet(['id1', 'store', '_late'], 2),
      (h) => h.mutSet(['id1', 'store', 'deep'], { _gone: 1, kept: 1 }),
    ],
  },
  // The same document updated by object and by id string, interleaved.
  {
    objects: () => ({ id0: creep('id0') }),
    steps: [
      (h) => h.update(['o', 'id0'], { x: 1 }),
      (h) => h.update(['s', 'id0'], { y: 2, x: 2 }),
      (h) => h.update(['o', 'id0'], { store: { energy: 9 } }),
      (h) => h.update(['s', 'id0'], { x: 3, store: null }),
      (h) => h.inc(['s', 'id0'], 'hits', 5),
      (h) => h.inc(['o', 'id0'], 'hits', -5),
      (h) => h.update(['o', 'id0'], { hits: 1 }),
    ],
  },
  // Arrays of objects merge index-wise into the live array; unrecorded edits of the aliased
  // array persist; a null element crashes removeHidden after the live merge.
  {
    objects: () => ({ id0: creep('id0'), id1: creep('id1') }),
    steps: [
      (h) => h.update(['o', 'id0'], { body: [{ hits: 50 }] }),
      (h) => h.mutSet(['id0', 'body', 1, 'hits'], 0),
      (h) => h.update(['o', 'id0'], { body: [undefined, undefined, { hits: 1 }, { type: 'tough', hits: 100 }] }),
      (h) => h.update(['o', 'id1'], { body: [], list: ['z'] }),
      (h) => h.update(['o', 'id1'], { body: [null] }),
    ],
  },
  // Cycles in update data: the live merge builds a cyclic graph, removeHidden overflows.
  {
    objects: () => ({ id0: creep('id0') }),
    steps: [
      (h) => {
        const c = { k: 1, _x: 2 };
        c.self = c;
        return h.update(['o', 'id0'], { fresh: c });
      },
      (h) => {
        const l = ['q'];
        l.push(l);
        return h.update(['o', 'id0'], { list: l });
      },
      (h) => {
        const s = { energy: 4, _x: 1 };
        return h.update(['o', 'id0'], { store: s, other: s, more: [s, s] }); // shared, acyclic
      },
    ],
  },
  // A recorded payload aliases the live store; a later, crashed update makes that store
  // cyclic, so execute's JSON.stringify throws.
  {
    objects: () => ({ id0: creep('id0') }),
    steps: [
      (h) => h.update(['o', 'id0'], { store: { energy: 1 } }),
      (h) => {
        const c = { k: 1 };
        c.self = c;
        return h.update(['o', 'id0'], { store: { loop: c } });
      },
    ],
  },
  // `$set` order: integer-like ids first, ascending, then first-touch order.
  {
    objects: () => ({ 10: creep('10'), 9: creep('9'), 2: creep('2'), abc: creep('abc'), id5: creep('id5') }),
    steps: [
      (h) => h.update(['o', 'id5'], { x: 1 }),
      (h) => h.update(['s', '10'], { x: 2 }),
      (h) => h.update(['o', 'abc'], { x: 3 }),
      (h) => h.update(['o', '9'], { x: 4 }),
      (h) => h.update(['s', '2'], { x: 5 }),
      (h) => h.update(['s', '0010'], { x: 6 }),
      (h) => h.remove(['o', 'abc'], false),
    ],
  },
  // `_id` in update data re-keys the live object; truthy scalars swallow objects.
  {
    objects: () => ({ id0: creep('id0', { flag: 5, s: 'str', t: true }), id1: creep('id1') }),
    steps: [
      (h) => h.update(['o', 'id0'], { flag: { a: 1 }, s: [1], t: { b: 2 } }),
      (h) => h.update(['o', 'id0'], { _id: 'renamed', x: 1 }),
      (h) => h.inc(['o', 'id0'], 'hits', 1),
      (h) => h.update(['o', 'id1'], { _id: null, x: 2 }),
      (h) => h.listOp('addToSet', ['o', 'id1'], 'list', 'c'),
    ],
  },
];

const lines = [];
const dropped = { model: 0, aborted: 0 };
for (const s of SCRIPTS) lines.push(scenario(s));
while (lines.length < 400 + SCRIPTS.length) {
  const s = scenario();
  if (s) lines.push(s);
}
writeFixture('bulk/cases.ndjson', lines);
console.log(`dropped: ${dropped.model} with array named properties/holes, ${dropped.aborted} whose dbBulk aborted`);
