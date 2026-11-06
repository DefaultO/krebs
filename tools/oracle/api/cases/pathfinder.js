// The native pathfinder bridge (runtime-driver.js:124 `pathFinder = create(global._nativeMod)`) end to end:
// PathFinder.search with a roomCallback CostMatrix, Room.findPath and Creep.moveTo (which writes memory._move).
// Needs the official native.node, i.e. node 22.
'use strict';
const { basicWorld, rooms } = require('./_lib');

const terrain = Array.from({ length: 2500 }, (_x, i) => {
    const x = i % 50, y = Math.floor(i / 50);
    return x === 0 || y === 0 || x === 49 || y === 49 ? (x === 25 || y === 25 ? '0' : '1') : x === 15 && y < 40 ? '1' : y === 30 && x > 20 ? '2' : '0';
}).join('');

// every room the searches can reach has terrain (the native throws 'Could not load terrain data' otherwise)
const NAMES = ['W3N3', 'W2N3', 'W1N3', 'W0N3', 'W3N2', 'W2N2', 'W1N2', 'W0N2', 'W3N1', 'W2N1', 'W1N1', 'W0N1', 'W3N0', 'W2N0', 'W1N0', 'W0N0'];

module.exports = [
    {
        name: 'pathfinder-bridge',
        needsNative: true,
        tags: ['runtime/runtime-driver.js:124'],
        world: basicWorld(`
module.exports.loop = function () {
    var from = new RoomPosition(11, 11, 'W1N1'), to = new RoomPosition(40, 35, 'W1N1');
    var r = PathFinder.search(from, {pos: to, range: 1}, {roomCallback: function (room) {
        var m = new PathFinder.CostMatrix();
        m.set(20, 20, 255);
        return room === 'W1N1' ? m : false;
    }});
    console.log('search', r.path.length, r.ops, r.cost, r.incomplete, r.path.slice(0, 3).map(String).join());
    console.log('findPath', JSON.stringify(Game.rooms.W1N1.findPath(from, to).slice(0, 4)));
    console.log('moveTo', Game.creeps.Harvester1.moveTo(to), JSON.stringify(Memory.creeps));
    console.log('multiroom', PathFinder.search(from, {pos: new RoomPosition(25, 25, 'W2N1'), range: 1}).path.length);
};`, { rooms: rooms('W2N1', 'W1N1', 'W3N1'), terrain: { W1N1: terrain, W2N1: terrain, W3N1: terrain } }),
        ticks: 2,
    },
];
