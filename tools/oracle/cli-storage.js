// CLI storage oracle (CLI-STORAGE): random request sequences against the official reference/storage/lib/db.js
// (LokiJS 1.5.9 collections as a loaded db.json has them: lazy binary indices) and against chitin's
// crates/runtime/js/cli/storage.js over an in-memory host; every result and the collections after each request
// must be equal.
//   node tools/oracle/cli-storage.js check <seed> <n>          compare in node, print mismatches
//   node tools/oracle/cli-storage.js gen <out.ndjson> <seed> <n> fixtures {name, scenario, result: {results, final}}
//   (official side; ids canonicalized by first appearance across the whole run)
'use strict';
const fs = require('fs');
const path = require('path');
const fake = require('./lib/fake-driver');
const { reference, prng } = require('./lib/common');
const loki = require('lokijs');
const q = require('q');

const config = fake.common.configManager.config;
config.storage = config.storage || new (require('events').EventEmitter)();
const storageMethods = require(reference('storage/lib/db.js'));

const COLLS = ['rooms.objects', 'users', 'market.orders'];

// ---- the official side: db.js over Loki collections loaded like a db.json ----
async function official(s) {
  const db = new loki('chitin-cli', { autosave: false });
  db.addCollection('env').insert({ data: {}, databaseVersion: 9 });
  for (const name of COLLS) {
    const c = db.addCollection(name, { adaptiveBinaryIndices: false });
    c.ensureUniqueIndex('_id');
    for (const d of s.initial[name]) c.insert(JSON.parse(JSON.stringify(d)));
  }
  db.loadDatabase = (opts, cb) => cb(null);
  config.storage.getDb = () => db;
  await config.storage.loadDb();
  const call = (method, ...args) => new Promise((resolve) => {
    const err = console.error;
    console.error = () => {};
    try {
      storageMethods[method](...JSON.parse(JSON.stringify(args)), (e, r) => resolve(e !== null && e !== undefined ? { error: String(e) } : { ok: r === undefined ? null : JSON.parse(JSON.stringify(r)) }));
    } finally {
      console.error = err;
    }
  });
  const out = [];
  for (const op of s.ops) {
    let r;
    if (op.m === 'update') r = await call('dbUpdate', op.c, op.query, op.update, op.params);
    else if (op.m === 'bulk') r = await call('dbBulk', op.c, op.bulk);
    else if (op.m === 'findEx') r = await call('dbFindEx', op.c, op.query, op.opts);
    else r = await call('dbRequest', op.c, op.m, op.args);
    const state = {};
    for (const name of COLLS) state[name] = db.getCollection(name).data.map(strip);
    out.push({ r: strip(r), state });
  }
  return out;
}

function strip(v) {
  if (Array.isArray(v)) return v.map(strip);
  if (v && typeof v === 'object') {
    const o = {};
    for (const k of Object.keys(v)) if (k !== '$loki' && k !== 'meta') o[k] = strip(v[k]);
    return o;
  }
  return v;
}

// ---- chitin's side: storage.js over an in-memory host ----
function chitinStorage(store) {
  let gen = 0;
  global.__cli = {
    docs: (name) => JSON.stringify(store[name] || []),
    write(name, writesJson) {
      const docs = store[name] || (store[name] = []);
      const inserted = [];
      for (const w of JSON.parse(writesJson)) {
        if (w.op === 'insert') {
          if (!w.doc._id) w.doc._id = `gen${++gen}`; // db.js genId
          docs.push(w.doc);
          inserted.push(w.doc);
        } else {
          const i = docs.findIndex((d) => d._id === w.id);
          if (i < 0) continue;
          if (w.op === 'remove') docs.splice(i, 1);
          else docs[i] = w.doc;
        }
      }
      return JSON.stringify(inserted);
    },
  };
  const Module = require('module');
  const file = path.join(__dirname, '../../crates/runtime/js/cli/storage.js');
  const m = new Module(file, module);
  m.filename = file;
  m.paths = module.paths;
  m._compile(fs.readFileSync(file, 'utf8'), file);
  return m.exports;
}

async function chitin(s) {
  const store = JSON.parse(JSON.stringify(s.initial));
  const st = chitinStorage(store);
  const out = [];
  for (const op of s.ops) {
    const c = st.db[op.c];
    let p;
    if (op.m === 'update') p = c.update(op.query, op.update, op.params);
    else if (op.m === 'bulk') p = c.bulk(op.bulk);
    else if (op.m === 'findEx') p = c.findEx(op.query, op.opts);
    else p = c[op.m](...op.args);
    const r = await p.then((v) => ({ ok: v === undefined ? null : v }), (e) => ({ error: String(e) }));
    const state = {};
    for (const name of COLLS) state[name] = JSON.parse(JSON.stringify(store[name]));
    out.push({ r, state });
  }
  return out;
}

// ---- random scenarios ----
function random(seed, i) {
  const g = prng((seed * 2246822519 + i) >>> 0);
  const pick = (a) => a[g.int(a.length)];
  const val = () => pick(['W1N1', 'W2N1', 'W1N2', null, 5, '5', 0, '', true, false, 'u1', 'u2', 3.5, '10', 10, undefined]);
  let id = 0;
  const doc = (c) => {
    const d = { _id: `${c[0]}${++id}` };
    if (g.chance(0.9)) d.room = pick(['W1N1', 'W2N1', 'W1N2', 'W3N3', null, 5]);
    if (g.chance(0.8)) d.user = pick(['u1', 'u2', '3', null, 5, '5']);
    if (g.chance(0.8)) d.type = pick(['spawn', 'source', 'creep', 'controller', 'road']);
    if (g.chance(0.8)) d.x = pick([g.int(50), String(g.int(50)), null, -1]);
    if (g.chance(0.5)) d.store = { energy: g.int(300), ...(g.chance(0.3) ? { H: g.int(5) } : {}) };
    if (g.chance(0.3)) d.tags = ['a', 'b', 'c'].filter(() => g.chance(0.5));
    if (c === 'users' && g.chance(0.9)) d.username = pick(['Alice', 'bob', 'Carol', 'dave', 'Bob', 5]);
    if (g.chance(0.2)) d.body = [{ type: 'move', hits: g.int(101) }, { type: pick(['work', 'carry']), hits: 100 }];
    return d;
  };
  const initial = {};
  for (const c of COLLS) initial[c] = Array.from({ length: g.int(12) }, () => doc(c));
  const field = () => pick(['room', 'user', 'type', 'x', 'store.energy', 'tags', 'username', '_id', 'body.type', 'missing']);
  const cond = (depth) => {
    const k = g.int(depth > 1 ? 13 : 11);
    switch (k) {
      case 0: return { [field()]: val() };
      case 1: return { [field()]: { $ne: pick([null, val()]) } };
      case 2: return { [field()]: { $eq: pick([null, val()]) } };
      case 3: return { [field()]: { $in: [val(), val(), val()] } };
      case 4: return { [field()]: { $nin: [val(), val()] } };
      case 5: return { [field()]: { [pick(['$gt', '$gte', '$lt', '$lte'])]: val() } };
      case 6: return { [field()]: { $exists: g.chance(0.5) } };
      case 7: return { [field()]: { $regex: pick(['^W1', 'N1$', '^s', 'o', '[0-9]']) } };
      case 8: return { [field()]: val(), [field()]: { $ne: val() } };
      case 9: return {};
      case 10: return { [field()]: { $aeq: val() } };
      case 11: return { $and: [cond(depth - 1), cond(depth - 1)] };
      default: return { $or: [cond(depth - 1), cond(depth - 1)] };
    }
  };
  // An object `$set` on several docs gives them one shared object in Loki's memory, so a later `$merge` into one
  // changes all (STORE-ALIAS, not reproduced): objects only for single-doc updates.
  const update = (single) => {
    const u = {};
    if (g.chance(0.5)) u.$set = { [pick(['room', 'x', 'flag', 'store'])]: pick(single ? [val(), { energy: 1 }, 7] : [val(), 7]) };
    if (g.chance(0.3)) u.$inc = { [pick(['x', 'n', 'store.energy'])]: g.int(5) };
    if (g.chance(0.2)) u.$unset = { [pick(['room', 'user', 'tags'])]: true };
    if (g.chance(0.2)) u.$merge = { store: { H: g.int(3) } };
    if (g.chance(0.2)) u.$addToSet = { tags: pick(['a', 'z']) };
    if (g.chance(0.2)) u.$pull = { tags: pick(['a', 'b']) };
    return u;
  };
  const ops = [];
  for (let k = 3 + g.int(12); k > 0; k--) {
    const c = pick(COLLS);
    const all = initial[c];
    const m = pick(['find', 'find', 'findOne', 'count', 'by', 'update', 'update', 'removeWhere', 'insert', 'bulk', 'findEx', 'count']);
    if (m === 'find' || m === 'findOne' || m === 'removeWhere') ops.push({ c, m, args: [cond(2)] });
    else if (m === 'count') ops.push({ c, m, args: g.chance(0.2) ? [] : [cond(2)] });
    else if (m === 'by') ops.push({ c, m, args: ['_id', g.chance(0.7) && all.length ? pick(all)._id : pick(['x9', 5])] });
    else if (m === 'insert') ops.push({ c, m, args: [g.chance(0.6) ? doc(c) : [doc(c), doc(c), ...(g.chance(0.2) && all.length ? [{ _id: pick(all)._id }] : [])]] });
    else if (m === 'update') {
      // an upsert copies the query into the new doc: an operator object as `_id` is a non-string id chitin cannot store
      const query = g.chance(0.3) && all.length ? { _id: pick(all)._id } : cond(2);
      const upsert = g.chance(0.3) && !/"_id":{/.test(JSON.stringify(query));
      ops.push({ c, m, query, update: update(query._id !== undefined && Object.keys(query).length === 1), params: upsert ? { upsert: true } : undefined });
    }
    else if (m === 'bulk') ops.push({ c, m, bulk: [{ op: 'update', id: all.length ? pick(all)._id : 'x', update: update(true) }, { op: 'insert', data: doc(c) }, { op: 'remove', id: all.length ? pick(all)._id : 'x' }] });
    else ops.push({ c, m, query: cond(1), opts: { sort: { [pick(['x', 'room', 'user'])]: pick([1, -1]) }, ...(g.chance(0.5) ? { limit: 1 + g.int(4) } : {}), ...(g.chance(0.3) ? { offset: g.int(3) } : {}) } });
  }
  return { name: `cli-storage fuzz ${seed}/${i}`, scenario: { kind: 'cli-storage', mirror: 'common/lib/storage.js', initial, ops } };
}

// ids the two sides generate differ (genId is random): name them by first appearance
function canon(v) {
  const names = new Map();
  const walk = (x) => {
    if (Array.isArray(x)) return x.map(walk);
    if (x && typeof x === 'object') {
      const o = {};
      for (const k of Object.keys(x)) {
        o[k] = k === '_id' && typeof x[k] === 'string' && !/^[rum]\d+$/.test(x[k]) ? (names.has(x[k]) || names.set(x[k], `#${names.size + 1}`), names.get(x[k])) : walk(x[k]);
      }
      return o;
    }
    return x;
  };
  return JSON.stringify(walk(v));
}

async function main() {
  const [mode, ...args] = process.argv.slice(2);
  if (mode === 'check') {
    const [seed, n] = [Number(args[0] || 1), Number(args[1] || 100)];
    let bad = 0;
    for (let i = 0; i < n; i++) {
      const c = random(seed, i);
      const a = await official(c.scenario);
      const b = await chitin(c.scenario);
      for (let k = 0; k < a.length; k++) {
        if (canon(a[k]) !== canon(b[k])) {
          bad++;
          console.log(`${c.name} op ${k} ${JSON.stringify(c.scenario.ops[k])}\n  official ${canon(a[k]).slice(0, 600)}\n  chitin   ${canon(b[k]).slice(0, 600)}`);
          break;
        }
      }
    }
    console.log(`${n - bad}/${n} equal`);
  } else if (mode === 'case') {
    const c = random(Number(args[0]), Number(args[1]));
    const a = await official(c.scenario);
    const b = await chitin(c.scenario);
    const diff = (x, y, p) => {
      if (JSON.stringify(x) === JSON.stringify(y)) return null;
      if (x && y && typeof x === 'object' && typeof y === 'object') {
        for (const k of new Set([...Object.keys(x), ...Object.keys(y)])) {
          const d = diff(x[k], y[k], p + '.' + k);
          if (d) return d;
        }
        return p + ': key order ' + JSON.stringify(Object.keys(x)) + ' vs ' + JSON.stringify(Object.keys(y));
      }
      return p + ': ' + JSON.stringify(x) + ' vs ' + JSON.stringify(y);
    };
    for (let k = 0; k < a.length; k++) {
      console.log(k, JSON.stringify(c.scenario.ops[k]));
      const d = diff(JSON.parse(canon(a[k])), JSON.parse(canon(b[k])), '');
      if (d) { console.log('  DIFF', d); break; }
    }
  } else if (mode === 'gen') {
    const [out, seed, n] = [args[0], Number(args[1] || 1), Number(args[2] || 100)];
    const lines = [];
    for (let i = 0; i < n; i++) {
      const c = random(seed, i);
      const out = JSON.parse(canon(await official(c.scenario)));
      lines.push(JSON.stringify({ name: c.name, scenario: c.scenario, result: { results: out.map((x) => x.r), final: out.length ? out[out.length - 1].state : null } }));
    }
    fs.mkdirSync(path.dirname(out), { recursive: true });
    fs.writeFileSync(out, lines.join('\n') + '\n');
  } else {
    throw new Error('usage: cli-storage.js check <seed> <n> | gen <out> <seed> <n>');
  }
}

main().then(() => process.exit(0), (e) => {
  console.error(e);
  process.exit(1);
});
