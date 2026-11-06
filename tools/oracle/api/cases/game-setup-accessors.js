// Game setup (game.js makeGameObject) reads `.id` of every object it registers and runs the find predicates
// (`my`, `spawning`, ...) on each, before the bot's loop. A bot that puts accessors on RoomObject.prototype sees every
// one of those calls, in vanilla's order and number; one whose `id` getter answers something else than the id the
// constructor set breaks the registers the way vanilla's do (chitin reads its internal registers once per object).
'use strict';
const { basicWorld, doc, ME, OTHER } = require('./_lib');

const objects = () => [
    doc.controller('W1N1', 20, 20, { user: ME }),
    doc.source('W1N1', 10, 10),
    doc.spawn('W1N1', 25, 25, 'Spawn1', ME),
    doc.creep('W1N1', 11, 11, 'Harvester1', ME),
    doc.extension('W1N1', 26, 26, ME),
    { _id: 'd00000000000001', type: 'road', room: 'W1N1', x: 12, y: 12, hits: 5000, hitsMax: 5000, nextDecayTime: 1000 },
    { _id: 'd00000000000002', type: 'constructedWall', room: 'W1N1', x: 13, y: 13, hits: 1000, hitsMax: 300000000 },
    { _id: 'd00000000000003', type: 'container', room: 'W1N1', x: 11, y: 12, hits: 250000, hitsMax: 250000,
        store: { energy: 10 }, storeCapacity: 2000, nextDecayTime: 1000 },
    doc.site('W1N1', 14, 14, 'road', ME),
    doc.creep('W1N1', 30, 30, 'Intruder', OTHER),
    doc.controller('W2N1', 20, 20, { user: OTHER }),
    doc.spawn('W2N1', 25, 25, 'BobSpawn', OTHER),
];

module.exports = [
    {
        name: 'patched-setup-accessors',
        tags: ['game/game.js:30', 'game/game.js:38', 'game/game.js:178', 'game/game.js:259'],
        world: basicWorld(`
module.exports.loop = function () {
    console.log('counted since the last loop', JSON.stringify(global.counts));
    global.counts = {};
    if (Game.time === 101) {
        var counts = function (k) { global.counts[k] = (global.counts[k] || 0) + 1; };
        Object.defineProperty(RoomObject.prototype, 'my', {configurable: true, get() { counts('my'); return this.id === 'd00000000000002' ? true : undefined; }});
        Object.defineProperty(RoomObject.prototype, 'id', {configurable: true,
            get() { counts('id get'); return this._idv; },
            set(v) { counts('id set'); this._idv = v; }});
    }
    var room = Game.rooms.W1N1;
    console.log('finds', [FIND_STRUCTURES, FIND_MY_STRUCTURES, FIND_HOSTILE_STRUCTURES, FIND_CREEPS, FIND_MY_CREEPS,
        FIND_MY_SPAWNS, FIND_CONSTRUCTION_SITES, FIND_SOURCES].map(t => room.find(t).length).join(' '),
        Object.keys(Game.structures).sort().join(','), Object.keys(Game.creeps).join(','), Object.keys(Game.spawns).join(','),
        room.energyAvailable, room.energyCapacityAvailable);
    console.log('look', room.lookForAt(LOOK_STRUCTURES, 12, 12).length, !!Game.getObjectById('d00000000000003'));
};`, { objects: objects() }),
        ticks: 4,
    },
    {
        name: 'patched-setup-id-mismatch',
        tags: ['game/game.js:38', 'game/game.js:259'],
        world: basicWorld(`
module.exports.loop = function () {
    console.log('loop', Game.time, Object.keys(Game.creeps).join(','), Game.rooms.W1N1.find(FIND_STRUCTURES).length);
    if (Game.time === 101) {
        global.left = 3;
        Object.defineProperty(RoomObject.prototype, 'id', {configurable: true,
            get() { return global.left > 0 && global.left-- ? this._idv + 'x' : this._idv; },
            set(v) { this._idv = v; }});
    }
};`, { objects: objects() }),
        ticks: 4,
    },
    {
        // An accessor on a game class runs inside Game setup and can read objectsByRoom there: the table must hold the
        // room's later docs too, then and for the rest of the tick.
        name: 'patched-setup-reads-room-table',
        tags: ['game/game.js:178', 'game/rooms.js:1'],
        world: basicWorld(`
module.exports.loop = function () {
    var room = Game.rooms.W1N1, spawn = Game.spawns.Spawn1, x = spawn.pos.x, y = spawn.pos.y;
    console.log('read in setup', global.inSetup, 'site on the spawn', room.createConstructionSite(x, y, 'container'),
        'site on the extension', room.createConstructionSite(26, 26, 'container'),
        'site on the road', room.createConstructionSite(12, 12, 'road'), spawn.isActive());
    if (Game.time === 101) {
        Object.defineProperty(StructureSpawn.prototype, '_my', {configurable: true, get() { return this.__m; },
            set(v) { this.__m = v; global.inSetup = this.room.createConstructionSite(this.pos.x, this.pos.y, 'container'); }});
    }
};`, { objects: objects() }),
        ticks: 4,
    },
    {
        // A setter added to Object.prototype inside the player loop must not see the room tables being filled, which
        // vanilla did before the loop, however late the first read comes; the host reuses those docs next tick.
        name: 'patched-loop-proto-setter-room-table',
        tags: ['game/game.js:178', 'game/rooms.js:1'],
        world: basicWorld(`
module.exports.loop = function () {
    var room = Game.rooms.W1N1, source = room.find(FIND_SOURCES)[0];
    if (Game.time === 101) {
        var seen = [], track = function (key) {
            Object.defineProperty(Object.prototype, key, {configurable: true, get() { return undefined; }, set(v) { seen.push(key); global.raw = v; }});
        };
        track(source.id);
        track('W1N1');
        console.log('site', room.createConstructionSite(source.pos.x, source.pos.y, 'container'));
        delete Object.prototype[source.id];
        delete Object.prototype.W1N1;
        console.log('setter saw', seen.join(), 'raw doc', !!global.raw);
        if (global.raw) global.raw.energyCapacity = 12345;
    }
    console.log('source', Game.getObjectById(source.id).energyCapacity, room.createConstructionSite(source.pos.x, source.pos.y, 'container'));
};`, { objects: objects() }),
        ticks: 4,
    },
    {
        // `off` is on spawns and extensions only, so Game setup's fast path reads it on those: a getter on Object.prototype
        // still sees every doc, whether put there during setup, by an accessor on a game class, or (`global.early`)
        // before the tick, which then takes vanilla's statements.
        name: 'patched-setup-proto-getters',
        tags: ['game/game.js:178', 'game/game.js:259'],
        world: basicWorld(`
var names = ['off'];
function install() {
    names.forEach(function (key) {
        Object.defineProperty(Object.prototype, key, {configurable: true, get() { global.counts[key] = (global.counts[key] || 0) + 1; return undefined; }});
    });
}
module.exports.loop = function () {
    console.log('counts', Game.time, JSON.stringify(global.counts));
    global.counts = Object.create(null);
    if (Game.time === 101) {
        Object.defineProperty(StructureController.prototype, '_my', {configurable: true, get() { return this.__m; },
            set(v) { this.__m = v; if (!global.early) install(); }});
    }
    if (Game.time === 103) global.early = true;
    names.forEach(function (key) { delete Object.prototype[key]; });
    if (global.early) install();
};`, { objects: objects() }),
        ticks: 6,
    },
];
