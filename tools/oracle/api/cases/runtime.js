// VM-level behaviour of the runtime (02 VM-*, CPU-*, GLB-*): bootstrap globals cleaned up, class/global
// descriptors, persistence across ticks, global resets (halt, code version), CPU timeouts, heap statistics,
// inactive users, deprecation logs and the serialized-object guard.
'use strict';
const { basicWorld, ME } = require('./_lib');

const T = ['runtime/runtime.js:1', 'runtime/runtime.js:55', 'runtime/user-vm.js:42'];

module.exports = [
    {
        name: 'runtime-globals',
        tags: T.concat(['game/game.js:470']),
        world: basicWorld(`
module.exports.loop = function () {
    // V8 builtins are non-enumerable (and differ between node versions): list what the runtime added. Assigning
    // Date (the oracle's clock hook) or console through a node>=24 vm global makes them enumerable: skip them.
    // Sorted: a node vm global's key order depends on the process's earlier contexts (oracle artifact).
    var names = Object.keys(globalThis).filter(function (n) { return n !== 'Date' && n !== 'console'; }).sort();
    console.log('globals', names.length, names.slice(0, 40).join(), names.filter(function (n) { return n[0] == '_'; }).join());
    ['global', 'Game', 'RawMemory', 'console', '_', 'require', 'module', '__module', 'Creep', 'Room', 'RoomPosition', 'Store', 'Resource', 'OK', 'BODYPART_COST', 'PathFinder'].forEach(function (n) {
        var d = Object.getOwnPropertyDescriptor(globalThis, n);
        console.log(n, d ? [typeof d.value, d.writable, d.enumerable, d.configurable, typeof d.get].join() : 'absent');
    });
    console.log('absent', typeof setTimeout, typeof Buffer, typeof process, typeof _init, typeof __chitinDefine, typeof _natives, typeof structuredClone);
    console.log('lodash', _.VERSION, typeof _.runInContext, require('lodash') === _);
    global.persist = (global.persist || 0) + 1;
    console.log('constants', BODYPART_COST.work, global.oldCost === BODYPART_COST, global.oldCost && global.oldCost.work);
    BODYPART_COST.work = 1;
    global.oldCost = BODYPART_COST;
    console.log('persist', global.persist, global.prev === Game, global.prev && global.prev.time);
    var old = global.prevCreep;
    if (old) {
        console.log('stale', old === Game.creeps.Harvester1, Object.keys(old), old.pos.x, old instanceof Creep);
        try { console.log('getter', old.hits, old.fatigue); } catch (e) { console.log('getter', e.message); }
        console.log('stale move', old.move(TOP));
    }
    global.prev = Game;
    global.prevCreep = global.prevCreep || Game.creeps.Harvester1;
};`),
        // the creep's data changes (unread getters follow it), then the creep vanishes
        ticks: [{}, (w) => { const c = w.objects.find((o) => o.name === 'Harvester1'); c.hits = 120; c.fatigue = 4; c.x = 12; },
            (w) => { w.objects = w.objects.filter((o) => o.name !== 'Harvester1'); }],
    },
    {
        name: 'runtime-halt-and-heap',
        tags: T.concat(['runtime/runtime.js:20', 'runtime/runtime.js:228']),
        world: basicWorld(`
global.boots = (global.boots || 0) + 1;
module.exports.loop = function () {
    var h = Game.cpu.getHeapStatistics();
    console.log('heap', Object.keys(h).join(), typeof h.heap_size_limit, 'boots', global.boots);
    Memory.t = Game.time;
    Game.notify('before halt ' + Game.time);
    if (Game.time === 101) {
        try { Game.cpu.halt(); } catch (e) { console.log('after halt', e.message); }
    }
};`),
        ticks: 4,
    },
    {
        name: 'runtime-code-reset',
        tags: T.concat(['game/game.js:504']),
        world: basicWorld(null, {
            code: { timestamp: 1, modules: { main: "global.v = (global.v || 0) + 1; module.exports.loop = function () { console.log('v1', global.v); };" } },
        }),
        ticks: [{}, {}, { code: { timestamp: 2, modules: { main: "global.v = (global.v || 0) + 1; module.exports.loop = function () { console.log('v2', global.v); };" } } }, {}],
    },
    {
        name: 'runtime-timeout',
        caveat: 'isolated-vm renders the frames captured at the interrupt (OBS-STACK-TIMEOUT); node vm timeouts carry none, so the expected error text has no frames',
        tags: T.concat(['runtime/runtime-driver.js:40', 'runtime/runtime-driver.js:105']),
        world: basicWorld(`
module.exports.loop = function () {
    Memory.before = Game.time;
    Game.notify('queued before the loop');
    try {
        if (Game.time === 100) { while (true) {} }
    } finally {
        Memory.finallyRan = true;
    }
    console.log('not reached on the timeout tick', Game.time);
};`, { consoleCommands: [{ expression: '"lost"' }] }),
        ticks: 2,
    },
    {
        name: 'runtime-inactive-user',
        tags: ['runtime/data.js:106'],
        world: basicWorld(`module.exports.loop = function () { console.log('ran'); };`),
        ticks: [{}, (w) => { w.objects = w.objects.filter((o) => o.user !== ME); }, {}],
    },
    {
        name: 'runtime-deprecated-and-serialized',
        tags: ['game/game.js:113', 'game/game.js:122'],
        world: basicWorld(`
module.exports.loop = function () {
    console.log(Game.map.isRoomAvailable('W1N1'), Game.map.isRoomAvailable('W1N1'), Game.map.getTerrainAt(1, 1, 'W1N1'));
    var c = Game.creeps.Harvester1, s = Game.spawns.Spawn1;
    try { c.transfer({id: '0123456789abcdef01234567', pos: s.pos}, RESOURCE_ENERGY); } catch (e) { console.log(e.message); }
    try { console.log(c.transfer({id: 'short', pos: s.pos}, RESOURCE_ENERGY)); } catch (e) { console.log(e.message); }
    console.log(c.transfer(JSON.parse(JSON.stringify(s)), RESOURCE_ENERGY));
};`),
        ticks: 2,
    },
];
