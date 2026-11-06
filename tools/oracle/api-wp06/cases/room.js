// Room object surface outside find/look/paths: ctor fields, toString/toJSON, memory, getEventLog, getPositionAt,
// serializePath/deserializePath, RoomVisual, Room.Terrain / getTerrain (spec 03 ROOM-*, RV-*, TERRAIN-*).
'use strict';
const { base } = require('../world');

module.exports = [{
    name: 'room-basics',
    tags: ['game/rooms.js:419', 'game/rooms.js:536', 'game/rooms.js:540', 'game/rooms.js:551', 'game/rooms.js:573', 'game/rooms.js:971'],
    world: base({ games: { W1N2: { id: 'g1', mode: 'survival' } } }),
    ticks: [{ probes: [
        ['fields', () => {
            const r = Game.rooms.W1N1;
            return [Object.keys(r), r.name, r.energyAvailable, r.energyCapacityAvailable, r.survivalInfo, Game.rooms.W1N2.survivalInfo,
                Object.getOwnPropertyDescriptor(r, 'name'), String(r), JSON.stringify(r), JSON.stringify(Game.rooms.W1N2), r.controller.id,
                r.storage, 'storage' in r, Game.rooms.W1N2.energyAvailable, Object.keys(Room.prototype)];
        }],
        ['name readonly', () => { const r = Game.rooms.W1N1; r.name = 'X'; return r.name; }],
        ['memory', () => {
            const r = Game.rooms.W1N1;
            const a = r.memory, b = Game.rooms.W1N2.memory;
            Game.rooms.W1N2.memory = 0;
            const c = Game.rooms.W1N2.memory;
            return [a, b, c, JSON.stringify(Memory.rooms)];
        }],
        ['event log', () => {
            const r = Game.rooms.W1N1, a = r.getEventLog(), b = r.getEventLog();
            a.push('x');
            return [a === b, r.getEventLog(true), r.getEventLog(1), Game.rooms.W1N2.getEventLog(), Game.rooms.W1N2.getEventLog(true), r.getEventLog().length];
        }],
        ['getPositionAt', () => {
            const r = Game.rooms.W1N1, out = [];
            for (const a of [[0, 0], [49, 49], [50, 0], [-1, 5], [undefined, undefined], ['3', '4'], [null, 2]]) out.push(r.getPositionAt(...a));
            try { out.push(r.getPositionAt(NaN, 1)); } catch (e) { out.push(e.message); }
            return out;
        }],
        ['paths', () => {
            const path = Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(30, 30, 'W1N1'));
            const s = Room.serializePath(path), out = [s, Room.deserializePath(s), Room.serializePath([]), Room.deserializePath('')];
            for (const a of [[{x: 5, y: 5, direction: 1}, {direction: undefined}], [{x: -1, y: 0}], 'str', [{x: 10, y: 3, direction: 3}]]) {
                try { out.push(Room.serializePath(a)); } catch (e) { out.push(e.message); }
            }
            for (const a of ['1010', '0505123', 'xx05', '0505', '05050', '05059', '0505a', 5, '1212' + '8'.repeat(5)]) {
                try { out.push(Room.deserializePath(a)); } catch (e) { out.push(e.message); }
            }
            return out;
        }],
    ] }],
}, {
    name: 'room-visual',
    tags: ['game/rooms.js:1146', 'game/rooms.js:1150', 'game/rooms.js:1184', 'game/rooms.js:1215'],
    world: base(),
    ticks: [{ probes: [
        ['draw', () => {
            const v = Game.rooms.W1N1.visual, p = new RoomPosition(1, 2, 'W1N1');
            v.circle(1, 2).circle(p, {radius: 2}).line(1, 2, 3, 4).line(p, new RoomPosition(5, 6, 'W1N1'), {color: 'red'})
                .rect(1, 1, 2, 2).rect(p, 3, 4, {opacity: 1}).poly([p, [1, 1], {x: 3, y: 4}]).poly([]).poly([0, null]).poly('x')
                .text('hi', 5, 5).text('pos', p, {font: 1}).text(7, Game.creeps.Harv1);
            return [v.getSize(), v.export(), Object.keys(v), v.roomName];
        }],
        ['others', () => {
            const g = new RoomVisual(), m = new RoomVisual('map'), o = new RoomVisual('W5N5');
            g.circle(1, 1); m.import('abc'); o.import(''); o.import(5);
            const r = [g.export(), m.export(), o.export(), o.getSize(), o.clear() === o, o.export(), new RoomVisual('none').getSize()];
            try { new RoomVisual('W1N1').circle(null); } catch (e) { r.push(e.constructor.name); }
            return r;
        }],
    ] }],
}, {
    name: 'room-terrain',
    tags: ['game/rooms.js:1134', 'game/rooms.js:1223'],
    world: base(),
    ticks: [{ probes: [
        ['get', () => {
            const t = new Room.Terrain('W1N1'), out = [];
            for (let y = 0; y < 50; y += 7) for (let x = 0; x < 50; x += 3) out.push(t.get(x, y));
            out.push(t.get(50, 0), t.get(0, 50), t.get(-1, 0), t.get(1.5, 2), t.get('3', '4'), t.get(), Object.keys(t), typeof t.get, t instanceof Room.Terrain);
            return out;
        }],
        ['raw', () => {
            const t = Game.rooms.W1N2.getTerrain(), a = t.getRawBuffer(), b = new Uint8Array(2500), c = new Uint16Array(2600), out = [];
            out.push(a, t.getRawBuffer(b) === b, b.every((v, i) => v === a[i]), t.getRawBuffer(c) === c, c[2499], c[2500], a === t.getRawBuffer());
            for (const d of [[], {}, new Uint8Array(10), 'x', 1]) { try { out.push(t.getRawBuffer(d)); } catch (e) { out.push(e.constructor.name); } }
            return out;
        }],
        ['ctor', () => {
            const out = [];
            for (const n of ['W2N1', 'E2S2', 'W9N9', 'sim', undefined, 'w1n1', {toString: () => 'W1N1'}, 'constructor', 'toString', '__proto__']) {
                try { out.push(new Room.Terrain(n).get(0, 0)); } catch (e) { out.push(e.message); }
            }
            try { Room.Terrain('W1N1'); } catch (e) { out.push(e.constructor.name); }
            out.push(Game.rooms.W1N1.getTerrain() !== Game.rooms.W1N1.getTerrain());
            return out;
        }],
        ['wall+swamp code', () => {
            const t = new Room.Terrain('W1N1'), raw = t.getRawBuffer(), out = [];
            for (let i = 0; i < 2500; i++) if (raw[i] === 3) out.push([i, t.get(i % 50, i / 50 | 0), new RoomPosition(i % 50, i / 50 | 0, 'W1N1').lookFor('terrain')]);
            return out;
        }],
    ] }],
}];
