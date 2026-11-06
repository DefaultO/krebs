// The two player VMs of the API oracle (ARCHITECTURE.md §7.3). Both expose
//   tick(world, tickIndex) -> raw result {status: 'done'|'error'|'total-error'|'halted'|'inactive', out?, error?}
// so run.js can normalize and compare them. A halted VM must be recreated by the caller (make.js:280-284).
'use strict';
const path = require('path');
const vm = require('vm');
const { makeIsolate, intoContext, intoHost, heapStatistics, TIMEOUT_TEXT } = require('../lib/ivm-shim');
const { reference } = require('../lib/common');
const { createSandbox } = require('./sandbox');
const { officialBundle, ourBundle, SCREEPS_ROOT } = require('./bundle');
const W = require('./world');

// The official pathfinder addon needs node 22's ABI; without it only path-free cases run (`hasNative`).
let nativeMod, hasNative = false;
function native() {
    if (nativeMod === undefined) {
        try {
            nativeMod = require(path.join(SCREEPS_ROOT, 'node_modules/@screeps/driver/native/build/Release/native.node'));
            hasNative = true;
        } catch (e) {
            nativeMod = {
                version: 11,
                loadTerrain() {},
                search() {
                    throw new Error('native pathfinder unavailable: run the oracle with node 22 (' + e.message + ')');
                },
            };
        }
    }
    return nativeMod;
}

// make.js:31 pathfinderFactory.init(native, rooms): the addon's terrain is process-global, so both sides share it
// The world blob pads the map with all-wall rooms (TERRAIN-WORLD-BLOB); here: 3 rooms around the bounding box.
function loadTerrain(w) {
    if (!(native(), hasNative)) return;
    const xy = w.rooms.map((r) => W.roomNameToXY(r._id));
    const [x0, x1] = [Math.min(...xy.map((p) => p[0])) - 3, Math.max(...xy.map((p) => p[0])) + 3];
    const [y0, y1] = [Math.min(...xy.map((p) => p[1])) - 3, Math.max(...xy.map((p) => p[1])) + 3];
    const rooms = [];
    for (let x = x0; x <= x1; x++) {
        for (let y = y0; y <= y1; y++) {
            const name = (x < 0 ? 'W' + (-x - 1) : 'E' + x) + (y < 0 ? 'N' + (-y - 1) : 'S' + y);
            rooms.push({ room: name, terrain: w.terrain[name] || '1'.repeat(2500) });
        }
    }
    require(path.join(SCREEPS_ROOT, 'node_modules/@screeps/driver/lib/path-finder.js')).init(nativeMod, rooms);
}

const seedOf = (name, t) => {
    let h = 2166136261;
    for (const c of `${name}#${t}`) h = Math.imul(h ^ c.charCodeAt(0), 16777619);
    return h >>> 0;
};

// driver/lib/index.js:35-52 (GAME-SHARD), the playerSandbox script
const shardScript = (name) => `Game.shard = Object.create(null, {
        name: {value: ${JSON.stringify(name)}, writable: true, enumerable: true},
        type: {value: 'normal', writable: true, enumerable: true},
        ptr: {value: false, enumerable: true}
    });`;

let officialScript, ourScripts = 0;

class OfficialVM {
    constructor(w, caseName) {
        this.caseName = caseName;
        const { ctx, setTick } = createSandbox();
        this.ctx = ctx;
        this.setTick = setTick;
        this.halted = false;
        const { ivm, isolate, halt } = makeIsolate(ctx, { onHalt: () => (this.halted = true) });
        loadTerrain(w);
        officialScript = officialScript || new vm.Script(officialBundle(), { filename: '<runtime>' });
        officialScript.runInContext(ctx);
        const G = vm.runInContext('globalThis', ctx);
        // user-vm.js:59-71
        G.global = G;
        G._ivm = ivm;
        G._isolate = isolate;
        G._context = {};
        G._worldSize = w.worldSize;
        G._nativeMod = native();
        G._constants = intoContext(ctx, require(reference('common/lib/constants.js')));
        // driver/lib/index.js:56-73 registrations (function sources already strings)
        G._customObjectPrototypes = intoContext(ctx, w.customObjectPrototypes.map(([objectType, name, opts]) => ({ objectType, name, opts })));
        G._customIntentTypes = intoContext(ctx, w.customIntentTypes);
        G._halt = halt;
        vm.runInContext('_init();', ctx);
        this.start = G._start;
        // user-vm.js:79-82: one buffer, 2500 bytes per room, in rooms-collection order
        const buffer = new G.ArrayBuffer(2500 * w.rooms.length);
        const view = new G.Uint8Array(buffer);
        const offsets = {};
        w.rooms.forEach((r, i) => {
            offsets[r._id] = i * 2500;
            for (let k = 0; k < 2500; k++) view[i * 2500 + k] = +w.terrain[r._id][k];
        });
        G._setStaticTerrainData(buffer, intoContext(ctx, offsets));
        vm.runInContext(`new function () {
            delete global._ivm; delete global._isolate; delete global._context; delete global._init;
            delete global._evalFn; delete global._start; delete global._setStaticTerrainData;
            delete global._worldSize; delete global._nativeMod; delete global._constants; delete global._halt;
            delete global._customObjectPrototypes; delete global._customIntentTypes;
        }`, ctx);
    }

    tick(w, t) {
        const data = W.officialData(w);
        if (!data) return { status: 'inactive' };
        this.setTick(seedOf(this.caseName, t), W.now(w));
        try {
            const run = this.start.apply(undefined, [intoContext(this.ctx, data)]);
            vm.runInContext(shardScript(w.shard), this.ctx);
            const out = run.applySync(undefined, []);
            if (this.halted) return { status: 'halted', error: 'CPU halted' };
            const copy = intoHost(out);
            void copy.memory.data.length;   // make.js:254-263 reads it after the run: a Memory serialized to undefined throws
            return { status: out.type, out: copy };
        } catch (e) {
            if (this.halted) return { status: 'halted', error: 'CPU halted' };
            return { status: 'total-error', error: (e && e.stack) || e };   // make.js:293
        }
    }
}

class OurVM {
    constructor(w, caseName) {
        this.caseName = caseName;
        const { ctx, setTick } = createSandbox();
        this.ctx = ctx;
        this.setTick = setTick;
        this.halted = false;
        this.vm = {};
        this.docs = { gen: 0, rooms: new Map() };   // doc_reuse.rs DocCache (null after docsOff)
        loadTerrain(w);
        const bundle = ourBundle(W.oracleData(w), { customObjectPrototypes: w.customObjectPrototypes, customIntentTypes: w.customIntentTypes });
        this.substituted = bundle.substituted;
        // run.js --coverage: one script name per bundle so coverage maps back to its files (frames still match <runtime>)
        const filename = process.env.ORACLE_COVERAGE ? `<runtime>#${++ourScripts}` : '<runtime>';
        if (process.env.ORACLE_COVERAGE) require('./coverage').register(filename, bundle.text, bundle.modules);
        new vm.Script(bundle.text, { filename }).runInContext(ctx);
        const G = vm.runInContext('globalThis', ctx);
        this.G = G;
        this.tickFn = G._init(this.natives(w));
    }

    // §6.7 natives over the case world (node implementations)
    natives(w) {
        const ctx = this.ctx, G = this.G;
        let cpu = 0;
        let bufferProto;
        const self = this;
        const own = (o, k) => (Object.prototype.hasOwnProperty.call(o, k) ? o[k] : undefined);
        return {
            cpuTime() {
                cpu += 1;
                return cpu / 100;   // Fixed meter: +0.01 ms per read
            },
            compileAndRun(code, filename, timeoutMs, kind) {
                try {
                    return new vm.Script(code, { filename }).runInContext(ctx, timeoutMs ? { timeout: timeoutMs, displayErrors: false } : { displayErrors: false });
                } catch (e) {
                    if (e && e.code === 'ERR_SCRIPT_EXECUTION_TIMEOUT') throw new G.Error(TIMEOUT_TEXT);
                    throw e;
                }
            },
            halt() {
                self.halted = true;
            },
            heapStats() {
                return intoContext(ctx, heapStatistics());
            },
            nativeMod: native(),
            // Lookups are own-property only, like the Rust maps: 'toString' or '__proto__' names no room, resource or
            // order (a node object's prototype must not leak into what the natives answer).
            terrain(roomName) {
                const t = own(w.terrain, roomName);
                return t === undefined ? undefined : intoContext(ctx, Uint8Array.from(t, Number));
            },
            worldSize: () => w.worldSize,
            isAccessible: (roomName) => w.accessibleRooms.includes(roomName),
            roomStatus(roomName) {
                for (const status of ['closed', 'novice', 'respawn']) {
                    if (own(w.roomStatusData[status], roomName) !== undefined) {
                        return intoContext(ctx, { status, timestamp: own(w.roomStatusData[status], roomName) });
                    }
                }
                return undefined;
            },
            // mapgrid.js:68-121 for one accessible room
            gridData(x, y) {
                const name = (x < 0 ? 'W' + (-x - 1) : 'E' + x) + (y < 0 ? 'N' + (-y - 1) : 'S' + y);
                if (!w.accessibleRooms.includes(name)) return undefined;
                const t = w.terrain[name];
                const dirs = { t: [0, 0, 1, 0], r: [49, 0, 0, 1], b: [0, 49, 1, 0], l: [0, 0, 0, 1] };
                const out = {};
                for (const d in dirs) {
                    let [cx, cy, dx, dy] = dirs[d], n = 0;
                    for (let i = 0; i < 50; i++, cx += dx, cy += dy) if (t[cy * 50 + cx] == 0) n++;
                    if (n > 0) out[d] = n;
                }
                return intoContext(ctx, out);
            },
            marketOrders(rt) {
                const o = own(W.oracleData(w).market.orders, rt);
                return o === undefined ? undefined : JSON.stringify(o);
            },
            marketOrder(id) {
                const o = own(W.oracleData(w).market.orders.all, id);
                return o === undefined ? undefined : JSON.stringify(o);
            },
            marketHistory: (rt) => JSON.stringify(own(W.oracleData(w).market.history, rt) || []),
            now: () => W.now(w),
            base64ToBuffer(b64) {
                bufferProto = bufferProto || G.Object.create(G.Uint8Array.prototype);
                return G.Object.setPrototypeOf(intoContext(ctx, Uint8Array.from(Buffer.from(b64, 'base64'))), bufferProto);
            },
            mark() {},
            docsOff() {
                self.docs = null;
            },
            shardName: w.shard,
            profile: 'private',
            setupList: process.env.ORACLE_SETUP_LIST === '1',
        };
    }

    // runtime doc_reuse.rs DocCache::plan, over the blobs' parsed docs (the oracle world's seq is the object index)
    plan(a) {
        const d = this.docs;
        if (!d) return undefined;
        const fresh = ['creep', 'powerCreep', 'spawn', 'invaderCore', 'tombstone'];
        let out = '[' + d.gen++;
        const next = new Map(), rooms = JSON.parse(a.meta).rooms;
        a.blobs.forEach((b, i) => {
            if (b == null || next.has(rooms[i][0])) return;
            const p = JSON.parse(b);
            const docs = p.objects.map((o, j) => ({ seq: p.seq[j], json: W.ascii(JSON.stringify(o)), kind: typeof o.type === 'string' ? o.type : '', effects: Object.prototype.hasOwnProperty.call(o, 'effects') }));
            next.set(rooms[i][0], docs);
            const prev = d.rooms.get(rooms[i][0]);
            if (!prev) return;
            const ops = [], changed = [];
            let k = 0, run = null;
            const flush = () => run && ops.push(run[0], run[1]);
            for (const doc of docs) {
                while (k < prev.length && prev[k].seq < doc.seq) k++;
                if (k < prev.length && prev[k].seq === doc.seq && prev[k].json === doc.json && !fresh.includes(doc.kind) && !doc.effects) {
                    if (run && run[0] + run[1] === k) run[1]++;
                    else {
                        flush();
                        run = [k, 1];
                    }
                } else {
                    flush();
                    run = null;
                    ops.push(-1 - doc.seq);
                    changed.push(doc.json);
                }
            }
            flush();
            out += `,[${i},${W.ascii(JSON.stringify(p.room))},[${ops.join(',')}]${changed.map((c) => ',' + c).join('')}]`;
        });
        d.rooms = next;
        return out + ']';
    }

    tick(w, t) {
        const made = W.chitinInput(w, this.vm);
        if (!made) return { status: 'inactive' };
        const a = made.input;
        const plans = this.plan(a);
        this.setTick(seedOf(this.caseName, t), W.now(w));
        let out, copy;
        try {
            out = this.tickFn(a.meta, intoContext(this.ctx, a.blobs), intoContext(this.ctx, a.eventLogs), a.memory,
                intoContext(this.ctx, a.segments), intoContext(this.ctx, a.foreignSegment),
                a.modules === undefined ? undefined : intoContext(this.ctx, a.modules), plans);
        } catch (e) {
            this.vm = made.vm;   // a throw out of __tick leaves the VM on its input Memory
            if (this.halted) return { status: 'halted', error: 'CPU halted', input: a };
            return { status: 'total-error', error: (e && e.stack) || e, input: a };
        }
        // the VM now holds this run's Memory; until the host commits it, it must pass Memory explicitly
        this.vm = { codeVersion: made.vm.codeVersion, memory: null };
        if (this.halted) return { status: 'halted', error: 'CPU halted', input: a };
        try {
            copy = intoHost(out);   // §4.2 step 2: ValueSerializer of outMessage (DataCloneError = total error)
        } catch (e) {
            return { status: 'total-error', error: (e && e.stack) || e, input: a };
        }
        if (copy.memory === undefined) copy.memory = made.vm.memory;   // unchanged
        this.vm.memory = copy.memory;
        return { status: out.type, out: copy, input: a };
    }
}

module.exports = { OfficialVM, OurVM, nativeAvailable: () => (native(), hasNative) };
