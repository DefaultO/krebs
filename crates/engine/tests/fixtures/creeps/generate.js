// Oracle for WP09's room processing: runs the official processor (reference/engine/src/processor.js, loaded
// unchanged) over scenarios through a fake driver whose bulk writers are the real driver bulk writer, and
// writes cases.ndjson next to this file. The tick handlers of every object type but creeps are replaced by
// no-ops on this side and in the Rust replay (their owners are other packages), so a case checks
// processor.js, movement.js, utils.js and the creep files only. Each line is a §12.2 room scenario plus
// `result` = {ops, docsAfter, flagsAfter, roomInfo, eventLog, mapView, activated, notifications, stats,
// crashed}. Usage: node22 crates/engine/tests/fixtures/creeps/generate.js
// (Meant to move to tools/oracle/gen/ once the shared fake driver of tools/oracle/lib exists.)
'use strict';
const Module = require('module');
const path = require('path');
const fs = require('fs');
const EventEmitter = require('events');
const { reference, prng } = require(path.join(__dirname, '..', '..', '..', '..', '..', 'tools', 'oracle', 'lib', 'common'));
const { dbBulk } = require(path.join(__dirname, '..', '..', '..', '..', '..', 'tools', 'oracle', 'lib', 'loki-update'));

const SERVER_MODULES = process.env.SCREEPS_MODULES || 'E:\\screeps\\server\\node_modules';
const LODASH = reference(path.join('lodash-3.10.1', 'lodash.js'));
const ENGINE = reference(path.join('engine', 'src'));
const C = require(reference(path.join('common', 'lib', 'constants.js')));
const ROOM = 'W1N1';

// ---- current scenario state, read and written by the fake driver ----
let cur = null;
const captured = (name) => (cur.ops[name] = cur.ops[name] || []);

const common = {
  storage: {
    db: new Proxy({}, {
      get: (_, coll) => ({
        bulk(ops) {
          // The storage RPC: payloads are serialized now.
          captured(coll).push(...JSON.parse(JSON.stringify(ops)));
          return Promise.resolve();
        },
      }),
    }),
  },
};

// Every `Math.random` the engine reaches (lodash binds its own at load) reads the current stream.
const rng = require(path.join(__dirname, '..', '..', '..', '..', '..', 'tools', 'oracle', 'lib', 'rng'));
let stream = null;
const fakeMath = Object.create(Math);
fakeMath.random = () => stream.random();
Math.random = () => stream.random();
const stubs = {
  q: require(path.join(SERVER_MODULES, 'q')),
  '@screeps/common': common,
  lodash: require(LODASH).runInContext({ Math: fakeMath }),
};
const NOOP_TICK = 'chitin-stub:noop-tick';
const resolve = Module._resolveFilename;
Module._resolveFilename = function (request, parent, ...rest) {
  if (request === 'bulk-require') return path.join(SERVER_MODULES, 'bulk-require', 'index.js');
  if (request in stubs) return `chitin-stub:${request}`;
  if (request === '@screeps/core') return 'chitin-stub:driver';
  const norm = request.split('\\').join('/');
  if (/^\.\/processor\/intents\/[^/]+\/tick$/.test(norm) && norm !== './processor/intents/creeps/tick') return NOOP_TICK;
  return resolve.call(this, request, parent, ...rest);
};
const register = (id, exports) => (require.cache[id] = { id, filename: id, loaded: true, exports, children: [] });
for (const [k, v] of Object.entries(stubs)) register(`chitin-stub:${k}`, v);
register(NOOP_TICK, function () {});

const makeBulk = require(reference(path.join('driver', 'lib', 'bulk.js')));

let fetchNext = null;
const driver = {
  constants: C,
  config: Object.assign(new EventEmitter(), { historyChunkSize: 20 }),
  connect: () => Promise.resolve(),
  queue: { create: () => Promise.resolve({ fetch: () => fetchNext(), markDone: () => { done(); return Promise.resolve(); } }) },
  getRoomIntents: () => Promise.resolve(cur.input.intents),
  getRoomObjects: () => Promise.resolve({ objects: cur.input.objects, users: cur.input.users }),
  getRoomTerrain: () => Promise.resolve({ t: { _id: 't', room: ROOM, type: 'terrain', terrain: cur.scenario.terrain } }),
  getGameTime: () => Promise.resolve(cur.scenario.gameTime),
  getRoomInfo: () => Promise.resolve(cur.input.roomInfo),
  getRoomFlags: () => Promise.resolve(cur.input.flags),
  clearRoomIntents: () => Promise.resolve(),
  bulkObjectsWrite: () => makeBulk('rooms.objects'),
  bulkUsersWrite: () => makeBulk('users'),
  bulkFlagsWrite: () => makeBulk('rooms.flags'),
  bulkUsersPowerCreeps: () => makeBulk('users.power_creeps'),
  getRoomStatsUpdater: () => ({ inc: (name, user, amount) => cur.stats.push([name, String(user), amount]) }),
  pathFinder: { make() {} },
  activateRoom: () => { cur.activated = true; return Promise.resolve(); },
  mapViewSave: (room, mapView) => { cur.mapView = JSON.stringify(mapView); return Promise.resolve(); },
  saveRoomEventLog: (room, log) => { cur.eventLog = JSON.stringify(log); return Promise.resolve(); },
  saveRoomInfo: (room, info) => { cur.roomInfo = JSON.parse(JSON.stringify(info)); return Promise.resolve(); },
  roomsStatsSave: () => Promise.resolve(),
  history: { upload: () => Promise.resolve(), saveTick: () => Promise.resolve() },
  sendNotification: (user, message) => { cur.notifications.push([String(user), message]); return Promise.resolve(); },
  getWorldSize: () => 4,
  // driver/lib/index.js:542-546, verbatim
  addRoomToUser: (roomId, user, bulk) => {
    if (!user.rooms || user.rooms.indexOf(roomId) == -1) {
      bulk.addToSet(user, 'rooms', roomId);
    }
  },
  mapById: (array) => array.reduce((r, i) => ((r[i._id.toString()] = i), r), {}),
};
register('chitin-stub:driver', driver);

const log = console.log;
console.log = (...args) => {
  if (cur && typeof args[0] === 'string' && args[0].startsWith('Error processing room')) {
    cur.crashed = String(args[1] && args[1].message || args[1]).split('\n')[0];
    return;
  }
  log(...args);
};
console.error = () => {}; // getOffsetsByDirection's "Wrong move direction" trace

// ---- scenarios ----
const T0 = 1000;
const clone = (x) => JSON.parse(JSON.stringify(x));
const NULL_LOG = { attacked: null, healed: null, attack: null, rangedAttack: null, rangedMassAttack: null, rangedHeal: null, harvest: null, heal: null, repair: null, build: null, say: null, upgradeController: null, reserveController: null };

function terrainOf(walls = [], swamps = []) {
  const t = Array(2500).fill('0');
  for (const [x, y] of swamps) t[y * 50 + x] = '2';
  for (const [x, y] of walls) t[y * 50 + x] = '1';
  return t.join('');
}

function creep(id, x, y, parts, extra = {}) {
  const body = parts.map((p) => (typeof p === 'string' ? { type: p, hits: 100 } : p));
  const hits = body.reduce((s, p) => s + p.hits, 0);
  return Object.assign({ _id: id, type: 'creep', x, y, room: ROOM, name: id, user: 'u1', body, hits, hitsMax: body.length * 100, spawning: false, fatigue: 0, store: {}, storeCapacity: body.filter((p) => p.type === 'carry').length * 50, ageTime: T0 + 1500, actionLog: clone(NULL_LOG) }, extra);
}

const users = { u1: { _id: 'u1', username: 'u1', gcl: 0 }, u2: { _id: 'u2', username: 'u2', gcl: 0 } };

function scenario(name, tags, objects, intents, extra = {}) {
  return Object.assign({
    kind: 'room', worldSeed: '0x1', clock: { baseMs: 0, baseTick: 0, nominalTickMs: 1000 }, gameTime: T0, room: ROOM,
    roomDoc: { _id: ROOM, status: 'normal' }, terrain: terrainOf(), objects, flags: [], users: clone(users),
    intents, accessibleRooms: [ROOM], roomStatusData: {}, worldSize: 4, compat: 'private', tags, name,
  }, extra);
}

const one = (uid, objects) => [[uid, { objects, objectsManual: null }]];
const SCENARIOS = [];
const add = (...a) => SCENARIOS.push(scenario(...a));

// Hand-written cases (the same situations as processor.rs's unit tests, plus edge rows).
const source = { _id: 's1', type: 'source', x: 11, y: 10, room: ROOM, energy: 3000, energyCapacity: 3000 };
const ctrl = (extra = {}) => Object.assign({ _id: 'ctrl', type: 'controller', x: 12, y: 12, room: ROOM, level: 1, user: 'u1', progress: 0, downgradeTime: 20000 }, extra);
add('move', ['creeps/move.js', 'intents/movement.js', 'src/processor.js'], [creep('c1', 10, 10, ['move'])], one('u1', { c1: { move: { direction: 3, id: 'undefined' } } }));
add('harvest+upgrade', ['creeps/harvest.js', 'creeps/upgradeController.js', 'src/utils.js'], [creep('c1', 10, 10, ['work', 'carry', 'move'], { store: { energy: 0 } }), source, ctrl()], one('u1', { c1: { harvest: { id: 's1' }, upgradeController: { id: 'ctrl' } } }));
add('harvest overflow drop', ['creeps/harvest.js', 'creeps/drop.js', 'intents/_create-energy.js'], [creep('c1', 10, 10, ['work', 'work', 'work', 'carry', 'move'], { store: { energy: 48 } }), source], one('u1', { c1: { harvest: { id: 's1' } } }));
add('upgrade levels up', ['creeps/upgradeController.js'], [creep('c1', 10, 10, ['work', 'work', 'carry', 'move'], { store: { energy: 50 } }), ctrl({ progress: 199, downgradeTime: 21000 })], one('u1', { c1: { upgradeController: { id: 'ctrl' } } }));
add('upgrade level 8 limit', ['creeps/upgradeController.js'], [creep('c1', 10, 10, Array(20).fill('work').concat(['carry', 'move']), { store: { energy: 50 } }), ctrl({ level: 8, progress: 0, downgradeTime: 300000 })], one('u1', { c1: { upgradeController: { id: 'ctrl' } } }));
add('pickup', ['creeps/pickup.js'], [creep('c1', 10, 10, ['carry', 'move']), { _id: 'd1', type: 'energy', x: 11, y: 10, room: ROOM, energy: 30, resourceType: 'energy' }], one('u1', { c1: { pickup: { id: 'd1' } } }));
add('build progress', ['creeps/build.js'], [creep('c1', 10, 10, ['work', 'work', 'carry', 'move'], { store: { energy: 50 } }), { _id: 'cs1', type: 'constructionSite', x: 10, y: 12, room: ROOM, user: 'u1', structureType: 'road', progress: 0, progressTotal: 300 }], one('u1', { c1: { build: { id: 'cs1', x: 10, y: 12 } } }));
for (const st of ['road', 'spawn', 'extension', 'rampart', 'constructedWall', 'link', 'storage', 'tower', 'observer', 'extractor', 'lab', 'terminal', 'container', 'nuker', 'factory', 'powerSpawn']) {
  const total = st === 'road' ? 300 : 10;
  add(`build completes ${st}`, ['creeps/build.js'], [creep('c1', 10, 10, ['work', 'work', 'carry', 'move'], { store: { energy: 50 } }), ctrl({ level: 8, downgradeTime: 300000 }), { _id: 'cs1', type: 'constructionSite', x: 10, y: 12, room: ROOM, user: 'u1', structureType: st, progress: total - 3, progressTotal: total, name: 'Spawn9' }], one('u1', { c1: { build: { id: 'cs1', x: 10, y: 12 } } }), { terrain: terrainOf([], [[10, 12]]) });
}
add('repair', ['creeps/repair.js'], [creep('c1', 10, 10, ['work', 'carry', 'move'], { store: { energy: 10 } }), { _id: 'r1', type: 'road', x: 10, y: 11, room: ROOM, hits: 4000, hitsMax: 5000, nextDecayTime: 5000 }], one('u1', { c1: { repair: { id: 'r1', x: 10, y: 11 } } }));
add('transfer clipped', ['creeps/transfer.js'], [creep('c1', 10, 10, ['carry', 'move'], { store: { energy: 50 } }), { _id: 'ct1', type: 'container', x: 11, y: 10, room: ROOM, store: { energy: 1990 }, storeCapacity: 2000, hits: 250000, hitsMax: 250000 }], one('u1', { c1: { transfer: { id: 'ct1', amount: 50, resourceType: 'energy' } } }));
add('transfer lab mineral', ['creeps/transfer.js'], [creep('c1', 10, 10, ['carry', 'move'], { store: { H: 50 } }), { _id: 'lab1', type: 'lab', x: 11, y: 10, room: ROOM, user: 'u1', store: { energy: 0 }, storeCapacity: 5000, storeCapacityResource: { energy: 2000 }, hits: 500, hitsMax: 500 }], one('u1', { c1: { transfer: { id: 'lab1', amount: 50, resourceType: 'H' } } }));
add('withdraw', ['creeps/withdraw.js'], [creep('c1', 10, 10, ['carry', 'move']), { _id: 'ct1', type: 'container', x: 11, y: 10, room: ROOM, store: { energy: 100 }, storeCapacity: 2000, hits: 250000, hitsMax: 250000 }], one('u1', { c1: { withdraw: { id: 'ct1', amount: 100, resourceType: 'energy' } } }));
add('withdraw lab empties', ['creeps/withdraw.js'], [creep('c1', 10, 10, ['carry', 'move']), { _id: 'lab1', type: 'lab', x: 11, y: 10, room: ROOM, user: 'u1', store: { energy: 0, H: 30 }, storeCapacityResource: { energy: 2000, H: 3000 }, hits: 500, hitsMax: 500 }], one('u1', { c1: { withdraw: { id: 'lab1', amount: 30, resourceType: 'H' } } }));
add('withdraw crash (no store)', ['creeps/withdraw.js'], [creep('c1', 10, 10, ['carry', 'move']), { _id: 'r1', type: 'road', x: 11, y: 10, room: ROOM, hits: 5000, hitsMax: 5000 }], one('u1', { c1: { withdraw: { id: 'r1', amount: 10, resourceType: 'energy' } } }));
add('suicide', ['creeps/suicide.js', 'creeps/_die.js'], [creep('c1', 10, 10, ['work', 'carry', 'move'], { store: { energy: 7 }, ageTime: T0 + 750 })], one('u1', { c1: { suicide: {} } }));
add('suicide boosted into container', ['creeps/suicide.js', 'creeps/_die.js'], [creep('c1', 10, 10, [{ type: 'work', hits: 100, boost: 'XUHO2' }, { type: 'carry', hits: 0, boost: 'KH' }, 'move', 'claim'], { store: { energy: 20, H: 5 } }), { _id: 'ct1', type: 'container', x: 10, y: 10, room: ROOM, store: { energy: 1980 }, storeCapacity: 2000, hits: 1, hitsMax: 250000 }], one('u1', { c1: { suicide: {} } }));
add('suicide with an object body', ['creeps/suicide.js', 'creeps/_die.js'], [creep('c1', 10, 10, ['move'], { body: { 0: { type: 'move', hits: 100 } } })], one('u1', { c1: { suicide: {} } }));
add('userSummoned dies next to a foreign creep', ['creeps/tick.js', 'creeps/_die.js'], [ctrl(), creep('s1', 10, 10, ['move'], { userSummoned: true, store: { energy: 5 } }), creep('f1', 20, 20, ['move'], { user: 'u2' })], []);
add('userSummoned without a controller', ['creeps/tick.js'], [creep('s1', 10, 10, ['move'], { userSummoned: true }), creep('f1', 20, 20, ['move'])], []);
add('userSummoned with only own creeps', ['creeps/tick.js'], [ctrl(), creep('s1', 10, 10, ['move'], { userSummoned: true }), creep('f1', 20, 20, ['move'])], []);
add('stronghold invader suicide', ['creeps/suicide.js', 'creeps/_die.js'], [creep('i1', 10, 10, ['work', 'move'], { user: '2', strongholdId: 'sh1', store: { energy: 30 }, storeCapacity: 50 })], one('2', { i1: { suicide: {} } }), { users: Object.assign({ 2: { _id: '2', username: 'Invader' } }, clone(users)) });
add('public say then suicide', ['creeps/say.js', 'creeps/suicide.js', 'creeps/_die.js'], [creep('c1', 10, 10, [{ type: 'work', hits: 100, boost: 'UO' }, 'carry', 'move'], { store: { energy: 9 }, tombstoneDecay: 77 })], one('u1', { c1: { say: { message: 'goodbye cruel world', isPublic: true }, suicide: {} } }));
add('private say then suicide', ['creeps/say.js', 'creeps/suicide.js', 'creeps/_die.js'], [creep('c1', 10, 10, ['claim', 'move'])], one('u1', { c1: { say: { message: 'psst', isPublic: false }, suicide: {} } }));
add('say', ['creeps/say.js'], [creep('c1', 10, 10, ['move'])], one('u1', { c1: { say: { message: 'hello world!!', isPublic: true } } }));
add('move tie', ['intents/movement.js'], [creep('c1', 10, 10, ['move']), creep('c2', 12, 10, ['move'])], one('u1', { c1: { move: { direction: 3 } }, c2: { move: { direction: 7 } } }));
add('move tie two users', ['intents/movement.js'], [creep('c1', 10, 10, ['move']), creep('c2', 12, 10, ['move'], { user: 'u2' })], [['u2', { objects: { c2: { move: { direction: 7 } } }, objectsManual: null }], ['u1', { objects: { c1: { move: { direction: 3 } } }, objectsManual: null }]]);
add('move rates', ['intents/movement.js'], [creep('c1', 10, 10, ['move', 'work', 'work']), creep('c2', 12, 10, ['move', 'move']), creep('c3', 11, 11, ['move'], { store: { energy: 50 } })], one('u1', { c3: { move: { direction: 1 } }, c1: { move: { direction: 3 } }, c2: { move: { direction: 7 } } }));
add('move swap', ['intents/movement.js'], [creep('c1', 10, 10, ['move']), creep('c2', 11, 10, ['move'])], one('u1', { c1: { move: { direction: 3 } }, c2: { move: { direction: 7 } } }));
add('move chain blocked', ['intents/movement.js'], [creep('c1', 10, 10, ['move']), creep('c2', 11, 10, ['move']), creep('c3', 12, 10, ['move'])], one('u1', { c1: { move: { direction: 3 } }, c2: { move: { direction: 3 } } }));
add('move wall and tunnel', ['intents/movement.js'], [creep('c1', 10, 10, ['move']), creep('c2', 20, 20, ['move']), { _id: 'r1', type: 'road', x: 21, y: 20, room: ROOM, hits: 5000, hitsMax: 5000, nextDecayTime: 5000 }], one('u1', { c1: { move: { direction: 3 } }, c2: { move: { direction: 3 } } }), { terrain: terrainOf([[11, 10], [21, 20]]) });
add('move fatigue swamp', ['intents/movement.js', 'creeps/_add-fatigue.js'], [creep('c1', 10, 10, ['work', 'work', 'move'])], one('u1', { c1: { move: { direction: 3 } } }), { terrain: terrainOf([], [[11, 10]]) });
add('move fatigued', ['intents/movement.js', 'creeps/_add-fatigue.js'], [creep('c1', 10, 10, ['work', 'move'], { fatigue: 4 })], one('u1', { c1: { move: { direction: 3 } } }));
add('edge exit', ['creeps/tick.js'], [creep('c1', 1, 10, ['move'])], one('u1', { c1: { move: { direction: 7 } } }));
add('stomp foreign site', ['intents/movement.js', 'intents/_create-energy.js'], [creep('c1', 10, 10, ['move']), { _id: 'cs1', type: 'constructionSite', x: 11, y: 10, room: ROOM, user: 'u2', structureType: 'road', progress: 101, progressTotal: 300 }], one('u1', { c1: { move: { direction: 3 } } }));
add('pull train', ['creeps/pull.js', 'intents/movement.js', 'creeps/_add-fatigue.js'], [creep('a', 10, 10, ['move']), creep('b', 9, 10, ['work'])], one('u1', { a: { move: { direction: 3, id: 'undefined' }, pull: { id: 'b' } }, b: { move: { id: 'a', direction: null } } }));
add('age death', ['creeps/tick.js', 'creeps/_die.js'], [creep('c1', 10, 10, ['move'], { ageTime: T0 + 1 })], []);
add('violent death twice', ['creeps/tick.js', 'creeps/_die.js'], [creep('c1', 10, 10, ['move'], { hits: 0, store: { energy: 5 } })], []);
add('spawning without spawn', ['creeps/tick.js'], [creep('c1', 10, 10, ['move'], { spawning: true, ageTime: undefined })], []);
add('portal', ['creeps/tick.js'], [creep('c1', 10, 10, ['move']), { _id: 'p1', type: 'portal', x: 11, y: 10, room: ROOM, destination: { room: 'W5N5', x: 20, y: 20 } }], one('u1', { c1: { move: { direction: 3 } } }));
add('drop onto container', ['creeps/drop.js', 'intents/_create-energy.js'], [creep('c1', 10, 10, ['carry', 'move'], { store: { energy: 40 } }), { _id: 'ct1', type: 'container', x: 10, y: 10, room: ROOM, store: { energy: 1980 }, storeCapacity: 2000, hits: 100, hitsMax: 250000 }], one('u1', { c1: { drop: { amount: 40, resourceType: 'energy' } } }));
add('drop onto pile', ['creeps/drop.js', 'intents/_create-energy.js'], [creep('c1', 10, 10, ['carry', 'move'], { store: { energy: 40 } }), { _id: 'd1', type: 'energy', x: 10, y: 10, room: ROOM, energy: 5, resourceType: 'energy' }], one('u1', { c1: { drop: { amount: 40, resourceType: 'energy' } } }));
add('manual intents replace', ['src/processor.js'], [creep('c1', 10, 10, ['move']), creep('c2', 13, 13, ['move'])], [['u1', { objects: { c1: { move: { direction: 3 } } }, objectsManual: { c1: { move: { direction: 5 } }, c2: { say: { message: 'hi', isPublic: false } } } }]]);
add('foreign object skipped', ['src/processor.js'], [creep('c1', 10, 10, ['move'], { user: 'u2' })], one('u1', { c1: { move: { direction: 3 } } }));
add('notifyWhenAttacked', ['src/processor.js'], [creep('c1', 10, 10, ['move']), { _id: 'r1', type: 'road', x: 5, y: 5, room: ROOM, hits: 5000, hitsMax: 5000 }], one('u1', { c1: { notifyWhenAttacked: { enabled: false } }, r1: { notifyWhenAttacked: { enabled: true } } }));
add('observer', ['src/processor.js'], [{ _id: 'ob1', type: 'observer', x: 5, y: 5, room: ROOM, user: 'u1', hits: 500, hitsMax: 500, observeRoom: 'W9N9' }, { _id: 'ob2', type: 'observer', x: 6, y: 5, room: ROOM, user: 'u1', hits: 500, hitsMax: 500 }], one('u1', { ob1: { observeRoom: { roomName: 'W2N2' } } }));
add('collapse timer and decay', ['src/processor.js'], [
  { _id: 'r1', type: 'road', x: 5, y: 5, room: ROOM, hits: 100, hitsMax: 5000, effects: [{ effect: C.EFFECT_COLLAPSE_TIMER, endTime: T0 }] },
  { _id: 'r2', type: 'road', x: 6, y: 5, room: ROOM, hits: 100, hitsMax: 5000, effects: { 0: { effect: C.EFFECT_COLLAPSE_TIMER, endTime: T0 + 1 } } },
  { _id: 'pb', type: 'powerBank', x: 7, y: 5, room: ROOM, hits: 100, hitsMax: 2000000, decayTime: T0 + 1 },
  { _id: 'dep', type: 'deposit', x: 8, y: 5, room: ROOM, depositType: 'mist', decayTime: T0 + 2 },
  { _id: 'dep2', type: 'deposit', x: 9, y: 5, room: ROOM, depositType: 'mist', decayTime: T0 + 400 },
], []);
add('map view kinds', ['src/processor.js'], [
  { _id: 'w', type: 'constructedWall', x: 1, y: 1, room: ROOM, hits: 1, hitsMax: 1 },
  { _id: 'nw', type: 'constructedWall', x: 2, y: 1, room: ROOM, hits: 1, hitsMax: 1, user: 'u1', newbieWall: true },
  { _id: 'ram', type: 'rampart', x: 3, y: 1, room: ROOM, user: 'u2', isPublic: true, hits: 1, hitsMax: 1 },
  { _id: 'ram2', type: 'rampart', x: 4, y: 1, room: ROOM, user: 'u2', hits: 1, hitsMax: 1 },
  { _id: 'tomb', type: 'tombstone', x: 5, y: 1, room: ROOM, user: 'u2', store: {}, deathTime: 1, decayTime: T0 + 5 },
  { _id: 'ruin', type: 'ruin', x: 6, y: 1, room: ROOM, user: 'u2', store: {}, destroyTime: 1, decayTime: T0 + 5 },
  { _id: 'cs', type: 'constructionSite', x: 7, y: 1, room: ROOM, user: 'u2', structureType: 'road', progress: 0, progressTotal: 300 },
  { _id: 'kl', type: 'keeperLair', x: 8, y: 1, room: ROOM },
  { _id: 'pw', type: 'energy', x: 9, y: 1, room: ROOM, resourceType: 'power', power: 10 },
  { _id: 'pt', type: 'portal', x: 10, y: 1, room: ROOM, destination: { room: 'W5N5', x: 1, y: 1 } },
  { _id: 'mn', type: 'mineral', x: 11, y: 1, room: ROOM, mineralType: 'H', mineralAmount: 5 },
  { _id: 'sh', type: 'extractor', x: 11, y: 1, room: ROOM, user: 'u1', hits: 500, hitsMax: 500 },
], []);
add('activation by reservation and core', ['src/processor.js'], [
  ctrl({ user: undefined, level: 0, reservation: { user: '2', endTime: T0 + 4000 } }),
  { _id: 'core', type: 'invaderCore', x: 20, y: 20, room: ROOM, level: 1, hits: 100000, hitsMax: 100000, deployTime: T0 + 50 },
], []);
// Keeper/invader creeps would run the NPC AI (another package); objects of user "3" and a controller of user "2" do not activate.
add('no activation for keepers', ['src/processor.js'], [ctrl({ user: '2', level: 1 }), { _id: 'kw', type: 'constructedWall', x: 12, y: 1, room: ROOM, user: '3', hits: 1, hitsMax: 1 }], []);
add('room info active', ['src/processor.js'], [{ _id: 's1', type: 'source', x: 11, y: 10, room: ROOM, energy: 3000, energyCapacity: 3000 }], [], { roomDoc: { _id: ROOM, status: 'normal', active: true } });

// `roomObjects` is a plain object: target ids naming Object.prototype members resolve to inherited values.
for (const key of ['__proto__', 'constructor', 'toString']) {
  add(`withdraw from ${key}`, ['creeps/withdraw.js', 'src/processor.js'], [creep('c1', 10, 10, ['carry', 'move'])], one('u1', { c1: { withdraw: { id: key, amount: 10, resourceType: 'energy' } } }));
}
{
  const PROTO_INTENTS = {
    transfer: { amount: 10, resourceType: 'energy' }, pickup: {}, harvest: {}, build: {}, repair: {}, dismantle: {}, attack: {}, rangedAttack: {},
    heal: {}, rangedHeal: {}, upgradeController: {}, claimController: {}, reserveController: {}, attackController: {}, signController: { sign: 'x' },
    generateSafeMode: {}, pull: {}, move: { direction: 3 },
  };
  const keys = ['__proto__', 'constructor', 'valueOf', 'hasOwnProperty'];
  const creeps = Object.keys(PROTO_INTENTS).map((a, i) => creep(`p${i}`, 5 + i, 20, ['work', 'carry', 'move', 'attack', 'ranged_attack', 'heal', 'claim'], { store: { energy: 50, G: 1000 }, storeCapacity: 1050 }));
  const objects = {};
  Object.keys(PROTO_INTENTS).forEach((a, i) => (objects[`p${i}`] = { [a]: Object.assign({ id: keys[i % keys.length] }, PROTO_INTENTS[a]) }));
  add('prototype-key targets', ['src/processor.js', 'creeps/intents.js'].concat(Object.keys(PROTO_INTENTS).map((a) => `creeps/${a}.js`)), creeps.concat([ctrl({ x: 30, y: 30 })]), one('u1', objects));
}

// Random rooms: creeps and structures close together, random M1 intents with random arguments.
const g = prng(0x0e09);
const ACTIONS = ['move', 'harvest', 'transfer', 'withdraw', 'pickup', 'drop', 'build', 'repair', 'upgradeController', 'say', 'suicide', 'pull'];
const RESOURCES = ['energy', 'energy', 'energy', 'H', 'O', 'XUHO2', 'power', 'bogus', 'ops'];
const BOOSTS = { work: ['UO', 'XUHO2', 'LH', 'XLH2O', 'GH', 'XGH2O', 'ZH'], carry: ['KH', 'XKH2O'], move: ['ZO', 'XZHO2'], tough: ['GO', 'XGHO2'] };
const PARTS = ['work', 'carry', 'move', 'move', 'tough', 'claim', 'attack', 'heal', 'ranged_attack'];
const near = () => 9 + g.int(5);
const maybe = (p, v) => (g.chance(p) ? v : undefined);

function randomStore(cap) {
  const s = {};
  for (let i = g.int(3); i > 0; i--) s[g.pick(RESOURCES.filter((r) => r !== 'bogus'))] = g.int(cap + 10);
  return s;
}

function randomCreep(i, T) {
  const parts = Array.from({ length: 1 + g.int(7) }, () => {
    const type = g.pick(PARTS);
    const p = { type, hits: g.chance(0.15) ? g.int(100) : 100 };
    if (BOOSTS[type] && g.chance(0.2)) p.boost = g.pick(BOOSTS[type]);
    return p;
  });
  const carry = parts.filter((p) => p.type === 'carry').length;
  const c = creep(`c${i}`, near(), near(), parts, {
    user: g.chance(0.7) ? 'u1' : 'u2',
    store: randomStore(carry * 50),
    storeCapacity: g.chance(0.9) ? carry * 50 : g.int(100),
    fatigue: g.chance(0.7) ? 0 : g.int(10),
    ageTime: g.chance(0.1) ? undefined : T + (g.chance(0.04) ? 1 : 2 + g.int(1500)),
    spawning: g.chance(0.05),
  });
  c.hits = parts.reduce((s, p) => s + p.hits, 0);
  if (g.chance(0.03)) c.hits = 0;
  if (g.chance(0.2)) c.actionLog = { say: null, harvest: { x: 1, y: 1 } };
  if (g.chance(0.05)) delete c.actionLog;
  if (g.chance(0.03)) c.tutorial = true;
  if (c.ageTime === undefined) delete c.ageTime;
  return c;
}

function randomObjects(T) {
  const objs = [];
  const id = (p) => `${p}${objs.length}`;
  const at = () => ({ x: near(), y: near(), room: ROOM });
  if (g.chance(0.7)) objs.push(Object.assign({ _id: 'ctrl', type: 'controller', level: g.int(9), user: g.pick(['u1', 'u2', undefined]), progress: g.int(300), downgradeTime: T + g.int(30000), safeMode: maybe(0.2, T + g.int(3) - 1), reservation: maybe(0.1, { user: 'u2', endTime: T + 100 }), upgradeBlocked: maybe(0.1, T + g.int(3) - 1), tutorial: maybe(0.05, true), effects: maybe(0.1, [{ power: C.PWR_OPERATE_CONTROLLER, level: 1 + g.int(5), endTime: T + g.int(3) - 1 }]) }, at()));
  for (let i = g.int(3); i > 0; i--) objs.push(Object.assign({ _id: id('s'), type: 'source', energy: g.int(4) * 10, energyCapacity: 3000, invaderHarvested: maybe(0.3, 5) }, at()));
  if (g.chance(0.4)) {
    const p = at();
    objs.push(Object.assign({ _id: id('m'), type: 'mineral', mineralType: 'H', mineralAmount: g.int(4) * 3, density: 1 }, p));
    if (g.chance(0.7)) objs.push(Object.assign({ _id: id('x'), type: 'extractor', user: g.pick(['u1', 'u2', undefined]), cooldown: g.chance(0.7) ? 0 : 3, hits: 500, hitsMax: 500 }, p));
  }
  if (g.chance(0.3)) objs.push(Object.assign({ _id: id('dep'), type: 'deposit', depositType: 'silicon', harvested: g.int(500), cooldownTime: maybe(0.5, T + g.int(3) - 1), decayTime: T + 5000 }, at()));
  for (let i = g.int(3); i > 0; i--) objs.push(Object.assign({ _id: id('ct'), type: 'container', store: randomStore(2000), storeCapacity: 2000, hits: g.chance(0.9) ? 1000 : 0, hitsMax: 250000, nextDecayTime: T + 50 }, at()));
  for (let i = g.int(4); i > 0; i--) objs.push(Object.assign({ _id: id('r'), type: 'road', hits: 1 + g.int(5000), hitsMax: 5000, nextDecayTime: T + 500 }, at()));
  for (let i = g.int(3); i > 0; i--) objs.push(Object.assign({ _id: id('cs'), type: 'constructionSite', user: g.pick(['u1', 'u2']), structureType: g.pick(['road', 'container', 'rampart', 'tower', 'constructedWall', 'storage']), progress: g.int(20), progressTotal: 1 + g.int(25) }, at()));
  for (let i = g.int(3); i > 0; i--) objs.push(Object.assign({ _id: id('d'), type: 'energy', resourceType: g.pick(['energy', 'H']), energy: 1 + g.int(60), H: 1 + g.int(60) }, at()));
  if (g.chance(0.3)) objs.push(Object.assign({ _id: id('st'), type: 'storage', user: g.pick(['u1', 'u2']), store: randomStore(1000), storeCapacity: 1000000, hits: 10000, hitsMax: 10000 }, at()));
  if (g.chance(0.3)) objs.push(Object.assign({ _id: id('lab'), type: 'lab', user: g.pick(['u1', 'u2']), store: { energy: g.int(100), H: maybe(0.5, g.int(50)) }, storeCapacity: 5000, storeCapacityResource: { energy: 2000, H: maybe(0.5, 3000) }, hits: 500, hitsMax: 500 }, at()));
  if (g.chance(0.2)) objs.push(Object.assign({ _id: id('term'), type: 'terminal', user: g.pick(['u1', 'u2']), store: randomStore(500), storeCapacity: 300000, hits: 3000, hitsMax: 3000, effects: maybe(0.5, [{ power: C.PWR_DISRUPT_TERMINAL, level: 1, endTime: T + g.int(3) - 1 }]) }, at()));
  for (let i = g.int(3); i > 0; i--) objs.push(Object.assign({ _id: id('ram'), type: 'rampart', user: g.pick(['u1', 'u2']), isPublic: g.chance(0.3), hits: 1000, hitsMax: 100000, nextDecayTime: T + 50 }, at()));
  if (g.chance(0.3)) objs.push(Object.assign({ _id: id('tomb'), type: 'tombstone', user: 'u2', store: randomStore(100), deathTime: T - 5, decayTime: T + 50, creepId: 'x', creepName: 'x', creepBody: ['move'] }, at()));
  if (g.chance(0.2)) objs.push(Object.assign({ _id: id('w'), type: 'constructedWall', hits: 1000, hitsMax: 300000000 }, at()));
  if (g.chance(0.1)) objs.push(Object.assign({ _id: id('tw'), type: 'tower', user: 'u1', store: { energy: 500 }, storeCapacityResource: { energy: 1000 }, hits: 3000, hitsMax: 3000 }, at()));
  for (const o of objs) for (const k of Object.keys(o)) if (o[k] === undefined) delete o[k];
  return objs;
}

// Fitting target types per action; a random id otherwise.
const TARGETS = {
  harvest: ['source', 'mineral', 'deposit'],
  transfer: ['container', 'storage', 'lab', 'terminal', 'tower', 'creep'],
  withdraw: ['container', 'storage', 'lab', 'terminal', 'tombstone', 'creep', 'tower'],
  pickup: ['energy'],
  build: ['constructionSite'],
  repair: ['road', 'rampart', 'container', 'constructedWall', 'storage'],
  upgradeController: ['controller'],
  pull: ['creep'],
  move: ['creep'],
};
const RANGE = { build: 3, repair: 3, upgradeController: 3 };

function randomIntent(name, c, docs) {
  const fits = docs.filter((o) => (TARGETS[name] || []).includes(o.type) && o !== c);
  const close = fits.filter((o) => Math.max(Math.abs(o.x - c.x), Math.abs(o.y - c.y)) <= (RANGE[name] || 1));
  const target = () => {
    const r = g.rand();
    if (r < 0.65 && close.length) return g.pick(close)._id;
    if (r < 0.85 && fits.length) return g.pick(fits)._id;
    return r < 0.95 ? g.pick(docs)._id : g.pick(['undefined', 'nope']);
  };
  const amount = () => g.pick([null, -5, 0, 1, 7, 25, 50, 50, 100, 1000]);
  const resource = () => (g.chance(0.6) ? g.pick(Object.keys(c.store).concat(['energy'])) : g.pick(RESOURCES));
  switch (name) {
    case 'move': return g.chance(0.8) ? { direction: g.pick([null, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), id: 'undefined' } : { id: target(), direction: null };
    case 'drop': return { amount: amount(), resourceType: resource() };
    case 'transfer': return { id: target(), amount: amount(), resourceType: resource() };
    case 'withdraw': return { id: target(), amount: amount(), resourceType: g.pick(RESOURCES) };
    case 'say': return { message: g.pick(['hi', 'a long message here', '', 'x'.repeat(12)]), isPublic: g.chance(0.5) };
    case 'suicide': return {};
    case 'build': case 'repair': return { id: target(), x: c.x, y: c.y };
    default: return { id: target() };
  }
}

for (let n = 0; n < 320; n++) {
  const T = T0 + g.int(4000);
  const objects = randomObjects(T);
  const creeps = Array.from({ length: 1 + g.int(6) }, (_, i) => randomCreep(i, T));
  // Creation order mixes creeps and structures (seeded Fisher-Yates).
  const docs = objects.concat(creeps);
  for (let i = docs.length - 1; i > 0; i--) {
    const j = g.int(i + 1);
    [docs[i], docs[j]] = [docs[j], docs[i]];
  }
  const ids = docs.map((o) => o._id);
  const byUser = { u1: {}, u2: {} };
  for (const c of creeps) {
    if (g.chance(0.15)) continue;
    const intents = {};
    for (const a of ACTIONS) if (g.chance(a === 'suicide' ? 0.03 : 0.35)) intents[a] = randomIntent(a, c, docs);
    if (g.chance(0.05)) intents.notifyWhenAttacked = { enabled: g.chance(0.5) };
    // A creep's intents may also be sent by the other user (skipped by the owner check).
    byUser[g.chance(0.9) ? c.user : g.pick(['u1', 'u2'])][c._id] = intents;
  }
  const order = g.chance(0.5) ? ['u1', 'u2'] : ['u2', 'u1'];
  const intents = order.filter((u) => Object.keys(byUser[u]).length).map((u) => [u, { objects: byUser[u], objectsManual: null }]);
  const walls = Array.from({ length: g.int(8) }, () => [near(), near()]);
  const swamps = Array.from({ length: g.int(12) }, () => [near(), near()]);
  const used = new Set(intents.flatMap(([, u]) => Object.values(u.objects).flatMap((i) => Object.keys(i))));
  const tags = ['src/processor.js', 'src/utils.js', 'intents/movement.js', 'intents/_create-energy.js', 'creeps/intents.js', 'creeps/tick.js', 'creeps/_add-fatigue.js']
    .concat(ACTIONS.filter((a) => used.has(a)).map((a) => `creeps/${a}.js`));
  SCENARIOS.push(scenario(`random ${n}`, tags, docs, intents, { gameTime: T, terrain: terrainOf(walls, swamps) }));
}

// Combat, healing and controller intents (own PRNG, so the rooms above stay as they are). Structures keep
// enough hits not to be destroyed: structures/_destroy belongs to another package and is not replayed here.
const COMBAT_TAGS = ['src/processor.js', 'src/utils.js', 'intents/_damage.js', 'creeps/_clear-newbie-walls.js', 'creeps/intents.js', 'creeps/tick.js', 'creeps/_die.js', 'creeps/_recalc-body.js', 'creeps/_drop-resources-without-space.js'];
add('attack with hit-back', COMBAT_TAGS.concat(['creeps/attack.js']), [creep('a', 10, 10, ['attack', 'attack', 'move']), creep('b', 11, 10, ['attack', 'tough', 'move'], { user: 'u2', notifyWhenAttacked: true })], one('u1', { a: { attack: { id: 'b', x: 11, y: 10 } } }));
add('attack from own rampart', COMBAT_TAGS.concat(['creeps/attack.js']), [creep('a', 10, 10, ['attack', 'move']), creep('b', 11, 10, ['attack', 'move'], { user: 'u2' }), { _id: 'ram', type: 'rampart', x: 10, y: 10, room: ROOM, user: 'u1', hits: 100000, hitsMax: 300000 }], one('u1', { a: { attack: { id: 'b', x: 11, y: 10 } } }));
add('attack redirected to rampart', COMBAT_TAGS.concat(['creeps/attack.js']), [creep('a', 10, 10, ['attack', 'move']), creep('b', 11, 10, ['move'], { user: 'u2' }), { _id: 'ram', type: 'rampart', x: 11, y: 10, room: ROOM, user: 'u2', hits: 100000, hitsMax: 300000, notifyWhenAttacked: true }], one('u1', { a: { attack: { id: 'b', x: 11, y: 10 } } }));
add('attack power bank', COMBAT_TAGS.concat(['creeps/attack.js']), [creep('a', 10, 10, ['attack', 'attack', 'attack', 'move']), { _id: 'pb', type: 'powerBank', x: 11, y: 10, room: ROOM, hits: 2000000, hitsMax: 2000000, power: 3000, decayTime: T0 + 4000 }], one('u1', { a: { attack: { id: 'pb', x: 11, y: 10 } } }));
add('attack newbie wall', COMBAT_TAGS.concat(['creeps/attack.js', 'creeps/_clear-newbie-walls.js']), [creep('a', 10, 10, ['attack', 'move']), { _id: 'nw1', type: 'constructedWall', x: 11, y: 10, room: ROOM, user: 'u2', hits: 1000000, hitsMax: 1000000, decayTime: { timestamp: 5 } }, { _id: 'nw2', type: 'constructedWall', x: 30, y: 30, room: ROOM, user: 'u2', hits: 1000000, hitsMax: 1000000, decayTime: { timestamp: 5 } }, { _id: 'w3', type: 'constructedWall', x: 31, y: 30, room: ROOM, hits: 1000000, hitsMax: 1000000 }], one('u1', { a: { attack: { id: 'nw1', x: 11, y: 10 } } }));
add('tough boosts and death', COMBAT_TAGS.concat(['creeps/rangedAttack.js', 'creeps/heal.js']), [creep('a', 10, 10, Array(6).fill({ type: 'ranged_attack', hits: 100, boost: 'XKHO2' }).concat(['move'])), creep('b', 12, 10, [{ type: 'tough', hits: 100, boost: 'XGHO2' }, { type: 'tough', hits: 60, boost: 'GO' }, 'carry', 'move'], { user: 'u2', store: { energy: 50 }, storeCapacity: 50 }), creep('h', 13, 10, ['heal', 'move'], { user: 'u2' })], [['u1', { objects: { a: { rangedAttack: { id: 'b' } } }, objectsManual: null }], ['u2', { objects: { h: { heal: { id: 'b' } } }, objectsManual: null }]]);
add('ranged mass attack', COMBAT_TAGS.concat(['creeps/rangedMassAttack.js']), [creep('a', 10, 10, ['ranged_attack', 'ranged_attack', 'move']), creep('b', 11, 10, ['move'], { user: 'u2' }), creep('c', 12, 12, ['move'], { user: 'u2' }), creep('d', 13, 10, ['move'], { user: 'u2' }), { _id: 'ram', type: 'rampart', x: 13, y: 10, room: ROOM, user: 'u1', hits: 100000, hitsMax: 300000 }, { _id: 'pb', type: 'powerBank', x: 10, y: 13, room: ROOM, hits: 2000000, hitsMax: 2000000, decayTime: T0 + 4000 }], one('u1', { a: { rangedMassAttack: {} } }));
add('dismantle', COMBAT_TAGS.concat(['creeps/dismantle.js']), [creep('a', 10, 10, ['work', 'work', 'carry', 'move'], { store: { energy: 49 }, storeCapacity: 50 }), { _id: 'r1', type: 'road', x: 11, y: 10, room: ROOM, hits: 50000, hitsMax: 50000 }], one('u1', { a: { dismantle: { id: 'r1' } } }));
add('dismantle NaN store', COMBAT_TAGS.concat(['creeps/dismantle.js']), [creep('a', 10, 10, ['work', 'move'], { store: {}, storeCapacity: 0 }), { _id: 'r1', type: 'road', x: 11, y: 10, room: ROOM, hits: 50000, hitsMax: 50000 }], one('u1', { a: { dismantle: { id: 'r1' } } }));
add('claim', COMBAT_TAGS.concat(['creeps/claimController.js']), [creep('a', 10, 10, ['claim', 'move']), ctrl({ level: 0, user: undefined, progress: undefined, downgradeTime: undefined, x: 11, y: 10 })], one('u1', { a: { claimController: { id: 'ctrl' } } }), { users: { u1: { _id: 'u1', username: 'u1', gcl: 1000001, rooms: ['W5N5'] }, u2: users.u2 } });
add('claim without gcl', COMBAT_TAGS.concat(['creeps/claimController.js']), [creep('a', 10, 10, ['claim', 'move']), ctrl({ level: 0, user: undefined, x: 11, y: 10 })], one('u1', { a: { claimController: { id: 'ctrl' } } }), { users: { u1: { _id: 'u1', username: 'u1', gcl: 999999, rooms: ['W5N5'] }, u2: users.u2 } });
add('reserve', COMBAT_TAGS.concat(['creeps/reserveController.js']), [creep('a', 10, 10, ['claim', 'claim', 'move']), ctrl({ level: 0, user: undefined, x: 11, y: 10 })], one('u1', { a: { reserveController: { id: 'ctrl' } } }));
add('attack controller', COMBAT_TAGS.concat(['creeps/attackController.js']), [creep('a', 10, 10, ['claim', 'claim', 'move']), ctrl({ level: 3, user: 'u2', x: 11, y: 10, downgradeTime: 50000 })], one('u1', { a: { attackController: { id: 'ctrl' } } }));
add('sign', COMBAT_TAGS.concat(['creeps/signController.js']), [creep('a', 10, 10, ['move']), ctrl({ x: 11, y: 10, sign: { user: 'u2', text: 'old', time: 1, datetime: 1 } })], one('u1', { a: { signController: { id: 'ctrl', sign: 'mine' } } }));
add('generate safe mode', COMBAT_TAGS.concat(['creeps/generateSafeMode.js']), [creep('a', 10, 10, ['carry', 'move'], { store: { G: 1000 }, storeCapacity: 1000 }), ctrl({ x: 11, y: 10 })], one('u1', { a: { generateSafeMode: { id: 'ctrl' } } }));

const gc = prng(0xc0b4);
const COMBAT = ['attack', 'rangedAttack', 'rangedMassAttack', 'heal', 'rangedHeal', 'dismantle', 'claimController', 'reserveController', 'attackController', 'signController', 'generateSafeMode', 'move', 'say', 'transfer'];
const COMBAT_TARGETS = {
  attack: ['creep', 'powerCreep', 'rampart', 'constructedWall', 'road', 'container', 'tower', 'powerBank'],
  rangedAttack: ['creep', 'powerCreep', 'rampart', 'constructedWall', 'road', 'container', 'tower', 'powerBank'],
  heal: ['creep', 'powerCreep'], rangedHeal: ['creep', 'powerCreep'],
  dismantle: ['rampart', 'constructedWall', 'road', 'container', 'tower', 'storage'],
  claimController: ['controller'], reserveController: ['controller'], attackController: ['controller'], signController: ['controller'], generateSafeMode: ['controller'],
  transfer: ['creep', 'container'], move: ['creep'],
};
const COMBAT_RANGE = { rangedAttack: 3, rangedHeal: 3 };
const cnear = () => 9 + gc.int(5);
const cmaybe = (p, v) => (gc.chance(p) ? v : undefined);
const CPARTS = ['attack', 'ranged_attack', 'heal', 'work', 'claim', 'tough', 'move', 'move', 'carry'];
const CBOOSTS = { attack: ['UH', 'XUH2O'], ranged_attack: ['KO', 'XKHO2'], heal: ['LO', 'XLHO2'], work: ['ZH', 'XZH2O'], tough: ['GO', 'GHO2', 'XGHO2'], move: ['ZO'], carry: ['KH'] };

function combatRoom(n) {
  const T = T0 + gc.int(4000);
  const objs = [];
  const id = (p) => `${p}${objs.length}`;
  const at = () => ({ x: cnear(), y: cnear(), room: ROOM });
  const effects = () => cmaybe(0.25, [gc.chance(0.5) ? { power: C.PWR_FORTIFY, level: 1, endTime: T + gc.int(3) - 1 } : { effect: C.EFFECT_INVULNERABILITY, endTime: T + gc.int(3) - 1 }]);
  if (gc.chance(0.8)) {
    const level = gc.int(4);
    objs.push(Object.assign({ _id: 'ctrl', type: 'controller', level, user: level ? gc.pick(['u1', 'u2']) : undefined, progress: 0, downgradeTime: T + 1000 + gc.int(30000), safeMode: cmaybe(0.15, T + gc.int(3) - 1), reservation: level ? undefined : cmaybe(0.3, { user: gc.pick(['u1', 'u2']), endTime: T + gc.int(5000) }), upgradeBlocked: cmaybe(0.1, T + gc.int(3) - 1), effects: cmaybe(0.1, [{ effect: C.EFFECT_INVULNERABILITY, endTime: T + gc.int(3) - 1 }]), sign: cmaybe(0.3, { user: 'u2', text: 'x', time: 1, datetime: 2 }), bindUser: cmaybe(0.05, 'u2'), safeModeAvailable: cmaybe(0.5, gc.int(3)) }, at()));
  }
  for (let i = gc.int(3); i > 0; i--) objs.push(Object.assign({ _id: id('ram'), type: 'rampart', user: gc.pick(['u1', 'u2']), isPublic: gc.chance(0.3), hits: 100000, hitsMax: 300000, effects: effects(), notifyWhenAttacked: cmaybe(0.3, true) }, at()));
  for (let i = gc.int(3); i > 0; i--) objs.push(Object.assign({ _id: id('w'), type: 'constructedWall', hits: 1000000, hitsMax: 300000000, effects: effects(), user: cmaybe(0.15, 'u2') }, at()));
  for (const o of objs) if (o.type === 'constructedWall' && o.user) o.decayTime = { timestamp: 1 };
  for (let i = gc.int(3); i > 0; i--) objs.push(Object.assign({ _id: id('r'), type: 'road', hits: 10000 + gc.int(40000), hitsMax: 50000, nextDecayTime: T + 500, notifyWhenAttacked: cmaybe(0.2, true) }, at()));
  for (let i = gc.int(2); i > 0; i--) objs.push(Object.assign({ _id: id('ct'), type: 'container', store: { energy: gc.int(100) }, storeCapacity: 2000, hits: 50000, hitsMax: 250000, nextDecayTime: T + 50 }, at()));
  if (gc.chance(0.3)) objs.push(Object.assign({ _id: id('tw'), type: 'tower', user: gc.pick(['u1', 'u2']), store: { energy: 500 }, storeCapacityResource: { energy: 1000 }, hits: 30000, hitsMax: 30000, notifyWhenAttacked: cmaybe(0.5, true), effects: effects() }, at()));
  if (gc.chance(0.2)) objs.push(Object.assign({ _id: id('st'), type: 'storage', user: gc.pick(['u1', 'u2']), store: { energy: 100 }, storeCapacity: 1000000, hits: 30000, hitsMax: 30000, effects: cmaybe(0.3, [{ power: C.PWR_SHIELD, level: 1, endTime: T + gc.int(3) - 1 }]) }, at()));
  if (gc.chance(0.2)) objs.push(Object.assign({ _id: id('pb'), type: 'powerBank', hits: 2000000, hitsMax: 2000000, power: 2000, decayTime: T + gc.pick([1, 2, 100, 600, 4000]) }, at()));
  for (let i = gc.int(2); i > 0; i--) objs.push(Object.assign({ _id: id('pc'), type: 'powerCreep', user: gc.pick(['u1', 'u2']), name: 'pc', hits: 300 + gc.int(700), hitsMax: 1000, store: {}, storeCapacity: 100, actionLog: { spawned: null }, ageTime: T + 3000, notifyWhenAttacked: cmaybe(0.3, true) }, at()));
  const creeps = [];
  for (let i = 1 + gc.int(6); i > 0; i--) {
    const parts = Array.from({ length: 1 + gc.int(7) }, () => {
      const type = gc.pick(CPARTS);
      const p = { type, hits: gc.chance(0.2) ? gc.int(100) : 100 };
      if (CBOOSTS[type] && gc.chance(0.25)) p.boost = gc.pick(CBOOSTS[type]);
      return p;
    });
    const carry = parts.filter((p) => p.type === 'carry').length;
    const store = gc.chance(0.15) ? {} : gc.chance(0.1) ? { G: 1000 + gc.int(10) } : { energy: gc.int(carry * 50 + 1) };
    const c = creep(`k${creeps.length}`, cnear(), cnear(), parts, { user: gc.chance(0.6) ? 'u1' : 'u2', store, storeCapacity: carry * 50, ageTime: T + 2 + gc.int(1500), notifyWhenAttacked: cmaybe(0.2, true), spawning: gc.chance(0.03) });
    c.hits = parts.reduce((s, p) => s + p.hits, 0);
    creeps.push(c);
  }
  const docs = objs.concat(creeps);
  for (const o of docs) for (const k of Object.keys(o)) if (o[k] === undefined) delete o[k];
  for (let i = docs.length - 1; i > 0; i--) {
    const j = gc.int(i + 1);
    [docs[i], docs[j]] = [docs[j], docs[i]];
  }
  const byUser = { u1: {}, u2: {} };
  for (const c of creeps) {
    if (gc.chance(0.1)) continue;
    const intents = {};
    for (const a of COMBAT) {
      if (!gc.chance(0.3)) continue;
      const fits = docs.filter((o) => (COMBAT_TARGETS[a] || []).includes(o.type) && (o !== c || gc.chance(0.2)));
      const close = fits.filter((o) => Math.max(Math.abs(o.x - c.x), Math.abs(o.y - c.y)) <= (COMBAT_RANGE[a] || 1));
      const r = gc.rand();
      const target = r < 0.7 && close.length ? gc.pick(close)._id : r < 0.9 && fits.length ? gc.pick(fits)._id : gc.pick(docs)._id;
      if (a === 'rangedMassAttack') intents[a] = {};
      else if (a === 'move') intents[a] = { direction: 1 + gc.int(8), id: 'undefined' };
      else if (a === 'say') intents[a] = { message: 'hey', isPublic: gc.chance(0.5) };
      else if (a === 'signController') intents[a] = { id: target, sign: gc.pick(['', 'hello']) };
      else if (a === 'transfer') intents[a] = { id: target, amount: gc.int(60), resourceType: 'energy' };
      else if (a === 'attack' || a === 'heal') intents[a] = { id: target, x: c.x, y: c.y };
      else intents[a] = { id: target };
    }
    byUser[c.user][c._id] = intents;
  }
  const order = gc.chance(0.5) ? ['u1', 'u2'] : ['u2', 'u1'];
  const intents = order.filter((u) => Object.keys(byUser[u]).length).map((u) => [u, { objects: byUser[u], objectsManual: null }]);
  const used = new Set(intents.flatMap(([, u]) => Object.values(u.objects).flatMap((i) => Object.keys(i))));
  const tags = COMBAT_TAGS.concat(COMBAT.filter((a) => used.has(a)).map((a) => `creeps/${a}.js`));
  const usersFor = {
    u1: { _id: 'u1', username: 'u1', gcl: gc.pick([0, 1000000, 5000000]), rooms: gc.pick([undefined, [], ['W1N1'], ['W5N5', 'W6N6']]), shardAccess: cmaybe(0.05, false) },
    u2: { _id: 'u2', username: 'u2', gcl: gc.pick([0, 5000000]), rooms: gc.pick([undefined, ['W5N5']]) },
  };
  for (const u of Object.values(usersFor)) for (const k of Object.keys(u)) if (u[k] === undefined) delete u[k];
  const walls = Array.from({ length: gc.int(5) }, () => [cnear(), cnear()]);
  SCENARIOS.push(scenario(`combat ${n}`, tags, docs, intents, { gameTime: T, users: usersFor, terrain: terrainOf(walls, []) }));
}
for (let n = 0; n < 220; n++) combatRoom(n);

// Movement stress (own PRNG): creeps packed into a few tiles, most moving, pull trains, follow-moves, swaps,
// ramparts/sites/roads/walls, safe mode, integer-like ids (for…in order) and creation-order shuffles.
const gm = prng(0x40e5);
const MOVE_TAGS = ['src/processor.js', 'intents/movement.js', 'creeps/move.js', 'creeps/pull.js', 'creeps/_add-fatigue.js', 'creeps/tick.js', 'creeps/intents.js'];
function movementRoom(n) {
  const T = T0 + gm.int(4000);
  const mnear = () => 20 + gm.int(5);
  const at = () => ({ x: mnear(), y: mnear(), room: ROOM });
  const objs = [];
  let seq = 0;
  const id = (p) => (seq++, gm.chance(0.15) ? String(1 + gm.int(40) + seq * 50) : `${p}${seq}`);
  if (gm.chance(0.6)) {
    objs.push(Object.assign({ _id: 'ctrl', type: 'controller', level: 3, user: gm.pick(['u1', 'u2']), progress: 0, downgradeTime: T + 20000, safeMode: gm.chance(0.4) ? T + 5 : undefined }, at()));
  }
  for (let i = gm.int(4); i > 0; i--) objs.push(Object.assign({ _id: id('r'), type: 'road', hits: 5000, hitsMax: 5000, nextDecayTime: T + 1000 }, at()));
  for (let i = gm.int(2); i > 0; i--) objs.push(Object.assign({ _id: id('w'), type: 'constructedWall', hits: 1000, hitsMax: 1000 }, at()));
  for (let i = gm.int(2); i > 0; i--) objs.push(Object.assign({ _id: id('ram'), type: 'rampart', user: gm.pick(['u1', 'u2']), isPublic: gm.chance(0.4), hits: 1000, hitsMax: 1000, nextDecayTime: T + 100 }, at()));
  for (let i = gm.int(3); i > 0; i--) objs.push(Object.assign({ _id: id('cs'), type: 'constructionSite', user: gm.pick(['u1', 'u2']), structureType: gm.pick(['road', 'spawn', 'rampart', 'container']), progress: gm.int(30), progressTotal: 300 }, at()));
  const creeps = [];
  const DIR = [[8, 1, 2], [7, 0, 3], [6, 5, 4]];
  const steps = new Map(); // creep -> planned direction
  const newCreep = (x, y) => {
    const parts = Array.from({ length: 1 + gm.int(5) }, () => ({ type: gm.pick(['move', 'move', 'work', 'carry', 'attack']), hits: gm.chance(0.1) ? 0 : 100 }));
    const carry = parts.filter((p) => p.type === 'carry').length;
    const c = creep(id('k'), x, y, parts, { user: gm.chance(0.75) ? 'u1' : 'u2', store: carry && gm.chance(0.6) ? { energy: gm.int(carry * 50 + 1) } : {}, storeCapacity: carry * 50, fatigue: gm.chance(0.8) ? 0 : 2 * gm.int(4), ageTime: T + 100 + gm.int(1000) });
    c.hits = parts.reduce((s, p) => s + p.hits, 0) || 1;
    creeps.push(c);
    return c;
  };
  // A contested tile: 2-5 neighbours step into it; sometimes its occupant steps out (maybe into a contender: a swap).
  const [tx, ty] = [mnear(), mnear()];
  const ring = [[-1, -1], [0, -1], [1, -1], [1, 0], [1, 1], [0, 1], [-1, 1], [-1, 0]];
  for (let i = ring.length - 1; i > 0; i--) {
    const j = gm.int(i + 1);
    [ring[i], ring[j]] = [ring[j], ring[i]];
  }
  const contenders = ring.slice(0, 2 + gm.int(4));
  for (const [dx, dy] of contenders) steps.set(newCreep(tx + dx, ty + dy), DIR[1 - dy][1 - dx]);
  if (gm.chance(0.4)) {
    const [dx, dy] = gm.chance(0.5) ? gm.pick(contenders) : gm.pick(ring);
    steps.set(newCreep(tx, ty), DIR[dy + 1][dx + 1]);
  }
  for (let i = gm.int(6); i > 0; i--) newCreep(mnear(), mnear());
  const docs = objs.concat(creeps);
  for (const o of docs) for (const k of Object.keys(o)) if (o[k] === undefined) delete o[k];
  for (let i = docs.length - 1; i > 0; i--) {
    const j = gm.int(i + 1);
    [docs[i], docs[j]] = [docs[j], docs[i]];
  }
  const byUser = { u1: {}, u2: {} };
  const adjacent = (a, b) => a !== b && Math.abs(a.x - b.x) <= 1 && Math.abs(a.y - b.y) <= 1;
  for (const c of creeps) {
    if (!steps.has(c) && gm.chance(0.1)) continue;
    const intents = {};
    const near = creeps.filter((o) => adjacent(o, c));
    const direction = steps.get(c) || 1 + gm.int(8);
    if (steps.has(c) || gm.chance(0.9)) intents.move = steps.has(c) || gm.chance(0.75) || !near.length ? { direction, id: 'undefined' } : { id: gm.pick(near)._id, direction: null };
    if (near.length && gm.chance(0.35)) intents.pull = { id: gm.pick(near)._id };
    byUser[gm.chance(0.95) ? c.user : gm.pick(['u1', 'u2'])][c._id] = intents;
  }
  const order = gm.chance(0.5) ? ['u1', 'u2'] : ['u2', 'u1'];
  const intents = order.filter((u) => Object.keys(byUser[u]).length).map((u) => [u, { objects: byUser[u], objectsManual: null }]);
  const walls = Array.from({ length: gm.int(3) }, () => [mnear(), mnear()]);
  const swamps = Array.from({ length: gm.int(5) }, () => [mnear(), mnear()]);
  SCENARIOS.push(scenario(`movement ${n}`, MOVE_TAGS, docs, intents, { gameTime: T, terrain: terrainOf(walls, swamps) }));
}
for (let n = 0; n < 250; n++) movementRoom(n);

// ---- run the official processor over every scenario ----
const results = [];
let index = -1;

function setup(s) {
  const objects = clone(s.objects);
  const userIds = new Set(objects.filter((o) => o.user).map((o) => String(o.user)));
  const usersById = {};
  for (const [id, u] of Object.entries(s.users)) if (userIds.has(id)) usersById[id] = clone(u);
  let intents = null;
  if (s.intents.length) {
    intents = { _id: 'i', room: ROOM, users: {} };
    for (const [uid, u] of s.intents) intents.users[uid] = clone(u);
  }
  return {
    scenario: s,
    input: { objects: driver.mapById(objects), users: usersById, intents, roomInfo: clone(s.roomDoc), flags: clone(s.flags) },
    ops: {}, stats: [], notifications: [], activated: false, crashed: null, mapView: null, eventLog: null, roomInfo: null,
  };
}

function done() {
  const s = cur.scenario;
  const docs = clone(s.objects);
  const flags = clone(s.flags);
  const opsObjects = cur.ops['rooms.objects'] || [];
  const opsFlags = cur.ops['rooms.flags'] || [];
  const err = dbBulk(docs, clone(opsObjects)) || dbBulk(flags, clone(opsFlags));
  if (err) throw new Error(`${s.name}: dbBulk aborted: ${err}`);
  results.push(Object.assign({}, s, {
    result: {
      ops: { objects: opsObjects, users: cur.ops.users || [], flags: opsFlags },
      docsAfter: docs, flagsAfter: flags, roomInfo: cur.roomInfo, eventLog: cur.eventLog, mapView: cur.mapView,
      activated: cur.activated, notifications: cur.notifications, stats: cur.stats, crashed: cur.crashed,
    },
  }));
}

fetchNext = () => {
  index++;
  if (index >= SCENARIOS.length) {
    const out = path.join(__dirname, 'cases.ndjson');
    fs.writeFileSync(out, results.map((r) => JSON.stringify(r)).join('\n') + '\n');
    log(`wrote ${out} (${results.length} cases, ${results.filter((r) => r.result.crashed).length} crashed)`);
    process.exit(0);
  }
  cur = setup(SCENARIOS[index]);
  stream = rng.roomStream(1n, SCENARIOS[index].gameTime, ROOM);
  // CLK-SIGN and every other processor clock read: vclock(T) with {baseMs: 0, baseTick: 0, nominalTickMs: 1000}.
  Date.now = () => SCENARIOS[index].gameTime * 1000;
  return Promise.resolve(ROOM);
};

// ---- utils.js, called directly ----
function utilsCases() {
  const utils = require(path.join(ENGINE, 'utils.js'));
  utils.getDriver(); // loads C, as every engine module does at require time
  const h = prng(0x07115);
  const enc = (v) => (v === undefined ? { $u: 1 } : typeof v === 'number' && (!Number.isFinite(v) || Object.is(v, -0)) ? { $n: Object.is(v, -0) ? '-0' : String(v) } : Array.isArray(v) ? v.map(enc) : v);
  const out = [];
  let worldSize = 4;
  driver.getWorldSize = () => worldSize;
  const call = (fn, args, f, extra = {}) => {
    let r;
    try {
      r = { result: enc(f()) };
    } catch (e) {
      r = { throws: e.message };
    }
    out.push(Object.assign({ fn, args }, extra, r));
  };
  const nums = [-3, -2, -1, 0, 1, 2, 3, 0.5, -0.5, 7];
  for (const dx of nums) for (const dy of nums) call('getDirection', [dx, dy], () => utils.getDirection(dx, dy));
  for (const d of [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, '3', null, 1.5, -1, '08']) call('getOffsetsByDirection', [d], () => utils.getOffsetsByDirection(d));
  const PARTS = ['move', 'work', 'carry', 'attack', 'ranged_attack', 'heal', 'tough', 'claim'];
  for (let i = 0; i < 40; i++) {
    const body = Array.from({ length: h.int(6) }, () => (h.chance(0.5) ? h.pick(PARTS) : { type: h.chance(0.95) ? h.pick(PARTS) : 'wings' }));
    call('calcCreepCost', [body], () => utils.calcCreepCost(body));
  }
  const TYPES = ['spawn', 'extension', 'road', 'constructedWall', 'rampart', 'link', 'storage', 'tower', 'observer', 'powerSpawn', 'extractor', 'lab', 'terminal', 'container', 'nuker', 'factory'];
  const COORD = () => h.pick([0, 1, 2, 47, 48, 49, h.int(50), h.int(50)]);
  for (let i = 0; i < 150; i++) {
    const t = Array(2500).fill('0');
    for (let k = h.int(120); k > 0; k--) t[h.pick([0, 49, h.int(50)]) * 50 + h.int(50)] = h.pick(['1', '2', '3']);
    for (let k = h.int(120); k > 0; k--) t[h.int(50) * 50 + h.pick([0, 49, h.int(50)])] = h.pick(['1', '2', '3']);
    const terrain = t.join('');
    const type = h.pick(TYPES), x = COORD(), y = COORD();
    call('checkConstructionSite/terrain', [terrain, type, x, y], () => utils.checkConstructionSite(terrain, type, x, y));
  }
  const objType = () => h.pick(TYPES.concat(['constructionSite', 'creep', 'source', 'mineral', 'controller']));
  const randomObjects = (n) => {
    const objs = {};
    for (let k = 0; k < n; k++) {
      const o = { _id: 'o' + k, type: objType(), x: 20 + h.int(4), y: 20 + h.int(4), user: h.pick(['u1', 'u1', 'u2', undefined]) };
      if (o.type === 'constructionSite') o.structureType = h.pick(TYPES);
      if (o.user === undefined) delete o.user;
      objs[o._id] = o;
    }
    return objs;
  };
  for (let i = 0; i < 150; i++) {
    const objects = randomObjects(h.int(6));
    const type = h.pick(TYPES), x = h.pick([20, 21, 22, 23, 0, 49]), y = h.pick([20, 21, 22, 23]);
    call('checkConstructionSite/objects', [objects, type, x, y], () => utils.checkConstructionSite(objects, type, x, y));
  }
  for (let i = 0; i < 150; i++) {
    const objects = randomObjects(h.int(12));
    const controller = h.chance(0.8) ? { level: h.int(9), user: h.pick(['u1', 'u2', undefined]), x: 21, y: 21 } : h.pick([undefined, 3, 0]);
    if (controller && controller.user === undefined) delete controller.user;
    const type = h.pick(TYPES);
    const offset = h.pick([undefined, 0, 1, -1]);
    call('checkControllerAvailability', [type, objects, controller === undefined ? null : controller, offset === undefined ? null : offset], () => utils.checkControllerAvailability(type, objects, controller, offset));
    const ids = Object.keys(objects);
    if (ids.length) {
      const id = h.pick(ids);
      const rc = h.chance(0.9) && controller && typeof controller === 'object' ? controller : undefined;
      call('checkStructureAgainstController', [id, objects, rc === undefined ? null : rc], () => utils.checkStructureAgainstController(objects[id], objects, rc));
    }
  }
  const RES = ['energy', 'power', 'H', 'O', 'XGH2O', 'ops'];
  for (let i = 0; i < 80; i++) {
    const o = {};
    if (h.chance(0.6)) {
      o.store = {};
      for (let k = h.int(4); k > 0; k--) o.store[h.pick(RES)] = h.pick([0, 1, 50, null, 'x', 3.5]);
    } else {
      for (let k = h.int(3); k > 0; k--) o[h.pick(RES)] = h.pick([0, 5, { amount: 3 }, null, 'x']);
    }
    if (h.chance(0.5)) o.storeCapacity = h.pick([0, 50, 2000, null]);
    if (h.chance(0.5)) o.storeCapacityResource = h.chance(0.8) ? { energy: h.pick([0, 300, 2000]), H: h.pick([undefined, 3000]) } : {};
    if (o.storeCapacityResource && o.storeCapacityResource.H === undefined) delete o.storeCapacityResource.H;
    call('calcResources', [o], () => utils.calcResources(o));
    const rt = h.pick(RES);
    call('capacityForResource', [o, rt], () => utils.capacityForResource(o, rt));
  }
  const BOOST = { work: ['UO', 'LH', 'GH', 'ZH', 'XUHO2', 'KH'], move: ['ZO', 'XZHO2'], carry: ['KH', 'XKH2O'], tough: ['GO', 'XGHO2'], attack: ['UH'], heal: ['LO'], ranged_attack: ['KO'], claim: [] };
  const METHOD = { work: ['harvest', 'build', 'repair', 'dismantle', 'upgradeController'], move: ['fatigue'], carry: ['capacity'], tough: ['damage'], attack: ['attack'], heal: ['heal', 'rangedHeal'], ranged_attack: ['rangedAttack', 'rangedMassAttack'], claim: ['x'] };
  for (let i = 0; i < 120; i++) {
    const body = Array.from({ length: h.int(8) }, () => {
      const type = h.pick(Object.keys(BOOST));
      const p = { type, hits: h.pick([0, 100, 50, 100]) };
      if (h.chance(0.3) && BOOST[type].length) p.boost = h.pick(BOOST[type]);
      if (h.chance(0.05)) p._oldHits = 100;
      return p;
    });
    const part = h.pick(Object.keys(BOOST));
    const method = h.pick(METHOD[part]);
    const without = h.chance(0.5);
    call('calcBodyEffectiveness', [body, part, method, 2, without], () => utils.calcBodyEffectiveness(body, part, method, 2, without));
  }
  // Not arrays: V8's error text names the callee expression.
  for (const body of [{}, 'abc', 5, null]) {
    call('calcBodyEffectiveness', [body, 'move', 'fatigue', 1, false], () => utils.calcBodyEffectiveness(body, 'move', 'fatigue', 1, false));
    call('calcCreepCost', [body], () => utils.calcCreepCost(body));
  }
  for (let i = 0; i < 40; i++) {
    const a = { x: h.int(50), y: h.int(50) }, b = h.chance(0.5) ? { x: h.int(50), y: h.int(50) } : { pos: { x: h.int(50), y: h.int(50) } };
    call('dist', [a, b], () => utils.dist(a, b));
    const t = { x: h.int(50), y: h.int(50) };
    call('comparatorDistance', [t, a, b], () => utils.comparatorDistance(t)(a, b));
    const e = h.chance(0.5) ? { x: h.pick([0, 1, 48, 49, 25]), y: h.pick([0, 25, 49]) } : { pos: { x: h.pick([0, 25]), y: 25 } };
    call('isAtEdge', [e], () => utils.isAtEdge(e));
  }
  const ROOMS = ['W1N1', 'E0S0', 'W0N0', 'E10S5', 'W127N127', 'E5N105', 'w3n3', 'W9N10', 'E100S100'];
  for (let i = 0; i < 60; i++) {
    const r1 = h.pick(ROOMS), r2 = h.pick(ROOMS), cont = h.chance(0.5);
    worldSize = h.pick([4, 12, 202, 256]);
    call('calcRoomsDistance', [r1, r2, cont], () => utils.calcRoomsDistance(r1, r2, cont), { worldSize });
    call('roomNameToXY', [r1], () => utils.roomNameToXY(r1));
    const x = h.int(300) - 150, y = h.int(300) - 150;
    call('getRoomNameFromXY', [x, y], () => utils.getRoomNameFromXY(x, y));
  }
  for (let i = 0; i < 300; i++) {
    const amount = h.pick([0, 100, 1000, h.int(1000000), h.int(300000) + 0.5]), range = h.pick([0, 1, 10, h.int(150), h.int(150)]);
    call('calcTerminalEnergyCost', [amount, range], () => utils.calcTerminalEnergyCost(amount, range));
  }
  // V8's Math.pow / Math.exp themselves (fdlibm ports on the Rust side).
  const special = [0, -0, 1, -1, 0.5, -0.5, 2, -2, 3, 1.2, 2.4, 1e-310, 1e300, -1e300, Infinity, -Infinity, NaN];
  for (const a of special) for (const b of special) call('Math.pow', [enc(a), enc(b)], () => Math.pow(a, b));
  for (let i = 0; i < 3000; i++) {
    const a = h.pick([h.rand() * 10, h.rand() * 1e6, h.int(100000), h.rand(), -h.rand() * 10, -h.int(20), h.rand() * 1e-5]);
    const b = h.pick([1.2, 2.4, 0.5, 2, 3, h.rand() * 4 - 2, h.int(40) - 20, h.rand() * 300 - 150, 1 / 3]);
    call('Math.pow', [enc(a), enc(b)], () => Math.pow(a, b));
  }
  for (const a of special) call('Math.exp', [enc(a)], () => Math.exp(a));
  for (let i = 0; i < 2000; i++) {
    const a = h.pick([h.rand() * 20 - 10, -h.rand() * 5, h.rand() * 1400 - 700, h.rand() * 1e-8, -h.int(150) / 30]);
    call('Math.exp', [enc(a)], () => Math.exp(a));
  }
  for (let l = 0; l < 60; l++) call('calcNeededGcl', [l], () => utils.calcNeededGcl(l));
  for (let i = 0; i < 40; i++) {
    const l = 1 + h.rand() * 40;
    call('calcNeededGcl', [l], () => utils.calcNeededGcl(l));
  }
  for (const m of Object.keys(C.REACTION_TIME).concat(['energy', 'H', 'nope'])) {
    call('calcTotalReactionsTime', [m], () => utils.calcTotalReactionsTime(m));
    call('getReactionVariants', [m], () => utils.getReactionVariants(m));
  }
  for (let i = 0; i < 60; i++) {
    const densities = {};
    for (const r of ['H', 'O', 'XUH2O', 'XGHO2', 'power', 'ops'].slice(0, 1 + h.int(6))) densities[r] = h.pick([1, 2, 5, 10, 100]);
    const target = h.pick([10, 100, 1000, 12345]), limit = h.pick([undefined, 1, 2, 3]);
    const seedHex = ((BigInt(h.int(2 ** 32)) << 32n) | BigInt(h.int(2 ** 32))).toString(16).padStart(16, '0');
    stream = rng.stream(BigInt('0x' + seedHex));
    call('calcReward', [densities, target, limit === undefined ? null : limit], () => utils.calcReward(densities, target, limit), { seed: seedHex });
  }
  for (let i = 0; i < 40; i++) {
    const target = { _id: 't1', type: h.pick(['creep', 'spawn', 'tower', 'road']), name: h.pick(['Bob', undefined]), room: 'W1N1', user: h.pick(['u1', 'u2', undefined]) };
    for (const k of Object.keys(target)) if (target[k] === undefined) delete target[k];
    const controller = h.chance(0.7) ? { user: h.pick(['u1', 'u2', undefined]) } : undefined;
    if (controller && controller.user === undefined) delete controller.user;
    const sent = [];
    const send = driver.sendNotification;
    driver.sendNotification = (u, m) => sent.push([u, m]);
    call('sendAttackingNotification', [target, controller === undefined ? null : controller], () => (utils.sendAttackingNotification(target, controller), sent));
    driver.sendNotification = send;
  }
  const outFile = path.join(__dirname, 'utils-cases.ndjson');
  fs.writeFileSync(outFile, out.map((r) => JSON.stringify(r)).join('\n') + '\n');
  log('wrote ' + outFile + ' (' + out.length + ' cases)');
}
utilsCases();

require(path.join(ENGINE, 'processor.js'));
