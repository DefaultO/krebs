// Loki oracle (STORE-MIGRATIONS, §9 import/export): the official reference/storage/lib/db.js `loadDb` (LokiJS
// reading a db.json file) and its `upgradeDb`, unchanged. `new Date()` is the scenario's `nowMs`.
//   node tools/oracle/loki.js gen <out.ndjson> <seed> <n>   random legacy DBs → {name, scenario, result}
//   node tools/oracle/loki.js load <db.json>                 load + upgrade one file, print the result
//   node tools/oracle/loki.js check <db.json>               load a chitin export; find by room/user; exit 1 on failure
// Scenario {kind:"loki", mirror:"storage/lib/db.js", nowMs, db:<db.json object>}; result {crash, version, env,
// collections:{name: [docs without $loki/meta]}} (env and collections only when the upgrade ran).
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const fake = require('./lib/fake-driver');
const { reference, prng } = require('./lib/common');
const q = require('q');
const loki = require('lokijs');

const config = fake.common.configManager.config;
config.storage = config.storage || new (require('events').EventEmitter)();
require(reference('storage/lib/db.js'));

let file = null;
config.storage.getDb = () => new loki(file, { autosave: false });

async function load(dbFile, nowMs) {
  file = dbFile;
  fake.begin({ worldSeed: '0x1', clock: { baseMs: nowMs, baseTick: 0, nominalTickMs: 1000 }, gameTime: 0 }, Math.random);
  const log = console.log;
  console.log = () => {}; // upgradeDb's progress lines
  let crash = null;
  try {
    await config.storage.loadDb();
  } catch (e) {
    crash = String(e).split('\n')[0];
  } finally {
    console.log = log;
  }
  fake.end();
  return crash;
}

function strip(d) {
  const o = Object.assign({}, d);
  delete o.$loki;
  delete o.meta;
  return o;
}

async function run(s) {
  const tmp = path.join(os.tmpdir(), `chitin-loki-${process.pid}.json`);
  fs.writeFileSync(tmp, JSON.stringify(s.db));
  // the db module keeps its Loki in a closure: read it back through a collection it exposes to loadDb's caller
  let captured = null;
  const getDb = config.storage.getDb;
  config.storage.getDb = () => (captured = getDb());
  const crash = await load(tmp, s.nowMs);
  config.storage.getDb = getDb;
  fs.unlinkSync(tmp);
  const env = captured.getCollection('env');
  const envDoc = env && env.get(1);
  const result = { crash, version: envDoc ? envDoc.databaseVersion : null };
  if (envDoc) result.env = JSON.parse(JSON.stringify(envDoc.data));
  result.collections = {};
  for (const c of captured.listCollections()) {
    if (c.name === 'env') continue;
    result.collections[c.name] = JSON.parse(JSON.stringify(captured.getCollection(c.name).data.map(strip)));
  }
  return result;
}

const COLL = (name, data) => ({ name, data: data.map((d, i) => Object.assign({}, d, { meta: { revision: 0, created: 0, version: 0 }, $loki: i + 1 })),
  idIndex: data.map((_, i) => i + 1), binaryIndices: {}, constraints: null, uniqueNames: ['_id'], transforms: {}, objType: name, dirty: true,
  cachedIndex: null, cachedBinaryIndex: null, cachedData: null, adaptiveBinaryIndices: false, transactional: false, cloneObjects: false,
  cloneMethod: 'parse-stringify', asyncListeners: false, disableChangesApi: true, autoupdate: false, ttl: null, maxId: data.length,
  DynamicViews: [], events: {}, changes: [] });

// A legacy DB at a random version: rooms with highway/inner/odd names and `active`, every converter type with
// its pre-v5 fields (some missing or null), power banks with `power`, spawns with `spawning.remainingTime`,
// power creeps, market orders with/without createdTimestamp, user '2' usually present.
function random(seed, i) {
  const g = prng((seed * 40503 + i) >>> 0);
  const maybe = (v) => (g.chance(0.15) ? undefined : g.chance(0.05) ? null : v);
  const rooms = [];
  for (const id of ['W1N1', 'W10N5', 'E105N3', 'W5N10', 'W0N0', 'E9S9', 'W12N12', 'W20N1']) {
    if (g.chance(0.7)) rooms.push({ _id: id, status: 'normal', ...(g.chance(0.5) ? { active: g.chance(0.6) } : {}), ...(g.chance(0.3) ? { bus: true } : {}) });
  }
  const objs = [];
  const room = () => (rooms.length ? g.pick(rooms)._id : 'W1N1');
  const res = ['energy', 'H', 'O', 'power', 'XGH2O', 'ops', 'G'];
  for (let k = 3 + g.int(12); k > 0; k--) {
    const type = g.pick(['spawn', 'extension', 'tower', 'link', 'storage', 'terminal', 'container', 'factory', 'creep', 'powerCreep',
      'tombstone', 'nuker', 'powerSpawn', 'lab', 'powerBank', 'source', 'invaderCore']);
    const o = { _id: `o${k}`, room: room(), type, x: g.int(50), y: g.int(50) };
    if (['spawn', 'extension', 'tower', 'link', 'nuker', 'powerSpawn', 'lab'].includes(type)) {
      o.energy = maybe(g.int(300));
      o.energyCapacity = maybe(300);
    }
    if (['storage', 'terminal', 'container', 'factory', 'creep', 'powerCreep', 'tombstone'].includes(type)) {
      if (g.chance(0.7)) o.energyCapacity = maybe(1000);
      for (const r of res) if (g.chance(0.4)) o[r] = maybe(g.int(500));
    }
    if (type === 'nuker') Object.assign(o, { G: maybe(g.int(5000)), GCapacity: maybe(5000) });
    if (type === 'powerSpawn') Object.assign(o, { power: maybe(g.int(100)), powerCapacity: maybe(100) });
    if (type === 'lab') {
      if (g.chance(0.6)) o.mineralType = g.pick(['H', 'XGH2O', '', null]);
      if (g.chance(0.6)) o.mineralAmount = maybe(g.int(3) * 100);
      if (g.chance(0.8)) o.mineralCapacity = maybe(3000);
    }
    if (type === 'powerBank') o.power = maybe(g.int(5000));
    if ((type === 'spawn' || type === 'invaderCore') && g.chance(0.6)) {
      o.spawning = g.chance(0.8) ? Object.assign({ name: 'c' }, g.chance(0.8) ? { remainingTime: maybe(g.int(30)) } : {}) : null;
    }
    objs.push(o);
  }
  const users = [{ _id: 'u1', username: 'a' }];
  if (g.chance(0.9)) users.push({ _id: '2', username: 'Invader', badge: { type: 1 } });
  const pcs = g.chance(0.5) ? [{ _id: 'pc1', user: 'u1', energyCapacity: maybe(100), ops: maybe(g.int(100)), energy: maybe(5) }] : null;
  const orders = [{ _id: 'm1', type: 'sell' }, { _id: 'm2', type: 'buy', createdTimestamp: g.pick([0, 5, null, 1700000000000]) }];
  const version = g.pick([undefined, 1, 4, 4.1, 5, 6, 7, 8, 9]);
  const envDoc = { data: { gameTime: g.pick([1, 77, '123', '45abc']), ...(g.chance(0.3) ? { activeRooms: ['old'] } : {}) } };
  if (version !== undefined) envDoc.databaseVersion = version;
  const collections = [COLL('env', [envDoc]), COLL('rooms', rooms), COLL('rooms.objects', objs), COLL('users', users), COLL('market.orders', orders)];
  if (pcs) collections.push(COLL('users.power_creeps', pcs));
  const db = { filename: 'db.json', collections, databaseVersion: 1.5, engineVersion: 1.5, autosave: false, options: {} };
  return { name: `loki fuzz ${seed}/${i} v${version}`, scenario: { kind: 'loki', mirror: 'storage/lib/db.js', nowMs: 1700000000000 + i, db } };
}

async function main() {
  const [mode, ...args] = process.argv.slice(2);
  if (mode === 'gen') {
    const [out, seed, n] = [args[0], Number(args[1] || 1), Number(args[2] || 100)];
    const lines = [];
    for (let i = 0; i < n; i++) {
      const c = random(seed, i);
      lines.push(JSON.stringify({ name: c.name, scenario: c.scenario, result: await run(c.scenario) }));
    }
    fs.mkdirSync(path.dirname(out), { recursive: true });
    fs.writeFileSync(out, lines.join('\n') + '\n');
  } else if (mode === 'load') {
    const s = { nowMs: Number(args[1] || 1700000000000), db: JSON.parse(fs.readFileSync(args[0], 'utf8')) };
    process.stdout.write(JSON.stringify(await run(s)) + '\n');
  } else if (mode === 'check') {
    let captured = null;
    const getDb = config.storage.getDb;
    config.storage.getDb = () => (captured = getDb());
    const crash = await load(path.resolve(args[0]), Date.now());
    if (crash) throw new Error(`load: ${crash}`);
    const objs = captured.getCollection('rooms.objects');
    const first = objs.data[0];
    const byRoom = first ? objs.find({ room: first.room }).length : 0;
    const users = captured.getCollection('users').find({ username: { $ne: null } }).length;
    if (first) objs.update(Object.assign(first, { checked: true })); // meta/idIndex usable
    const env = captured.getCollection('env').get(1);
    console.log(JSON.stringify({ gameTime: env.data.gameTime, version: env.databaseVersion, objects: objs.count(), byRoom, users,
      collections: captured.listCollections().map((c) => c.name).length }));
  } else {
    throw new Error('usage: loki.js gen <out> <seed> <n> | load <db.json> [nowMs] | check <db.json>');
  }
}

main().then(() => process.exit(0), (e) => {
  console.error(e);
  process.exit(1);
});
