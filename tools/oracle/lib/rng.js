// RNG-SEED, ORD-USER-KEY and the streams of spec 01 §16, in BigInt. Fixture-tested
// against engine::policy (tools/oracle/gen/policy-fixture.js).
'use strict';

const MASK = (1n << 64n) - 1n;

function fnv1a64(str) {
  let h = 0xcbf29ce484222325n;
  for (const b of Buffer.from(str, 'utf8')) h = ((h ^ BigInt(b)) * 0x100000001b3n) & MASK;
  return h;
}

function mix64(z) {
  z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & MASK;
  z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & MASK;
  return z ^ (z >> 31n);
}

// seed(tag, parts…): parts are BigInt ticks or fnv1a64(name).
function seed(worldSeed, tag, ...parts) {
  let h = mix64(BigInt(worldSeed) ^ fnv1a64(tag));
  for (const p of parts) h = mix64(h ^ BigInt(p));
  return h;
}

const rotl = (x, k) => ((x << BigInt(k)) | (x >> BigInt(64 - k))) & MASK;

// xoshiro256** seeded with SplitMix64 outputs from x = h. random() replaces Math.random.
function stream(h) {
  let x = BigInt(h);
  const s = [0, 1, 2, 3].map(() => {
    x = (x + 0x9e3779b97f4a7c15n) & MASK;
    return mix64(x);
  });
  const next = () => {
    const result = (rotl((s[1] * 5n) & MASK, 7) * 9n) & MASK;
    const t = (s[1] << 17n) & MASK;
    s[2] ^= s[0];
    s[3] ^= s[1];
    s[1] ^= s[2];
    s[0] ^= s[3];
    s[2] ^= t;
    s[3] = rotl(s[3], 45);
    return result;
  };
  return { next, random: () => Number(next() >> 11n) * 2 ** -53 };
}

const roomStream = (worldSeed, T, room) => stream(seed(worldSeed, 'room', BigInt(T), fnv1a64(room)));
const globalStream = (worldSeed, T) => stream(seed(worldSeed, 'global', BigInt(T)));
const cronStream = (worldSeed, G, job, unit) => stream(seed(worldSeed, 'cron', BigInt(G), fnv1a64(job), fnv1a64(unit)));
const orderKey = (worldSeed, T, uid) => seed(worldSeed, 'user-order', BigInt(T), fnv1a64(uid));

// Canonical array index ("0".."4294967294").
const arrayIndex = (k) => (/^(0|[1-9]\d{0,9})$/.test(k) && Number(k) < 4294967295 ? Number(k) : null);

// ORD-POINTS 1: integer-like ids ascending first, then players by (orderKey, uid bytes).
function sortUsers(worldSeed, T, uids) {
  const key = (u) => [arrayIndex(u), arrayIndex(u) === null ? orderKey(worldSeed, T, u) : 0n, Buffer.from(u, 'utf8')];
  return uids
    .map((u) => [u, key(u)])
    .sort(([, [ia, ka, ba]], [, [ib, kb, bb]]) => {
      if ((ia === null) !== (ib === null)) return ia === null ? 1 : -1;
      if (ia !== null) return ia - ib;
      if (ka !== kb) return ka < kb ? -1 : 1;
      return Buffer.compare(ba, bb);
    })
    .map(([u]) => u);
}

module.exports = { MASK, fnv1a64, mix64, seed, stream, roomStream, globalStream, cronStream, orderKey, sortUsers, arrayIndex };
