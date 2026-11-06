// Generates crates/util/tests/fixtures/num_*.txt: node's outputs for the
// conversions in crates/util/src/num.rs. Deterministic (seeded PRNG).
// Usage: node tools/util-gen-num-fixture.js
'use strict';
const fs = require('fs');
const path = require('path');

const outDir = path.join(__dirname, '..', 'crates', 'util', 'tests', 'fixtures');
fs.mkdirSync(outDir, { recursive: true });

const MASK = (1n << 64n) - 1n;
let state = 0x5eed1234abcd9876n;
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

const dv = new DataView(new ArrayBuffer(8));
function fromBits(b) { dv.setBigUint64(0, b); return dv.getFloat64(0); }
function bits(x) { dv.setFloat64(0, x); return dv.getBigUint64(0).toString(16).padStart(16, '0'); }
const nextUp = (x) => fromBits(BigInt('0x' + bits(x)) + 1n);
const nextDown = (x) => fromBits(BigInt('0x' + bits(x)) - 1n);

const edge = [
  0, -0, NaN, Infinity, -Infinity, 5e-324, -5e-324, 1e-323, 2.2250738585072014e-308,
  2.225073858507201e-308, 1.7976931348623157e308, -1.7976931348623157e308,
  Number.MAX_SAFE_INTEGER, 2 ** 53, 2 ** 53 + 2, 2 ** 60, 2 ** 64, 1e21, nextDown(1e21), 1e20,
  999999999999999900000, 1e-6, nextDown(1e-6), 1e-7, nextUp(1e-7), 1.5e-7, 123e-20, 0.1, 0.2, 0.3,
  0.1 + 0.2, 1 / 3, 2 / 3, 4.35, 0.5, 1.5, 2.5, -2.5, 1.005, 1.45, 8.345, 1e23, 9.5367431640625e-7,
  1.25, 1.125, 0.000001234, 0.0000001234, 99.5, 9.995, 0.05, 0.15, 0.25, 0.35, 0.45, 123.456,
  -123.456, 1e100, 1e-100, 4294967295, 4294967296, -1, 1, 10, 100, 1000, 12345678901234567890,
  0.000123, 1234.5678, 5e-7, 7e22, 1.7976931348623157e300, 3.0e-310,
];

function randomDouble() {
  switch (randInt(8)) {
    case 0: case 1: {
      // uniform over all bit patterns (finite only)
      for (;;) {
        const x = fromBits(next64());
        if (Number.isFinite(x)) return x;
      }
    }
    case 2: // subnormal
      return fromBits(next64() & ((1n << 52n) - 1n)) * (randInt(2) ? 1 : -1);
    case 3: // decimal-looking: integer / 10^j
      return (randInt(2) ? 1 : -1) * Number(next64() % 10n ** BigInt(1 + randInt(17))) / 10 ** randInt(22);
    case 4: // exact binary fractions (ties for toFixed/toPrecision)
      return (randInt(2) ? 1 : -1) * randInt(1 << 20) / 2 ** randInt(30);
    case 5: // moderate magnitudes
      return (rand() - 0.5) * 10 ** (randInt(50) - 25);
    case 6: // neighbours of powers of ten
      return [nextUp, nextDown, (x) => x][randInt(3)](10 ** (randInt(600) - 300));
    default: // integers
      return Math.floor((rand() - 0.5) * 2 ** randInt(80));
  }
}

function write(name, lines) {
  const file = path.join(outDir, name);
  fs.writeFileSync(file, lines.join('\n') + '\n');
  console.log(`${file}: ${lines.length} cases, ${fs.statSync(file).size} bytes`);
}

// Number::toString
{
  const lines = [];
  for (const x of edge) lines.push(`${bits(x)}\t${String(x)}`);
  for (let i = 0; i < 20000; i++) {
    const x = randomDouble();
    lines.push(`${bits(x)}\t${String(x)}`);
  }
  write('num_tostring.txt', lines);
}

// toFixed / toExponential / toPrecision
{
  const lines = [];
  const add = (x) => {
    const f = randInt(4) === 0 ? randInt(101) : randInt(21);
    lines.push(`f ${bits(x)} ${f}\t${x.toFixed(f)}`);
    const e = randInt(5) === 0 ? -1 : (randInt(4) === 0 ? randInt(101) : randInt(21));
    lines.push(`e ${bits(x)} ${e}\t${e < 0 ? x.toExponential() : x.toExponential(e)}`);
    const p = randInt(4) === 0 ? 1 + randInt(100) : 1 + randInt(21);
    lines.push(`p ${bits(x)} ${p}\t${x.toPrecision(p)}`);
  };
  for (const x of edge) {
    for (const f of [0, 1, 2, 3, 5, 10, 20, 100]) lines.push(`f ${bits(x)} ${f}\t${x.toFixed(f)}`);
    for (const e of [-1, 0, 1, 2, 5, 16, 20, 100]) lines.push(`e ${bits(x)} ${e}\t${e < 0 ? x.toExponential() : x.toExponential(e)}`);
    for (const p of [1, 2, 3, 6, 17, 21, 100]) lines.push(`p ${bits(x)} ${p}\t${x.toPrecision(p)}`);
  }
  for (let i = 0; i < 6000; i++) add(randomDouble());
  // Exact ties: odd / 2^j has exactly j fraction digits, the last one a 5.
  for (let j = 1; j <= 60; j++) {
    for (let r = 0; r < 4; r++) {
      const x = (2 * randInt(1 << 20) + 1) / 2 ** j * (randInt(2) ? 1 : -1);
      lines.push(`f ${bits(x)} ${j - 1}\t${x.toFixed(j - 1)}`);
      const sig = x.toExponential().replace(/e.*/, '').replace(/[-.]/g, '').length;
      if (sig >= 2 && sig <= 101) {
        lines.push(`e ${bits(x)} ${sig - 2}\t${x.toExponential(sig - 2)}`);
        lines.push(`p ${bits(x)} ${sig - 1}\t${x.toPrecision(sig - 1)}`);
      }
    }
  }
  // Integer ties: 25, 250, 1250 ... (trailing 5 before the zeros).
  for (const x of [5, 25, 250, 1250, 15, 35, 45, 125000, 9.5, 99.5, 999.5, 2 ** 53 * 5]) {
    const sig = x.toExponential().replace(/e.*/, '').replace(/[-.]/g, '').length;
    for (const p of [1, 2, 3, sig - 1].filter((p) => p >= 1)) {
      lines.push(`p ${bits(x)} ${p}\t${x.toPrecision(p)}`);
      lines.push(`e ${bits(x)} ${p - 1}\t${x.toExponential(p - 1)}`);
    }
  }
  write('num_format.txt', lines);
}

// toString(radix)
{
  const lines = [];
  for (const x of edge) for (const r of [2, 3, 7, 8, 16, 36]) lines.push(`${bits(x)} ${r}\t${x.toString(r)}`);
  for (let i = 0; i < 4000; i++) {
    let x;
    switch (randInt(4)) {
      case 0: x = randomDouble(); break;
      case 1: x = (rand() - 0.5) * 10 ** (randInt(40) - 10); break;
      case 2: x = randInt(1 << 30) / 2 ** randInt(40); break;
      default: x = Math.floor((rand() - 0.5) * 2 ** randInt(70)); break;
    }
    const r = 2 + randInt(35);
    lines.push(`${bits(x)} ${r}\t${x.toString(r)}`);
  }
  write('num_radix.txt', lines);
}

// StringToNumber
{
  const ws = ['', ' ', '\t', '\n', '\r\n', ' ', '﻿', ' ', '　', '\u0085', '​', '᠎', '\u000b\u000c'];
  const strs = [
    '', ' ', '0', '-0', '+0', '00', '007', '1.', '.5', '+.5', '-.5', '.', '+', '-', '1e', '1e+', '1e5', '1E5',
    '1e+5', '1e-5', '1.5e3', '.5e1', '5.e1', 'e5', '1_000', '1,000', 'Infinity', '+Infinity', '-Infinity',
    'infinity', 'INFINITY', 'inf', 'NaN', 'nan', '0x10', '0X1f', '0xFFFFFFFFFFFFFFFFF', '-0x10', '+0x10',
    '0x', '0x1g', '0b101', '0B2', '0o17', '0O8', '0o', '0b', '0x20000000000001', '0x20000000000003',
    '0x1fffffffffffff8', '0x1fffffffffffff800000000001', '1e400', '-1e400', '1e-400', '4.9e-324', '2.4e-324',
    '2.5e-324', '2.4703282292062328e-324', '2.4703282292062327e-324', '1.7976931348623158e308',
    '1.7976931348623157e308', '1.7976931348623159e308', '9007199254740993', '9007199254740993.0000000001',
    '0.1e1', '12abc', 'abc', '1 2', ' 12 ', '\t\n 12 \n', '١٢', '１２', '0.0000000000000000000000001',
    '123456789012345678901234567890', '1' + '0'.repeat(400), '0.' + '0'.repeat(400) + '1',
    '0x' + 'f'.repeat(300), '0b' + '1'.repeat(1100), '0o' + '7'.repeat(400),
  ];
  const lines = [];
  const add = (s) => {
    const v = Number(s);
    lines.push(`${Buffer.from(s, 'utf8').toString('hex')}\t${Number.isNaN(v) ? 'nan' : bits(v)}`);
  };
  for (const s of strs) add(s);
  for (const s of ['12', '0x1f', 'Infinity', '1.5']) for (const w of ws) { add(w + s); add(s + w); }
  for (let i = 0; i < 4000; i++) {
    const x = randomDouble();
    let s;
    switch (randInt(9)) {
      case 0: s = String(x); break;
      case 1: s = x.toExponential(randInt(25)).replace('e', pick(['e', 'E'])); break;
      case 2: s = x.toPrecision(1 + randInt(30)); break;
      case 3: s = pick(ws) + String(x) + pick(ws); break;
      case 4: s = '0x' + next64().toString(16).slice(0, 1 + randInt(16)) + (randInt(2) ? next64().toString(16) : ''); break;
      case 5: s = '0b' + next64().toString(2) + (randInt(2) ? next64().toString(2) : ''); break;
      case 6: s = '0o' + next64().toString(8); break;
      case 7: { // long decimal strings near halfway points
        const d = BigInt.asUintN(64, next64()).toString() + BigInt.asUintN(64, next64()).toString();
        s = d.slice(0, 1 + randInt(d.length)) + 'e' + (randInt(700) - 350);
        break;
      }
      default: { // random garbage-ish
        const chars = '0123456789.eE+-xXabcIn fy';
        s = '';
        for (let j = randInt(8); j > 0; j--) s += chars[randInt(chars.length)];
      }
    }
    add(s);
  }
  write('num_parse.txt', lines);
}
