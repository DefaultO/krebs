// Scenario builder for the room-processor oracle: rooms with terrain, objects, users and intents, emitted
// in the FROZEN scenario format of ARCHITECTURE §12.2 (read by crates/engine/src/scenario.rs). Object
// factories reproduce the document shapes vanilla creates (create-creep.js, build.js, game.js place-spawn,
// cli/map.js); every field can be overridden. Intents are sanitized by the official common/lib/system.js
// exactly as the runner stores them, and users are ordered by ORD-POLICY (lib/rng.js) unless `order` is
// given. Case modules (tools/oracle/cases/<area>.js) export arrays of `build()` results:
//
//   const { scenario, C } = require('../lib/scenario');
//   const s = scenario('harvest: full source', { tags: ['creeps/harvest.js:20'] });
//   const u = s.user('u1'); s.controller(20, 20, { user: u, level: 2 });
//   const src = s.source(10, 10), c = s.creep(11, 10, { user: u, body: 'work,carry,move' });
//   s.intent(u, c, 'harvest', { id: src });
//   module.exports = [s.build()];
//
// Porters keep case modules in their own fixture area (crates/engine/tests/fixtures/<area>/<name>.js,
// requiring this file by its path from there) and run `node22 tools/oracle/processor.js gen <that file>`,
// which writes <name>.ndjson next to it; tests/oracle_processor.rs finds every such fixture. Also here:
// globalScenario (rooms, terminals, orders, power creeps, users.intents docs → tools/oracle/global.js),
// cronScenario (one cron job, env → tools/oracle/cron.js), and the fuzz generators random/randomGlobal.
'use strict';
const { driver } = require('./fake-driver');
const rng = require('./rng');
const { prng } = require('./common');

const C = driver.constants;
const system = driver.system;
const NPC = {
  2: { _id: '2', username: 'Invader', usernameLower: 'invader', gcl: 0, cpu: 0, active: 1, cpuAvailable: 10000 },
  3: { _id: '3', username: 'Source Keeper', usernameLower: 'source keeper', gcl: 0, cpu: 0, active: 1, cpuAvailable: 10000 },
};
const ACTION_LOG = { attacked: null, healed: null, attack: null, rangedAttack: null, rangedMassAttack: null, rangedHeal: null,
  harvest: null, heal: null, repair: null, build: null, say: null, upgradeController: null, reserveController: null };

// 'work,carry,move' | ['work', {type:'move', boost:'ZO'}] → creep body.
function body(spec) {
  const parts = typeof spec === 'string' ? spec.split(',').map((s) => s.trim()) : spec;
  return parts.map((p) => (typeof p === 'string' ? { type: p, hits: 100 } : Object.assign({ type: p.type, hits: 100 }, p)));
}

// Terrain: '0'/'1'/'2' row-major (y*50+x). Default: plain with border walls and an exit span 20..29 on
// each side. `walls`/`swamps`: [[x,y], …] or a predicate (x, y) → bool.
function terrain({ walls = [], swamps = [], exits = true } = {}) {
  const t = [];
  const has = (set, x, y) => (typeof set === 'function' ? set(x, y) : set.some(([a, b]) => a === x && b === y));
  for (let y = 0; y < 50; y++) {
    for (let x = 0; x < 50; x++) {
      const edge = x === 0 || y === 0 || x === 49 || y === 49;
      const exit = exits && edge && (x >= 20 && x <= 29 || y >= 20 && y <= 29) && !(x % 49 === 0 && y % 49 === 0);
      t.push(has(walls, x, y) || (edge && !exit) ? '1' : has(swamps, x, y) ? '2' : '0');
    }
  }
  return t.join('');
}

class Scenario {
  constructor(name, opts = {}) {
    this.name = name;
    this.tags = opts.tags || [];
    this.room = opts.room || 'W1N1';
    this.gameTime = opts.time || 1000;
    this.worldSeed = opts.worldSeed || '0x1';
    this.clock = opts.clock || { baseMs: 1700000000000, baseTick: 0, nominalTickMs: 1000 };
    this.roomDoc = Object.assign({ status: 'normal' }, opts.roomDoc);
    this.terrainStr = typeof opts.terrain === 'string' ? opts.terrain : terrain(opts.terrain);
    this.worldSize = opts.worldSize || 12;
    this.accessibleRooms = opts.accessibleRooms || [this.room];
    this.compat = opts.compat || 'private';
    this.order = opts.order || null;
    this.objects = [];
    this.flags = [];
    this.users = {};
    this.intents = new Map(); // uid → Map(objectId → raw intents | room raw arrays)
    this.manual = new Map();
    this.counter = 0;
  }

  id(prefix) {
    return `${prefix}${++this.counter}`;
  }

  user(id, fields) {
    if (!this.users[id]) {
      this.users[id] = NPC[id] ? Object.assign({}, NPC[id]) : { _id: id, username: `User_${id}`, usernameLower: `user_${id}`,
        cpu: 100, gcl: 0, cpuAvailable: 10000, active: 10000, rooms: [] };
    }
    Object.assign(this.users[id], fields);
    return id;
  }

  // Any object: {_id, type, room, x, y, …fields}. Returns its id.
  object(type, x, y, fields = {}) {
    const doc = Object.assign({ _id: fields._id || this.id(type.slice(0, 4)), type, room: this.room, x, y }, fields);
    if (doc.user) this.user(doc.user);
    this.objects.push(doc);
    return doc._id;
  }

  creep(x, y, { user = 'u1', name, body: b = 'work,carry,move', store = {}, ...rest } = {}) {
    const parts = body(b);
    const carry = parts.filter((p) => p.type === C.CARRY).length * C.CARRY_CAPACITY;
    return this.object('creep', x, y, Object.assign({
      name: name || `creep${this.counter + 1}`, user, body: parts, store, storeCapacity: carry,
      hits: parts.length * 100, hitsMax: parts.length * 100, spawning: false, fatigue: 0, notifyWhenAttacked: true,
      ageTime: this.gameTime + C.CREEP_LIFE_TIME, actionLog: Object.assign({}, ACTION_LOG),
    }, rest));
  }

  spawn(x, y, { user = 'u1', name, energy = C.SPAWN_ENERGY_START, ...rest } = {}) {
    return this.object('spawn', x, y, Object.assign({
      name: name || `Spawn${this.counter + 1}`, user, store: { energy }, storeCapacityResource: { energy: C.SPAWN_ENERGY_CAPACITY },
      hits: C.SPAWN_HITS, hitsMax: C.SPAWN_HITS, spawning: null, notifyWhenAttacked: true, off: false,
    }, rest));
  }

  extension(x, y, { user = 'u1', energy = 0, level = 2, ...rest } = {}) {
    return this.object('extension', x, y, Object.assign({
      user, store: { energy }, storeCapacityResource: { energy: C.EXTENSION_ENERGY_CAPACITY[level] },
      hits: C.EXTENSION_HITS, hitsMax: C.EXTENSION_HITS, notifyWhenAttacked: true, off: false,
    }, rest));
  }

  source(x, y, { energy = C.SOURCE_ENERGY_CAPACITY, ...rest } = {}) {
    return this.object('source', x, y, Object.assign({
      energy, energyCapacity: C.SOURCE_ENERGY_CAPACITY, ticksToRegeneration: C.ENERGY_REGEN_TIME, invaderHarvested: 0,
    }, rest));
  }

  // Owned when `user` is given (place-spawn shape), else a neutral level-0 controller.
  controller(x, y, { user, level = user ? 1 : 0, ...rest } = {}) {
    const owned = user ? { user, progress: 0, downgradeTime: this.gameTime + C.CONTROLLER_DOWNGRADE[level || 1], safeMode: null, safeModeAvailable: 1 } : {};
    if (user) this.user(user);
    return this.object('controller', x, y, Object.assign({ level }, owned, rest));
  }

  mineral(x, y, { mineralType = 'H', density = 3, ...rest } = {}) {
    return this.object('mineral', x, y, Object.assign({ mineralType, density, mineralAmount: C.MINERAL_DENSITY[density] }, rest));
  }

  site(x, y, structureType, { user = 'u1', progress = 0, ...rest } = {}) {
    return this.object('constructionSite', x, y, Object.assign({
      structureType, user, progress, progressTotal: C.CONSTRUCTION_COST[structureType],
    }, rest));
  }

  drop(x, y, amount, resourceType = 'energy', rest = {}) {
    return this.object('energy', x, y, Object.assign({ [resourceType]: amount, resourceType }, rest));
  }

  container(x, y, { store = { energy: 0 }, ...rest } = {}) {
    return this.object('container', x, y, Object.assign({
      store, storeCapacity: C.CONTAINER_CAPACITY, hits: C.CONTAINER_HITS, hitsMax: C.CONTAINER_HITS,
      nextDecayTime: this.gameTime + C.CONTAINER_DECAY_TIME, notifyWhenAttacked: true,
    }, rest));
  }

  road(x, y, rest = {}) {
    return this.object('road', x, y, Object.assign({
      hits: C.ROAD_HITS, hitsMax: C.ROAD_HITS, nextDecayTime: this.gameTime + C.ROAD_DECAY_TIME, notifyWhenAttacked: true,
    }, rest));
  }

  wall(x, y, rest = {}) {
    return this.object('constructedWall', x, y, Object.assign({ hits: C.WALL_HITS, hitsMax: C.WALL_HITS_MAX, notifyWhenAttacked: true }, rest));
  }

  rampart(x, y, { user = 'u1', ...rest } = {}) {
    return this.object('rampart', x, y, Object.assign({
      user, hits: C.RAMPART_HITS, hitsMax: C.RAMPART_HITS_MAX[2], nextDecayTime: this.gameTime + C.RAMPART_DECAY_TIME, notifyWhenAttacked: true,
    }, rest));
  }

  storage(x, y, { user = 'u1', store = { energy: 0 }, ...rest } = {}) {
    return this.object('storage', x, y, Object.assign({
      user, store, storeCapacity: C.STORAGE_CAPACITY, hits: C.STORAGE_HITS, hitsMax: C.STORAGE_HITS, notifyWhenAttacked: true,
    }, rest));
  }

  tower(x, y, { user = 'u1', energy = 0, ...rest } = {}) {
    return this.object('tower', x, y, Object.assign({
      user, store: { energy }, storeCapacityResource: { energy: C.TOWER_CAPACITY }, hits: C.TOWER_HITS, hitsMax: C.TOWER_HITS,
      notifyWhenAttacked: true, actionLog: { attack: null, heal: null, repair: null },
    }, rest));
  }

  terminal(x, y, { user = 'u1', store = { energy: 0 }, ...rest } = {}) {
    return this.object('terminal', x, y, Object.assign({
      notifyWhenAttacked: true, user, store, storeCapacity: C.TERMINAL_CAPACITY, hits: C.TERMINAL_HITS, hitsMax: C.TERMINAL_HITS,
    }, rest));
  }

  tombstone(x, y, { user = 'u1', store = { energy: 0 }, ...rest } = {}) {
    return this.object('tombstone', x, y, Object.assign({
      user, deathTime: this.gameTime - 1, decayTime: this.gameTime + 15, creepId: 'deadcreep', creepName: 'dead',
      creepTicksToLive: 1, creepBody: ['work', 'carry', 'move'], store,
    }, rest));
  }

  flag(user, name, x, y, color = 1, secondaryColor = 1) {
    this.user(user);
    let f = this.flags.find((d) => d.user === user);
    if (!f) this.flags.push((f = { _id: this.id('flags'), user, room: this.room, data: '' }));
    f.data += `${f.data ? '|' : ''}${name}~${color}~${secondaryColor}~${x}~${y}`;
    return name;
  }

  // Runtime intents (`intents.set(id, name, args)`): sanitized at build(). Object order = first set.
  intent(uid, objectId, name, args = {}) {
    this.user(uid);
    const objs = this.intents.get(uid) || this.intents.set(uid, new Map()).get(uid);
    const o = objs.get(objectId) || objs.set(objectId, {}).get(objectId);
    o[name] = args;
    return this;
  }

  // Room intents (`intents.pushByName('room', name, args)`).
  roomIntent(uid, name, args = {}) {
    this.user(uid);
    const objs = this.intents.get(uid) || this.intents.set(uid, new Map()).get(uid);
    const o = objs.get('room') || objs.set('room', {}).get('room');
    (o[name] = o[name] || []).push(Object.assign({ roomName: this.room }, args));
    return this;
  }

  // API add-object-intent (INT-MANUAL): `objectsManual[id] = sanitize({name: args})`, last call wins.
  manualIntent(uid, objectId, name, args = {}) {
    this.user(uid);
    const objs = this.manual.get(uid) || this.manual.set(uid, {}).get(uid);
    objs[objectId] = system.sanitizeUserIntents({ [name]: args });
    return this;
  }

  build() {
    const uids = [...new Set([...this.intents.keys(), ...this.manual.keys()])];
    const order = this.order || rng.sortUsers(this.worldSeed, BigInt(this.gameTime), uids);
    const intents = order.filter((u) => uids.includes(u)).map((uid) => {
      let objects = null;
      if (this.intents.has(uid)) {
        objects = {};
        for (const [id, raw] of this.intents.get(uid)) {
          if (id === 'room') {
            const r = {};
            system.sanitizeUserRoomIntents(raw, r);
            if (r[this.room]) objects.room = r[this.room].room;
          } else objects[id] = system.sanitizeUserIntents(raw);
        }
      }
      return [uid, { objects, objectsManual: this.manual.get(uid) || null }];
    });
    const scenario = {
      kind: 'room', worldSeed: this.worldSeed, clock: this.clock, gameTime: this.gameTime, room: this.room,
      roomDoc: this.roomDoc, terrain: this.terrainStr, objects: this.objects, flags: this.flags, users: this.users,
      intents, accessibleRooms: this.accessibleRooms, roomStatusData: {}, worldSize: this.worldSize, compat: this.compat,
      tags: this.tags,
    };
    return { name: this.name, scenario: JSON.parse(JSON.stringify(scenario)) };
  }
}

const scenario = (name, opts) => new Scenario(name, opts);

// Global-stage scenarios (tools/oracle/global.js): several rooms, market orders, power creeps and
// users.intents docs. Room objects take the room factories with `{room}` in their options.
class GlobalScenario extends Scenario {
  constructor(name, opts = {}) {
    super(name, opts);
    this.rooms = [];
    this.db = { 'market.orders': [], 'users.power_creeps': [], 'users.intents': [], transactions: [], 'users.money': [], 'users.resources': [] };
  }

  addRoom(name, fields = {}) {
    this.rooms.push(Object.assign({ _id: name, status: 'normal' }, fields));
    return name;
  }

  user(id, fields) {
    return super.user(id, Object.assign(this.users[id] ? {} : { money: 0, power: 0 }, fields));
  }

  // market.orders doc as createOrder stores it (prices ×1000).
  addOrder({ user = 'u1', type = 'sell', resourceType = 'energy', price = 1000, totalAmount = 1000, roomName = 'W1N1', ...rest } = {}) {
    if (user) this.user(user);
    const doc = Object.assign({ _id: this.id('order'), createdTimestamp: this.clock.baseMs, user, active: true, type, amount: totalAmount,
      remainingAmount: totalAmount, resourceType, price, totalAmount, roomName, created: this.gameTime - 1 }, rest);
    if (!user) delete doc.user;
    this.db['market.orders'].push(doc);
    return doc._id;
  }

  // users.power_creeps doc as createPowerCreep stores it.
  powerCreep(user, name, { className = 'operator', ...rest } = {}) {
    this.user(user);
    const doc = Object.assign({ _id: this.id('pc'), name, className, user, level: 0, hitsMax: 1000, store: {}, storeCapacity: 100,
      spawnCooldownTime: 0, powers: {} }, rest);
    this.db['users.power_creeps'].push(doc);
    return doc._id;
  }

  // One users.intents doc ({user, intents}), sanitized like the runner's storeIntents `global` branch.
  // Docs are processed in call order.
  globalIntents(uid, raw) {
    this.user(uid);
    this.db['users.intents'].push({ _id: this.id('ui'), user: uid, intents: system.sanitizeUserIntents(raw) });
    return this;
  }

  build() {
    if (!this.rooms.some((r) => r._id === this.room) && this.objects.some((o) => o.room === this.room)) this.addRoom(this.room);
    const scenario = {
      kind: 'global', worldSeed: this.worldSeed, clock: this.clock, gameTime: this.gameTime, worldSize: this.worldSize,
      compat: this.compat, tags: this.tags,
      db: Object.assign({ rooms: this.rooms, 'rooms.objects': this.objects, users: Object.values(this.users) }, this.db),
    };
    return { name: this.name, scenario: JSON.parse(JSON.stringify(scenario)) };
  }
}

const globalScenario = (name, opts) => new GlobalScenario(name, opts);

// Cron scenarios (tools/oracle/cron.js): a world fragment plus env keys for one job at game time G
// (`time`). Every added room also gets a rooms.terrain doc (`terrain` option of addRoom: a string or the
// terrain() options).
class CronScenario extends GlobalScenario {
  constructor(name, job, opts = {}) {
    super(name, opts);
    this.job = job;
    // chitin's active set always exists: vanilla's missing ACTIVE_ROOMS key makes `$nin: undefined` throw.
    this.env = Object.assign({ activeRooms: [] }, opts.env);
    this.db['rooms.terrain'] = [];
  }

  addRoom(name, { terrain: t, ...fields } = {}) {
    this.db['rooms.terrain'].push({ _id: this.id('terrain'), room: name, terrain: typeof t === 'string' ? t : terrain(t), type: 'terrain' });
    return super.addRoom(name, fields);
  }

  build() {
    const { scenario: g } = super.build();
    const scenario = { kind: 'cron', worldSeed: g.worldSeed, clock: g.clock, gameTime: g.gameTime, job: this.job, db: g.db,
      env: JSON.parse(JSON.stringify(this.env)), compat: g.compat, tags: g.tags };
    return { name: this.name, scenario };
  }
}

const cronScenario = (name, job, opts) => new CronScenario(name, job, opts);

// Real rooms with random intents: a recorded room scenario (fixtures/oracle/recorded/rooms.ndjson, from
// record/run.js) keeps its state, its owners' creeps and spawns get random M1 intents at nearby objects.
let recordedRooms = null;
function randomRecorded(seed, i) {
  if (!recordedRooms) {
    const f = require('path').join(__dirname, '../../../crates/engine/tests/fixtures/oracle/recorded/rooms.ndjson');
    recordedRooms = require('fs').readFileSync(f, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l).scenario);
  }
  const g = prng((seed * 6151 + i) >>> 0);
  const pick = (a) => a[g.int(a.length)];
  const base = JSON.parse(JSON.stringify(recordedRooms[(seed * 31 + i) % recordedRooms.length]));
  const near = (o) => base.objects.filter((t) => t !== o && Math.abs(t.x - o.x) <= 1 && Math.abs(t.y - o.y) <= 1);
  const intents = new Map(g.chance(0.5) ? base.intents : []);
  for (const o of base.objects) {
    if (!o.user || o.user === '2' || o.user === '3' || !g.chance(0.7)) continue;
    const uid = o.user;
    const objs = Object.assign({}, (intents.get(uid) || {}).objects);
    const nb = near(o);
    const t = nb.length && g.chance(0.8) ? pick(nb) : pick(base.objects);
    let raw = null;
    if (o.type === 'creep') {
      const name = pick(['move', 'harvest', 'transfer', 'withdraw', 'pickup', 'drop', 'build', 'repair', 'upgradeController', 'say']);
      raw = { [name]: { move: { direction: 1 + g.int(8) }, drop: { resourceType: 'energy', amount: g.int(60) }, say: { message: 'x', isPublic: true } }[name] ||
        { id: t._id, resourceType: 'energy', amount: g.int(60) } };
    } else if (o.type === 'spawn' && !o.spawning) {
      raw = { createCreep: { name: `f${seed}_${i}`, body: pick([['work', 'carry', 'move'], ['move'], ['work', 'work', 'carry', 'move']]) } };
    }
    if (!raw) continue;
    objs[o._id] = system.sanitizeUserIntents(raw);
    intents.set(uid, { objects: objs, objectsManual: null });
  }
  const order = rng.sortUsers(base.worldSeed, BigInt(base.gameTime), [...intents.keys()]);
  base.intents = order.map((u) => [u, intents.get(u)]);
  base.tags = ['fuzz', 'recorded'];
  return { name: `recorded fuzz ${seed}/${i}`, scenario: base };
}

// A random global stage: creeps crossing room edges, terminal sends, orders and deals between 2–3 players.
function randomGlobal(seed, i) {
  const g = prng((seed * 104729 + i) >>> 0);
  const pick = (a) => a[g.int(a.length)];
  const s = globalScenario(`global fuzz ${seed}/${i}`, { time: 100 + g.int(100000), worldSeed: `0x${(seed + 1).toString(16)}` });
  const rooms = ['W1N1', 'W2N1', 'W1N2', 'W5N5'].map((r) => s.addRoom(r, g.chance(0.1) ? { status: 'out of borders' } : {}));
  const users = ['u1', 'u2', 'u3'];
  users.forEach((u) => s.user(u, { money: g.int(5) * 1000000 }));
  const res = ['energy', 'H', 'O', 'power'];
  const terminals = {};
  for (const room of rooms) {
    if (g.chance(0.8)) {
      const store = { energy: g.int(50000) };
      store[pick(res)] = g.int(5000);
      terminals[room] = s.terminal(20, 20, { room, user: pick(users), store, cooldownTime: g.chance(0.2) ? s.gameTime + 5 : undefined });
    }
  }
  for (const room of Object.keys(terminals)) {
    const t = s.objects.find((o) => o._id === terminals[room]);
    if (g.chance(0.4)) t.send = { targetRoomName: pick(rooms), resourceType: pick(res), amount: g.int(3000), description: g.chance(0.5) ? 'a<b' : undefined };
  }
  for (let k = g.int(3); k > 0; k--) {
    const room = pick(rooms);
    s.creep(0, 25, { room, user: pick(users), interRoom: { room: pick(rooms), x: 49, y: 25 } });
  }
  // PWR_OPERATE_TERMINAL effects (active or expired, every level)
  for (const id of Object.values(terminals)) {
    if (g.chance(0.3)) s.objects.find((o) => o._id === id).effects = [{ power: C.PWR_OPERATE_TERMINAL, level: 1 + g.int(5), endTime: s.gameTime + (g.chance(0.7) ? 10 : -1) }];
  }
  // intershard resources (direct deals), power levels, experimentation
  for (const u of users) {
    if (g.chance(0.5)) s.user(u, { resources: { pixel: g.int(3000) } });
    if (g.chance(0.5)) s.user(u, { power: g.int(10) * g.int(10) * 1000 });
    if (g.chance(0.1)) s.user(u, { powerExperimentationTime: s.clock.baseMs + (g.chance(0.5) ? 1e12 : -1) });
  }
  const orders = [];
  for (let k = g.int(4); k > 0; k--) {
    const expired = g.chance(0.15) ? { createdTimestamp: s.clock.baseMs - 31 * 86400000 } : {};
    orders.push(s.addOrder(Object.assign({ user: pick(users), type: pick(['sell', 'buy']), resourceType: pick(res.concat('pixel')), price: 1 + g.int(5000),
      totalAmount: g.int(5000), roomName: pick(Object.keys(terminals).concat(rooms)) }, expired)));
  }
  // power spawns and power creeps (unspawned in users.power_creeps, some spawned into rooms, some dying)
  const spawns = [];
  for (const room of rooms) {
    if (g.chance(0.4)) spawns.push(s.object('powerSpawn', 22, 22, { room, user: pick(users), store: { energy: 0, power: 0 }, hits: 5000, hitsMax: 5000 }));
  }
  const pcs = [];
  for (const u of users) {
    for (let k = g.int(3); k > 0; k--) {
      const level = g.int(4), name = `PC${k}`, spawned = g.chance(0.4);
      const powers = level ? { 1: { level } } : {};
      const id = s.powerCreep(u, name, { level, hitsMax: 1000 + 1000 * level, storeCapacity: 100 + 100 * level, powers,
        spawnCooldownTime: spawned ? null : (g.chance(0.3) ? s.clock.baseMs + 1e12 : 0), ...(g.chance(0.2) ? { deleteTime: s.clock.baseMs + 1e9 } : {}) });
      if (spawned) {
        s.object('powerCreep', g.chance(0.3) ? 22 : 10 + k, 22, { _id: id, room: pick(rooms), user: u, name, className: 'operator', level, powers,
          hits: g.chance(0.2) ? 0 : 1000 + 1000 * level, hitsMax: 1000 + 1000 * level, storeCapacity: 100 + 100 * level, ageTime: s.gameTime + (g.chance(0.2) ? 1 : 3000),
          store: g.chance(0.5) ? { ops: g.int(50) } : {}, actionLog: g.chance(0.3) ? { say: { message: 'hi', isPublic: g.chance(0.5) } } : {} });
      }
      pcs.push({ u, id, name });
    }
  }
  for (const u of users) {
    const raw = {};
    const mine = pcs.filter((p) => p.u === u);
    if (g.chance(0.3)) raw.createPowerCreep = [{ name: pick(['A', 'B', 'PC1']), className: pick(['operator', 'operator', 'x']) }];
    if (mine.length && g.chance(0.3)) raw.spawnPowerCreep = [{ id: pick(spawns.concat('none')), name: pick(mine).name }];
    if (mine.length && g.chance(0.3)) raw.upgradePowerCreep = [{ id: pick(mine).id, power: pick([1, 1, 2, 3, 99]) }];
    if (mine.length && g.chance(0.2)) raw.deletePowerCreep = [{ id: pick(mine).id, cancel: g.chance(0.3) }];
    if (mine.length && g.chance(0.2)) raw.renamePowerCreep = [{ id: pick(mine).id, name: pick(['R', 'PC1', 'x'.repeat(60)]) }];
    if (mine.length && g.chance(0.2)) raw.suicidePowerCreep = [{ id: pick(mine).id }];
    if (g.chance(0.5)) raw.createOrder = [{ type: pick(['sell', 'buy']), resourceType: pick(res), price: g.int(5) + 0.001, totalAmount: g.int(2000), roomName: pick(rooms) }];
    if (orders.length && g.chance(0.6)) raw.deal = [{ orderId: pick(orders), amount: g.int(3000), targetRoomName: pick(rooms) }];
    if (orders.length && g.chance(0.2)) raw.cancelOrder = [{ orderId: pick(orders) }];
    if (orders.length && g.chance(0.2)) raw.changeOrderPrice = [{ orderId: pick(orders), newPrice: g.int(6) + 0.5 }];
    if (orders.length && g.chance(0.2)) raw.extendOrder = [{ orderId: pick(orders), addAmount: g.int(500) }];
    if (Object.keys(raw).length) s.globalIntents(u, raw);
  }
  s.tags.push('fuzz');
  return s.build();
}

// A random room exercising the M1 handlers (fuzz mode): owned room with spawns/extensions/sources, a few
// creeps of two players, drops, sites, roads, containers, and random M1 intents aimed at nearby objects.
function random(seed, i) {
  const g = prng((seed * 7919 + i) >>> 0);
  const pick = (a) => a[g.int(a.length)];
  const s = scenario(`fuzz ${seed}/${i}`, { time: 100 + g.int(100000), worldSeed: `0x${(seed + 1).toString(16)}`,
    terrain: { swamps: (x, y) => (x * 7 + y * 13 + i) % 11 === 0, walls: (x, y) => (x * 3 + y * 5 + i) % 37 === 0 } });
  const users = ['u1', 'u2'];
  const level = 1 + g.int(4);
  const ctrl = s.controller(25, 25, { user: 'u1', level, progress: g.int(C.CONTROLLER_LEVELS[level] || 1000) });
  const xy = () => [3 + g.int(44), 3 + g.int(44)];
  const near = ([x, y]) => [Math.max(1, Math.min(48, x + g.int(3) - 1)), Math.max(1, Math.min(48, y + g.int(3) - 1))];
  const sources = [s.source(...xy(), { energy: g.int(3001) }), s.source(...xy(), { energy: g.int(3001) })];
  const spawns = [s.spawn(...xy(), { energy: g.int(301) })];
  const exts = [];
  for (let k = g.int(4); k > 0; k--) exts.push(s.extension(...xy(), { energy: g.int(51), level }));
  const drops = [];
  for (let k = g.int(3); k > 0; k--) drops.push(s.drop(...xy(), 1 + g.int(500)));
  const sites = [];
  for (let k = g.int(3); k > 0; k--) sites.push(s.site(...xy(), pick(['extension', 'road', 'container', 'constructedWall']), { progress: g.int(50) }));
  const roads = [];
  for (let k = g.int(3); k > 0; k--) roads.push(s.road(...xy(), { hits: 1 + g.int(5000) }));
  const conts = [];
  for (let k = g.int(2); k > 0; k--) conts.push(s.container(...xy(), { store: { energy: g.int(2001) } }));
  const creeps = [];
  for (let k = 1 + g.int(5); k > 0; k--) {
    const user = pick(users);
    const parts = Array.from({ length: 1 + g.int(6) }, () => pick(['work', 'carry', 'move', 'move', 'carry']));
    const carry = parts.filter((p) => p === 'carry').length * 50;
    creeps.push([s.creep(...xy(), { user, body: parts, store: { energy: g.int(carry + 1) }, fatigue: g.chance(0.2) ? 2 : 0 }), user]);
  }
  const targets = [...sources, ...spawns, ...exts, ...drops, ...sites, ...roads, ...conts, ...creeps.map((c) => c[0])];
  const amount = () => (g.chance(0.1) ? undefined : g.int(100)); // the runtime always sends one; NaN → null edge
  for (const [c, user] of creeps) {
    // The first intent's target decides where the creep stands (mostly in range).
    let near0 = null;
    for (let k = 1 + g.int(3); k > 0; k--) {
      const name = pick(['move', 'harvest', 'transfer', 'withdraw', 'pickup', 'drop', 'build', 'repair', 'upgradeController', 'say', 'suicide']);
      const t = pick(targets);
      const args = {
        move: { direction: 1 + g.int(8) },
        harvest: { id: pick(sources) },
        transfer: { id: t, resourceType: 'energy', amount: amount() },
        withdraw: { id: t, resourceType: 'energy', amount: amount() },
        pickup: { id: drops.length ? pick(drops) : t },
        drop: { resourceType: 'energy', amount: g.int(60) },
        build: { id: sites.length ? pick(sites) : t },
        repair: { id: pick([...roads, ...conts, t]) },
        upgradeController: { id: ctrl },
        say: { message: 'hi', isPublic: g.chance(0.5) },
        suicide: {},
      }[name];
      if (near0 === null) near0 = args.id || t;
      s.intent(user, c, name, args);
    }
    const doc = s.objects.find((o) => o._id === c);
    const tdoc = s.objects.find((o) => o._id === near0);
    if (tdoc && tdoc !== doc && g.chance(0.8)) [doc.x, doc.y] = near([tdoc.x, tdoc.y]);
  }
  if (g.chance(0.5)) s.intent('u1', spawns[0], 'createCreep', { name: `fz${i}`, body: ['work', 'carry', 'move'] });
  if (g.chance(0.3)) s.roomIntent('u1', 'createConstructionSite', { x: 2 + g.int(46), y: 2 + g.int(46), structureType: pick(['road', 'extension', 'container']) });
  s.tags.push('fuzz');
  return s.build();
}

// A random room for the NPC AI (fuzz mode "npc"): keepers at sources, invader squads (melee, ranged, healers,
// dismantlers; some damaged), a player's creeps, ramparts/walls/spawns, stale and fresh cached paths.
function randomNpc(seed, i) {
  const g = prng((seed * 15485863 + i) >>> 0);
  const pick = (a) => a[g.int(a.length)];
  const s = scenario(`npc fuzz ${seed}/${i}`, { time: 100 + g.int(100000), worldSeed: `0x${(seed + 1).toString(16)}`,
    terrain: { swamps: (x, y) => (x * 7 + y * 13 + i) % 9 === 0, walls: (x, y) => (x * 3 + y * 5 + i) % 23 === 0 || (x === 20 + (i % 9) && y > 10 && y < 40) } });
  const xy = () => [3 + g.int(44), 3 + g.int(44)];
  const near = ([x, y], r) => [Math.max(1, Math.min(48, x + g.int(2 * r + 1) - r)), Math.max(1, Math.min(48, y + g.int(2 * r + 1) - r))];
  const pack = (x, y) => String.fromCharCode(32 + ((x << 6) | y));
  const damage = (id) => {
    const doc = s.objects.find((o) => o._id === id);
    if (g.chance(0.3)) {
      doc.body.forEach((p, k) => { if (g.chance(0.4)) doc.body[k].hits = 0; });
      doc.hits = doc.body.reduce((a, p) => a + p.hits, 0);
    }
    if (g.chance(0.3)) {
      let [x, y] = [doc.x, doc.y];
      let path = '';
      for (let k = 1 + g.int(5); k > 0; k--) { [x, y] = near([x, y], 1); path += pack(x, y); }
      doc.memory_move = { dest: pack(...xy()), path, time: s.gameTime - g.int(60), ...(g.chance(0.5) ? { lastMove: s.gameTime - g.int(3) } : {}) };
    }
  };
  if (g.chance(0.5)) s.controller(25, 25, g.chance(0.6) ? { user: 'u1', level: 1 + g.int(4) } : {});
  const sources = [s.source(...xy()), s.source(...xy())];
  if (g.chance(0.5)) s.mineral(...xy());
  for (let k = g.int(3); k > 0; k--) s.spawn(...xy(), { user: 'u1' });
  for (let k = g.int(6); k > 0; k--) (g.chance(0.5) ? s.rampart(...xy(), { user: 'u1', isPublic: g.chance(0.3) }) : s.wall(...xy()));
  for (let k = g.int(4); k > 0; k--) s.road(...xy());
  if (g.chance(0.6)) {
    for (let k = 1 + g.int(2); k > 0; k--) {
      const srcId = pick(sources);
      const src = s.objects.find((o) => o._id === srcId);
      const id = s.creep(...near([src.x, src.y], g.chance(0.5) ? 1 : 6), { user: '3', name: `Keeper${i}_${k}`, body: KEEPER_BODY_FUZZ,
        ...(g.chance(0.3) ? { memory_sourceId: src._id } : {}), notifyWhenAttacked: false });
      damage(id);
    }
  }
  const squad = [];
  for (let k = g.int(4); k > 0; k--) {
    const b = pick(['move,attack,attack', 'move,ranged_attack,ranged_attack', 'move,heal,heal', 'work,attack,move', 'move,move,ranged_attack,heal', 'tough,move,attack']);
    const id = s.creep(...xy(), { user: '2', name: `invader_W1N1_${k}`, body: b, notifyWhenAttacked: false });
    damage(id);
    squad.push(id);
  }
  for (let k = g.int(4); k > 0; k--) {
    const atId = squad.length && g.chance(0.6) ? pick(squad) : null;
    const at = atId ? s.objects.find((o) => o._id === atId) : null;
    const id = s.creep(...(at ? near([at.x, at.y], 3) : xy()), { user: 'u1', body: pick(['work,carry,move', 'attack,move', 'ranged_attack,move', 'heal,move']) });
    damage(id);
    if (g.chance(0.3)) s.intent('u1', id, 'move', { direction: 1 + g.int(8) });
  }
  s.tags.push('fuzz', 'npc');
  return s.build();
}
const KEEPER_BODY_FUZZ = 'tough,tough,move,move,attack,attack,ranged_attack,ranged_attack';

// A random cron run (fuzz mode "cron"): one sector W0..W10 × N0..N10 with highways, SK and centre rooms, random
// terrain and objects, and one random job.
function randomCron(seed, i) {
  const g = prng((seed * 49979687 + i) >>> 0);
  const pick = (a) => a[g.int(a.length)];
  const jobs = ['roomsForceUpdate', 'genPowerBanks', 'genInvaders', 'purgeTransactions', 'recreateNpcOrders', 'calcMarketStats',
    'deletePowerCreeps', 'genDeposits', 'genStrongholds', 'expandStrongholds'];
  const job = jobs[i % jobs.length];
  const G = 1000 + g.int(100000);
  const s = cronScenario(`cron fuzz ${seed}/${i} ${job}`, job, { time: G, worldSeed: `0x${(seed + 1).toString(16)}` });
  const now = s.clock.baseMs + G * 1000;
  const names = [];
  for (let x = 0; x <= 10; x++) for (let y = 0; y <= 10; y++) if (g.chance(0.6) || x === 5 && y === 5) names.push(`W${x}N${y}`);
  const terrains = [];
  for (const name of names) {
    const [x, y] = [+name.slice(1, name.indexOf('N')), +name.slice(name.indexOf('N') + 1)];
    const bus = x % 10 === 0 || y % 10 === 0;
    const k = g.int(1000);
    const t = { walls: (tx, ty) => (tx === 0 || ty === 0 || tx === 49 || ty === 49) ? ((tx + ty + k) % 7 !== 0) : ((tx * 3 + ty * 5 + k) % 13 === 0 || (tx > 20 && tx < 30 && ty > 20 && ty < 26 && k % 3 === 0)) };
    const fields = {};
    if (bus) { fields.bus = true; if (g.chance(0.7)) fields.depositType = pick(['mist', 'metal', 'silicon', 'biomass']); }
    if (g.chance(0.08)) fields.status = 'out of borders';
    if (g.chance(0.1)) fields.novice = now + (g.chance(0.5) ? 1e9 : -1);
    if (g.chance(0.3)) fields.nextForceUpdateTime = G + g.int(40) - 20;
    if (g.chance(0.3)) fields.powerBankTime = G + g.int(200) - 100;
    if (g.chance(0.3)) fields.invaderGoal = pick([1, 1000, 100000]);
    if (g.chance(0.2)) fields.nextNpcMarketOrder = G + g.int(200) - 100;
    s.addRoom(name, Object.assign({ terrain: t }, fields));
    terrains.push({ room: name, terrain: s.db['rooms.terrain'][s.db['rooms.terrain'].length - 1].terrain });
    const center = x % 10 >= 4 && x % 10 <= 6 && y % 10 >= 4 && y % 10 <= 6;
    if (!bus && !(center && !(x % 10 === 5 && y % 10 === 5)) && g.chance(0.8)) {
      const owner = g.chance(0.4) ? { user: pick(['u1', 'u2']), level: 1 + g.int(8) } : (g.chance(0.2) ? { reservation: { user: pick(['u1', '2']), endTime: G + 100 } } : {});
      s.controller(10 + g.int(30), 10 + g.int(30), Object.assign({ room: name }, owner));
    }
    if (!bus) for (let n = 1 + g.int(2); n > 0; n--) s.source(5 + g.int(40), 5 + g.int(40), { room: name, invaderHarvested: g.int(120000) });
    if (g.chance(0.1)) s.creep(25, 25, { room: name, user: '2' });
    if (g.chance(0.1)) s.creep(26, 25, { room: name, user: 'u1', body: 'claim,move' });
    if (center && g.chance(job === 'genStrongholds' ? 0.04 : 0.3)) s.object('invaderCore', 20 + g.int(10), 20 + g.int(10), { room: name, user: '2', level: g.int(6), hits: 100000, hitsMax: 100000,
      strongholdId: `sh${name}`, effects: [], ...(g.chance(0.7) ? { deployTime: null } : {}), nextExpandTime: G + g.int(200) - 100 });
    if (center && g.chance(job === 'genStrongholds' ? 0.01 : 0.1)) s.object('ruin', 22, 22, { room: name, structure: { type: 'invaderCore', id: 'old', hits: 0, hitsMax: 100000 } });
    if (bus && g.chance(0.2)) s.object('deposit', 5 + g.int(40), 5 + g.int(40), { room: name, depositType: 'mist', harvested: g.int(80000), decayTime: G + 1000 });
    if (g.chance(0.15)) s.terminal(25, 25, { room: name, user: g.chance(0.5) ? null : 'u1', store: {} });
  }
  for (const u of ['u1', 'u2']) s.user(u, { power: g.int(20) * g.int(20) * 1000, money: g.int(5) * 1e6 });
  for (let n = g.int(4); n > 0; n--) s.powerCreep(pick(['u1', 'u2']), `pc${n}`, { level: g.int(5), ...(g.chance(0.7) ? { deleteTime: now + g.int(2000) - 1000 } : {}) });
  for (let n = g.int(3); n > 0; n--) s.addOrder({ user: g.chance(0.5) ? null : 'u1', roomName: pick(names), resourceType: pick(['H', 'O', 'energy']) });
  for (let n = g.int(250); n > 0; n--) s.db.transactions.push({ _id: `tx${n}`, time: g.int(2000), sender: pick(['u1', 'u2', undefined]), recipient: pick(['u1', 'u2']), resourceType: 'energy', amount: 1 });
  const day = 24 * 3600 * 1000;
  for (let n = g.int(12); n > 0; n--) s.db['users.money'].push({ _id: `m${n}`, date: new Date(now - g.int(20) * day - g.int(day)).toISOString(), tick: G, user: 'u1',
    type: pick(['market.sell', 'market.sell', 'market.buy']), balance: 1, change: g.int(100) / 10, market: { resourceType: pick(['H', 'energy']), price: g.int(50) / 10, amount: 1 + g.int(9) } });
  s.env.activeRooms = names.filter(() => g.chance(0.2));
  if (job === 'expandStrongholds') {
    const accessible = s.rooms.filter((r) => r.status === 'normal' && !r.openTime).map((r) => r._id);
    s.env.accessibleRooms = JSON.stringify(accessible);
    s.env.terrainData = require('zlib').deflateSync(JSON.stringify(terrains)).toString('base64');
  }
  s.tags.push('fuzz', 'cron');
  return s.build();
}


// A random stronghold room (fuzz mode "stronghold"): an invader core of any level, deploying or deployed, with
// towers, ramparts, roads, containers, defenders (some named like its population), hostiles, nukes.
function randomStronghold(seed, i) {
  const g = prng((seed * 32452843 + i) >>> 0);
  const pick = (a) => a[g.int(a.length)];
  const time = (100 + g.int(100000)) * (g.chance(0.3) ? 10 : 1);
  const s = scenario(`stronghold fuzz ${seed}/${i}`, { time, worldSeed: `0x${(seed + 1).toString(16)}`,
    terrain: { swamps: (x, y) => (x * 7 + y * 13 + i) % 9 === 0, walls: (x, y) => (x * 3 + y * 5 + i) % 29 === 0 } });
  const level = 1 + g.int(5);
  const [cx, cy] = [10 + g.int(30), 10 + g.int(30)];
  const near = (r) => [cx + g.int(2 * r + 1) - r, cy + g.int(2 * r + 1) - r];
  const sid = 'sh1';
  const deploying = g.chance(0.25);
  const behavior = g.chance(0.1) ? 'default' : `bunker${level}`;
  const pops = { 2: [['weakDefender', 'simple-melee']], 3: [['fullDefender', 'simple-melee'], ['fullDefender', 'simple-melee']],
    4: [['fortifier', 'fortifier'], ['boostedDefender', 'coordinated'], ['boostedRanger', 'coordinated'], ['boostedDefender', 'coordinated']],
    5: [['fortifier', 'fortifier'], ['fullBoostedMelee', 'coordinated'], ['fullBoostedRanger', 'coordinated'], ['fullBoostedRanger', 'coordinated']] };
  const population = pops[level] && g.chance(0.7) ? pops[level].map(([body, b]) => ({ body, behavior: b })) : undefined;
  if (g.chance(0.7)) s.controller(25, 25, g.chance(0.5) ? { user: '2', level: 8, downgradeTime: s.gameTime + g.int(6000) }
    : (g.chance(0.5) ? { reservation: { user: pick(['2', 'u1']), endTime: s.gameTime + g.int(5000) } } : { user: 'u1', level: 3 }));
  const core = s.object('invaderCore', cx, cy, { user: '2', level, strongholdId: sid, templateName: `bunker${level}`, depositType: pick(['silicon', 'metal', 'biomass', 'mist']),
    hits: 100000, hitsMax: 100000, actionLog: {}, strongholdBehavior: behavior,
    effects: deploying ? [] : [{ effect: 1002, power: 1002, endTime: s.gameTime + (g.chance(0.05) ? 0 : 50000), duration: 75000 }],
    ...(deploying ? { deployTime: s.gameTime + g.int(3) } : { decayTime: s.gameTime + 50000 }),
    ...(population ? { population } : {}),
    ...(g.chance(0.2) ? { spawning: { name: 'defender0', needTime: 100, spawnTime: s.gameTime + g.int(3) } } : {}) });
  for (let k = g.int(5); k > 0; k--) {
    const [x, y] = near(2);
    s.tower(x, y, { user: '2', energy: g.chance(0.5) ? g.int(30) : g.int(1000), strongholdId: sid, storeCapacityResource: { energy: 1000 } });
    if (g.chance(0.7)) s.rampart(x, y, { user: '2', strongholdId: sid, hits: g.int(1000000), hitsTarget: g.chance(0.6) ? 500000 : undefined });
  }
  for (let k = g.int(8); k > 0; k--) s.rampart(...near(3), { user: '2', strongholdId: sid, hits: 1 + g.int(1000000), ...(g.chance(0.6) ? { hitsTarget: 500000 } : {}) });
  for (let k = g.int(4); k > 0; k--) s.road(...near(2), { strongholdId: sid, hits: 1 + g.int(5000), hitsMax: 5000 });
  if (g.chance(0.5)) s.container(...near(2), { strongholdId: sid });
  if (g.chance(0.3)) s.object('nuke', ...near(2), { landTime: s.gameTime + 1000, launchRoomName: 'W9N9' });
  for (let k = g.int(4); k > 0; k--) {
    const pop = population && g.chance(0.6) ? `defender${g.int(population.length)}` : `sh${k}`;
    const b = pick(['attack,attack,move', 'ranged_attack,move', 'work,carry,move', 'attack,ranged_attack,move']);
    const id = s.creep(...near(3), { user: '2', name: pop, body: b, strongholdId: sid, notifyWhenAttacked: false, store: { energy: g.int(50) } });
    const doc = s.objects.find((o) => o._id === id);
    if (g.chance(0.3)) { doc.hits = Math.floor(doc.hits / 2); }
  }
  for (let k = g.int(4); k > 0; k--) s.creep(...(g.chance(0.6) ? near(5) : [3 + g.int(44), 3 + g.int(44)]), { user: 'u1', body: pick(['attack,move', 'ranged_attack,move', 'work,move', 'heal,move']) });
  if (deploying && g.chance(0.5)) s.site(...near(1), 'road', { progress: g.int(300) });
  if (deploying && g.chance(0.5)) { s.road(...near(1)); s.extension(...near(1), { user: 'u1', level: 3 }); s.creep(...near(1), { user: 'u1', body: 'work,carry,move', store: { energy: 40 } }); }
  if (!deploying && g.chance(0.2)) {
    s.objects.find((o) => o._id === core).hits = 1 + g.int(60);
    const a = s.creep(cx + 1, cy, { user: 'u1', body: 'attack,attack,move' });
    s.intent('u1', a, 'attack', { id: core, x: cx, y: cy });
  }
  s.tags.push('fuzz', 'stronghold');
  return s.build();
}

// A random room for power creeps (fuzz mode "pc"): operators of both players with random powers, levels,
// cooldowns and ops, every power target type (some with effects already), and the room-side power creep
// intents (usePower, enableRoom, renew, move, transfer, withdraw, drop, pickup, say), plus creeps hitting them.
function randomPc(seed, i) {
  const g = prng((seed * 86028121 + i) >>> 0);
  const pick = (a) => a[g.int(a.length)];
  const s = scenario(`pc fuzz ${seed}/${i}`, { time: 100 + g.int(100000), worldSeed: `0x${(seed + 1).toString(16)}`,
    terrain: { swamps: (x, y) => (x * 7 + y * 13 + i) % 9 === 0, walls: (x, y) => (x * 3 + y * 5 + i) % 31 === 0 } });
  const xy = () => [3 + g.int(44), 3 + g.int(44)];
  const near = ([x, y], r) => [Math.max(1, Math.min(48, x + g.int(2 * r + 1) - r)), Math.max(1, Math.min(48, y + g.int(2 * r + 1) - r))];
  const owner = pick(['u1', 'u1', 'u2']);
  const ctrl = g.chance(0.85) ? s.controller(25, 25, Object.assign({ user: owner, level: 1 + g.int(8), isPowerEnabled: g.chance(0.8) },
    g.chance(0.2) ? { safeMode: s.gameTime + g.int(200) - 50 } : {})) : null;
  const level = ctrl ? s.objects.find((o) => o._id === ctrl).level : 0;
  const effects = (power) => (g.chance(0.3) ? [{ effect: power, power, level: 1 + g.int(5), endTime: s.gameTime + g.int(200) - 50 }] : undefined);
  const withFx = (power, rest = {}) => { const e = effects(power); return e ? Object.assign({ effects: e }, rest) : rest; };
  const own = () => (g.chance(0.85) ? owner : pick(['u1', 'u2']));
  const targets = [];
  const add = (id) => { targets.push(id); return id; };
  // spawns or extensions without a controller crash vanilla's _calc_spawns for the whole room
  if (ctrl) add(s.spawn(...xy(), Object.assign({ user: own() }, withFx(pick([C.PWR_OPERATE_SPAWN, C.PWR_DISRUPT_SPAWN])))));
  for (let k = g.int(3); k > 0; k--) add(s.tower(...xy(), Object.assign({ user: own(), energy: g.int(1001) }, withFx(C.PWR_OPERATE_TOWER))));
  if (g.chance(0.7)) add(s.storage(...xy(), Object.assign({ user: own(), store: { energy: g.int(20000) } }, withFx(C.PWR_OPERATE_STORAGE))));
  if (g.chance(0.6)) add(s.terminal(...xy(), Object.assign({ user: own(), store: { energy: g.int(20000) } }, withFx(pick([C.PWR_OPERATE_TERMINAL, C.PWR_DISRUPT_TERMINAL])))));
  if (g.chance(0.5)) add(s.container(...xy(), { store: { energy: g.int(2001) } }));
  if (g.chance(0.5)) add(s.object('factory', ...xy(), Object.assign({ user: own(), store: { energy: g.int(1000) }, hits: 1000, hitsMax: 1000 },
    g.chance(0.5) ? { level: 1 + g.int(5) } : {}, withFx(C.PWR_OPERATE_FACTORY))));
  if (g.chance(0.5)) add(s.object('lab', ...xy(), Object.assign({ user: own(), store: { energy: 0 }, storeCapacityResource: { energy: 2000 }, storeCapacity: 3000, hits: 500, hitsMax: 500 }, withFx(C.PWR_OPERATE_LAB))));
  if (g.chance(0.4)) add(s.object('observer', ...xy(), { user: own(), hits: 500, hitsMax: 500 }));
  const pspawn = g.chance(0.6) ? add(s.object('powerSpawn', ...xy(), Object.assign({ user: own(), store: { energy: 0, power: 0 },
    storeCapacityResource: { energy: 5000, power: 100 }, hits: 5000, hitsMax: 5000 }, withFx(C.PWR_OPERATE_POWER)))) : null;
  const pbank = g.chance(0.2) ? add(s.object('powerBank', ...xy(), { store: { power: 3000 }, hits: 2000000, hitsMax: 2000000, decayTime: s.gameTime + 3000 })) : null;
  add(s.source(...xy(), withFx(pick([C.PWR_DISRUPT_SOURCE, C.PWR_REGEN_SOURCE]))));
  if (g.chance(0.7)) add(s.mineral(...xy(), Object.assign({ mineralAmount: g.chance(0.2) ? 0 : g.int(70000) },
    g.chance(0.2) ? { nextRegenerationTime: s.gameTime + 100 } : {}, withFx(C.PWR_REGEN_MINERAL))));
  for (let k = g.int(3); k > 0; k--) add(g.chance(0.5) ? s.rampart(...xy(), Object.assign({ user: own(), hits: 1 + g.int(100000) }, withFx(C.PWR_FORTIFY))) : s.wall(...xy(), withFx(C.PWR_FORTIFY)));
  for (let k = ctrl ? g.int(5) : 0; k > 0; k--) s.extension(...xy(), { user: own(), energy: g.int(51), level: Math.max(2, level), ...(g.chance(0.1) ? { off: true } : {}) });
  if (g.chance(0.3)) add(s.site(...xy(), 'road'));
  const drops = [];
  for (let k = g.int(2); k > 0; k--) drops.push(add(s.drop(...xy(), 1 + g.int(200), pick(['energy', 'ops']))));
  if (ctrl) targets.push(ctrl);
  if (g.chance(0.1)) add(s.object('portal', ...xy(), { destination: g.chance(0.5) ? { room: 'W2N1', x: 25, y: 25 } : { shard: 'shard1', room: 'W1N1' } }));
  const pcs = [];
  for (let k = 1 + g.int(3); k > 0; k--) {
    const user = pick(['u1', 'u1', 'u2']);
    const lvl = g.int(26);
    const powers = {};
    for (let n = 1 + g.int(6); n > 0; n--) {
      const p = g.chance(0.3) ? C.PWR_GENERATE_OPS : 1 + g.int(19);
      powers[p] = { level: g.int(6), ...(g.chance(0.4) ? { cooldownTime: s.gameTime + g.int(100) - 50 } : {}) };
    }
    const hitsMax = 1000 * (lvl + 1);
    const cap = 100 * (lvl + 1);
    const ops = g.chance(0.2) ? cap : g.int(Math.min(cap, 300) + 1);
    const at = g.chance(0.1) ? [pick([0, 49]), 20 + g.int(10)] : xy();
    const id = s.object('powerCreep', ...at, { user, name: `pc${i}_${k}`, className: 'operator', level: lvl, hits: g.chance(0.3) ? g.int(hitsMax) + 1 : hitsMax, hitsMax,
      store: g.chance(0.1) ? {} : { ops, ...(g.chance(0.3) ? { energy: g.int(50) } : {}) }, storeCapacity: cap, ageTime: s.gameTime + g.int(5000), powers,
      notifyWhenAttacked: true, actionLog: { spawned: null, attack: null, attacked: null, healed: null, power: null, say: null } });
    pcs.push([id, user, powers]);
  }
  // The target types each power accepts (usePower.js switch).
  const kinds = { 2: ['spawn'], 9: ['spawn'], 3: ['tower'], 10: ['tower'], 4: ['storage'], 5: ['lab'], 6: ['storage', 'terminal', 'factory', 'container'],
    7: ['observer'], 8: ['terminal'], 15: ['terminal'], 11: ['source'], 13: ['source'], 14: ['mineral'], 16: ['powerSpawn'],
    17: ['rampart', 'constructedWall'], 18: ['controller'], 19: ['factory'] };
  const typeOf = (id) => s.objects.find((o) => o._id === id).type;
  for (const [pc, user, powers] of pcs) {
    let first = null;
    for (let k = 1 + g.int(3); k > 0; k--) {
      const name = first === null && g.chance(0.7) ? 'usePower'
        : pick(['usePower', 'usePower', 'move', 'transfer', 'withdraw', 'drop', 'pickup', 'say', 'enableRoom', 'renew']);
      const owned = Object.keys(powers).map(Number);
      const power = g.chance(0.85) ? pick(owned) : g.int(21);
      const fit = targets.filter((id) => (kinds[power] || []).includes(typeOf(id)));
      const t = name === 'usePower' && fit.length && g.chance(0.8) ? pick(fit) : pick(targets);
      const args = {
        usePower: { power, id: t },
        move: { direction: 1 + g.int(8) },
        transfer: { id: t, resourceType: pick(['ops', 'energy']), amount: g.int(100) },
        withdraw: { id: t, resourceType: 'energy', amount: g.int(100) },
        drop: { resourceType: 'ops', amount: g.int(60) },
        pickup: { id: drops.length ? pick(drops) : t },
        say: { message: pick(['hi', 'operator power', 'ü€😀x']), isPublic: g.chance(0.5) },
        enableRoom: { id: ctrl && g.chance(0.8) ? ctrl : t },
        renew: { id: pick([pspawn, pbank, t].filter(Boolean)) },
      }[name];
      if (first === null) first = args.id || null;
      s.intent(user, pc, name, args);
    }
    const doc = s.objects.find((o) => o._id === pc);
    const tdoc = first && s.objects.find((o) => o._id === first);
    if (tdoc && doc.x !== 0 && doc.x !== 49 && g.chance(0.85)) [doc.x, doc.y] = near([tdoc.x, tdoc.y], g.chance(0.7) ? 1 : 3);
  }
  for (let k = g.int(3); k > 0; k--) {
    const [pc, user] = pick(pcs);
    const doc = s.objects.find((o) => o._id === pc);
    const foe = user === 'u1' ? 'u2' : 'u1';
    const c = s.creep(...near([doc.x, doc.y], 1), { user: foe, body: pick(['attack,attack,move', 'ranged_attack,move', 'heal,heal,move']) });
    const b = s.objects.find((o) => o._id === c).body[0].type;
    s.intent(foe, c, b === 'attack' ? 'attack' : b === 'heal' ? 'heal' : 'rangedAttack', { id: pc, x: doc.x, y: doc.y });
  }
  s.tags.push('fuzz', 'pc');
  return s.build();
}

module.exports = { scenario, globalScenario, cronScenario, random, randomRecorded, randomGlobal, randomNpc, randomStronghold, randomCron, randomPc, terrain, body, C, system };
