// ENV-ONE-REALM / GLB-SYSFUNC / J5: bots patch builtins in the shared realm. Engine code that calls them sees the
// patches exactly as vanilla does, Memory parsing uses the current JSON.parse while serialization uses the frozen
// copy, and the runtime data assembly (vanilla: ExternalCopy; chitin: host/prelude.js with captured builtins) is
// immune. Also require.cache hot reload.
'use strict';
const { basicWorld, doc, ME, OTHER } = require('./_lib');

module.exports = [
    {
        name: 'patched-builtins',
        tags: ['runtime/system-functions.js:1', 'game/game.js:485', 'runtime/runtime.js:247'],
        world: basicWorld(`
var realParse = JSON.parse, realStringify = JSON.stringify, realPush = Array.prototype.push, realKeys = Object.keys;
module.exports.loop = function () {
    if (Game.time === 101) {
        JSON.parse = function (s) { var o = realParse(s); o.viaPatchedParse = true; return o; };
        JSON.stringify = function () { return '"patched stringify"'; };
        Array.prototype.push = function () { global.pushes = (global.pushes || 0) + 1; return realPush.apply(this, arguments); };
        Object.keys = function (o) { global.keysCalls = (global.keysCalls || 0) + 1; return realKeys(o); };
        Array.prototype.sort = function () { return this; };
        Object.prototype.injected = 'enumerable junk';
        Math.ceil = function () { return 42; };
    }
    Memory.n = (Memory.n || 0) + 1;
    console.log('mem', realStringify(Memory), Object.keys(Game.creeps).length, Game.rooms.W1N1.find(FIND_CREEPS).length,
        typeof Game.creeps.Harvester1.store.getUsedCapacity(), global.pushes > 0, global.keysCalls > 0);
    Game.notify('n' + Memory.n);
    RawMemory.segments[1] = 'seg';
    for (var k in Game.spawns) console.log('spawn key', k);
};`, { memory: '{"n":0}' }),
        ticks: 3,
    },
    {
        // Game setup pushes onto a spatial slot's array when objects share a tile; a patched push sees every one
        // of them, whether or not the room is looked at (J3 keeps the slots in a Map until the first look).
        name: 'patched-push-setup',
        tags: ['game/game.js:43', 'game/game.js:414', 'game/rooms.js:688'],
        world: basicWorld(`
var realPush = Array.prototype.push;
module.exports.loop = function () {
    var seen = global.pushes;
    if (Game.time === 101) {
        Array.prototype.push = function () { global.pushes = (global.pushes || 0) + 1; return realPush.apply(this, arguments); };
    }
    console.log('pushes before loop', seen);
    global.pushes = 0;
    if (Game.time === 103) {
        console.log('look', JSON.stringify(Game.rooms.W1N1.lookForAt(LOOK_STRUCTURES, 25, 25).map(s => s.structureType)),
            Game.rooms.W1N1.lookForAt(LOOK_ENERGY, 26, 26).length, Game.rooms.W1N1.lookForAt(LOOK_CREEPS, 25, 25).length);
    }
};`, {
            objects: [
                doc.controller('W1N1', 20, 20, { user: ME }),
                doc.spawn('W1N1', 25, 25, 'Spawn1', ME),
                doc.extension('W1N1', 25, 25, ME),
                doc.creep('W1N1', 25, 25, 'Stacked1', ME),
                doc.creep('W1N1', 25, 25, 'Stacked2', OTHER),
                doc.extension('W1N1', 26, 26, ME),
                doc.energy('W1N1', 26, 26, 10),
                doc.energy('W1N1', 26, 26, 20),
                doc.extension('W2N1', 5, 5, OTHER),
                doc.extension('W2N1', 5, 5, OTHER),
            ],
        }),
        ticks: 4,
    },
    {
        // RoomPosition parses its room name with substr/charAt/parseInt: a patch sees every call, during Game setup
        // too, and a patched charAt changes the parse even for a name parsed before (J10 reuses the parse only while
        // all three are the originals). charAt calls are not counted: the vanilla harness's require uses charAt.
        name: 'patched-roomname-parse',
        tags: ['game/rooms.js:1276', 'utils.js:374'],
        world: basicWorld(`
var realSubstr = String.prototype.substr, realParseInt = parseInt, realCharAt = String.prototype.charAt;
module.exports.loop = function () {
    console.log('calls before loop', global.substrs, global.parseInts);
    global.substrs = global.parseInts = 0;
    if (Game.time === 101) {
        String.prototype.substr = function () { global.substrs++; return realSubstr.apply(this, arguments); };
        parseInt = function (s, r) { global.parseInts++; return realParseInt(s, r); };
    }
    if (Game.time === 102) {
        String.prototype.substr = realSubstr;
        parseInt = realParseInt;
        String.prototype.charAt = function (i) { return this == 'E3S4' && i === 0 ? 'W' : realCharAt.call(this, i); };
    }
    if (Game.time === 103) {
        String.prototype.charAt = realCharAt;
    }
    var p = [new RoomPosition(1, 2, 'W1N1'), new RoomPosition(3, 4, 'W1N1'), new RoomPosition(5, 6, 'E3S4')];
    console.log('positions', JSON.stringify(p), global.substrs, global.parseInts);
    try { new RoomPosition(1, 1, 'nowhere'); } catch (e) { console.log('invalid', e.message); }
};`),
        ticks: 5,
    },
    {
        name: 'require-hot-reload',
        tags: ['game/game.js:551'],
        world: basicWorld(`
module.exports.loop = function () {
    var a = require('counter');
    if (Game.time === 101) { delete require.cache.counter; }
    if (Game.time === 102) { require.cache.counter = {fake: true}; }
    if (Game.time === 103) { require.cache.fresh = 'injected'; }
    console.log('counter', JSON.stringify(require('counter')), a === require('counter'), global.evals, require('fresh'));
};`, {
            code: { timestamp: 1, modules: { main: null, counter: 'global.evals = (global.evals || 0) + 1; module.exports = {n: global.evals};', fresh: 'module.exports = "module";' } },
        }),
        ticks: 4,
    },
];
