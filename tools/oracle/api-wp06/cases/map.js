// Game.map over the world natives: describeExits, findRoute (A* + heap tie-breaking, callbacks), findExit,
// isRoomAvailable, getRoomStatus, getTerrainAt, getRoomTerrain, getRoomLinearDistance, getWorldSize, map visual
// (spec 03 MAP-*, MV-*; game/path-utils.js Heap/OpenClosed).
'use strict';
const { base } = require('../world');

const statuses = { closed: { E2S2: 0, W9N9: 12 }, novice: { W2S2: 5000 }, respawn: { E2N2: 7000, W2S2: 1 } };

module.exports = [{
    name: 'map-exits-status',
    tags: ['game/map.js:20', 'game/map.js:201', 'game/map.js:209', 'game/map.js:235', 'game/map.js:261', 'game/map.js:265', 'game/map.js:269'],
    world: base({ roomStatusData: statuses }),
    ticks: [{ probes: [
        ['describeExits', () => ['W2N2', 'W1N1', 'W0N0', 'E0S0', 'E1S1', 'E2S2', 'E2N2', 'W2S2', 'W9N9', 'w1n1', 'W1N1x', ' W1N1', 'sim', '', undefined,
            {toString: () => 'W1N1'}, 5].map(n => { try { return Game.map.describeExits(n); } catch (e) { return e.constructor.name; } })],
        ['all exits', () => { const out = {}; for (const y of ['N2', 'N1', 'N0', 'S0', 'S1', 'S2']) for (const x of ['W2', 'W1', 'W0', 'E0', 'E1', 'E2']) out[x + y] = Game.map.describeExits(x + y); return out; }],
        ['status', () => ['W1N1', 'E2S2', 'W2S2', 'E2N2', 'W9N9', 'W50N50', 'w1n1', 'W1N1x', undefined, {toString: () => 'E2N2'}, ['W1N1'], {toString: () => 'W1N1'}]
            .map(n => { try { return Game.map.getRoomStatus(n); } catch (e) { return e.constructor.name; } })],
        ['available', () => ['W1N1', 'E2S2', 'W2S2', 'W50N50', 'w1n1', {toString: () => 'W1N1'}, ['W1N1']].map(n => Game.map.isRoomAvailable(n))],
        ['terrainAt', () => [Game.map.getTerrainAt(0, 0, 'W1N1'), Game.map.getTerrainAt(25, 25, 'W1N1'), Game.map.getTerrainAt(new RoomPosition(22, 0, 'W1N1')),
            Game.map.getTerrainAt({x: 1, y: 1, roomName: 'W2N1'}), Game.map.getTerrainAt(50, 0, 'W1N1'), Game.map.getTerrainAt(1, 1, 'W9N9'),
            Game.map.getTerrainAt(1, 1), Game.map.getTerrainAt('1', '1', 'W1N1'), Game.map.getTerrainAt(1.5, 1, 'W1N1')]],
        ['roomTerrain', () => { const out = []; for (const n of ['W1N1', 'W9N9']) { try { out.push(Game.map.getRoomTerrain(n).get(0, 0)); } catch (e) { out.push(e.message); } } return out; }],
        ['distance', () => [Game.map.getRoomLinearDistance('W1N1', 'E2S2'), Game.map.getRoomLinearDistance('W1N1', 'E2S2', true),
            Game.map.getRoomLinearDistance('W1N1', 'W1N1'), Game.map.getRoomLinearDistance('W1N1', 'bad'), Game.map.getRoomLinearDistance('W1N1', 'E10S1', true)]],
        ['worldSize', () => [Game.map.getWorldSize(), Object.keys(Game.map)]],
    ] }],
}, {
    name: 'map-find-route',
    tags: ['game/map.js:69', 'game/map.js:188', 'game/path-utils.js:39', 'game/path-utils.js:5', 'game/rooms.js:1130'],
    world: base({ roomStatusData: statuses }),
    ticks: [{ probes: [
        ['routes', () => {
            const out = [];
            for (const [a, b] of [['W1N1', 'W1N1'], ['W1N1', 'W1N2'], ['W1N1', 'E2S2'], ['W2N2', 'E1S1'], ['E1S1', 'W2N2'], ['W1N1', 'W0S1'],
                ['W1N1', 'W40N1'], ['w1n1', 'W1N2'], ['xW1N1', 'W1N2'], ['W1N1', 'W1N1junk'], ['W1N1', 'E5S5'], ['E2N2', 'W2S2']]) {
                out.push(Game.map.findRoute(a, b));
            }
            out.push(Game.map.findRoute(Game.rooms.W1N1, Game.rooms.W1N2), Game.map.findRoute({name: 'W2N2'}, 'E0S0'));
            return out;
        }],
        ['callbacks', () => {
            const calls = [];
            const r = [
                Game.map.findRoute('W2N1', 'W0S1', {routeCallback: (to, from) => { calls.push(from + '>' + to); return to == 'W1N1' ? Infinity : 1; }}),
                Game.map.findRoute('W2N1', 'W0S1', {routeCallback: () => 0}),
                Game.map.findRoute('W2N1', 'W0S1', {routeCallback: to => to[1] == '0' ? 5 : NaN}),
                Game.map.findRoute('W2N1', 'W0S1', {routeCallback: to => to[0] == 'E' ? '3' : -1}),
                Game.map.findRoute('W2N1', 'W0S1', {routeCallback: () => 0.5}),
                Game.map.findRoute('W2N1', 'W0S1', {routeCallback: () => Infinity}),
                Game.map.findRoute('W2N1', 'W0S1', {routeCallback: 'x'.length ? undefined : 1}),
            ];
            return [r, calls.slice(0, 40), calls.length];
        }],
        ['findExit', () => [Game.map.findExit('W1N1', 'W1N2'), Game.map.findExit('W1N1', 'W1N1'), Game.map.findExit('W1N1', 'W40N1'),
            Game.map.findExit('W1N1', 'E1S1', {routeCallback: () => 2}), Game.rooms.W1N1.findExitTo('W0N1'), Game.rooms.W1N1.findExitTo('W1N1'),
            Game.rooms.W1N2.findExitTo(Game.rooms.W1N1)]],
        ['heap reuse', () => {
            const out = [];
            for (let i = 0; i < 140; i++) out.push(Game.map.findRoute(i % 2 ? 'W2N2' : 'E1S1', i % 3 ? 'E0S0' : 'W1N0').length);
            return out.join('');
        }],
    ] }],
}, {
    name: 'map-visual',
    tags: ['game/map.js:274'],
    world: base(),
    ticks: [{ probes: [
        ['draw', () => {
            const v = Game.map.visual, p = new RoomPosition(1, 2, 'W1N1'), q = new RoomPosition(3, 4, 'W1N2'), out = [];
            out.push(v === Game.map.visual, Object.keys(v), Object.getOwnPropertyNames(v));
            out.push(v.circle(p) === v, v.circle(p, {fill: 'red'}), v.line(p, q), v.rect(p, 3, 4), v.poly([p, Game.creeps.Harv1, {x: 1, y: 1, roomName: 'X'}]),
                v.poly([]), v.text('t', q, {color: 1}), v.import('raw'), v.getSize(), v.export());
            for (const f of [() => v.circle({x: 1, y: 1, roomName: 'W1N1'}), () => v.line(p, {}), () => v.line(1, p), () => v.rect(), () => v.text('a', null)]) {
                try { out.push(f()); } catch (e) { out.push(e.message); }
            }
            out.push(v.clear() === v, v.getSize(), v.export());
            return out;
        }],
    ] }],
}, {
    // findRoute with non-uniform costs: falsy/NaN callback results fall back to 1, so they compete with 1.5 rooms
    name: 'map-route-costs',
    tags: ['game/map.js:69', 'game/path-utils.js:39'],
    world: base(),
    ticks: [{ probes: [
        ['mixed costs', () => {
            const out = [];
            for (const cb of [to => /N1$|S0$/.test(to) ? 0 : 1.5, to => /^W/.test(to) ? NaN : 1.5, to => /^E/.test(to) ? undefined : 1.5,
                to => /N2$|S2$/.test(to) ? '' : 1.25]) {
                for (const [a, b] of [['W2N1', 'E2N1'], ['W2N2', 'E2S2'], ['E2S2', 'W2N2'], ['W2S1', 'E2N0']]) {
                    const r = Game.map.findRoute(a, b, {routeCallback: cb});
                    out.push(Array.isArray(r) ? r.map(s => s.room).join(',') : r);
                }
            }
            return out;
        }],
    ] }],
}];
