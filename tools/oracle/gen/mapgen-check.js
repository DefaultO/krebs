// World-generation oracle (ARCHITECTURE §12.3): runs the official backend/lib/cli/map.js generateRoom, unchanged,
// over a fake @screeps/common db with Math.random = rng.js stream(h), and writes
//   crates/engine/tests/fixtures/mapgen/rooms.ndjson  exact cases: input (stream, opts, neighbours) → inserted docs
//   crates/engine/tests/fixtures/mapgen/stats.json    per room class over N rooms: distributions + flood connectivity
// Usage (node 22): node tools/oracle/gen/mapgen-check.js [N=2000]   env SCREEPS_ROOT (default E:/screeps/server)
// N = 0 rewrites only rooms.ndjson (~1 min: the hang case runs 50M draws); the stats run one child process per class
// (~10 min at N = 2000).
'use strict';
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { ROOT, writeFixture } = require('../lib/common');
const rng = require('../lib/rng');

const SCREEPS = process.env.SCREEPS_ROOT || 'E:/screeps/server';
const N = +(process.argv[2] || 2000);
const ONLY = process.argv[3]; // child: one class, stats JSON on stdout

// Before lodash loads: `_.shuffle` captures Math.random at load time. map.js loops forever where no
// controller/mineral tile qualifies (~1 room in 20k); after DRAWS draws the room is recorded as `Error: hang`
// (the Rust port gives up there too and the world generator redraws the room).
const DRAWS = 50e6;
let current = rng.stream(0n);
let draws = 0;
Math.random = () => {
  if (++draws > DRAWS) throw new Error('hang');
  return current.random();
};
const mod = (p) => require(path.join(SCREEPS, 'node_modules', p));
const common = mod('@screeps/common');
const map = mod('@screeps/backend/lib/cli/map');
const q = mod('q');

// The rooms.* collections generateRoom touches; PNG and terrain-blob post-steps are not under test.
let world = {};
let inserted;
common.storage.db.rooms = {
  findOne: ({ _id }) => q(world[_id] ? { _id } : null),
  insert: (doc) => q((inserted.room = doc)),
};
common.storage.db['rooms.terrain'] = {
  findOne: ({ room }) => q(world[room] ? { room, terrain: world[room] } : null),
  insert: (doc) => q((inserted.terrain = doc.terrain)),
};
common.storage.db['rooms.objects'] = { insert: (docs) => q((inserted.objects = docs)) };
map.updateRoomImageAssets = () => q();
map.updateTerrainData = () => q('OK');

// generateRoom on stream h with `neighbours` = {roomName: terrain}; returns {out} or {err}.
async function gen(h, room, opts, neighbours) {
  world = neighbours;
  inserted = { objects: [] };
  current = rng.stream(h);
  draws = 0;
  try {
    await map.generateRoom(room, opts);
    return { out: inserted };
  } catch (e) {
    return { err: String(e) };
  }
}

const CLASSES = {
  normal: {},
  keeper: { sources: 3, controller: false, keeperLairs: true },
  center: { sources: 3, controller: false },
  highway: { sources: 0, controller: false, mineral: false },
};
const H = (tag, i) => rng.seed(0x6d6170n, tag, BigInt(i));
const hex = (b) => b.toString(16).padStart(16, '0');
const nameOf = (x, y) => (x < 0 ? `W${-x - 1}` : `E${x}`) + (y < 0 ? `N${-y - 1}` : `S${y}`);
const edgeOf = (t, side) => [...Array(50).keys()].filter((i) => {
  const [x, y] = { top: [i, 49], right: [0, i], bottom: [i, 0], left: [49, i] }[side];
  return !(+t[y * 50 + x] & 1);
});
const SIDES = { top: [0, -1], right: [1, 0], bottom: [0, 1], left: [-1, 0] };

async function exactCases() {
  const lines = [];
  const g = rng.stream(H('mapgen-cases', 0));
  const int = (n) => Math.floor(g.random() * n);
  const pick = (a) => a[int(a.length)];
  const terrains = [];
  for (let i = 0; i < 160; i++) {
    const [x, y] = [int(256) - 128, int(256) - 128];
    const room = nameOf(x, y);
    const opts = JSON.parse(JSON.stringify(CLASSES[pick(Object.keys(CLASSES))]));
    if (int(3) === 0) opts.terrainType = 1 + int(28);
    if (int(3) === 0) opts.swampType = int(15);
    if (int(5) === 0) opts.sources = int(4);
    if (int(5) === 0) opts.mineral = pick([false, 'X', 'H', '']);
    if (int(6) === 0) opts.controller = !opts.controller;
    if (int(8) === 0) opts.keeperLairs = true;
    const neighbours = {};
    const nb = {};
    if (terrains.length && int(2) === 0) {
      for (const side of Object.keys(SIDES)) {
        if (int(2)) continue;
        const k = int(terrains.length);
        neighbours[nameOf(x + SIDES[side][0], y + SIDES[side][1])] = terrains[k];
        nb[side] = k;
        // The neighbour's facing edge, as map.js reads it: given back (must match), given wrong, or copied.
        const theirs = edgeOf(terrains[k], side);
        const roll = int(10);
        if (roll < 3) (opts.exits = opts.exits || {})[side] = [...theirs].reverse();
        if (roll === 3) (opts.exits = opts.exits || {})[side] = theirs.length ? theirs.slice(1) : [7];
      }
    }
    const free = Object.keys(SIDES).filter((s) => !(s in nb));
    if (free.length && int(4) === 0) (opts.exits = opts.exits || {})[pick(free)] = pick([[], [0, 1, 2, 49], [10, 11, 12, 13], [25], [48, 49]]);
    if (i === 0) opts.terrainType = 0;
    if (i === 1) opts.swampType = 15;
    const input = JSON.parse(JSON.stringify(opts));
    const h = H('mapgen-case', i);
    const res = await gen(h, room, opts, neighbours);
    if (res.out) terrains.push(res.out.terrain);
    lines.push({ tag: 'cli/map.js:23', h: hex(h), room, opts: input, neighbours: nb, ...res });
  }
  // A centre room with no mineral spot: map.js never returns (stopped after DRAWS draws).
  const h = H('mapgen-center', 982);
  lines.push({ tag: 'cli/map.js:521', h: hex(h), room: 'W1N1', opts: CLASSES.center, neighbours: {}, ...(await gen(h, 'W1N1', { ...CLASSES.center }, {})) });
  // One-tile exit, high wall fill: some attempts pass the flood with no source tile left, so the 1000-try
  // `_genTerrain` restart runs (map.js:342-344; the random cases above never reach it).
  for (const [k, terrainType] of [[35, 15], [65, 19]]) {
    const opts = { terrainType, swampType: 0, sources: 1, exits: { top: [24], right: [], bottom: [], left: [] } };
    const s = H('mapgen-src', k);
    lines.push({ tag: 'cli/map.js:342', h: hex(s), room: 'W1N1', opts, neighbours: {}, ...(await gen(s, 'W1N1', JSON.parse(JSON.stringify(opts)), {})) });
  }
  return lines;
}

// Statistics of one generated room: the quantities the Rust distribution test compares.
function measure(out) {
  const t = out.terrain;
  let wall = 0, swamp = 0;
  for (const c of t) {
    if (+c & 1) wall++;
    if (+c & 2) swamp++;
  }
  // 8-connectivity of the non-wall tiles (GEN-FLOOD on the final terrain).
  const open = [...t].map((c) => !(+c & 1));
  const seen = new Uint8Array(2500);
  const start = open.indexOf(true);
  let reached = 0;
  if (start >= 0) {
    const st = [start];
    seen[start] = 1;
    while (st.length) {
      const i = st.pop();
      reached++;
      const [x, y] = [i % 50, Math.floor(i / 50)];
      for (let dx = -1; dx <= 1; dx++) for (let dy = -1; dy <= 1; dy++) {
        const [nx, ny] = [x + dx, y + dy];
        if (nx < 0 || ny < 0 || nx > 49 || ny > 49 || !open[ny * 50 + nx] || seen[ny * 50 + nx]) continue;
        seen[ny * 50 + nx] = 1;
        st.push(ny * 50 + nx);
      }
    }
  }
  const count = (type) => out.objects.filter((o) => o.type === type).length;
  const mineral = out.objects.find((o) => o.type === 'mineral');
  return {
    wall: wall / 2500, swamp: swamp / 2500, connected: reached === open.filter(Boolean).length,
    exits: Object.keys(SIDES).map((s) => edgeOf(t, { top: 'bottom', right: 'left', bottom: 'top', left: 'right' }[s]).length),
    sources: count('source'), lairs: count('keeperLair'), controllers: count('controller'),
    mineral: mineral ? mineral.mineralType : '-', density: mineral ? mineral.density : 0,
  };
}

// Stats of one class over N rooms (rooms map.js never finishes are counted in `hung`, not measured).
async function classStats(cls) {
  const opts = CLASSES[cls];
  const t0 = Date.now();
  const acc = { n: 0, hung: 0, wall: 0, wall2: 0, swamp: 0, swamp2: 0, exitTiles: 0, connected: 0, sources: {}, lairs: {}, controllers: {}, mineral: {}, density: {} };
  for (let i = 0; i < N; i++) {
    const res = await gen(H(`mapgen-${cls}`, i), 'W1N1', JSON.parse(JSON.stringify(opts)), {});
    if (res.err === 'Error: hang') {
      acc.hung++;
      continue;
    }
    if (res.err) throw new Error(`${cls} ${i}: ${res.err}`);
    const m = measure(res.out);
    acc.n++;
    acc.wall += m.wall;
    acc.wall2 += m.wall * m.wall;
    acc.swamp += m.swamp;
    acc.swamp2 += m.swamp * m.swamp;
    acc.exitTiles += m.exits.reduce((a, b) => a + b, 0) / 4;
    acc.connected += m.connected;
    for (const k of ['sources', 'lairs', 'controllers', 'mineral', 'density']) acc[k][m[k]] = (acc[k][m[k]] || 0) + 1;
  }
  const mean = (s) => s / acc.n;
  const result = {
    n: acc.n, hung: acc.hung, wallMean: mean(acc.wall), wallSd: Math.sqrt(mean(acc.wall2) - mean(acc.wall) ** 2),
    swampMean: mean(acc.swamp), swampSd: Math.sqrt(mean(acc.swamp2) - mean(acc.swamp) ** 2),
    exitTilesMean: mean(acc.exitTiles), connected: acc.connected,
    sources: acc.sources, lairs: acc.lairs, controllers: acc.controllers, mineral: acc.mineral, density: acc.density,
  };
  console.error(cls, JSON.stringify(result), `${((Date.now() - t0) / N).toFixed(1)} ms/room`);
  return result;
}

(async () => {
  if (ONLY) return process.stdout.write(JSON.stringify(await classStats(ONLY)));
  writeFixture('mapgen/rooms.ndjson', await exactCases());
  if (!N) return; // N = 0: exact cases only, stats.json kept
  const child = (cls) => new Promise((resolve, reject) =>
    execFile(process.execPath, [__filename, String(N), cls], { maxBuffer: 1 << 24 }, (err, stdout, stderr) => {
      process.stderr.write(stderr);
      return err ? reject(err) : resolve([cls, JSON.parse(stdout)]);
    }));
  const stats = Object.fromEntries(await Promise.all(Object.keys(CLASSES).map(child)));
  const out = path.join(ROOT, 'crates', 'engine', 'tests', 'fixtures', 'mapgen', 'stats.json');
  fs.writeFileSync(out, JSON.stringify(stats, null, 1) + '\n');
  console.log(`wrote ${path.relative(ROOT, out)}`);
})();
