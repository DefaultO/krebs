'use strict';
// Shared W1N1..W5N1 test world of the WP07 cases (files starting with '_' are not case modules).
const { D, world: worldTerrain, runtimeData } = require('../world');

const ROOMS = worldTerrain(0, 7, 1, 1);
const ALL_ROOMS = ['W1N1', 'W2N1', 'W3N1', 'W4N1', 'W5N1'];

// W1N1: own RCL2 room. "Worker" hub at (10,20) with one object on every neighbour tile.
function hubObjects() {
    return [
        D.controller({ _id: 'ctrl1', x: 30, y: 30 }),
        D.creep({ _id: 'cWork', name: 'Worker', x: 10, y: 20, store: { energy: 50 } }),
        D.source({ _id: 'srcE', x: 9, y: 19, energy: 0 }),
        D.extension({ _id: 'ext2', x: 10, y: 19, store: { energy: 0 } }),
        D.extension({ _id: 'extF', x: 11, y: 19 }),
        D.tombstone({ _id: 'tomb', x: 9, y: 20, store: { energy: 20, H: 5 } }),
        D.creep({ _id: 'cEmpty', name: 'Empty', x: 11, y: 20, body: ['work', 'carry', 'carry', 'move'], storeCapacity: 100 }),
        D.structure('container', { _id: 'cont', x: 9, y: 21, store: { energy: 100, H: 10 }, storeCapacity: 2000, hits: 250000, hitsMax: 250000, nextDecayTime: 1500 }),
        D.energy({ _id: 'drop1', x: 10, y: 21, energy: 100 }),
        D.source({ _id: 'src1', x: 11, y: 21 }),
        D.source({ _id: 'srcFar', x: 40, y: 25 }),
        D.energy({ _id: 'dropFar', x: 45, y: 45, resourceType: 'H', H: 30, energy: undefined }),
        D.structure('road', { _id: 'road1', x: 10, y: 22, hits: 4000, hitsMax: 5000, nextDecayTime: 1200 }),
        D.structure('road', { _id: 'roadFar', x: 10, y: 24, hits: 4000, hitsMax: 5000, nextDecayTime: 1200 }),
        D.site({ _id: 'siteRoad', x: 12, y: 22 }),
        D.site({ _id: 'siteFar', x: 14, y: 20, structureType: 'road' }),
        D.site({ _id: 'siteExt', x: 8, y: 22, structureType: 'extension', progressTotal: 3000 }),
        D.site({ _id: 'siteBlocked', x: 11, y: 22, structureType: 'spawn', progressTotal: 15000 }),
        D.creep({ _id: 'cBob', name: 'Bob', user: 'u2', x: 11, y: 22, store: { energy: 10 }, actionLog: { say: { message: 'hi bob', isPublic: false } } }),
        D.site({ _id: 'siteMine', x: 12, y: 21, structureType: 'constructedWall' }),
        D.creep({ _id: 'cMine', name: 'Mine', x: 12, y: 21, store: { energy: 0 } }),
        D.site({ _id: 'siteHostile', x: 9, y: 22, structureType: 'road', user: 'u2' }),
        D.site({ _id: 'siteOnWall', x: 8, y: 21, structureType: 'tower' }),
        D.structure('constructedWall', { _id: 'wall1', x: 8, y: 21, hits: 1, hitsMax: 300000000 }),
        D.spawn({ _id: 'spawn1', x: 20, y: 20 }),
        D.creep({ _id: 'cBaby', name: 'Baby', x: 20, y: 20, spawning: true, ageTime: undefined }),
        D.creep({ _id: 'cTired', name: 'Tired', x: 5, y: 30, fatigue: 4 }),
        D.creep({ _id: 'cStuck', name: 'Stuck', x: 6, y: 30, body: ['work', 'carry'] }),
        D.creep({ _id: 'cHurt', name: 'Hurt', x: 7, y: 30, body: [{ type: 'move', hits: 0 }, { type: 'work', hits: 50 }, 'carry', 'carry'], hits: 250 }),
        D.creep({ _id: 'cLoud', name: 'Loud', x: 8, y: 30, actionLog: { say: { message: 'public!', isPublic: true } } }),
        D.creep({ _id: 'cBobPub', name: 'BobPub', user: 'u2', x: 9, y: 30, actionLog: { say: { message: 'yo', isPublic: true } } }),
        D.creep({ _id: 'cCarrier', name: 'Carrier', x: 12, y: 19, body: ['carry', 'move'], store: { energy: 40 } }),
        D.creep({ _id: 'cUp', name: 'Up', x: 28, y: 28, store: { energy: 50 } }),
        D.creep({ _id: 'cUpFar', name: 'UpFar', x: 34, y: 34, store: { energy: 50 } }),
    ];
}

// "Looter" hub at (30,10): withdraw targets on every neighbour tile.
function lootObjects() {
    return [
        D.creep({ _id: 'cLoot', name: 'Looter', x: 30, y: 10, body: ['carry', 'carry', 'move'], storeCapacity: 100, store: { energy: 10 } }),
        D.structure('storage', { _id: 'stor', x: 29, y: 9, user: 'u1', store: { energy: 5000, H: 50 }, storeCapacity: 1000000, hits: 10000, hitsMax: 10000 }),
        D.structure('terminal', { _id: 'term', x: 30, y: 9, user: 'u1', store: { energy: 500 }, storeCapacity: 300000, effects: [{ power: 15, level: 1, endTime: 1010 }] }),
        D.structure('storage', { _id: 'storBob', x: 31, y: 9, user: 'u2', store: { energy: 500 }, storeCapacity: 1000000 }),
        D.structure('rampart', { _id: 'rampBob', x: 31, y: 9, user: 'u2', isPublic: false, hits: 1000, hitsMax: 3000000 }),
        D.structure('container', { _id: 'contPub', x: 29, y: 10, store: { energy: 30 }, storeCapacity: 2000 }),
        D.structure('rampart', { _id: 'rampPub', x: 29, y: 10, user: 'u2', isPublic: true }),
        D.structure('nuker', { _id: 'nuker', x: 31, y: 10, user: 'u1', store: { energy: 100, G: 0 }, storeCapacityResource: { energy: 300000, G: 5000 } }),
        D.structure('powerBank', { _id: 'pbank', x: 29, y: 11, store: { power: 3000 }, hits: 2000000, hitsMax: 2000000, decayTime: 5000 }),
        { _id: 'ruin1', type: 'ruin', x: 30, y: 11, room: 'W1N1', structureType: 'extension', destroyTime: 990, decayTime: 1500, store: { energy: 40 }, structure: { id: 'oldext', type: 'extension', hits: 0, hitsMax: 1000, user: 'u1' } },
        D.tombstone({ _id: 'tombBob', x: 31, y: 11, user: 'u2', store: { H: 7 } }),
        D.structure('storage', { _id: 'storEmpty', x: 31, y: 12, user: 'u1', store: {}, storeCapacity: 1000000 }),
        D.structure('terminal', { _id: 'termOld', x: 29, y: 12, user: 'u1', store: { energy: 500 }, storeCapacity: 300000, effects: [{ power: 15, level: 1, endTime: 990 }] }),
    ];
}

// Mineral ring around "Miner" (40,40) in W1N1 (RCL2: own extractor inactive) and W5N1 (RCL6); W2N1 owned by
// bob (safe mode), W3N1 reserved by bob, W4N1 reserved by alice, each with an own creep at (10,20).
function remoteObjects() {
    return [
        D.creep({ _id: 'cMiner', name: 'Miner', x: 40, y: 40 }),
        D.mineral({ _id: 'mNone', x: 39, y: 39 }),
        D.mineral({ _id: 'mForeign', x: 40, y: 39 }),
        D.structure('extractor', { _id: 'xForeign', x: 40, y: 39, user: 'u2', cooldown: 0 }),
        D.mineral({ _id: 'mInactive', x: 41, y: 39 }),
        D.structure('extractor', { _id: 'xInactive', x: 41, y: 39, user: 'u1', cooldown: 0 }),
        D.mineral({ _id: 'mEmpty', x: 39, y: 40, mineralAmount: 0 }),
        D.deposit({ _id: 'dep', x: 41, y: 40 }),
        D.deposit({ _id: 'depCool', x: 39, y: 41, cooldownTime: 1010 }),
        D.mineral({ _id: 'mUnowned', x: 41, y: 41 }),
        D.structure('extractor', { _id: 'xUnowned', x: 41, y: 41, cooldown: 0 }),
        D.controller({ _id: 'ctrl5', room: 'W5N1', level: 6 }),
        D.creep({ _id: 'cMiner5', name: 'Miner5', room: 'W5N1', x: 40, y: 40 }),
        D.mineral({ _id: 'mOk', room: 'W5N1', x: 39, y: 39 }),
        D.structure('extractor', { _id: 'xOk', room: 'W5N1', x: 39, y: 39, user: 'u1', cooldown: 0 }),
        D.mineral({ _id: 'mCool', room: 'W5N1', x: 40, y: 39 }),
        D.structure('extractor', { _id: 'xCool', room: 'W5N1', x: 40, y: 39, user: 'u1', cooldown: 3 }),
        D.controller({ _id: 'ctrl2', room: 'W2N1', x: 11, y: 19, user: 'u2', level: 3, safeMode: 1500 }),
        D.creep({ _id: 'cRaid2', name: 'Raid2', room: 'W2N1', x: 10, y: 20, body: ['work', 'carry', 'claim', 'attack', 'ranged_attack', 'heal', 'move'], store: { energy: 10, G: 1000 } }),
        D.source({ _id: 'src2', room: 'W2N1', x: 11, y: 21 }),
        D.structure('container', { _id: 'cont2', room: 'W2N1', x: 9, y: 21, store: { energy: 100 }, storeCapacity: 2000 }),
        D.creep({ _id: 'cBob2', name: 'Bob2', user: 'u2', room: 'W2N1', x: 9, y: 19 }),
        D.controller({ _id: 'ctrl3', room: 'W3N1', x: 11, y: 19, user: undefined, level: 0, reservation: { user: 'u2', endTime: 3000 } }),
        D.creep({ _id: 'cRaid3', name: 'Raid3', room: 'W3N1', x: 10, y: 20, body: ['work', 'carry', 'claim', 'move'], store: { energy: 10 } }),
        D.source({ _id: 'src3', room: 'W3N1', x: 11, y: 21 }),
        D.controller({ _id: 'ctrl4', room: 'W4N1', x: 11, y: 19, user: undefined, level: 0, reservation: { user: 'u1', endTime: 3000 } }),
        D.creep({ _id: 'cRaid4', name: 'Raid4', room: 'W4N1', x: 10, y: 20, body: ['work', 'carry', 'claim', 'move'], store: { energy: 10 } }),
        D.source({ _id: 'src4', room: 'W4N1', x: 11, y: 21 }),
    ];
}

const world = (opts, patch) => {
    const objects = hubObjects().concat(lootObjects(), remoteObjects());
    if (patch) patch(objects);
    return Object.assign({ objects, rooms: ALL_ROOMS }, opts || {});
};
const tick = (codeObj, opts, patch) => runtimeData(Object.assign(world(opts, patch), { code: codeObj }));
const byId = (objects, id) => objects.find(o => o._id === id);

module.exports = { ROOMS, ALL_ROOMS, hubObjects, lootObjects, remoteObjects, world, tick, byId };
