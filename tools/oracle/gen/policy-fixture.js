// Generates crates/engine/tests/fixtures/policy/cases.ndjson: RNG-SEED (fnv1a64, mix64,
// seed, streams), ORD-USER-KEY and the room user order, lodash 3.10.1 _.random/_.shuffle/
// _.sample draw patterns (the real lodash drawing from a stream), and vclock.
// u64 values are 16-digit hex strings. Usage: node tools/oracle/gen/policy-fixture.js
'use strict';
const { reference, prng, writeFixture } = require('../lib/common');
const rng = require('../lib/rng');
const vclock = require('../lib/vclock');

const hex = (b) => b.toString(16).padStart(16, '0');
const g = prng(0x9011c7);
const u64 = () => (BigInt(g.int(2 ** 32)) << 32n) | BigInt(g.int(2 ** 32));

// The real lodash, its Math.random reading whichever stream is current.
let current = null;
const fakeMath = Object.create(Math);
fakeMath.random = () => current.random();
const _ = require(reference('lodash-3.10.1/lodash.js')).runInContext({ Math: fakeMath });

const names = ['', 'a', 'W1N1', 'E127S127', 'room', 'user-order', 'genInvaders', 'W0N0', 'é', '中文', '😀x', 'roomsForceUpdate'];
const randName = () => (g.chance(0.5) ? g.pick(names) : String.fromCharCode(...Array.from({ length: g.int(12) }, () => 32 + g.int(95))));
const uidPool = ['2', '3', '10', '07', '0', '4294967294', '4294967295', '5f3a0c1d2e4b6a7', 'a1b2c3d4e5f6a7b', 'ffffffffffffffff', '123456789012345', 'user', 'Z'];
const randUid = () => (g.chance(0.4) ? g.pick(uidPool) : u64().toString(16).slice(0, 15));

const lines = [];
for (let i = 0; i < 200; i++) {
  const s = randName();
  lines.push({ kind: 'fnv', s, h: hex(rng.fnv1a64(s)) });
  const z = u64();
  lines.push({ kind: 'mix', z: hex(z), h: hex(rng.mix64(z)) });
  const ws = u64();
  const tag = g.pick(['room', 'global', 'cron', 'user-order', 'gen-edge', randName()]);
  const parts = Array.from({ length: g.int(4) }, () => (g.chance(0.5) ? BigInt(g.int(1e6)) : u64()));
  lines.push({ kind: 'seed', ws: hex(ws), tag, parts: parts.map(hex), h: hex(rng.seed(ws, tag, ...parts)) });
}

for (let i = 0; i < 100; i++) {
  const h = u64();
  const s = rng.stream(h);
  const next = [0, 1, 2, 3].map(() => hex(s.next()));
  lines.push({ kind: 'stream', h: hex(h), next, random: [0, 1, 2, 3].map(() => s.random()) });

  const ws = u64();
  const T = g.int(1e7);
  const [room, job, unit] = [randName(), randName(), randName()];
  const draw = (st) => [0, 1, 2].map(() => st.random());
  lines.push({
    kind: 'streams', ws: hex(ws), T, room, job, unit,
    room_r: draw(rng.roomStream(ws, T, room)), global_r: draw(rng.globalStream(ws, T)), cron_r: draw(rng.cronStream(ws, T, job, unit)),
  });

  const uids = [...new Set(Array.from({ length: 1 + g.int(8) }, randUid))];
  lines.push({ kind: 'order', ws: hex(ws), T, uids, keys: uids.map((u) => hex(rng.orderKey(ws, T, u))), sorted: rng.sortUsers(ws, T, uids) });
}

const bounds = [[0, 1], [1, 5], [0, 0], [-3, 3], [5, 1], [0, 99], [1.5, 3], [0, 0.5], [-1.25, 7], [10, 10.5], [1e9, 1e9 + 7], [0, 1e15]];
for (let i = 0; i < 150; i++) {
  const h = u64();
  current = rng.stream(h);
  const calls = Array.from({ length: 1 + g.int(6) }, () => {
    const [min, max] = g.pick(bounds);
    const floating = g.chance(0.3);
    return [min, max, floating, _.random(min, max, floating)];
  });
  lines.push({ kind: 'random', h: hex(h), calls, after: current.random() });

  const h2 = u64();
  current = rng.stream(h2);
  const a = Array.from({ length: g.int(20) }, (_x, j) => j * 3 + g.int(3));
  lines.push({ kind: 'shuffle', h: hex(h2), a, out: _.shuffle(a), after: current.random() });

  const h3 = u64();
  current = rng.stream(h3);
  const b = a.slice(0, g.int(5));
  const out = _.sample(b);
  lines.push({ kind: 'sample', h: hex(h3), a: b, out: out === undefined ? null : out, after: current.random() });
}

for (let i = 0; i < 50; i++) {
  const clock = { baseMs: g.int(2e12) - 1e11, baseTick: g.int(1e6), nominalTickMs: g.pick([1000, 1, 50, 3000, g.int(5000)]) };
  const T = Math.max(0, clock.baseTick + g.int(1e6) - 2e5);
  const rebaseT = clock.baseTick + g.int(1e5);
  const nominal = g.pick([1000, 10, 4000]);
  const after = vclock.rebase(clock, rebaseT, nominal);
  lines.push({ kind: 'vclock', clock, T, at: vclock.at(clock, T), rebaseT, nominal, after, at2: vclock.at(after, T) });
}

writeFixture('policy/cases.ndjson', lines);
