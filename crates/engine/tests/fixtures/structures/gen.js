// Generates crates/engine/tests/fixtures/structures/cases.ndjson: hand-written room scenarios run
// through the REAL vanilla handler files owned by WP10 (reference/engine/src/processor/intents/**)
// with a fake driver (constants, notifications, addRoomToUser/removeRoomFromUser copied from
// driver/lib/index.js:542-552), the real driver/lib/bulk.js writers (execute() captured as the
// storage RPC sees it) and the real movement.js module state (init'ed as processor.js:225 does).
// Math.random is the room stream of tools/oracle/lib/rng.js, Date.now the scenario's vnow.
// A scenario is a list of handler calls on one scope; roomObjects keys follow the objects' order.
// Recorded per case: crash message (vanilla would discard the room), each bulk's ops, the live
// roomObjects / flags / users / roomInfo (graph codec), eventLog, notifications, stats.inc calls.
// Handler-level until WP03's processor oracle lands; the replay is
// crates/engine/src/processor/intents/_calc_spawns.rs (mod oracle).
// Usage: <node 22> crates/engine/tests/fixtures/structures/gen.js
'use strict';
const Module = require('module');
const path = require('path');
const ROOT = path.join(__dirname, '..', '..', '..', '..', '..');
const { reference, writeFixture, encode } = require(path.join(ROOT, 'tools', 'oracle', 'lib', 'common'));
const { roomStream } = require(path.join(ROOT, 'tools', 'oracle', 'lib', 'rng'));

const LODASH = reference('lodash-3.10.1/lodash.js');
const C = require(reference('common/lib/constants.js'));
let captured, notifications, worldSize;
const fakeDriver = {
  constants: C,
  config: { emit() {} },
  sendNotification(userId, message) {
    notifications.push([userId, message]);
  },
  addRoomToUser(roomId, user, bulk) {
    if (!user.rooms || user.rooms.indexOf(roomId) == -1) bulk.addToSet(user, 'rooms', roomId);
  },
  removeRoomFromUser(roomId, user, bulk) {
    if (user.rooms && user.rooms.indexOf(roomId) != -1) bulk.pull(user, 'rooms', roomId);
  },
  getWorldSize: () => worldSize,
};
const stubs = {
  q: { when: (x) => Promise.resolve(x) },
  '@screeps/common': {
    storage: {
      db: new Proxy({}, {
        get: (_t, coll) => ({
          bulk(ops) {
            captured[coll] = JSON.parse(JSON.stringify(ops)); // the storage RPC
            return Promise.resolve();
          },
        }),
      }),
    },
  },
  'chitin-fake-driver': fakeDriver,
};
const resolve = Module._resolveFilename;
Module._resolveFilename = function (request, ...rest) {
  if (request === 'lodash') return LODASH;
  if (request in stubs) return `chitin-stub:${request}`;
  return resolve.call(this, request, ...rest);
};
for (const [k, v] of Object.entries(stubs)) require.cache[`chitin-stub:${k}`] = { id: k, filename: k, loaded: true, exports: v, children: [] };
process.env.DRIVER_MODULE = 'chitin-fake-driver';

const _ = require(LODASH);
const makeBulk = require(reference('driver/lib/bulk.js'));
const handler = (name) => require(reference(`engine/src/processor/intents/${name}.js`));
const movement = handler('movement');

// ---- scenario builders ----
const ROOM = 'W1N1';
const plain = () => '0'.repeat(2500);
function terrain({ walls = [], swamps = [] } = {}) {
  const t = plain().split('');
  for (const [x, y] of walls) t[y * 50 + x] = String(Number(t[y * 50 + x]) | 1);
  for (const [x, y] of swamps) t[y * 50 + x] = String(Number(t[y * 50 + x]) | 2);
  return t.join('');
}
const base = (o) =>
  Object.assign(
    {
      worldSeed: '0x1',
      gameTime: 1000,
      room: ROOM,
      vnow: 1700000000000,
      worldSize: 12,
      terrain: plain(),
      roomInfo: { _id: ROOM, status: 'normal' },
      users: [{ _id: 'u1', username: 'alice', rooms: [ROOM] }, { _id: 'u2', username: 'bob', rooms: [] }],
      flags: [],
      energyAvailable: 0,
      objects: [],
      calls: [],
    },
    o,
  );
const ctrl = (o) => Object.assign({ _id: 'ctrl', type: 'controller', x: 25, y: 25, room: ROOM, level: 2, user: 'u1', progress: 0, downgradeTime: 20000, safeModeAvailable: 1 }, o);
const spawn = (id, x, y, o) =>
  Object.assign({ _id: id, type: 'spawn', name: `Spawn${id}`, x, y, room: ROOM, user: 'u1', hits: 5000, hitsMax: 5000, store: { energy: 300 }, storeCapacityResource: { energy: 300 }, notifyWhenAttacked: true, spawning: null }, o);
const ext = (id, x, y, o) => Object.assign({ _id: id, type: 'extension', x, y, room: ROOM, user: 'u1', store: { energy: 50 }, storeCapacityResource: { energy: 50 }, hits: 1000, hitsMax: 1000 }, o);
const creep = (id, x, y, o) =>
  Object.assign(
    {
      _id: id, type: 'creep', name: `c${id}`, x, y, room: ROOM, user: 'u1',
      body: [{ type: 'move', hits: 100 }, { type: 'carry', hits: 100 }],
      store: { energy: 0 }, storeCapacity: 50, hits: 200, hitsMax: 200, ageTime: 2000, fatigue: 0,
      actionLog: { attacked: null, healed: null },
    },
    o,
  );
const obj = (id, type, x, y, o) => Object.assign({ _id: id, type, x, y, room: ROOM }, o);
const tick = (object) => ({ fn: `${object.split(':')[0]}/tick`, object: object.split(':')[1] });

const cases = [];
const add = (name, o) => cases.push(Object.assign(base(o), { name }));

// ---- _calc_spawns ----
add('calc_spawns: RCL1 keeps the closest spawn, extensions off at RCL1', {
  objects: [ctrl({ level: 1 }), spawn('s1', 30, 30), spawn('s2', 26, 26, { off: false }), ext('e1', 20, 20), ext('e2', 21, 21, { off: true })],
  calls: [{ fn: '_calc_spawns' }],
});
add('calc_spawns: distance ties keep key order', {
  objects: [ctrl({ level: 2 }), ...[1, 2, 3, 4, 5, 6, 7].map((i) => ext(`e${i}`, 25 + (i % 2 ? 3 : -3), 25 + (i > 4 ? 1 : -1))), spawn('s1', 30, 25)],
  calls: [{ fn: '_calc_spawns' }],
});
add('calc_spawns: under the limit clears off', {
  objects: [ctrl({ level: 8 }), spawn('s1', 30, 30, { off: true }), ext('e1', 20, 20, { off: true }), ext('e2', 21, 21, { off: false })],
  calls: [{ fn: '_calc_spawns' }],
});
add('calc_spawns: no controller throws', {
  objects: [spawn('s1', 30, 30)],
  calls: [{ fn: '_calc_spawns' }],
});
add('calc_spawns: unowned controller level 0', {
  objects: [ctrl({ level: 0, user: null }), spawn('s1', 30, 30), ext('e1', 20, 20)],
  calls: [{ fn: '_calc_spawns' }],
});

// ---- spawns: createCreep (through spawns/intents) ----
const create = (intent, more) => ({ fn: 'spawns/intents', object: 's1', intent: Object.assign({ createCreep: intent }, more) });
const room1 = (extra = [], ctrlOver = {}) => [ctrl(ctrlOver), spawn('s1', 25, 30), ...extra];
add('createCreep: old charging drains spawns then extensions', {
  objects: room1([spawn('s2', 20, 30, { store: { energy: 100 } }), ext('e1', 25, 35), ext('e2', 25, 32, { store: { energy: 20 } })], { level: 8 }),
  calls: [create({ name: 'Bob', body: ['work', 'work', 'work', 'work', 'move'], energyStructures: undefined, directions: undefined })],
});
add('createCreep: not enough energy', {
  objects: room1([], { level: 8 }),
  calls: [create({ name: 'Bob', body: ['claim'] })],
});
add('createCreep: energyStructures list order, duplicates, invalid and off ids', {
  objects: room1([ext('e1', 25, 35), ext('e2', 25, 32, { off: true }), ext('e3', 25, 33), ext('e4', 25, 34, { user: 'u2' })], { level: 8 }),
  calls: [create({ name: 'Bob', body: ['carry', 'move', 'carry'], energyStructures: ['e3', 'nope', 'e2', 'e4', 'e3', 'e1', 's1'] })],
});
add('createCreep: energyStructures cannot pay', {
  objects: room1([ext('e1', 25, 35)], { level: 8 }),
  calls: [create({ name: 'Bob', body: ['work', 'work'], energyStructures: ['e1'] })],
});
add('createCreep: directions coerced, deduplicated, kept', {
  objects: room1(),
  calls: [create({ name: 'Bob', body: ['move'], directions: ['3', 3, 1, '1'] })],
});
add('createCreep: invalid direction rejects', {
  objects: room1(),
  calls: [create({ name: 'Bob', body: ['move'], directions: [1, 9] })],
});
add('createCreep: empty directions pass', {
  objects: room1(),
  calls: [create({ name: 'Bob', body: ['move'], directions: [] })],
});
add('createCreep: already spawning', {
  objects: room1().map((o) => (o._id === 's1' ? Object.assign(o, { spawning: { name: 'X', needTime: 3, spawnTime: 1002 } }) : o)),
  calls: [create({ name: 'Bob', body: ['move'] })],
});
add('createCreep: second spawn inactive at RCL1', {
  objects: [ctrl({ level: 1 }), spawn('s0', 25, 26), spawn('s1', 25, 40)],
  calls: [create({ name: 'Bob', body: ['move'] })],
});
add('createCreep: operate spawn shortens, body cut at 50', {
  objects: room1([], { level: 8 }).map((o) =>
    o._id === 's1' ? Object.assign(o, { store: { energy: 3000 }, effects: [{ power: C.PWR_OPERATE_SPAWN, level: 2, endTime: 1001 }] }) : o,
  ),
  calls: [create({ name: 'Bob', body: Array(55).fill('move') })],
});
add('createCreep: tutorial spawn, then renewCreep is rejected the same tick', {
  objects: room1([creep('c1', 25, 31, { ageTime: 1100 })]).map((o) => (o._id === 's1' ? Object.assign(o, { tutorial: true }) : o)),
  calls: [create({ name: 'Bob', body: ['carry', 'move'] }, { renewCreep: { id: 'c1' } })],
});

// ---- spawns: tick ----
const spawning = (o) => ({ name: 'Bob', needTime: 9, spawnTime: 1001, ...o });
add('spawn tick: born on the first free direction, spawning cleared', {
  terrain: terrain({ walls: [[25, 29]] }),
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning() }), obj('w', 'constructedWall', 26, 29), creep('bob', 25, 30, { name: 'Bob', spawning: true })],
  calls: [tick('spawns:s1')],
});
add('spawn tick: a road makes a wall tile free, obstacle sites block', {
  terrain: terrain({ walls: [[25, 29]] }),
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning({ directions: [8, 1, 2] }) }), obj('cs', 'constructionSite', 24, 29, { structureType: 'tower', user: 'u2' }), obj('r', 'road', 25, 29), creep('bob', 25, 30, { name: 'Bob', spawning: true })],
  calls: [tick('spawns:s1')],
});
add('spawn tick: surrounded, retries next tick', {
  terrain: terrain({ walls: [[24, 29], [25, 29], [26, 29], [24, 30], [26, 30], [24, 31], [25, 31]] }),
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning() }), ext('e1', 26, 31), creep('bob', 25, 30, { name: 'Bob', spawning: true })],
  calls: [tick('spawns:s1')],
});
add('spawn tick: not yet', {
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning({ spawnTime: 1005 }) }), creep('bob', 25, 30, { name: 'Bob', spawning: true })],
  calls: [tick('spawns:s1')],
});
add('spawn tick: disrupted spawn delays', {
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning(), effects: [{ power: C.PWR_DISRUPT_SPAWN, level: 1, endTime: 1001 }] }), creep('bob', 25, 30, { name: 'Bob', spawning: true })],
  calls: [tick('spawns:s1')],
});
add('spawn tick: missing creep still clears spawning', {
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning() })],
  calls: [tick('spawns:s1')],
});
add('spawn tick: empty directions keep the creep on the spawn tile', {
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning({ directions: [] }) }), creep('bob', 25, 30, { name: 'Bob', spawning: true })],
  calls: [tick('spawns:s1')],
});
add('spawn tick: surrounded with a hostile, an unchosen direction is free', {
  terrain: terrain({ walls: [[24, 29], [25, 29], [26, 29], [24, 30]] }),
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning({ directions: [3, 4, 5] }) }), creep('h1', 26, 30, { user: 'u2' }), ext('e1', 26, 31), ext('e2', 25, 31), creep('bob', 25, 30, { name: 'Bob', spawning: true })],
  calls: [tick('spawns:s1')],
});
add('spawn tick: surrounded with a hostile, spawn stomp', {
  terrain: terrain({ walls: [[24, 29], [25, 29], [26, 29], [24, 30], [24, 31]] }),
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning({ directions: [3, 4, 5] }) }), creep('h1', 26, 30, { user: 'u2' }), ext('e1', 26, 31), ext('e2', 25, 31), creep('bob', 25, 30, { name: 'Bob', spawning: true })],
  calls: [tick('spawns:s1')],
});
add('spawn tick: regeneration below 300', {
  energyAvailable: 299,
  objects: [ctrl(), spawn('s1', 25, 30, { store: { energy: 120 } })],
  calls: [tick('spawns:s1')],
});
add('spawn tick: no regeneration at energyAvailable 300 or for tutorial', {
  energyAvailable: 300,
  objects: [ctrl(), spawn('s1', 25, 30, { store: { energy: 120 } }), spawn('s2', 25, 20, { store: { energy: 10 }, tutorial: true })],
  calls: [tick('spawns:s1'), tick('spawns:s2')],
});
add('spawn tick: only the allowed closest spawns regenerate', {
  energyAvailable: 10,
  objects: [ctrl({ level: 7 }), spawn('s1', 25, 40, { store: { energy: 1 } }), spawn('s2', 25, 27, { store: { energy: 1 } }), spawn('s3', 25, 21, { store: { energy: 1 } })],
  calls: [tick('spawns:s1'), tick('spawns:s2'), tick('spawns:s3')],
});
add('spawn tick: foreign controller, no regeneration', {
  energyAvailable: 10,
  objects: [ctrl({ user: 'u2' }), spawn('s1', 25, 30, { store: { energy: 1 } })],
  calls: [tick('spawns:s1')],
});

// ---- spawns: renew / recycle / directions / cancel ----
const intent = (object, intents) => ({ fn: 'spawns/intents', object, intent: intents });
add('renewCreep: charges and extends ageTime', {
  objects: [ctrl(), spawn('s1', 25, 30), creep('c1', 26, 31, { ageTime: 1100, body: [{ type: 'work', hits: 100 }, { type: 'move', hits: 100 }, { type: 'carry', hits: 100 }] })],
  calls: [intent('s1', { renewCreep: { id: 'c1' } })],
});
add('renewCreep: rejected for range, claim, age, foreign', {
  objects: [ctrl(), spawn('s1', 25, 30), creep('c1', 27, 31), creep('c2', 26, 30, { body: [{ type: 'claim', hits: 100 }] }), creep('c3', 24, 30, { ageTime: 2500 }), creep('c4', 24, 29, { user: 'u2' })],
  calls: ['c1', 'c2', 'c3', 'c4', 'missing'].map((id) => intent('s1', { renewCreep: { id } })),
});
add('renewCreep: boosted creep', {
  objects: [ctrl(), spawn('s1', 25, 30), creep('c1', 26, 31, { ageTime: 1100, body: [{ type: 'carry', hits: 100, boost: 'KH' }, { type: 'move', hits: 100 }] })],
  calls: [intent('s1', { renewCreep: { id: 'c1' } })],
});
add('recycleCreep', {
  objects: [ctrl(), spawn('s1', 25, 30), creep('c1', 26, 31, { store: { energy: 10 } })],
  calls: [intent('s1', { recycleCreep: { id: 'c1' } })],
});
add('recycleCreep: rejected', {
  objects: [ctrl(), spawn('s1', 25, 30), creep('c1', 27, 31), creep('c2', 26, 31, { spawning: true })],
  calls: [intent('s1', { recycleCreep: { id: 'c1' } }), intent('s1', { recycleCreep: { id: 'c2' } })],
});
add('setSpawnDirections: replaces the spawning object', {
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning({ directions: [1, 2] }) })],
  calls: [intent('s1', { setSpawnDirections: { directions: [5, '5', 4] } })],
});
add('setSpawnDirections: rejected', {
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning() }), spawn('s2', 25, 20)],
  calls: [intent('s1', { setSpawnDirections: { directions: [0] } }), intent('s1', { setSpawnDirections: { directions: [] } }), intent('s2', { setSpawnDirections: { directions: [1] } })],
});
add('cancelSpawning', {
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning() }), creep('bob', 25, 30, { name: 'Bob', spawning: true })],
  calls: [intent('s1', { cancelSpawning: {} })],
});
add('cancelSpawning: missing creep throws', {
  objects: [ctrl(), spawn('s1', 25, 30, { spawning: spawning() })],
  calls: [intent('s1', { cancelSpawning: {} })],
});

// ---- controllers ----
add('controller tick: reservation expires, owned rooms list', {
  users: [{ _id: 'u1', username: 'alice', rooms: [] }],
  objects: [ctrl({ reservation: { user: 'u2', endTime: 1001 } })],
  calls: [tick('controllers:ctrl')],
});
add('controller tick: reservation kept, user 2 stops early', {
  objects: [ctrl({ user: '2', reservation: { user: 'u2', endTime: 1005 } })],
  calls: [tick('controllers:ctrl')],
});
add('controller tick: upgrade blocked and safe mode', {
  objects: [ctrl({ _upgradeBlocked: 1500, _safeModeActivated: 1 })],
  calls: [tick('controllers:ctrl')],
});
add('controller tick: safe mode without cooldown in a novice room', {
  roomInfo: { _id: ROOM, status: 'normal', novice: 1700000000001 },
  objects: [ctrl({ _safeModeActivated: 1, upgradeBlocked: 999 })],
  calls: [tick('controllers:ctrl')],
});
add('controller tick: downgradeTime initialised', {
  objects: [ctrl({ downgradeTime: null, level: 3 })],
  calls: [tick('controllers:ctrl')],
});
add('controller tick: upgraded restores', {
  objects: [ctrl({ _upgraded: 5, downgradeTime: 1050 })],
  calls: [tick('controllers:ctrl')],
});
add('controller tick: 3000-tick warning', {
  objects: [ctrl({ downgradeTime: 4000, level: 4 })],
  calls: [tick('controllers:ctrl')],
});
add('controller tick: downgrade cascades to level 0', {
  gameTime: 30000,
  objects: [ctrl({ downgradeTime: 10, level: 2, progress: 5, safeMode: 1500, upgradeBlocked: 1200 })],
  calls: [tick('controllers:ctrl')],
});
add('controller tick: downgrade by one level', {
  objects: [ctrl({ downgradeTime: 1001, level: 5, progress: 100 })],
  calls: [tick('controllers:ctrl')],
});
add('controller tick: owner without a user doc throws', {
  users: [],
  objects: [ctrl()],
  calls: [tick('controllers:ctrl')],
});
add('activateSafeMode: accepted, then rejections', {
  objects: [ctrl({ downgradeTime: 20000, level: 4 }), ctrl({ _id: 'c2', safeModeCooldown: 1000 }), ctrl({ _id: 'c3', safeModeAvailable: 0 }), ctrl({ _id: 'c4', level: 4, downgradeTime: 15000 })],
  calls: ['ctrl', 'c2', 'c3', 'c4'].map((id) => ({ fn: 'controllers/activateSafeMode', object: id, intent: {} })),
});
add('unclaim', {
  objects: [ctrl({ level: 3, safeMode: 2000, upgradeBlocked: 5 })],
  calls: [{ fn: 'controllers/unclaim', object: 'ctrl', intent: {} }],
});

// ---- sources, energy, extensions, construction sites ----
const source = (id, o) => obj(id, 'source', 10, 10, Object.assign({ energy: 3000, energyCapacity: 3000 }, o));
add('source tick: regeneration timer starts', {
  objects: [ctrl(), source('so', { energy: 2990 })],
  calls: [tick('sources:so')],
});
add('source tick: refill, disrupt and regen powers', {
  objects: [
    ctrl(),
    source('so1', { energy: 100, nextRegenerationTime: 1001 }),
    source('so2', { energy: 100, nextRegenerationTime: 1001, effects: [{ power: C.PWR_DISRUPT_SOURCE, level: 1, endTime: 1100 }] }),
    source('so3', { energy: 100, nextRegenerationTime: 1100, effects: [{ power: C.PWR_REGEN_SOURCE, level: 3, endTime: 1001 }] }),
    source('so4', { energy: 100, nextRegenerationTime: 1100, effects: [{ power: C.PWR_REGEN_SOURCE, level: 3, endTime: 1002 }] }),
  ],
  calls: [tick('sources:so1'), tick('sources:so2'), tick('sources:so3'), tick('sources:so4')],
});
add('source tick: capacities', {
  objects: [ctrl({ user: null, level: 0 }), source('so1', { energy: 2000 }), source('so2', { energyCapacity: 1500, energy: 1500 })],
  calls: [tick('sources:so1'), tick('sources:so2')],
});
add('source tick: reserved and keeper capacities', {
  objects: [ctrl({ user: null, level: 0, reservation: { user: 'u1', endTime: 2000 } }), source('so1', { energyCapacity: 1500, energy: 1500 })],
  calls: [tick('sources:so1')],
});
add('source tick: no controller', {
  objects: [source('so1', { energy: 3000 })],
  calls: [tick('sources:so1')],
});
add('energy tick: decay and removal', {
  objects: [
    obj('d1', 'energy', 5, 5, { energy: 2500, resourceType: 'energy' }),
    obj('d2', 'energy', 5, 6, { energy: 1, resourceType: 'energy' }),
    obj('d3', 'energy', 5, 7, { power: 77, resourceType: 'power' }),
    obj('d4', 'energy', 5, 8, { energy: 300 }),
    obj('d5', 'energy', 5, 9, { energy: 'x', resourceType: 'energy' }),
  ],
  calls: ['d1', 'd2', 'd3', 'd4', 'd5'].map((id) => tick(`energy:${id}`)),
});
add('extension tick: capacity follows RCL', {
  objects: [ctrl({ level: 8 }), ext('e1', 20, 20), ext('e2', 21, 20, { storeCapacityResource: { energy: 200 } }), ext('e3', 22, 20, { storeCapacityResource: undefined })],
  calls: ['e1', 'e2', 'e3'].map((id) => tick(`extensions:${id}`)),
});
add('extension tick: no controller, level 0', {
  objects: [ext('e1', 20, 20)],
  calls: [tick('extensions:e1')],
});
add('construction site tick: no-op', {
  objects: [obj('cs', 'constructionSite', 20, 20, { structureType: 'road', user: 'u1', progress: 5, progressTotal: 300 })],
  calls: [{ fn: 'construction-sites/tick', object: 'cs' }],
});

// ---- roads, containers, tombstones, ruins ----
add('road tick: decay on plain, swamp, wall, legacy swamp; removal', {
  terrain: terrain({ swamps: [[11, 10]], walls: [[12, 10]] }),
  objects: [
    obj('r1', 'road', 10, 10, { hits: 5000, nextDecayTime: 1001 }),
    obj('r2', 'road', 11, 10, { hits: 5000, nextDecayTime: 1001 }),
    obj('r3', 'road', 12, 10, { hits: 100000, nextDecayTime: 1001 }),
    obj('r4', 'road', 13, 10, { hits: 50 }),
    obj('r5', 'road', 14, 10, { hits: 5000, nextDecayTime: 1500 }),
    obj('sw', 'swamp', 15, 10),
    obj('r6', 'road', 15, 10, { hits: 5000 }),
  ],
  calls: ['r1', 'r2', 'r3', 'r4', 'r5', 'r6'].map((id) => tick(`roads:${id}`)),
});
add('container tick: decay owned/unowned, empty removal', {
  objects: [ctrl({ level: 1 }), obj('k1', 'container', 10, 10, { hits: 250000, nextDecayTime: 1001, store: { energy: 10 } }), obj('k2', 'container', 11, 10, { hits: 4000, store: {} }), obj('k3', 'container', 12, 10, { store: { energy: 0 } })],
  calls: ['k1', 'k2', 'k3'].map((id) => tick(`containers:${id}`)),
});
add('container tick: unowned room rate', {
  objects: [ctrl({ level: 0, user: null }), obj('k1', 'container', 10, 10, { hits: 250000 })],
  calls: [tick('containers:k1')],
});
add('container tick: removal drops the store', {
  objects: [obj('k1', 'container', 10, 10, { hits: 100, store: { energy: 10, H: 0, power: 3 } })],
  calls: [tick('containers:k1')],
});
add('tombstone and ruin ticks: empty stores, not yet', {
  objects: [
    obj('t1', 'tombstone', 10, 10, { decayTime: 1001, store: {} }),
    obj('t2', 'tombstone', 11, 10, { decayTime: 1005, store: { energy: 5 } }),
    obj('u1r', 'ruin', 12, 10, { decayTime: 1000 }),
    obj('u2r', 'ruin', 13, 10, { decayTime: 1500, store: { energy: 5 } }),
  ],
  calls: [tick('tombstones:t1'), tick('tombstones:t2'), tick('ruins:u1r'), tick('ruins:u2r')],
});
add('tombstone tick: store dropped', {
  objects: [obj('t1', 'tombstone', 10, 10, { decayTime: 1001, store: { energy: 5, O: 3 } })],
  calls: [tick('tombstones:t1')],
});

// ---- structures/_destroy ----
const destroy = (object, attackType) => ({ fn: 'structures/_destroy', object, attackType });
add('destroy: ruin keeps the store, spawning creep removed', {
  objects: [ctrl(), ext('e1', 20, 20), spawn('s1', 25, 30, { spawning: spawning() }), creep('bob', 25, 30, { name: 'Bob', spawning: true }), obj('w', 'constructedWall', 3, 3, { hits: 10 })],
  calls: [destroy('e1'), destroy('s1'), destroy('w')],
});
add('destroy: nuke leaves no ruin, collapse timer extends the ruin', {
  objects: [
    ext('e1', 20, 20),
    obj('pb', 'powerBank', 30, 30, { hits: 0, hitsMax: 2000000, store: { power: 1000 } }),
    obj('t1', 'tower', 31, 31, { user: '2', store: { energy: 10 }, effects: [{ effect: C.EFFECT_COLLAPSE_TIMER, power: C.EFFECT_COLLAPSE_TIMER, endTime: 5000 }] }),
    obj('t2', 'tower', 32, 31, { user: '2', effects: [{ effect: C.EFFECT_COLLAPSE_TIMER, endTime: 1200 }] }),
  ],
  calls: [destroy('e1', C.EVENT_ATTACK_TYPE_NUKE), destroy('pb'), destroy('t1'), destroy('t2')],
});

// ---- room intents ----
const roomIntents = (user, intents) => ({ fn: 'room/intents', user, intent: intents });
const site = (o) => Object.assign({ roomName: ROOM, x: 20, y: 20, structureType: 'road', name: '' }, o);
add('room intents: flags created, removed, renamed', {
  flags: [{ _id: 'f1', user: 'u1', room: ROOM, data: 'Old~1~2~10~10|Keep~3~4~5~5' }],
  calls: [
    roomIntents('u1', {
      removeFlag: [{ roomName: ROOM, name: 'Old' }, { roomName: ROOM, name: 'Nope' }],
      createFlag: [
        { roomName: ROOM, x: 1, y: 2, name: 'A|b~c', color: 1, secondaryColor: 10 },
        { roomName: ROOM, x: 1, y: 2, name: 'Keep', color: 1, secondaryColor: 1 },
        { roomName: ROOM, x: 50, y: 2, name: 'Off', color: 1, secondaryColor: 1 },
        { roomName: ROOM, x: 3, y: 3, name: 'Bad', color: 11, secondaryColor: 1 },
        { roomName: ROOM, x: null, y: 3, name: 'NullX', color: 2, secondaryColor: 2 },
      ],
    }),
  ],
});
add('room intents: first flag of a user is inserted', {
  calls: [roomIntents('u2', { createFlag: [{ roomName: ROOM, x: 1, y: 2, name: 'F', color: 5, secondaryColor: 5 }] })],
});
add('room intents: two users, flag edits of the first are lost (BUG-FLAGS-REPARSE)', {
  flags: [{ _id: 'f1', user: 'u1', room: ROOM, data: 'A~1~1~1~1' }, { _id: 'f2', user: 'u2', room: ROOM, data: 'B~1~1~1~1' }],
  calls: [roomIntents('u1', { removeFlag: [{ roomName: ROOM, name: 'A' }] }), roomIntents('u2', { removeFlag: [{ roomName: ROOM, name: 'B' }] })],
});
add('room intents: a new flag doc crashes the next user (BUG-CRASH-FLAG-FIRST)', {
  flags: [{ _id: 'f2', user: 'u2', room: ROOM, data: 'B~1~1~1~1' }],
  calls: [roomIntents('u1', { createFlag: [{ roomName: ROOM, x: 1, y: 2, name: 'F', color: 5, secondaryColor: 5 }] }), roomIntents('u2', {})],
});
add('room intents: construction sites', {
  terrain: terrain({ swamps: [[20, 20]], walls: [[21, 20], [0, 9], [0, 10], [0, 11], [10, 0], [11, 0]] }),
  objects: [ctrl({ level: 3 }), ext('e1', 30, 30), obj('m', 'mineral', 40, 40, { mineralType: 'H', mineralAmount: 100 })],
  calls: [
    roomIntents('u1', {
      createConstructionSite: [
        site(),
        site({ x: 21 }),
        site({ x: 21, structureType: 'tower' }),
        site({ x: 22, structureType: 'spawn', name: 'Spawn2' }),
        site({ x: 30, y: 30, structureType: 'rampart' }),
        site({ x: 30, y: 30, structureType: 'container' }),
        site({ x: 40, y: 40, structureType: 'extractor' }),
        site({ x: 11, y: 1, structureType: 'tower' }),
        site({ x: 1, y: 10, structureType: 'tower' }),
        site({ x: 0, y: 5 }),
        site({ x: 5, structureType: 'nope' }),
        site({ x: 6, structureType: 'observer' }),
        site({ x: 7, structureType: 'extension' }),
      ],
    }),
  ],
});
add('room intents: sites in a room owned or reserved by someone else', {
  objects: [ctrl({ user: 'u2' })],
  calls: [roomIntents('u1', { createConstructionSite: [site()] })],
});
add('room intents: sites in a reserved room', {
  objects: [ctrl({ user: null, level: 0, reservation: { user: 'u2', endTime: 3000 } })],
  calls: [roomIntents('u1', { createConstructionSite: [site()] }), roomIntents('u2', { createConstructionSite: [site({ x: 30 })] })],
});
add('room intents: removeConstructionSite without refund', {
  objects: [ctrl(), obj('cs1', 'constructionSite', 20, 20, { structureType: 'road', user: 'u2', progress: 1, progressTotal: 300 }), obj('cs2', 'constructionSite', 21, 20, { structureType: 'road', user: 'u2', progress: 0 })],
  calls: [roomIntents('u1', { removeConstructionSite: [{ roomName: ROOM, id: 'cs1' }, { roomName: ROOM, id: 'missing' }, { roomName: ROOM, id: 'ctrl' }] }), roomIntents('u2', { removeConstructionSite: [{ roomName: ROOM, id: 'cs2' }] })],
});
add('room intents: removeConstructionSite refunds', {
  objects: [obj('cs1', 'constructionSite', 20, 20, { structureType: 'road', user: 'u1', progress: 41, progressTotal: 300 })],
  calls: [roomIntents('u1', { removeConstructionSite: [{ roomName: ROOM, id: 'cs1' }] })],
});
add('room intents: destroyStructure', {
  objects: [ctrl(), ext('e1', 20, 20, { user: 'u2' }), obj('w1', 'constructedWall', 3, 3, { hits: 1, decayTime: 5000 }), obj('src', 'source', 5, 5)],
  calls: [roomIntents('u1', { destroyStructure: [{ roomName: ROOM, id: 'e1' }, { roomName: ROOM, id: 'w1' }, { roomName: ROOM, id: 'src' }, { roomName: ROOM, id: 'ctrl' }] })],
});
add('room intents: destroyStructure blocked by a hostile creep, not owner', {
  objects: [ctrl(), ext('e1', 20, 20), creep('h', 10, 10, { user: '3' })],
  calls: [roomIntents('u1', { destroyStructure: [{ roomName: ROOM, id: 'e1' }] }), roomIntents('u2', { destroyStructure: [{ roomName: ROOM, id: 'e1' }] })],
});

add('room intents: genEnergy only for user 3, retries blocked tiles', {
  users: [{ _id: 'u1', username: 'alice', rooms: [] }, { _id: '3', username: 'Source Keeper' }],
  terrain: terrain({ walls: [[33, 21]] }),
  objects: [obj('src', 'source', 9, 21)],
  calls: [roomIntents('u1', { genEnergy: { roomName: ROOM } }), roomIntents('3', { genEnergy: { roomName: ROOM } }), roomIntents('3', { genEnergy: true })],
});

// ---- minerals, extractors, walls, ramparts, storages, deposits, portals, keeper lairs ----
const mineral = (id, o) => obj(id, 'mineral', 40, 40, Object.assign({ mineralType: 'H', mineralAmount: 0, density: 3 }, o));
add('mineral tick: timer starts', {
  objects: [mineral('m1')],
  calls: [tick('minerals:m1')],
});
add('mineral tick: refill with density rerolls', {
  objects: [mineral('m1', { nextRegenerationTime: 1001, density: 1 }), mineral('m2', { nextRegenerationTime: 1001, density: 4 }), mineral('m3', { nextRegenerationTime: 1000, density: 2 }), mineral('m4', { nextRegenerationTime: 1001, density: 3 })],
  calls: ['m1', 'm2', 'm3', 'm4'].map((id) => tick(`minerals:${id}`)),
});
add('mineral tick: regen power', {
  objects: [mineral('m1', { mineralAmount: 100, effects: [{ power: C.PWR_REGEN_MINERAL, level: 2, endTime: 1001 }] }), mineral('m2', { mineralAmount: 100, effects: [{ power: C.PWR_REGEN_MINERAL, level: 2, endTime: 1002 }] })],
  calls: [tick('minerals:m1'), tick('minerals:m2')],
});
add('extractor tick: cooldown and harvest cooldown', {
  objects: [obj('x1', 'extractor', 40, 40, { cooldown: 3 }), obj('x2', 'extractor', 41, 40, { cooldown: 0.5, _cooldown: 5 }), obj('x3', 'extractor', 42, 40, { cooldown: 0 })],
  calls: ['x1', 'x2', 'x3'].map((id) => tick(`extractors:${id}`)),
});
add('wall tick: hitsMax by RCL, newbie walls', {
  objects: [
    ctrl({ level: 1 }),
    obj('w1', 'constructedWall', 3, 3, { hits: 10, hitsMax: 300000000 }),
    obj('w2', 'constructedWall', 4, 3, { hits: 10, hitsMax: 1, newbieWall: true }),
    obj('w3', 'constructedWall', 5, 3, { hits: 10, ticksToLive: 50, user: 'u1' }),
    obj('w4', 'constructedWall', 6, 3, { hits: 10, decayTime: 6000, user: 'u1' }),
    obj('w5', 'constructedWall', 7, 3, { hits: 10, decayTime: 1001 }),
    obj('w6', 'constructedWall', 8, 3, { hits: 10, decayTime: { timestamp: 1600000000000 } }),
    obj('w7', 'constructedWall', 9, 3, { hits: 10, decayTime: { timestamp: 1800000000000 } }),
  ],
  calls: ['w1', 'w2', 'w3', 'w4', 'w5', 'w6', 'w7'].map((id) => tick(`constructedWalls:${id}`)),
});
add('wall tick: unowned controller removes newbie walls', {
  objects: [ctrl({ level: 0, user: null }), obj('w1', 'constructedWall', 3, 3, { hits: 10, hitsMax: 0, decayTime: 50000 })],
  calls: [tick('constructedWalls:w1')],
});
add('rampart: setPublic, shield, hitsMax, decay', {
  objects: [
    ctrl({ level: 3 }),
    obj('r1', 'rampart', 10, 10, { user: 'u1', hits: 1000, hitsMax: 1, nextDecayTime: 1001 }),
    obj('r2', 'rampart', 11, 10, { user: 'u2', hits: 200, hitsMax: 300000 }),
    obj('r3', 'rampart', 12, 10, { user: 'u1', hits: 5000, effects: [{ power: C.PWR_SHIELD, endTime: 1000 }] }),
    obj('r4', 'rampart', 13, 10, { user: 'u1', hits: 5000, effects: [{ power: C.PWR_SHIELD, endTime: 1001 }] }),
    obj('r5', 'rampart', 14, 10, { user: '2', hits: 5000, hitsMax: 7, nextDecayTime: 2000 }),
  ],
  calls: [{ fn: 'ramparts/set-public', object: 'r1', intent: { isPublic: 1 } }, ...['r1', 'r2', 'r3', 'r4', 'r5'].map((id) => tick(`ramparts:${id}`))],
});
add('storage tick: capacity, operate storage', {
  objects: [ctrl({ level: 4 }), obj('st1', 'storage', 10, 10, { user: 'u1', storeCapacity: 0, effects: [{ power: C.PWR_OPERATE_STORAGE, level: 1, endTime: 1001 }] }), obj('st2', 'storage', 11, 10, { user: 'u2', storeCapacity: 1000000 })],
  calls: [tick('storages:st1'), tick('storages:st2')],
});
add('deposit, portal and keeper lair ticks', {
  objects: [
    obj('dp1', 'deposit', 10, 10, { depositType: 'mist', _cooldown: 7, decayTime: 999 }),
    obj('p1', 'portal', 20, 20, { unstableDate: 1600000000000, destination: { room: 'W2N2', x: 5, y: 5 } }),
    obj('p2', 'portal', 25, 25, { decayTime: 999 }),
    obj('pw', 'constructedWall', 26, 26),
    obj('lair1', 'keeperLair', 30, 30),
    obj('lair2', 'keeperLair', 35, 35, { nextSpawnTime: 1001 }),
    creep('k2', 36, 36, { user: '3', name: 'Keeperlair2', hits: 5000 }),
    obj('lair3', 'keeperLair', 40, 30),
    creep('k3', 41, 31, { user: '3', name: 'Keeperlair3', hits: 5000 }),
  ],
  calls: [tick('deposits:dp1'), tick('portals:p1'), tick('portals:p2'), tick('keeper-lairs:lair1'), tick('keeper-lairs:lair2'), tick('keeper-lairs:lair3')],
});

// ---- links, towers, terminal, factory, power spawn ----
const link = (id, x, y, o) =>
  obj(id, 'link', x, y, Object.assign({ user: 'u1', store: { energy: 400 }, storeCapacityResource: { energy: 800 }, cooldown: 0, actionLog: { transferEnergy: null }, _actionLog: { transferEnergy: null } }, o));
add('link transfer: clamped, loss, cooldown by distance, then ticks', {
  objects: [ctrl({ level: 5 }), link('l1', 24, 24), link('l2', 28, 26, { store: { energy: 700 } }), link('l3', 15, 15, { cooldown: 2 })],
  calls: [
    { fn: 'links/intents', object: 'l1', intent: { transfer: { id: 'l2', amount: 300 } } },
    { fn: 'links/intents', object: 'l3', intent: { transfer: { id: 'l1', amount: 10 } } },
    tick('links:l1'),
    tick('links:l2'),
    tick('links:l3'),
  ],
});
add('link transfer: rejections', {
  objects: [ctrl({ level: 5 }), link('l1', 10, 10), link('l2', 11, 10, { store: { energy: 800 } }), obj('s', 'storage', 12, 10, { store: { energy: 0 } }), link('l4', 13, 10, { user: 'u2' })],
  calls: [
    { fn: 'links/intents', object: 'l1', intent: { transfer: { id: 'l2', amount: 10 } } },
    { fn: 'links/intents', object: 'l1', intent: { transfer: { id: 's', amount: 10 } } },
    { fn: 'links/intents', object: 'l1', intent: { transfer: { id: 'l2', amount: -1 } } },
    { fn: 'links/intents', object: 'l1', intent: { transfer: { id: 'l2', amount: 500 } } },
    { fn: 'links/intents', object: 'l4', intent: { transfer: { id: 'l1', amount: 10 } } },
  ],
});
const tower = (id, x, y, o) =>
  obj(id, 'tower', x, y, Object.assign({ user: 'u1', store: { energy: 1000 }, storeCapacityResource: { energy: 1000 }, hits: 3000, hitsMax: 3000, actionLog: { attack: null, heal: null, repair: null }, _actionLog: { attack: null, heal: null, repair: null } }, o));
add('tower heal and repair with falloff and powers', {
  objects: [
    ctrl({ level: 8 }),
    tower('t1', 10, 10),
    tower('t2', 40, 40, { effects: [{ power: C.PWR_OPERATE_TOWER, level: 3, endTime: 1001 }, { power: C.PWR_DISRUPT_TOWER, level: 1, endTime: 2000 }] }),
    creep('c1', 22, 10, { hits: 100, actionLog: { healed: null } }),
    obj('pc', 'powerCreep', 30, 30, { user: 'u2', hits: 10, actionLog: { healed: null } }),
    obj('rd', 'road', 10, 40, { hits: 100, hitsMax: 5000 }),
    obj('wl', 'constructedWall', 41, 40, { hits: 10, hitsMax: 300 }),
  ],
  calls: [
    { fn: 'towers/intents', object: 't1', intent: { heal: { id: 'c1' }, repair: { id: 'rd' }, attack: { id: 'c1' } } },
    { fn: 'towers/intents', object: 't2', intent: { heal: { id: 'pc' } } },
    { fn: 'towers/intents', object: 't1', intent: { repair: { id: 'rd' } } },
    { fn: 'towers/intents', object: 't2', intent: { repair: { id: 'wl' } } },
    tick('towers:t1'),
    tick('towers:t2'),
  ],
});
add('tower rejections', {
  objects: [ctrl(), tower('t1', 10, 10, { store: { energy: 9 } }), tower('t2', 11, 10), creep('c1', 12, 10, { spawning: true }), obj('src', 'source', 13, 10), obj('rd', 'road', 14, 10, { hits: 5000, hitsMax: 5000 }), creep('c2', 15, 10)],
  calls: [
    { fn: 'towers/intents', object: 't1', intent: { heal: { id: 'c2' } } },
    { fn: 'towers/intents', object: 't2', intent: { heal: { id: 'c1' } } },
    { fn: 'towers/intents', object: 't2', intent: { heal: { id: 'src' } } },
    { fn: 'towers/intents', object: 't2', intent: { repair: { id: 'rd' } } },
    { fn: 'towers/intents', object: 't2', intent: { repair: { id: 'src' } } },
    { fn: 'towers/intents', object: 't2', intent: { attack: { id: 't2' } } },
    { fn: 'towers/intents', object: 't2', intent: { attack: { id: 'c1' } } },
    { fn: 'towers/intents', object: 't1', intent: { attack: { id: 'c2' } } },
    tick('towers:t1'),
  ],
});
add('tower attack', {
  objects: [ctrl(), tower('t1', 10, 10), creep('c1', 20, 20, { user: 'u2' })],
  calls: [{ fn: 'towers/intents', object: 't1', intent: { attack: { id: 'c1' } } }],
});
const terminal = (id, o) => obj(id, 'terminal', 10, 10, Object.assign({ user: 'u1', store: { energy: 1000, H: 500 }, storeCapacity: 300000 }, o));
add('terminal send and tick', {
  objects: [
    ctrl({ level: 6 }),
    terminal('tm1'),
    terminal('tm2', { store: { energy: 1000 }, effects: [{ power: C.PWR_OPERATE_TERMINAL, level: 5, endTime: 1000 }] }),
    terminal('tm3', { store: { energy: 100, H: 500 } }),
    terminal('tm4', { user: 'u2', storeCapacity: 0 }),
  ],
  calls: [
    { fn: 'terminal/send', object: 'tm1', intent: { targetRoomName: 'E5S5', resourceType: 'H', amount: 400, description: 'hi' } },
    { fn: 'terminal/send', object: 'tm2', intent: { targetRoomName: 'W1N9', resourceType: 'energy', amount: 900, description: '' } },
    { fn: 'terminal/send', object: 'tm3', intent: { targetRoomName: 'W9N9', resourceType: 'H', amount: 400, description: '' } },
    { fn: 'terminal/send', object: 'tm1', intent: { targetRoomName: 'sim', resourceType: 'H', amount: 1, description: '' } },
    { fn: 'terminal/send', object: 'tm1', intent: { targetRoomName: 'W2N2', resourceType: 'nope', amount: 1, description: '' } },
    { fn: 'terminal/send', object: 'tm1', intent: { targetRoomName: 'W2N2', resourceType: 'H', amount: 600, description: '' } },
    tick('terminal:tm1'),
    tick('terminal:tm4'),
  ],
});
const factory = (id, o) => obj(id, 'factory', 10, 10, Object.assign({ user: 'u1', store: { U: 600, energy: 300 }, storeCapacity: 50000, actionLog: { produce: null }, _actionLog: { produce: null } }, o));
add('factory produce and tick', {
  objects: [
    ctrl({ level: 8 }),
    factory('f1'),
    factory('f2', { level: 1, store: { metal: 100 } }),
    factory('f3', { level: 1, store: { [C.RESOURCE_UTRIUM_BAR]: 20, [C.RESOURCE_LEMERGIUM_BAR]: 20, energy: 100, silicon: 200 }, effects: [{ power: C.PWR_OPERATE_FACTORY, level: 1, endTime: 1000 }] }),
    factory('f4', { cooldownTime: 1001 }),
    factory('f5', { storeCapacity: 100 }),
  ],
  calls: [
    { fn: 'factories/produce', object: 'f1', intent: { resourceType: C.RESOURCE_UTRIUM_BAR, amount: 1 } },
    { fn: 'factories/produce', object: 'f2', intent: { resourceType: C.RESOURCE_COMPOSITE, amount: 1 } },
    { fn: 'factories/produce', object: 'f3', intent: { resourceType: C.RESOURCE_TRANSISTOR, amount: 1 } },
    { fn: 'factories/produce', object: 'f4', intent: { resourceType: C.RESOURCE_UTRIUM_BAR, amount: 1 } },
    { fn: 'factories/produce', object: 'f5', intent: { resourceType: C.RESOURCE_UTRIUM_BAR, amount: 1 } },
    { fn: 'factories/produce', object: 'f1', intent: { resourceType: 'nope', amount: 1 } },
    tick('factories:f1'),
    tick('factories:f2'),
  ],
});
add('factory tick: foreign controller', {
  objects: [ctrl({ level: 6, user: 'u2' }), factory('f1', { storeCapacity: 50000 })],
  calls: [tick('factories:f1')],
});
const pspawn = (id, o) => obj(id, 'powerSpawn', 10, 10, Object.assign({ user: 'u1', store: { energy: 500, power: 10 } }, o));
add('power spawn processPower', {
  objects: [ctrl({ level: 8 }), pspawn('ps1'), pspawn('ps2', { store: { energy: 200, power: 2 }, effects: [{ power: C.PWR_OPERATE_POWER, level: 5, endTime: 1000 }] }), pspawn('ps3', { store: { energy: 40, power: 10 } })],
  calls: ['ps1', 'ps2', 'ps3'].map((id) => ({ fn: 'power-spawns/intents', object: id, intent: { processPower: {} } })),
});

// ---- labs ----
const lab = (id, x, y, store, o) => {
  const m = Object.keys(store).find((k) => k !== 'energy' && store[k]);
  return obj(
    id, 'lab', x, y,
    Object.assign(
      {
        user: 'u1', store, storeCapacityResource: m ? { energy: 2000, [m]: 3000 } : { energy: 2000 }, storeCapacity: m ? null : 5000, cooldownTime: 0,
        actionLog: { runReaction: null, reverseReaction: null }, _actionLog: { runReaction: null, reverseReaction: null },
      },
      o,
    ),
  );
};
add('lab runReaction: new product, drained reagent, then tick', {
  objects: [ctrl({ level: 8 }), lab('lb', 20, 20, { energy: 0 }), lab('l1', 21, 21, { energy: 0, H: 5 }), lab('l2', 22, 22, { energy: 0, O: 100 })],
  calls: [{ fn: 'labs/intents', object: 'lb', intent: { runReaction: { lab1: 'l1', lab2: 'l2' }, reverseReaction: { lab1: 'l1', lab2: 'l2' } } }, tick('labs:lb'), tick('labs:l1')],
});
add('lab runReaction: rejections and operate lab', {
  objects: [
    ctrl({ level: 8 }),
    lab('lb', 20, 20, { energy: 0, UH: 2990 }),
    lab('lc', 20, 25, { energy: 0, OH: 10 }, { effects: [{ power: C.PWR_OPERATE_LAB, level: 2, endTime: 1001 }] }),
    lab('l1', 21, 21, { energy: 0, U: 50 }),
    lab('l2', 22, 22, { energy: 0, H: 50 }),
    lab('l3', 23, 23, { energy: 0, O: 50 }),
    lab('l4', 21, 24, { energy: 0, H: 50 }),
    lab('l5', 22, 26, { energy: 0, O: 50 }),
  ],
  calls: [
    { fn: 'labs/intents', object: 'lb', intent: { runReaction: { lab1: 'l1', lab2: 'l2' } } },
    { fn: 'labs/intents', object: 'lb', intent: { runReaction: { lab1: 'l1', lab2: 'l3' } } },
    { fn: 'labs/intents', object: 'lc', intent: { runReaction: { lab1: 'l4', lab2: 'l5' } } },
  ],
});
add('lab runReaction: reagent without a REACTIONS row throws', {
  objects: [ctrl({ level: 8 }), lab('lb', 20, 20, { energy: 0 }), lab('l1', 21, 21, { energy: 0, XGH2O: 50 }), lab('l2', 22, 22, { energy: 0, H: 50 })],
  calls: [{ fn: 'labs/intents', object: 'lb', intent: { runReaction: { lab1: 'l1', lab2: 'l2' } } }],
});
add('lab reverseReaction', {
  objects: [ctrl({ level: 8 }), lab('lb', 20, 20, { energy: 0, OH: 5 }), lab('l1', 21, 21, { energy: 0 }), lab('l2', 22, 22, { energy: 0, H: 10 })],
  calls: [{ fn: 'labs/intents', object: 'lb', intent: { reverseReaction: { lab1: 'l1', lab2: 'l2' } } }],
});
add('lab reverseReaction: same lab, wrong reagents, missing lab throws', {
  objects: [ctrl({ level: 8 }), lab('lb', 20, 20, { energy: 0, OH: 5 }), lab('l1', 21, 21, { energy: 0, U: 10 }), lab('l2', 22, 22, { energy: 0 })],
  calls: [
    { fn: 'labs/intents', object: 'lb', intent: { reverseReaction: { lab1: 'l1', lab2: 'l1' } } },
    { fn: 'labs/intents', object: 'lb', intent: { reverseReaction: { lab1: 'l1', lab2: 'l2' } } },
    { fn: 'labs/intents', object: 'lb', intent: { reverseReaction: { lab1: 'gone', lab2: 'l2' } } },
  ],
});
add('lab boostCreep and unboostCreep rejections', {
  objects: [ctrl({ level: 8 }), lab('lb', 20, 20, { energy: 10, XUH2O: 100 }), lab('lc', 25, 20, { energy: 100, XUH2O: 100 }), creep('c1', 21, 21), creep('c2', 26, 20, { body: [{ type: 'move', hits: 100 }] }), creep('c3', 27, 22, { user: 'u2' })],
  calls: [
    { fn: 'labs/intents', object: 'lb', intent: { boostCreep: { id: 'c1' } } },
    { fn: 'labs/intents', object: 'lc', intent: { boostCreep: { id: 'c2' } } },
    { fn: 'labs/intents', object: 'lc', intent: { boostCreep: { id: 'c3' } } },
    { fn: 'labs/intents', object: 'lc', intent: { unboostCreep: { id: 'c2' } } },
    { fn: 'labs/intents', object: 'lc', intent: { unboostCreep: { id: 'c3' } } },
  ],
});
add('lab boostCreep', {
  objects: [ctrl({ level: 8 }), lab('lb', 20, 20, { energy: 100, XUH2O: 90 }), creep('c1', 21, 21, { body: [{ type: 'attack', hits: 100 }, { type: 'attack', hits: 100 }, { type: 'move', hits: 100 }, { type: 'attack', hits: 100 }] })],
  calls: [{ fn: 'labs/intents', object: 'lb', intent: { boostCreep: { id: 'c1', bodyPartsCount: -1 } } }],
});

// ---- nukers, nukes ----
const nuker = (id, o) => obj(id, 'nuker', 10, 10, Object.assign({ user: 'u1', store: { energy: 300000, G: 5000 }, storeCapacityResource: { energy: 300000, G: 5000 }, cooldownTime: 0 }, o));
add('launchNuke: rejections', {
  objects: [ctrl({ level: 8 }), nuker('n1', { store: { energy: 300000, G: 4999 } }), nuker('n2', { cooldownTime: 1001 }), nuker('n3')],
  calls: [
    { fn: 'nukers/launch-nuke', object: 'n1', intent: { x: 25, y: 25, roomName: 'W2N2' } },
    { fn: 'nukers/launch-nuke', object: 'n2', intent: { x: 25, y: 25, roomName: 'W2N2' } },
    { fn: 'nukers/launch-nuke', object: 'n3', intent: { x: 50, y: 25, roomName: 'W2N2' } },
    { fn: 'nukers/launch-nuke', object: 'n3', intent: { x: 25, y: 25, roomName: 'W20N2' } },
    { fn: 'nukers/launch-nuke', object: 'n3', intent: { x: 25, y: 25, roomName: 'sim' } },
  ],
});
add('launchNuke: novice room', {
  roomInfo: { _id: ROOM, status: 'normal', novice: 1800000000000 },
  objects: [ctrl({ level: 8 }), nuker('n3')],
  calls: [{ fn: 'nukers/launch-nuke', object: 'n3', intent: { x: 25, y: 25, roomName: 'W2N2' } }],
});
add('launchNuke', {
  objects: [ctrl({ level: 8 }), nuker('n3')],
  calls: [{ fn: 'nukers/launch-nuke', object: 'n3', intent: { x: 25, y: 25, roomName: 'W2N2' } }],
});
add('nuke pretick cancels createCreep on the landing tick', {
  objects: [obj('nk', 'nuke', 25, 25, { landTime: 1001 }), obj('nk2', 'nuke', 5, 5, { landTime: 1005 })],
  calls: [
    { fn: 'nukes/pretick', object: 'nk2', intents: { users: { u1: { objects: { s1: { createCreep: { name: 'A' } } } } } } },
    { fn: 'nukes/pretick', object: 'nk', intents: { users: { u1: { objects: { s1: { createCreep: { name: 'A' } }, s2: { renewCreep: { id: 'x' } } } }, u2: { objects: {} } } } },
  ],
});
add('nuke tick: novice room removes it', {
  roomInfo: { _id: ROOM, status: 'normal', respawnArea: 1800000000000 },
  objects: [obj('nk', 'nuke', 25, 25, { landTime: 5000 })],
  calls: [tick('nukes:nk')],
});
add('nuke tick: gone one tick after landing', {
  objects: [obj('nk', 'nuke', 25, 25, { landTime: 1000 }), obj('nk2', 'nuke', 5, 5, { landTime: 9000 })],
  calls: [tick('nukes:nk'), tick('nukes:nk2')],
});
add('nuke tick: landing', {
  objects: [ctrl({ safeMode: 2000 }), obj('nk', 'nuke', 25, 25, { landTime: 1001 }), obj('cs', 'constructionSite', 40, 40, { structureType: 'road', user: 'u1' })],
  calls: [tick('nukes:nk')],
});

// ---- edge cases ----
add('calc_spawns: odd controller levels', {
  objects: [ctrl({ level: '2' }), spawn('s1', 30, 30), spawn('s2', 26, 26), ...[1, 2, 3, 4, 5, 6].map((i) => ext(`e${i}`, 20 + i, 20))],
  calls: [{ fn: '_calc_spawns' }],
});
add('calc_spawns: fractional level', {
  objects: [ctrl({ level: 2.7 }), ...[1, 2, 3, 4, 5, 6].map((i) => ext(`e${i}`, 20 + i, 20))],
  calls: [{ fn: '_calc_spawns' }],
});
add('room intents: border tiles at 48, extractor on a mineral', {
  terrain: terrain({ walls: [[49, 9], [49, 10], [49, 11], [9, 49], [10, 49]] }),
  objects: [ctrl({ level: 6 }), obj('m', 'mineral', 40, 40, { mineralType: 'H', mineralAmount: 100 }), obj('sp', 'spawn', 30, 30, { user: 'u1' })],
  calls: [
    roomIntents('u1', {
      createConstructionSite: [
        site({ x: 48, y: 10, structureType: 'tower' }),
        site({ x: 10, y: 48, structureType: 'tower' }),
        site({ x: 40, y: 40, structureType: 'extractor' }),
        site({ x: 40, y: 40, structureType: 'extractor' }),
        site({ x: 30, y: 30, structureType: 'road' }),
        site({ x: 30, y: 30, structureType: 'tower' }),
        site({ x: 48, y: 48, structureType: 'container' }),
      ],
    }),
  ],
});
add('controller tick: upgraded but blocked falls through to the downgrade', {
  objects: [ctrl({ _upgraded: 3, upgradeBlocked: 1500, downgradeTime: 900, level: 3, progress: 7 })],
  calls: [tick('controllers:ctrl')],
});
add('keeper lair: damaged keeper starts the timer, an undamaged one does not', {
  objects: [obj('lairA', 'keeperLair', 30, 30), creep('kA', 31, 31, { user: '3', name: 'KeeperlairA', hits: 4999 }), obj('lairB', 'keeperLair', 40, 30), creep('kB', 41, 31, { user: '3', name: 'KeeperlairB', hits: 5000 })],
  calls: [tick('keeper-lairs:lairA'), tick('keeper-lairs:lairB')],
});
add('a tick sequence: create, charge, extension capacity, regeneration', {
  energyAvailable: 150,
  objects: [ctrl({ level: 2 }), spawn('s1', 25, 30, { store: { energy: 100 } }), ext('e1', 25, 33), ext('e2', 24, 33, { storeCapacityResource: { energy: 0 } })],
  calls: [create({ name: 'Bob', body: ['work', 'move'] }), tick('spawns:s1'), tick('extensions:e1'), tick('extensions:e2')],
});

// ---- review: adversarial inputs (add-object-intent shapes, NaN, strings, prototype keys) ----
add('createCreep: an array intent (add-object-intent) throws at body.slice', {
  objects: room1(),
  calls: [create(undefined, { createCreep: [] })],
});
add('lab reverseReaction: Object.prototype member ids are truthy non-labs', {
  objects: [ctrl({ level: 8 }), lab('lb', 20, 20, { energy: 0, OH: 5 }), lab('l1', 21, 21, { energy: 0 }), lab('l2', 22, 22, { energy: 0 })],
  calls: [
    { fn: 'labs/intents', object: 'lb', intent: { reverseReaction: { lab1: 'constructor', lab2: 'l2' } } },
    { fn: 'labs/intents', object: 'lb', intent: { reverseReaction: { lab1: 'l1', lab2: '__proto__' } } },
    { fn: 'labs/intents', object: 'lb', intent: { reverseReaction: { lab1: 'hasOwnProperty', lab2: 'toString' } } },
  ],
});
add('room intents: NaN coordinates alias tile 0 in the terrain', {
  terrain: terrain({ swamps: [[0, 0]] }),
  objects: [ctrl({ level: 2 })],
  calls: [
    roomIntents('u1', {
      createConstructionSite: [site({ x: NaN, y: NaN }), site({ x: NaN, y: NaN }), site({ x: NaN, y: NaN, structureType: 'extension' })],
      createFlag: [{ roomName: ROOM, x: NaN, y: NaN, name: 'N', color: 1, secondaryColor: 1 }],
    }),
  ],
});
add('room intents: a non-array createConstructionSite iterates the sanitized fields', {
  objects: [ctrl({ level: 2 })],
  calls: [roomIntents('u1', { createConstructionSite: site() })],
});
add('room intents: a non-array createFlag throws on the first field', {
  calls: [roomIntents('u1', { createFlag: { roomName: ROOM, x: 1, y: 2, name: 'F', color: 1, secondaryColor: 1 } })],
});
add('room intents: removing a site twice queues two removes', {
  objects: [ctrl(), obj('cs1', 'constructionSite', 20, 20, { structureType: 'road', user: 'u1', progress: 1 })],
  calls: [roomIntents('u1', { removeConstructionSite: [{ roomName: ROOM, id: 'cs1' }, { roomName: ROOM, id: 'cs1' }, { roomName: ROOM, id: 'toString' }] })],
});
add('link transfer: a NaN amount passes the checks', {
  objects: [ctrl({ level: 5 }), link('l1', 24, 24), link('l2', 28, 26, { store: { energy: 700 } })],
  calls: [{ fn: 'links/intents', object: 'l1', intent: { transfer: { id: 'l2', amount: NaN } } }, tick('links:l1'), tick('links:l2')],
});
add('controller tick: string level and progress', {
  objects: [ctrl({ level: '3', downgradeTime: 1001, progress: '5' })],
  calls: [tick('controllers:ctrl')],
});
add('extension and source ticks: string levels and amounts', {
  objects: [ctrl({ level: '8' }), ext('e1', 20, 20), source('so', { energy: '100', nextRegenerationTime: '1001' })],
  calls: [tick('extensions:e1'), tick('sources:so')],
});
add('renewCreep: an empty body', {
  objects: [ctrl(), spawn('s1', 25, 30), creep('c1', 26, 31, { ageTime: 1100, body: [] })],
  calls: [intent('s1', { renewCreep: { id: 'c1' } })],
});
add('nuke pretick: a null user entry throws', {
  objects: [obj('nk', 'nuke', 25, 25, { landTime: 1001 })],
  calls: [{ fn: 'nukes/pretick', object: 'nk', intents: { users: { u1: { objects: {} }, u2: null } } }],
});
add('terminal send: a trailing newline is no room name', {
  objects: [ctrl({ level: 6 }), terminal('tm1')],
  calls: [{ fn: 'terminal/send', object: 'tm1', intent: { targetRoomName: 'W1N9\n', resourceType: 'H', amount: 10, description: '' } }],
});
add('calc_spawns: 70 extensions sort stably past the insertion-sort size', {
  objects: [ctrl({ level: 8 }), ...Array.from({ length: 70 }, (_v, i) => ext(`x${i}`, 25 + ((i * 7) % 11) - 5, 25 + ((i * 3) % 9) - 4))],
  calls: [{ fn: '_calc_spawns' }],
});
add('destroy: a spawn whose spawning name is its own name removes itself first', {
  objects: [ctrl(), spawn('s1', 25, 30, { name: 'Self', spawning: spawning({ name: 'Self' }) })],
  calls: [destroy('s1')],
});
add('tower heal: a creep without actionLog throws after the energy is spent', {
  objects: [ctrl({ level: 8 }), tower('t1', 10, 10), creep('c1', 12, 10, { hits: 100, actionLog: undefined })],
  calls: [{ fn: 'towers/intents', object: 't1', intent: { heal: { id: 'c1' } } }],
});
add('createCreep: NaN extension energy poisons the old charging', {
  objects: room1([ext('e1', 25, 35, { store: { energy: 'x' } }), ext('e2', 25, 32)], { level: 8 }).map((o) => (o._id === 's1' ? Object.assign(o, { store: { energy: 100 } }) : o)),
  calls: [create({ name: 'Bob', body: ['work', 'move'] })],
});
add('unclaim and controller tick: string user.rooms', {
  users: [{ _id: 'u1', username: 'alice', rooms: 'W2N2W1N1' }],
  objects: [ctrl({ level: 1, downgradeTime: 1001 })],
  calls: [tick('controllers:ctrl')],
});
add('activateSafeMode: string fields', {
  objects: [ctrl({ level: '4', safeModeAvailable: '1', safeModeCooldown: '999', downgradeTime: '20000' })],
  calls: [{ fn: 'controllers/activateSafeMode', object: 'ctrl', intent: {} }, tick('controllers:ctrl')],
});
add('storage, terminal, factory ticks: string controller level', {
  objects: [ctrl({ level: '8' }), obj('st', 'storage', 10, 10, { user: 'u1' }), terminal('tm', { storeCapacity: 1 }), factory('f1', { storeCapacity: 1 })],
  calls: [tick('storages:st'), tick('terminal:tm'), tick('factories:f1')],
});
add('calc_spawns: null and negative levels', {
  objects: [ctrl({ level: null }), spawn('s1', 30, 30), ctrl({ _id: 'c2', level: -3 }), ext('e1', 20, 20, { off: false })],
  calls: [{ fn: '_calc_spawns' }],
});
add('wall tick: an array decayTime never expires', {
  objects: [obj('w1', 'constructedWall', 3, 3, { hits: 10, decayTime: [1] })],
  calls: [tick('constructedWalls:w1')],
});

// ---- run ----
function fresh(v) {
  return _.cloneDeep(v);
}

function invoke(call, scope) {
  const f = handler(call.fn);
  const object = scope.roomObjects[call.object];
  if (call.fn === '_calc_spawns') {
    // processor.js:145-150 collects them in key order
    const spawns = [], extensions = [];
    _.forEach(scope.roomObjects, (o) => {
      if (o.type == 'extension') extensions.push(o);
      if (o.type == 'spawn') spawns.push(o);
    });
    return f(spawns, extensions, scope);
  }
  if (call.fn.startsWith('room/')) return f(call.user, fresh(call.intent), scope);
  if (call.fn.endsWith('/tick')) return f(object, scope);
  if (call.fn === 'structures/_destroy') return f(object, scope, call.attackType);
  if (call.fn === 'nukes/pretick') return f(object, call.intents, scope);
  return f(object, fresh(call.intent), scope);
}

// The vanilla files a case runs, as "<dir>/<file>.js" tags (tools/parity-report.js counts them): each
// call's file, the per-intent files an intents.js dispatches to (camelCase key → kebab-case file), and
// the helpers they call.
const HELPERS = {
  'spawns/create-creep.js': ['spawns/_charge-energy.js'],
  'spawns/renew-creep.js': ['spawns/_charge-energy.js'],
  'spawns/tick.js': ['spawns/_born-creep.js'],
  'room/destroy-structure.js': ['structures/_destroy.js'],
};
function tags(c) {
  const out = new Set();
  const addFile = (f) => {
    if (out.has(f)) return;
    out.add(f);
    for (const h of HELPERS[f] || []) addFile(h);
  };
  for (const call of c.calls) {
    addFile(call.fn === '_calc_spawns' ? 'intents/_calc_spawns.js' : `${call.fn}.js`);
    if (call.fn.endsWith('/intents')) {
      const dir = call.fn.split('/')[0];
      for (const k of Object.keys(call.intent)) {
        const f = `${dir}/${k.replace(/[A-Z]/g, (m) => '-' + m.toLowerCase())}.js`;
        if (require('fs').existsSync(reference(`engine/src/processor/intents/${dir}`) + '/' + f.split('/')[1])) addFile(f);
      }
    }
  }
  return [...out];
}

const realRandom = Math.random, realNow = Date.now;
function run(c) {
  captured = {};
  notifications = [];
  worldSize = c.worldSize;
  const stats = [];
  const roomObjects = {};
  for (const o of fresh(c.objects)) roomObjects[o._id] = o;
  const users = {};
  for (const u of fresh(c.users)) users[u._id] = u;
  const flags = fresh(c.flags);
  const roomInfo = fresh(c.roomInfo);
  let roomController;
  _.forEach(roomObjects, (o) => {
    if (o.type == 'controller') roomController = o;
  });
  const bulk = makeBulk('rooms.objects'), bulkUsers = makeBulk('users'), bulkFlags = makeBulk('rooms.flags');
  const eventLog = [];
  const calls = fresh(c.calls); // nukes/pretick mutates its intents: recorded below
  const scope = {
    roomObjects, roomTerrain: c.terrain, bulk, bulkUsers, bulkUsersPowerCreeps: makeBulk('users.power_creeps'),
    stats: { inc: (name, userId, amount) => stats.push([name, userId, amount]) },
    flags, bulkFlags, gameTime: c.gameTime, roomInfo, users, eventLog, roomController, energyAvailable: c.energyAvailable,
  };
  movement.init(roomObjects, c.terrain);
  Math.random = roomStream(c.worldSeed, c.gameTime, c.room).random;
  Date.now = () => c.vnow;
  let crash = null;
  try {
    for (const call of calls) invoke(call, scope);
  } catch (e) {
    crash = e.message;
  } finally {
    Math.random = realRandom;
    Date.now = realNow;
  }
  bulk.execute();
  bulkUsers.execute();
  bulkFlags.execute();
  return {
    name: c.name,
    tags: tags(c),
    worldSeed: c.worldSeed, gameTime: c.gameTime, room: c.room, vnow: c.vnow, worldSize: c.worldSize, terrain: c.terrain === plain() ? null : c.terrain,
    energyAvailable: c.energyAvailable,
    roomInfo: encode(c.roomInfo), users: encode(c.users), flags: encode(c.flags), objects: encode(c.objects), calls: encode(c.calls),
    expect: {
      crash,
      ops: { objects: captured['rooms.objects'] || [], users: captured.users || [], flags: captured['rooms.flags'] || [] },
      state: encode(roomObjects), flags: encode(flags), users: encode(users), roomInfo: encode(roomInfo),
      eventLog: encode(eventLog), notifications, stats, calls: encode(calls),
    },
  };
}

writeFixture('structures/cases.ndjson', cases.map(run));
