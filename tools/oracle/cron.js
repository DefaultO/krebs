// Cron oracle (ARCHITECTURE §12.2, 01 CRON-STAGE): one official job of reference/backend/lib/cronjobs.js (or
// strongholds.js) runs unchanged against the REAL reference/storage/lib/db.js over an in-memory Loki, reached
// through the same wrappers common/lib/storage.js builds over its RPC client (JSON both ways). Date =
// vclock(G); Math.random = the cron stream of (job, G, unit) of the unit being processed (RNG-SEED), the
// unit taken from the job's outer loop: rooms for roomsForceUpdate/genPowerBanks/genInvaders, the terminal
// room for recreateNpcOrders, the sector centre for genDeposits/genStrongholds, the core `_id` for
// expandStrongholds. Storage's genId draws come from a separate stream (chitin ids use no RNG). Same command
// line as processor.js (node 22):  node tools/oracle/cron.js gen <cases.js|all>
//
// Scenario (kind "cron"): {"kind":"cron","worldSeed":"0x1","clock":{…},"gameTime":G,"job":"genPowerBanks",
//   "db":{collection: [docs]},"env":{key: value},"compat":"private","tags":[…]}
// Outcome: every collection after the job (Loki `$loki`/`meta` dropped; `_id`s the job created → '#<n>' per
// collection, in collection order), `env` (activeRooms sorted), `crashed`. `units` in the result lists the
// units in draw order with their draw counts.
'use strict';
const { EventEmitter } = require('events');
const fake = require('./lib/fake-driver');
const proc = require('./processor');
const rng = require('./lib/rng');

const { capture, json, reference, common } = fake;
const q = require('q');
const loki = require('lokijs');

const config = common.configManager.config;
config.storage = config.storage || new EventEmitter();
const storageMethods = require(reference('storage/lib/db.js'));
const backendUtils = require(reference('backend/lib/utils.js'));
require(reference('backend/lib/cronjobs.js')); // fills config.cronjobs

// Which storage results carry the job's unit (see the header).
const UNITS = {
  roomsForceUpdate: { iterate: 'rooms' },
  genPowerBanks: { iterate: 'rooms' },
  genInvaders: { iterate: 'rooms' },
  genDeposits: { iterate: 'rooms' },
  expandStrongholds: { iterate: 'rooms.objects' },
  recreateNpcOrders: { findOne: 'rooms' },
  genStrongholds: { sectorRegex: true },
};

function loadLoki(s) {
  const db = new loki('chitin-oracle', { autosave: false });
  db.addCollection('env').insert({ data: Object.assign({ gameTime: s.gameTime }, json(s.env || {})), databaseVersion: 9 });
  for (const [name, docs] of Object.entries(s.db || {})) {
    const c = db.addCollection(name);
    c.ensureUniqueIndex('_id');
    for (const d of json(docs)) c.insert(d);
  }
  db.loadDatabase = (opts, cb) => cb(null);
  config.storage.getDb = () => db;
  return config.storage.loadDb().then(() => db);
}

async function run(s) {
  await proc.idle();
  const job = config.cronjobs[s.job];
  if (!job) throw new Error(`unknown cron job ${s.job}`);
  const db = await loadLoki(s);
  const G = BigInt(s.gameTime);
  const unitRule = UNITS[s.job] || {};
  const units = [];
  let unit = null;
  const setUnit = (u) => {
    unit = { key: String(u), stream: rng.cronStream(s.worldSeed, G, s.job, String(u)), draws: 0 };
    units.push(unit);
  };
  const genIdStream = rng.stream(rng.seed(s.worldSeed, 'oracle-genid', G)).random;
  let inStorage = false;
  const random = () => {
    if (inStorage) return genIdStream();
    if (!unit) {
      capture.storeErrors.push('Math.random outside a unit');
      setUnit('<none>');
    }
    unit.draws++;
    return unit.stream.random();
  };
  const onResult = (method, args, result) => {
    const [coll, op, opArgs] = args;
    if (method !== 'dbRequest') return result;
    if (op === 'find' && coll === unitRule.iterate && Array.isArray(result)) {
      result[Symbol.iterator] = function* () {
        for (const x of Array.prototype.values.call(this)) {
          setUnit(x._id);
          yield x;
        }
      };
    }
    const query = opArgs && opArgs[0];
    if (op === 'findOne' && coll === unitRule.findOne && query && typeof query._id === 'string') setUnit(query._id);
    const m = unitRule.sectorRegex && op === 'find' && coll === 'rooms' && query && query._id && typeof query._id.$regex === 'string' &&
      /^\^([WE]\d*)\\d([NS]\d*)\\d\$$/.exec(query._id.$regex);
    if (m) setUnit(`${m[1]}5${m[2]}5`);
    return result;
  };
  const st = {
    room: null, worldSeed: s.worldSeed, clock: s.clock, gameTime: s.gameTime, db: {},
    rpc(method, ...args) {
      args = json(args); // the RPC frame
      const d = q.defer();
      inStorage = true;
      try {
        storageMethods[method](...args, (err, res) => (err ? d.reject(err) : d.resolve(onResult(method, args, json(res)))));
      } finally {
        inStorage = false;
      }
      return d.promise;
    },
  };
  fake.begin(st, random);
  fake.reached();
  try {
    await job[1]({ utils: backendUtils });
  } catch (e) {
    capture.crashed = String((e && e.stack) || e).split('\n')[0];
    proc.log(`Cron ${s.job} error:`, (e && e.stack) || e);
  }
  await new Promise((r) => setImmediate(r));
  const known = new Set(Object.values(s.db || {}).flat().map((d) => d._id));
  const outcome = {};
  for (const c of db.listCollections().map((c) => c.name).filter((n) => n !== 'env')) {
    let n = 0;
    outcome[c] = db.getCollection(c).data.map((d) => {
      const o = json(d);
      delete o.$loki;
      delete o.meta;
      if (!known.has(o._id)) o._id = `#${++n}`;
      return o;
    });
  }
  const env = json(db.getCollection('env').get(1).data);
  if (Array.isArray(env.activeRooms)) env.activeRooms.sort();
  outcome.env = env;
  outcome.crashed = capture.crashed !== null;
  const result = json({ outcome, crash: capture.crashed, fRows: capture.fRows, reached: fake.reached(),
    units: units.map((u) => [u.key, u.draws]), storeErrors: capture.storeErrors });
  fake.end();
  return result;
}

module.exports = { run };

if (require.main === module) proc.main();
