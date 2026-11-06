// The world map grid behind Game.map (driver/lib/runtime/mapgrid.js; chitin reads it through natives.gridData,
// J3) and terrain views (J1), as seen by Game.map.describeExits/findRoute and Room.Terrain.
'use strict';
const { basicWorld, rooms } = require('./_lib');

// W1N1: walls on the top and right edges; W2N1: one plain exit tile on its left edge
const wall = (pred) => Array.from({ length: 2500 }, (_x, i) => (pred(i % 50, Math.floor(i / 50)) ? '1' : '0')).join('');

const w = basicWorld(`
module.exports.loop = function () {
    ['W1N1', 'W2N1', 'W3N1', 'W1N2', 'W0N0', 'E5S5', 'sim', 'W1N1x'].forEach(function (r) {
        console.log('exits', r, JSON.stringify(Game.map.describeExits(r)));
    });
    console.log('route', JSON.stringify(Game.map.findRoute('W1N1', 'W3N1')), JSON.stringify(Game.map.findRoute('W3N1', 'W1N2')));
    console.log('exit', Game.map.findExit('W1N1', 'W2N1'), Game.map.getRoomLinearDistance('W1N1', 'W3N1'), Game.map.getWorldSize());
    var t = Game.map.getRoomTerrain('W1N1');
    console.log('terrain', t.get(0, 0), t.get(10, 10), t.get(49, 25), typeof t.getRawBuffer());
    try { Game.map.getRoomTerrain('W9N9'); } catch (e) { console.log(e.message); }
};`, {
    rooms: rooms('W2N1', 'W1N1', 'W3N1', 'W1N2'),
    terrain: {
        W1N1: wall((x, y) => y === 0 || x === 49),
        W2N1: wall((x, y) => x === 0 && y !== 20),
    },
});

module.exports = [
    { name: 'map-grid', tags: ['runtime/mapgrid.js:68', 'runtime/runtime.js:206', 'game/map.js:20'], world: w, ticks: 2 },
];
