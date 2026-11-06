// The J1/J3 world natives behind Room.Terrain, Game.map and the staticTerrainData reads of rooms.js/map.js.
// Vanilla keys these by the stored room names, so a name that merely parses to the same coordinates ('W1N01', 'w1n1')
// is a different room for terrain/status/accessibility (describeExits goes through roomNameToXY and does alias).
// An own terrain entry beats an Object.prototype-polluted key; a closed room may carry a null timestamp (vanilla
// stores Infinity for out-of-borders rooms, which JSON turns into null).
'use strict';
const { base } = require('../world');

module.exports = [{
    name: 'natives-room-names',
    tags: ['game/rooms.js:1223', 'game/map.js:20', 'game/map.js:201', 'game/map.js:209', 'game/map.js:235'],
    world: base({ roomStatusData: { closed: { E2S2: null }, novice: { E1S2: 9000 } } }),
    ticks: [{ probes: [
        ['aliases', () => ['W1N1', 'W1N01', 'W01N1', 'w1n1', 'E2S2', 'E1S2', 'E01S2', 'E1S02', 'W1N1 '].map(n => [n,
            Game.map.getRoomStatus(n), Game.map.isRoomAvailable(n), Game.map.describeExits(n), Game.map.getTerrainAt(0, 0, n),
            (() => { try { return new Room.Terrain(n).get(25, 25); } catch (e) { return e.message; } })()])],
    ] }],
}, {
    name: 'natives-terrain-own-keys',
    tags: ['game/rooms.js:1223', 'game/map.js:235'],
    world: base(),
    ticks: [{ probes: [
        ['polluted prototype', () => {
            const out = [];
            Object.prototype.E1S1 = [7, 7, 7];
            Object.prototype.NotARoom = [1, 2];
            try {
                const t = new Room.Terrain('E1S1');
                out.push(t.getRawBuffer().length, t.get(0, 0), t.get(25, 25), Game.map.getTerrainAt(25, 25, 'E1S1'),
                    new Room.Terrain('NotARoom').get(1, 0), Game.map.getTerrainAt(0, 0, 'NotARoom'));
            } finally {
                delete Object.prototype.E1S1;
                delete Object.prototype.NotARoom;
            }
            out.push(new Room.Terrain('E1S1').getRawBuffer().length, (() => { try { return new Room.Terrain('NotARoom'); } catch (e) { return e.message; } })());
            return out;
        }],
    ] }, { probes: [
        ['next tick', () => [new Room.Terrain('E1S1').get(25, 25), new Room.Terrain('E1S0').getRawBuffer().length]],
    ] }],
}];
