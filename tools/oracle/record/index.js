// chitin recorder: a screepsmod for the official server (E:\screeps\server) that makes a run replayable
// exactly (ARCHITECTURE §12.4). Load it twice: as `node --require <this file>` (NODE_OPTIONS, so Math.random
// and Date are wrapped before lodash captures Math.random) and as a mods.json entry (the hooks).
// tools/oracle/record/run.js sets both up in an isolated server directory.
//
// What it does, per engine process:
//   processor  Math.random = the room stream of (room, T), Date = vclock(T) while a room is processed; every
//              processed room becomes a line of rooms.ndjson: {name:"<T>/<room>", scenario (FROZEN room
//              format, from the processor's own reads), recorded: {ops, mapView, eventLog, activated,
//              roomInfo, notifications, stats, history, crash}}.
//   main       Math.random = the global stream of T, Date = vclock(T) during the global stage; global.ndjson
//              gets {name:"<T>/global", scenario (kind global, from getInterRoom), recorded: {ops, activated}};
//              a db.json-shaped snapshot (collections + env keys) at the first tick and every
//              CHITIN_RECORD_SNAPSHOT_EVERY ticks (100) as snapshot-<T>.json.
//   runner     order.ndjson: {T, i, user} per runUser, the fetch order (record with runner_threads = 1).
// Environment: CHITIN_RECORD_DIR (./chitin-record), CHITIN_RECORD_SEED (0x1), CHITIN_RECORD_BASE_MS
// (1700000000000; the clock is {baseMs, baseTick: 0, nominalTickMs: 1000}), CHITIN_RECORD_SNAPSHOT_EVERY.
// Crons are not recorded: run with them off (they draw from Math.random outside any stream).
'use strict';
const fs = require('fs');
const path = require('path');
const rng = require('../lib/rng');
const vclock = require('../lib/vclock');

const SEED = process.env.CHITIN_RECORD_SEED || '0x1';
const CLOCK = { baseMs: Number(process.env.CHITIN_RECORD_BASE_MS || 1700000000000), baseTick: 0, nominalTickMs: 1000 };
const OUT = path.resolve(process.env.CHITIN_RECORD_DIR || 'chitin-record');
const SNAPSHOT_EVERY = Number(process.env.CHITIN_RECORD_SNAPSHOT_EVERY || 100);

// ---- preload: the wrappers (idempotent; inactive until a hook sets a stream / time) ----
if (!global.__chitinRecord) {
  const preloaded = !Object.keys(require.cache).some((k) => /[\\/]lodash[\\/]/.test(k));
  let random = null;
  let now = null;
  // Engine processes only (storage, backend keep the real Math.random and Date).
  if (/[\\/]@screeps[\\/]engine[\\/]/.test(process.argv[1] || '')) {
    const realRandom = Math.random;
    Math.random = () => (random ? random() : realRandom());
    const RealDate = Date;
    class VDate extends RealDate {
      constructor(...a) {
        if (a.length || now === null) super(...a);
        else super(now);
      }
      static now() {
        return now === null ? RealDate.now() : now;
      }
      static [Symbol.hasInstance](x) {
        return x instanceof RealDate;
      }
    }
    global.Date = VDate;
  }
  global.__chitinRecord = {
    preloaded,
    set(T, stream) {
      random = stream ? stream.random : null;
      now = T === null ? null : vclock.at(CLOCK, T);
    },
  };
}
const R = global.__chitinRecord;

const json = (v) => (v === undefined ? undefined : JSON.parse(JSON.stringify(v)));
const append = (file, line) => {
  fs.mkdirSync(OUT, { recursive: true });
  fs.appendFileSync(path.join(OUT, file), JSON.stringify(line) + '\n');
};
// Wraps obj[name] (a promise-returning fn) with after(args, result).
function tap(obj, name, after) {
  const orig = obj[name];
  obj[name] = function (...args) {
    const res = orig.apply(this, args);
    after(args, res);
    return res;
  };
}

function processor(config) {
  const driver = config.engine.driver;
  const storage = config.common.storage;
  const rooms = {}; // roomId → capture
  let cur = null;
  let T = null;
  const at = (roomId) => (rooms[roomId] = rooms[roomId] || { reads: {}, ops: {}, activated: false, notifications: [], stats: [],
    mapView: null, eventLog: null, roomInfo: null, history: null, crash: null });
  for (const [fn, key] of [['getRoomIntents', 'intents'], ['getRoomObjects', 'objects'], ['getRoomTerrain', 'terrain'], ['getRoomInfo', 'info'], ['getRoomFlags', 'flags']]) {
    tap(driver, fn, ([roomId], p) => p.then((d) => (at(roomId).reads[key] = json(d))));
  }
  tap(driver, 'getGameTime', (_a, p) => p.then((t) => (T = +t)));
  // Users only named by intents are not read by the processor; the scenario needs their docs too.
  const origIntents = driver.getRoomIntents;
  driver.getRoomIntents = (roomId) => origIntents(roomId).then((intents) => {
    const ids = intents ? Object.keys(intents.users || {}) : [];
    if (!ids.length) return intents;
    return storage.db.users.find({ _id: { $in: ids } }).then((users) => ((at(roomId).reads.intentUsers = users), intents));
  });
  // A processed room is written once its history save arrived (an activated room saves history after the
  // processor loop moved on; every 20 ticks only after a slow upload), or 10 ticks later.
  let done = [];
  const byTick = {}; // "<T>/<room>" → capture, for the late history saves
  config.engine.on('processorLoopStage', (stage, roomId) => {
    if (stage !== 'processRoom') return;
    const ready = (d) => d.c.T !== T && (d.c.history !== null || !d.c.activated || T - d.c.T >= 10);
    for (const d of done.filter(ready)) {
      delete byTick[`${d.c.T}/${d.roomId}`];
      append('rooms.ndjson', roomLine(d.roomId, d.c, driver));
    }
    done = done.filter((d) => !ready(d));
    cur = at(roomId);
    cur.T = T;
    byTick[`${T}/${roomId}`] = cur;
    R.set(T, rng.roomStream(SEED, BigInt(T), roomId));
  });
  for (const coll of ['rooms.objects', 'users', 'rooms.flags', 'users.power_creeps']) {
    tap(storage.db[coll], 'bulk', ([ops]) => cur && ((cur.ops[coll] = cur.ops[coll] || []).push(...json(ops))));
  }
  tap(driver, 'mapViewSave', ([roomId, v]) => (at(roomId).mapView = JSON.stringify(v)));
  tap(driver, 'saveRoomEventLog', ([roomId, v]) => (at(roomId).eventLog = JSON.stringify(v)));
  tap(driver, 'saveRoomInfo', ([roomId, v]) => (at(roomId).roomInfo = json(v)));
  tap(driver, 'activateRoom', ([room]) => [].concat(room).forEach((r) => rooms[r] && (rooms[r].activated = true)));
  tap(driver, 'sendNotification', ([user, message]) => cur && cur.notifications.push([user, message]));
  tap(driver.history, 'saveTick', ([roomId, t, data]) => byTick[`${t}/${roomId}`] && (byTick[`${t}/${roomId}`].history = data));
  const origStats = driver.getRoomStatsUpdater;
  driver.getRoomStatsUpdater = (room) => {
    const u = origStats(room);
    return { inc: (name, userId, amount) => (at(room).stats.push([userId, name, amount]), u.inc(name, userId, amount)) };
  };
  const log = console.log;
  console.log = (...a) => {
    if (typeof a[0] === 'string' && a[0].startsWith('Error processing room ')) at(a[0].slice(22, -1)).crash = String(a[1]).split('\n')[0];
    log(...a);
  };
  const origClear = driver.clearRoomIntents;
  driver.clearRoomIntents = (roomId) => {
    const c = rooms[roomId];
    delete rooms[roomId];
    if (cur === c) cur = null;
    if (c && c.T !== undefined) done.push({ roomId, c });
    return origClear(roomId);
  };
}

// The FROZEN room scenario the processor saw, plus what it did.
function roomLine(room, c, driver) {
  const r = c.reads;
  const objects = Object.values((r.objects && r.objects.objects) || {});
  const users = Object.assign({}, (r.objects && r.objects.users) || {});
  for (const u of r.intentUsers || []) users[u._id] = users[u._id] || u;
  const info = Object.assign({}, r.info);
  delete info._id;
  const terrainDoc = Object.values(r.terrain || {})[0] || {};
  const scenario = {
    kind: 'room', worldSeed: SEED, clock: CLOCK, gameTime: c.T, room, roomDoc: info, terrain: terrainDoc.terrain, objects,
    flags: r.flags || [], users,
    intents: Object.entries((r.intents && r.intents.users) || {}).map(([uid, i]) => [uid, { objects: i.objects || null, objectsManual: i.objectsManual || null }]),
    accessibleRooms: [room], roomStatusData: {}, worldSize: driver.getWorldSize(), compat: 'private', tags: ['recorded'],
  };
  const { reads, T, ...recorded } = c;
  void reads;
  void T;
  return { name: `${c.T}/${room}`, scenario, recorded };
}

function main(config) {
  const driver = config.engine.driver;
  const storage = config.common.storage;
  let T = null;
  let g = null;
  let snapshotted = null;
  config.engine.on('mainLoopStage', (stage) => {
    if (stage === 'start') driver.getGameTime().then((t) => (T = +t));
    if (stage === 'global' && T !== null) {
      R.set(T, rng.globalStream(SEED, BigInt(T)));
      g = { T, ops: {}, activated: [] };
    }
    if (stage === 'commit2' && g) {
      R.set(null, null);
      const line = g;
      g = null;
      if (line.data) append('global.ndjson', globalLine(line));
    }
    if (stage === 'finish') {
      driver.getGameTime().then((t) => {
        if (snapshotted === null || t - snapshotted >= SNAPSHOT_EVERY) {
          snapshotted = +t;
          return snapshot(config, +t);
        }
      });
    }
  });
  tap(driver, 'getInterRoom', (_a, p) => p.then((data) => g && (g.data = json(data))));
  for (const coll of ['rooms.objects', 'rooms', 'users', 'market.orders', 'users.money', 'transactions', 'users.resources', 'users.power_creeps']) {
    tap(storage.db[coll], 'bulk', ([ops]) => g && ((g.ops[coll] = g.ops[coll] || []).push(...json(ops))));
  }
  tap(driver, 'activateRoom', ([room]) => g && g.activated.push(...[].concat(room)));
}

// A global scenario from the five getInterRoom results (rooms.objects = the movers, then the other
// terminals/power spawns/power creeps; only accessible rooms are known).
function globalLine(g) {
  const [gameTime, creeps, accessibleRooms, roomObjects, rest = {}] = g.data;
  const seen = new Set(creeps.map((o) => o._id));
  const db = {
    rooms: Object.values(accessibleRooms || {}),
    'rooms.objects': creeps.concat(roomObjects.filter((o) => !seen.has(o._id))),
    users: rest.users || [],
    'market.orders': rest.orders || [],
    'users.power_creeps': rest.userPowerCreeps || [],
    'users.intents': rest.userIntents || [],
    transactions: [], 'users.money': [], 'users.resources': [],
  };
  const scenario = { kind: 'global', worldSeed: SEED, clock: CLOCK, gameTime: +gameTime, worldSize: 0, compat: 'private', tags: ['recorded'], db };
  return { name: `${gameTime}/global`, scenario, recorded: { ops: g.ops, activated: [...new Set(g.activated)].sort() } };
}

// db.json-shaped: {collections: [{name, data}], env: {…}} with the env keys chitin imports.
async function snapshot(config, T) {
  const storage = config.common.storage;
  const collections = [];
  for (const name of config.common.dbCollections) collections.push({ name, data: await storage.db[name].find({}) });
  const env = {};
  const users = collections.find((c) => c.name === 'users').data;
  const keys = ['gameTime', 'accessibleRooms', 'roomStatusData', 'activeRooms', 'tickRate', 'mainLoopPaused'];
  for (const u of users) keys.push(`memory:${u._id}`, `memorySegments:${u._id}`, `publicMemorySegments:${u._id}`);
  for (const k of keys) {
    const v = await storage.env.get(k);
    if (v !== undefined && v !== null) env[k] = v;
  }
  const rooms = collections.find((c) => c.name === 'rooms').data;
  for (const r of rooms) {
    const mv = await storage.env.get(`mapView:${r._id}`);
    if (mv) env[`mapView:${r._id}`] = mv;
  }
  env.roomEventLog = await storage.env.hmget('roomEventLog', rooms.map((r) => r._id)).catch(() => undefined);
  fs.mkdirSync(OUT, { recursive: true });
  fs.writeFileSync(path.join(OUT, `snapshot-${T}.json`), JSON.stringify({ tick: T, clock: CLOCK, worldSeed: SEED, collections, env }));
}

function runner(config) {
  const driver = config.engine.driver;
  let i = 0;
  if (String(process.env.RUNNER_THREADS) !== '1') console.log('chitin recorder: run with runner_threads = 1 for a replayable fetch order');
  config.engine.on('runnerLoopStage', (stage, userId) => {
    if (stage !== 'runUser') return;
    const n = i++;
    driver.getGameTime().then((T) => append('order.ndjson', { T: +T, i: n, user: String(userId) }));
  });
}

// Backend: every cron job becomes a no-op (their draws and wall-clock timing are outside any stream),
// unless CHITIN_RECORD_CRONS=1.
function backend(config) {
  if (process.env.CHITIN_RECORD_CRONS === '1') return;
  const off = () => {
    if (!config.cronjobs) return setTimeout(off, 200);
    for (const job of Object.values(config.cronjobs)) job[1] = () => Promise.resolve();
    console.log('chitin recorder: cron jobs disabled');
  };
  off();
}

module.exports = function (config) {
  if (config.backend) backend(config);
  if (!config.engine) return;
  if (!R.preloaded) console.log('chitin recorder: not preloaded (NODE_OPTIONS=--require …/record/index.js); lodash draws stay unseeded');
  config.engine.on('init', (type) => {
    if (type === 'processor') processor(config);
    if (type === 'main') main(config);
    if (type === 'runner') runner(config);
  });
};
