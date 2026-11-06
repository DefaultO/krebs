// WP06 side-by-side API harness: runs the vanilla engine game files and chitin's ports of the WP06 files
// (crates/runtime/js/...) in two node `vm` contexts over the same fake runtimeData, and snapshots what probe code
// observes. Everything that is not a WP06 file is the vanilla source on both sides (game.js, utils.js, console.js,
// creeps.js, ...), so a difference can only come from a WP06 port. Node 22 is required (official native.node).
//
// A case (cases/*.js) = { name, tags, world, ticks: [{ probes: [[label, fn], ...] }] }. Probe functions are
// stringified and evaluated inside each context, so they may only use sandbox globals (Game, RoomPosition, ...).
// ponytail: this is the WP06-local stand-in for WP05's tools/oracle/api harness; cases and fixtures carry the full
// input (world + probe source), so that harness can adopt them unchanged.
'use strict';
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const { reference } = require('../lib/common');

const ROOT = path.join(__dirname, '..', '..', '..');
const OURS = path.join(ROOT, 'crates', 'runtime', 'js');
const SCREEPS = process.env.SCREEPS_ROOT || 'E:/screeps/server/node_modules';
const REF = reference('');

// Vanilla file -> chitin port (chitin side only).
const OWNED = {
    'engine/src/game/rooms.js': 'engine/game/rooms.js',
    'engine/src/game/path-finder.js': 'engine/game/path-finder.js',
    'engine/src/game/path-utils.js': 'engine/game/path-utils.js',
    'engine/src/game/map.js': 'engine/game/map.js',
    'engine/src/game/flags.js': 'engine/game/flags.js',
    'engine/src/game/construction-sites.js': 'engine/game/construction-sites.js',
    'engine/src/game/sources.js': 'engine/game/sources.js',
    'engine/src/game/minerals.js': 'engine/game/minerals.js',
    'driver/lib/path-finder.js': 'driver/path-finder.js',
};
const ownedByAbs = new Map(Object.entries(OWNED).map(([v, o]) => [path.resolve(REF, v), path.join(OURS, o)]));
// A port resolves its requires from its vanilla location (same relative layout as the bundle).
const vanillaOf = new Map([...ownedByAbs].map(([v, o]) => [o, v]));

const native = require(path.join(SCREEPS, '@screeps/driver/native/build/Release/native.node'));

// ---------------------------------------------------------------------------------------------------------------
// World terrain: rooms W2N2..E1S1 (engine x,y in -3..2), matched exit spans (plain or swamp), walled outer border.

const X0 = -3, X1 = 2, Y0 = -3, Y1 = 2;
const roomName = (x, y) => (x < 0 ? 'W' + (-x - 1) : 'E' + x) + (y < 0 ? 'N' + (-y - 1) : 'S' + y);
const ROOMS = [];
for (let y = Y0; y <= Y1; y++) for (let x = X0; x <= X1; x++) ROOMS.push(roomName(x, y));

function prng(seed) {
    let a = seed >>> 0;
    return () => {
        a = (a + 0x6d2b79f5) >>> 0;
        let t = a;
        t = Math.imul(t ^ (t >>> 15), t | 1);
        t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
}

function buildTerrain() {
    const rand = prng(0x06060606);
    const ri = (a, b) => a + Math.floor(rand() * (b - a + 1));
    // spans[key] = 50 codes along a shared edge: 1 wall, 0 plain, 2 swamp
    const span = () => {
        const e = new Array(50).fill(1);
        if (rand() < 0.15) return e;                 // closed edge
        const swampy = rand() < 0.3;                 // a whole edge of swamp exits (describeExits ignores it)
        for (let n = ri(1, 3); n > 0; --n) {
            const len = ri(1, 8), start = ri(2, 47 - len);
            for (let i = start; i < start + len; ++i) e[i] = swampy || rand() < 0.2 ? 2 : 0;
        }
        return e;
    };
    const vEdge = {}, hEdge = {};
    for (let x = X0; x <= X1; ++x) for (let y = Y0; y <= Y1; ++y) {
        if (x < X1) vEdge[x + ',' + y] = span();
        if (y < Y1) hEdge[x + ',' + y] = span();
    }
    const wall = new Array(50).fill(1);
    const out = {};
    for (let y = Y0; y <= Y1; ++y) for (let x = X0; x <= X1; ++x) {
        const t = new Uint8Array(2500);
        let w = new Uint8Array(2500);
        for (let i = 0; i < 2500; ++i) w[i] = rand() < 0.4 ? 1 : 0;
        for (let it = 0; it < 4; ++it) {
            const n2 = new Uint8Array(2500);
            for (let yy = 0; yy < 50; ++yy) for (let xx = 0; xx < 50; ++xx) {
                let n = 0;
                for (let dy = -1; dy <= 1; ++dy) for (let dx = -1; dx <= 1; ++dx) {
                    const a = xx + dx, b = yy + dy;
                    n += a < 0 || b < 0 || a > 49 || b > 49 ? 1 : w[b * 50 + a];
                }
                n2[yy * 50 + xx] = n >= 5 ? 1 : 0;
            }
            w = n2;
        }
        for (let i = 0; i < 2500; ++i) t[i] = w[i] ? 1 : rand() < 0.15 ? 2 : 0;
        for (let i = 0; i < 6; ++i) t[ri(1, 48) * 50 + ri(1, 48)] = 3;   // wall+swamp code
        const top = y > Y0 ? hEdge[x + ',' + (y - 1)] : wall, bottom = y < Y1 ? hEdge[x + ',' + y] : wall;
        const left = x > X0 ? vEdge[(x - 1) + ',' + y] : wall, right = x < X1 ? vEdge[x + ',' + y] : wall;
        for (let i = 0; i < 50; ++i) {
            t[i] = top[i]; t[49 * 50 + i] = bottom[i]; t[i * 50] = left[i]; t[i * 50 + 49] = right[i];
        }
        // keep the tile inside each exit tile walkable so exits are reachable
        for (let i = 1; i < 49; ++i) {
            if (t[i] !== 1) t[50 + i] = 0;
            if (t[49 * 50 + i] !== 1) t[48 * 50 + i] = 0;
            if (t[i * 50] !== 1) t[i * 50 + 1] = 0;
            if (t[i * 50 + 49] !== 1) t[i * 50 + 48] = 0;
        }
        // a clear 11x11 plaza in the middle of every room for hand-placed objects
        for (let yy = 20; yy <= 30; ++yy) for (let xx = 20; xx <= 30; ++xx) t[yy * 50 + xx] = 0;
        out[roomName(x, y)] = t;
    }
    return out;
}
const TERRAIN = buildTerrain();


function worldSize(names) {   // common/index.js calcWorldSize
    let minX = Infinity, minY = Infinity, maxX = 0, maxY = 0;
    for (const n of names) {
        const m = /^([WE])(\d+)([NS])(\d+)$/.exec(n);
        const x = m[1] === 'W' ? -m[2] - 1 : +m[2], y = m[3] === 'N' ? -m[4] - 1 : +m[4];
        if (x < minX) minX = x; if (y < minY) minY = y; if (x > maxX) maxX = x; if (y > maxY) maxY = y;
    }
    return Math.max(maxX - minX + 1, maxY - minY + 1);
}

// The natives chitin's runtime driver exposes (ARCHITECTURE.md §6.7), computed here independently of mapgrid.js.
function makeNatives(world) {
    const accessible = new Set(world.accessibleRooms);
    const xy = n => { const m = /^([WE])(\d+)([NS])(\d+)$/.exec(n); return [m[1] === 'W' ? -m[2] - 1 : +m[2], m[3] === 'N' ? -m[4] - 1 : +m[4]]; };
    const grid = new Map();
    for (const n of accessible) {
        const t = TERRAIN[n], e = {};
        const count = f => { let c = 0; for (let i = 0; i < 50; i++) if (t[f(i)] === 0) c++; return c; };
        const sides = { t: i => i, r: i => i * 50 + 49, b: i => 2450 + i, l: i => i * 50 };
        for (const k of ['t', 'r', 'b', 'l']) { const c = count(sides[k]); if (c > 0) e[k] = c; }
        grid.set(xy(n).join(','), e);
    }
    const st = world.roomStatusData;
    return {
        gridData: (x, y) => grid.get(x + ',' + y),
        isAccessible: n => accessible.has(n),
        roomStatus: n => {
            for (const s of ['closed', 'novice', 'respawn']) {
                if (Object.prototype.hasOwnProperty.call(st[s], n)) return { status: s, timestamp: st[s][n] };
            }
            return undefined;
        },
    };
}

// ---------------------------------------------------------------------------------------------------------------
// A context with a tiny CommonJS loader. `side` = 'vanilla' | 'chitin'.

function makeContext(side, world) {
    const ctx = vm.createContext({});
    vm.runInContext('this.global = this;', ctx);
    const cache = new Map();
    const natives = makeNatives(world);
    let driver;

    function resolve(from, req) {
        if (req === '~runtime-driver' || /(^|\/)runtime-driver$/.test(req)) return '~runtime-driver';
        if (req === 'lodash') return path.join(OURS, 'vendor/lodash.js');
        if (req === 'util') return 'node:util';
        if (req === '@screeps/common/lib/constants') return path.join(REF, 'common/lib/constants.js');
        let p;
        if (req.startsWith('.')) {
            p = path.resolve(path.dirname(vanillaOf.get(from) || from), req);
            for (const c of [p, p + '.js', path.join(p, 'index.js')]) {
                if (fs.existsSync(c) && fs.statSync(c).isFile()) { p = c; break; }
            }
        } else {
            p = require.resolve(req, { paths: [path.dirname(from), SCREEPS] });
        }
        if (side === 'chitin' && ownedByAbs.has(p)) return ownedByAbs.get(p);
        return p;
    }

    function load(file) {
        if (cache.has(file)) return cache.get(file).exports;
        if (file === '~runtime-driver') return driver;
        if (file === 'node:util') return require('util');
        const module = vm.runInContext('({exports: {}})', ctx);
        cache.set(file, module);
        const src = fs.readFileSync(file, 'utf8');
        const fn = vm.runInContext(`(function (exports, require, module, __filename, __dirname) {${src}\n})`, ctx, { filename: file });
        fn.call(module.exports, module.exports, req => load(resolve(file, req)), module, file, path.dirname(file));
        return module.exports;
    }

    driver = vm.runInContext('({})', ctx);
    driver.constants = load(path.join(REF, 'common/lib/constants.js'));
    const pathFinderModule = load(resolve(path.join(REF, 'driver/lib/x.js'), './path-finder'));
    driver.pathFinder = pathFinderModule.create(native);
    // driver/lib/path-finder.js init() runs host-side, out of reach of game code: cases opt in to probe it directly
    if (world.exposeDriverPathFinder) ctx.__driverPathFinder = pathFinderModule;
    driver.getWorldSize = () => world.worldSize;
    driver.bufferFromBase64 = b => Buffer.from(b, 'base64');
    if (side === 'chitin') Object.assign(driver, natives);

    // chitin J1: one terrain cache per VM, like vanilla's one staticTerrainData per VM (runtime.js:18)
    const terrainCache = side === 'chitin' ? makeTerrainCache(ctx) : null;
    return { side, ctx, load, terrainCache, game: p => load(path.join(REF, 'engine/src/game', p)) };
}

// chitin J1 (ARCHITECTURE.md §7.2), the shape the host builds once per VM before any user code. Vanilla's
// staticTerrainData is a plain {} holding every room as an own property, so (a) a cached room is an own property here
// (plain-object hit on the hot path), (b) a miss falls through to a Proxy prototype that asks natives.terrain(name)
// FIRST, so a bot's Object.prototype.W1N1 cannot shadow real terrain, and (c) a non-room key then resolves like
// vanilla's {} ('constructor', '__proto__', polluted keys). natives.terrain matches stored room names exactly
// ('w1n1', 'W1N01' are not W1N1).
function makeTerrainCache(ctx) {
    const mkU8 = vm.runInContext('(a => new Uint8Array(a))', ctx);
    const terrain = n => Object.prototype.hasOwnProperty.call(TERRAIN, n) ? mkU8(Array.from(TERRAIN[n])) : undefined;
    return vm.runInContext(`(terrain => {
        const defineProperty = Object.defineProperty, reflectGet = Reflect.get;
        return Object.create(new Proxy(Object.prototype, {
            get(t, k, receiver) {
                const a = typeof k == 'string' ? terrain(k) : undefined;
                if (!a) return reflectGet(t, k, receiver);
                defineProperty(receiver, k, {value: a, writable: true, enumerable: true, configurable: true});
                return a;
            },
        }));
    })`, ctx)(terrain);
}

// ---------------------------------------------------------------------------------------------------------------
// runtimeData (driver/lib/runtime/data.js shapes) from a case world.

function normalizeWorld(w) {
    const world = JSON.parse(JSON.stringify(w));
    world.accessibleRooms = world.accessibleRooms || ROOMS.filter(n => !(world.roomStatusData && world.roomStatusData.closed && n in world.roomStatusData.closed));
    world.roomStatusData = Object.assign({ novice: {}, respawn: {}, closed: {} }, world.roomStatusData);
    world.worldSize = worldSize(ROOMS);
    return world;
}

function makeRuntimeData(c, world) {
    const me = world.user;
    const objects = world.objects || [];
    const userObjects = {}, roomIds = {};
    for (const o of objects) if (o.user === me) {
        userObjects[o._id] = o;
        if (o.type !== 'flag' && o.type !== 'constructionSite') {
            roomIds[o.room] = true;
            if (o.type === 'observer') roomIds[o.observeRoom] = true;
        }
    }
    const rooms = {};
    for (const n of ROOMS) if (roomIds[n]) rooms[n] = Object.assign({ _id: n, status: 'normal' }, (world.roomDocs || {})[n]);
    const roomObjects = {};
    for (const o of objects) if (o.user !== me && roomIds[o.room]) roomObjects[o._id] = o;
    Object.assign(roomObjects, userObjects);
    const users = {};
    for (const [id, u] of Object.entries(world.users)) users[id] = Object.assign({ _id: id, badge: {} }, u);
    const rd = {
        userObjects,
        user: Object.assign({ _id: me, cpu: 100, gcl: 0, power: 0, resources: {} }, world.users[me]),
        userCode: {},
        userCodeTimestamp: 0,
        userMemory: { data: world.memory || '', userId: me },
        consoleCommands: [],
        time: world.time || 1000,
        rooms,
        roomObjects,
        flags: (world.flags || []).map((f, i) => Object.assign({ _id: 'f' + i, user: me }, f)),
        accessibleRooms: JSON.stringify(world.accessibleRooms),
        roomStatusData: world.roomStatusData,
        transactions: { outgoing: [], incoming: [] },
        cpu: 100,
        cpuBucket: 10000,
        roomEventLog: world.eventLog || {},
        userPowerCreeps: {},
        users,
        market: { orders: { all: {} }, history: {}, myOrders: [] },
    };
    if (world.games) rd.games = world.games;
    // fresh context-realm objects, as ExternalCopy delivers them
    const data = vm.runInContext('JSON.parse', c.ctx)(JSON.stringify(rd));
    if (c.side === 'vanilla') {
        const mkU8 = vm.runInContext('(a => new Uint8Array(a))', c.ctx);
        const terrain = vm.runInContext('({})', c.ctx);
        for (const n of ROOMS) terrain[n] = mkU8(Array.from(TERRAIN[n]));
        data.staticTerrainData = terrain;
        const WorldMapGrid = c.load(path.join(REF, 'driver/lib/runtime/mapgrid.js'));
        data.mapGrid = new WorldMapGrid(vm.runInContext('JSON.parse', c.ctx)(data.accessibleRooms), terrain);
    } else {
        // J1 cache; J3: the ports read accessibility, status and the grid only through the driver natives, so the
        // chitin side does not get vanilla's per-player copies at all (vanilla structures.js launchNuke still reads
        // roomStatusData: no WP06 case calls it)
        data.staticTerrainData = c.terrainCache;
        delete data.accessibleRooms;
        delete data.roomStatusData;
    }
    if (world.customObjectPrototypes) {
        // mod hook (runtime.js:34-44): property functions arrive as source and are evaluated in the isolate
        data.customObjectPrototypes = vm.runInContext('JSON.parse', c.ctx)(JSON.stringify(world.customObjectPrototypes));
        for (const cop of data.customObjectPrototypes) {
            for (const k of Object.keys(cop.opts.properties || {})) cop.opts.properties[k] = vm.runInContext(`(${cop.opts.properties[k]})`, c.ctx);
        }
    }
    return data;
}

// driver/lib/runtime/runtime.js:59-100 intents object, built inside the context.
const INTENTS_SRC = `(function() {
    let intentCpu = 0.2, freeMethods = Object.create(null, {say: {value: true}, pull: {value: true}});
    return {
        list: Object.create(null), cpu: 0,
        set(id, name, data) { this.list[id] = this.list[id] || Object.create(null);
            if (!freeMethods[name] && !this.list[id][name]) this.cpu += intentCpu; this.list[id][name] = data; },
        push(name, data, maxLen) { this.list[name] = this.list[name] || [];
            if (maxLen && this.list[name].length >= maxLen) return false; this.list[name].push(data); this.cpu += intentCpu; return true; },
        pushByName(id, name, data, maxLen) { this.list[id] = this.list[id] || Object.create(null);
            this.list[id][name] = this.list[id][name] || [];
            if (maxLen && this.list[id][name].length >= maxLen) return false; this.list[id][name].push(data); this.cpu += intentCpu; return true; },
        remove(id, name) { if (this.list[id] && this.list[id][name]) { delete this.list[id][name]; this.cpu -= intentCpu; return true; } return false; }
    };
})()`;

// ---------------------------------------------------------------------------------------------------------------
// Snapshot: JSON-safe, identity-aware view of what a probe returned.

function snapshot(v, G) {
    const seen = new Map();
    const rle = a => {
        const out = [];
        for (let i = 0; i < a.length;) { let j = i; while (j < a.length && Object.is(a[j], a[i])) j++; out.push(j - i > 1 ? `${a[i]}x${j - i}` : `${a[i]}`); i = j; }
        return out.join(',');
    };
    const className = v => {
        for (const k of Object.keys(G)) if (/^[A-Z]/.test(k) && k !== 'Energy' && G[k] === v.constructor) return k;
        return '?';
    };
    const snap = v => {
        if (v === undefined) return { $: 'undefined' };
        if (typeof v === 'number') {
            if (Number.isNaN(v)) return { $: 'NaN' };
            if (!Number.isFinite(v)) return { $: String(v) };
            if (Object.is(v, -0)) return { $: '-0' };
            return v;
        }
        if (v === null || typeof v === 'boolean' || typeof v === 'string') return v;
        if (typeof v === 'function') return { $fn: v.name };
        if (typeof v !== 'object') return { $: String(v) };
        if (seen.has(v)) return { $ref: seen.get(v) };
        if (G.RoomPosition && v instanceof G.RoomPosition) {
            const r = { $pos: [v.x, v.y, v.roomName], packed: v.__packedPos };
            const own = Object.keys(v);
            if (own.length) { seen.set(v, seen.size); r.own = {}; for (const k of own) r.own[k] = snap(v[k]); }
            return r;
        }
        if (G.Room && v instanceof G.Room) return { $room: v.name, same: G.Game.rooms[v.name] === v };
        if (G.RoomObject && v instanceof G.RoomObject) {
            const id = Object.prototype.hasOwnProperty.call(v, 'id') ? v.id : undefined;
            return { $obj: className(v), id: id === undefined ? null : id,
                name: id === undefined && typeof v.name === 'string' ? v.name : undefined,
                same: id !== undefined ? G.Game.getObjectById(id) === v : G.Game.flags[v.name] === v };
        }
        if (ArrayBuffer.isView(v)) return { $ta: Object.prototype.toString.call(v).slice(8, -1), len: v.length, v: rle(v) };
        seen.set(v, seen.size);
        if (Array.isArray(v)) {
            const r = { $arr: v.map(snap) };
            const extra = Object.keys(v).filter(k => !/^\d+$/.test(k));
            if (extra.length) { r.extra = {}; for (const k of extra) r.extra[k] = snap(v[k]); }
            return r;
        }
        const r = {};
        for (const k of Object.keys(v)) r[k] = snap(v[k]);
        return r;
    };
    return snap(v);
}

function errorView(e) {
    if (e && typeof e === 'object' && 'message' in e) return { $throw: e.constructor && e.constructor.name, message: String(e.message) };
    return { $throw: typeof e, message: String(e) };
}

// ---------------------------------------------------------------------------------------------------------------
// Run one case on one side; returns the per-tick observations.

let terrainLoaded = false;

function runCase(side, kase) {
    const world = normalizeWorld(kase.world);
    const c = makeContext(side, world);
    if (!terrainLoaded) {
        // Upload once: every case shares the same static terrain (it is world-static in the game too).
        c.load(path.join(REF, 'driver/lib/path-finder.js')).init(native,
            ROOMS.map(room => ({ room, terrain: Array.from(TERRAIN[room]).join('') })));
        terrainLoaded = true;
    }
    const game = c.game('game.js');
    const consoleMod = c.game('console.js');
    const out = [];
    let cpu = 0;
    for (let t = 0; t < kase.ticks.length; t++) {
        const tick = kase.ticks[t];
        const tw = tick.world ? normalizeWorld(Object.assign({}, kase.world, tick.world)) : world;
        const data = makeRuntimeData(c, tw);
        const intents = vm.runInContext(INTENTS_SRC, c.ctx);
        let memoryString = data.userMemory.data;
        const rawMemory = vm.runInContext('({})', c.ctx);
        rawMemory.get = () => memoryString;
        rawMemory.set = v => { delete rawMemory._parsed; memoryString = v; };
        const fakeConsole = consoleMod.makeConsole(data.user._id, fn => fn);
        game.init(c.ctx, {}, data, intents, rawMemory, fakeConsole, [], Infinity, () => (cpu += 0.01), () => {}, undefined,
            () => ({}), undefined);
        const probes = [];
        for (const [label, fn] of tick.probes) {
            const f = vm.runInContext(`(${fn.toString()})`, c.ctx, { filename: `probe:${kase.name}:${label}` });
            let r;
            try { r = snapshot(f(), c.ctx); } catch (e) { r = errorView(e); }
            probes.push([label, r]);
        }
        out.push({
            probes,
            intents: snapshot(intents.list, c.ctx),
            intentsCpu: Math.round(intents.cpu * 1000) / 1000,
            console: snapshot(consoleMod.getMessages(data.user._id), c.ctx),
            visual: snapshot(consoleMod.getVisual(data.user._id), c.ctx),
            memory: rawMemory._parsed ? JSON.stringify(rawMemory._parsed) : memoryString,
        });
    }
    return out;
}

// Reusable by tools/oracle/api (WP05): world -> runtimeData, the natives fakes, the snapshot codec.
module.exports = { runCase, makeContext, makeRuntimeData, normalizeWorld, makeNatives, makeTerrainCache, snapshot, ROOMS, TERRAIN, OWNED, ROOT };
