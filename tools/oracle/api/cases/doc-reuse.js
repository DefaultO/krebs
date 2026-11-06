// Doc reuse (chitin J11, runtime doc_reuse.rs + host/prelude.js): docs unchanged since a VM's last run are taken
// over instead of parsed again. Vanilla parses every doc each tick, so whatever the player does to the objects it
// reaches (getters that return a piece of the doc, builtins and engine functions it patches to see the raw docs
// engine code passes them) never carries into the next tick. The world does not run between ticks here, so almost
// every doc is unchanged; the tick patches change, drop and add some.
'use strict';
const { basicWorld, doc, ME, id } = require('./_lib');

function withExtras(w) {
    w.objects.push(
        doc.spawn('W1N1', 35, 35, 'Spawn2', ME, { spawning: { name: 'Baby', needTime: 9, spawnTime: 105, directions: [1, 3] } }),
        doc.creep('W1N1', 35, 35, 'Baby', ME, { spawning: true }),
        doc.source('W1N1', 40, 40, { effects: [{ power: 13, level: 1, endTime: 200 }] }),
        { _id: id('t'), type: 'container', room: 'W1N1', x: 9, y: 9, hits: 250000, hitsMax: 250000, store: { energy: 10, H: 5 }, storeCapacity: 2000 },
    );
    return w;
}

// t1: the extension's doc changes; t2: the foreign creep goes (every later doc moves up one seq) and energy drops
function churn(w, t) {
    if (t === 1) w.objects.find((o) => o.type === 'extension').store.energy = 40;
    if (t === 2) {
        w.objects.splice(w.objects.findIndex((o) => o.name === 'Intruder'), 1);
        w.objects.push(doc.energy('W1N1', 12, 12, 25));
    }
}

const probes = `
function find(type) { return Game.rooms.W1N1.find(FIND_STRUCTURES, {filter: {structureType: type}}); }
function report(g) {
    var c = Game.creeps.Harvester1, s = Game.spawns.Spawn2, ext = find(STRUCTURE_EXTENSION)[0], box = find(STRUCTURE_CONTAINER)[0];
    var src = Game.rooms.W1N1.find(FIND_SOURCES), boosted = src.filter(function (x) { return x.effects && x.effects.length; })[0];
    console.log('same', c === g.c, c.body === g.body, s === g.s, s.store === g.store, s.spawning && s.spawning.directions === g.dirs,
        ext === g.ext, Game.getObjectById(src[0].id) === g.src);
    console.log('body', JSON.stringify(c.body), 'stores', JSON.stringify(s.store), JSON.stringify(ext.store), JSON.stringify(box.store),
        box.store.getUsedCapacity(), ext.store.getFreeCapacity(RESOURCE_ENERGY));
    console.log('spawning', JSON.stringify(s.spawning), 'effects', boosted && JSON.stringify(boosted.effects),
        'owner', JSON.stringify(Game.rooms.W1N1.controller.owner), Game.rooms.W1N1.find(FIND_DROPPED_RESOURCES).length);
    return {c: c, s: s, ext: ext, box: box, src: src[0], boosted: boosted};
}`;

const identity = withExtras(basicWorld(`${probes}
module.exports.loop = function () {
    var o = report(global.prev || {});
    // everything the player reaches, written to: none of it may reach the next tick
    o.c.body[0].hits = 1;
    o.c.body[1].type = 'attack';
    o.c.body.push({type: 'tough', hits: 7});
    o.s.store.energy = 1;
    o.ext.store.energy = 2;
    o.box.store.H = 99;
    o.s.spawning.directions.push(8);
    o.s.spawning.directions[0] = 5;
    if (o.boosted) o.boosted.effects[0].ticksRemaining = 1;
    Game.rooms.W1N1.controller.owner.username = 'Mallory';
    global.prev = {c: o.c, body: o.c.body, s: o.s, store: o.s.store, dirs: o.s.spawning.directions, ext: o.ext, src: o.src};
};`));

// Patches that hand the player raw docs, then write to them (cumulatively, so a doc carried over would show it):
// lodash and Object.entries (the Store constructor and getUsedCapacity), a getter on Object.prototype (engine code
// reading a key a doc lacks) and `call` shadowed on RoomObject (game objects' constructors pass it `effects`).
const leaks = withExtras(basicWorld(`${probes}
var realEntries = Object.entries, realSum = _.sum, realCall = Function.prototype.call;
function bump(o) { if (o && typeof o.energy === 'number') o.energy += 10; }
module.exports.loop = function () {
    report({});
    if (Game.time === 100) {
        _.sum = function (c) { bump(c); return realSum.apply(this, arguments); };
        Object.entries = function (o) { bump(o); return realEntries(o); };
        RoomObject.call = function (self, x, y, room, effects) {
            if (effects && effects[0]) effects[0].endTime += 100;
            return realCall.apply(RoomObject, arguments);
        };
        Object.defineProperty(Object.prototype, 'storeCapacity', {configurable: true, get: function () { bump(this.store); return undefined; }});
        find(STRUCTURE_CONTAINER)[0].store.getUsedCapacity();
        find(STRUCTURE_EXTENSION)[0].store.getFreeCapacity(RESOURCE_ENERGY);
        find(STRUCTURE_SPAWN).forEach(function (s) { return s.store.getFreeCapacity(RESOURCE_ENERGY); });
        report({});
    }
};`));

// The builtin check cannot see this one: `call` shadowed on RoomObject alone. Docs with effects are never taken over.
const shadowedCall = withExtras(basicWorld(`${probes}
var realCall = Function.prototype.call;
module.exports.loop = function () {
    report({});
    if (Game.time === 100) RoomObject.call = function (self, x, y, room, effects) {
        if (effects && effects[0]) effects[0].endTime += 100;
        return realCall.apply(RoomObject, arguments);
    };
};`));

module.exports = [
    {
        name: 'doc-reuse-identity',
        tags: ['game/creeps.js:92', 'game/structures.js:1395', 'game/store.js:19', 'game/rooms.js:1833'],
        world: identity,
        ticks: [{}, churn, churn, {}],
    },
    {
        name: 'doc-reuse-patched-leaks',
        tags: ['game/store.js:19', 'game/rooms.js:1833', 'runtime/runtime.js:247'],
        world: leaks,
        ticks: [{}, churn, churn, {}],
    },
    {
        name: 'doc-reuse-shadowed-call',
        tags: ['game/rooms.js:1833', 'game/sources.js:39'],
        world: shadowedCall,
        ticks: 4,
    },
];
