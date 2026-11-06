// Replays a recording (run.js) through the oracle and checks that the official processor / global stage
// under lib/fake-driver.js does exactly what the real server did: every bulk op list, mapView, eventLog,
// activation, notification, stat, roomInfo save, history and crash. Optionally writes the replayed cases as
// oracle fixtures (every <every>-th tick, all crashes), so chitin replays real-server ticks.
//   node22 tools/oracle/record/check.js <record dir> [--fixtures <out dir>] [--every 10]
'use strict';
const fs = require('fs');
const path = require('path');
const proc = require('../processor');

const args = process.argv.slice(2);
const dir = path.resolve(args[0] || 'chitin-record');
const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
const fixtures = opt('--fixtures', null);
const every = Number(opt('--every', 10));
const read = (f) => (fs.existsSync(path.join(dir, f)) ? fs.readFileSync(path.join(dir, f), 'utf8').split('\n').filter(Boolean).map(JSON.parse) : []);
const parse = (s) => (s === null || s === undefined ? null : JSON.parse(s));

function diff(a, b, p = '') {
  if (JSON.stringify(a) === JSON.stringify(b)) return null;
  if (a && b && typeof a === 'object' && typeof b === 'object') {
    const ka = Object.keys(a), kb = Object.keys(b);
    if (Array.isArray(a) !== Array.isArray(b)) return `${p}: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`;
    for (const k of new Set([...ka, ...kb])) {
      const d = diff(a[k], b[k], `${p}.${k}`);
      if (d) return d;
    }
    return `${p}: key order ${ka} vs ${kb}`;
  }
  return `${p}: recorded ${JSON.stringify(a)}, oracle ${JSON.stringify(b)}`;
}

// Loki's `$loki`/`meta` come with every document the real storage returns; chitin documents have neither
// (D16), so fixtures are replayed again without them.
function chitinDocs(s) {
  s = JSON.parse(JSON.stringify(s));
  const strip = (d) => d && typeof d === 'object' && (delete d.$loki, delete d.meta);
  if (s.kind === 'room') {
    [...s.objects, ...s.flags, ...Object.values(s.users), s.roomDoc].forEach(strip);
  } else {
    Object.values(s.db).forEach((docs) => docs.forEach(strip));
  }
  return s;
}

function checkRoom(line, r) {
  const rec = line.recorded, o = r.outcome;
  const info = rec.roomInfo ? Object.assign({ _id: line.scenario.room }, line.scenario.roomDoc, JSON.parse(JSON.stringify(rec.roomInfo))) : null;
  return diff(rec.ops, r.ops, 'ops') || diff(rec.crash, r.crash, 'crash') ||
    (rec.crash ? null : diff(parse(rec.mapView), o.mapView, 'mapView') || diff(parse(rec.eventLog), o.eventLog, 'eventLog') ||
      diff(rec.activated, o.activated, 'activated') || diff(rec.notifications, o.notifications, 'notifications') ||
      diff(rec.stats, o.stats, 'stats') || diff(parse(rec.history), o.history, 'history') || (info && diff(info, o.roomInfo, 'roomInfo')));
}

function checkGlobal(line, r) {
  return diff(line.recorded.ops, r.ops, 'ops') || diff(line.recorded.activated, r.outcome.activated, 'activated');
}

async function main() {
  const out = { rooms: [], global: [] };
  let bad = 0, n = 0;
  for (const [kind, file, check] of [['rooms', 'rooms.ndjson', checkRoom], ['global', 'global.ndjson', checkGlobal]]) {
    for (const line of read(file)) {
      const r = await proc.run(line.scenario);
      n++;
      const d = check(line, r);
      if (d) {
        bad++;
        if (bad <= 20) proc.log(`MISMATCH ${line.name}: ${d}`);
      }
      const T = line.scenario.gameTime;
      const busy = kind === 'rooms' || Object.keys(line.recorded.ops).length || line.recorded.activated.length;
      if (fixtures && !d && busy && (T % every === 0 || r.crash)) {
        const scenario = chitinDocs(line.scenario);
        out[kind].push({ name: `recorded ${line.name}`, scenario, result: await proc.run(scenario) });
      }
    }
  }
  proc.log(`checked ${n} recorded cases: ${n - bad} identical, ${bad} different`);
  if (fixtures) {
    fs.mkdirSync(fixtures, { recursive: true });
    for (const [k, lines] of Object.entries(out)) {
      if (!lines.length) continue;
      fs.writeFileSync(path.join(fixtures, `${k}.ndjson`), lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
      proc.log(`wrote ${lines.length} fixtures to ${path.join(fixtures, `${k}.ndjson`)}`);
    }
  }
  process.exit(bad ? 1 : 0);
}

main().catch((e) => {
  proc.log((e && e.stack) || e);
  process.exit(1);
});
