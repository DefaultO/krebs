// Store (05 / 03 Store): own truthy amounts, Proxy defaults, capacity methods per storage model.
'use strict';
const { basicWorld, doc, ME, id } = require('./_lib');

const T = ['game/store.js:19', 'game/store.js:42', 'game/store.js:86'];

const probes = `
function probe(label, st) {
    var res = ['H', 'energy', 'power', undefined, 'nonsense'].map(function (r) {
        return [st.getCapacity(r), st.getUsedCapacity(r), st.getFreeCapacity(r)];
    });
    console.log(label, JSON.stringify(Object.keys(st)), JSON.stringify(st), String(st), st.H, st.energy, st.nonsense,
        st instanceof Store, Object.getOwnPropertyNames(st).join(), JSON.stringify(res), 'energy' in st, st._sum);
}
module.exports.loop = function () {
    var r = Game.rooms.W1N1;
    probe('creep', Game.creeps.Harvester1.store);
    probe('spawn', Game.spawns.Spawn1.store);
    probe('ext', r.find(FIND_MY_STRUCTURES, {filter: {structureType: 'extension'}})[0].store);
    r.find(FIND_STRUCTURES).forEach(function (s) { if (s.store) probe(s.structureType, s.store); });
    var s = Game.creeps.Harvester1.store;
    console.log('identity', s === Game.creeps.Harvester1.store, Object.keys(Game.creeps.Harvester1));
};`;

const w = basicWorld(probes);
w.objects.push(
    { _id: id('t'), type: 'container', room: 'W1N1', x: 5, y: 5, hits: 250000, hitsMax: 250000, store: { energy: 100, H: 20, O: 0 }, storeCapacity: 2000, nextDecayTime: 500 },
    { _id: id('l'), type: 'lab', room: 'W1N1', x: 6, y: 6, user: ME, hits: 500, hitsMax: 500, cooldown: 0, store: { energy: 200, H: 50 }, storeCapacityResource: { energy: 2000, H: 3000 }, storeCapacity: 0 },
    { _id: id('l'), type: 'lab', room: 'W1N1', x: 7, y: 6, user: ME, hits: 500, hitsMax: 500, cooldown: 0, store: { energy: 0 }, storeCapacityResource: { energy: 2000 }, storeCapacity: 3000 },
    { _id: id('p'), type: 'powerSpawn', room: 'W1N1', x: 8, y: 6, user: ME, hits: 5000, hitsMax: 5000, store: { energy: 10, power: 0 }, storeCapacityResource: { energy: 5000, power: 100 } },
);
w.objects.find((o) => o.name === 'Harvester1').store = { energy: 20, H: 5, O: 0 };

module.exports = [
    { name: 'store-shape', tags: T, world: w, ticks: 1 },
    // A replaced global Proxy receives every Store's handler: its own for each Store, whatever it does to it.
    {
        name: 'store-patched-proxy',
        tags: T,
        world: basicWorld(`
module.exports.loop = function () {
    if (Game.time === 101) {
        global.seen = new Set(); global.made = 0; global.calls = 0;
        var Real = Proxy;
        global.Proxy = function (target, handler) {
            global.made++; global.seen.add(handler);
            var get = handler.get;
            handler.get = function (a, b, c) { global.calls++; return get.call(this, a, b, c); };
            return new Real(target, handler);
        };
        return;
    }
    global.calls = 0;
    var energy = Game.spawns.Spawn1.store.energy;
    console.log('stores', global.made, 'handlers', global.seen.size, 'trap calls for one read', global.calls, energy);
};`),
        ticks: 5,
    },
];
void doc;
