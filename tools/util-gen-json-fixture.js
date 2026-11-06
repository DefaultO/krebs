// Generates crates/util/tests/fixtures/json_cases.txt: JSON texts with node's
// JSON.stringify(JSON.parse(text)) output (compact and indented), plus texts
// JSON.parse rejects. Deterministic (seeded PRNG).
// Usage: node tools/util-gen-json-fixture.js
//
// Record format: "<tag> <byte length>\n<bytes>\n". Each case starts with an
// "in" record followed by "err" (empty) or "c" / "p<indent>" records.
'use strict';
const fs = require('fs');
const path = require('path');

const outFile = path.join(__dirname, '..', 'crates', 'util', 'tests', 'fixtures', 'json_cases.txt');

const MASK = (1n << 64n) - 1n;
let state = 0x0123456789abcdefn;
function next64() {
  state = (state + 0x9e3779b97f4a7c15n) & MASK;
  let z = state;
  z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & MASK;
  z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & MASK;
  return z ^ (z >> 31n);
}
const rand = () => Number(next64() >> 11n) / 2 ** 53;
const randInt = (n) => Number(next64() % BigInt(n));
const pick = (a) => a[randInt(a.length)];

const ws = () => (randInt(4) ? '' : pick([' ', '\n', '\t', '\r\n', '  ', ' \t\n ']));

const specialChars = ['"', '\\', '/', '\b', '\f', '\n', '\r', '\t', '\u0000', '\u0001', '\u001f', '\u007f',
  'é', '中', ' ', ' ', '﻿', ' ', '😀', '𝄞', '퟿', '', '￿'];
function randString() {
  let s = '';
  for (let n = randInt(4) === 0 ? randInt(40) : randInt(8); n > 0; n--) {
    s += randInt(3) ? String.fromCharCode(32 + randInt(95)) : pick(specialChars);
  }
  return s;
}
const indexKeys = ['0', '1', '2', '10', '42', '007', '01', '-1', '1.0', '4294967294', '4294967295', '4294967296', '__proto__', '', 'constructor'];
const randKey = () => (randInt(4) === 0 ? pick(indexKeys) : randString());

// Serialises a string as a JSON literal, randomly choosing escape styles.
function quote(s) {
  let out = '"';
  for (const ch of s) {
    const cp = ch.codePointAt(0);
    const u = (c) => '\\u' + c.toString(16).padStart(4, '0')[randInt(2) ? 'toUpperCase' : 'toLowerCase']();
    if (ch === '"') out += '\\"';
    else if (ch === '\\') out += '\\\\';
    else if (cp < 0x20) out += randInt(2) && '\b\f\n\r\t'.includes(ch) ? JSON.stringify(ch).slice(1, -1) : u(cp);
    else if (ch === '/' && randInt(2)) out += '\\/';
    else if (randInt(6) === 0) {
      out += cp > 0xffff ? u(0xd800 + ((cp - 0x10000) >> 10)) + u(0xdc00 + ((cp - 0x10000) & 0x3ff)) : u(cp);
    } else out += ch;
  }
  return out + '"';
}

const numberTexts = ['0', '-0', '1', '-1', '0.5', '-0.0', '1.0', '1e0', '1E+2', '1e-7', '1.5e-7', '0e10', '10E-1',
  '123456789012345678901234567890', '9007199254740993', '1e21', '1e400', '-1e400', '1e-400', '5e-324',
  '0.1', '0.30000000000000004', '1.7976931348623157e308', '2.2250738585072014e-308', '4294967295',
  '18446744073709551615', '18446744073709551616', '9999999999999999999', '99999999999999999999',
  '0.000001', '0.0000001', '100000000000000000000', '1234.5678e-2'];
function randNumberText() {
  switch (randInt(5)) {
    case 0: return pick(numberTexts);
    case 1: return String(randInt(1000000) - 500000);
    case 2: return String((rand() - 0.5) * 10 ** (randInt(60) - 30));
    case 3: return ((rand() - 0.5) * 10 ** (randInt(40) - 20)).toExponential(randInt(18)).replace('e', pick(['e', 'E']));
    default: return (rand() * 1000).toFixed(randInt(8));
  }
}

function randValue(depth) {
  const r = randInt(depth > 4 ? 6 : 9);
  switch (r) {
    case 0: return 'null';
    case 1: return pick(['true', 'false']);
    case 2: case 3: return randNumberText();
    case 4: case 5: return quote(randString());
    case 6: case 7: { // object, sometimes with duplicate keys
      const keys = [];
      for (let n = randInt(8); n > 0; n--) keys.push(keys.length && randInt(6) === 0 ? pick(keys) : randKey());
      return '{' + ws() + keys.map((k) => quote(k) + ws() + ':' + ws() + randValue(depth + 1)).join(ws() + ',' + ws()) + ws() + '}';
    }
    default: {
      const items = [];
      for (let n = randInt(6); n > 0; n--) items.push(randValue(depth + 1));
      return '[' + ws() + items.join(ws() + ',' + ws()) + ws() + ']';
    }
  }
}

// A Memory-like document of a few hundred KB.
function memoryDoc() {
  const creeps = {};
  for (let i = 0; i < 1500; i++) {
    creeps[`${pick(['harvester', 'upgrader', 'builder', 'hauler'])}_${i}`] = {
      role: pick(['harvester', 'upgrader', 'builder', 'hauler']),
      room: `W${randInt(60)}N${randInt(60)}`,
      working: randInt(2) === 0,
      target: randInt(3) ? next64().toString(16).slice(0, 15) : null,
      _move: { dest: { x: randInt(50), y: randInt(50), room: `E${randInt(60)}S${randInt(60)}` }, time: 12345678 + randInt(100000), path: Array.from({ length: randInt(40) }, () => randInt(9)).join('') },
      cost: rand() * 1000,
    };
  }
  const rooms = {};
  for (let i = 0; i < 200; i++) {
    rooms[`W${i}N${i % 7}`] = { sources: Array.from({ length: 2 }, () => next64().toString(16)), avoid: randInt(2), level: randInt(9), stats: { energy: rand() * 300000, cpu: rand() * 20 } };
  }
  const stats = {};
  for (let i = 0; i < 300; i++) stats[String(30000000 + randInt(1000000))] = { cpu: rand() * 50, bucket: randInt(10001) };
  return JSON.stringify({ creeps, rooms, stats, spawns: {}, flags: {}, version: '1.2.3' }, null, '\t');
}

const invalid = ['', ' ', '01', '-01', '1.', '.5', '+1', '1e', '1e+', '-', '--1', 'NaN', 'Infinity', '-Infinity',
  '[1,]', '[,1]', '{,}', '{"a"}', '{"a":}', '{"a":1,}', '{a:1}', "{'a':1}", "'a'", '"abc', '"\\x"', '"\\u12"',
  '"\\u12G4"', '"\t"', '"\n"', '"\u0000"', 'tru', 'nul', 'True', '[', ']', '{', '}', '1 2', '[1 2]', '{"a" 1}',
  '"a"x', 'null null', ' 1', '﻿1', '0x10', '[1]]', '{"a":1}}', ' []'];

const lines = [];
const rec = (tag, s) => {
  const b = Buffer.from(s, 'utf8');
  lines.push(Buffer.from(`${tag} ${b.length}\n`), b, Buffer.from('\n'));
};
function addCase(text, indents) {
  rec('in', text);
  let v;
  try { v = JSON.parse(text); } catch (e) { rec('err', ''); return; }
  rec('c', JSON.stringify(v));
  for (const n of indents) rec('p' + n, JSON.stringify(v, null, n));
}

for (const s of invalid) addCase(s, []);
for (const s of ['0', '-0', ' "x" ', 'true', 'null', '[]', '{}', '[[]]', '{"":{}}', '"\\ud83d\\ude00"', '"\\uD834\\uDD1E"',
  '{"b":1,"a":2,"1":3,"0":4}', '{"a":1,"a":2}', '{"a":1,"b":2,"a":3}', '{"__proto__":[1]}', '{"4294967295":1,"4294967294":2,"1":3}'])
  addCase(s, [2]);
addCase('['.repeat(500) + ']'.repeat(500), []);
addCase('['.repeat(400) + '{"a":' .repeat(100) + '1' + '}'.repeat(100) + ']'.repeat(400), [1]);
for (let i = 0; i < 1500; i++) addCase(ws() + randValue(0) + ws(), randInt(4) === 0 ? [pick([1, 2, 4, 10, 12])] : []);
addCase(memoryDoc(), []);

fs.writeFileSync(outFile, Buffer.concat(lines));
console.log(`${outFile}: ${fs.statSync(outFile).size} bytes`);
