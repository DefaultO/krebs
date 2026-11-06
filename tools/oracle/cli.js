// CLI oracle (CLI-SERVER … CLI-STRONGHOLDS-*): the official reference/backend/lib/cli/*.js, unchanged, behind the
// official server.js connection listener on a fake socket, over the REAL storage db.js (LokiJS from the scenario's
// db.json, `$loki`/`meta` stripped from what it returns: FMT-LOKI-ARTIFACTS), with the official cronjobs, utils and
// strongholds modules. A scenario is a db.json, bot AI files and CLI lines; the result is each line's output, the
// collections after the last line and the env keys chitin models.
//   node tools/oracle/cli.js gen <out.ndjson>
// What chitin cannot share and the replay masks: random (genId ids, bot badges, ruin ids), wall-clock dates
// (registeredDate, the `t<timestamp>` branch of bots.reload), the greeting's version.
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const stream = require('stream');
const { EventEmitter } = require('events');
const fake = require('./lib/fake-driver');
const { reference, prng } = require('./lib/common');
const loki = require('lokijs');
const q = require('q');

const commonConfig = fake.common.configManager.config;
commonConfig.storage = commonConfig.storage || new EventEmitter();
commonConfig.cli = commonConfig.cli || new EventEmitter();
commonConfig.backend = commonConfig.backend || new EventEmitter();
const storageMethods = require(reference('storage/lib/db.js'));
const ASSETS = fs.mkdtempSync(path.join(os.tmpdir(), 'chitin-cli-assets-'));
fs.mkdirSync(path.join(ASSETS, 'map', 'zoom2'), { recursive: true });
process.env.ASSET_DIR = ASSETS;
const BOTS = fs.mkdtempSync(path.join(os.tmpdir(), 'chitin-cli-bots-'));
commonConfig.common.bots = {};
// after common/lib/storage.js _connect: `db` holds the 20 collections (help(storage.db) lists them at load)
{
  const proxy = fake.common.storage.db;
  const db = {};
  for (const c of ['leaderboard.power', 'leaderboard.seasons', 'leaderboard.world', 'users.intents', 'market.orders', 'market.stats', 'rooms',
    'rooms.objects', 'rooms.flags', 'rooms.intents', 'rooms.terrain', 'transactions', 'users', 'users.code', 'users.console',
    'users.messages', 'users.money', 'users.notifications', 'users.resources', 'users.power_creeps']) db[c] = proxy[c];
  fake.common.storage.db = db;
}
require(reference('backend/lib/cronjobs.js'));
require(reference('backend/lib/cli/server.js'));

function strip(v) {
  if (Array.isArray(v)) return v.map(strip);
  if (v && typeof v === 'object') {
    const o = {};
    for (const k of Object.keys(v)) if (k !== '$loki' && k !== 'meta') o[k] = strip(v[k]);
    return o;
  }
  return v;
}

async function run(s) {
  // the storage process
  const tmp = path.join(os.tmpdir(), `chitin-cli-${process.pid}.json`);
  fs.writeFileSync(tmp, JSON.stringify(s.db));
  let db = null;
  commonConfig.storage.getDb = () => (db = new loki(tmp, { autosave: false }));
  await commonConfig.storage.loadDb();
  fs.unlinkSync(tmp);
  const st = { room: null, worldSeed: '0x1', clock: { baseMs: s.nowMs, baseTick: 0, nominalTickMs: 1000 }, gameTime: 0, db: {},
    rpc(method, ...args) {
      args = JSON.parse(JSON.stringify(args));
      const d = q.defer();
      const err = console.error;
      console.error = () => {};
      try {
        storageMethods[method](...args, (e, res) => (e ? d.reject(e) : d.resolve(res === undefined ? res : strip(JSON.parse(JSON.stringify(res))))));
      } finally {
        console.error = err;
      }
      return d.promise;
    } };
  const g = prng(7);
  fake.begin(st, () => g.rand());
  // bot AIs
  for (const [name, files] of Object.entries(s.bots || {})) {
    const dir = path.join(BOTS, name);
    fs.mkdirSync(dir, { recursive: true });
    for (const [f, src] of Object.entries(files)) fs.writeFileSync(path.join(dir, f), src);
    commonConfig.common.bots[name] = dir;
  }
  // one connection through server.js's listener
  let out = '';
  const socket = new stream.Duplex({
    read() {},
    write(chunk, enc, cb) {
      out += chunk.toString();
      cb();
    },
  });
  socket.remoteAddress = '127.0.0.1';
  socket.remotePort = 1;
  const log = console.log;
  console.log = () => {};
  commonConfig.cli.connectionListener(socket);
  const settle = async () => {
    for (let i = 0; i < 20; i++) await new Promise((r) => setImmediate(r));
    await new Promise((r) => setTimeout(r, 5));
    for (let i = 0; i < 20; i++) await new Promise((r) => setImmediate(r));
  };
  await settle();
  const greeting = out;
  const outputs = [];
  for (const line of s.lines) {
    out = '';
    socket.push(line + '\r\n');
    await settle();
    outputs.push(out);
  }
  console.log = log;
  fake.end();
  const collections = {};
  for (const name of ['rooms', 'rooms.objects', 'rooms.flags', 'users', 'users.code', 'market.orders']) {
    const c = db.getCollection(name);
    collections[name] = c ? strip(JSON.parse(JSON.stringify(c.data))) : [];
  }
  const envData = db.getCollection('env').get(1).data;
  const env = {};
  for (const k of Object.keys(envData).sort()) {
    if (/^(memory:|activeRooms$|mainLoopPaused$|tickRate$|gameTime$)/.test(k)) env[k] = envData[k];
  }
  return { greeting, outputs, collections, env };
}

// A 3×3 block around W2N2: plain rooms walled at the edges, a controller and a source each, the special users,
// a player owning W1N1 (spawn, creep, extension, flags), a market order.
function world() {
  const rooms = [], terrain = [], objects = [];
  let id = 0;
  const oid = () => `o${(++id).toString(16).padStart(14, '0')}`;
  for (const x of [1, 2, 3]) for (const y of [1, 2, 3]) {
    const name = `W${x}N${y}`;
    rooms.push({ _id: name, status: 'normal' });
    let t = '';
    for (let yy = 0; yy < 50; yy++) for (let xx = 0; xx < 50; xx++) {
      const edge = xx === 0 || yy === 0 || xx === 49 || yy === 49;
      t += edge && !(xx > 20 && xx < 30 || yy > 20 && yy < 30) ? '1' : ((xx * 7 + yy * 3 + x) % 23 === 0 ? '1' : (xx + yy) % 17 === 0 ? '2' : '0');
    }
    terrain.push({ room: name, terrain: t, type: 'terrain' });
    objects.push({ _id: oid(), room: name, type: 'controller', x: 10 + x, y: 10 + y, level: 0 });
    objects.push({ _id: oid(), room: name, type: 'source', x: 30, y: 30 + y, energy: 3000, energyCapacity: 3000, ticksToRegeneration: 300 });
  }
  const alice = 'a11ce000000000a';
  objects[0] = Object.assign(objects[0], { user: alice, level: 3, progress: 10, downgradeTime: 5000, safeModeAvailable: 1 });
  objects.push({ _id: oid(), room: 'W1N1', type: 'spawn', x: 25, y: 25, name: 'Spawn1', user: alice, store: { energy: 300 }, storeCapacityResource: { energy: 300 }, hits: 5000, hitsMax: 5000, spawning: null, notifyWhenAttacked: true });
  objects.push({ _id: oid(), room: 'W1N1', type: 'creep', x: 26, y: 25, name: 'c1', user: alice, body: [{ type: 'move', hits: 100 }], store: {}, storeCapacity: 0, hits: 100, hitsMax: 100, ageTime: 1600 });
  objects.push({ _id: oid(), room: 'W1N1', type: 'extension', x: 27, y: 27, user: alice, store: { energy: 50 }, storeCapacityResource: { energy: 50 }, hits: 1000, hitsMax: 1000 });
  const users = [
    { _id: '2', username: 'Invader', usernameLower: 'invader', gcl: 0, cpu: 0, active: 1, cpuAvailable: 10000 },
    { _id: '3', username: 'Source Keeper', usernameLower: 'source keeper', gcl: 0, cpu: 0, active: 1, cpuAvailable: 10000 },
    { _id: 'f4b532d08c3952a', username: 'Screeps', usernameLower: 'screeps', gcl: 0, cpu: 0, active: false, cpuAvailable: 0, badge: { type: 12, color1: '#999999', color2: '#999999', color3: '#999999', flip: false, param: 26 } },
    { _id: alice, username: 'Alice', usernameLower: 'alice', gcl: 1000, cpu: 100, active: 10000, cpuAvailable: 10000, rooms: ['W1N1'] },
  ];
  const coll = (name, data) => ({ name, data: data.map((d, i) => Object.assign({}, d, { meta: { revision: 0, created: 0, version: 0 }, $loki: i + 1 })),
    idIndex: data.map((_, i) => i + 1), binaryIndices: {}, constraints: null, uniqueNames: ['_id'], transforms: {}, objType: name, dirty: true,
    cachedIndex: null, cachedBinaryIndex: null, cachedData: null, adaptiveBinaryIndices: false, transactional: false, cloneObjects: false,
    cloneMethod: 'parse-stringify', asyncListeners: false, disableChangesApi: true, autoupdate: false, ttl: null, maxId: data.length,
    DynamicViews: [], events: {}, changes: [] });
  const env = { data: { gameTime: 100, mainLoopPaused: '0', tickRate: 1000, activeRooms: [], [`memory:${alice}`]: '{"a":1}' }, databaseVersion: 9 };
  return {
    filename: 'db.json', databaseVersion: 1.5, engineVersion: 1.5, autosave: false, options: {},
    collections: [coll('env', [env]), coll('users', users), coll('rooms', rooms), coll('rooms.objects', objects), coll('rooms.terrain', terrain),
      coll('rooms.flags', [{ _id: 'fl1', user: alice, room: 'W1N1', data: 'Flag1~1~1~20~20' }]),
      coll('users.code', [{ _id: 'cd1', user: alice, branch: 'default', modules: { main: '' }, activeWorld: true, activeSim: true, timestamp: 1 }]),
      coll('market.orders', [{ _id: 'mo1', type: 'sell', resourceType: 'H', price: 1000, amount: 5, remainingAmount: 5, totalAmount: 5, roomName: 'W1N1', active: true, created: 1, createdTimestamp: 1 }])],
  };
}

const BOT = { 'main.js': 'module.exports.loop = function() {};\n', 'util.helper.js': 'module.exports = 1;\n', 'readme.txt': 'not a module' };

const LINES = [
  // CLI-OUTPUT, CLI-SANDBOX
  '1 + 1', 'var x = {a: [1, 2, {b: {c: {d: 1}}}], s: "it\'s"}', 'x', 'print(x, [1, 2, 3], undefined, null)', 'print()',
  'Promise.resolve(42)', 'Promise.resolve("")', 'Promise.reject("nope")', 'throw new Error("boom")', 'foo(', 'undefined', '"undefined"',
  'typeof storage.db', 'Object.keys(this).filter(k => ["print","storage","map","bots","strongholds","system","help"].includes(k)).sort().join()',
  // CLI-HELP
  'help()', 'help(storage)', 'help(storage.db)', 'help(storage.env)', 'help(storage.pubsub)', 'help(map)', 'help(system)', 'help(bots)', 'help(strongholds)',
  'help(map.openRoom)', 'help(42)',
  // CLI-STORAGE
  "storage.db.users.findOne({username: 'Alice'})", "storage.db['rooms.objects'].find({room: 'W1N1'}).then(r => print(r.map(o => o.type)))",
  "storage.db['rooms.objects'].count({user: {$ne: null}})", "storage.db.rooms.find({_id: {$regex: '^W2'}}).then(r => print(r.map(o => o._id)))",
  "storage.db['rooms.objects'].update({type: 'source', room: 'W1N1'}, {$set: {energy: 1}})",
  "storage.db['rooms.objects'].findOne({type: 'source', room: 'W1N1'}).then(s => print(s.energy))",
  "storage.db.users.update({username: 'Alice'}, {$inc: {gcl: 5}, $set: {cpu: 50}})", "storage.db.users.findOne({username: 'Alice'}).then(u => print(u.gcl, u.cpu))",
  "storage.db['rooms.flags'].find({user: 'a11ce000000000a'})",
  "storage.db['market.orders'].update({_id: 'mo1'}, {$set: {price: 2000}})",
  "storage.db['rooms.objects'].removeWhere({type: 'extension'})",
  "storage.db['rooms.objects'].findEx({room: 'W1N1'}, {sort: {x: -1}, limit: 2}).then(r => print(r.map(o => o.x)))",
  "storage.env.get(storage.env.keys.GAMETIME)", "storage.env.get('memory:a11ce000000000a')", "storage.env.set('myKey', {v: 1})", "storage.env.get('myKey')",
  "storage.env.hmset('myHash', {a: 1})", "storage.env.hget('myHash', 'a')", "storage.env.del('myKey')", "storage.env.get('myKey')",
  // CLI-SYSTEM-*
  'system.getTickDuration()', 'system.setTickDuration(250)', 'system.getTickDuration()', "system.setTickDuration('x')",
  'system.pauseSimulation()', "storage.env.get('mainLoopPaused')", 'system.resumeSimulation()', "system.runCronjob('nope')",
  // CLI-MAP-* (closeRoom/openRoom without a timestamp; generateRoom is mapgen's own oracle)
  "map.closeRoom('W3N3')", "map.closeRoom('W3N3')", "map.openRoom('W3N3')",
  "storage.db.rooms.findOne({_id: 'W3N3'}).then(r => print(r.status))",
  // CLI-BOTS-*
  "bots.spawn('nobot', 'W2N2')", "bots.spawn('test', 'W1N1')", "bots.spawn('test', 'W2N2', {username: 'Alice'})",
  "bots.spawn('test', 'W2N2', {username: 'TestBot', cpu: 50, gcl: 2, x: 20, y: 20})",
  "storage.db.users.findOne({username: 'TestBot'}).then(u => print(u.cpu, u.gcl, u.bot, u.active))",
  "storage.db['rooms.objects'].findOne({room: 'W2N2', type: 'controller'}).then(c => print(c.level, c.safeMode))",
  "bots.reload('nobody')", "bots.reload('test')",
  "storage.db['users.code'].find({}).then(r => print(r.length))",
  "bots.removeUser('Nobody')", "bots.removeUser('Alice')", "bots.removeUser('TestBot')",
  "storage.db.users.findOne({username: 'TestBot'})",
  // CLI-STRONGHOLDS-*
  "strongholds.expand('W1N1')", "strongholds.spawn('W2N2', {templateName: 'bunker1', x: 20, y: 20})", "strongholds.spawn('W1N1')",
  'help(strongholds).length > 100',
];

async function main() {
  const [mode, out] = process.argv.slice(2);
  if (mode !== 'gen') throw new Error('usage: cli.js gen <out.ndjson>');
  const scenario = { kind: 'cli', mirror: ['backend/lib/cli/server.js', 'cli/sandbox.js', 'cli/help.js', 'cli/system.js', 'cli/map.js', 'cli/bots.js', 'cli/strongholds.js'],
    nowMs: 1700000000000, db: world(), bots: { test: BOT }, lines: LINES };
  const result = await run(scenario);
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, JSON.stringify({ name: 'cli session', scenario, result }) + '\n');
}

main().then(() => process.exit(0), (e) => {
  console.error(e);
  process.exit(1);
});
