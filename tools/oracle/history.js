// History oracle (POST-HISTORY, FMT-HISTORY-CHUNK): the official reference/driver/lib/history.js over the REAL
// reference/storage/lib/db.js env (in-memory Loki, as cron.js), driven by the official `saveRoomHistory` of
// reference/engine/src/processor.js:522-536, whose source is extracted and evaluated unchanged. A scenario is a
// list of saveRoomHistory calls `{room, gameTime, nowMs, objects}`; the outcome lists every `saveRoomHistory`
// engine event `[room, baseTime, result]` in emit order and the env hashes left behind.
//   node tools/oracle/history.js fuzz <seed> <n>      random scenarios → NDJSON lines {name, scenario, result}
//   node tools/oracle/history.js gen <out.ndjson> <seed> <n>
'use strict';
const fs = require('fs');
const path = require('path');
const fake = require('./lib/fake-driver');
const { reference, prng } = require('./lib/common');
const q = require('q');
const loki = require('lokijs');

const { common } = fake;
const config = common.configManager.config;
config.storage = config.storage || new (require('events').EventEmitter)();
const storageMethods = require(reference('storage/lib/db.js'));
const history = require(reference('driver/lib/history.js'));

// processor.js's saveRoomHistory, verbatim, over a driver that has the real history module.
const src = fs.readFileSync(reference('engine/src/processor.js'), 'utf8');
const fn = /function saveRoomHistory\(roomId, objects, gameTime\) \{[\s\S]*?\r?\n\}\r?\n/.exec(src)[0];
let currentHistoryPromise;
const driver = { config: { historyChunkSize: 20 }, history };
// eslint-disable-next-line no-eval
const saveRoomHistory = eval(`(${fn.replace('function saveRoomHistory', 'function')})`);

async function run(s) {
  const db = new loki('chitin-history', { autosave: false });
  db.addCollection('env').insert({ data: {}, databaseVersion: 9 });
  db.loadDatabase = (opts, cb) => cb(null);
  config.storage.getDb = () => db;
  await config.storage.loadDb();
  const st = { room: null, worldSeed: '0x1', clock: { baseMs: 0, baseTick: 0, nominalTickMs: 1000 }, gameTime: 0, db: {},
    rpc(method, ...args) {
      args = JSON.parse(JSON.stringify(args));
      const d = q.defer();
      storageMethods[method](...args, (err, res) => (err ? d.reject(err) : d.resolve(res === undefined ? res : JSON.parse(JSON.stringify(res)))));
      return d.promise;
    } };
  fake.begin(st, Math.random);
  const events = [];
  const listener = (room, base, result) => events.push(JSON.parse(JSON.stringify([room, base, result])));
  config.engine.on('saveRoomHistory', listener);
  currentHistoryPromise = q.when();
  driver.config.historyChunkSize = s.chunk;
  let crash = null;
  try {
    for (const c of s.calls) {
      Date.now = () => c.nowMs;
      await saveRoomHistory(c.room, c.objects, c.gameTime);
    }
  } catch (e) {
    crash = String(e);
  }
  config.engine.removeListener('saveRoomHistory', listener);
  const env = db.getCollection('env').get(1).data;
  const hashes = {};
  for (const k of Object.keys(env).sort()) if (k.startsWith('roomHistory:')) hashes[k] = Object.keys(env[k]).sort((a, b) => a - b);
  fake.end();
  return { events, hashes, crash };
}

// Rooms that are active on some ticks (gaps end chunks or leave bases missing); objects appear, move, change
// nested fields (store, body), lose keys and disappear.
function random(seed, i) {
  const g = prng((seed * 2654435761 + i) >>> 0);
  const rooms = ['W1N1', 'W2N1', 'E3S4'].slice(0, 1 + g.int(3));
  const chunk = g.chance(0.8) ? 20 : 1 + g.int(10);
  const objs = {};
  for (const r of rooms) {
    objs[r] = {};
    for (let k = g.int(4); k > 0; k--) {
      const id = g.chance(0.1) ? String(100 + g.int(900)) : `o${r}${k}`;
      objs[r][id] = { _id: id, type: g.pick(['creep', 'source', 'spawn']), x: g.int(50), y: g.int(50), store: { energy: g.int(100) } };
    }
  }
  const calls = [];
  const start = g.int(60);
  for (let t = start; t < start + 30 + g.int(70); t++) {
    for (const r of rooms) {
      if (!g.chance(0.85)) continue;
      const o = objs[r];
      for (const id of Object.keys(o)) {
        const x = o[id];
        if (g.chance(0.3)) x.x = g.int(50);
        if (g.chance(0.2)) x.store = g.chance(0.5) ? { energy: g.int(100) } : { energy: g.int(100), H: g.int(5) };
        if (g.chance(0.05)) delete x.y;
        if (g.chance(0.05)) x.y = g.chance(0.5) ? null : 0;
        if (g.chance(0.05)) x.body = [{ type: 'move', hits: g.int(101) }];
        if (g.chance(0.03)) delete o[id];
      }
      if (g.chance(0.1)) {
        const id = `n${t}${r}`;
        o[id] = { _id: id, type: 'energy', x: g.int(50), y: g.int(50), amount: g.int(500) };
      }
      calls.push({ room: r, gameTime: t, nowMs: 1700000000000 + t * 1000, objects: JSON.parse(JSON.stringify(o)) });
    }
  }
  return { name: `history fuzz ${seed}/${i}`, scenario: { kind: 'history', mirror: 'driver/lib/history.js', chunk, calls } };
}

async function main() {
  const [mode, ...args] = process.argv.slice(2);
  if (mode === 'fuzz' || mode === 'gen') {
    const [seed, n] = mode === 'fuzz' ? [Number(args[0] || 1), Number(args[1] || 100)] : [Number(args[1] || 1), Number(args[2] || 100)];
    const lines = [];
    for (let i = 0; i < n; i++) {
      const c = random(seed, i);
      lines.push(JSON.stringify({ name: c.name, scenario: c.scenario, result: await run(c.scenario) }));
    }
    if (mode === 'fuzz') process.stdout.write(lines.join('\n') + '\n');
    else {
      fs.mkdirSync(path.dirname(args[0]), { recursive: true });
      fs.writeFileSync(args[0], lines.join('\n') + '\n');
    }
  } else {
    throw new Error('usage: history.js fuzz <seed> <n> | gen <out> <seed> <n>');
  }
}

main().then(() => process.exit(0), (e) => {
  console.error(e);
  process.exit(1);
});
