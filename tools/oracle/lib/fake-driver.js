// A fake @screeps/driver (and @screeps/common) under which the official engine processor, global
// stage and crons run unchanged in this node process (ARCHITECTURE §12.2). Injected through
// Module._resolveFilename. The bulk writers are the REAL reference/driver/lib/bulk.js over a
// capturing store: every op crosses the storage RPC as JSON and is applied with ../lib/loki-update.js
// to the scenario's collections. Pathfinding is the official glue + native.node (node 22 only).
// Math.random is the current scenario's RNG-SEED stream and Date is vclock(T) (CLOCK-VIRTUAL), both
// installed before any official module (lodash captures Math.random at load).
//
//   const fake = require('./fake-driver');   // first: installs the hooks
//   fake.begin({room, worldSeed, clock, gameTime, worldSize, db, terrains}); …run official code…; fake.capture
//
// `db` maps collection names to arrays of documents (collection order); reads return JSON copies like
// the storage RPC. Inserts lacking `_id` get placeholder ids (`opts.genId`) so both sides can compare.
'use strict';
const Module = require('module');
const path = require('path');
const { EventEmitter } = require('events');
const { reference } = require('./common');
const rng = require('./rng');
const vclock = require('./vclock');

const SCREEPS_ROOT = process.env.SCREEPS_ROOT || 'E:/screeps/server';
const NATIVE = path.join(SCREEPS_ROOT, 'node_modules/@screeps/driver/native/build/Release/native.node');

// ---- determinism: one wrapper each, the scenario swaps what is behind it. Installed before lodash
// loads anywhere (it keeps `nativeRandom = Math.random`). ----
let random = rng.stream(0n).random; // ponytail: a fixed stream outside scenarios (module loads draw nothing)
Math.random = () => random();
process.env.TZ = 'UTC'; // CLK-MARKET-HISTORY: calendar days of vclock in UTC (main sets TZ=UTC too)
const RealDate = Date;
let now = null; // null = real clock (only outside a scenario)
class VDate extends RealDate {
  constructor(...a) {
    if (a.length || now === null) super(...a);
    else super(now);
  }
  static now() {
    return now === null ? RealDate.now() : now;
  }
}
global.Date = VDate;
// The official background intervals (processor.js's unhandled-rejection log, storage db.js's TTL sweeper)
// are not part of a tick and would run at random points of a scenario: never scheduled here.
global.setInterval = () => ({ ref() { return this; }, unref() { return this; }, hasRef: () => false });

// ---- module resolution: official sources from reference/, their deps from the installed server, the
// driver/common names to the fakes below (registered in require.cache at the end of this file) ----
const LODASH = reference('lodash-3.10.1/lodash.js');
const STUBS = ['@screeps/driver', '@screeps/core', '@screeps/common'];
const resolve = Module._resolveFilename;
Module._resolveFilename = function (request, parent, ...rest) {
  if (request === 'lodash') return LODASH;
  if (STUBS.includes(request)) return `chitin-oracle:${request}`;
  if (request.startsWith('@screeps/common/')) return require.resolve(path.join(path.dirname(reference('common/index.js')), request.slice(16)));
  try {
    return resolve.call(this, request, parent, ...rest);
  } catch (e) {
    if (/^[./]/.test(request) || path.isAbsolute(request)) throw e;
    return require.resolve(request, { paths: [path.join(SCREEPS_ROOT, 'node_modules')] });
  }
};
process.env.DRIVER_MODULE = '@screeps/driver';
delete process.env.PTR; // engine config.js: ptr mode changes construction costs

// ---- which official files a scenario ran: V8 precise coverage (synchronous in-process session) ----
const inspector = require('inspector');
const { fileURLToPath } = require('url');
const session = new inspector.Session();
session.connect();
session.post('Profiler.enable');
session.post('Profiler.startPreciseCoverage', { callCount: true, detailed: false });
const REF = path.dirname(reference('engine'));
// reference/… paths of the files whose functions ran since the last call (module wrappers and lodash excluded).
function reached() {
  let res = null;
  session.post('Profiler.takePreciseCoverage', (e, r) => (res = r));
  const out = [];
  for (const sc of res.result) {
    if (!sc.url.startsWith('file:')) continue;
    const rel = path.relative(REF, fileURLToPath(sc.url)).split(path.sep).join('/');
    if (rel.startsWith('..') || rel.startsWith('lodash')) continue;
    if (sc.functions.some((f) => f.ranges[0].startOffset > 0 && f.ranges[0].count > 0)) out.push(`reference/${rel}`);
  }
  return out.sort();
}

const { updateDocument, dbBulk } = require('./loki-update');
const json = (v) => (v === undefined ? undefined : JSON.parse(JSON.stringify(v))); // the storage RPC
const q = () => require('q');

// ---- the scenario state ----
let state = null;
const capture = {};

function by(coll) {
  return state.db[coll] || (state.db[coll] = []);
}

// Collections, with inserts given placeholder ids: '#<n>' per collection, '#f<n>' for room objects
// inserted into another room (launchNuke's nuke), which are also listed in `capture.foreign`.
function storeBulk(coll, ops) {
  (capture.ops[coll] = capture.ops[coll] || []).push(...json(ops));
  ops = json(ops);
  const foreign = (d) => coll === 'rooms.objects' && !!state.room && d.room !== state.room;
  for (const op of ops) if (op.op === 'insert' && foreign(op.data)) capture.foreign.push(op.data);
  const counters = capture.idCounters;
  const err = dbBulk(by(coll), ops, {
    genId(d) {
      const key = coll + (foreign(d) ? ':f' : '');
      counters[key] = (counters[key] || 0) + 1;
      return (foreign(d) ? '#f' : '#') + counters[key];
    },
  });
  if (err) capture.storeErrors.push(`${coll}: ${err}`);
  return q().when({});
}

// Wraps the real bulk.js writer to flag F rows of spec 01 §19A it reaches. BUG-INSERT-COUNTER covers
// every counter reuse (chitin never reuses a temp id), not only the overwrite: removing the newest
// pending insert and inserting again also hands out its number twice.
function flagged(writer) {
  let issued = 0;
  const w = Object.create(writer);
  w.insert = function (data, id) {
    const n = writer.insert(data, id);
    if (n <= issued) flag('BUG-INSERT-COUNTER');
    issued = Math.max(issued, n);
    return n;
  };
  w.inc = function (id, key, amount) {
    if (String(key).includes('.')) flag('BUG-DOTKEY-INC');
    return writer.inc(id, key, amount);
  };
  return w;
}
const flag = (row) => capture.fRows.includes(row) || capture.fRows.push(row);

// ---- fake @screeps/common ----
// `storage` mirrors common/lib/storage.js's wrappers. Engine scenarios only use `bulk` (the capturing
// store above); a scenario that sets `state.rpc(method, ...args)` (cron.js: the real storage db.js over an
// in-memory Loki) gets every call routed there, exactly as the RPC client sends them.
const commonConfig = require(reference('common/lib/config-manager.js')).config;
const realCommon = require(reference('common/index.js'));
const rpc = (method, ...args) => {
  if (!state || !state.rpc) throw new Error(`storage ${method} outside a storage-backed scenario`);
  return state.rpc(method, ...args);
};
function wrapCollection(coll) {
  const wrap = {};
  for (const m of ['find', 'findOne', 'by', 'clear', 'count', 'ensureIndex', 'removeWhere', 'insert']) {
    wrap[m] = (...args) => rpc('dbRequest', coll, m, args);
  }
  wrap.update = (query, update, params) => rpc('dbUpdate', coll, query, update, params);
  wrap.bulk = (ops) => (state && state.rpc ? rpc('dbBulk', coll, ops) : storeBulk(coll, ops));
  wrap.findEx = (query, opts) => rpc('dbFindEx', coll, query, opts);
  return wrap;
}
const envMethods = { get: 'dbEnvGet', mget: 'dbEnvMget', set: 'dbEnvSet', setex: 'dbEnvSetex', expire: 'dbEnvExpire', ttl: 'dbEnvTtl',
  del: 'dbEnvDel', hmget: 'dbEnvHmget', hmset: 'dbEnvHmset', hget: 'dbEnvHget', hset: 'dbEnvHset', sadd: 'dbEnvSadd', smembers: 'dbEnvSmembers' };
// The real storage module object itself, so common's own helpers (getGametime) read through it too.
const storage = Object.assign(realCommon.storage, {
  db: new Proxy({}, { get: (_t, coll) => wrapCollection(coll) }),
  env: Object.assign({ keys: realCommon.storage.env.keys }, ...Object.entries(envMethods).map(([k, m]) => ({ [k]: (...a) => rpc(m, ...a) }))),
  pubsub: { keys: realCommon.storage.pubsub.keys, publish: () => q().when(), subscribe() {} },
  queue: {},
  _connect: () => q().when(),
});
const common = Object.assign({}, realCommon, {
  configManager: { config: commonConfig, load() {} },
  storage,
});

// ---- fake @screeps/driver: the members the engine uses (reference/driver/lib/index.js) ----
const config = Object.assign(new EventEmitter(), {
  mainLoopMinDuration: 1000,
  mainLoopResetInterval: 5000,
  cpuMaxPerTick: 500,
  cpuBucketSize: 10000,
  customIntentTypes: {},
  historyChunkSize: 20,
});
commonConfig.engine = config;

let pathFinderGlue = null;
let native = null;
const queues = {};

const driver = {
  config,
  constants: commonConfig.common.constants,
  strongholds: commonConfig.common.strongholds,
  system: commonConfig.common.system,
  customObjectPrototypes: [],
  connect: () => q().when(true),
  queue: {
    create(name) {
      return (queues[name] = queues[name] || {
        waiting: null,
        fetch() {
          const d = q().defer();
          this.waiting = d;
          return d.promise;
        },
        markDone(id) {
          capture.done = true;
          if (state && state.onDone) state.onDone(id);
          return q().when();
        },
      });
    },
  },
  mapById(array, fn) {
    const result = {};
    for (const i of array) {
      result[i._id.toString()] = i;
      fn && fn(i);
    }
    return result;
  },
  // PROC-INPUT, index.js:245-280,412-423
  getRoomIntents: (roomId) => q().when(json(by('rooms.intents').find((d) => d.room === roomId) || null)),
  getRoomObjects(roomId) {
    const objects = json(by('rooms.objects').filter((d) => d.room === roomId));
    let users = {};
    const result = {
      objects: driver.mapById(objects, (obj) => {
        if (obj.user) users[obj.user] = true;
      }),
    };
    users = Object.keys(users);
    result.users = driver.mapById(json(by('users').filter((u) => users.includes(String(u._id)))));
    return q().when(result);
  },
  getRoomTerrain: (roomId) => q().when(driver.mapById(json(by('rooms.terrain').filter((d) => d.room === roomId)))),
  getGameTime: () => q().when(state.gameTime),
  getRoomInfo: (roomId) => q().when(json(by('rooms').find((d) => d._id === roomId) || null)),
  getRoomFlags: (roomId) => q().when(json(by('rooms.flags').filter((d) => d.room === roomId))),
  clearRoomIntents: () => q().when(),
  bulkObjectsWrite: () => flagged(bulk('rooms.objects')),
  bulkFlagsWrite: () => flagged(bulk('rooms.flags')),
  bulkUsersWrite: () => flagged(bulk('users')),
  bulkRoomsWrite: () => flagged(bulk('rooms')),
  bulkTransactionsWrite: () => flagged(bulk('transactions')),
  bulkMarketOrders: () => flagged(bulk('market.orders')),
  bulkMarketIntershardOrders: () => flagged(bulk('market.orders')),
  bulkUsersMoney: () => flagged(bulk('users.money')),
  bulkUsersResources: () => flagged(bulk('users.resources')),
  bulkUsersPowerCreeps: () => flagged(bulk('users.power_creeps')),
  activateRoom(room) {
    capture.activated.push(...[].concat(room));
    return q().when();
  },
  saveRoomInfo(roomId, roomInfo) {
    capture.roomInfoSaved = true;
    const doc = by('rooms').find((d) => d._id === roomId);
    if (doc) updateDocument(doc, json({ $set: roomInfo }));
    return q().when();
  },
  mapViewSave(roomId, mapView) {
    capture.mapView = JSON.stringify(mapView);
    return q().when();
  },
  saveRoomEventLog(roomId, eventLog) {
    capture.eventLog = JSON.stringify(eventLog);
    return q().when();
  },
  sendNotification(userId, message) {
    capture.notifications.push([userId, message]);
    return q().when();
  },
  getRoomStatsUpdater: () => ({
    inc(name, userId, amount) {
      capture.stats.push([userId, name, amount]);
    },
  }),
  roomsStatsSave: () => q().when(),
  history: {
    upload: () => q().when(),
    saveTick(roomId, gameTime, data) {
      capture.history = data;
      return q().when();
    },
  },
  getWorldSize: () => state.worldSize,
  addRoomToUser(roomId, user, bulk) {
    if (!user.rooms || user.rooms.indexOf(roomId) == -1) bulk.addToSet(user, 'rooms', roomId);
  },
  removeRoomFromUser(roomId, user, bulk) {
    if (user.rooms && user.rooms.indexOf(roomId) != -1) bulk.pull(user, 'rooms', roomId);
  },
  // GLOB-INPUT, index.js:429-449: the vanilla queries over the scenario collections.
  getInterRoom() {
    const objects = by('rooms.objects');
    const orders = json(by('market.orders'));
    const userPowerCreeps = json(by('users.power_creeps'));
    const userIntents = json(by('users.intents'));
    const ids = [...orders, ...userPowerCreeps, ...userIntents].map((d) => d.user);
    return q().when([
      state.gameTime,
      json(objects.filter((d) => ['creep', 'powerCreep'].includes(d.type) && d.interRoom != null)),
      driver.mapById(json(by('rooms').filter((r) => r.status === 'normal' && (!r.openTime || r.openTime < Date.now())))),
      json(objects.filter((d) => ['terminal', 'powerSpawn', 'powerCreep'].includes(d.type))),
      {
        users: json(by('users').filter((u) => ids.includes(u._id))), // Loki $in: indexOf, strict

        orders,
        userPowerCreeps,
        userIntents,
        shardName: '',
      },
    ]);
  },
  clearGlobalIntents() {
    state.db['users.intents'] = [];
    return q().when();
  },
  clearMarketIntents: () => q().when(),
  get pathFinder() {
    if (!pathFinderGlue) {
      native = require(NATIVE);
      pathFinderGlue = require(reference('driver/lib/path-finder.js'));
      Object.defineProperty(driver, 'pathFinder', { value: pathFinderGlue.create(native), enumerable: true });
    }
    return driver.pathFinder;
  },
};
let bulk = null;

const stubs = { '@screeps/driver': driver, '@screeps/core': driver, '@screeps/common': common };
for (const [k, v] of Object.entries(stubs)) require.cache[`chitin-oracle:${k}`] = { id: k, filename: k, loaded: true, exports: v, children: [], paths: [] };
bulk = require(reference('driver/lib/bulk.js'));

// Starts a scenario: collections, clock, RNG stream, pathfinder terrain. `stream` is 'room', 'global', or a
// function used as Math.random (cron.js: the current unit's cron stream).
function begin(s, stream) {
  state = s;
  for (const k of Object.keys(capture)) delete capture[k];
  Object.assign(capture, {
    ops: {}, idCounters: {}, foreign: [], storeErrors: [], fRows: [], activated: [], notifications: [], stats: [],
    roomInfoSaved: false, mapView: null, eventLog: null, history: null, crashed: null, done: false,
  });
  const T = BigInt(s.gameTime);
  random = typeof stream === 'function' ? stream
    : (stream === 'global' ? rng.globalStream(s.worldSeed, T) : rng.roomStream(s.worldSeed, T, s.room)).random;
  now = vclock.at(s.clock, s.gameTime);
  if (s.terrains && s.terrains.length) {
    void driver.pathFinder;
    pathFinderGlue.init(native, s.terrains); // [{room, terrain}]; loadTerrain overwrites per room
  }
}

function end() {
  state = null;
  now = null;
}

module.exports = { driver, common, capture, begin, end, queues, reference, json, reached };
