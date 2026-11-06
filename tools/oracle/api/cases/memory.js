// Memory and RawMemory (02 MEM-*), including the failure matrix rows that discard the whole tick (§13.2).
'use strict';
const { basicWorld } = require('./_lib');

const T = ['game/game.js:479', 'runtime/runtime.js:102', 'runtime/runtime.js:246', 'runtime/system-functions.js:1'];

module.exports = [
    {
        name: 'memory-getter',
        tags: T,
        world: basicWorld(`
module.exports.loop = function () {
    var before = Object.getOwnPropertyDescriptor(global, 'Memory');
    console.log('before', typeof before.get, typeof before.set, before.enumerable, before.configurable, 'value' in before);
    console.log('proto', Object.getPrototypeOf(Memory), typeof Memory.hasOwnProperty, JSON.stringify(Memory));
    var after = Object.getOwnPropertyDescriptor(global, 'Memory');
    console.log('after', after.writable, after.enumerable, after.configurable, after.value === Memory, RawMemory._parsed === Memory);
    Memory = {replaced: true};
    console.log('assign ignored', JSON.stringify(Memory));
    Memory.n = (Memory.n || 0) + 1;
    Memory.nested = {deep: [1, NaN, undefined, Infinity, -0]};
    Memory.fn = function () {};
    console.log('raw get unchanged', RawMemory.get() === RawMemory.get(), typeof RawMemory.get());
};`, { memory: '{"a":1,"creeps":{}}' }),
        ticks: 2,
    },
    {
        name: 'memory-strict-assign',
        tags: T,
        world: basicWorld(`'use strict';
module.exports.loop = function () {
    Memory.x = 1;
    try { Memory = {}; } catch (e) { console.log('strict', e.constructor.name, e.message); }
};`),
        ticks: 1,
    },
    {
        name: 'memory-memhack',
        tags: T.concat(['game/game.js:492']),
        world: basicWorld(`
var cached;
module.exports.loop = function () {
    if (cached) {
        delete global.Memory;
        global.Memory = cached;
        RawMemory._parsed = cached;
    } else {
        cached = Memory;
    }
    Memory.ticks = (Memory.ticks || 0) + 1;
    console.log('hack', Memory.ticks, Object.getOwnPropertyDescriptor(global, 'Memory').writable, Object.keys(RawMemory));
};`, { memory: '{"ticks":5}' }),
        ticks: 3,
    },
    {
        name: 'memory-rawset',
        tags: T.concat(['runtime/runtime.js:108']),
        world: basicWorld(`
module.exports.loop = function () {
    if (Game.time % 2 === 0) {
        Memory.lost = 'written before set';
        RawMemory.set('{"fromSet":' + Game.time + '}');
        Memory.alsoLost = true;
        console.log('after set', RawMemory.get(), '_parsed' in RawMemory, JSON.stringify(Memory));
    } else {
        RawMemory.set('{"early":1}');
        Memory.added = Game.time;
        console.log('set then access', JSON.stringify(Memory));
    }
    try { RawMemory.set(5); } catch (e) { console.log(e.message); }
    try { RawMemory.set('x'.repeat(2 * 1024 * 1024 + 1)); } catch (e) { console.log(e.message); }
    console.log('raw', Object.getOwnPropertyNames(RawMemory), Object.keys(RawMemory), RawMemory.interShardSegment === '');
};`, { memory: '{"old":true}' }),
        ticks: 2,
    },
    {
        name: 'memory-invalid-and-roots',
        tags: T.concat(['game/game.js:484']),
        world: basicWorld(`
module.exports.loop = function () {
    var m = Memory;
    console.log('mem', typeof m, JSON.stringify(m), m === null ? 'null' : Object.getPrototypeOf(m) === null);
    if (Game.time === 106) RawMemory.set('{"fixed":true}');
};`, { memory: 'not json', time: 100 }),
        ticks: [{}, { memory: '' }, { memory: 'null' }, { memory: '5' }, { memory: '0' }, { memory: '[1,2]' }, { memory: '"str"' }, {}],
    },
    {
        name: 'memory-serialize-throws',
        tags: T.concat(['runtime/runtime.js:247']),
        world: basicWorld(`
module.exports.loop = function () {
    Game.notify('queued ' + Game.time);
    RawMemory.segments[3] = 'seg';
    Memory.t = Game.time;
    if (Game.time === 101) Memory.self = Memory;
    if (Game.time === 102) Memory.big = 10n;
    if (Game.time === 103) Memory.bad = {toJSON() { throw new Error('toJSON throws'); }};
    // serializes to undefined: the run returns, then make.js reads memory.data.length and rejects it
    if (Game.time === 105) Memory.toJSON = function () {};
    console.log('logged but lost', Game.time);
};`, { memory: '{"keep":1}' }),
        ticks: 7,
    },
    {
        name: 'memory-creep-accessor',
        tags: T.concat(['game/creeps.js:95']),
        world: basicWorld(`
module.exports.loop = function () {
    var c = Game.creeps.Harvester1;
    console.log('creep mem', JSON.stringify(c.memory), JSON.stringify(Memory.creeps));
    c.memory.role = 'h' + Game.time;
    if (Game.time === 101) Memory.creeps = 'undefined';
    if (Game.time === 102) Memory.creeps = 5;
    try { c.memory = {set: 1}; } catch (e) { console.log(e.message); }
    var foe = Game.rooms.W1N1.find(FIND_HOSTILE_CREEPS)[0];
    console.log('foe mem', foe.memory);
    try { foe.memory = {}; } catch (e) { console.log(e.message); }
    console.log('room mem', JSON.stringify(Game.rooms.W1N1.memory), 'spawn mem', JSON.stringify(Game.spawns.Spawn1.memory));
};`, { memory: '{"creeps":{"Harvester1":0}}' }),
        ticks: 4,
    },
];
