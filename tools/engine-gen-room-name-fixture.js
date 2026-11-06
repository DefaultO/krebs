// Generates crates/engine/tests/fixtures/room_names.tsv from the official functions:
//   engine/src/utils.js   roomNameToXY, getRoomNameFromXY
//   backend/lib/utils.js  isBus, isCenter, isVeryCenter
//   driver/lib/path-finder.js:11 room-name regex (strict parse; numbers via common/index.js)
// Usage: node tools/engine-gen-room-name-fixture.js
//
// Columns (tab separated):
//   x y name engineX engineY strictX strictY oursX oursY highway center veryCenter sector
// x/y and the classification columns are "-" for hand-written odd names. NaN marks the
// engine's NaN results, "-" in strictX/Y marks names the strict regex rejects. oursX/Y is the
// engine function with its digit-position ladder (10 -> 3, 100 -> 4) continued past 1000, where
// upstream breaks; the script checks it equals the engine whenever the x number is below 1000.
'use strict';
const fs = require('fs');
const path = require('path');
const Module = require('module');

const root = path.join(__dirname, '..');
const ref = p => path.join(root, 'reference', p);

// The reference trees ship without node_modules: use the vendored lodash, stub everything else.
const stub = new Proxy(function () {}, { get: () => stub, apply: () => stub, construct: () => stub });
const load = Module._load;
Module._load = function (request, parent, isMain) {
    if (request === 'lodash') return load(ref('lodash-3.10.1/lodash.js'), parent, isMain);
    try { return load.apply(this, arguments); } catch (e) { return stub; }
};

const engine = require(ref('engine/src/utils.js'));
const backend = require(ref('backend/lib/utils.js'));
const common = require(ref('common/index.js'));

const strict = name => /^([WE])([0-9]+)([NS])([0-9]+)$/.test(name) ? common.roomNameToXY(name) : null;

// Sector center by the backend's own sector definition: strongholds.js:132 turns a center
// W15N25 into the sector regex ^W1\dN2\d$, so a room's center replaces each last digit with 5.
function sectorCenter(name) {
    const center = name.replace(/\d([NS])/, '5$1').replace(/\d$/, '5');
    const re = new RegExp(center.replace(/^([WE]\d*)5([NS]\d*)5$/, (s, p1, p2) => `^${p1}\\d${p2}\\d$`));
    if (!re.test(name)) throw new Error(`sector regex of ${center} does not match ${name}`);
    return center;
}

const rows = [];
const b = v => (v ? 1 : 0);
// engine/src/utils.js roomNameToXY with `verticalPos` taken from the digit count of xx.
function oursEngine(name) {
    let xx = parseInt(name.substr(1), 10);
    const verticalPos = xx >= 10 ? String(xx).length + 1 : 2;
    let yy = parseInt(name.substr(verticalPos + 1), 10);
    if (name.charAt(0) === 'W' || name.charAt(0) === 'w') xx = -xx - 1;
    if (name.charAt(verticalPos) === 'N' || name.charAt(verticalPos) === 'n') yy = -yy - 1;
    return [xx, yy];
}

function row(x, y, name) {
    const e = engine.roomNameToXY(name);
    const s = strict(name);
    const o = oursEngine(name);
    if (!(parseInt(name.substr(1), 10) >= 1000) && String(o) !== String(e)) {
        throw new Error(`oursEngine(${JSON.stringify(name)}) = ${o}, engine = ${e}`);
    }
    const known = x !== '-';
    rows.push([
        x, y, name, e[0], e[1], s ? s[0] : '-', s ? s[1] : '-', o[0], o[1],
        known ? b(backend.isBus(x) || backend.isBus(y)) : '-',
        known ? b(backend.isCenter(x, y)) : '-',
        known ? b(backend.isVeryCenter(x, y)) : '-',
        known ? sectorCenter(name) : '-',
    ].map(String).join('\t'));
}

const coord = (x, y) => row(x, y, engine.getRoomNameFromXY(x, y));
for (let x = -22; x <= 21; x++) for (let y = -22; y <= 21; y++) coord(x, y);

const far = [0, 4, 5, 6, 9, 10, 11, 99, 100, 101, 499, 500, 501, 998, 999, 1000, 1001, 1005, 9999,
    10000, 10005, 99999, 123456, 1000000, 2147483646, 2147483647];
const signed = far.flatMap(v => [v, -v - 1]);
for (const x of signed) for (const y of signed) coord(x, y);

for (const name of ['w5n5', 'e5s5', 'W5n5', 'w15N150', 'W5N5x', 'E5S5abc', 'W05N5', 'W5N05',
    'X5Y5', 'W 5 5', ' W5N5', 'W 5 5', '', 'W', 'W5', 'WN5', 'W5N', 'W-5N5', 'E-55',
    'W5N-5', 'W+5N5', 'E0S0', 'W0N0', 'sim', 'W5.5N5', 'E1e3S5', 'W99N99', 'w999s999',
    'W1000N5', 'W10000N5', 'W1000S1000', 'E99999999999S5', 'W2147483648N0', 'E0S2147483648']) {
    row('-', '-', name);
}

// Seeded fuzz: near-valid names (any letter case, leading zeros, signs, whitespace, junk) so the
// lenient engine parser is compared well beyond the hand-picked cases.
let seed = 0x9e3779b9;
const rand = n => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) % n;
};
const pick = s => s[rand(s.length)];
const digits = () => Array.from({ length: rand(6) }, () => pick('0123456789')).join('');
const junk = () => (rand(4) ? '' : pick([' ', '-', '+', '.', '\v', '\u00a0', '\u2028', '\u3000', 'x', '\u00e9', '0', '9']));
for (let i = 0; i < 3000; i++) {
    row('-', '-', junk() + pick('WENSwensX') + junk() + digits() + junk() + pick('WENSwensY') + junk() + digits() + junk());
}

const out = path.join(root, 'crates/engine/tests/fixtures/room_names.tsv');
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, rows.join('\n') + '\n');
console.log(`wrote ${out} (${rows.length} rows)`);
