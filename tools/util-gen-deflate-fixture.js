// Generates crates/util/tests/fixtures/deflate_cases.bin: varied inputs and
// node zlib's raw / zlib / gzip compressions of them (several levels and
// strategies), which util::deflate must inflate exactly.
// Usage: node tools/util-gen-deflate-fixture.js
//
// Record format: "<name> <byte length>\n<bytes>\n". "orig:<case>" holds an
// input; "<case>:<kind>-<variant>" a compression of it, kind = raw|zlib|gzip.
'use strict';
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const outFile = path.join(__dirname, '..', 'crates', 'util', 'tests', 'fixtures', 'deflate_cases.bin');

const MASK = (1n << 64n) - 1n;
let state = 0xdef1a7e0n;
function next64() {
  state = (state + 0x9e3779b97f4a7c15n) & MASK;
  let z = state;
  z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & MASK;
  z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & MASK;
  return z ^ (z >> 31n);
}
const randInt = (n) => Number(next64() % BigInt(n));
const randomBytes = (n) => Buffer.from(Array.from({ length: n }, () => randInt(256)));

const words = ['creep', 'harvest', 'energy', 'spawn', 'room', 'controller', 'the', 'a', 'of', 'to', 'W12N34', 'path', 'move', 'source'];
const text = (n) => { let s = ''; while (s.length < n) s += words[randInt(words.length)] + (randInt(8) ? ' ' : '.\n'); return Buffer.from(s.slice(0, n)); };
function memoryJson(n) {
  let s = '{"creeps":{';
  while (s.length < n) s += `"c${randInt(100000)}":{"role":"${words[randInt(5)]}","x":${randInt(50)},"y":${randInt(50)},"t":${randInt(1e8)}},`;
  return Buffer.from(s + '"end":1}}');
}
const block = randomBytes(4000);

const inputs = {
  empty: Buffer.alloc(0),
  one: Buffer.from('a'),
  text: text(3000),
  memory: memoryJson(60000),
  random: randomBytes(12000),
  runs: Buffer.concat([Buffer.alloc(70000, 'x'), Buffer.from('abcabcabcabcabc'.repeat(500)), Buffer.alloc(300, 0)]),
  counter: Buffer.from(new Uint16Array(Array.from({ length: 20000 }, (_, i) => i)).buffer),
  far: Buffer.concat([block, text(28700), block]), // repeat near the 32 KiB window edge
  bytes: Buffer.from(Array.from({ length: 256 * 20 }, (_, i) => (i * 7) & 255)),
};

const C = zlib.constants;
const variants = {
  'raw-l0': (b) => zlib.deflateRawSync(b, { level: 0 }),
  'raw-l1': (b) => zlib.deflateRawSync(b, { level: 1 }),
  'raw-l6': (b) => zlib.deflateRawSync(b, { level: 6 }),
  'raw-l9': (b) => zlib.deflateRawSync(b, { level: 9 }),
  'raw-fixed': (b) => zlib.deflateRawSync(b, { strategy: C.Z_FIXED }),
  'raw-huffman': (b) => zlib.deflateRawSync(b, { strategy: C.Z_HUFFMAN_ONLY }),
  'raw-rle': (b) => zlib.deflateRawSync(b, { strategy: C.Z_RLE }),
  'raw-filtered': (b) => zlib.deflateRawSync(b, { strategy: C.Z_FILTERED }),
  'raw-w10m1': (b) => zlib.deflateRawSync(b, { windowBits: 10, memLevel: 1 }),
  'zlib-l6': (b) => zlib.deflateSync(b),
  'zlib-l1': (b) => zlib.deflateSync(b, { level: 1 }),
  'gzip-l6': (b) => zlib.gzipSync(b),
  'gzip-l9': (b) => zlib.gzipSync(b, { level: 9 }),
};

// gzip member with FEXTRA, FNAME, FCOMMENT and FHCRC set.
function gzipAllFlags(b) {
  const head = Buffer.from([0x1f, 0x8b, 8, 4 | 8 | 16 | 2, 1, 2, 3, 4, 0, 3]);
  const extra = Buffer.from([5, 0, 1, 2, 3, 4, 5]);
  const name = Buffer.from('file.json\0');
  const comment = Buffer.from('a comment\0');
  const pre = Buffer.concat([head, extra, name, comment]);
  const hcrc = Buffer.alloc(2);
  hcrc.writeUInt16LE(zlib.crc32(pre) & 0xffff);
  const trailer = Buffer.alloc(8);
  trailer.writeUInt32LE(zlib.crc32(b), 0);
  trailer.writeUInt32LE(b.length, 4);
  return Buffer.concat([pre, hcrc, zlib.deflateRawSync(b), trailer]);
}

// Raw stream with sync-flush points (empty stored blocks mid-stream).
async function syncFlushed(b) {
  const d = zlib.createDeflateRaw();
  const chunks = [];
  d.on('data', (c) => chunks.push(c));
  for (let i = 0; i < b.length; i += 7000) {
    d.write(b.subarray(i, i + 7000));
    await new Promise((r) => d.flush(C.Z_SYNC_FLUSH, r));
  }
  d.end();
  await new Promise((r) => d.on('end', r));
  return Buffer.concat(chunks);
}

(async () => {
  const parts = [];
  const rec = (name, b) => parts.push(Buffer.from(`${name} ${b.length}\n`), b, Buffer.from('\n'));
  for (const [name, input] of Object.entries(inputs)) {
    rec(`orig:${name}`, input);
    const all = { ...variants, 'gzip-flags': gzipAllFlags, 'raw-syncflush': syncFlushed };
    for (const [v, f] of Object.entries(all)) {
      const c = await f(input);
      // Sanity: node itself must round-trip what we store.
      const back = v.startsWith('raw') ? zlib.inflateRawSync(c) : v.startsWith('zlib') ? zlib.inflateSync(c) : zlib.gunzipSync(c);
      if (!back.equals(input)) throw new Error(`${name}:${v} does not round-trip in node`);
      rec(`${name}:${v}`, c);
    }
  }
  // Two concatenated gzip members inflate to the concatenated inputs.
  rec('orig:twomembers', Buffer.concat([inputs.text, inputs.one]));
  rec('twomembers:gzip-concat', Buffer.concat([zlib.gzipSync(inputs.text), zlib.gzipSync(inputs.one)]));
  fs.writeFileSync(outFile, Buffer.concat(parts));
  console.log(`${outFile}: ${fs.statSync(outFile).size} bytes`);
})();
