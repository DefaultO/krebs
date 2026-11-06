// Room.createConstructionSite / createFlag and their RoomPosition forms: validation order and return codes, intents,
// spawn/flag naming, tick-local counters (spec 03 ROOM-CREATECS, ROOM-CREATEFLAG, POS-CREATEFLAG, POS-CREATECS).
'use strict';
const { base } = require('../world');

const manySites = [];
for (let i = 0; i < 98; i++) manySites.push({ _id: 'ms' + i, type: 'constructionSite', room: 'W2N2', x: 1 + i % 40, y: 10 + (i / 40 | 0), user: 'u1', structureType: 'road', progress: 0, progressTotal: 300 });

module.exports = [{
    name: 'create-construction-site',
    tags: ['game/rooms.js:1029', 'game/rooms.js:1630'],
    world: base(),
    ticks: [{ probes: [
        ['args', () => {
            const r = Game.rooms.W1N1;
            return [r.createConstructionSite(), r.createConstructionSite(50, 10, 'road'), r.createConstructionSite(-1, 10, 'road'),
                r.createConstructionSite(10, NaN, 'road'), r.createConstructionSite(10, 10), r.createConstructionSite(10, 10, 'bogus'),
                r.createConstructionSite({x: 26, y: 26}, 'road'), r.createConstructionSite('26', 26, 'road'), r.createConstructionSite(26, '26', 'road')];
        }],
        ['forms', () => {
            const r = Game.rooms.W1N1, p = new RoomPosition(26, 23, 'W1N1');
            return [r.createConstructionSite(p, 'road'), r.createConstructionSite(Game.creeps.Harv1, STRUCTURE_CONTAINER),
                r.createConstructionSite(p, STRUCTURE_SPAWN, 'Named'), r.createConstructionSite(new RoomPosition(28, 23, 'W1N2'), 'road'),
                p.createConstructionSite(STRUCTURE_TOWER), new RoomPosition(29, 22, 'W1N1').createConstructionSite(STRUCTURE_SPAWN, 'PosNamed')];
        }],
        ['targets', () => {
            const r = Game.rooms.W1N1;
            return [r.createConstructionSite(25, 25, 'road'), r.createConstructionSite(25, 27, 'road'), r.createConstructionSite(25, 27, 'rampart'),
                r.createConstructionSite(29, 29, 'road'), r.createConstructionSite(22, 24, 'container'), r.createConstructionSite(24, 26, 'rampart'),
                r.createConstructionSite(24, 26, 'road'), r.createConstructionSite(30, 20, 'extractor'), r.createConstructionSite(21, 21, 'road'),
                r.createConstructionSite(0, 10, 'road'), r.createConstructionSite(1, 1, 'road'), r.createConstructionSite(1, 25, 'container'),
                r.createConstructionSite(1, 25, 'extension'), r.createConstructionSite(48, 30, 'link')];
        }],
        ['terrain', () => {
            const r = Game.rooms.W1N1, t = r.getTerrain(), out = [];
            for (let y = 2; y < 48 && out.length < 6; y++) for (let x = 2; x < 48 && out.length < 6; x++) {
                if (t.get(x, y) === TERRAIN_MASK_WALL) out.push([x, y, r.createConstructionSite(x, y, 'road'), r.createConstructionSite(x, y, 'container')]);
            }
            return out;
        }],
        ['rcl', () => {
            const r = Game.rooms.W1N1;
            return [r.createConstructionSite(20, 25, 'tower'), r.createConstructionSite(20, 26, 'storage'), r.createConstructionSite(20, 27, 'spawn'),
                r.createConstructionSite(20, 28, 'extension'), r.createConstructionSite(30, 28, 'lab'), r.createConstructionSite(30, 27, 'observer')];
        }],
        ['spawn names', () => {
            const r = Game.rooms.W1N1;
            return [r.createConstructionSite(30, 25, 'spawn', 'Spawn1'), r.createConstructionSite(30, 25, 'spawn', 'BobSpawn'),
                r.createConstructionSite(30, 25, 'spawn', 'Spawn2'), r.createConstructionSite(30, 25, 'spawn', 'x'.repeat(101)),
                r.createConstructionSite(30, 25, 'spawn', 5), r.createConstructionSite(30, 24, 'spawn'), r.createConstructionSite(30, 23, 'spawn', 'Spawn3')];
        }],
        ['owner', () => [Game.rooms.W1N2.createConstructionSite(22, 25, 'road'), Game.rooms.W1N2.createConstructionSite(30, 30, 'road')]],
        ['intents', () => 0],
    ] }],
}, {
    name: 'create-construction-site-spawn-names',
    tags: ['game/rooms.js:1077'],
    world: base({ objects: base().objects.map(o => o._id == 'c1' ? Object.assign({}, o, { level: 7 }) : o) }),
    ticks: [{ probes: [
        ['auto', () => {
            const r = Game.rooms.W1N1;
            return [r.createConstructionSite(30, 25, 'spawn'), r.createConstructionSite(30, 24, 'spawn', 'Spawn3'),
                r.createConstructionSite(30, 23, 'spawn'), r.createConstructionSite(30, 22, 'spawn', 'Fresh'), r.createConstructionSite(30, 21, 'spawn', 'Fresh'),
                new RoomPosition(29, 21, 'W1N1').createConstructionSite('spawn', 'ViaPos')];
        }],
    ] }],
}, {
    name: 'create-construction-site-full',
    tags: ['game/rooms.js:1071'],
    world: base({ objects: base().objects.concat(manySites) }),
    ticks: [{ probes: [
        ['full', () => {
            const r = Game.rooms.W1N1;
            return [r.createConstructionSite(26, 22, 'road'), r.createConstructionSite(27, 22, 'road'), r.createConstructionSite(28, 22, 'road'),
                r.createConstructionSite(10, 10, 'bogus')];
        }],
    ] }],
}, {
    name: 'create-construction-site-reserved',
    tags: ['game/rooms.js:1052'],
    world: base({
        objects: [
            { _id: 'c9', type: 'controller', room: 'W1N1', x: 25, y: 22, level: 0, reservation: { user: 'u2', endTime: 3000 } },
            { _id: 'cr9', type: 'creep', room: 'W1N1', x: 25, y: 25, user: 'u1', name: 'Res', body: [], store: {}, spawning: false },
            { _id: 'c8', type: 'controller', room: 'W1N2', x: 25, y: 22, level: 0, reservation: { user: 'u1', endTime: 3000 } },
            { _id: 'cr8', type: 'creep', room: 'W1N2', x: 25, y: 25, user: 'u1', name: 'Res2', body: [], store: {}, spawning: false },
        ],
    }),
    ticks: [{ probes: [
        ['reserved', () => [Game.rooms.W1N1.createConstructionSite(26, 26, 'road'), Game.rooms.W1N2.createConstructionSite(26, 26, 'road'),
            Game.rooms.W1N2.createConstructionSite(26, 27, 'extension'), Game.rooms.W1N2.createConstructionSite(26, 27, 'container')]],
    ] }],
}, {
    name: 'create-flag',
    tags: ['game/rooms.js:978', 'game/rooms.js:1622'],
    world: base(),
    ticks: [{ probes: [
        ['create', () => {
            const r = Game.rooms.W1N1;
            return [r.createFlag(10, 10), r.createFlag(11, 11), r.createFlag(12, 12, 'Flag1'), r.createFlag(12, 12, 'New', COLOR_RED, COLOR_BLUE),
                r.createFlag(12, 12, 'New'), r.createFlag(13, 13, 'Col', '1'), r.createFlag(13, 13, 'Col2', 11), r.createFlag(13, 13, 'Col3', 2, 12),
                r.createFlag(13, 13, 'x'.repeat(101)), r.createFlag(50, 1, 'Oob'), r.createFlag(), r.createFlag(new RoomPosition(5, 5, 'W1N2'), 'OtherRoom', 3),
                r.createFlag(Game.creeps.Harv1, 'OnCreep'), r.createFlag(14, 14, 0, 0, 0), r.createFlag(15, 15, {}, 2)];
        }],
        ['pos form', () => [new RoomPosition(20, 21, 'W1N1').createFlag(), new RoomPosition(20, 22, 'W1N1').createFlag('PF', 4, 5),
            new RoomPosition(20, 22, 'W1N2').createFlag('PF2')]],
        ['game flags', () => [Object.keys(Game.flags), Game.flags.New && [Game.flags.New.color, Game.flags.New.secondaryColor, Game.flags.New.pos],
            Game.rooms.W1N1.find(FIND_FLAGS).length, Game.getObjectById('flag_New'), Game.getObjectById('flag_Flag1') === Game.flags.Flag1]],
    ] }],
}, {
    name: 'create-flag-removed-name',
    tags: ['game/rooms.js:1010', 'game/flags.js:57'],
    world: base(),
    ticks: [{ probes: [
        ['remove then create', () => [Game.flags.Flag1.remove(), Game.rooms.W1N1.createFlag(5, 5, 'Flag1'), Game.rooms.W1N1.createFlag(6, 6)]],
        ['full', () => {
            for (let i = Object.keys(Game.flags).length; i < FLAGS_LIMIT - 1; i++) Game.flags['pad' + i] = 1;
            return [Game.rooms.W1N1.createFlag(7, 7, 'Last'), Game.rooms.W1N1.createFlag(8, 8, 'OneTooMany'), Game.rooms.W1N1.createFlag(50, 8, 'Oob'),
                (delete Game.flags.Last, Game.rooms.W1N1.createFlag(8, 8, 'Last'))];
        }],
    ] }],
}, {
    // 98 existing sites: the tick-local createdConstructionSites counter (shared by the Room and RoomPosition forms)
    // decides the third call; make() resets it, so the next tick allows two more again
    name: 'create-construction-site-counter',
    tags: ['game/rooms.js:1029', 'game/rooms.js:1630'],
    world: base({ objects: base().objects.concat(manySites.slice(0, 96)) }),
    ticks: [0, 1].map(() => ({ probes: [
        ['count', () => [Game.rooms.W1N1.createConstructionSite(26, 22, 'road'), new RoomPosition(27, 22, 'W1N1').createConstructionSite('road'),
            Game.rooms.W1N1.createConstructionSite(28, 22, 'road'), new RoomPosition(29, 22, 'W1N1').createConstructionSite('road')]],
    ] })),
}];
