// Generates cases.ndjson next to this file: the official runner path for one user, end to end, as the oracle
// of crates/engine/src/runner/make.rs. Runs UNCHANGED reference/engine/src/runner.js (saveResult),
// reference/driver/lib/runtime/{make,data}.js, reference/driver/lib/index.js (getAllUsers, saveUserMemory,
// saveUserMemorySegments, saveUserIntents, sendConsoleMessages, sendConsoleError) and
// reference/engine/src/utils.js storeIntents, over @screeps/common whose storage is the REAL
// reference/storage/lib/db.js on lokijs 1.5.9 in memory behind a JSON round trip (the storage RPC). Only the
// isolate is faked: it returns the case's run result. Date is pinned to NOW (CLK-NOTIFY buckets).
//
// Lines: {kind: "getUsers", users, order}: TICK-GET-USERS over the Loki operators, `order` = getAllUsers ids.
//        {kind: "run", config, time, now, users, uid, objects, notifications, run, intents, count,
//         expect: {outcome, error, cpu, cpuBucket, users, env, roomsIntents, usersIntents, notifications, pubsub}}
// Covers: runtime/make.js, runtime/data.js (reference/driver/lib), engine/src/runner.js.
// Usage: node22 crates/engine/tests/fixtures/make/gen.js   (needs E:\screeps\server node_modules: lokijs, q, he)
'use strict';
const path = require('path');
const Module = require('module');
const zlib = require('zlib');
const ROOT = path.join(__dirname, '..', '..', '..', '..', '..');
const { reference, writeFixture } = require(path.join(ROOT, 'tools', 'oracle', 'lib', 'common'));
const SCREEPS = process.env.SCREEPS_ROOT || 'E:\\screeps\\server';
const nm = (p) => require.resolve(path.join(SCREEPS, 'node_modules', p));

const NOW = 1700000123456;
const RealDate = Date;
global.Date = class extends RealDate {
  constructor(...a) {
    if (a.length) super(...a);
    else super(NOW);
  }
  static now() {
    return NOW;
  }
};

// ---- module wiring ----
const captured = { data: null };
const stubs = {
  'isolated-vm': {
    ExternalCopy: class {
      constructor(d) {
        this.d = d;
        captured.data = d;
      }
      copyInto() {
        return this.d;
      }
      release() {}
    },
  },
  native: {},
};
const userVm = {
  run: null,
  create: () => Promise.resolve(),
  clear() {},
  get: () => ({
    start: () =>
      Promise.resolve({
        // ExternalCopy semantics: a structured clone keeps undefined-valued keys (runtime.js:186 `{username, id}`)
        apply: () => (userVm.run.reject ? Promise.reject(userVm.run.reject) : Promise.resolve(structuredClone(userVm.run))),
        release() {},
      }),
    isolate: { compileScriptSync: () => ({ runSync() {} }) }, // the driver's playerSandbox listener (Game.shard)
    context: {},
  }),
};
const pathFinder = { create: () => ({}), init() {} };
const map = {
  q: nm('q'),
  lodash: reference('lodash-3.10.1/lodash.js'),
  lokijs: nm('lokijs'),
  he: nm('he'),
  'generic-pool': nm('generic-pool'),
  '@screeps/common': reference('common/index.js'),
  '@screeps/common/lib/constants': reference('common/lib/constants.js'),
  '@screeps/engine/dist/utils': reference('engine/src/utils.js'),
  '@screeps/core': reference('driver/lib/index.js'),
};
const resolve = Module._resolveFilename;
Module._resolveFilename = function (request, ...rest) {
  if (request in stubs) return `chitin-stub:${request}`;
  if (/native\/build\/Release\/native/.test(request)) return 'chitin-stub:native';
  if (request in map) return map[request];
  return resolve.call(this, request, ...rest);
};
const fake = (key, exports) => (require.cache[key] = { id: key, filename: key, loaded: true, exports, children: [] });
for (const [k, v] of Object.entries(stubs)) fake(`chitin-stub:${k}`, v);
fake(reference('driver/lib/runtime/user-vm.js'), userVm);
fake(reference('driver/lib/path-finder.js'), pathFinder);

const q = require(map.q);
const _ = require(map.lodash);
const common = require(map['@screeps/common']);
common.configManager.config.storage = {};
const loki = require(map.lokijs);
const storageDb = require(reference('storage/lib/db.js'));

// @screeps/common/lib/storage.js wrapCollection/env/pubsub, in process, with the RPC's JSON round trip
const json = (v) => (v === undefined ? undefined : JSON.parse(JSON.stringify(v)));
// q promises, as rpc.js returns: their unhandled rejections (make.js:233-244 on an unknown username) are q's
const call = (fn, ...args) => q.Promise((ok, fail) => storageDb[fn](...json(args), (err, r) => (err ? fail(err) : ok(json(r)))));
const pubsub = [];
const COLLECTIONS = ['users', 'users.code', 'users.console', 'users.notifications', 'users.intents', 'users.power_creeps', 'rooms', 'rooms.objects', 'rooms.flags', 'rooms.intents', 'rooms.terrain', 'transactions', 'market.orders', 'market.stats'];
for (const c of COLLECTIONS) {
  const w = {};
  for (const m of ['find', 'findOne', 'by', 'clear', 'count', 'ensureIndex', 'removeWhere', 'insert']) w[m] = (...a) => call('dbRequest', c, m, a);
  w.update = (query, update, params) => call('dbUpdate', c, query, update, params);
  w.bulk = (b) => call('dbBulk', c, b);
  w.findEx = (query, opts) => call('dbFindEx', c, query, opts);
  common.storage.db[c] = w;
}
const ENV = { get: 'dbEnvGet', mget: 'dbEnvMget', set: 'dbEnvSet', setex: 'dbEnvSetex', expire: 'dbEnvExpire', ttl: 'dbEnvTtl', del: 'dbEnvDel', hmget: 'dbEnvHmget', hmset: 'dbEnvHmset', hget: 'dbEnvHget', hset: 'dbEnvHset', sadd: 'dbEnvSadd', smembers: 'dbEnvSmembers' };
for (const [k, fn] of Object.entries(ENV)) common.storage.env[k] = (...a) => call(fn, ...a);
common.storage.pubsub.publish = (ch, msg) => (pubsub.push([ch, msg]), q.when());
common.storage.pubsub.subscribe = () => {};

const driver = require(map['@screeps/core']);
driver.connect = () => q.when();
let nextUser = null;
let loop = null;
driver.queue = { create: () => q.when({ fetch: () => q.when(nextUser), markDone: () => q.when() }) };
driver.startLoop = (name, fn) => (loop = fn);
const consoleError = console.error;
const consoleLog = console.log;
console.log = () => {};
console.error = () => {};
require(reference('engine/src/runner.js'));

async function settle() {
  for (let i = 0; i < 20; i++) await new Promise((r) => setImmediate(r));
}

async function freshDb() {
  common.configManager.config.storage.getDb = () => {
    // as db.original.json: env doc 1 (values, current schema) and 2 (expirations)
    const d = new loki('chitin.db', { adapter: new loki.LokiMemoryAdapter() });
    const env = d.addCollection('env');
    env.insert({ data: {}, databaseVersion: 9 });
    env.insert({ data: {} });
    return d;
  };
  await common.configManager.config.storage.loadDb();
}

const strip = (docs) => docs.map((d) => _.omit(d, ['meta', '$loki']));

// ---- cases ----
const lines = [];

async function getUsers() {
  await freshDb();
  const actives = [undefined, null, 0, 1, 10000, true, false, '0', '', 'x'];
  const cpus = [undefined, null, 0, 100, -5, 0.5, '100', 'abc', '', ' ', true, false, [], [5], [1, 2], {}, '0x10', '-3'];
  const users = [];
  let n = 0;
  for (const active of actives)
    for (const cpu of cpus) {
      const u = { _id: `u${n}`, username: `p${n}`, lastUsedDirtyTime: (n * 7919) % 13 };
      if (active !== undefined) u.active = active;
      if (cpu !== undefined) u.cpu = cpu;
      users.push(u);
      n++;
    }
  for (const u of users) await common.storage.db.users.insert(u);
  const all = await driver.getAllUsers();
  lines.push({ kind: 'getUsers', users: strip(await common.storage.db.users.find({})), order: all.map((u) => u._id) });
}

const base = { cpuMaxPerTick: 500, cpuBucketSize: 10000 };

let storedIntents = null;
const saveUserIntents = driver.saveUserIntents;
driver.saveUserIntents = (uid, intents) => ((storedIntents = json(intents)), saveUserIntents(uid, intents));

async function run(c) {
  await freshDb();
  _.extend(driver.config, base, c.config || {});
  const time = c.time || 1000;
  const env = common.storage.env;
  await env.set('gameTime', time);
  await env.set('terrainData', zlib.deflateSync(Buffer.from('[]')).toString('base64'));
  await env.set('accessibleRooms', '[]');
  await env.set('roomStatusData', '{}');
  const users = c.users || [];
  for (const u of users) await common.storage.db.users.insert(u);
  // the user's objects: one creep, plus every object its intents name (in `__room`, else W1N1)
  const run = c.run ? structuredClone(c.run) : null;
  // runtime.js:186 builds `{username, id}`: both slots exist, possibly undefined (the fixture line drops them,
  // as chitin's __store JSON does)
  const afs = run && run.activeForeignSegment;
  if (afs) run.activeForeignSegment = { username: afs.username, id: afs.id };
  const list = (run && run.intentsList) || {};
  if (c.objects !== 0) {
    await common.storage.db['rooms.objects'].insert({ type: 'creep', room: 'W1N1', x: 10, y: 10, user: c.uid });
    for (const id of Object.keys(list).filter((k) => !['room', 'global', 'notify'].includes(k))) {
      await common.storage.db['rooms.objects'].insert({ _id: id, type: 'creep', room: list[id].__room || 'W1N1', x: 20, y: 20, user: c.uid });
      delete list[id].__room;
    }
  }
  for (const n of c.notifications || []) await common.storage.db['users.notifications'].insert(n);
  if (c.memory !== undefined) await env.set(`memory:${c.uid}`, c.memory);
  // two pending console commands: data.js:149 consumes them once a run reads its data
  for (const e of ['1+1', 'Game.time']) await common.storage.db['users.console'].insert({ user: c.uid, expression: e, hidden: false });
  // INT-MANUAL: backend/lib/game/api/game.js:520-525 for each [room, user, objectId, name, intent], before the run
  const manual = [];
  for (const [room, user, id, name, intent] of c.manual || []) {
    const sanitized = common.configManager.config.common.system.sanitizeUserIntents({ [name]: intent }, {});
    manual.push([room, user, id, json(sanitized)]);
    await common.storage.db['rooms.intents'].update({ room }, { $merge: { users: { [user]: { objectsManual: { [id]: sanitized } } } } }, { upsert: true });
    await env.sadd('activeRooms', room); // backend utils.activateRoom
  }
  userVm.run = run || {};
  captured.data = null;
  storedIntents = null;
  pubsub.length = 0;
  nextUser = c.uid;
  let rejected = null;
  const make = driver.makeRuntime;
  driver.makeRuntime = (uid) => make(uid).catch((e) => ((rejected = e), Promise.reject(e)));
  const objects = (await common.storage.db['rooms.objects'].find({ user: c.uid })).length;
  await loop();
  await settle();
  driver.makeRuntime = make;
  lines.push({
    kind: 'run',
    name: c.name,
    config: _.pick(driver.config, ['cpuMaxPerTick', 'cpuBucketSize']),
    time,
    now: NOW,
    users: strip(users),
    uid: c.uid,
    objects,
    memory: c.memory === undefined ? null : c.memory,
    notifications: c.notifications || [],
    manual,
    run: run && !run.reject ? run : null,
    intents: storedIntents,
    count: run && !run.reject ? intentsCount(list) : 0,
    expect: {
      // data.js rejects `false`; make.js rethrows it as {error: false}
      outcome: rejected && rejected.error === false ? 'deactivated' : rejected ? 'rejected' : 'run',
      error: rejected ? rejected.error : null,
      cpu: captured.data ? num(captured.data.cpu) : null,
      cpuBucket: captured.data ? num(captured.data.cpuBucket) : null,
      users: strip(await common.storage.db.users.find({})),
      env: storageEnv(),
      roomsIntents: strip(await common.storage.db['rooms.intents'].find({})).map((d) => _.omit(d, '_id')),
      usersIntents: strip(await common.storage.db['users.intents'].find({})).map((d) => _.omit(d, '_id')),
      notifications: strip(await common.storage.db['users.notifications'].find({})).map((n) => _.omit(n, '_id')),
      consoleLeft: (await common.storage.db['users.console'].find({})).length,
      pubsub: pubsub.slice(),
    },
  });
}

// the env hash after the run, minus the setup keys
function storageEnv() {
  let item;
  storageDb.dbRequest('env', 'get', [1], (e, r) => (item = r));
  const data = (item && item.data) || {};
  const out = {};
  for (const k of Object.keys(data)) if (!['gameTime', 'terrainData', 'accessibleRooms', 'roomStatusData'].includes(k)) out[k] = data[k];
  return json(out);
}

const num = (x) => (x === Infinity ? 'Infinity' : x);

// make.js:98-111 intentsCount, for the Rust side's UserIntents.count (the expected billing checks it)
function intentsCount(list) {
  const free = { say: true, pull: true };
  let count = 0;
  for (const e in list) for (const n in list[e]) if (!free[n]) count += Array.isArray(list[e][n]) ? list[e][n].length : 1;
  return count;
}

const user = (extra) => _.extend({ _id: 'u1', username: 'Player', cpu: 100, cpuAvailable: 10000, active: 10000, gcl: 0 }, extra);
const other = (extra) => _.extend({ _id: 'u2', username: 'Other', cpu: 100, cpuAvailable: 10000, active: 10000 }, extra);
const result = (extra) =>
  _.extend(
    {
      type: 'done',
      usedTime: 3,
      usedCleanTime: 2.41,
      usedDirtyTime: 4,
      intentsList: {},
      memory: { data: '{"a":1}', userId: 'u1' },
      console: { log: [], results: [] },
    },
    extra,
  );

function cases() {
  const cs = [];
  // RUN-SKIP: penalty
  for (const p of [1, 3, '2', true, 0, -1])
    for (const avail of [10000, -50]) cs.push({ name: `penalty ${JSON.stringify(p)} avail ${avail}`, uid: 'u1', users: [user({ skipTicksPenalty: p, cpuAvailable: avail })], run: result() });
  // CPU-NEGATIVE-SKIP
  for (const avail of [-1, -50, -199, -200, -201, -1000, '-5', [-3], -0.5])
    for (const cpu of [100, 20, '100']) cs.push({ name: `negative ${JSON.stringify(avail)} cpu ${JSON.stringify(cpu)}`, uid: 'u1', users: [user({ cpuAvailable: avail, cpu })], run: result() });
  // RUN-DEACTIVATE
  cs.push({ name: 'no objects', uid: 'u1', users: [user({})], objects: 0, run: result() });
  cs.push({ name: 'no objects, negative bucket', uid: 'u1', users: [user({ cpuAvailable: -5 })], objects: 0, run: result() });
  // CPU-LIMIT + CPU-BUCKET-UPDATE
  const lists = [{}, { a: { move: { direction: 1 } } }, { a: { move: {}, say: { message: 'x' } }, b: { harvest: {}, pull: {} } }, { room: { createFlag: [{ roomName: 'W1N1' }, { roomName: 'W1N1' }] }, a: { transfer: {} }, notify: [{ message: 'm' }] }, { room: { createConstructionSite: [{ roomName: 'W1N1' }, { roomName: 'W1N1' }, { roomName: 'W1N1' }] }, a: { move: {}, harvest: {}, transfer: {}, build: {}, repair: {} }, b: { say: {}, attack: {} } }];
  let i = 0;
  for (const avail of [10000, 9999, 0, undefined, null, 50, '100', 9990.5, 40000, 9900.5])
    for (const cpu of [100, 20, '100', 600])
      for (const clean of [0, 3.2, 99.99, 250.5, 0.81]) {
        const list = lists[Math.floor(i++ / 5) % lists.length]; // every list meets every clean value
        const u = user({ cpu });
        if (avail === undefined) delete u.cpuAvailable;
        else u.cpuAvailable = avail;
        cs.push({ name: `bill avail ${JSON.stringify(avail)} cpu ${cpu} clean ${clean}`, uid: 'u1', users: [u], run: result({ usedCleanTime: clean, usedTime: Math.ceil(clean) + 1, usedDirtyTime: Math.ceil(clean) + 2, intentsList: list }) });
      }
  cs.push({ name: 'cap a fraction over the bucket', uid: 'u1', users: [user({ cpuAvailable: 9900.7 })], run: result({ usedCleanTime: 0 }) });
  cs.push({ name: 'small cpuMaxPerTick', config: { cpuMaxPerTick: 50, cpuBucketSize: 500 }, uid: 'u1', users: [user({ cpuAvailable: 480 })], run: result({ usedCleanTime: 7 }) });
  // segment settings
  const segs = [
    { activeSegments: [1, 2] },
    { activeSegments: [] },
    { defaultPublicSegment: 5 },
    { defaultPublicSegment: null },
    { publicSegments: '1,2' },
    { publicSegments: '' },
    { activeForeignSegment: null },
    { activeForeignSegment: { username: 'Other', id: 3 } },
    { activeForeignSegment: { username: 'Other' } },
    { activeForeignSegment: { username: 'Other', id: 0 } },
    { activeForeignSegment: { username: 'nobody', id: 4 } },
    { activeForeignSegment: { username: 'other', id: 4 } },
    // setActiveForeignSegment(undefined[, id]): findOne({username: undefined}) is `{}` after the RPC's JSON
    { activeForeignSegment: { username: undefined } },
    { activeForeignSegment: { username: undefined, id: 4 } },
  ];
  for (const s of segs)
    for (const dps of [7, 0, undefined])
      for (const cur of [undefined, { username: 'Other', id: 1, user_id: 'u2' }]) {
        const u = user({});
        if (cur) u.activeForeignSegment = cur;
        const o = other(dps === undefined ? {} : { defaultPublicSegment: dps });
        cs.push({ name: `segments ${JSON.stringify(s)} dps ${dps} cur ${!!cur}`, uid: 'u1', users: [u, o], run: result(s) });
      }
  // ... which finds the first users doc, here one with a default public segment
  for (const s of [{ username: undefined }, { username: undefined, id: 4 }])
    cs.push({ name: `segments undefined username ${JSON.stringify(s)}, Other first`, uid: 'u1', users: [other({ defaultPublicSegment: 7 }), user({})], run: result({ activeForeignSegment: s }) });
  // visuals, cpu stats, errors, Memory limit
  cs.push({ name: 'visuals', uid: 'u1', users: [user({})], run: result({ visual: { W1N1: 'v1\n', '': 'g\n', map: 'm\n' } }) });
  cs.push({ name: 'CPU limit text', uid: 'u1', users: [user({})], run: result({ type: 'error', error: 'Error: CPU limit reached' }) });
  cs.push({ name: 'timeout text', uid: 'u1', users: [user({})], run: result({ type: 'error', error: 'Error: Script execution timed out: CPU time limit reached' }) });
  cs.push({ name: 'partial error', uid: 'u1', users: [user({ notifyPrefs: { errorsInterval: 5 } })], run: result({ type: 'error', error: 'TypeError: x is not a function\n    at main:1:1', intentsList: { a: { move: { direction: 3 } } } }) });
  cs.push({ name: 'error, bad interval', uid: 'u1', users: [user({ notifyPrefs: { errorsInterval: 'x' } })], run: result({ type: 'error', error: 'Error: e' }) });
  cs.push({ name: 'error, existing notification', uid: 'u1', users: [user({})], notifications: [{ _id: 'n1', user: 'u1', message: 'Error: e', type: 'error', date: NOW - 1000, count: 4 }], run: result({ type: 'error', error: 'Error: e' }) });
  cs.push({ name: 'total error', uid: 'u1', users: [user({})], run: { reject: { stack: 'Error: Too many active segments' } } }); // an isolate Error: make.js:293 reads .stack
  cs.push({ name: 'memory over 2 MiB', uid: 'u1', users: [user({})], memory: 'old', run: result({ memory: { data: 'x'.repeat(2 * 1024 * 1024 + 1), userId: 'u1' } }) });
  cs.push({ name: 'memory exactly 2 MiB', uid: 'u1', users: [user({})], run: result({ memory: { data: 'y'.repeat(2 * 1024 * 1024), userId: 'u1' } }) });
  cs.push({ name: 'segments saved', uid: 'u1', users: [user({})], run: result({ memorySegments: { 3: 'three', 10: '' } }) });
  cs.push({ name: 'console', uid: 'u1', users: [user({})], run: result({ console: { log: [{ message: 'hello' }, { message: '<b>' }], results: ['42'] } }) });
  // INT-SAVE / INT-NOTIFY
  const notify = [0, 5, -3, 2000, null, '7', 1.5, 1440, 0.01, 1441].map((g, k) => ({ message: `n${k}`, groupInterval: g }));
  notify.push({ message: 'é'.repeat(600) }, { message: '😀'.repeat(300), groupInterval: 1 });
  for (let k = 0; k < 15; k++) notify.push({ message: `extra${k}`, groupInterval: 0 });
  cs.push({ name: 'notify', uid: 'u1', users: [user({})], run: result({ intentsList: { notify } }) });
  cs.push({ name: 'notify existing', uid: 'u1', users: [user({})], notifications: [{ _id: 'n1', user: 'u1', message: 'hi', date: NOW, type: 'msg', count: 2 }], run: result({ intentsList: { notify: [{ message: 'hi', groupInterval: 0 }] } }) });
  cs.push({
    name: 'rooms and global',
    uid: 'u1',
    users: [user({})],
    run: result({
      intentsList: {
        b: { harvest: { id: 's' }, __room: 'W2N1' },
        room: { createFlag: [{ roomName: 'W1N1', x: 1, y: 2, name: 'F', color: 1, secondaryColor: 1 }, { roomName: 'W2N1', x: 3, y: 4 }] },
        a: { move: { direction: 2 }, say: { message: 'm' } },
        global: { createOrder: [{ type: 'sell', resourceType: 'energy', price: 1.5, totalAmount: 10, roomName: 'W1N1' }] },
      },
    }),
  });
  // INT-MANUAL: API intents merged per object id, then the runner's `objects` merged beside them
  const manual = [
    ['W1N1', 'u1', 'a', 'move', { direction: 4 }],
    ['W1N1', 'u1', 'c', 'say', { message: 'api', isPublic: true }],
    ['W1N1', 'u1', 'a', 'harvest', { id: 'src' }],
    ['W1N1', 'u1', 'c', 'say', { message: 'again' }],
    ['W2N1', 'u2', 'x', 'attack', { id: 'y', x: 1, y: 2 }],
    ['W1N1', 'u2', 'x', 'move', { direction: 1 }],
  ];
  cs.push({ name: 'manual and run', uid: 'u1', users: [user({}), other({})], manual, run: result({ intentsList: { a: { move: { direction: 2 } }, d: { harvest: { id: 's' }, __room: 'W2N1' } } }) });
  cs.push({ name: 'manual, skipped run', uid: 'u1', users: [user({ cpuAvailable: -3 }), other({})], manual, run: result() });
  return cs;
}

(async () => {
  try {
    await getUsers();
    for (const c of cases()) await run(c);
  } catch (e) {
    consoleError(e);
    process.exit(1);
  }
  console.log = consoleLog;
  // a long run of one character (the 2 MiB Memory cases) is written as {"$repeat": [char, length]}
  const compact = (v) =>
    typeof v === 'string' && v.length > 4096 && v.split(v[0]).length === v.length + 1
      ? { $repeat: [v[0], v.length] }
      : Array.isArray(v)
        ? v.map(compact)
        : v && typeof v === 'object'
          ? _.mapValues(v, compact)
          : v;
  writeFixture('make/cases.ndjson', lines.map(compact));
  process.exit(0);
})();
