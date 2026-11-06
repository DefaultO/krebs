// Global-stage oracle (ARCHITECTURE §12.2): the official reference/engine/src/processor/global.js runs
// unchanged over lib/fake-driver.js with Math.random = the global stream of T and Date = vclock(T). Same
// command line as processor.js (node 22):  node tools/oracle/global.js [gen <cases.js|all> | fuzz <seed> <n> global]
//
// Scenario (kind "global", read by crates/engine/src/scenario.rs):
//   {"kind":"global","worldSeed":"0x1","clock":{…},"gameTime":T,"worldSize":12,"compat":"private","tags":[…],
//    "db":{"rooms":[…],"rooms.objects":[…],"users":[…],"market.orders":[…],"users.power_creeps":[…],
//          "users.intents":[…],"transactions":[…],"users.money":[…],"users.resources":[…]}}
// The five getInterRoom results come from `db` through the vanilla queries (driver index.js:429-449), so
// `users.intents` order is the processing order (ORD-POINTS 2/3). Outcome: every collection above after
// the bulk ops (inserts without `_id` get '#<n>' per collection), `activated` (sorted room names) and
// `crashed`.
'use strict';
const fake = require('./lib/fake-driver');
const proc = require('./processor');

const { capture, json } = fake;
const COLLECTIONS = ['rooms', 'rooms.objects', 'users', 'market.orders', 'users.power_creeps', 'users.intents', 'transactions',
  'users.money', 'users.resources'];

async function run(s) {
  proc.prepare();
  await proc.idle();
  const globalStage = require(fake.reference('engine/src/processor/global.js'));
  const db = json(Object.fromEntries(COLLECTIONS.map((c) => [c, (s.db && s.db[c]) || []])));
  fake.begin({ room: null, worldSeed: s.worldSeed, clock: s.clock, gameTime: s.gameTime, worldSize: s.worldSize, db }, 'global');
  fake.reached(); // reset the coverage counters
  try {
    await globalStage();
  } catch (e) {
    capture.crashed = String((e && e.stack) || e).split('\n')[0];
    proc.log('Global stage error:', (e && e.stack) || e);
  }
  await new Promise((r) => setImmediate(r));
  const outcome = Object.assign({}, db, { activated: [...new Set(capture.activated)].sort(), crashed: capture.crashed !== null });
  const result = json({ outcome, crash: capture.crashed, fRows: capture.fRows, reached: fake.reached(), ops: capture.ops,
    storeErrors: capture.storeErrors });
  fake.end();
  return result;
}

module.exports = { run, COLLECTIONS };

if (require.main === module) proc.main();
