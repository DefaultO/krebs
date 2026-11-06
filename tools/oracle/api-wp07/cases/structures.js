'use strict';
// Structure (game/structures.js) cases: Structure/OwnedStructure base, spawn, Spawning, controller, extension,
// then every other structure class.
const { D } = require('../world');
const { probe, code, matrix, JUNK } = require('./_lib');
const { ROOMS, ALL_ROOMS, tick, byId } = require('./_world');

const cases = [];
const add = (name, tags, ticks) => cases.push({ name, tags, rooms: ROOMS, ticks });

// Extra spawns and renew/recycle targets around spawn1 (20,20).
const spawnObjects = () => [
    D.spawn({ _id: 'spawnBusy', name: 'Busy', x: 25, y: 35, spawning: { name: 'Baby', needTime: 9, spawnTime: 1005, directions: [1, 2] } }),
    D.spawn({ _id: 'spawnOff', name: 'Off', x: 27, y: 35, off: true }),
    D.spawn({ _id: 'spawnNoDir', name: 'NoDir', x: 29, y: 35, spawning: { name: 'Kid', needTime: 3, spawnTime: 1001 } }),
    D.spawn({ _id: 'spawnBob', name: 'BobSpawn', user: 'u2', room: 'W2N1', x: 20, y: 20, spawning: { name: 'Bobby', needTime: 9, spawnTime: 1003, directions: [3] } }),
    D.spawn({ _id: 'spawn5', name: 'Five', room: 'W5N1', x: 20, y: 20, store: { energy: 100 } }),
    D.creep({ _id: 'cOld', name: 'Old', x: 21, y: 20, ageTime: 1100, body: ['work', 'work', 'carry', 'move'] }),
    D.creep({ _id: 'cFresh', name: 'Fresh', x: 19, y: 20, ageTime: 2490 }),
    D.creep({ _id: 'cClaim', name: 'Claimer', x: 21, y: 21, body: ['claim', 'move'] }),
    D.creep({ _id: 'cBoost', name: 'Boosted', x: 19, y: 21, ageTime: 1100, body: [{ type: 'work', hits: 100, boost: 'UH' }, 'move'] }),
    D.creep({ _id: 'cBobNear', name: 'BobNear', user: 'u2', x: 21, y: 19, ageTime: 1100 }),
    D.creep({ _id: 'cFive', name: 'Fiver', room: 'W5N1', x: 21, y: 20, ageTime: 1100 }),
];

add('structure-base', ['game/structures.js:39', 'game/structures.js:62', 'game/structures.js:68', 'game/structures.js:72',
    'game/structures.js:89', 'game/structures.js:108', 'game/structures.js:129', 'game/structures.js:135'], [
    tick(probe([
        ...['ext2', 'extF', 'stor', 'storBob', 'cont', 'road1', 'wall1', 'xInactive', 'xOk', 'xForeign', 'xUnowned', 'rampBob', 'rampPub',
            'term', 'nuker', 'pbank', 'ctrl1', 'ctrl2', 'ctrl3', 'spawn1', 'cont2'].map(id =>
            `[G('${id}').hits, G('${id}').hitsMax, G('${id}').structureType, G('${id}').owner, G('${id}').my, String(G('${id}')), G('${id}').isActive(), G('${id}') instanceof OwnedStructure, Object.keys(G('${id}'))]`),
        ...matrix('notifyWhenAttacked', ['ext2', 'road1', 'cont', 'storBob', 'cont2', 'xOk', 'ctrl3', 'spawn1', 'spawnBob'],
            [''], ['true', 'false', "'x'", 'undefined', '0']),
        ...matrix('destroy', ['ext2', 'road1', 'storBob', 'cont2', 'xOk', 'ctrl3', 'ctrl1', 'src3'], ['']),
        "Object.getOwnPropertyNames(Structure.prototype)",
        "Object.getOwnPropertyNames(OwnedStructure.prototype)",
        "Object.getPrototypeOf(OwnedStructure.prototype) === Structure.prototype",
        "[Structure.length, OwnedStructure.length, Structure.name]",
        "new Structure()",
        "new OwnedStructure().my",
        "new Structure('nope')",
        "new Structure('cWork').structureType",
        "Object.keys(Game.structures)",
        "Game.rooms.W1N1.controller.id + Game.rooms.W1N1.storage.id + Game.rooms.W1N1.terminal.id",
        "JSON.stringify(G('ext2'))",
    ]), {}, (o) => {
        byId(o, 'road1').notifyWhenAttacked = false;
        o.push(D.structure('road', { _id: 'roadNull', x: 3, y: 3, user: null }), D.structure('extension', { _id: 'extNoStore', x: 4, y: 3, user: 'u1', off: true }));
    }),
    // no hostile creeps in W5N1, then a hostile power creep there
    tick(probe(matrix('destroy', ['xOk', 'spawn5'], ['']).concat(["G('roadNull').owner", "G('roadNull').my", "G('extNoStore').energy"])),
        {}, (o) => {
            o.push(D.spawn({ _id: 'spawn5', name: 'Five', room: 'W5N1' }));
            o.push(D.structure('road', { _id: 'roadNull', x: 3, y: 3, user: null }), D.structure('extension', { _id: 'extNoStore', x: 4, y: 3, user: 'u1', off: true, store: undefined, storeCapacityResource: undefined }));
        }),
    tick(probe(matrix('destroy', ['xOk'], [''])), {}, (o) => {
        o.push({ _id: 'pcBob', type: 'powerCreep', user: 'u2', name: 'PcBob', className: 'operator', level: 1, room: 'W5N1', x: 5, y: 5, hits: 1000, hitsMax: 1000, store: {}, storeCapacity: 100, ageTime: 5000, powers: {} });
    }),
]);

add('structure-isactive', ['game/structures.js:108'], [
    // RCL ranking: 6 extensions at RCL2 (5 allowed) by distance to the controller (30,30), ties by object order.
    tick(probe(['e1', 'e2', 'e3', 'e4', 'e5', 'e6', 'e7', 'tw', 'lnk', 'obs'].map(id => `G('${id}').isActive()`)), {}, (o) => {
        o.splice(0, o.length, ...o.filter(x => x.type !== 'extension'));
        o.push(D.extension({ _id: 'e1', x: 31, y: 31 }), D.extension({ _id: 'e2', x: 29, y: 29 }), D.extension({ _id: 'e3', x: 35, y: 30 }),
            D.extension({ _id: 'e4', x: 30, y: 35 }), D.extension({ _id: 'e5', x: 25, y: 25 }), D.extension({ _id: 'e6', x: 30, y: 25 }),
            D.extension({ _id: 'e7', x: 32, y: 32, user: 'u2' }),
            D.structure('tower', { _id: 'tw', x: 31, y: 29, user: 'u1', store: { energy: 0 }, storeCapacityResource: { energy: 1000 } }),
            D.structure('link', { _id: 'lnk', x: 31, y: 28, user: 'u1', store: { energy: 0 }, storeCapacityResource: { energy: 800 } }),
            D.structure('observer', { _id: 'obs', x: 31, y: 27, user: 'u1' }));
    }),
]);

add('spawn-spawncreep', ['game/structures.js:1053', 'game/structures.js:1063'], [
    tick(code(function() {
        var s = Game.spawns.Spawn1;
        P('energy', () => [s.room.energyAvailable, s.room.energyCapacityAvailable]);
        var names = [undefined, '', 'Worker', 'New1', 123, 'x'.repeat(100), 'y'.repeat(101), {toString() { return 'Obj'; }}, null, 0, true];
        names.forEach((n, i) => P('name #' + i, () => s.spawnCreep(['work', 'move'], n, {dryRun: true})));
        var opts = [undefined, {}, 5, 'str', null, {dryRun: 1}, {energyStructures: [Game.getObjectById('extF')], dryRun: true},
            {energyStructures: [Game.getObjectById('ext2'), Game.getObjectById('extF'), s, s], dryRun: true},
            {energyStructures: 'abc', dryRun: true}, {energyStructures: [{id: 'extF'}, {id: 'nope'}, {id: 'cWork'}, {id: 'spawnOff'}], dryRun: true},
            {directions: [1, '2', 2], dryRun: true}, {directions: [], dryRun: true}, {directions: [0], dryRun: true},
            {directions: [1.5], dryRun: true}, {directions: 'x', dryRun: true}, {directions: null, dryRun: true}, {directions: [8, 9], dryRun: true},
            {directions: ['3', true], dryRun: true}, [1, 2]];
        opts.forEach((o, i) => P('opts #' + i, () => s.spawnCreep(['work', 'move'], 'Opt' + i, o)));
        var bodies = [[], ['work'], ['work', 'carry', 'move'], ['foo'], new Array(51).fill('move'), new Array(50).fill('tough'),
            'work', ['work', 'work', 'work', 'work'], null, undefined, [undefined], ['claim'], ['WORK'], {0: 'work', length: 1}];
        bodies.forEach((b, i) => P('body #' + i, () => s.spawnCreep(b, 'Body' + i, {dryRun: true})));
        P('foreign', () => Game.getObjectById('spawnBob').spawnCreep(['move'], 'X'));
        P('busy', () => Game.spawns.Busy.spawnCreep(['move'], 'X'));
        P('off', () => Game.spawns.Off.spawnCreep(['move'], 'X'));
        P('memory before', () => Memory.creeps);
        var body = ['work', 'carry', 'move'];
        P('real', () => s.spawnCreep(body, 'Real', {memory: {role: 'h'}, directions: [3, 3, '4'], energyStructures: [Game.getObjectById('extF'), s]}));
        body.push('move');
        P('dup in tick', () => s.spawnCreep(['move'], 'Real'));
        P('second spawn', () => Game.spawns.Five.spawnCreep(['move'], 'Other'));
        P('second spawn same name', () => Game.spawns.Five.spawnCreep(['move'], 'Other', {dryRun: true}));
        P('stub', () => Game.creeps.Real);
        P('stub fields', () => { var c = Game.creeps.Real; return [c.id, c.name, c.spawning, c.my, c.body, c.owner, c.ticksToLive, c.carryCapacity, c.carry, c.store.energy, c.fatigue, c.hits, c.hitsMax, c.saying, c.memory, String(c), c.pos, c.room && c.room.name, c instanceof Creep]; });
        P('stub move', () => Game.creeps.Real.move(1));
        P('memory after', () => Memory.creeps);
        P('same spawn again', () => s.spawnCreep(['move'], 'Again'));
    }), { memory: '{"creeps":{"New1":{"keep":1}}}' }, (o) => o.push(...spawnObjects())),
    tick(code(function() {
        P('memory not object', () => { Memory.creeps = 7; return Game.spawns.Spawn1.spawnCreep(['move'], 'M1'); });
        P('memory after', () => Memory.creeps);
        P('memory undefined', () => { delete Memory.creeps; return Game.spawns.Five.spawnCreep(['move'], 'M2', {memory: 0}); });
        P('memory after 2', () => Memory.creeps);
    }), {}, (o) => o.push(...spawnObjects())),
]);

add('spawn-createcreep', ['game/structures.js:897', 'game/structures.js:933'], [
    tick(code(function() {
        var s = Game.spawns.Spawn1;
        P('can', () => [s.canCreateCreep(['work']), s.canCreateCreep(['work'], 'Worker'), s.canCreateCreep(['work'], 'y'.repeat(101)),
            s.canCreateCreep([]), s.canCreateCreep(['foo']), s.canCreateCreep(new Array(6).fill('work')), s.canCreateCreep(['work'], 5),
            s.canCreateCreep(new Array(50).fill('move')), s.canCreateCreep(new Array(51).fill('move')), s.canCreateCreep('work'),
            Game.spawns.Busy.canCreateCreep(['work']), Game.spawns.Off.canCreateCreep(['work']), Game.getObjectById('spawnBob').canCreateCreep(['work'])]);
        P('create named', () => s.createCreep(['work', 'move'], 'Named', {role: 'a'}));
        P('create dup', () => s.createCreep(['move'], 'Named'));
        P('create mem only', () => Game.spawns.Five.createCreep(['move'], {role: 'b'}));
        P('create random', () => Game.spawns.Five.createCreep(['move']));
        P('create random 2', () => Game.spawns.Five.createCreep(['move'], undefined, {x: 1}));
        P('create no energy', () => Game.spawns.Five.createCreep(new Array(10).fill('work')));
        P('stub', () => Game.creeps.Named);
        P('memory', () => Memory.creeps);
    }), {}, (o) => o.push(...spawnObjects())),
]);

add('spawn-props', ['game/structures.js:851', 'game/structures.js:857', 'game/structures.js:865', 'game/structures.js:893',
    'game/structures.js:1220', 'game/structures.js:1301', 'game/structures.js:1312', 'game/structures.js:1328'], [
    tick(code(function() {
        ['spawn1', 'spawnBusy', 'spawnOff', 'spawnNoDir', 'spawnBob', 'spawn5'].forEach(id => {
            var s = Game.getObjectById(id);
            ['name', 'energy', 'energyCapacity', 'spawning', 'my', 'owner', 'hits', 'structureType', 'memory'].forEach(p => P(id + '.' + p, () => s[p]));
            P(id + '.store', () => [s.store.energy, s.store.getCapacity('energy'), s.store.getFreeCapacity()]);
            P(id + '.toString', () => String(s));
            P(id + '.keys', () => Object.keys(s));
            P(id + '.isActive', () => s.isActive());
        });
        P('Game.spawns', () => Object.keys(Game.spawns));
        P('Spawn alias', () => Spawn === StructureSpawn);
        var sp = Game.spawns.Busy.spawning;
        P('spawning', () => [sp.name, sp.needTime, sp.remainingTime, sp.directions, sp.spawn === Game.spawns.Busy, Object.keys(sp), sp instanceof StructureSpawn.Spawning]);
        P('spawning same', () => Game.spawns.Busy.spawning === sp);
        [[1, 2], ['3', 3], [], [0], [9], [1.5], 'x', [true], undefined, [8, '8', 1]].forEach((d, i) =>
            P('setDirections #' + i, () => sp.setDirections(d)));
        P('cancel', () => sp.cancel());
        P('foreign spawning', () => [Game.getObjectById('spawnBob').spawning.setDirections([1]), Game.getObjectById('spawnBob').spawning.cancel()]);
        P('no dirs', () => Game.getObjectById('spawnNoDir').spawning.directions);
        P('Spawning proto', () => Object.getOwnPropertyNames(StructureSpawn.Spawning.prototype));
        P('Spawn proto', () => Object.getOwnPropertyNames(StructureSpawn.prototype));
        P('mem set', () => { Game.spawns.Spawn1.memory = {a: 1}; return Memory.spawns; });
        P('mem foreign', () => { Game.getObjectById('spawnBob').memory = {}; });
        P('mem str undefined', () => { Memory.spawns = 'undefined'; return Game.spawns.Spawn1.memory; });
        P('mem nonobject', () => { Memory.spawns = 3; return Game.spawns.Spawn1.memory; });
        P('mem nonobject set', () => { Game.spawns.Spawn1.memory = {}; });
        [true, false, 'x', 1].forEach(v => P('notify ' + v, () => Game.spawns.Spawn1.notifyWhenAttacked(v)));
        P('notify foreign', () => Game.getObjectById('spawnBob').notifyWhenAttacked(false));
        P('notify busy', () => Game.spawns.Busy.notifyWhenAttacked(false));
    }), { memory: '{"spawns":{"Spawn1":{"x":1}}}' }, (o) => o.push(...spawnObjects())),
]);

add('spawn-renew-recycle', ['game/structures.js:1237', 'game/structures.js:1269'], [
    tick(probe([
        ...matrix('renewCreep', ['spawn1', 'spawnBusy', 'spawnOff', 'spawnBob', 'spawn5'],
            ["G('cOld')", "G('cFresh')", "G('cClaim')", "G('cBoost')", "G('cBobNear')", "G('cFive')", "G('cBaby')", "G('cWork')", ...JUNK]),
        ...matrix('recycleCreep', ['spawn1', 'spawnBusy', 'spawnOff', 'spawnBob', 'spawn5'],
            ["G('cOld')", "G('cClaim')", "G('cBobNear')", "G('cFive')", "G('cBaby')", "G('cWork')", ...JUNK]),
    ]), {}, (o) => o.push(...spawnObjects())),
    // not enough energy for a renew: room energy below the per-part cost (every W1N1 spawn and extension empty)
    tick(probe(matrix('renewCreep', ['spawn1'], ["G('cOld')"]).concat(['Game.rooms.W1N1.energyAvailable'])), {}, (o) => {
        o.push(...spawnObjects());
        o.forEach(x => { if (x.room === 'W1N1' && (x.type === 'spawn' || x.type === 'extension')) x.store = { energy: 0 }; });
    }),
]);

add('controller', ['game/structures.js:168', 'game/structures.js:174', 'game/structures.js:201', 'game/structures.js:211'], [
    tick(code(function() {
        ['ctrl1', 'ctrl2', 'ctrl3', 'ctrl4', 'ctrl5', 'ctrlSign', 'ctrlHard', 'ctrl8'].forEach(id => {
            var c = Game.getObjectById(id);
            ['ticksToDowngrade', 'reservation', 'level', 'progress', 'progressTotal', 'upgradeBlocked', 'safeMode', 'safeModeCooldown',
                'safeModeAvailable', 'sign', 'isPowerEnabled', 'my', 'owner'].forEach(p => P(id + '.' + p, () => c[p]));
            P(id + '.unclaim', () => c.unclaim());
            P(id + '.keys', () => Object.keys(c));
        });
        P('activate ctrl1', () => Game.getObjectById('ctrl1').activateSafeMode());
        P('activate ctrl5', () => Game.getObjectById('ctrl5').activateSafeMode());
        P('activate ctrl1 again', () => Game.getObjectById('ctrl1').activateSafeMode());
        ['ctrl2', 'ctrl3', 'ctrlSign', 'ctrlHard', 'ctrl8'].forEach(id => P('activate ' + id, () => Game.getObjectById(id).activateSafeMode()));
        P('controller proto', () => Object.getOwnPropertyNames(StructureController.prototype));
    }), { rooms: ALL_ROOMS.concat(['W6N1', 'W0N1', 'W7N1']) }, (o) => {
        o.push(D.controller({ _id: 'ctrlSign', room: 'W6N1', level: 3, safeModeAvailable: 0, sign: { user: 'u2', text: 'mine', time: 900, datetime: 1599999999000 } }),
            D.controller({ _id: 'ctrlHard', room: 'W0N1', level: 4, safeModeCooldown: 1500, hardSign: { text: 'sys', time: 5, datetime: 1500000000000 }, sign: { user: 'u1', text: 'x', time: 1, datetime: 1 }, isPowerEnabled: true }),
            D.controller({ _id: 'ctrl8', room: 'W7N1', level: 8, progress: 5, upgradeBlocked: 1100, downgradeTime: 1500 }));
    }),
    // an own controller already in safe mode blocks activation everywhere
    tick(probe(["G('ctrl1').activateSafeMode()", "G('ctrl5').activateSafeMode()", "G('ctrl1').safeMode"]), {}, (o) => {
        byId(o, 'ctrl5').safeMode = 1200;
    }),
    // ticksToDowngrade below CONTROLLER_DOWNGRADE[level]/2 - threshold
    tick(probe(["G('ctrl1').activateSafeMode()", "G('ctrl1').ticksToDowngrade"]), {}, (o) => {
        byId(o, 'ctrl1').downgradeTime = 3000;
    }),
    tick(probe(["G('ctrl1').activateSafeMode()", "G('ctrl1').ticksToDowngrade"]), {}, (o) => {
        byId(o, 'ctrl1').downgradeTime = 6000;
    }),
]);

add('extension', ['game/structures.js:243', 'game/structures.js:249'], [
    tick(probe(['ext2', 'extF', 'extNoStore', 'extBob'].map(id =>
        `[G('${id}').energy, G('${id}').energyCapacity, G('${id}').store.getUsedCapacity('energy'), G('${id}').store.getCapacity('energy'), G('${id}').my, G('${id}').owner, String(G('${id}'))]`)
        .concat(["Object.getOwnPropertyNames(StructureExtension.prototype)", "Game.rooms.W1N1.energyAvailable"])), {}, (o) => {
        o.push(D.extension({ _id: 'extNoStore', x: 4, y: 3, off: true, store: undefined, storeCapacityResource: undefined }),
            D.extension({ _id: 'extBob', room: 'W2N1', user: 'u2', storeCapacityResource: { energy: 0 } }));
    }),
]);

// W5N1 promoted to RCL8 with one of every structure; W1N1 (RCL2) copies show the inactive paths.
const S = (type, o) => D.structure(type, Object.assign({ room: 'W5N1', user: 'u1' }, o));
const lab = (o) => S('lab', Object.assign({ store: { energy: 2000 }, storeCapacityResource: { energy: 2000 }, storeCapacity: 5000, hits: 500, hitsMax: 500 }, o));
const rcl8 = (o) => {
    byId(o, 'ctrl5').level = 8;
    o.push(
        lab({ _id: 'labA', x: 20, y: 20, store: { energy: 2000, H: 100 } }),
        lab({ _id: 'labB', x: 21, y: 20, store: { energy: 0, O: 100 } }),
        lab({ _id: 'labC', x: 20, y: 21, store: { energy: 50 } }),
        lab({ _id: 'labOH', x: 21, y: 21, store: { energy: 2000, OH: 100 } }),
        lab({ _id: 'labH4', x: 22, y: 21, store: { energy: 0, H: 4 } }),
        lab({ _id: 'labFull', x: 19, y: 21, store: { energy: 0, OH: 2998 } }),
        lab({ _id: 'labCool', x: 19, y: 20, store: { energy: 0 }, cooldownTime: 1005 }),
        lab({ _id: 'labOp', x: 22, y: 20, store: { energy: 0, OH: 2990 }, effects: [{ power: 5, level: 2, endTime: 1100 }] }),
        lab({ _id: 'labFar', x: 30, y: 30, store: { energy: 0, H: 100 } }),
        lab({ _id: 'labUH', x: 20, y: 19, store: { energy: 40, UH: 60 } }),
        lab({ _id: 'labLow', x: 21, y: 19, store: { energy: 10, UH: 60 } }),
        lab({ _id: 'labBob', x: 23, y: 21, user: 'u2', store: { energy: 0, H: 100 } }),
        lab({ _id: 'labW1', room: 'W1N1', x: 3, y: 10 }),
        D.creep({ _id: 'cBoostMe', name: 'BoostMe', room: 'W5N1', x: 21, y: 18, body: ['attack', 'attack', { type: 'attack', hits: 100, boost: 'UH' }, 'move', 'work'] }),
        D.creep({ _id: 'cUnboost', name: 'Unboost', room: 'W5N1', x: 19, y: 19, body: [{ type: 'attack', hits: 100, boost: 'UH' }, 'move'] }),
        D.creep({ _id: 'cBobLab', name: 'BobLab', user: 'u2', room: 'W5N1', x: 22, y: 19, body: [{ type: 'attack', hits: 100, boost: 'UH' }, 'attack'] }),
        S('link', { _id: 'linkA', x: 30, y: 10, store: { energy: 800 }, storeCapacityResource: { energy: 800 } }),
        S('link', { _id: 'linkB', x: 35, y: 15, store: { energy: 0 }, storeCapacityResource: { energy: 800 } }),
        S('link', { _id: 'linkHalf', x: 36, y: 15, store: { energy: 500 }, storeCapacityResource: { energy: 800 } }),
        S('link', { _id: 'linkCool', x: 37, y: 15, store: { energy: 800 }, storeCapacityResource: { energy: 800 }, cooldown: 3 }),
        S('link', { _id: 'linkEmpty', x: 38, y: 15, store: { energy: 0 }, storeCapacityResource: { energy: 800 } }),
        S('link', { _id: 'linkBob', x: 39, y: 15, user: 'u2', store: { energy: 800 }, storeCapacityResource: { energy: 800 } }),
        S('link', { _id: 'linkBobR', x: 40, y: 15, user: 'u2', store: { energy: 800 }, storeCapacityResource: { energy: 800 } }),
        S('rampart', { _id: 'rampOnLink', x: 40, y: 15, user: 'u2', isPublic: true }),
        S('link', { _id: 'linkOther', room: 'W1N1', x: 3, y: 12, store: { energy: 0 }, storeCapacityResource: { energy: 800 } }),
        S('observer', { _id: 'obs', x: 10, y: 10 }),
        S('observer', { _id: 'obsOp', x: 11, y: 10, effects: [{ power: 7, level: 1, endTime: 1100 }] }),
        S('observer', { _id: 'obsBob', x: 12, y: 10, user: 'u2' }),
        S('observer', { _id: 'obsW1', room: 'W1N1', x: 3, y: 14 }),
        S('powerSpawn', { _id: 'ps', x: 15, y: 10, store: { energy: 100, power: 2 }, storeCapacityResource: { energy: 5000, power: 100 } }),
        S('powerSpawn', { _id: 'psLow', x: 16, y: 10, store: { energy: 49, power: 2 }, storeCapacityResource: { energy: 5000, power: 100 } }),
        S('powerSpawn', { _id: 'psOp', x: 17, y: 10, store: { energy: 100, power: 2 }, storeCapacityResource: { energy: 5000, power: 100 }, effects: [{ power: 16, level: 1, endTime: 1100 }] }),
        S('powerSpawn', { _id: 'psEmpty', x: 18, y: 10, store: {}, storeCapacityResource: { energy: 5000, power: 100 } }),
        S('rampart', { _id: 'ramp', x: 5, y: 5, isPublic: false, nextDecayTime: 1050, hits: 300000, hitsMax: 10000000 }),
        S('rampart', { _id: 'rampDecay', x: 6, y: 5, isPublic: true, decayTime: 1060 }),
        S('rampart', { _id: 'rampBob5', x: 7, y: 5, user: 'u2' }),
        S('terminal', { _id: 'term5', x: 25, y: 25, store: { energy: 10000, H: 500 }, storeCapacity: 300000 }),
        S('terminal', { _id: 'termCool', x: 26, y: 25, store: { energy: 10000, H: 500 }, storeCapacity: 300000, cooldownTime: 1003 }),
        S('terminal', { _id: 'termBob', x: 27, y: 25, user: 'u2', store: { energy: 10000 }, storeCapacity: 300000 }),
        S('tower', { _id: 'tower', x: 25, y: 30, store: { energy: 500 }, storeCapacityResource: { energy: 1000 } }),
        S('tower', { _id: 'towerLow', x: 26, y: 30, store: { energy: 9 }, storeCapacityResource: { energy: 1000 } }),
        S('tower', { _id: 'towerBob', x: 27, y: 30, user: 'u2', store: { energy: 500 }, storeCapacityResource: { energy: 1000 } }),
        S('tower', { _id: 'towerW1', room: 'W1N1', x: 3, y: 16, store: { energy: 500 }, storeCapacityResource: { energy: 1000 } }),
        S('nuker', { _id: 'nuke5', x: 30, y: 35, store: { energy: 300000, G: 5000 }, storeCapacityResource: { energy: 300000, G: 5000 } }),
        S('nuker', { _id: 'nukeCool', x: 31, y: 35, store: { energy: 300000, G: 5000 }, storeCapacityResource: { energy: 300000, G: 5000 }, cooldownTime: 1500 }),
        S('nuker', { _id: 'nukeLow', x: 32, y: 35, store: { energy: 300000, G: 4999 }, storeCapacityResource: { energy: 300000, G: 5000 } }),
        S('factory', { _id: 'fac', x: 35, y: 35, level: 1, store: { U: 1000, energy: 1000, utrium_bar: 20, zynthium_bar: 20 }, storeCapacity: 50000, effects: [{ power: 19, level: 1, endTime: 1100 }] }),
        S('factory', { _id: 'facCool', x: 36, y: 35, store: { U: 1000, energy: 1000 }, storeCapacity: 50000, cooldownTime: 1010 }),
        S('factory', { _id: 'facFull', x: 37, y: 35, store: { U: 1000, energy: 49000 }, storeCapacity: 50000 }),
        S('factory', { _id: 'facNoOp', x: 38, y: 35, level: 1, store: { utrium_bar: 20, zynthium_bar: 20, energy: 20 }, storeCapacity: 50000 }),
        S('factory', { _id: 'facNoCap', x: 39, y: 35, store: { U: 1000, energy: 1000 } }),
        S('container', { _id: 'cont5', x: 40, y: 35, user: undefined, store: { energy: 10 }, storeCapacity: 2000, decayTime: 1100 }),
        S('storage', { _id: 'stor5', x: 41, y: 35, store: { energy: 10 }, storeCapacity: 1000000 }),
        S('constructedWall', { _id: 'wall5', x: 42, y: 35, user: undefined, ticksToLive: 77 }),
        S('road', { _id: 'road5', x: 43, y: 35, user: undefined, decayTime: 1003 }),
        S('portal', { _id: 'portal5', x: 44, y: 35, user: undefined, destination: { x: 10, y: 11, room: 'W1N1' }, decayTime: 2000 }),
        S('portal', { _id: 'portalShard', x: 45, y: 35, user: undefined, destination: { shard: 'shard1', room: 'W1N1' } }),
        S('keeperLair', { _id: 'lair', x: 46, y: 35, user: '3', nextSpawnTime: 1020 }),
        S('keeperLair', { _id: 'lairIdle', x: 47, y: 35, user: '3' }),
        S('powerBank', { _id: 'pbank5', x: 46, y: 37, user: undefined, store: { power: 3000 }, decayTime: 5000 }),
        S('invaderCore', { _id: 'core', x: 46, y: 39, user: '2', level: 2, deployTime: 1100, spawning: { name: 'Inv', needTime: 5, spawnTime: 1002 } }),
        S('invaderCore', { _id: 'coreIdle', x: 47, y: 39, user: '2', level: 0 }),
        S('extractor', { _id: 'xCool5', x: 40, y: 39, cooldown: 3 }),
    );
};

add('structure-lab', ['game/structures.js:298', 'game/structures.js:307', 'game/structures.js:317', 'game/structures.js:360',
    'game/structures.js:412', 'game/structures.js:443'], [
    tick(probe([
        ...['labA', 'labB', 'labC', 'labOH', 'labCool', 'labOp', 'labUH', 'labW1'].map(id =>
            `[G('${id}').energy, G('${id}').energyCapacity, G('${id}').cooldown, G('${id}').mineralAmount, G('${id}').mineralCapacity, G('${id}').mineralType, G('${id}').store.getUsedCapacity()]`),
        ...matrix('runReaction', ['labC', 'labOH', 'labCool', 'labFull', 'labOp', 'labBob', 'labW1', 'labFar'],
            ["G('labA'), G('labB')", "G('labB'), G('labA')", "G('labA'), G('labA')", "G('labA'), G('labH4')", "G('labA'), G('labFar')",
                "G('labA'), G('labBob')", "G('labC'), G('labB')", "G('labA'), G('tower')", "G('labA'), undefined", "undefined, G('labB')",
                "{id: 'abcdefabcdefabcdefabcdef'}, G('labB')", "G('labA'), {id: 'labB'}"]),
        ...matrix('reverseReaction', ['labOH', 'labC', 'labCool', 'labOp', 'labBob', 'labW1', 'labA'],
            ["G('labC'), G('labB')", "G('labA'), G('labB')", "G('labB'), G('labA')", "G('labC'), G('labC')", "G('labFull'), G('labC')",
                "G('labC'), G('labFar')", "G('labC'), G('tower')", "undefined, G('labC')"]),
        ...matrix('boostCreep', ['labUH', 'labLow', 'labA', 'labBob', 'labW1', 'labCool'],
            ["G('cBoostMe')", "G('cBoostMe'), 1", "G('cBoostMe'), 2", "G('cBoostMe'), 3", "G('cBoostMe'), '2'", "G('cUnboost')", "G('cBobLab')",
                "G('cFive')", "G('cWork')", ...JUNK]),
        ...matrix('unboostCreep', ['labUH', 'labA', 'labCool', 'labBob', 'labW1', 'labLow'],
            ["G('cUnboost')", "G('cBoostMe')", "G('cBobLab')", "G('cWork')", ...JUNK]),
        "Object.getOwnPropertyNames(StructureLab.prototype)",
    ]), {}, rcl8),
    // a lab in a room whose controller is gone: data(this.room.controller.id) throws
    tick(probe(["G('labA').runReaction(G('labB'), G('labC'))", "G('labA').boostCreep(G('cBoostMe'))", "G('labA').reverseReaction(G('labB'), G('labC'))",
        "G('labA').unboostCreep(G('cUnboost'))"]), {}, (o) => { rcl8(o); o.splice(o.indexOf(byId(o, 'ctrl5')), 1); }),
]);

add('structure-link-observer', ['game/structures.js:475', 'game/structures.js:481', 'game/structures.js:488', 'game/structures.js:542',
    'game/structures.js:548'], [
    tick(probe([
        ...['linkA', 'linkB', 'linkCool', 'linkBob', 'linkOther'].map(id => `[G('${id}').energy, G('${id}').energyCapacity, G('${id}').cooldown, G('${id}').store.energy]`),
        ...matrix('transferEnergy', ['linkA', 'linkHalf', 'linkCool', 'linkEmpty', 'linkBob', 'linkBobR', 'linkOther'],
            ["G('linkB')", "G('linkB'), 100", "G('linkB'), 801", "G('linkB'), -1", "G('linkB'), 0", "G('linkB'), '300'", "G('linkHalf')", "G('linkHalf'), 400",
                "G('linkA')", "G('linkBob')", "G('linkOther')", "G('tower')", ...JUNK]),
        ...matrix('observeRoom', ['obs', 'obsOp', 'obsBob', 'obsW1'],
            ["'W5N1'", "'W15N1'", "'W16N1'", "'W5N11'", "'W5S10'", "'E4N1'", "'E5N1'", "'w5n1'", "'W5N1x'", "5", "undefined", "'E127S127'", "'W0N0'"]),
        "Object.getOwnPropertyNames(StructureObserver.prototype)",
    ]), {}, rcl8),
]);

add('structure-misc', ['game/structures.js:149', 'game/structures.js:155', 'game/structures.js:262', 'game/structures.js:268',
    'game/structures.js:279', 'game/structures.js:285', 'game/structures.js:579', 'game/structures.js:585', 'game/structures.js:599',
    'game/structures.js:605', 'game/structures.js:613', 'game/structures.js:640', 'game/structures.js:646', 'game/structures.js:651',
    'game/structures.js:666', 'game/structures.js:672', 'game/structures.js:684', 'game/structures.js:690', 'game/structures.js:833',
    'game/structures.js:839', 'game/structures.js:1398', 'game/structures.js:1404', 'game/structures.js:1474', 'game/structures.js:1480',
    'game/structures.js:1486'], [
    tick(probe([
        "[G('cont5').store.energy, G('cont5').storeCapacity, G('cont5').ticksToDecay, G('cont').ticksToDecay, G('cont5').my, G('cont5').owner]",
        "[G('xCool5').cooldown, G('xOk').cooldown]",
        "[G('lair').my, G('lair').owner, G('lair').ticksToSpawn, G('lairIdle').ticksToSpawn, String(G('lair'))]",
        "[G('pbank5').power, G('pbank5').ticksToDecay, G('pbank5').my, G('pbank5').owner, G('pbank').ticksToDecay]",
        ...['ps', 'psLow', 'psOp', 'psEmpty'].map(id => `[G('${id}').energy, G('${id}').energyCapacity, G('${id}').power, G('${id}').powerCapacity, G('${id}').store.power, G('${id}').processPower()]`),
        "G('ps').processPower()",
        "[G('ramp').ticksToDecay, G('rampDecay').ticksToDecay, G('ramp').isPublic, G('rampDecay').isPublic, G('rampBob5').isPublic]",
        ...matrix('setPublic', ['ramp', 'rampBob5'], ['true', 'false', "'x'", '0', 'undefined']),
        "[G('road5').ticksToDecay, G('road1').ticksToDecay, G('stor5').store.energy, G('stor5').storeCapacity, G('wall5').ticksToLive, G('wall1').ticksToLive]",
        "[G('portal5').destination, G('portal5').ticksToDecay, G('portalShard').destination, G('portalShard').ticksToDecay, G('portal5').my]",
        "[G('core').level, G('core').spawning, G('core').ticksToDeploy, String(G('core')), G('core').my, G('core').owner, G('coreIdle').spawning, G('coreIdle').ticksToDeploy]",
        "[G('core').spawning.name, G('core').spawning.remainingTime, G('core').spawning.spawn === G('core'), G('core').spawning.cancel(), G('core').spawning.setDirections([1])]",
        ...['Container', 'Extractor', 'KeeperLair', 'PowerBank', 'PowerSpawn', 'Rampart', 'Road', 'Storage', 'Wall', 'Portal', 'InvaderCore']
            .map(n => `Object.getOwnPropertyNames(Structure${n}.prototype)`),
        "Object.keys(Game.structures)",
    ]), {}, rcl8),
    // powerSpawn and others in a room below their RCL
    tick(probe(["G('ps').processPower()", "G('ps').isActive()"]), {}, (o) => { rcl8(o); byId(o, 'ctrl5').level = 7; }),
]);

add('structure-terminal-tower', ['game/structures.js:702', 'game/structures.js:708', 'game/structures.js:714', 'game/structures.js:754',
    'game/structures.js:760', 'game/structures.js:766', 'game/structures.js:786', 'game/structures.js:806'], [
    tick(probe([
        "[G('term5').store.energy, G('term5').storeCapacity, G('term5').cooldown, G('termCool').cooldown]",
        ...matrix('send', ['term5', 'termCool', 'termBob', 'term'], [''],
            ["'H', 100, 'W1N1'", "'H', 100, 'W1N1', 'desc'", "'H', 100, 'E10S10'", "'energy', 1000, 'W3N1'", "'energy', 9000, 'W3N1'",
                "'energy', 9999, 'W30N1'", "'H', 501, 'W1N1'", "'H', 100, 'w1n1'", "'H', 100, 'W1N1 '", "'foo', 1, 'W1N1'", "'H', 0, 'W1N1'",
                "'H', -5, 'W1N1'", "'H', 100, 'W1N1', 'x'.repeat(101)", "'H', 100, 'W1N1', 5", "'H', 100, 'W1N1', ''", "'H', '100', 'W1N1'",
                "'H', 1e9, 'W1N1'", "'energy', 100, 'W5N1'", "'H', undefined, 'W1N1'", "'H', 100, 'E127S127'"]),
        "[G('tower').energy, G('tower').energyCapacity, G('towerLow').store.energy]",
        ...['attack', 'heal', 'repair'].map(m => matrix(m, ['tower', 'towerLow', 'towerBob', 'towerW1'],
            ["G('cFive')", "G('cBob')", "G('pcMine5')", "G('ps')", "G('spawn5')", "G('wall5')", "G('road5')", "G('ctrl5')", "G('src1')",
                "G('cBaby')", "G('siteRoad')", ...JUNK])).reduce((a, b) => a.concat(b), []),
        "Object.getOwnPropertyNames(StructureTerminal.prototype)",
        "Object.getOwnPropertyNames(StructureTower.prototype)",
    ]), {}, (o) => {
        rcl8(o);
        o.push(D.creep({ _id: 'cFive', name: 'Fiver', room: 'W5N1', x: 21, y: 20, ageTime: 1100 }),
            { _id: 'pcMine5', type: 'powerCreep', user: 'u1', name: 'Pc5', className: 'operator', level: 1, room: 'W5N1', x: 11, y: 21, hits: 1000, hitsMax: 1000, store: {}, storeCapacity: 100, ageTime: 5000, powers: {} },
            D.spawn({ _id: 'spawn5', name: 'Five', room: 'W5N1' }));
    }),
]);

add('structure-nuker', ['game/structures.js:1341', 'game/structures.js:1347', 'game/structures.js:1356'], [
    tick(probe([
        "[G('nuke5').energy, G('nuke5').energyCapacity, G('nuke5').ghodium, G('nuke5').ghodiumCapacity, G('nuke5').cooldown, G('nukeCool').cooldown, G('nuker').ghodium]",
        ...matrix('launchNuke', ['nuke5', 'nukeCool', 'nukeLow', 'nuker'],
            ["new RoomPosition(25, 25, 'W1N1')", "new RoomPosition(25, 25, 'W15N1')", "new RoomPosition(25, 25, 'W16N1')", "new RoomPosition(25, 25, 'W2N1')",
                "new RoomPosition(25, 25, 'W3N1')", "{x: 25, y: 25, roomName: 'W1N1'}", "undefined", "G('cWork')", "G('cWork').pos"]),
        "G('nuker').launchNuke(new RoomPosition(1, 1, 'W5N1'))",
        "Object.getOwnPropertyNames(StructureNuker.prototype)",
        // J3 (roomStatus native): a closed room is not blocked; status read per map, null timestamps still count;
        // a roomName that is an inherited key of the parsed vanilla map (Object.prototype) blocks
        "G('nuke5').launchNuke(new RoomPosition(25, 25, 'W4N1'))",
        "G('nuke5').launchNuke(new RoomPosition(25, 25, 'W6N1'))",
        "G('nuke5').launchNuke(new RoomPosition(25, 25, 'W7N1'))",
        ...['constructor', 'toString', '__proto__', 'W2N1x'].map(n =>
            `G('nuke5').launchNuke(Object.defineProperty(new RoomPosition(25, 25, 'W1N1'), 'roomName', {value: '${n}'}))`),
        "G('nuke5').launchNuke(Object.defineProperty(new RoomPosition(25, 25, 'W1N1'), 'roomName', {value: ['W2N1']}))",
    ]), { roomStatusData: { novice: { W2N1: 1700000000000, W7N1: null }, respawn: { W3N1: 1700000000000 }, closed: { W4N1: 1700000000000, W6N1: null } } }, rcl8),
    tick(probe(["G('nuke5').launchNuke(new RoomPosition(25, 25, 'W1N1'))"]), { roomDocs: { W5N1: { novice: 1600000000001 } } }, rcl8),
    tick(probe(["G('nuke5').launchNuke(new RoomPosition(25, 25, 'W1N1'))"]), { roomDocs: { W5N1: { respawnArea: 1600000000001 } } }, rcl8),
    tick(probe(["G('nuke5').launchNuke(new RoomPosition(25, 25, 'W1N1'))"]), { roomDocs: { W5N1: { novice: 1600000000000, respawnArea: 1 } } }, rcl8),
    // Not cased: a runner holding the initial `{}` roomStatusData (TypeError). chitin never exposes that state
    // (spec 05 RUNTIME-ROOM-STATUS-DATA, BUG-STATUSDATA-COLD = F).
]);

add('structure-factory', ['game/structures.js:1421', 'game/structures.js:1427', 'game/structures.js:1434'], [
    tick(probe([
        "[G('fac').level, G('fac').store.U, G('fac').storeCapacity, G('fac').cooldown, G('facCool').cooldown, G('facCool').level]",
        ...matrix('produce', ['fac', 'facCool', 'facFull', 'facNoOp', 'facNoCap'],
            ["'utrium_bar'", "'composite'", "'U'", "'energy'", "'foo'", "undefined", "'battery'", "'crystal'"]),
        "Object.getOwnPropertyNames(StructureFactory.prototype)",
    ]), {}, (o) => { rcl8(o); o.push(S('factory', { _id: 'facBob', x: 1, y: 1, user: 'u2', store: {}, storeCapacity: 50000 })); }),
    tick(probe(["G('fac').produce('utrium_bar')", "G('facNoOp').produce('composite')"]), {}, (o) => { rcl8(o); byId(o, 'ctrl5').level = 6; }),
    tick(probe(["G('fac').produce('composite')"]), { time: 1101 }, rcl8),
]);

// Return-code rows the cases above never reach (each found by a return-code mutation sweep of structures.js).
add('structure-rows', ['game/structures.js:89', 'game/structures.js:108', 'game/structures.js:360', 'game/structures.js:412',
    'game/structures.js:488', 'game/structures.js:613', 'game/structures.js:714', 'game/structures.js:1356', 'game/structures.js:1434'], [
    tick(probe([
        'Structure.prototype.destroy.call({})', 'Structure.prototype.notifyWhenAttacked.call({}, true)',   // no room
        "G('ruin1').structure.destroy()", "G('ruin1').structure.notifyWhenAttacked(false)",
        "G('labOH').reverseReaction(G('labH4'), G('labC'))",        // output lab would overflow
        "G('labUH').boostCreep(G('cBoostMe'))",                     // energy enough, mineral short
        "G('linkN1').transferEnergy(G('linkN2'))",                  // room without a controller
        "G('psBob5').processPower()", "G('nukeBob').launchNuke(new RoomPosition(25, 25, 'W1N1'))", "G('facBob5').produce('utrium_bar')",
        "G('term5').send('energy', 9990, 'W3N1')",                  // energy amount + cost above the store
    ]), { rooms: ALL_ROOMS.concat(['W6N1']) }, (o) => {
        rcl8(o);
        byId(o, 'labUH').store = { energy: 100, UH: 10 };   // patched, not added: an 11th W5N1 lab would be inactive
        byId(o, 'labH4').store = { energy: 0, H: 2998 };
        o.push(
            S('link', { _id: 'linkN1', room: 'W6N1', x: 10, y: 10, store: { energy: 800 }, storeCapacityResource: { energy: 800 } }),
            S('link', { _id: 'linkN2', room: 'W6N1', x: 20, y: 10, store: { energy: 0 }, storeCapacityResource: { energy: 800 } }),
            S('powerSpawn', { _id: 'psBob5', user: 'u2', x: 18, y: 12, store: { energy: 100, power: 2 }, storeCapacityResource: { energy: 5000, power: 100 } }),
            S('nuker', { _id: 'nukeBob', user: 'u2', x: 33, y: 35, store: { energy: 300000, G: 5000 }, storeCapacityResource: { energy: 300000, G: 5000 } }),
            S('factory', { _id: 'facBob5', user: 'u2', x: 34, y: 37, store: { U: 1000, energy: 1000 }, storeCapacity: 50000 }));
    }),
]);

module.exports = cases;
