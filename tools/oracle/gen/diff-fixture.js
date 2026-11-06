// Generates the WP01 world-service fixtures under crates/engine/tests/fixtures/world/ by running the official
// server code ($SCREEPS_ROOT, default E:/screeps/server):
//   common.ndjson   @screeps/common index.js (= reference/common/index.js): getDiff, roomNameToXY,
//                   calcWorldSize, encodeTerrain, decodeTerrain, checkTerrain
//   mapgrid.ndjson  driver/lib/runtime/mapgrid.js WorldMapGrid._buildGridData exit counts
//   access.ndjson   driver/lib/index.js updateAccessibleRoomsList + updateRoomStatusData (TICK-ACCESSIBLE)
//   blob.ndjson     JSON.stringify({room, seq, objects}) with non-ASCII as \uXXXX (RoomSnapshot::blob)
//   terrain-blob.ndjson  backend/lib/cli/map.js updateTerrainData (TERRAIN-WORLD-BLOB: walled rooms + ring)
//   data.ndjson     driver/lib/runtime/data.js get(userId) (ORDER-RUNTIME-ROOMIDS/OBJECTS, VIS-ROOMS, …)
// Storage is the official @screeps/storage db.js over an in-memory lokijs, with the shipped DBs' index modes
// (spec 11 STORE-INDEX-MODE) and every request and reply JSON round-tripped like the storage RPC.
// Usage: E:\screeps\node22\node-v22.22.3-win-x64\node.exe tools/oracle/gen/diff-fixture.js
'use strict';
const path = require('path');
const { EventEmitter } = require('events');
const { prng, writeFixture } = require('../lib/common');

const SERVER = process.env.SCREEPS_ROOT || 'E:/screeps/server';
const mod = (p) => path.join(SERVER, 'node_modules', p);
const common = require(mod('@screeps/common'));
const config = common.configManager.config;
config.storage = new EventEmitter(); // what @screeps/storage/lib/index.js sets up before loading db.js
const storage = require(mod('@screeps/storage/lib/db.js'));
const loki = require(mod('lokijs'));
const driver = require(mod('@screeps/driver/lib/index.js'));
const RuntimeBuffer = require(mod('buffer')).Buffer; // the buffer@4.9 webpack bundles into the runtime (same code)
const WorldMapGrid = require(mod('@screeps/driver/lib/runtime/mapgrid.js'));

const g = prng(0x01d1ff);
const rt = (x) => (x === undefined ? undefined : JSON.parse(JSON.stringify(x)));
const enc = (n) => (Number.isFinite(n) ? n : String(n)); // Infinity/NaN survive the NDJSON
const encValue = (v) => (typeof v === 'number' && !Number.isFinite(v) ? { number: String(v) } : v); // any JSON value, or a non-finite number
const hex = (n = 15) => Array.from({ length: n }, () => '0123456789abcdef'[g.int(16)]).join('');
const shuffle = (a) => { for (let i = a.length - 1; i > 0; i--) { const j = g.int(i + 1); [a[i], a[j]] = [a[j], a[i]]; } return a; };

// ---- common.ndjson ----

const SCALARS = [0, 1, -1, 2.5, 1e21, 1e-7, 0.1, 123456789, '', 'a', 'W1N1', 'é€', true, false, null];
const SUBKEYS = ['a', 'b', 'c', '0', '1', '10', 'x', 'energy', 'length', 'user'];
const DOCKEYS = ['type', 'x', 'y', 'store', 'body', 'hits', 'user', '0', '2', 'name', 'memory', 'spawning', 'length', 'effects', 'flag'];
function genVal(d = 0) {
  const r = g.rand();
  if (d > 2 || r < 0.5) return g.pick(SCALARS);
  if (r < 0.72) return Array.from({ length: g.int(4) }, () => genVal(d + 1));
  const o = {};
  for (let i = g.int(4); i > 0; i--) o[g.pick(SUBKEYS)] = genVal(d + 1);
  return o;
}
function genDoc(id) {
  const o = id === undefined ? {} : { _id: id };
  for (let i = g.int(6); i > 0; i--) o[g.pick(DOCKEYS)] = genVal();
  return o;
}
// A nearby version of a nested value: some keys/indices changed, dropped or added.
function nudge(v) {
  if (Array.isArray(v)) {
    const a = v.map((x) => (g.chance(0.3) ? nudge(x) : x));
    if (g.chance(0.3)) a.pop();
    if (g.chance(0.3)) a.push(genVal(2));
    return a;
  }
  if (v && typeof v === 'object') {
    const o = {};
    for (const k of Object.keys(v)) if (!g.chance(0.25)) o[k] = g.chance(0.3) ? nudge(v[k]) : v[k];
    if (g.chance(0.4)) o[g.pick(SUBKEYS)] = genVal(2);
    return o;
  }
  return g.chance(0.5) ? v : g.pick(SCALARS);
}
function mutate(doc) {
  const o = {};
  for (const k of Object.keys(doc)) {
    const r = g.rand();
    if (k === '_id' || r < 0.45) o[k] = doc[k];
    else if (r < 0.6) continue; // deleted
    else if (r < 0.85) o[k] = nudge(doc[k]);
    else o[k] = g.pick([genVal(), null, 0, '', false, [], {}]);
  }
  if (g.chance(0.3)) o[g.pick(DOCKEYS)] = genVal();
  return o;
}
function genId(used) {
  for (;;) {
    const r = g.rand();
    const id = r < 0.12 ? String(g.int(40)) : r < 0.15 ? g.int(30) : r < 0.17 ? undefined : hex(6);
    if (id === undefined || !used.has(String(id)) || g.chance(0.1)) { // ~10% repeats an id on purpose
      used.add(String(id));
      return id;
    }
  }
}
function diffCase() {
  const used = new Set();
  const old = Array.from({ length: g.int(8) }, () => genDoc(genId(used)));
  const fresh = [];
  for (const d of old) {
    const r = g.rand();
    if (r < 0.2) continue;
    fresh.push(r < 0.4 ? rt(d) : mutate(d));
  }
  for (let i = g.int(3); i > 0; i--) fresh.push(genDoc(genId(used)));
  if (g.chance(0.3)) shuffle(fresh);
  const [o, n] = [rt(old), rt(fresh)];
  return { kind: 'diff', tag: 'common/index.js:69', old: o, new: n, out: JSON.stringify(common.getDiff(rt(o), rt(n))) };
}
function diffShapes() {
  // Hand-picked FMT-ROOM-DIFF shapes: dropped nested deletion, array subdiff, type flips, falsy transitions.
  const pairs = [
    [{ _id: 'a', store: { energy: 50, power: 1 } }, { _id: 'a', store: { power: 1 } }],
    [{ _id: 'a', body: [{ type: 'move', hits: 100 }, { type: 'work', hits: 100 }] }, { _id: 'a', body: [{ type: 'move', hits: 100 }, { type: 'work', hits: 50 }] }],
    [{ _id: 'a', body: [1, 2, 3] }, { _id: 'a', body: [1, 2] }],
    [{ _id: 'a', x: { length: 2 } }, { _id: 'a', x: [5, 6] }],
    [{ _id: 'a', x: [5, 6] }, { _id: 'a', x: { length: 2, 0: 5, 1: 6 } }],
    [{ _id: 'a', x: 1, y: 0, s: '', b: false, n: null }, { _id: 'a', x: 0, y: 1, s: 'q', b: true, n: 0 }],
    [{ _id: 'a', o: { a: 1 } }, { _id: 'a', o: null }],
    [{ _id: 'a', o: null }, { _id: 'a', o: { a: 1 } }],
    [{ _id: 'a', o: {} }, { _id: 'a', o: {} }],
    [{ _id: 'a', o: { a: { b: 1 } } }, { _id: 'a', o: { a: { b: 2 } } }],
    [{ _id: 'a', 10: 1, 2: 2, z: 3 }, { _id: 'a', 2: 3, 1: 4, z: 3, y: 5 }],
    // lodash 3 `_.size` takes a numeric `length` as the size: such a diff counts as empty and is dropped
    [{ _id: 'a', length: 1, x: 1 }, { _id: 'a', length: 0, x: 2 }],
    [{ _id: 'a', length: 1, x: 1 }, { _id: 'a', length: 2, x: 2 }],
    [{ _id: 'a', o: { length: 3, a: 1 } }, { _id: 'a', o: { length: 0, a: 2 } }],
    [{ _id: 'a', o: { length: 3, a: 1 } }, { _id: 'a', o: { length: 1.5, a: 2 } }],
  ];
  return pairs.map(([a, b]) => ({ kind: 'diff', tag: 'common/index.js:90', old: [a], new: [b], out: JSON.stringify(common.getDiff([a], [b])) }));
}

const NAMES = ['W1N1', 'E0S0', 'W0N0', 'E10S5', 'w5n5', 'w105', 'W1005', 'E1_1', '_1A1', 'W01N01', 'W1N1x', 'W1N', 'WN1', '', 'W', 'sim',
  'W99999999999999999999N1', 'ıw1n1', 'ſ1n1', 'ß1N1', 'W1N1 ', ' W1N1', 'E5Z7', 'Q7R8', '1234', '12345', 'W127N127', 'E127S127', 'Ẃ1N1'];
function worldSizeCase() {
  const names = Array.from({ length: g.int(6) }, () => (g.chance(0.7) ? g.pick(['W', 'E']) + g.int(12) + g.pick(['N', 'S']) + g.int(12) : g.pick(NAMES)));
  const out = common.calcWorldSize(names.map((n) => ({ _id: n })));
  return { kind: 'worldSize', tag: 'common/index.js:183', names, out: enc(out) };
}
function encodeCase() {
  const terrain = Array.from({ length: g.int(120) }, () => {
    const r = g.rand();
    if (r < 0.05) return g.pick([5, null, 'x', [1]]);
    return {
      x: g.chance(0.9) ? g.int(50) : g.pick([2.5, '3', 50, -1]),
      y: g.chance(0.9) ? g.int(50) : g.pick([1.5, '7', 51, -2]),
      type: g.pick(['wall', 'wall', 'swamp', 'plain', 'lava', undefined]),
      ...(g.chance(0.1) ? { room: 'W1N1' } : {}),
    };
  });
  return { kind: 'encode', tag: 'common/index.js:25', terrain: rt(terrain), out: common.encodeTerrain(rt(terrain)) };
}
function terrainString(len = 2500, alphabet = '0001123') {
  return Array.from({ length: len }, () => (g.chance(0.98) ? alphabet[g.int(alphabet.length)] : g.pick(['4', '5', '9', 'a', ' ', 'é', 'Z']))).join('');
}
function decodeCase() {
  const s = g.chance(0.7) ? terrainString() : terrainString(g.int(2600));
  return { kind: 'decode', tag: 'common/index.js:43', str: s, out: common.decodeTerrain(s, 'W1N1') };
}
function checkCase() {
  const s = terrainString(g.chance(0.8) ? 2500 : g.int(2600), '0123');
  const queries = Array.from({ length: 40 }, () => {
    const x = g.chance(0.85) ? g.int(50) : g.pick([-1, 50, 51, 2.7, -0.5, 1e9]);
    const y = g.chance(0.85) ? g.int(50) : g.pick([-1, 50, 49.5, -3]);
    const mask = g.pick([1, 2, 3, 0, -1, 4]);
    return [x, y, mask, common.checkTerrain(s, x, y, mask)];
  });
  return { kind: 'check', tag: 'common/index.js:61', str: s, queries };
}
function commonFixture() {
  const lines = [...diffShapes()];
  for (let i = 0; i < 400; i++) lines.push(diffCase());
  for (const name of NAMES) {
    const xy = common.roomNameToXY(name);
    lines.push({ kind: 'xy', tag: 'common/index.js:142', name, out: xy[0] === undefined ? null : xy });
  }
  for (let i = 0; i < 60; i++) lines.push(worldSizeCase());
  const huge = '9'.repeat(400); // parses to Infinity: Infinity - Infinity = NaN, and Math.max(NaN, …) is NaN
  for (const names of [[], ['W5N5', 'W1N1'], ['E1S1', 'E4S9'], ['W5N5', 'E4S4'], ['bad'], ['E' + huge + 'S1'], ['E1S1', 'W' + huge + 'N1']]) {
    lines.push({ kind: 'worldSize', tag: 'common/index.js:183', names, out: enc(common.calcWorldSize(names.map((n) => ({ _id: n })))) });
  }
  for (let i = 0; i < 12; i++) lines.push(encodeCase());
  for (let i = 0; i < 5; i++) lines.push(decodeCase());
  for (let i = 0; i < 8; i++) lines.push(checkCase());
  writeFixture('world/common.ndjson', lines);
}

// ---- mapgrid.ndjson ----

function mapgridFixture() {
  const lines = [];
  for (let i = 0; i < 40; i++) {
    const p = g.rand();
    const codes = Array.from({ length: 2500 }, () => (g.rand() < p ? g.pick([1, 1, 2, 3]) : 0));
    if (i === 0) codes.fill(1);
    if (i === 1) codes.fill(0);
    if (i === 2) codes.fill(2);
    const terrain = codes.join('');
    const grid = WorldMapGrid.prototype._buildGridData.call({}, ['W1N1'], { W1N1: Uint8Array.from(codes) });
    lines.push({ tag: 'runtime/mapgrid.js:68', terrain, out: grid['-2,-2'] });
  }
  writeFixture('world/mapgrid.ndjson', lines);
}

// ---- blob.ndjson ----

const ascii = (s) => s.replace(/[\u0080-\uffff]/g, (c) => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'));
const BLOB_SCALARS = [...SCALARS, 5e-324, 1.7976931348623157e308, 2 ** 53, -1.5, 123456789012345680000, 'ü\u0001"\\\n', '😀', '\u2028', 'x\u007f'];
function blobFixture() {
  const lines = [];
  for (let i = 0; i < 30; i++) {
    const n = i === 0 ? 0 : g.int(8);
    let seq = g.int(1000);
    const seqs = [];
    const objects = [];
    for (let k = 0; k < n; k++) {
      seq += 1 + g.int(1e6);
      seqs.push(seq);
      const d = { _id: hex(), type: g.pick(['creep', 'spawn', 'source']) };
      for (let j = g.int(5); j > 0; j--) d[g.pick(DOCKEYS)] = g.chance(0.5) ? g.pick(BLOB_SCALARS) : genVal(1);
      objects.push(rt(d));
    }
    const room = rt({ _id: 'W1N1', status: 'normal', ...(g.chance(0.5) ? { novice: 1, name: 'ñ' } : {}) });
    lines.push({ room, seq: seqs, objects, out: ascii(JSON.stringify({ room, seq: seqs, objects })) });
  }
  writeFixture('world/blob.ndjson', lines);
}

// ---- storage harness (access + data) ----

// Index modes of a server started from a shipped DB (spec 11 STORE-INDEX-MODE): lazy everywhere except the
// collections the runtime creates (users.console, users.power_creeps, users.money, users.resources).
const ADAPTIVE = new Set(['users.console', 'users.power_creeps', 'users.money', 'users.resources']);
let lk;
config.storage.getDb = () => {
  lk = new loki('chitin-oracle', { adapter: new loki.LokiMemoryAdapter(), autosave: false });
  lk.addCollection('env');
  return lk;
};
function call(fn, ...args) {
  return new Promise((resolve, reject) => storage[fn](...rt(args), (err, r) => (err ? reject(err) : resolve(rt(r)))));
}
for (const c of config.common.dbCollections) {
  const db = {};
  for (const m of ['find', 'findOne', 'by', 'clear', 'count', 'ensureIndex', 'removeWhere', 'insert']) db[m] = (...a) => call('dbRequest', c, m, a);
  db.update = (q, u, p) => call('dbUpdate', c, q, u, p || {});
  db.bulk = (b) => call('dbBulk', c, b);
  db.findEx = (q, o) => call('dbFindEx', c, q, o);
  common.storage.db[c] = db;
}
Object.assign(common.storage.env, {
  get: (k) => call('dbEnvGet', k),
  mget: (k) => call('dbEnvMget', k),
  set: (k, v) => call('dbEnvSet', k, v),
  hget: (k, f) => call('dbEnvHget', k, f),
  hmget: (k, f) => call('dbEnvHmget', k, f),
  hset: (k, f, v) => call('dbEnvHset', k, f, v),
  hmset: (k, d) => call('dbEnvHmset', k, d),
});
async function freshDb(collections, env) {
  await config.storage.loadDb();
  for (const c of config.common.dbCollections) {
    lk.addCollection(c, { adaptiveBinaryIndices: ADAPTIVE.has(c) });
    await call('dbRequest', c, 'count', [{}]); // storage's getOrAddCollection: the indices exist before any insert
  }
  for (const [c, docs] of Object.entries(collections)) for (const d of docs) await call('dbRequest', c, 'insert', [d]);
  lk.getCollection('env').insert({ data: rt(env) });
}

// ---- access.ndjson ----

async function accessFixture() {
  const lines = [];
  const now = 1_700_000_000_000;
  const realNow = Date.now;
  Date.now = () => now;
  const VALUES = [undefined, undefined, undefined, null, now - 1000, now + 1000, now, 0, '', true, false, 'abc', ' ', String(now + 5), String(now - 5), [now + 5], [], {}];
  try {
    for (let i = 0; i < 40; i++) {
      const rooms = [];
      for (let k = g.int(10); k >= 0; k--) {
        const r = { _id: g.pick(['W', 'E']) + g.int(9) + g.pick(['N', 'S']) + g.int(9) };
        if (rooms.some((x) => x._id === r._id)) continue;
        const status = g.pick(['normal', 'normal', 'normal', 'out of borders', 'closed', undefined]);
        if (status !== undefined) r.status = status;
        for (const f of ['openTime', 'novice', 'respawnArea']) {
          const v = g.pick(VALUES);
          if (v !== undefined) r[f] = v;
        }
        rooms.push(rt(r));
      }
      await freshDb({ rooms }, {});
      await driver.updateAccessibleRoomsList();
      await driver.updateRoomStatusData();
      const accessible = JSON.parse(await common.storage.env.get('accessibleRooms'));
      const status = JSON.parse(await common.storage.env.get('roomStatusData'));
      lines.push({ tag: 'lib/index.js:490', now, rooms, accessible, status });
    }
  } finally {
    Date.now = realNow;
  }
  writeFixture('world/access.ndjson', lines);
}

// ---- terrain-blob.ndjson ----

async function terrainBlobFixture() {
  const cliMap = require(mod('@screeps/backend/lib/cli/map.js'));
  const zlib = require('zlib');
  const lines = [];
  for (let i = 0; i < 25; i++) {
    const rooms = [];
    const terrain = [];
    for (let k = 1 + g.int(12); k > 0; k--) {
      const name = g.pick(['W', 'E']) + g.int(i < 5 ? 3 : 12) + g.pick(['N', 'S']) + g.int(i < 5 ? 3 : 12);
      if (rooms.some((r) => r._id === name)) continue;
      rooms.push({ _id: name, status: g.chance(0.2) ? 'out of borders' : 'normal' });
      terrain.push({ room: name, terrain: Array.from({ length: 2500 }, () => g.int(4)).join('') });
    }
    await freshDb({ rooms, 'rooms.terrain': terrain }, {});
    await cliMap.updateTerrainData();
    const blob = JSON.parse(zlib.inflateSync(Buffer.from(await common.storage.env.get('terrainData'), 'base64')));
    const entries = blob.map((e) => [e.room, /^1{2500}$/.test(e.terrain) ? 'walls' : e.terrain === terrain.find((t) => t.room === e.room)?.terrain ? 'same' : 'other']);
    lines.push({ tag: 'cli/map.js:661', rooms, terrain, entries });
  }
  writeFixture('world/terrain-blob.ndjson', lines);
}

// ---- data.ndjson ----

const ROOMS = ['W1N1', 'W2N1', 'W1N2', 'W2N2', 'E0S0', 'W3N3', 'E1S1'];
const MARKET_KEYS = ['energy', 'H', 'XGH2O', 'token', 'all', 'nope'];

// A scenario: the collections as inserted (creation order) and the env, for the user `me`.
function randomScenario() {
  const me = hex();
  const others = Array.from({ length: 1 + g.int(3) }, () => hex());
  const npcs = g.chance(0.5) ? ['2', '3'] : [];
  const rooms = shuffle(ROOMS.slice()).slice(0, 2 + g.int(5)).map((n) => ({ _id: n, status: 'normal' }));
  const roomNames = rooms.map((r) => r._id);
  const users = shuffle([me, ...others, ...npcs]).map((id) => ({ _id: id, username: 'U' + id, usernameLower: 'u' + id, badge: { type: g.int(5) }, cpu: 100, cpuAvailable: g.int(5000), active: 1 }));
  const meDoc = users.find((u) => u._id === me);
  Object.assign(meDoc, g.pick([{ cpu: 100, cpuAvailable: 450 }, { cpu: 0 }, { cpu: 20, cpuAvailable: -50 }, { cpuAvailable: 10 }, { cpu: 300, cpuAvailable: 10000 }]));
  const ids = new Set();
  const objId = () => {
    for (;;) {
      const id = g.chance(0.06) ? String(1 + g.int(60)) : hex();
      if (!ids.has(id)) return ids.add(id), id;
    }
  };
  const objects = [];
  for (let k = 8 + g.int(30); k > 0; k--) {
    const r = g.rand();
    const owner = r < 0.4 ? me : r < 0.7 ? g.pick(others) : r < 0.8 && npcs.length ? g.pick(npcs) : undefined;
    const room = g.pick(roomNames);
    const o = { _id: objId(), room, x: g.int(50), y: g.int(50) };
    if (owner) {
      o.user = owner;
      o.type = g.pick(owner === me
        ? ['creep', 'creep', 'spawn', 'extension', 'constructionSite', 'constructionSite', 'flag', 'observer', 'observer', 'controller', 'tombstone', 'powerCreep']
        : ['creep', 'spawn', 'controller', 'tower', 'constructionSite']);
    } else {
      o.type = g.pick(['source', 'mineral', 'controller', 'road', 'energy']);
    }
    // CONTROLLER_STRUCTURES allows one observer per room (and an owner keeps the old one's count), so a user never
    // has two in a room.
    if (o.type === 'observer' && objects.some((x) => x.type === 'observer' && x.user === owner && x.room === room)) o.type = 'extension';
    if (o.type === 'observer' && !g.chance(0.15)) o.observeRoom = g.pick([...roomNames, 'W9N9', null, '7', 'W1N1x']);
    if (o.type === 'controller') {
      if (!owner && g.chance(0.5)) o.reservation = { user: g.pick([...others, ...npcs, me, 'gone']), endTime: 5 };
      if (g.chance(0.4)) o.sign = { user: g.pick([...others, me, 'gone']), text: 's' };
    }
    if (o.type === 'creep' && g.chance(0.2)) o.interRoom = { room: g.pick(roomNames), x: 1, y: 1 };
    objects.push(o);
  }
  const flags = Array.from({ length: g.int(5) }, () => ({ _id: hex(), user: g.pick([me, ...others]), room: g.pick(roomNames), data: 'F~1~1~25~25' }));
  const parties = [me, me, ...others, ...npcs, 'gone', undefined];
  const transactions = Array.from({ length: g.chance(0.06) ? 380 + g.int(150) : g.int(12) }, () => {
    const t = { _id: hex(), time: g.int(6), resourceType: 'energy', amount: 1 };
    const [s, rc] = [g.pick(parties), g.pick(parties)];
    if (s !== undefined) t.sender = s;
    if (rc !== undefined) t.recipient = rc;
    return t;
  });
  const consoleDocs = Array.from({ length: g.int(4) }, (_x, i) => ({ _id: hex(), user: g.pick([me, ...others]), expression: 'e' + i, hidden: g.chance(0.3) }));
  const powerCreeps = Array.from({ length: g.int(4) }, (_x, i) => ({ _id: hex(), user: g.pick([me, ...others]), name: 'pc' + i, className: 'operator' }));
  const orders = Array.from({ length: g.int(7) }, () => ({
    _id: g.chance(0.1) ? String(1 + g.int(50)) : hex(),
    user: g.pick([me, ...others, undefined]),
    type: g.pick(['sell', 'buy']),
    resourceType: g.pick(MARKET_KEYS),
    price: 1000 + g.int(5000),
    active: g.pick([true, true, true, false, 1]),
    ...(g.chance(0.2) ? { id: 'stale' } : {}),
  }));
  for (let i = orders.length - 1; i > 0; i--) if (orders.slice(0, i).some((o) => o._id === orders[i]._id)) orders.splice(i, 1); // unique _id index
  const stats = Array.from({ length: g.int(5) }, () => ({ _id: hex(), resourceType: g.pick(MARKET_KEYS), date: '2026-10-' + (10 + g.int(9)), transactions: g.int(9), volume: g.int(999), avgPrice: g.int(9) + 0.5, stddevPrice: 0.25 }));
  const branches = [];
  for (let k = g.int(4); k > 0; k--) {
    const modules = { main: 'module.exports.loop = function() {}' };
    for (let j = g.int(4); j > 0; j--) {
      const name = g.pick(['lib$DOT$util', 'lib.util', 'dir$SLASH$x', 'a$BACKSLASH$b', '1', 'plain', 'wasm$DOT$bin']);
      modules[name] = name.startsWith('wasm') || g.chance(0.15) ? { binary: Buffer.from(hex(8)).toString('base64') } : g.pick(['// ' + name, 5, null, ['x']]);
    }
    branches.push({ _id: hex(), user: g.pick([me, me, ...others]), branch: 'b' + k, modules, timestamp: g.pick([0, 1234567, undefined]), activeWorld: false });
  }
  const mine = branches.filter((b) => b.user === me);
  if (mine.length && g.chance(0.85)) g.pick(mine).activeWorld = true;
  const env = { gameTime: 100 + g.int(1000), roomStatusData: '{}', ['memory:' + me]: g.pick(['', '{"a":1}', 'é', undefined]) };
  if (g.chance(0.6)) {
    meDoc.activeSegments = g.pick([[], [1, 5, 7], [0], [3, 1]]);
    env['memorySegments:' + me] = { 1: 'one', 3: '', 7: 'seven' };
  }
  if (g.chance(0.4)) {
    const other = g.pick(others);
    meDoc.activeForeignSegment = { username: 'U' + other, user_id: g.chance(0.9) ? other : 'gone', id: g.pick([5, 0, 2, undefined]) };
    env['memorySegments:' + other] = { 0: 'zero', 5: 'five' };
    env['publicMemorySegments:' + other] = g.pick(['5,0', '5', '', '2']);
  }
  const logRooms = roomNames.filter(() => g.chance(0.5));
  env['roomEventLog:'] = Object.fromEntries(logRooms.map((n) => [n, '[{"event":1,"room":"' + n + '"}]']));
  return { me, collections: { rooms, users, 'rooms.objects': objects, 'rooms.flags': flags, transactions, 'users.console': consoleDocs, 'users.power_creeps': powerCreeps, 'market.orders': orders, 'market.stats': stats, 'users.code': branches }, env };
}

// Hand-written cases for the ordering rules (spec 11 2.25, spec 02 VIS-ROOMS).
function handScenarios() {
  const me = 'aaaaaaaaaaaaaa1';
  const other = 'bbbbbbbbbbbbbb2';
  const base = (objects, extra = {}) => ({
    me,
    collections: {
      rooms: ['W1N1', 'W2N1', 'W1N2', 'W2N2', 'W3N3', 'W5N5'].map((n) => ({ _id: n, status: 'normal' })),
      users: [{ _id: me, username: 'Me', usernameLower: 'me', badge: {}, cpu: 100, cpuAvailable: 0 }, { _id: other, username: 'Other', usernameLower: 'other', badge: { a: 1 }, cpu: 100 }, { _id: '2', username: 'Invader' }],
      'rooms.objects': objects,
      ...extra,
    },
    env: { gameTime: 50, roomStatusData: '{}' },
  });
  return [
    // observer target lands right after the observer's room; a room first seen later keeps its first position
    base([
      { _id: 'c1', type: 'creep', user: me, room: 'W2N1' },
      { _id: 'f1', type: 'creep', user: other, room: 'W2N2' },
      { _id: 'o1', type: 'observer', user: me, room: 'W1N1', observeRoom: 'W2N2' },
      { _id: 's1', type: 'spawn', user: me, room: 'W2N2' },
      { _id: 'f2', type: 'creep', user: other, room: 'W1N1' },
      { _id: 'n1', type: 'source', room: 'W2N1' },
      { _id: 'i1', type: 'creep', user: '2', room: 'W2N1' },
    ]),
    // "null" / "undefined" observeRoom names; a target that is not a room
    base([
      { _id: 'o1', type: 'observer', user: me, room: 'W1N1', observeRoom: null },
      { _id: 'o2', type: 'observer', user: me, room: 'W2N1' },
      { _id: 'o3', type: 'observer', user: me, room: 'W1N2', observeRoom: 'W9N9' },
      { _id: 'f1', type: 'creep', user: other, room: 'W1N2' },
    ]),
    // a room with only own sites and a legacy flag object: no vision, own docs still delivered
    base([
      { _id: 'cs1', type: 'constructionSite', user: me, room: 'W3N3' },
      { _id: 'f1', type: 'creep', user: other, room: 'W3N3' },
      { _id: 'c1', type: 'creep', user: me, room: 'W1N1' },
      { _id: 'fl1', type: 'flag', user: me, room: 'W3N3' },
      { _id: 'cs2', type: 'constructionSite', user: me, room: 'W1N1' },
    ]),
    // integer-like ids and an integer-like observeRoom enumerate first
    base([
      { _id: 'zz', type: 'creep', user: me, room: 'W2N1' },
      { _id: '42', type: 'creep', user: other, room: 'W2N1' },
      { _id: 'o1', type: 'observer', user: me, room: 'W1N1', observeRoom: '7' },
      { _id: '5', type: 'creep', user: me, room: 'W1N1' },
    ]),
    // a tombstone alone gives vision; a reserved controller alone does not
    base([
      { _id: 't1', type: 'tombstone', user: me, room: 'W5N5' },
      { _id: 'k1', type: 'controller', room: 'W1N1', reservation: { user: me }, sign: { user: other } },
      { _id: 'k2', type: 'controller', room: 'W5N5', reservation: { user: other }, sign: { user: '2' } },
    ]),
    // no objects: data.js deactivates and rejects
    base([{ _id: 'f1', type: 'creep', user: other, room: 'W1N1' }]),
    // observeRoom '__proto__' (reachable through add-object-intent) sets the hash's prototype, not a key
    base([
      { _id: 'o1', type: 'observer', user: me, room: 'W1N1', observeRoom: '__proto__' },
      { _id: 'c1', type: 'creep', user: me, room: 'W2N1' },
    ]),
    // transaction `time` values of every type: Loki's simplesort order, then the 100 limit
    base([{ _id: 'c1', type: 'creep', user: me, room: 'W1N1' }], {
      transactions: [undefined, null, false, true, '', 0, '7', 'abc', [], 5, {}, ' ', '5', 3, 'abd', -1, [2], 'é', 'z']
        .flatMap((time, i) => [{ _id: 'to' + i, sender: me, recipient: other, time }, { _id: 'ti' + i, sender: other, recipient: me, time }]),
    }),
    // binary modules through the runtime's buffer@4.9: '=' inside the data, arrays, JSON-ified Buffers, throws
    base([{ _id: 'c1', type: 'creep', user: me, room: 'W1N1' }], {
      'users.code': [{ _id: 'b1', user: me, branch: 'default', activeWorld: true, timestamp: 5, modules: {
        main: '', a: { binary: 'QUJD=QUJD' }, b: { binary: 'QQ==QQ' }, c: { binary: ' Q!UJ D\n' }, d: { binary: 'QUJDR' },
        e: { binary: [1, 256, -1, '7', 2.9, null, [5], {}] }, f: { binary: { type: 'Buffer', data: [104, 105] } },
        g: { binary: { type: 'Buffer', data: [104], _isBuffer: 1 } }, h: { binary: { length: 'x' } }, i: { binary: '' },
        t1: { binary: 5 }, t2: { binary: null }, t3: { binary: true }, t4: { binary: { a: 1 } },
      } }],
    }),
    // CPU-LIMIT on stored values of other types: `||`, `<`, `+` (concatenation) and `>` as JS evaluates them
    ...[{ cpu: '1', cpuAvailable: 2 }, { cpu: 100, cpuAvailable: '4' }, { cpu: 100, cpuAvailable: [5] }, { cpu: [], cpuAvailable: -3 },
      { cpu: 100, cpuAvailable: '-7' }, { cpu: 50, cpuAvailable: {} }, { cpu: 100, cpuAvailable: true }, { cpu: 1e308, cpuAvailable: 1e308 }]
      .map((fields) => {
        const sc = base([{ _id: 'c1', type: 'creep', user: me, room: 'W1N1' }]);
        Object.assign(sc.collections.users[0], fields);
        return sc;
      }),
  ];
}

async function runData(sc) {
  await freshDb(sc.collections, sc.env);
  const data = require(mod('@screeps/driver/lib/runtime/data.js'));
  let rd;
  try {
    rd = await data.get(sc.me);
  } catch (e) {
    if (e === false) return { rejected: true };
    throw e;
  }
  // game.js:561: in the runtime `driver.bufferFromBase64` is the bundle's buffer@4.9 (ENV-BINARY-BUFFER), not node's Buffer.
  const binary = (b) => {
    try {
      return { binary: [...RuntimeBuffer.from(b, 'base64')] };
    } catch (e) {
      return { throws: String(e) };
    }
  };
  const code = Object.entries(rd.userCode).map(([k, v]) => [k,
    typeof v === 'string' ? v : v && typeof v === 'object' && v.binary !== undefined ? binary(v.binary) : { coerced: String(v) }]);
  return {
    roomIds: Object.keys(rd.roomEventLog),
    eventLogs: Object.values(rd.roomEventLog).map((v) => (v === undefined ? null : v)),
    roomObjects: Object.keys(rd.roomObjects),
    rooms: Object.keys(rd.rooms),
    users: Object.keys(rd.users),
    flags: rd.flags.map((f) => f._id),
    outgoing: rd.transactions.outgoing.map((t) => t._id),
    incoming: rd.transactions.incoming.map((t) => t._id),
    consoleCommands: rd.consoleCommands.map((c) => c._id),
    powerCreeps: Object.keys(rd.userPowerCreeps),
    myOrders: rd.market.myOrders.map((o) => o._id),
    cpu: encValue(rd.cpu),
    cpuBucket: encValue(rd.cpuBucket),
    memory: rd.userMemory.data,
    memorySegments: rd.memorySegments || null,
    foreign: rd.foreignMemorySegment ? { ...rd.foreignMemorySegment, data: rd.foreignMemorySegment.data === undefined ? null : rd.foreignMemorySegment.data } : null,
    code,
    codeTimestamp: rd.userCodeTimestamp,
    time: rd.time,
    // getCachedMarketOrders / getCachedMarketHistory per key, as JSON (undefined → null). The Loki `meta`/`$loki`
    // fields the private server leaks into orders are not chitin data (spec 05 market GOTCHA): stripped here.
    marketOrders: Object.fromEntries(MARKET_KEYS.map((k) => {
      const m = rt(rd.market.orders[k]);
      for (const id in m || {}) delete m[id].meta, delete m[id].$loki;
      return [k, m === undefined ? null : ascii(JSON.stringify(m))];
    })),
    marketHistory: Object.fromEntries(MARKET_KEYS.map((k) => [k, rd.market.history[k] === undefined ? null : ascii(JSON.stringify(rd.market.history[k]))])),
  };
}

async function dataFixture() {
  const lines = [];
  const scenarios = [...handScenarios()];
  for (let i = 0; i < 150; i++) scenarios.push(randomScenario());
  // data.js caches market orders per gameTime and market history for 3 h of Date.now(): every scenario gets its
  // own gameTime and a clock 4 h after the previous one, so each reads its own collections.
  const realNow = Date.now;
  try {
    for (const [i, sc] of scenarios.entries()) {
      sc.env.gameTime = 1000 + i;
      Date.now = () => 1_700_000_000_000 + i * 4 * 3600 * 1000;
      const input = rt(sc);
      lines.push({ tag: 'runtime/data.js:99', ...input, out: await runData(sc) });
    }
  } finally {
    Date.now = realNow;
  }
  writeFixture('world/data.ndjson', lines);
}

(async () => {
  commonFixture();
  mapgridFixture();
  blobFixture();
  await accessFixture();
  await terrainBlobFixture();
  await dataFixture();
  process.exit(0); // storage's env-expiry timer would keep node alive
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
