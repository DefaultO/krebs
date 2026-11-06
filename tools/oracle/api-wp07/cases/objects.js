'use strict';
// PowerCreep (game/power-creeps.js), Tombstone, Ruin, Deposit and Nuke cases.
const { D } = require('../world');
const { probe, code, matrix, JUNK } = require('./_lib');
const { ROOMS, tick, byId } = require('./_world');

const cases = [];
const add = (name, tags, ticks) => cases.push({ name, tags, rooms: ROOMS, ticks });

// Account power creeps (users.power_creeps) and the room objects of the spawned ones.
const PC = (o) => Object.assign({
    user: 'u1', className: 'operator', level: 3, hitsMax: 3000, store: {}, storeCapacity: 300,
    spawnCooldownTime: null, powers: { 1: { level: 2 }, 2: { level: 1 } }, deleteTime: null,
}, o);
const userPCs = [
    PC({ _id: 'pcA', name: 'Alpha', powers: { 1: { level: 2, cooldownTime: 1010 }, 2: { level: 1 }, 6: { level: 1 } } }),
    PC({ _id: 'pcU', name: 'Unspawned', level: 0, powers: {}, spawnCooldownTime: 1600000005000 }),
    PC({ _id: 'pcD', name: 'Deleting', deleteTime: 1600000100000, spawnCooldownTime: 1500000000000, level: 1, powers: { 1: { level: 1 } }, shard: 'shard0' }),
    PC({ _id: 'pcMax', name: 'Maxed', level: 25, powers: { 1: { level: 5 } } }),
];
const roomPC = (id, o) => Object.assign({}, userPCs.find(p => p._id === id), {
    type: 'powerCreep', room: 'W5N1', x: 20, y: 25, hits: 2500, ageTime: 5000, actionLog: { say: { message: 'op', isPublic: false } },
    notifyWhenAttacked: true, store: { ops: 120, energy: 10 },
}, o);
const pcWorld = (o) => {
    byId(o, 'ctrl5').level = 8;
    byId(o, 'ctrl5').isPowerEnabled = true;
    o.push(
        roomPC('pcA'),
        roomPC('pcMax', { x: 30, y: 30, store: {} }),
        { _id: 'pcBob', type: 'powerCreep', user: 'u2', name: 'BobPc', className: 'operator', level: 1, room: 'W5N1', x: 21, y: 25, hits: 1000, hitsMax: 1000, store: {}, storeCapacity: 100, ageTime: 4000, powers: { 1: { level: 1 } }, actionLog: { say: { message: 'pub', isPublic: true } } },
        D.structure('powerSpawn', { _id: 'ps5', room: 'W5N1', user: 'u1', x: 20, y: 26, store: { energy: 100, power: 2 }, storeCapacityResource: { energy: 5000, power: 100 } }),
        D.structure('powerSpawn', { _id: 'psBob', room: 'W2N1', user: 'u2', x: 20, y: 26, store: {}, storeCapacityResource: { energy: 5000, power: 100 } }),
        D.structure('powerSpawn', { _id: 'psW1', room: 'W1N1', user: 'u1', x: 3, y: 18, store: {}, storeCapacityResource: { energy: 5000, power: 100 } }),
        D.structure('powerBank', { _id: 'pbank5', room: 'W5N1', x: 19, y: 25, store: { power: 100 }, decayTime: 5000 }),
        D.structure('storage', { _id: 'stor5', room: 'W5N1', user: 'u1', x: 21, y: 24, store: { energy: 100, ops: 50 }, storeCapacity: 1000000 }),
        D.spawn({ _id: 'spawn5', name: 'Five', room: 'W5N1', x: 22, y: 25, effects: [{ power: 2, level: 4, endTime: 1100 }] }),
        D.energy({ _id: 'drop5', room: 'W5N1', x: 20, y: 24, resourceType: 'ops', ops: 10, energy: undefined }),
        D.creep({ _id: 'cFive', name: 'Fiver', room: 'W5N1', x: 21, y: 26, store: { energy: 0 } }),
    );
};
const PC_TAGS = ['game/power-creeps.js:9', 'game/power-creeps.js:15', 'game/power-creeps.js:34', 'game/power-creeps.js:45',
    'game/power-creeps.js:75', 'game/power-creeps.js:102'];

add('powercreep-props', PC_TAGS, [
    tick(code(function() {
        P('Game.powerCreeps', () => Object.keys(Game.powerCreeps));
        ['Alpha', 'Unspawned', 'Deleting', 'Maxed'].forEach(n => {
            var c = Game.powerCreeps[n];
            ['id', 'name', 'my', 'owner', 'level', 'className', 'hitsMax', 'hits', 'shard', 'spawnCooldownTime', 'deleteTime', 'powers',
                'saying', 'carryCapacity', 'ticksToLive', 'memory', 'room', 'pos', 'effects'].forEach(p => P(n + '.' + p, () => c[p]));
            P(n + '.store', () => [c.store.ops, c.store.getCapacity(), c.carry === c.store]);
            P(n + '.toString', () => String(c));
            P(n + '.keys', () => Object.keys(c));
            P(n + '.byId', () => Game.getObjectById(c.id) === c);
        });
        var b = Game.getObjectById('pcBob');
        ['name', 'my', 'owner', 'saying', 'memory', 'powers', 'ticksToLive'].forEach(p => P('bob.' + p, () => b[p]));
        P('bob.setmem', () => { b.memory = {}; });
        P('find', () => [Game.rooms.W5N1.find(FIND_POWER_CREEPS).length, Game.rooms.W5N1.find(FIND_MY_POWER_CREEPS).length, Game.rooms.W5N1.find(FIND_HOSTILE_POWER_CREEPS).length]);
        P('proto', () => Object.getOwnPropertyNames(PowerCreep.prototype));
        P('statics', () => Object.getOwnPropertyNames(PowerCreep));
        P('empty', () => new PowerCreep().name);
        P('empty keys', () => Object.keys(new PowerCreep()));
        P('mem set', () => { Game.powerCreeps.Alpha.memory = {a: 1}; return Memory.powerCreeps; });
        P('mem str undefined', () => { Memory.powerCreeps = 'undefined'; return Game.powerCreeps.Alpha.memory; });
        P('mem nonobject', () => { Memory.powerCreeps = 1; return [Game.powerCreeps.Alpha.memory, Game.powerCreeps.Unspawned.memory]; });
        P('mem nonobject set', () => { Game.powerCreeps.Alpha.memory = {}; });
        P('mem undefined set', () => { delete Memory.powerCreeps; Game.powerCreeps.Unspawned.memory = 5; return Memory.powerCreeps; });
    }), { user: { power: 30000 }, powerCreeps: userPCs }, pcWorld),
]);

add('powercreep-methods', PC_TAGS.concat(['game/power-creeps.js:106', 'game/power-creeps.js:113', 'game/power-creeps.js:120',
    'game/power-creeps.js:127', 'game/power-creeps.js:134', 'game/power-creeps.js:141', 'game/power-creeps.js:148', 'game/power-creeps.js:155',
    'game/power-creeps.js:162', 'game/power-creeps.js:191', 'game/power-creeps.js:204', 'game/power-creeps.js:217', 'game/power-creeps.js:246',
    'game/power-creeps.js:295', 'game/power-creeps.js:319', 'game/power-creeps.js:342', 'game/power-creeps.js:356', 'game/power-creeps.js:375',
    'game/power-creeps.js:395']), [
    tick(probe([
        ...['Alpha', 'Unspawned', 'pcBob'].map(a => [
            `G('${a}').move(1)`, `G('${a}').move(G('cFive'))`, `G('${a}').moveTo(25, 25)`, `G('${a}').moveByPath('2025' + '3')`,
            `G('${a}').transfer(G('stor5'), 'ops', 10)`, `G('${a}').transfer(G('stor5'), 'energy')`, `G('${a}').transfer(G('ctrl5'), 'energy')`,
            `G('${a}').transfer(G('cFive'), 'energy', 5)`, `G('${a}').transfer(G('Unspawned'), 'energy')`,
            `G('${a}').withdraw(G('stor5'), 'ops')`, `G('${a}').withdraw(G('stor5'), 'energy', 300)`, `G('${a}').drop('ops', 5)`, `G('${a}').drop('H')`,
            `G('${a}').pickup(G('drop5'))`, `G('${a}').say('hi', true)`, `G('${a}').suicide()`, `G('${a}').delete()`, `G('${a}').delete(true)`,
            `G('${a}').renew(G('ps5'))`, `G('${a}').renew(G('pbank5'))`, `G('${a}').renew(G('psW1'))`, `G('${a}').renew(G('stor5'))`, `G('${a}').renew()`,
            `G('${a}').enableRoom(G('ctrl5'))`, `G('${a}').enableRoom(G('ctrl2'))`, `G('${a}').enableRoom(G('stor5'))`, `G('${a}').enableRoom({id: 'abcdefabcdefabcdefabcdef'})`,
            `G('${a}').notifyWhenAttacked(false)`, `G('${a}').notifyWhenAttacked(true)`, `G('${a}').notifyWhenAttacked('x')`,
            `G('${a}').rename('New')`, `G('${a}').rename('Alpha')`, `G('${a}').rename('')`, `G('${a}').rename(5)`, `G('${a}').rename('x'.repeat(101))`,
            `G('${a}').cancelOrder('say')`, `G('${a}').cancelOrder('move')`,
        ]).reduce((x, y) => x.concat(y), []),
        ...matrix('spawn', ['Alpha', 'Unspawned', 'Deleting', 'pcBob'], ["G('ps5')", "G('psBob')", "G('psW1')", "G('stor5')", 'undefined']),
        ...matrix('upgrade', ['Alpha', 'Unspawned', 'Maxed', 'pcBob'], ['1', '2', '6', '13', '99', "'1'", 'undefined']),
        ...matrix('usePower', ['Alpha', 'Maxed', 'Unspawned', 'pcBob'],
            ['1', '1, G("cFive")', '2', '2, G("spawn5")', '2, G("ps5")', '6', '6, G("stor5")', '6, G("ctrl1")', '13', '99', "'2', G('spawn5')"]),
    ]), { user: { power: 30000 }, powerCreeps: userPCs }, pcWorld),
    // no free power levels; creation
    tick(probe([
        "PowerCreep.create('Fresh', POWER_CLASS.OPERATOR)", "PowerCreep.create('Alpha', 'operator')", "PowerCreep.create('', 'operator')",
        "PowerCreep.create('Fresh2', 'executor')", "PowerCreep.create(5, 'operator')", "PowerCreep.create('x'.repeat(101), 'operator')",
        "G('Alpha').upgrade(1)",
    ]), { user: { power: 9000 }, powerCreeps: userPCs }, pcWorld),
    tick(probe(["PowerCreep.create('Fresh', 'operator')", "PowerCreep.create('Fresh', 'operator')", "PowerCreep.create('Fresh', 'nope')",
        "G('Alpha').upgrade(1)", "G('Unspawned').upgrade(1)"]), { user: { power: 1e9 }, powerCreeps: userPCs }, pcWorld),
    // room without power enabled; hostile safe mode room; ops shortage
    tick(probe(["G('Alpha').usePower(2, G('spawn5'))", "G('Alpha').usePower(1)"]), { user: { power: 30000 }, powerCreeps: userPCs }, (o) => {
        pcWorld(o); byId(o, 'ctrl5').isPowerEnabled = false;
    }),
    tick(probe(["G('Alpha').usePower(2, G('spawn5'))", "G('Alpha').enableRoom(G('ctrl5'))"]), { user: { power: 30000 }, powerCreeps: userPCs }, (o) => {
        pcWorld(o); byId(o, 'ctrl5').user = 'u2'; byId(o, 'ctrl5').safeMode = 1500;
    }),
    tick(probe(["G('Alpha').usePower(2, G('spawn5'))", "G('Alpha').usePower(1)"]), { user: { power: 30000 }, powerCreeps: userPCs }, (o) => {
        pcWorld(o); byId(o, 'pcA').store = { ops: 99 }; byId(o, 'spawn5').effects = [{ power: 2, level: 1, endTime: 1100 }];
    }),
    // return-code rows the ticks above never reach (each found by a return-code mutation sweep of power-creeps.js)
    tick(probe(["G('Maxed').upgrade(1)", "G('Alpha').upgrade(99)", "G('Lvl5').upgrade(1)", "PowerCreep.create('Alpha', 'operator')",
        "PowerCreep.prototype.delete.call({})", "PowerCreep.prototype.rename.call({}, 'x')", "G('Alpha').enableRoom({})",
        "G('Alpha').enableRoom(undefined)", "G('Alpha').renew(G('pbankFar'))", "G('Alpha').cancelOrder('nothing')"]),
        { user: { power: 1e9 }, powerCreeps: userPCs.concat([PC({ _id: 'pcLvl5', name: 'Lvl5', level: 10, powers: { 1: { level: 5 } } })]) },
        (o) => { pcWorld(o); o.push(D.structure('powerBank', { _id: 'pbankFar', room: 'W5N1', x: 40, y: 40, store: { power: 100 }, decayTime: 5000 })); }),
    // global intent cap: 50 per name, the 51st is silently dropped but still returns OK
    tick(code(function() {
        for (var i = 0; i < 52; i++) P('create ' + i, () => PowerCreep.create('Pc' + i, 'operator'));
    }), { user: { power: 1e12 }, powerCreeps: userPCs }, pcWorld),
]);

add('tombstone-ruin', ['game/tombstones.js:8', 'game/tombstones.js:33', 'game/tombstones.js:41', 'game/tombstones.js:236',
    'game/ruins.js:12', 'game/ruins.js:29', 'game/ruins.js:38', 'game/ruins.js:68'], [
    tick(code(function() {
        ['tomb', 'tombBob', 'tombPc', 'tombNone'].forEach(id => {
            var t = Game.getObjectById(id);
            ['deathTime', 'ticksToDecay', 'id'].forEach(p => P(id + '.' + p, () => t[p]));
            P(id + '.store', () => [t.store.energy, t.store.H, t.store.getUsedCapacity()]);
            P(id + '.creep', () => t.creep);
            P(id + '.creep fields', () => { var c = t.creep; return c && [c.id, c.name, c.spawning, c.my, c.body, c.owner, c.ticksToLive, c.carryCapacity, c.store.getCapacity(), c.carry.getFreeCapacity(), c.fatigue, c.hits, c.hitsMax, c.saying, c.className, c.level, c.powers, String(c), c instanceof Creep, c instanceof PowerCreep, c.pos, c.room && c.room.name]; });
            P(id + '.creep move', () => t.creep && t.creep.move(1));
            P(id + '.toString', () => String(t));
            P(id + '.keys', () => Object.keys(t));
        });
        ['ruin1', 'ruinRoad', 'ruinBob'].forEach(id => {
            var r = Game.getObjectById(id);
            ['structureType', 'destroyTime', 'ticksToDecay', 'id'].forEach(p => P(id + '.' + p, () => r[p]));
            P(id + '.store', () => [r.store.energy, r.store.getUsedCapacity()]);
            P(id + '.structure', () => r.structure);
            P(id + '.structure fields', () => { var s = r.structure; return [s.id, s.hits, s.hitsMax, s.structureType, s.owner, s.my, s instanceof OwnedStructure, s instanceof Structure, String(s)]; });
            P(id + '.structure isActive', () => r.structure.isActive());
            P(id + '.toString', () => String(r));
        });
        P('Tombstone proto', () => Object.getOwnPropertyNames(Tombstone.prototype));
        P('Ruin proto', () => Object.getOwnPropertyNames(Ruin.prototype));
        P('new Tombstone', () => new Tombstone('nope'));
        P('new Ruin', () => new Ruin());
        P('find', () => [Game.rooms.W1N1.find(FIND_TOMBSTONES).length, Game.rooms.W1N1.find(FIND_RUINS).length]);
    }), {}, (o) => {
        o.push(D.tombstone({ _id: 'tombPc', x: 20, y: 30, creepId: undefined, powerCreepId: 'pcX', powerCreepName: 'Ghost', powerCreepClassName: 'operator', powerCreepLevel: 4, powerCreepTicksToLive: 12, powerCreepSaying: 'boo', powerCreepPowers: { 1: { level: 2 } }, store: { ops: 3 } }),
            D.tombstone({ _id: 'tombNone', x: 21, y: 30, creepId: undefined, user: null, store: {} }),
            { _id: 'ruinRoad', type: 'ruin', x: 22, y: 30, room: 'W1N1', structureType: 'road', destroyTime: 995, decayTime: 1003, store: {}, structure: { id: 'oldroad', type: 'road', hits: 0, hitsMax: 5000 } },
            { _id: 'ruinBob', type: 'ruin', x: 23, y: 30, room: 'W1N1', structureType: 'spawn', destroyTime: 995, decayTime: 1003, store: { energy: 5 }, structure: { id: 'oldspawn', type: 'spawn', hits: 0, hitsMax: 5000, user: 'u2' } });
    }),
]);

add('deposit-nuke', ['game/deposits.js:6', 'game/deposits.js:21', 'game/deposits.js:29', 'game/deposits.js:38', 'game/deposits.js:40',
    'game/nukes.js:8', 'game/nukes.js:26', 'game/nukes.js:35', 'game/nukes.js:40'], [
    tick(probe([
        ...['dep', 'depCool', 'depNoDecay', 'depZero'].map(id =>
            `[G('${id}').depositType, G('${id}').cooldown, G('${id}').lastCooldown, G('${id}').ticksToDecay, String(G('${id}')), Object.keys(G('${id}'))]`),
        ...['nk1', 'nk2'].map(id => `[G('${id}').timeToLand, G('${id}').launchRoomName, String(G('${id}')), Object.keys(G('${id}')), G('${id}').pos, G('${id}').room.name]`),
        "Object.getOwnPropertyNames(Deposit.prototype)", "Object.getOwnPropertyNames(Nuke.prototype)",
        "[Game.rooms.W1N1.find(FIND_DEPOSITS).length, Game.rooms.W1N1.find(FIND_NUKES).length]",
        "new Deposit('nope')", "new Nuke()",
    ]), {}, (o) => {
        o.push(D.deposit({ _id: 'depNoDecay', x: 2, y: 40, decayTime: undefined, harvested: 0, depositType: 'mist' }),
            D.deposit({ _id: 'depZero', x: 3, y: 40, harvested: 12345, cooldownTime: 1000, depositType: 'biomass' }),
            { _id: 'nk1', type: 'nuke', room: 'W1N1', x: 25, y: 25, landTime: 50000, launchRoomName: 'W5N1' },
            { _id: 'nk2', type: 'nuke', room: 'W1N1', x: 26, y: 25, landTime: 1000, launchRoomName: 'W9N9', effects: [{ effect: 1001, endTime: 1100 }] });
    }),
]);

module.exports = cases;
