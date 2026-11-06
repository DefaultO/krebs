// API paths into WP05's engine utils and intent store that other cases miss: isActive
// (checkStructureAgainstController), transaction cost, directions, border construction sites (checkTerrain),
// reaction variants, intents.remove (cancelOrder refunds 0.2 CPU), getUsed, power-creep find caches.
'use strict';
const { basicWorld, ME, OTHER, id } = require('./_lib');

const w = basicWorld(`
module.exports.loop = function () {
    var r = Game.rooms.W1N1, c = Game.creeps.Harvester1;
    console.log('active', r.find(FIND_MY_STRUCTURES).map(function (s) { return s.structureType + ':' + s.isActive(); }).join());
    console.log('cost', Game.market.calcTransactionCost(1000, 'W1N1', 'W5N5'), Game.map.getRoomLinearDistance('W1N1', 'W9N1', true));
    console.log('dir', c.pos.getDirectionTo(30, 30), c.pos.getDirectionTo(11, 1), c.pos.getDirectionTo(c.pos));
    console.log('border', r.createConstructionSite(1, 10, STRUCTURE_EXTENSION), r.createConstructionSite(10, 1, STRUCTURE_ROAD), r.createConstructionSite(48, 48, STRUCTURE_WALL));
    var lab = r.find(FIND_MY_STRUCTURES, {filter: {structureType: STRUCTURE_LAB}});
    console.log('reverse', lab[0].reverseReaction(lab[1], lab[2]));
    console.log('cancel', c.move(TOP), c.say('x'), c.cancelOrder('move'), c.cancelOrder('move'), c.cancelOrder('say'), c.transfer(Game.spawns.Spawn1, RESOURCE_ENERGY));
    console.log('used', typeof Game.cpu.getUsed(), Game.cpu.getUsed() >= 0);
    console.log('pcs', r.find(FIND_MY_POWER_CREEPS).map(String).join(), r.find(FIND_HOSTILE_POWER_CREEPS).map(String).join(), r.find(FIND_POWER_CREEPS).length, Object.keys(Game.powerCreeps).join());
};`);
const lab = (x, store) => ({ _id: id('l'), type: 'lab', room: 'W1N1', x, y: 40, user: ME, hits: 500, hitsMax: 500, cooldown: 0, store, storeCapacityResource: { energy: 2000 }, storeCapacity: 3000 });
w.objects.find((o) => o.type === 'controller' && o.room === 'W1N1').level = 6;
w.objects.push(lab(30, { energy: 0, OH: 100 }), lab(31, { energy: 0 }), lab(32, { energy: 0 }));
w.objects.push({ _id: 'pc1000000000001', type: 'powerCreep', room: 'W1N1', x: 20, y: 30, user: ME, name: 'Ops', className: 'operator', level: 2, hits: 1000, hitsMax: 1000, ageTime: 5000, store: {}, storeCapacity: 100, powers: { 1: { level: 1, cooldownTime: 0 } } });
w.objects.push({ _id: 'pc2000000000002', type: 'powerCreep', room: 'W1N1', x: 21, y: 30, user: OTHER, name: 'Foe', className: 'operator', level: 1, hits: 1000, hitsMax: 1000, ageTime: 5000, store: {}, storeCapacity: 100, powers: {} });
w.powerCreeps = [
    { _id: 'pc1000000000001', user: ME, name: 'Ops', className: 'operator', level: 2, hitsMax: 1000, store: {}, storeCapacity: 100, spawnCooldownTime: 0, powers: { 1: { level: 1 } } },
    { _id: 'pc3000000000003', user: ME, name: 'Benched', className: 'operator', level: 0, hitsMax: 1000, store: {}, storeCapacity: 100, spawnCooldownTime: 0, powers: {} },
];
w.terrain = { W1N1: Array.from({ length: 2500 }, (_x, i) => (i % 50 === 0 && Math.floor(i / 50) === 11 ? '0' : i % 50 === 0 || Math.floor(i / 50) === 0 ? '1' : '0')).join('') };

module.exports = [
    { name: 'api-utils', tags: ['src/utils.js:478', 'src/utils.js:348', 'runtime/runtime.js:92', 'game/game.js:19'], world: w, ticks: 1 },
];
