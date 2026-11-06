'use strict';
// Creep (game/creeps.js) cases. Loop bodies run inside the player sandbox; `P(label, fn)` records the result
// (or the thrown error) in the journal next to every intents call.
const { D } = require('../world');
const { probe, code, matrix, JUNK } = require('./_lib');
const { ROOMS, tick, byId } = require('./_world');

const cases = [];
const add = (name, tags, ticks) => cases.push({ name, tags, rooms: ROOMS, ticks });

add('creep-props', ['game/creeps.js:60', 'game/creeps.js:71', 'game/creeps.js:95', 'game/creeps.js:122', 'game/creeps.js:589'], [
    tick(code(function() {
        var names = ['Worker', 'Empty', 'Baby', 'Hurt', 'Loud', 'Stuck'];
        P('creep names', () => Object.keys(Game.creeps));
        names.forEach(n => {
            var c = Game.creeps[n];
            ['id', 'name', 'body', 'my', 'owner', 'spawning', 'ticksToLive', 'carryCapacity', 'fatigue',
                'hits', 'hitsMax', 'saying', 'memory'].forEach(p => P(n + '.' + p, () => c[p]));
            P(n + '.store', () => [c.store.energy, c.store.getCapacity(), c.store.getFreeCapacity(), c.store.getUsedCapacity()]);
            P(n + '.carry', () => [c.carry === c.store, c.carry.energy, Object.keys(c.carry)]);
            P(n + '.toString', () => String(c));
            P(n + '.pos', () => [c.pos.x, c.pos.y, c.pos.roomName, c.room.name, c.effects]);
            P(n + '.keys', () => Object.keys(c));
            P(n + '.json', () => JSON.stringify(c));
            P(n + '.active', () => ['move', 'work', 'carry', 'attack', 'claim', undefined].map(t => c.getActiveBodyparts(t)));
        });
        ['cBob', 'cBobPub'].forEach(id => {
            var c = Game.getObjectById(id);
            ['name', 'my', 'owner', 'saying', 'memory', 'ticksToLive'].forEach(p => P(id + '.' + p, () => c[p]));
            P(id + '.setmem', () => { c.memory = {}; });
        });
        P('proto', () => Object.getOwnPropertyNames(Creep.prototype));
        P('proto desc', () => Object.getOwnPropertyNames(Creep.prototype).map(k => {
            var d = Object.getOwnPropertyDescriptor(Creep.prototype, k);
            return k + ':' + (d.get ? 'g' : '') + (d.set ? 's' : '') + (d.enumerable ? 'e' : '') + (d.configurable ? 'c' : '') + (d.writable ? 'w' : '') + (typeof d.value === 'function' ? d.value.length + d.value.name : '');
        }));
        P('ctor', () => [Creep.name, Creep.length, Object.getPrototypeOf(Creep.prototype) === RoomObject.prototype, Creep.prototype.constructor === Creep]);
        P('globals desc', () => { var d = Object.getOwnPropertyDescriptor(global, 'Creep'); return [d.enumerable, d.writable, d.configurable]; });
        P('empty creep', () => { var c = new Creep(); return [c.id, Object.keys(c)]; });
        P('empty creep name', () => new Creep().name);
        P('empty creep memory', () => new Creep().memory);
        P('missing creep', () => new Creep('nope'));
        var w = Game.creeps.Worker;
        P('cache poison', () => { w._hits = 5; return w.hits; });
        P('mem set', () => { w.memory = 7; return Memory.creeps.Worker; });
        P('mem empty', () => Game.creeps.Empty.memory);
        P('mem str undefined', () => { Memory.creeps = 'undefined'; return w.memory; });
        P('mem nonobject get', () => { Memory.creeps = 5; return w.memory; });
        P('mem nonobject set', () => { w.memory = {}; });
        P('mem undefined set', () => { delete Memory.creeps; w.memory = {a: 1}; return Memory.creeps; });
    }), { memory: '{"creeps":{"Worker":{"role":"w"},"Empty":0}}' }),
]);

// Stale wrappers kept in a global across ticks: cached getters stay, unread ones read the new tick.
const stale = code(function() {
    if (Game.time == 1000) {
        global.kept = Game.creeps.Worker;
        P('t0 hits', () => global.kept.hits);
        P('t0 move', () => global.kept.move(1));
    }
    else if (Game.time == 1001) {
        P('stale hits', () => global.kept.hits);
        P('stale fatigue', () => global.kept.fatigue);
        P('stale store', () => global.kept.store.energy);
        P('stale pos', () => [global.kept.pos.x, global.kept.pos.y]);
        P('stale move', () => global.kept.move(1));
        P('stale ttl', () => global.kept.ticksToLive);
        P('fresh is other', () => Game.creeps.Worker !== global.kept);
    }
    else {
        P('gone creep getter', () => global.kept.name);
        P('gone creep move', () => global.kept.move(1));
    }
});
add('creep-stale', ['game/creeps.js:50'], [
    tick(stale, { time: 1000 }),
    tick(stale, { time: 1001 }, (o) => { const w = byId(o, 'cWork'); w.hits = 200; w.fatigue = 2; w.x = 11; }),
    tick(stale, { time: 1002 }, (o) => { o.splice(o.indexOf(byId(o, 'cWork')), 1); }),
]);

add('creep-move', ['game/creeps.js:126', 'game/creeps.js:1016'], [
    tick(code(function() {
        var w = Game.creeps.Worker, e = Game.creeps.Empty;
        [1, 8, 0, 9, -1, '3', 1.5, true, [3], NaN, undefined, null, {}, '8', 2.9, 'x', 8.5, ' 4 ', '0x2', 1e-9].forEach(d =>
            P('move ' + String(d) + ' ' + typeof d, () => e.move(d)));
        P('move creep adjacent', () => w.move(e));
        P('move creep far', () => w.move(Game.creeps.Up));
        P('move creep foreign', () => w.move(Game.getObjectById('cBob')));
        P('move spawning creep', () => w.move(Game.creeps.Baby));
        P('move self', () => w.move(w));
        ['Baby', 'Tired', 'Stuck', 'Hurt'].forEach(n => {
            P(n + ' move', () => Game.creeps[n].move(3));
            P(n + ' move creep', () => Game.creeps[n].move(w));
        });
        P('foreign move', () => Game.getObjectById('cBob').move(1));
        P('move spawn obj', () => e.move(Game.spawns.Spawn1));
        P('move plain creep-like', () => e.move({id: 'cWork', pos: w.pos}));
        P('mutated body', () => { e.body.length = 0; return e.move(2); });
        P('getActive', () => [w.getActiveBodyparts('work'), Game.creeps.Hurt.getActiveBodyparts('carry'), Game.creeps.Hurt.getActiveBodyparts('work'), Game.creeps.Hurt.getActiveBodyparts('move')]);
        P('cancel', () => [e.cancelOrder('move'), e.cancelOrder('move'), w.cancelOrder('nothing'), w.cancelOrder()]);
        P('foreign cancel', () => Game.getObjectById('cBob').cancelOrder('move'));
    })),
]);

add('creep-harvest', ['game/creeps.js:335'], [
    tick(probe(matrix('harvest',
        ['cWork', 'cEmpty', 'cBob', 'cBaby', 'cCarrier', 'cHurt', 'cMiner', 'cMiner5', 'cRaid2', 'cRaid3', 'cRaid4'],
        ["G('srcE')", "G('src1')", "G('srcFar')", "G('mNone')", "G('mForeign')", "G('mInactive')", "G('mEmpty')",
            "G('dep')", "G('depCool')", "G('mUnowned')", "G('mOk')", "G('mCool')", "G('src2')", "G('src3')", "G('src4')",
            "G('ext2')", "G('cEmpty')", "G('drop1')", "G('ctrl1')", ...JUNK]))),
]);

add('creep-transfer', ['game/creeps.js:428'], [
    tick(probe([
        ...matrix('transfer', ['cWork', 'cEmpty', 'cBob', 'cBaby', 'cUp', 'cUpFar', 'cCarrier'],
            ["G('ext2')", "G('extF')", "G('spawn1')", "G('cEmpty')", "G('cWork')", "G('cBaby')", "G('cont')", "G('tomb')",
                "G('drop1')", "G('src1')", "G('ctrl1')", "G('cBob')", "G('siteRoad')", "G('road1')", ...JUNK], [", 'energy'"]),
        ...matrix('transfer', ['cWork'], ["G('ext2')", "G('cEmpty')", "G('cont')"],
            ["", ", 'H'", ", 'foo'", ", 'energy', 0", ", 'energy', 10", ", 'energy', 50", ", 'energy', 51", ", 'energy', -1",
                ", 'energy', '10'", ", 'energy', 0.5", ", 'energy', NaN", ", 'energy', '-1'", ", 'energy', null", ", 'energy', [20]"]),
        "G('cWork').transfer(G('ctrl1'), 'H')",
        "G('cUp').transfer(G('ctrl1'), 'energy', 1)",
        "G('cRaid2').transfer(G('ctrl2'), 'energy')",
        "G('cRaid4').transfer(G('ctrl4'), 'energy')",
    ]), {}, (o) => { byId(o, 'cont').store = { energy: 1990, H: 10 }; }),
]);

add('creep-withdraw', ['game/creeps.js:493'], [
    tick(probe([
        ...matrix('withdraw', ['cLoot', 'cWork', 'cEmpty', 'cBob', 'cBaby', 'cRaid2'],
            ["G('stor')", "G('term')", "G('termOld')", "G('storBob')", "G('contPub')", "G('nuker')", "G('pbank')", "G('ruin1')",
                "G('tombBob')", "G('storEmpty')", "G('cont')", "G('tomb')", "G('extF')", "G('ext2')", "G('spawn1')", "G('drop1')",
                "G('src1')", "G('ctrl1')", "G('cEmpty')", "G('cont2')", ...JUNK], [", 'energy'"]),
        ...matrix('withdraw', ['cLoot'], ["G('stor')", "G('tombBob')", "G('ruin1')"],
            ["", ", 'H'", ", 'G'", ", 'foo'", ", 'energy', 0", ", 'energy', 10", ", 'energy', 90", ", 'energy', 91", ", 'energy', -1",
                ", 'H', 60", ", 'energy', '5'", ", 'energy', NaN", ", 'power'"]),
        ...matrix('withdraw', ['cEmpty'], ["G('cont')", "G('tomb')"], [", 'H'", ", 'energy'", ", 'energy', 100", ", 'energy', 101", ", 'G'"]),
    ])),
]);

add('creep-pickup-drop', ['game/creeps.js:404', 'game/creeps.js:566'], [
    tick(probe([
        ...matrix('pickup', ['cWork', 'cEmpty', 'cBob', 'cBaby', 'cCarrier', 'cLoot'],
            ["G('drop1')", "G('dropFar')", "G('src1')", "G('tomb')", "G('cont')", ...JUNK]),
        ...matrix('drop', ['cWork', 'cEmpty', 'cBob', 'cBaby', 'cLoot', 'cRaid2'], [''],
            ["'energy'", "'energy', 10", "'energy', 50", "'energy', 51", "'energy', 0", "'energy', '20'", "'energy', -5",
                "'H'", "'G'", "'G', 1000", "'foo', 1", "undefined", "'energy', NaN", "'energy', 0.5", "'energy', true"]),
    ])),
]);

add('creep-upgrade', ['game/creeps.js:922'], [
    tick(probe(matrix('upgradeController', ['cUp', 'cUpFar', 'cWork', 'cEmpty', 'cBob', 'cBaby', 'cCarrier', 'cStuck', 'cRaid2', 'cRaid3', 'cRaid4'],
        ["G('ctrl1')", "G('ctrl2')", "G('ctrl3')", "G('ctrl4')", "G('src1')", "G('spawn1')", ...JUNK]))),
    tick(probe(["G('cUp').upgradeController(G('ctrl1'))", "G('cUp').transfer(G('ctrl1'), 'energy')"]), { user: { shardAccess: false } }),
    tick(probe(["G('cUp').upgradeController(G('ctrl1'))", "G('ctrl1').upgradeBlocked"]), {}, (o) => { byId(o, 'ctrl1').upgradeBlocked = 1050; }),
    tick(probe(["G('cUp').upgradeController(G('ctrl1'))", "G('ctrl1').upgradeBlocked"]), {}, (o) => { byId(o, 'ctrl1').upgradeBlocked = 1000; }),
    tick(probe(["G('cUp').upgradeController(G('ctrl1'))"]), {}, (o) => { byId(o, 'ctrl1').level = 0; }),
]);

add('creep-build-repair', ['game/creeps.js:734', 'game/creeps.js:762'], [
    tick(probe([
        ...matrix('build', ['cWork', 'cEmpty', 'cBob', 'cBaby', 'cCarrier', 'cMine', 'cUp'],
            ["G('siteRoad')", "G('siteFar')", "G('siteExt')", "G('siteBlocked')", "G('siteMine')", "G('siteHostile')",
                "G('siteOnWall')", "G('road1')", "G('ext2')", ...JUNK]),
        ...matrix('repair', ['cWork', 'cEmpty', 'cBob', 'cBaby', 'cCarrier', 'cUp'],
            ["G('road1')", "G('roadFar')", "G('extF')", "G('spawn1')", "G('ctrl1')", "G('cont')", "G('wall1')", "G('src1')",
                "G('cEmpty')", "G('siteRoad')", "G('drop1')", ...JUNK]),
    ])),
    // own safe mode: only own creeps block an obstacle site
    tick(probe(matrix('build', ['cWork'], ["G('siteBlocked')", "G('siteMine')", "G('siteOnWall')", "G('siteExt')"])),
        {}, (o) => { byId(o, 'ctrl1').safeMode = 1200; }),
    // non-obstacle site under a creep: never blocked
    tick(probe(matrix('build', ['cWork'], ["G('siteBlocked')"])), {}, (o) => { byId(o, 'siteBlocked').structureType = 'container'; }),
]);

add('creep-say-suicide', ['game/creeps.js:813', 'game/creeps.js:826'], [
    tick(code(function() {
        var msgs = ['hi', 123, undefined, null, {}, 'a very long message over ten chars', ['a', 'b'], true, NaN];
        msgs.forEach((m, i) => [undefined, true, 0, 'yes'].forEach(p => P('say ' + i + ' ' + p, () => Game.creeps.Worker.say(m, p))));
        P('say sym', () => Game.creeps.Worker.say(Symbol('x')));
        P('say obj toString', () => Game.creeps.Empty.say({toString() { return 'custom'; }}));
        P('say foreign', () => Game.getObjectById('cBob').say('x'));
        P('say spawning', () => Game.creeps.Baby.say('x'));
        P('suicide', () => Game.creeps.Worker.suicide());
        P('suicide again', () => Game.creeps.Worker.suicide());
        P('suicide foreign', () => Game.getObjectById('cBob').suicide());
        P('suicide spawning', () => Game.creeps.Baby.suicide());
        P('cancel say', () => Game.creeps.Empty.cancelOrder('say'));
    })),
]);

// Combat / controller methods. "Fight" (12,20) next to own creeps, a fortified rampart and an invulnerable wall.
const fighters = () => [
    D.creep({ _id: 'cFight', name: 'Fight', x: 12, y: 20, body: ['tough', 'attack', 'ranged_attack', 'heal', 'work', 'claim', 'move'], hits: 700, hitsMax: 700, store: { energy: 0, G: 999 } }),
    D.structure('rampart', { _id: 'rampFort', x: 13, y: 20, user: 'u2', effects: [{ power: 17, level: 1, endTime: 1100 }] }),
    D.structure('constructedWall', { _id: 'wallInv', x: 13, y: 21, effects: [{ effect: 1001, endTime: 1100 }] }),
    D.structure('constructedWall', { _id: 'wallOld', x: 13, y: 19, effects: [{ effect: 1001, endTime: 1000 }] }),
    D.creep({ _id: 'cBobFar', name: 'BobFar', user: 'u2', x: 15, y: 20 }),
    { _id: 'pcMine', type: 'powerCreep', user: 'u1', name: 'PcMine', className: 'operator', level: 1, room: 'W1N1', x: 11, y: 21, hits: 1000, hitsMax: 1000, store: {}, storeCapacity: 100, ageTime: 5000, powers: {} },
];
const COMBAT_TARGETS = ["G('cEmpty')", "G('cBob')", "G('cBobFar')", "G('cFight')", "G('cBaby')", "G('rampFort')", "G('wallInv')", "G('wallOld')",
    "G('extF')", "G('spawn1')", "G('ctrl1')", "G('src1')", "G('siteRoad')", "G('pcMine')", "G('PcMine')", "G('cBob2')", "G('ctrl2')", "G('cont2')", ...JUNK];
add('creep-combat', ['game/creeps.js:593', 'game/creeps.js:626', 'game/creeps.js:658', 'game/creeps.js:678', 'game/creeps.js:706',
    'game/creeps.js:1024', 'game/creeps.js:1101', 'game/creeps.js:996'], [
    tick(probe([
        ...['attack', 'rangedAttack', 'heal', 'rangedHeal', 'dismantle', 'pull'].map(m =>
            matrix(m, ['cFight', 'cWork', 'cBob', 'cBaby', 'cRaid2'], COMBAT_TARGETS)).reduce((a, b) => a.concat(b), []),
        ...matrix('rangedMassAttack', ['cFight', 'cWork', 'cBob', 'cBaby', 'cRaid2'], ['']),
        ...matrix('notifyWhenAttacked', ['cFight', 'cWork', 'cBob', 'cBaby'], [''], ['true', 'false', "'x'", 'undefined', '1']),
    ]), {}, (o) => { o.push(...fighters()); byId(o, 'cWork').notifyWhenAttacked = false; }),
]);

add('creep-controller', ['game/creeps.js:839', 'game/creeps.js:888', 'game/creeps.js:961', 'game/creeps.js:1057', 'game/creeps.js:1080'], [
    tick(probe([
        ...['claimController', 'attackController', 'reserveController', 'generateSafeMode', 'signController'].map(m =>
            matrix(m, ['cRaid2', 'cRaid3', 'cRaid4', 'cBob2', 'cWork', 'cFight', 'cBaby'],
                ["G('ctrl2')", "G('ctrl3')", "G('ctrl4')", "G('ctrl1')", "G('cont2')", "G('src3')", ...JUNK],
                m === 'signController' ? [", 'hello'", '', ', 5'] : [''])).reduce((a, b) => a.concat(b), []),
        "G('cRaid4').claimController(G('ctrl4'))",
        "G('cRaid3').claimController(G('ctrl4'))",
    ]), {}, (o) => o.push(...fighters())),
    // GCL_NOVICE: 3 rooms owned and the creep's room novice
    tick(probe(["G('cRaid4').claimController(G('ctrl4'))", "G('cRaid4').reserveController(G('ctrl4'))"]),
        { user: { rooms: ['W1N1', 'W5N1', 'W9N9'], gcl: 1e12 }, roomDocs: { W4N1: { novice: 1600000000001 } } }),
    tick(probe(["G('cRaid4').claimController(G('ctrl4'))"]),
        { user: { rooms: ['W1N1', 'W5N1', 'W9N9'], gcl: 1e12 }, roomDocs: { W4N1: { novice: 1600000000000 } } }),
    tick(probe(["G('cRaid4').claimController(G('ctrl4'))", "G('cRaid4').reserveController(G('ctrl4'))"]), { user: { shardAccess: false } }),
    tick(probe(["G('cRaid4').claimController(G('ctrl4'))"]), { user: { rooms: [], gcl: 0 } }),
    tick(probe(["G('cRaid4').claimController(G('ctrl4'))", "G('cRaid4').claimController(G('ctrl4'))"]), { user: { rooms: ['W1N1'], gcl: 0 } }),
    tick(probe(["G('cRaid3').attackController(G('ctrl3'))", "G('cRaid4').attackController(G('ctrl4'))"]), {}, (o) => {
        byId(o, 'ctrl3').upgradeBlocked = 1001; byId(o, 'ctrl4').effects = [{ effect: 1001, endTime: 1001 }];
    }),
]);

// GAME-CREEPS-DUP-NAME: a later own creep with an existing name is suicided during Game construction.
add('creep-dup-name', ['game/creeps.js:813', 'game/creeps.js:122'], [
    tick(code(function() {
        P('creeps', () => Object.keys(Game.creeps).map(n => n + ':' + Game.creeps[n].id));
        P('dups', () => ['dup1', 'dup2', 'dup3'].map(id => Game.getObjectById(id).name));
    }), {}, (o) => o.push(D.creep({ _id: 'dup1', name: 'Worker', x: 2, y: 2 }), D.creep({ _id: 'dup2', name: 'Worker', x: 3, y: 2, spawning: true }),
        D.creep({ _id: 'dup3', name: '7', x: 4, y: 2 }), D.creep({ _id: 'dup4', name: '7', x: 5, y: 2 }))),
]);

// Prototype patches persist across ticks (classes are built once per global) and the engine calls the patched
// public paths: moveTo -> this.move, moveByPath -> this.move, transfer(controller) -> this.upgradeController,
// Structure#destroy -> this.room.find, harvest -> extractor.isActive().
const patched = code(function() {
    if (!Creep.prototype._origMove) {
        Creep.prototype._origMove = Creep.prototype.move;
        Creep.prototype.move = function(d) { P('patched move ' + this.name, () => d); return this._origMove(d); };
        Creep.prototype.upgradeController = function(t) { P('patched upgrade', () => t.id); return 42; };
        Room.prototype._origFind = Room.prototype.find;
        Room.prototype.find = function(type) { P('patched find ' + type, () => 0); return []; };
        StructureExtractor.prototype.isActive = function() { P('patched isActive', () => this.id); return true; };
    }
    P('moveTo', () => Game.creeps.M1.moveTo(15, 40));
    P('moveByPath', () => Game.creeps.M1.moveByPath('0540' + '3'));
    P('transfer to controller', () => Game.creeps.Up.transfer(Game.getObjectById('ctrl1'), 'energy'));
    P('destroy', () => Game.getObjectById('xOk').destroy());
    P('harvest inactive', () => Game.creeps.Miner.harvest(Game.getObjectById('mInactive')));
    P('flag', () => Creep.prototype._origMove !== undefined);
});
add('creep-patched-proto', ['game/creeps.js:302', 'game/creeps.js:332', 'game/creeps.js:456', 'game/structures.js:80', 'game/creeps.js:380'], [
    tick(patched, { time: 1000 }, (o) => o.push(D.creep({ _id: 'mv1', name: 'M1', x: 5, y: 40 }))),
    tick(patched, { time: 1001 }, (o) => o.push(D.creep({ _id: 'mv1', name: 'M1', x: 6, y: 40 }))),
]);

// moveTo: clean lane at y=40 (x 3..20) for movers; the wall column x=25 (y<=40) forces detours.
const movers = () => [
    D.creep({ _id: 'mv1', name: 'M1', x: 5, y: 40 }),
    D.creep({ _id: 'mv2', name: 'M2', x: 5, y: 42 }),
    D.creep({ _id: 'mv3', name: 'M3', x: 5, y: 44 }),
    D.creep({ _id: 'mv4', name: 'M4', x: 5, y: 46 }),
    D.creep({ _id: 'mvT', name: 'MT', x: 3, y: 44, fatigue: 2 }),
    D.creep({ _id: 'mvR', name: 'MR', x: 20, y: 46 }),
];
const moveToCode = code(function() {
    var m = n => Game.creeps[n];
    if (Game.time == 1000) {
        P('numbers', () => m('M1').moveTo(15, 40));
        P('mem M1', () => Memory.creeps.M1);
        var o3 = {reusePath: 3};
        P('third-arg opts', () => m('M2').moveTo(15, 42, o3));
        P('third-arg opts mutated', () => o3);
        var vis = {stroke: 'red'}, o2 = {visualizePathStyle: vis, serializeMemory: false};
        P('pos+opts', () => m('M3').moveTo(new RoomPosition(15, 44, 'W1N1'), o2));
        P('opts cloned, style defaulted', () => [o2, vis]);
        P('mem M3', () => Memory.creeps.M3);
        P('object target', () => m('M4').moveTo(m('M1'), {reusePath: 0}));
        P('mem M4', () => Memory.creeps.M4);
        P('tired', () => m('MT').moveTo(10, 44));
        P('tired visualized', () => m('MT').moveTo(10, 44, {visualizePathStyle: {}}));
        P('cross wall', () => m('MR').moveTo(30, 20));
        P('other room', () => m('Worker').moveTo(new RoomPosition(10, 10, 'W2N1')));
        P('mem worker', () => Memory.creeps.Worker);
        ['Baby', 'Stuck', 'Hurt'].forEach(n => P(n, () => m(n).moveTo(1, 1)));
        P('foreign', () => Game.getObjectById('cBob').moveTo(1, 1));
        var e = m('Empty');
        [[undefined], [null], [{}], ['a'], [5], [5, undefined], [NaN, 5], [5, NaN], [{x: 5, y: 5}], [{pos: {x: 5, y: 5, roomName: 'W1N1'}}],
            [11, 20], [e], [e.pos], [60, 20], [10.5, 20.5], ['12', 20], [12, '20'], [{id: 'abcdefabcdefabcdefabcdef'}]].forEach((a, i) =>
            P('Empty moveTo #' + i, () => e.moveTo.apply(e, a)));
        P('noPathFinding', () => m('Up').moveTo(20, 28, {noPathFinding: true}));
        P('unreachable', () => m('Loud').moveTo(25, 10));
        P('mem loud', () => Memory.creeps.Loud);
        Memory.creeps.UpFar = 5;
        P('non-object memory', () => m('UpFar').moveTo(30, 34));
        P('memory after', () => Memory.creeps.UpFar);
    }
    else {
        ['M1', 'M2', 'M3', 'M4', 'MR', 'Worker', 'Loud'].forEach(n => {
            P(n + ' again', () => m(n).moveTo.apply(m(n), n == 'MR' ? [30, 20] : n == 'Worker' ? [new RoomPosition(10, 10, 'W2N1')] :
                n == 'Loud' ? [25, 10] : n == 'M3' ? [new RoomPosition(15, 44, 'W1N1'), {serializeMemory: false, visualizePathStyle: {}}] :
                    n == 'M4' ? [m('M1')] : [15, +n[1] * 2 + 38]));
            P(n + ' mem', () => Memory.creeps[n]);
        });
        P('M1 other dest', () => m('M1').moveTo(16, 40));
        P('M1 mem 2', () => Memory.creeps.M1);
        P('M2 reuse 0', () => m('M2').moveTo(15, 42, {reusePath: 0}));
    }
});
add('creep-moveto', ['game/creeps.js:158'], [
    tick(moveToCode, { time: 1000 }, (o) => o.push(...movers())),
    // followed the path one step; time 1001 within reusePath
    tick(moveToCode, { time: 1001 }, (o) => {
        o.push(...movers());
        byId(o, 'mv1').x = 6; byId(o, 'mv2').x = 6; byId(o, 'mv3').x = 6; byId(o, 'mvR').x = 21; byId(o, 'mvR').y = 47;
    }),
    // blocked: did not move, still within reusePath
    tick(moveToCode, { time: 1003 }, (o) => {
        o.push(...movers());
        byId(o, 'mv1').x = 6; byId(o, 'mv2').x = 6; byId(o, 'mv3').x = 7; byId(o, 'mv3').y = 45;
    }),
    // reusePath expired (5 for M1, 3 for M2)
    tick(moveToCode, { time: 1007 }, (o) => {
        o.push(...movers());
        byId(o, 'mv1').x = 7; byId(o, 'mv2').x = 7;
        const w = byId(o, 'cWork'); w.room = 'W2N1'; w.x = 12; w.y = 20;
    }),
]);

// MOVETO-REUSE / MOVETO-EARLY-CHECKS boundaries: a cached path is kept while time <= _move.time + reusePath;
// fatigue 1 already returns ERR_TIRED.
const reuseCode = code(function() {
    var m = n => Game.creeps[n];
    P('M1', () => m('M1').moveTo(15, 40));
    P('M2', () => m('M2').moveTo(15, 42, {reusePath: 2}));
    P('MT1', () => m('MT1').moveTo(10, 38));
    P('mem', () => [Memory.creeps.M1, Memory.creeps.M2, Memory.creeps.MT1]);
});
const reuseWorld = (t) => (o) => {
    o.push(...movers(), D.creep({ _id: 'mvT1', name: 'MT1', x: 3, y: 38, fatigue: 1 }));
    // followed their tick-1000 paths (M1 0641 4333333332, M2 0643 4333333332) one step per tick
    Object.assign(byId(o, 'mv1'), [{ x: 5, y: 40 }, { x: 6, y: 41 }, { x: 7, y: 41 }, { x: 8, y: 41 }][t]);
    Object.assign(byId(o, 'mv2'), [{ x: 5, y: 42 }, { x: 6, y: 43 }, { x: 7, y: 43 }, { x: 7, y: 43 }][t]);
};
add('creep-moveto-reuse', ['game/creeps.js:158'], [
    tick(reuseCode, { time: 1000 }, reuseWorld(0)),
    tick(reuseCode, { time: 1002 }, reuseWorld(1)),   // M2 at its boundary (1000 + 2)
    tick(reuseCode, { time: 1005 }, reuseWorld(2)),   // M1 at its boundary (1000 + 5); M2 expired
    tick(reuseCode, { time: 1006 }, reuseWorld(3)),   // M1 expired
]);

add('creep-movebypath', ['game/creeps.js:305'], [
    tick(code(function() {
        var c = Game.creeps.M1, e = Game.creeps.Empty;
        var steps = c.pos.findPathTo(15, 40);
        var ser = Room.serializePath(steps);
        var pf = PathFinder.search(c.pos, {pos: new RoomPosition(15, 40, 'W1N1'), range: 0}).path;
        P('steps', () => steps);
        P('by steps', () => c.moveByPath(steps));
        P('by string', () => c.moveByPath(ser));
        P('by positions', () => c.moveByPath(pf));
        P('positions incl. self', () => c.moveByPath([c.pos].concat(pf)));
        P('positions self last', () => c.moveByPath([pf[0], c.pos]));
        P('positions far', () => c.moveByPath(pf.slice(3)));
        P('positions mixed', () => c.moveByPath([pf[3], {x: 1, y: 1}]));
        P('positions mixed hit', () => c.moveByPath([pf[0], {x: 1, y: 1}]));
        P('other creep path', () => e.moveByPath(steps));
        P('foreign', () => Game.getObjectById('cBob').moveByPath(steps));
        P('foreign str', () => Game.getObjectById('cBob').moveByPath('0540'));
        P('foreign start', () => Game.getObjectById('cBob').moveByPath('1121' + '3'));
        [[], '', 5, {}, null, undefined, 'abc', '0540', '05403', '054039', '0540x', '05403333', [{x: 6, y: 40, dx: 1, dy: 0, direction: 3}],
            [{x: '6', y: '40', dx: '1', dy: 0, direction: '3'}]].forEach(p =>
            P('path ' + JSON.stringify(p), () => c.moveByPath(p)));
        P('tired', () => Game.creeps.MT.moveByPath('0444' + '3'));
        P('cross room', () => c.moveByPath([new RoomPosition(6, 40, 'W2N1')]));
    }), {}, (o) => o.push(...movers())),
]);

// Return-code rows the matrices above never reach (each found by a return-code mutation sweep of creeps.js).
add('creep-rows', ['game/creeps.js:158', 'game/creeps.js:839', 'game/creeps.js:888', 'game/creeps.js:961', 'game/creeps.js:1024'], [
    tick(probe([
        "G('cBoxed').moveTo(10, 10)", "Memory.creeps.Boxed",   // MOVETO-RESULT: walled in by terrain and own creeps
        "G('cW4').claimController(G('ctrl4'))",                 // no CLAIM part
        "G('cW4').reserveController(G('ctrl4'))",               // no CLAIM part
        "G('cRaid4').attackController(G('ctrl4'))",             // neither owner nor reservation
        "G('cRaid4').claimController(G('ctrl4'))",              // unowned controller of the creep's room in safe mode
        "G('cCarrier').dismantle(G('extF'))",                   // no WORK part
    ]), { user: { gcl: 1e12 } }, (o) => {
        Object.assign(byId(o, 'ctrl4'), { reservation: null, safeMode: 1500 });
        o.push(D.creep({ _id: 'cW4', name: 'W4', room: 'W4N1', x: 12, y: 20 }), D.creep({ _id: 'cBoxed', name: 'Boxed', x: 1, y: 1 }),
            D.creep({ _id: 'cBox1', name: 'Box1', x: 2, y: 1 }), D.creep({ _id: 'cBox2', name: 'Box2', x: 1, y: 2 }),
            D.creep({ _id: 'cBox3', name: 'Box3', x: 2, y: 2 }));
    }),
]);

module.exports = cases;
