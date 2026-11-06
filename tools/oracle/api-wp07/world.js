'use strict';
// Builders for vanilla-shaped runtimeData (driver/lib/runtime/data.js) used by the WP07 cases.
// Object docs follow the shapes the official processor writes (spec 11).

const PLAIN = '0'.repeat(2500);
// Room terrain with a wall column at x=25 (y 0..40) and a swamp patch, so paths are not straight lines.
// Edges are walls except an exit span (20..29) towards each neighbour present in `exits` {t,r,b,l}.
function terrain(exits) {
    exits = exits || { t: true, r: true, b: true, l: true };
    const t = PLAIN.split('');
    for (let y = 0; y <= 40; y++) t[y * 50 + 25] = '1';
    for (let y = 10; y < 15; y++) for (let x = 10; x < 15; x++) t[y * 50 + x] = '2';
    for (let i = 0; i < 50; i++) { t[i] = '1'; t[49 * 50 + i] = '1'; t[i * 50] = '1'; t[i * 50 + 49] = '1'; }
    for (let i = 20; i < 30; i++) {
        if (exits.t) t[i] = '0';
        if (exits.r) t[i * 50 + 49] = '0';
        if (exits.l) t[i * 50] = '0';
        if (exits.b) t[49 * 50 + i] = '0';
    }
    return t.join('');
}

// Terrains for a rectangular world W{x0}..W{x1} x N{y0}..N{y1}; outer borders closed.
function world(x0, x1, y0, y1) {
    const rooms = {};
    for (let x = x0; x <= x1; x++) for (let y = y0; y <= y1; y++) {
        // W(x+1) is west of Wx; N(y+1) is north of Ny.
        rooms[`W${x}N${y}`] = terrain({ l: x < x1, r: x > x0, t: y < y1, b: y > y0 });
    }
    return rooms;
}

const body = (parts) => parts.map(p => typeof p === 'string' ? { type: p, hits: 100 } : p);

const D = {
    creep: (o) => Object.assign({
        type: 'creep', name: o.name || o._id, x: 10, y: 20, room: 'W1N1', user: 'u1',
        body: body(['work', 'carry', 'move']), hits: 300, hitsMax: 300, spawning: false, fatigue: 0,
        ageTime: 2500, actionLog: { attacked: null, healed: null, harvest: null, say: null },
        notifyWhenAttacked: true, store: { energy: 0 }, storeCapacity: 50,
    }, o, o.body ? { body: body(o.body) } : {}),
    spawn: (o) => Object.assign({
        type: 'spawn', name: o.name || 'Spawn1', x: 20, y: 20, room: 'W1N1', user: 'u1',
        store: { energy: 300 }, storeCapacityResource: { energy: 300 }, hits: 5000, hitsMax: 5000,
        spawning: null, notifyWhenAttacked: true,
    }, o),
    extension: (o) => Object.assign({
        type: 'extension', x: 22, y: 22, room: 'W1N1', user: 'u1', store: { energy: 50 },
        storeCapacityResource: { energy: 50 }, hits: 1000, hitsMax: 1000, notifyWhenAttacked: true,
    }, o),
    controller: (o) => Object.assign({
        type: 'controller', x: 30, y: 30, room: 'W1N1', user: 'u1', level: 2, progress: 100,
        downgradeTime: 20000, safeModeAvailable: 1, safeModeCooldown: null, safeMode: null,
        upgradeBlocked: null, reservation: null, sign: null, isPowerEnabled: false,
    }, o),
    source: (o) => Object.assign({
        type: 'source', x: 11, y: 21, room: 'W1N1', energy: 3000, energyCapacity: 3000,
        ticksToRegeneration: 300, nextRegenerationTime: null, invaderHarvested: 0,
    }, o),
    mineral: (o) => Object.assign({
        type: 'mineral', x: 40, y: 40, room: 'W1N1', mineralType: 'H', mineralAmount: 70000, density: 3,
    }, o),
    deposit: (o) => Object.assign({
        type: 'deposit', x: 40, y: 10, room: 'W1N1', depositType: 'silicon', harvested: 10,
        cooldownTime: 0, decayTime: 50000,
    }, o),
    structure: (type, o) => Object.assign({ type, x: 5, y: 5, room: 'W1N1', hits: 1000, hitsMax: 1000 }, o),
    site: (o) => Object.assign({
        type: 'constructionSite', x: 12, y: 22, room: 'W1N1', user: 'u1', structureType: 'road',
        progress: 0, progressTotal: 300,
    }, o),
    energy: (o) => Object.assign({ type: 'energy', x: 10, y: 21, room: 'W1N1', resourceType: 'energy', energy: 100 }, o),
    tombstone: (o) => Object.assign({
        type: 'tombstone', x: 9, y: 20, room: 'W1N1', user: 'u1', deathTime: 990, decayTime: 1100,
        creepId: 'deadc', creepName: 'Dead', creepTicksToLive: 3, creepBody: ['work', 'carry', 'move'],
        creepSaying: undefined, store: { energy: 20 },
    }, o),
};

let nextId = 0;
const id = (prefix) => (prefix || 'o') + (++nextId).toString(16).padStart(4, '0');

const USERS = {
    u1: { _id: 'u1', username: 'alice', badge: {} },
    u2: { _id: 'u2', username: 'bob', badge: {} },
    2: { _id: '2', username: 'Invader', badge: {} },
    3: { _id: '3', username: 'Source Keeper', badge: {} },
};

/**
 * runtimeData as data.js builds it: roomObjects = visible room objects in DB order, then own objects
 * (`_.extend(mapById(roomObjects), userObjects)`).
 */
function runtimeData({ time = 1000, me = 'u1', user = {}, objects = [], rooms = ['W1N1'], roomDocs = {},
    memory = '', code, codeTimestamp = 1, powerCreeps = [], market, transactions, roomStatusData,
    accessibleRooms, flags = [] }) {
    const roomObjects = {}, userObjects = {};
    for (const o of objects) {
        roomObjects[o._id] = o;
        if (o.user === me) userObjects[o._id] = o;
    }
    const roomMap = {};
    for (const r of rooms) roomMap[r] = Object.assign({ _id: r, status: 'normal' }, roomDocs[r] || {});
    const userPowerCreeps = {};
    for (const pc of powerCreeps) userPowerCreeps[pc._id] = pc;
    const userIds = {};
    userIds[me] = true;
    for (const o of objects) {
        if (o.user) userIds[o.user] = true;
        if (o.type === 'controller' && o.reservation) userIds[o.reservation.user] = true;
        if (o.type === 'controller' && o.sign) userIds[o.sign.user] = true;
    }
    const users = {};
    for (const u of Object.keys(userIds)) if (USERS[u]) users[u] = USERS[u];
    return {
        userObjects,
        user: Object.assign({
            _id: me, username: USERS[me].username, cpu: 100, cpuAvailable: 10000, gcl: 1000000, power: 0,
            money: 0, rooms: ['W1N1'], resources: {}, active: 10000,
        }, user),
        userCode: code,
        userCodeTimestamp: codeTimestamp,
        userMemory: { data: memory, userId: me },
        consoleCommands: [],
        time,
        rooms: roomMap,
        roomObjects: Object.assign(roomObjects, userObjects),
        flags,
        accessibleRooms: JSON.stringify(accessibleRooms || rooms),
        roomStatusData: roomStatusData || { novice: {}, respawn: {}, closed: {} },
        transactions: transactions || { outgoing: [], incoming: [] },
        cpu: null,
        cpuBucket: null,
        roomEventLog: {},
        userPowerCreeps,
        users,
        market: market || { orders: { all: {} }, history: {}, myOrders: [] },
    };
}

module.exports = { D, id, body, terrain, world, PLAIN, USERS, runtimeData };
