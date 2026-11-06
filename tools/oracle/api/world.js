// A case world (an in-memory stand-in for the DB/env of one player's view) and the three host halves the API
// oracle needs around a run:
//   officialData(w)    driver/lib/runtime/data.js `get`, the DB queries replaced by scans with Loki's result order
//                      (spec 11 ORDER-RUNTIME-OBJECTS/-ROOMIDS, ORDER-GAME-ROOMS, ORDER-MISC) and every query
//                      result JSON round-tripped like the storage RPC (STORE-RPC-SERIALIZATION).
//   chitinInput(w, vm) engine/src/runner/data.rs per ARCHITECTURE.md §6.6: per-user meta JSON + room blobs.
//   applyOutput(w, o)  driver/lib/runtime/make.js after a run: Memory, segments and segment settings persisted.
//
// World fields (see `world()` for defaults): me, time, users[], rooms[] (rooms collection, creation order),
// objects[] (rooms.objects, creation order = seq), flags[], powerCreeps[], code {modules, timestamp},
// memory, segments {id: string} (mine), foreignSegments {userId: {id: string}}, publicSegments {userId: "1,2"}, consoleCommands[], eventLogs {room: string},
// transactions[], market {orders[], stats[]}, accessibleRooms[], roomStatusData, terrain {room: 2500 chars},
// worldSize, shard, clock {baseMs, baseTick, nominalTickMs}.
'use strict';
const vclock = require('../lib/vclock');

const rpc = (v) => (v === undefined ? undefined : JSON.parse(JSON.stringify(v)));

function roomNameToXY(name) {
    // common/index.js roomNameToXY
    let [, hor, x, ver, y] = name.match(/^(\w)(\d+)(\w)(\d+)$/);
    x = hor === 'W' ? -x - 1 : +x;
    y = ver === 'N' ? -y - 1 : +y;
    return [x, y];
}

// common/index.js:183 (BUG-WORLDSIZE: maxX/maxY start at 0)
function calcWorldSize(rooms) {
    let minX = Infinity, minY = Infinity, maxX = 0, maxY = 0;
    for (const r of rooms) {
        const [x, y] = roomNameToXY(r._id);
        minX = Math.min(minX, x); minY = Math.min(minY, y);
        maxX = Math.max(maxX, x); maxY = Math.max(maxY, y);
    }
    return Math.max(maxX - minX + 1, maxY - minY + 1);
}

function world(w) {
    const out = Object.assign({
        time: 1,
        users: [],
        rooms: [],
        objects: [],
        flags: [],
        powerCreeps: [],
        code: { modules: { main: '' }, timestamp: 1 },
        memory: '',
        segments: {},
        foreignSegments: {},
        publicSegments: {},
        consoleCommands: [],
        eventLogs: {},
        transactions: [],
        market: { orders: [], stats: [] },
        roomStatusData: { novice: {}, respawn: {}, closed: {} },
        terrain: {},
        shard: 'shard0',
        customObjectPrototypes: [],   // [[objectType, name, opts]] as a mod registers them (function sources as strings)
        customIntentTypes: {},
        clock: { baseMs: 1600000000000, baseTick: 0, nominalTickMs: 1000 },
    }, w);
    if (!out.accessibleRooms) out.accessibleRooms = out.rooms.filter((r) => r.status !== 'out of borders').map((r) => r._id);
    if (out.worldSize === undefined) out.worldSize = calcWorldSize(out.rooms);
    for (const r of out.rooms) if (!out.terrain[r._id]) out.terrain[r._id] = '0'.repeat(2500);
    return out;
}

const user = (w) => w.users.find((u) => u._id === w.me);

// data.js:151-165 (CPU-TICKLIMIT)
function cpuOf(u) {
    if (!u.cpu) return [Infinity, Infinity];
    let bucket = u.cpuAvailable || 0;
    if (bucket < 0) bucket = 0;
    return [Math.min(bucket + u.cpu, 500), bucket];
}

// data.js:103-127: own objects (creation order), roomIds (first appearance), userIdsHash
function visibility(w) {
    const own = w.objects.filter((o) => o.user === w.me);
    const roomIdsHash = {};
    const userIdsHash = { [w.me]: true };
    for (const i of own) {
        if (i.type == 'flag' || i.type == 'constructionSite') continue;
        roomIdsHash[i.room] = true;
        if (i.type == 'observer') roomIdsHash[i.observeRoom] = true;
        if (i.type == 'controller' && i.sign) userIdsHash[i.sign.user] = true;
    }
    return { own, roomIds: Object.keys(roomIdsHash), userIdsHash };
}

// ORDER-MISC: findEx sort {time:-1} limit 100 (stable; ties keep data order)
const lastTransactions = (w, key) => w.transactions.filter((t) => t[key] === w.me)
    .map((t, i) => [t, i]).sort((a, b) => b[0].time - a[0].time || a[1] - b[1]).slice(0, 100).map((p) => p[0]);

function unescapeModules(modules) {
    // data.js:167-176 (renamed keys move to the end)
    for (const key in modules) {
        const newKey = key.replace(/\$DOT\$/g, '.').replace(/\$SLASH\$/g, '/').replace(/\$BACKSLASH\$/g, '\\');
        if (newKey != key) {
            modules[newKey] = modules[key];
            delete modules[key];
        }
    }
    return modules;
}

// data.js:31-75 shapes (the `~oracle` module of hybrid bundles reads these too)
function marketData(w) {
    const orders = { all: {} };
    for (const o of rpc(w.market.orders.filter((o) => o.active))) {
        o.id = '' + o._id;
        delete o._id;
        orders[o.resourceType] = orders[o.resourceType] || {};
        orders[o.resourceType][o.id] = o;
        orders.all[o.id] = o;
    }
    const history = { all: [] };
    for (const s of rpc(w.market.stats)) {
        delete s._id;
        history[s.resourceType] = history[s.resourceType] || [];
        history[s.resourceType].push(s);
        history.all.push(s);
    }
    return { orders, history };
}

function oracleData(w) {
    return { accessibleRooms: w.accessibleRooms, roomStatusData: w.roomStatusData, market: marketData(w) };
}

// data.js `get` → runtimeData, or null when the user has no objects (data.js:106, TICK-USER-INACTIVE)
function officialData(w) {
    const { own, roomIds, userIdsHash } = visibility(w);
    if (!own.length) return null;
    const userObjects = {};
    for (const o of rpc(own)) userObjects[o._id] = o;
    const u = user(w);
    const foreign = rpc(roomIds.flatMap((r) => w.objects.filter((o) => o.room === r && o.user !== w.me)));
    for (const i of foreign) {
        if (i.user) userIdsHash[i.user] = true;
        if (i.type == 'controller' && i.reservation) userIdsHash[i.reservation.user] = true;
        if (i.type == 'controller' && i.sign) userIdsHash[i.sign.user] = true;
    }
    const outgoing = lastTransactions(w, 'sender'), incoming = lastTransactions(w, 'recipient');
    outgoing.forEach((i) => i.recipient && (userIdsHash[i.recipient] = true));
    incoming.forEach((i) => i.sender && (userIdsHash[i.sender] = true));
    const [cpu, cpuBucket] = cpuOf(u);
    const roomObjects = {};
    for (const o of foreign) roomObjects[o._id] = o;
    Object.assign(roomObjects, userObjects);
    const rooms = {};
    for (const r of rpc(w.rooms.filter((r) => roomIds.includes(r._id)))) rooms[r._id] = r;
    const roomEventLog = {};
    for (const r of roomIds) roomEventLog[r] = w.eventLogs[r] === undefined ? null : w.eventLogs[r];
    const users = {};
    for (const x of rpc(w.users.filter((x) => userIdsHash[x._id]))) users[x._id] = x;
    const powerCreeps = {};
    for (const pc of rpc(w.powerCreeps.filter((pc) => pc.user === w.me))) powerCreeps[pc._id] = pc;

    const data = {
        userObjects,
        user: rpc(u),
        userCode: unescapeModules(rpc(w.code.modules) || {}),
        userCodeTimestamp: w.code.timestamp || 0,
        userMemory: { data: w.memory || '', userId: w.me },
        consoleCommands: rpc(w.consoleCommands),
        time: w.time,
        rooms,
        roomObjects,
        flags: rpc(w.flags.filter((f) => f.user === w.me)),
        accessibleRooms: JSON.stringify(w.accessibleRooms),
        roomStatusData: rpc(w.roomStatusData),
        transactions: { outgoing: rpc(outgoing), incoming: rpc(incoming) },
        cpu,
        cpuBucket,
        roomEventLog,
        userPowerCreeps: powerCreeps,
        users,
        market: Object.assign(marketData(w), { myOrders: rpc(w.market.orders.filter((o) => o.user === w.me)) }),
    };
    const seg = segmentsOf(w, u);
    if (seg.memorySegments) data.memorySegments = seg.memorySegments;
    if (seg.foreign) data.foreignMemorySegment = seg.foreign;
    return data;
}

// data.js:228-260
function segmentsOf(w, u) {
    const out = {};
    if (u.activeSegments && u.activeSegments.length > 0) {
        out.memorySegments = {};
        for (const id of u.activeSegments) out.memorySegments[id] = w.segments[id] || '';
    }
    const f = u.activeForeignSegment;
    if (f && f.user_id && f.id) {
        const target = w.users.find((x) => x._id === f.user_id);
        const pub = w.publicSegments[f.user_id];
        if (pub && pub.split(',').indexOf('' + f.id) != -1) {
            const data = (w.foreignSegments[f.user_id] || {})[f.id];
            // never-written segment: env.hget answers undefined and the RPC frame drops it (`data` stays undefined)
            out.foreign = { username: f.username, id: f.id, data };
        }
    }
    return out;
}

// The blob of one room (§3.4): ASCII JSON {"room","seq","objects"}; non-ASCII as \uXXXX like Val::write_json.
const ascii = (s) => s.replace(/[\u007f-￿]/g, (c) => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'));
function blob(w, roomName) {
    const objs = [], seq = [];
    w.objects.forEach((o, i) => {
        if (o.room === roomName) {
            objs.push(o);
            seq.push(i);
        }
    });
    const room = w.rooms.find((r) => r._id === roomName);
    return ascii(JSON.stringify({ room, seq, objects: objs }));
}

// §6.12: the host ships binary modules decoded (Uint8Array), text modules as strings
function decodeBinary(modules) {
    for (const k in modules) if (modules[k] && typeof modules[k] === 'object') modules[k] = Uint8Array.from(Buffer.from(modules[k].binary, 'base64'));
    return modules;
}

// data.rs (§6.6). `vm` = {codeVersion, memory} the VM already holds (undefined for a fresh VM); returns the
// __tick arguments plus the updated vm state.
function chitinInput(w, vm = {}) {
    const { own, roomIds, userIdsHash } = visibility(w);
    if (!own.length) return null;
    const u = user(w);
    const roomIdx = (name) => w.rooms.findIndex((r) => r._id === name);
    const loadOnly = [];
    for (const o of own) if (!roomIds.includes(o.room) && !loadOnly.includes(o.room)) loadOnly.push(o.room);
    const metaRooms = [...roomIds.map((r) => [r, 1, roomIdx(r)]), ...loadOnly.map((r) => [r, 0, roomIdx(r)])];
    for (const r of roomIds) {
        for (const i of w.objects) {
            if (i.room !== r || i.user === w.me) continue;
            if (i.user) userIdsHash[i.user] = true;
            if (i.type == 'controller' && i.reservation) userIdsHash[i.reservation.user] = true;
            if (i.type == 'controller' && i.sign) userIdsHash[i.sign.user] = true;
        }
    }
    const outgoing = lastTransactions(w, 'sender'), incoming = lastTransactions(w, 'recipient');
    outgoing.forEach((i) => i.recipient && (userIdsHash[i.recipient] = true));
    incoming.forEach((i) => i.sender && (userIdsHash[i.sender] = true));
    const users = {};
    for (const x of w.users.filter((x) => userIdsHash[x._id])) users[x._id] = { _id: x._id, username: x.username, badge: x.badge || {} };
    const powerCreeps = {};
    for (const pc of w.powerCreeps.filter((pc) => pc.user === w.me)) powerCreeps[pc._id] = pc;
    const [cpu, cpuBucket] = cpuOf(u);
    const seg = segmentsOf(w, u);
    const meta = {
        time: w.time,
        user: u,
        cpu: cpu === Infinity ? null : cpu,
        cpuBucket: cpuBucket === Infinity ? null : cpuBucket,
        codeVersion: w.code.timestamp || 0,
        rooms: metaRooms,
        flags: w.flags.filter((f) => f.user === w.me),
        powerCreeps,
        users,
        transactions: { outgoing, incoming },
        consoleCommands: w.consoleCommands,
        myOrders: w.market.orders.filter((o) => o.user === w.me),
        segments: { activeSegments: u.activeSegments || [], foreign: seg.foreign || null, publicSegments: w.publicSegments[w.me] || '' },
        shard: w.shard,
    };
    const codeVersion = w.code.timestamp || 0;
    const input = {
        meta: ascii(JSON.stringify(meta)),
        blobs: metaRooms.map(([r, , idx]) => (idx < 0 ? null : blob(w, r))),
        eventLogs: roomIds.map((r) => (w.eventLogs[r] === undefined ? null : w.eventLogs[r])),
        memory: vm.memory === (w.memory || '') ? undefined : w.memory || '',
        segments: seg.memorySegments,
        foreignSegment: seg.foreign,
        modules: vm.codeVersion === codeVersion ? undefined : decodeBinary(unescapeModules(rpc(w.code.modules) || {})),
    };
    return { input, vm: { codeVersion, memory: w.memory || '' } };
}

// make.js:196-251 + runner.js saves, for one normalized output (see normalize.js)
function applyOutput(w, out) {
    w.consoleCommands = [];
    if (out.status === 'total-error' || out.status === 'halted') return;
    const u = user(w);
    w.memory = out.memory;
    for (const k in out.memorySegments || {}) w.segments[k] = out.memorySegments[k];
    if (out.activeSegments) u.activeSegments = out.activeSegments;
    if (out.defaultPublicSegment !== undefined) u.defaultPublicSegment = out.defaultPublicSegment;
    if (out.activeForeignSegment !== undefined) {
        const f = out.activeForeignSegment;
        if (f === null) delete u.activeForeignSegment;
        else if (u.activeForeignSegment && f.username == u.activeForeignSegment.username && f.id) u.activeForeignSegment.id = f.id;
        else {
            const t = w.users.find((x) => x.username === f.username);
            const next = { username: f.username, id: f.id };
            if (t) {
                next.user_id = t._id;
                if (!next.id && t.defaultPublicSegment) next.id = t.defaultPublicSegment;
            }
            u.activeForeignSegment = next;
        }
    }
    if (out.publicSegments) w.publicSegments[w.me] = out.publicSegments;
}

const now = (w) => vclock.at(w.clock, w.time);

module.exports = { world, officialData, chitinInput, applyOutput, oracleData, roomNameToXY, now, ascii };
