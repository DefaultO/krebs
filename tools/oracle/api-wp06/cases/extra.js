// Remaining object kinds in find/look, the default cost matrix (getPathfindingGrid2) per option set, safe mode,
// PathFinder.use across ticks (stale register), custom object prototypes with find/look constants
// (spec 03 FIND-*, LOOK-*, ROOM-DEFAULT-COSTMATRIX, PF-USE, MOD-CUSTOM-PROTO; spec 09 PF-API-USE).
'use strict';
const { base } = require('../world');

const more = [
    { _id: 't1', type: 'tombstone', room: 'W1N1', x: 23, y: 23, user: 'u2', deathTime: 990, decayTime: 1100, creepId: 'old', creepName: 'Old',
      creepBody: ['move'], store: { energy: 5 } },
    { _id: 'ru1', type: 'ruin', room: 'W1N1', x: 23, y: 23, destroyTime: 990, decayTime: 1200, structure: { id: 'x', type: 'road', hits: 0, hitsMax: 5000 }, store: {} },
    { _id: 'n1', type: 'nuke', room: 'W1N1', x: 24, y: 23, landTime: 1500, launchRoomName: 'E2S2' },
    { _id: 'd1', type: 'deposit', room: 'W1N1', x: 29, y: 23, depositType: 'mist', harvested: 10, cooldownTime: 1005, decayTime: 5000 },
    { _id: 'pc1', type: 'powerCreep', room: 'W1N1', x: 26, y: 23, user: 'u2', name: 'PC', className: 'operator', level: 1, hits: 1000, hitsMax: 1000,
      store: {}, storeCapacity: 100, ageTime: 5000 },
    { _id: 'po1', type: 'portal', room: 'W1N1', x: 30, y: 30, destination: { room: 'E2S2', x: 25, y: 25 } },
    { _id: 'ra2', type: 'rampart', room: 'W1N1', x: 21, y: 23, user: 'u2', hits: 1000, hitsMax: 300000, isPublic: true },
    { _id: 'ra3', type: 'rampart', room: 'W1N1', x: 21, y: 24, user: 'u1', hits: 1000, hitsMax: 300000, isPublic: false },
    { _id: 'cs6', type: 'constructionSite', room: 'W1N1', x: 21, y: 25, user: 'u1', structureType: 'road', progress: 0, progressTotal: 300 },
    { _id: 'cs7', type: 'constructionSite', room: 'W1N1', x: 21, y: 26, user: 'u2', structureType: 'tower', progress: 0, progressTotal: 5000 },
    { _id: 'r3', type: 'road', room: 'W1N1', x: 21, y: 26, hits: 5000, hitsMax: 5000 },
    { _id: 'r4', type: 'road', room: 'W1N1', x: 21, y: 21, hits: 5000, hitsMax: 5000 },
    { _id: 'r5', type: 'road', room: 'W1N1', x: 0, y: 0, hits: 5000, hitsMax: 5000 },
    { _id: 'cr5', type: 'creep', room: 'W1N1', x: 28, y: 26, user: '2', name: 'Invader_1', hits: 100, hitsMax: 100, body: [{ type: 'attack', hits: 100 }],
      store: {}, spawning: false },
    { _id: 'ct1', type: 'container', room: 'W1N1', x: 20, y: 24, hits: 250000, hitsMax: 250000, store: {}, storeCapacity: 2000 },
];

const matrixProbe = () => {
    const sets = [{}, {ignoreCreeps: true}, {ignoreDestructibleStructures: true}, {ignoreRoads: true},
        {ignoreCreeps: true, ignoreDestructibleStructures: true, ignoreRoads: true}];
    return sets.map(o => {
        let bits;
        const r = Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(25, 20, 'W1N1'),
            Object.assign({costCallback: (rn, m) => { if (rn == 'W1N1') bits = Array.from(m._bits); }}, o));
        return [bits.map((v, i) => v ? i + ':' + v : '').filter(Boolean).join(' '), r.length];
    });
};

module.exports = [{
    name: 'find-look-more-types',
    tags: ['game/rooms.js:584', 'game/rooms.js:659', 'game/rooms.js:707'],
    world: base({ objects: base().objects.concat(more) }),
    ticks: [{ probes: [
        ['find', () => [FIND_TOMBSTONES, FIND_RUINS, FIND_NUKES, FIND_DEPOSITS, FIND_POWER_CREEPS, FIND_MY_POWER_CREEPS, FIND_HOSTILE_POWER_CREEPS,
            FIND_HOSTILE_CREEPS, FIND_STRUCTURES, FIND_HOSTILE_STRUCTURES, FIND_CONSTRUCTION_SITES].map(t => Game.rooms.W1N1.find(t))],
        ['lookAt', () => [Game.rooms.W1N1.lookAt(23, 23), Game.rooms.W1N1.lookAt(24, 23), Game.rooms.W1N1.lookAt(26, 23), Game.rooms.W1N1.lookAt(29, 23),
            Game.rooms.W1N1.lookAt(21, 26), Game.rooms.W1N1.lookAt(0, 0)]],
        ['area', () => [Game.rooms.W1N1.lookAtArea(22, 22, 24, 30, true), Game.rooms.W1N1.lookAtArea(22, 22, 23, 23)]],
        ['forAt', () => [LOOK_TOMBSTONES, LOOK_RUINS, LOOK_NUKES, LOOK_DEPOSITS, LOOK_POWER_CREEPS].map(t =>
            [Game.rooms.W1N1.lookForAt(t, 23, 23), Game.rooms.W1N1.lookForAtArea(t, 20, 20, 30, 30, true)])],
        ['closest', () => [Game.creeps.Harv1.pos.findClosestByRange(FIND_HOSTILE_CREEPS), Game.creeps.Harv1.pos.findClosestByPath(FIND_TOMBSTONES),
            Game.creeps.Harv1.pos.findInRange(FIND_STRUCTURES, 4).length]],
        ['matrix', matrixProbe],
    ] }],
}, {
    name: 'path-matrix-safe-mode',
    tags: ['game/rooms.js:157', 'game/rooms.js:198'],
    world: base({ objects: base().objects.concat(more).map(o => o._id == 'c1' ? Object.assign({}, o, { safeMode: 1500 }) : o) }),
    ticks: [{ probes: [
        ['safe', () => [Game.rooms.W1N1.controller.safeMode, Game.rooms.W1N1.controller.my]],
        ['matrix', matrixProbe],
        ['cached', () => {
            const ms = [];
            const cb = (rn, m) => { ms.push(m); };
            Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(25, 20, 'W1N1'), {costCallback: cb});
            Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(25, 20, 'W1N1'), {costCallback: cb});
            ms[0].set(25, 24, 255);
            return [ms.length, ms[0] === ms[1], Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(25, 20, 'W1N1'))];
        }],
    ] }],
}, {
    name: 'path-use-stale-register',
    tags: ['game/path-finder.js:67', 'game/rooms.js:869'],
    world: base(),
    ticks: [
        { probes: [['t1', () => Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(30, 30, 'W1N1')).length]] },
        { probes: [['t2 use false', () => { PathFinder.use(false); return Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(30, 30, 'W1N1')); }]] },
        { probes: [['t3', () => Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(30, 30, 'W1N1'))]] },
    ],
}, {
    name: 'path-use-first-tick',
    tags: ['game/path-finder.js:67'],
    world: base(),
    ticks: [
        { probes: [['t1 use false', () => { PathFinder.use(false); return Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(30, 30, 'W1N1')); }]] },
        { probes: [['t2', () => Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(30, 30, 'W1N1'))],
            ['t2 use true', () => { PathFinder.use(true); return Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(30, 30, 'W1N1')); }]] },
    ],
}, {
    name: 'custom-object-prototypes',
    tags: ['game/rooms.js:516', 'game/rooms.js:787', 'game/rooms.js:836'],
    world: base({
        objects: base().objects.concat([
            { _id: 'w9', type: 'widget', room: 'W1N1', x: 26, y: 26, user: 'u2', power: 3 },
            { _id: 'w8', type: 'widget', room: 'W1N1', x: 26, y: 26, user: 'u1', power: 0 },
            { _id: 'g1', type: 'gadget', room: 'W1N1', x: 20, y: 20 },
        ]),
        customObjectPrototypes: [
            { objectType: 'widget', name: 'Widget', opts: { properties: { power: 'o => o.power' }, userOwned: true, findConstant: 9001, lookConstant: 'widget' } },
            { objectType: 'gadget', name: 'Gadget', opts: { lookConstant: 'gadget' } },
        ],
    }),
    ticks: [{ probes: [
        ['find', () => [Game.rooms.W1N1.find(9001), Game.rooms.W1N1.find(9001, {filter: 'my'})]],
        ['look', () => [Game.rooms.W1N1.lookAt(26, 26), Game.rooms.W1N1.lookForAt('widget', 26, 26), Game.rooms.W1N1.lookForAt('gadget', 20, 20),
            Game.rooms.W1N1.lookAtArea(19, 19, 27, 27, true).filter(i => i.type != 'terrain'), Game.rooms.W1N1.lookForAtArea('widget', 25, 25, 27, 27),
            Game.rooms.W1N1.lookAt(20, 20)]],
    ] }],
}];
