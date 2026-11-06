// The intent store and its CPU accounting (02 INT-STORE, CPU-INTENT-COST): set last-wins, free say/pull, push
// caps, pushByName, remove refunds, intents kept by reference, clone failure = total error.
'use strict';
const { basicWorld } = require('./_lib');

const T = ['runtime/runtime.js:63', 'game/game.js:174', 'runtime/runtime.js:271'];

module.exports = [
    {
        name: 'intents-cpu',
        tags: T,
        world: basicWorld(`
module.exports.loop = function () {
    var c = Game.creeps.Harvester1, s = Game.spawns.Spawn1;
    console.log('say', c.say('hi'), c.say('again', true));
    console.log('move', c.move(TOP), c.move(BOTTOM), c.move(LEFT));
    console.log('drop', c.drop(RESOURCE_ENERGY));
    console.log('spawn', s.spawnCreep([WORK, MOVE], 'New' + Game.time, {memory: {r: 1}}), s.spawnCreep([MOVE], 'Other'));
    var r = [];
    for (var i = 0; i < 21; i++) r.push(Game.notify('n' + i, i));
    console.log('notify', r.join());
    console.log('flag', Game.rooms.W1N1.createFlag(10, 10, 'F' + Game.time), Game.rooms.W1N1.createConstructionSite(15, 15, STRUCTURE_ROAD));
    console.log('market', Game.market.createOrder({type: ORDER_SELL, resourceType: RESOURCE_ENERGY, price: 0.123456, totalAmount: 1000, roomName: 'W1N1'}));
};`),
        ticks: 2,
    },
    {
        name: 'intents-by-reference',
        tags: T,
        world: basicWorld(`
module.exports.loop = function () {
    var msg = {text: 'original'};
    Game.notify(msg);
    msg.text = 'mutated after the call';
    var body = [WORK, CARRY, MOVE];
    Game.spawns.Spawn1.spawnCreep(body, 'Ref');
    body.push(MOVE);
};`),
        ticks: 1,
    },
    {
        name: 'intents-clone-failure',
        tags: T.concat(['runtime/runtime.js:296']),
        world: basicWorld(`
module.exports.loop = function () {
    Memory.t = Game.time;
    if (Game.time === 100) Game.notify(function () {});
    if (Game.time === 101) Game.notify(Symbol('s'));
    console.log('logged', Game.time);
};`),
        ticks: 3,
    },
];
