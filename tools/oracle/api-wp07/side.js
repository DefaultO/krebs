'use strict';
// WP07 API differential harness: one "side" = one player VM running the official runtime
// (reference/driver/lib/runtime/runtime.js + runtime-driver.js, reference/engine/src/game/*) in a node
// vm context behind a minimal isolated-vm shim. The chitin side is identical except that the files WP07
// owns (game/{creeps,structures,power-creeps,market,tombstones,ruins,deposits,nukes}.js) are read from
// crates/runtime/js/engine/game/. Everything else (rooms.js, game.js, utils.js, ...) is vanilla on both
// sides, so a diff can only come from a WP07 file. The chitin VM gets runtimeData as chitin delivers it: no
// market.orders/history and no roomStatusData (those reach chitin's files through the runtime-driver natives below).
// ponytail: stand-in until WP05's tools/oracle/api harness lands; cases/fixtures are its input format.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const { reference } = require('../lib/common');

const ROOT = path.join(__dirname, '..', '..', '..');
const OWNED = ['creeps', 'structures', 'power-creeps', 'market', 'tombstones', 'ruins', 'deposits', 'nukes'];
const DRIVER_PKG = process.env.SCREEPS_DRIVER || 'E:/screeps/server/node_modules/@screeps/driver';

let nativeMod = null;
function native() {
    if (!nativeMod) nativeMod = require(DRIVER_PKG + '/native/build/Release/native.node');
    return nativeMod;
}
// The official addon keeps one process-wide terrain table (loadTerrain).
function loadTerrain(rooms) {
    require(DRIVER_PKG + '/lib/path-finder.js').init(native(), rooms);
}

const ALIAS = {
    'lodash': () => reference('lodash-3.10.1/lodash.js'),
    '~runtime-driver': () => reference('driver/lib/runtime/runtime-driver.js'),
    '@screeps/engine/src/utils': () => reference('engine/src/utils.js'),
    '@screeps/engine/src/game/game': () => reference('engine/src/game/game.js'),
    '@screeps/engine/src/game/console': () => reference('engine/src/game/console.js'),
    '@screeps/common/lib/system': () => reference('common/lib/system.js'),
    '@screeps/pathfinding': () => reference('pathfinding/index.js'),
};

// Canonical value printer (keeps undefined/NaN/-0/key order/prototype tags), evaluated in both realms.
const SER = `function ser(v, depth, seen) {
        depth = depth || 0; seen = seen || [];
        if (v === undefined) return 'undefined';
        if (v === null) return 'null';
        var t = typeof v;
        if (t === 'number') return Object.is(v, -0) ? '-0' : String(v);
        if (t === 'string') return JSON.stringify(v);
        if (t === 'boolean' || t === 'bigint') return String(v);
        if (t === 'symbol') return 'symbol';
        if (t === 'function') return 'fn:' + v.name + '/' + v.length;
        if (seen.indexOf(v) !== -1) return '<cycle>';
        if (depth > 5) return '<deep>';
        seen = seen.concat([v]);
        if (Array.isArray(v)) {
            var items = [];
            for (var i = 0; i < v.length; i++) items.push(i in v ? ser(v[i], depth + 1, seen) : 'hole');
            return '[' + items.join(',') + ']';
        }
        var proto = Object.getPrototypeOf(v), tag = '';
        if (proto === null) tag = 'null:';
        else if (proto !== Object.prototype) {
            try { tag = '<' + String(v) + '>'; } catch (e) { tag = '<!' + (e && e.message) + '>'; }
        }
        var keys = Object.keys(v), parts = [];
        for (var k = 0; k < keys.length; k++) {
            var val;
            try { val = ser(v[keys[k]], depth + 1, seen); } catch (e) { val = '!' + (e && e.message); }
            parts.push(JSON.stringify(keys[k]) + ':' + val);
        }
        return tag + '{' + parts.join(',') + '}';
}`;
const hostSer = vm.runInThisContext('(' + SER + ')');

// In-context journal: probes from the case code and every intents call, in call order.
const CONTEXT_HELPERS = `
(function(g) {
    var journal = [];
    var ser = (${SER});
    g.__ser = ser;
    g.__journal = journal;
    g.__probe = function(label, value, threw) {
        journal.push([threw ? 'throw' : 'probe', String(label), threw ? String(value) : ser(value)]);
    };
    g.__wrapIntents = function(intents) {
        ['set', 'push', 'pushByName', 'remove'].forEach(function(op) {
            var orig = intents[op];
            intents[op] = function() {
                var r = orig.apply(this, arguments);
                journal.push([op, ser(Array.prototype.slice.call(arguments)), ser(r)]);
                return r;
            };
        });
    };
})(globalThis);
`;

/**
 * One player VM. `variant` is 'vanilla' or 'chitin'. `opts`: {worldSize, rooms: {name: terrain2500},
 * accessibleRooms: [names], seed}.
 */
function createSide(variant, opts) {
    const ctx = vm.createContext({});
    const run = (code, filename) => vm.runInContext(code, ctx, { filename: filename || 'harness' });
    run('globalThis.global = globalThis;');
    run(CONTEXT_HELPERS, 'helpers');
    const inContext = run('(function(s) { return JSON.parse(s); })');

    // Owned files still at their WP00 stub (no `exports.make`) stay vanilla so ported files test early.
    const ownedSource = {};
    for (const f of OWNED) {
        const ours = path.join(ROOT, 'crates', 'runtime', 'js', 'engine', 'game', f + '.js');
        if (/exports\.make\b/.test(fs.readFileSync(ours, 'utf8'))) {
            ownedSource[path.normalize(reference(`engine/src/game/${f}.js`))] = ours;
        }
    }

    const cache = new Map();
    function resolve(request, dir) {
        if (ALIAS[request]) return path.normalize(ALIAS[request]());
        if (request === 'util' || request === 'isolated-vm') return request;
        if (!request.startsWith('.')) return path.normalize(require.resolve(request, { paths: [DRIVER_PKG + '/../..'] }));
        let p = path.resolve(dir, request);
        if (fs.existsSync(p) && fs.statSync(p).isDirectory()) p = path.join(p, 'index.js');
        else if (!p.endsWith('.js')) p += '.js';
        return path.normalize(p);
    }
    function load(id) {
        if (cache.has(id)) return cache.get(id).exports;
        if (id === 'util') return require('util');
        const module = run('({exports: {}})');
        cache.set(id, module);
        // WP07_SELF=1: harness self-test, both sides vanilla.
        const src = variant === 'chitin' && !process.env.WP07_SELF && ownedSource[id] ? ownedSource[id] : id;
        const code = fs.readFileSync(src, 'utf8');
        const fn = run('(function (exports, require, module, __filename, __dirname) {' + code + '\n})', src);
        const dir = path.dirname(id);
        fn.call(module.exports, module.exports, (r) => load(resolve(r, dir)), module, id, dir);
        if (id === path.normalize(reference('engine/src/game/game.js'))) {
            // Journal every intent call (harness-only hook; vanilla game.init is called unchanged).
            const init = module.exports.init;
            module.exports.init = function(g, code, data, intents) {
                ctx.__wrapIntents(intents);
                return init.apply(this, arguments);
            };
        }
        return module.exports;
    }

    // isolated-vm shim (the parts runtime.js / runtime-driver.js / game.js touch).
    let cpuNs = 0n;
    const ivm = {
        ExternalCopy: class { constructor(v) { this.v = structuredClone(v); } copyInto() { return this.v; } },
        Reference: class { constructor(v) { this.v = v; } deref() { return this.v; } },
    };
    const isolate = {
        get cpuTime() { cpuNs += 10000n; return cpuNs; },
        compileScriptSync(code, options) {
            const script = new vm.Script(code, { filename: options && options.filename });
            return { runSync: () => script.runInContext(ctx) };
        },
        getHeapStatisticsSync() { return {}; },
    };
    const C = require(reference('common/lib/constants.js'));
    Object.assign(ctx, {
        _ivm: ivm, _isolate: isolate, _context: ctx, _halt: undefined,
        _worldSize: opts.worldSize || 12,
        _nativeMod: native(),
        _constants: inContext(JSON.stringify(C)),
        _customObjectPrototypes: inContext('[]'),
        _customIntentTypes: inContext('{}'),
    });

    load(path.normalize(reference('driver/lib/runtime/runtime.js')));
    // chitin's market.js and StructureNuker#launchNuke read world data through runtime-driver natives (ARCHITECTURE
    // 6.7, J3); here they serve the tick's vanilla runtimeData.market / roomStatusData, which the chitin VM itself
    // never receives (see tick). Natives take strings only: the JS side must do the property-key conversion.
    let market = null, statusData = null;
    const own = (o, k) => o != null && Object.prototype.hasOwnProperty.call(o, k);
    const str = (fn) => (k) => {
        if (typeof k !== 'string') throw new TypeError('native called with a ' + typeof k);
        return fn(k);
    };
    const driverModule = load(path.normalize(reference('driver/lib/runtime/runtime-driver.js')));
    driverModule.marketOrders = str((rt) => own(market.orders, rt) ? JSON.stringify(market.orders[rt]) : undefined);
    driverModule.marketOrder = str((id) => own(market.orders.all, id) ? JSON.stringify(market.orders.all[id]) : undefined);
    driverModule.marketHistory = str((rt) => own(market.history, rt) ? JSON.stringify(market.history[rt]) : undefined);
    // {status, timestamp} of a non-normal room (map.js getRoomStatus order: closed, novice, respawn).
    driverModule.roomStatus = str((name) => {
        for (const s of ['closed', 'novice', 'respawn']) {
            if (own(statusData[s], name)) return { status: s, timestamp: statusData[s][name] };
        }
        return undefined;
    });
    run('_init();');
    const names = Object.keys(opts.rooms || {});
    const buffer = run(`new ArrayBuffer(${names.length * 2500})`);
    const view = new Uint8Array(buffer);
    const offsets = {};
    names.forEach((name, i) => {
        offsets[name] = i * 2500;
        const t = opts.rooms[name];
        for (let j = 0; j < 2500; j++) view[i * 2500 + j] = t.charCodeAt(j) - 48;
    });
    ctx._setStaticTerrainData(buffer, inContext(JSON.stringify(offsets)));
    const start = ctx._start;
    for (const g of ['_ivm', '_isolate', '_context', '_init', '_evalFn', '_start', '_setStaticTerrainData',
        '_worldSize', '_nativeMod', '_constants', '_halt', '_customObjectPrototypes', '_customIntentTypes']) {
        delete ctx[g];
    }

    // Deterministic sandbox: seeded Math.random, fixed Date.now (per tick).
    run(`(function() {
        var s = 0;
        globalThis.__seed = function(n) { s = n >>> 0; };
        Math.random = function() {
            s = (s + 0x6d2b79f5) >>> 0;
            var t = s;
            t = Math.imul(t ^ (t >>> 15), t | 1);
            t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
            return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
        };
        var RealDate = Date, now = 0;
        globalThis.__setNow = function(n) { now = n; };
        Date.now = function() { return now; };
    })();`);

    /** Runs one tick. `data` is a JSON-able runtimeData (cpu/cpuBucket may be null for Infinity). */
    function tick(data, { seed = 1, now = 1600000000000 } = {}) {
        ctx.__journal.length = 0;
        ctx.__seed(seed);
        ctx.__setNow(now);
        market = data.market;
        statusData = data.roomStatusData;
        let vmData = data;
        if (variant === 'chitin' && !process.env.WP07_SELF) {
            // What chitin delivers (§6.6 meta): no world order book, price history or room status data.
            vmData = Object.assign({}, data, { market: { myOrders: data.market.myOrders } });
            delete vmData.roomStatusData;
        }
        const d = inContext(JSON.stringify(vmData));
        if (data.cpu === null) d.cpu = Infinity;
        if (data.cpuBucket === null) d.cpuBucket = Infinity;
        const fn = start(d).deref();
        const out = fn();
        const journal = ctx.__journal.slice();
        return {
            journal,
            error: out.error === undefined ? null : String(out.error).split('\n')[0],
            intents: hostSer(out.intentsList),
            memory: out.memory.data,
            console: out.console.log.map(m => m.message),
            results: out.console.results,
            visual: out.visual || null,
        };
    }
    return { tick };
}

module.exports = { createSide, loadTerrain, OWNED };
