'use strict';
// Records differential fixtures for crates/pathfinder from the official native PathFinder.
//
//   E:\screeps\node22\node-v22.22.3-win-x64\node.exe tools/pf-gen-fixture.js [cases]
//
// Queries go through the official JS glue (driver/lib/path-finder.js) so option clamping is
// covered too; the native addon call is intercepted to record the normalized arguments.
// Output (crates/pathfinder/tests/fixtures/):
//   world.txt      room terrains, the cost matrix pool, room callback profiles
//   cases-NN.txt   one query per line, see `caseLine` for the format
// Deterministic: same seed, same files.
const fs = require('fs');
const path = require('path');

const DRIVER = process.env.SCREEPS_DRIVER || 'E:/screeps/server/node_modules/@screeps/driver';
const native = require(DRIVER + '/native/build/Release/native.node');
const glue = require(DRIVER + '/lib/path-finder.js');
const OUT = path.join(__dirname, '../crates/pathfinder/tests/fixtures');
const CASES = Number(process.argv[2]) || 8000;
const MAX_FILE = 1500000;

// mulberry32
let seed = 0x5eed2026;
function rand() {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
const ri = (a, b) => a + Math.floor(rand() * (b - a + 1));
const pick = arr => arr[Math.floor(rand() * arr.length)];
const chance = p => rand() < p;

// Room coords: E0 = 0, W0 = -1, S0 = 0, N0 = -1 (same as the Rust RoomXY)
const roomName = (x, y) => (x < 0 ? 'W' + (-x - 1) : 'E' + x) + (y < 0 ? 'N' + (-y - 1) : 'S' + y);
function parseRoom(name) {
    const m = /^([WE])(\d+)([NS])(\d+)$/.exec(name);
    return { x: m[1] === 'W' ? -Number(m[2]) - 1 : Number(m[2]), y: m[3] === 'N' ? -Number(m[4]) - 1 : Number(m[4]) };
}

// ---- World: W5..E4 x N5..S4, walled outer edge, matching exits between neighbours ----
const X0 = -5, X1 = 4, Y0 = -5, Y1 = 4;
const inWorld = (x, y) => x >= X0 && x <= X1 && y >= Y0 && y <= Y1;

function exitSpans() {
    const e = new Array(50).fill(false);
    if (chance(0.1)) return e;
    for (let n = ri(1, 4); n > 0; --n) {
        const len = ri(1, 10), start = ri(1, 48 - len + 1);
        for (let i = start; i < start + len && i <= 48; ++i) e[i] = true;
    }
    return e;
}
const vExits = {}, hExits = {}; // vExits[x,y]: between (x,y) and (x+1,y); hExits: (x,y) and (x,y+1)
for (let x = X0; x <= X1; ++x) {
    for (let y = Y0; y <= Y1; ++y) {
        if (x < X1) vExits[x + ',' + y] = exitSpans();
        if (y < Y1) hExits[x + ',' + y] = exitSpans();
    }
}
const none = new Array(50).fill(false);

function genTerrain(rx, ry) {
    const t = new Uint8Array(2500); // y * 50 + x
    const style = pick(['cave', 'cave', 'open', 'maze', 'swampy', 'highway']);
    const density = { cave: 0.45, open: 0.3, maze: 0.5, swampy: 0.4, highway: 0.05 }[style];
    let wall = new Uint8Array(2500);
    for (let i = 0; i < 2500; ++i) wall[i] = chance(density) ? 1 : 0;
    const iters = style === 'maze' ? 1 : style === 'highway' ? 0 : 4;
    for (let it = 0; it < iters; ++it) {
        const next = new Uint8Array(2500);
        for (let y = 0; y < 50; ++y) {
            for (let x = 0; x < 50; ++x) {
                let n = 0;
                for (let dy = -1; dy <= 1; ++dy) for (let dx = -1; dx <= 1; ++dx) {
                    const xx = x + dx, yy = y + dy;
                    n += xx < 0 || yy < 0 || xx > 49 || yy > 49 ? 1 : wall[yy * 50 + xx];
                }
                next[y * 50 + x] = n >= 5 ? 1 : 0;
            }
        }
        wall = next;
    }
    const swampP = style === 'swampy' ? 0.6 : style === 'highway' ? 0.02 : 0.2;
    let swamp = new Uint8Array(2500);
    for (let i = 0; i < 2500; ++i) swamp[i] = chance(swampP) ? 1 : 0;
    if (style !== 'highway') {
        for (let it = 0; it < 3; ++it) {
            const next = new Uint8Array(2500);
            for (let y = 1; y < 49; ++y) for (let x = 1; x < 49; ++x) {
                let n = 0;
                for (let dy = -1; dy <= 1; ++dy) for (let dx = -1; dx <= 1; ++dx) n += swamp[(y + dy) * 50 + x + dx];
                next[y * 50 + x] = n >= 5 ? 1 : 0;
            }
            swamp = next;
        }
    }
    for (let i = 0; i < 2500; ++i) {
        t[i] = wall[i] ? (chance(0.02) ? 3 : 1) : swamp[i] ? 2 : 0;
    }
    // Room edges: walls except exits, which match the neighbouring room
    const left = inWorld(rx - 1, ry) ? vExits[(rx - 1) + ',' + ry] : none;
    const right = inWorld(rx + 1, ry) ? vExits[rx + ',' + ry] : none;
    const top = inWorld(rx, ry - 1) ? hExits[rx + ',' + (ry - 1)] : none;
    const bottom = inWorld(rx, ry + 1) ? hExits[rx + ',' + ry] : none;
    const open = (x, y) => { if (t[y * 50 + x] & 1) t[y * 50 + x] = chance(0.2) ? 2 : 0; };
    for (let i = 0; i < 50; ++i) {
        t[i * 50 + 0] = 1; t[i * 50 + 49] = 1; t[i] = 1; t[49 * 50 + i] = 1;
    }
    for (let i = 1; i < 49; ++i) {
        // exits plus a usually-clear approach two tiles deep
        if (left[i]) { t[i * 50] = chance(0.2) ? 2 : 0; if (chance(0.9)) { open(1, i); open(2, i); } }
        if (right[i]) { t[i * 50 + 49] = chance(0.2) ? 2 : 0; if (chance(0.9)) { open(48, i); open(47, i); } }
        if (top[i]) { t[i] = chance(0.2) ? 2 : 0; if (chance(0.9)) { open(i, 1); open(i, 2); } }
        if (bottom[i]) { t[49 * 50 + i] = chance(0.2) ? 2 : 0; if (chance(0.9)) { open(i, 48); open(i, 47); } }
    }
    return t;
}

const rooms = [];
const terrain = {};
for (let x = X0; x <= X1; ++x) {
    for (let y = Y0; y <= Y1; ++y) {
        const t = genTerrain(x, y);
        terrain[roomName(x, y)] = t;
        rooms.push({ room: roomName(x, y), terrain: Array.from(t).join('') });
    }
}

// ---- Cost matrix pool (x * 50 + y layout, like CostMatrix._bits) ----
function genMatrix(kind) {
    const m = new Uint8Array(2500);
    const set = (x, y, v) => { if (x >= 0 && y >= 0 && x < 50 && y < 50) m[x * 50 + y] = v; };
    switch (kind) {
        case 'zeros': break;
        case 'sparse':
            for (let i = 0; i < 2500; ++i) {
                if (chance(0.03)) m[i] = 255; else if (chance(0.05)) m[i] = pick([1, 1, 2, 3, 5, 10, 20]);
            }
            break;
        case 'roads':
            for (let r = ri(1, 6); r > 0; --r) {
                let x = ri(1, 48), y = ri(1, 48);
                const dx = ri(-1, 1), dy = ri(-1, 1);
                for (let s = ri(5, 60); s > 0; --s) {
                    set(x, y, 1);
                    x += chance(0.7) ? dx : ri(-1, 1); y += chance(0.7) ? dy : ri(-1, 1);
                    if (x < 1 || y < 1 || x > 48 || y > 48) break;
                }
            }
            for (let i = 0; i < 2500; ++i) if (chance(0.01)) m[i] = 255;
            break;
        case 'blocks':
            for (let r = ri(1, 8); r > 0; --r) {
                const x = ri(0, 49), y = ri(0, 49), w = ri(1, 12), h = ri(1, 12), v = pick([255, 255, 50, 2, 5]);
                for (let xx = x; xx < x + w; ++xx) for (let yy = y; yy < y + h; ++yy) set(xx, yy, v);
            }
            break;
        case 'dense':
            for (let i = 0; i < 2500; ++i) m[i] = ri(0, 255);
            break;
        case 'gradient': {
            const cx = ri(0, 49), cy = ri(0, 49);
            for (let x = 0; x < 50; ++x) for (let y = 0; y < 50; ++y) m[x * 50 + y] = 1 + Math.max(Math.abs(x - cx), Math.abs(y - cy)) % 7;
            break;
        }
        case 'creeps':
            for (let i = ri(1, 40); i > 0; --i) set(ri(1, 48), ri(1, 48), 255);
            break;
        case 'wallopen': // nonzero values on wall tiles, including room edges
            for (let i = 0; i < 2500; ++i) if (chance(0.2)) m[i] = ri(1, 254);
            for (let i = 0; i < 50; ++i) if (chance(0.3)) set(pick([0, 49]), i, ri(1, 10));
            for (let i = 0; i < 50; ++i) if (chance(0.3)) set(i, pick([0, 49]), ri(1, 10));
            break;
    }
    return m;
}
const matrixKinds = [];
const matrices = [];
for (const [kind, n] of [['zeros', 2], ['sparse', 10], ['roads', 10], ['blocks', 8], ['dense', 3], ['gradient', 3], ['creeps', 8], ['wallopen', 4]]) {
    for (let i = 0; i < n; ++i) { matrices.push(genMatrix(kind)); matrixKinds.push(kind); }
}
const plainMatrices = matrices.map((_, i) => i).filter(i => matrixKinds[i] !== 'wallopen');
const wallOpenMatrices = matrices.map((_, i) => i).filter(i => matrixKinds[i] === 'wallopen');

// ---- Room callback profiles: room -> M<k> | F (false) | T (throw) | X (wrong-length array) ----
// Absent rooms return undefined. Profile 'none' passes no roomCallback at all. A 'shared' profile
// copies every M<k> into one scratch Uint8Array and returns that, like a bot reusing one matrix:
// the addon keeps a pointer, so rooms entered earlier see the later writes.
const profiles = [];
profiles.push({ none: true, map: {} });
profiles.push({ none: true, map: {} });
profiles.push({ none: false, map: {} });
for (const [pm, pf, px, wallOpen, throws] of [
    [0.3, 0, 0, false, false], [0.6, 0, 0, false, false], [0.95, 0, 0, false, false],
    [0.3, 0.05, 0, false, false], [0.5, 0.1, 0.02, false, false], [0.8, 0.2, 0, false, false],
    [0.5, 0.3, 0, false, false], [0.2, 0.02, 0.05, false, false], [0.7, 0.05, 0, false, false],
    [0.6, 0.05, 0, true, false], [0.9, 0, 0, true, false], [0.5, 0.05, 0, false, true],
    [0.4, 0, 0, false, true], [0.5, 0.1, 0, false, false], [0.9, 0.1, 0, false, false],
]) {
    const map = {};
    for (const r of rooms) {
        if (chance(pf)) map[r.room] = 'F';
        else if (chance(px)) map[r.room] = 'X';
        else if (chance(pm)) map[r.room] = 'M' + (wallOpen && chance(0.5) ? pick(wallOpenMatrices) : pick(plainMatrices));
    }
    if (throws) map[pick(rooms).room] = 'T';
    profiles.push({ none: false, map });
}

// ---- Run through the official glue ----
let nativeArgs;
const mod = {
    version: native.version,
    loadTerrain: native.loadTerrain,
    search(...args) { nativeArgs = args; return native.search(...args); },
};
glue.init(mod, rooms);
const pf = glue.create(mod);
class RoomPosition {
    constructor(x, y, roomName) { this.x = x; this.y = y; this.roomName = roomName; }
}
pf.make({ RoomPosition });

function randomTile(room, walkable) {
    const t = terrain[room];
    for (let tries = 0; tries < 200; ++tries) {
        const x = ri(0, 49), y = ri(0, 49);
        if (!walkable || !t) return { x, y };
        if (x > 0 && y > 0 && x < 49 && y < 49 && !(t[y * 50 + x] & 1)) return { x, y };
    }
    return { x: ri(1, 48), y: ri(1, 48) };
}

function randomPos(base, spread) {
    const rx = base.x + ri(-spread, spread), ry = base.y + ri(-spread, spread);
    const room = roomName(rx, ry);
    const t = chance(0.75) ? randomTile(room, true) : randomTile(room, false);
    return new RoomPosition(t.x, t.y, room);
}

const optOr = (p, values) => chance(p) ? undefined : pick(values);

function genQuery() {
    if (chance(0.07)) { // long haul across the world, many rooms and callbacks
        const options = {
            maxOps: pick([20000, 50000, 100000]),
            maxRooms: pick([32, 64, 64]),
            swampCost: optOr(0.5, [1, 2, 10]),
            heuristicWeight: optOr(0.5, [1, 1.5, 3]),
        };
        for (const k of Object.keys(options)) if (options[k] === undefined) delete options[k];
        return {
            origin: randomPos({ x: ri(X0, X1), y: ri(Y0, Y1) }, 0),
            goals: [{ pos: randomPos({ x: ri(X0, X1), y: ri(Y0, Y1) }, 0), range: pick([0, 1, 3]) }],
            options,
            profile: pick([0, 2, 3, 4, 5, 6, 10, 11, 17]),
        };
    }
    const originRoom = { x: ri(X0, X1), y: ri(Y0, Y1) };
    const r = rand();
    let origin;
    if (r < 0.85) origin = randomPos(originRoom, 0);
    else {
        const t = randomTile(roomName(originRoom.x, originRoom.y), false);
        if (r < 0.93) { if (chance(0.5)) t.x = pick([0, 49]); else t.y = pick([0, 49]); }
        origin = new RoomPosition(t.x, t.y, roomName(originRoom.x, originRoom.y));
    }
    const flee = chance(0.12);
    const ng = chance(0.7) ? 1 : chance(0.65) ? ri(2, 4) : chance(0.8) ? ri(5, 12) : ri(20, 40);
    const goals = [];
    for (let i = 0; i < ng; ++i) {
        const spread = flee ? pick([0, 0, 0, 1]) : pick([0, 0, 0, 0, 1, 1, 1, 2, 3, 4, 6, 9]);
        let pos = randomPos(parseRoom(origin.roomName), spread);
        if (!flee && chance(0.02)) pos = randomPos({ x: ri(-60, 60), y: ri(-60, 60) }, 0);
        if (flee && chance(0.7)) pos = new RoomPosition(Math.min(49, Math.max(0, origin.x + ri(-4, 4))), Math.min(49, Math.max(0, origin.y + ri(-4, 4))), origin.roomName);
        if (chance(0.03)) pos = new RoomPosition(origin.x, origin.y, origin.roomName);
        let range;
        if (flee) range = pick([1, 2, 3, 4, 5, 6, 8, 10, 12, 15, 20, 30]);
        else if (chance(0.3)) range = undefined;
        else range = pick([0, 0, 1, 1, 1, 2, 3, 4, 5, 7, 10, 20, 50, -2, 2.7]);
        goals.push(range === undefined ? pos : { pos, range });
    }
    const options = {
        plainCost: optOr(0.65, [1, 2, 3, 5, 10, 0, -3, 300, 2.5, 254, 255]),
        swampCost: optOr(0.65, [1, 2, 4, 5, 10, 25, 0, 1000, 4.9]),
        heuristicWeight: optOr(0.6, [1, 1.1, 1.2, 1.5, 2, 3.3, 9, 10, 0.5, 0, -1, 2.123456789]),
        maxOps: optOr(0.5, [1, 2, 5, 10, 50, 100, 500, 1000, 3000, 10000, 20000, 0, -10]),
        maxRooms: optOr(0.55, [1, 2, 3, 4, 8, 16, 32, 64, 100, 0, -1]),
        maxCost: optOr(0.8, [1, 5, 10, 50, 100, 200, 500, 1000, 0xffffffff, Infinity, 2.5]),
        flee,
    };
    for (const k of Object.keys(options)) if (options[k] === undefined) delete options[k];
    return { origin, goals: ng === 1 && chance(0.5) ? goals[0] : goals, options, profile: ri(0, profiles.length - 1) };
}

const posStr = p => p.roomName + ' ' + p.x + ' ' + p.y;
const DIRS = { '0,-1': 1, '1,-1': 2, '1,0': 3, '1,1': 4, '0,1': 5, '-1,1': 6, '-1,0': 7, '-1,-1': 8 };
const world = p => { const r = parseRoom(p.roomName); return { x: r.x * 50 + p.x, y: r.y * 50 + p.y }; };

// Line format:
//   O <room> <x> <y> G <n> (<range> <room> <x> <y>)*      origin, normalized goals
//   J <plain|0> <swamp|0> <weight|-> <maxOps|0> <maxCost|0> <maxRooms|0> <flee>  JS options after
//     `| 0`; weight is ToNumber(option), or - when the option is falsy
//   N <plain> <swamp> <maxRooms> <maxOps> <maxCost> <flee> <weight>             native arguments
//   P <profile> C <n> <room>*                             callback profile, rooms it was called for
//   = ok <ops> <cost> <incomplete> <dirs|->               dirs: path as directions 1..8 from origin
//   = err <terrain|throw>
function caseLine(q) {
    const calls = [];
    const prof = profiles[q.profile];
    const options = Object.assign({}, q.options);
    const scratch = new Uint8Array(2500);
    if (!prof.none) {
        options.roomCallback = function (name) {
            calls.push(name);
            const c = prof.map[name];
            if (c === undefined) return undefined;
            if (c === 'F') return false;
            if (c === 'T') throw new Error('callback threw');
            if (c === 'X') return { _bits: new Uint8Array(100) };
            if (prof.shared) { scratch.set(matrices[Number(c.slice(1))]); return { _bits: scratch }; }
            return { _bits: matrices[Number(c.slice(1))] };
        };
    }
    nativeArgs = undefined;
    let result;
    try {
        const ret = pf.search(q.origin, q.goals, options);
        let prev = world(q.origin);
        let dirs = '';
        for (const p of ret.path) {
            const w = world(p);
            const d = DIRS[(w.x - prev.x) + ',' + (w.y - prev.y)];
            if (!d) throw new Error('non-adjacent path step');
            dirs += d;
            prev = w;
        }
        result = ['ok', ret.ops, ret.cost, ret.incomplete ? 1 : 0, dirs || '-'].join(' ');
    } catch (e) {
        if (e.message === 'non-adjacent path step') throw e;
        result = 'err ' + (e.message === 'Could not load terrain data' ? 'terrain' : e.message === 'callback threw' ? 'throw' : e.message);
    }
    if (!nativeArgs) throw new Error('native search not reached');
    const [, ngoals, , plain, swamp, maxRooms, maxOps, maxCost, flee, weight] = nativeArgs;
    const goals = (Array.isArray(q.goals) ? q.goals : [q.goals]).map((g, i) => {
        const p = g.pos || g;
        return ngoals[i].range + ' ' + posStr(p);
    });
    const o = q.options;
    const i32 = v => v | 0;
    return [
        'O', posStr(q.origin), 'G', goals.length, goals.join(' '),
        'J', i32(o.plainCost), i32(o.swampCost), o.heuristicWeight ? String(Number(o.heuristicWeight)) : '-',
        i32(o.maxOps), i32(o.maxCost), i32(o.maxRooms), o.flee ? 1 : 0,
        'N', plain, swamp, maxRooms, maxOps, maxCost, flee ? 1 : 0, String(weight),
        'P', q.profile, 'C', calls.length, calls.join(' '),
        '=', result,
    ].join(' ').replace(/ +/g, ' ');
}

fs.mkdirSync(OUT, { recursive: true });
for (const f of fs.readdirSync(OUT)) if (/^cases-\d+\.txt$/.test(f)) fs.unlinkSync(path.join(OUT, f));

let file = 0, buf = [], size = 0;
const stats = { ok: 0, incomplete: 0, err: 0, empty: 0, steps: 0 };
const flush = () => {
    fs.writeFileSync(path.join(OUT, 'cases-' + String(file).padStart(2, '0') + '.txt'), buf.join('\n') + '\n');
    ++file; buf = []; size = 0;
};
const record = q => {
    const line = caseLine(q);
    const res = line.slice(line.indexOf(' = ') + 3).split(' ');
    if (res[0] === 'err') ++stats.err;
    else {
        ++stats.ok;
        if (res[3] === '1') ++stats.incomplete;
        if (res[4] === '-') ++stats.empty; else stats.steps += res[4].length;
    }
    if (size + line.length > MAX_FILE) flush();
    buf.push(line);
    size += line.length + 1;
};
for (let n = 0; n < CASES; ++n) record(genQuery());

// ---- Edge cases, generated after the main ones so those keep their random stream ----
const firstShared = profiles.length;
for (const pm of [0.5, 0.8, 0.95]) {
    const map = {};
    for (const r of rooms) {
        if (chance(0.05)) map[r.room] = 'F';
        else if (chance(pm)) map[r.room] = 'M' + pick(plainMatrices);
    }
    profiles.push({ none: false, shared: true, map });
}
let edge = 0;
for (let n = 0; n < 400; ++n, ++edge) { // one reused scratch matrix
    const q = genQuery();
    q.profile = ri(firstShared, profiles.length - 1);
    record(q);
}
for (let n = 0; n < 150; ++n, ++edge) { // heuristicWeight values that `||` keeps but ToNumber mangles
    const q = genQuery();
    q.options.heuristicWeight = pick(['abc', {}, '0', '2.5', true, [], [3], ' 4 ', '-1', 'Infinity']);
    record(q);
}
for (let n = 0; n < 60; ++n, ++edge) { // huge goal ranges; flee with weight 9 overflows u32 h * weight
    const q = genQuery();
    const goals = Array.isArray(q.goals) ? q.goals : [q.goals];
    q.goals = goals.map(g => ({ pos: g.pos || g, range: pick([1e9, 2 ** 31, 2 ** 32 + 5, 5e8, 477218589]) }));
    if (chance(0.7)) {
        q.options.flee = true;
        const w = pick([9, 5, 1.2, undefined]);
        if (w === undefined) delete q.options.heuristicWeight; else q.options.heuristicWeight = w;
    }
    record(q);
}
for (let n = 0; n < 20; ++n, ++edge) { // origin in a room without terrain
    const q = genQuery();
    q.origin = new RoomPosition(ri(0, 49), ri(0, 49), roomName(pick([X0 - 1, X1 + 1, X1 + 3]), ri(Y0, Y1)));
    record(q);
}
flush();

const worldLines = ['# generated by tools/pf-gen-fixture.js from the official native PathFinder (version ' + native.version + ')'];
for (const r of rooms) worldLines.push('room ' + r.room + ' ' + r.terrain);
matrices.forEach((m, i) => worldLines.push('matrix ' + i + ' ' + Buffer.from(m).toString('hex')));
const kind = p => p.none ? 'none' : p.shared ? 'shared' : 'cb';
profiles.forEach((p, i) => worldLines.push('profile ' + i + ' ' + kind(p) + ' ' + Object.entries(p.map).map(([k, v]) => k + '=' + v).join(' ')));
fs.writeFileSync(path.join(OUT, 'world.txt'), worldLines.join('\n') + '\n');
console.log(CASES + ' + ' + edge + ' edge cases in ' + file + ' files', stats);
