// Room-processor oracle (ARCHITECTURE §12.2): the official reference/engine/src/processor.js runs
// unchanged over lib/fake-driver.js, one FROZEN room scenario at a time, with Math.random = the room
// stream of (room, T) and Date = vclock(T). Needs node 22 (the official native.node pathfinder):
//   E:\screeps\node22\node-v22.22.3-win-x64\node.exe tools/oracle/processor.js <mode>
// Modes:
//   (none)              NDJSON: a scenario (or {name, scenario}) per stdin line → {name, result} per line
//   gen <cases.js|all> [out.ndjson]
//                       runs a case module (an array of lib/scenario.js `build()` results) twice
//                       (determinism guard) and writes its fixture: tools/oracle/cases/<area>.js →
//                       crates/engine/tests/fixtures/oracle/<area>/cases.ndjson; a module kept in a
//                       fixture dir (crates/…/fixtures/<area>/<name>.js, for porters) → <name>.ndjson
//                       next to it. `all` = tools/oracle/cases/*.js + crates/engine/tests/fixtures/**/*.js
//   fuzz <seed> <n> [room|recorded|global]
//                       n random scenarios (lib/scenario.js random/randomRecorded/randomGlobal) → lines
// global.js and cron.js take the same modes; every mode dispatches on the scenario `kind`.
// Result: {outcome, crash, fRows, ops, storeErrors}; `outcome` is what crates/engine/src/scenario.rs
// compares: {objects, foreignInserts, users, flags, roomInfo, eventLog, mapView, activated, notifications,
// stats, history, crashed}. Objects inserted without `_id` get '#<n>' (counter order), '#f<n>' if foreign.
// `fRows` lists F rows of spec 01 §19A the case reached (Rust replays skip those cases); `reached` the
// reference files whose functions ran (V8 coverage), which decides when a Rust replay is enforced.
'use strict';
const util = require('util');
const fs = require('fs');
const path = require('path');
const fake = require('./lib/fake-driver');
const { ROOT, reference } = require('./lib/common');

const { capture, driver, json } = fake;

// The official code logs to stdout; the protocol owns stdout.
const log = (...a) => process.stderr.write(util.format(...a) + '\n');
console.log = (...a) => {
  if (typeof a[0] === 'string' && a[0].startsWith('Error processing room ')) capture.crashed = String(a[1]).split('\n')[0];
  log(...a);
};
console.error = log;
console.warn = log;
console.info = log;

const parse = (s) => (s === null ? null : JSON.parse(s));

// FROZEN room scenario → the collections the fake driver serves.
function collections(s) {
  const users = {};
  for (const [id, doc] of Object.entries(s.users || {})) users[id] = doc._id === undefined ? Object.assign({ _id: id }, doc) : doc;
  const intents = {};
  for (const [uid, i] of s.intents || []) {
    const u = (intents[uid] = {});
    if (i.objects != null) u.objects = i.objects;
    if (i.objectsManual != null) u.objectsManual = i.objectsManual;
  }
  return json({
    'rooms.objects': s.objects,
    users: Object.values(users),
    'rooms.flags': s.flags || [],
    rooms: [Object.assign({ _id: s.room }, s.roomDoc)],
    'rooms.terrain': [{ _id: `terrain-${s.room}`, room: s.room, terrain: s.terrain, type: 'terrain' }],
    'rooms.intents': (s.intents || []).length ? [{ _id: `intents-${s.room}`, room: s.room, users: intents }] : [],
  });
}

// Every engine module is loaded up front, so module-load calls (`utils.getDriver()`, bulk-require) never
// show up in a scenario's `reached`.
function preload(dir) {
  for (const f of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, f.name);
    if (f.isDirectory()) preload(p);
    else if (f.name.endsWith('.js')) require(p);
  }
}

let prepared = false;
function prepare() {
  if (!prepared) preload(reference('engine/src/processor'));
  prepared = true;
}

let started = false;
function start() {
  prepare();
  if (!started) require(reference('engine/src/processor.js')); // connects, creates the queue, its fetch waits for us
  started = true;
}

// Resolves once the processor loop (if started) is parked at fetch again, so coverage of the next scenario
// does not include its restart.
function idle() {
  return new Promise((resolve) => {
    const poll = () => (!started || (fake.queues.rooms && fake.queues.rooms.waiting) ? resolve() : setTimeout(poll, 0));
    poll();
  });
}

// Any scenario kind: room (here), global (global.js), cron (cron.js).
function run(s) {
  if (s.kind === 'global') return require('./global').run(s);
  if (s.kind === 'cron') return require('./cron').run(s);
  return runRoom(s);
}

// Runs one room scenario through processRoom; resolves after markDone and the history chain.
function runRoom(s) {
  start();
  return new Promise((resolve, reject) => {
    const st = {
      room: s.room,
      worldSeed: s.worldSeed,
      clock: s.clock,
      gameTime: s.gameTime,
      worldSize: s.worldSize,
      db: collections(s),
      terrains: [{ room: s.room, terrain: s.terrain }],
      onDone: () => setImmediate(() => setImmediate(() => {
        try {
          resolve(result(st, s));
        } catch (e) {
          reject(e);
        } finally {
          fake.end();
        }
      })),
    };
    fake.begin(st, 'room');
    fake.reached(); // reset the coverage counters
    const fetch = () => {
      const roomsQueue = fake.queues.rooms; // created after connect(); the loop restarts with setTimeout(loop, 0)
      if (!roomsQueue || !roomsQueue.waiting) return setTimeout(fetch, 0);
      const w = roomsQueue.waiting;
      roomsQueue.waiting = null;
      w.resolve(s.room);
    };
    fetch();
  });
}

function result(st, s) {
  const foreign = new Set(capture.foreign);
  const outcome = {
    objects: st.db['rooms.objects'].filter((d) => !foreign.has(d)),
    foreignInserts: capture.foreign.map((d) => [d.room, d]),
    users: st.db.users,
    flags: st.db['rooms.flags'],
    roomInfo: st.db.rooms[0],
    eventLog: parse(capture.eventLog),
    mapView: parse(capture.mapView),
    activated: capture.activated.includes(s.room),
    notifications: capture.notifications,
    stats: capture.stats,
    history: parse(capture.history),
    crashed: capture.crashed !== null,
  };
  return json({ outcome, crash: capture.crashed, fRows: capture.fRows, reached: fake.reached(), ops: capture.ops,
    storeErrors: capture.storeErrors });
}

async function runAll(cases) {
  const out = [];
  for (const c of cases) out.push(await run(c.scenario));
  return out;
}

async function gen(file, outFile) {
  const area = path.basename(file, '.js');
  const cases = require(path.resolve(file));
  const names = new Set();
  for (const c of cases) {
    if (names.has(c.name)) throw new Error(`${file}: duplicate case name ${c.name}`);
    names.add(c.name);
  }
  const a = await runAll(cases);
  const b = await runAll(cases);
  cases.forEach((c, i) => {
    if (JSON.stringify(a[i]) !== JSON.stringify(b[i])) {
      const k = Object.keys(a[i]).find((k) => JSON.stringify(a[i][k]) !== JSON.stringify(b[i][k]));
      throw new Error(`${c.name}: the official processor is not deterministic here (${k}: ${JSON.stringify(a[i][k])} vs ${JSON.stringify(b[i][k])})`);
    }
  });
  const inFixtures = path.resolve(file).startsWith(path.join(ROOT, 'crates'));
  outFile = outFile || (inFixtures ? path.resolve(file).replace(/\.js$/, '.ndjson') : path.join(ROOT, 'crates/engine/tests/fixtures/oracle', area, 'cases.ndjson'));
  fs.mkdirSync(path.dirname(outFile), { recursive: true });
  fs.writeFileSync(outFile, cases.map((c, i) => JSON.stringify({ name: c.name, scenario: c.scenario, result: a[i] })).join('\n') + '\n');
  const crashed = a.filter((r) => r.crash).length;
  log(`wrote ${path.relative(ROOT, outFile)} (${cases.length} cases${crashed ? `, ${crashed} crashing` : ''})`);
}

// The command line shared by processor.js, global.js and cron.js (any of them runs any scenario kind).
async function cli() {
  const [mode, ...args] = process.argv.slice(2);
  const write = (o) => process.stdout.write(JSON.stringify(o) + '\n');
  if (mode === 'gen') {
    const js = (dir) => (fs.existsSync(dir) ? fs.readdirSync(dir, { recursive: true }).filter((f) => f.endsWith('.js')).map((f) => path.join(dir, f)) : []);
    const files = args[0] === 'all' ? [...js(path.join(__dirname, 'cases')), ...js(path.join(ROOT, 'crates/engine/tests/fixtures'))] : [args[0]];
    for (const f of files) await gen(f, args[0] === 'all' ? undefined : args[1]);
  } else if (mode === 'fuzz') {
    const gens = require('./lib/scenario');
    const random = { room: gens.random, recorded: gens.randomRecorded, global: gens.randomGlobal, npc: gens.randomNpc, stronghold: gens.randomStronghold, cron: gens.randomCron, pc: gens.randomPc }[args[2] || 'room'];
    const seed = Number(args[0] || 1), n = Number(args[1] || 100);
    for (let i = 0; i < n; i++) {
      const c = random(seed, i);
      write({ name: c.name, scenario: c.scenario, result: await run(c.scenario) });
    }
  } else {
    const rl = require('readline').createInterface({ input: process.stdin });
    for await (const line of rl) {
      if (!line.trim()) continue;
      const j = JSON.parse(line);
      const s = j.scenario || j;
      write({ name: j.name, result: await run(s) });
    }
  }
  process.exit(0); // processor.js keeps an interval alive
}

function main() {
  cli().catch((e) => {
    log(e && e.stack || e);
    process.exit(1);
  });
}

// Exported before main() runs: global.js/cron.js require this module back while it is the entry point.
module.exports = { run, runRoom, collections, prepare, idle, main, log };

if (require.main === module) main();
