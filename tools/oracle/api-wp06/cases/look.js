// Room.lookAt / lookForAt / lookAtArea / lookForAtArea, RoomPosition.look / lookFor (spec 03 ROOM-LOOK*, LOOK-*).
'use strict';
const { base } = require('../world');

// Many roads so that lookAtArea switches the structure type to the spatial strategy on small areas.
const roads = [];
for (let x = 20; x <= 30; x++) roads.push({ _id: 'rd' + x, type: 'road', room: 'W1N1', x, y: 30, hits: 5000, hitsMax: 5000 });

module.exports = [{
    name: 'look-at',
    tags: ['game/rooms.js:659', 'game/rooms.js:768', 'game/rooms.js:798', 'game/rooms.js:1594', 'game/rooms.js:1602'],
    world: base(),
    ticks: [{ probes: [
        ['lookAt xy', () => [Game.rooms.W1N1.lookAt(27, 25), Game.rooms.W1N1.lookAt(24, 26), Game.rooms.W1N1.lookAt(27, 24)]],
        ['lookAt pos/obj', () => [Game.rooms.W1N1.lookAt(new RoomPosition(25, 25, 'W1N1')), Game.rooms.W1N1.lookAt(Game.creeps.Harv1),
            Game.rooms.W1N1.lookAt(new RoomPosition(25, 25, 'W1N2'))]],
        ['lookAt flags/terrain', () => [Game.rooms.W1N1.lookAt(20, 20), Game.rooms.W1N1.lookAt(0, 10), Game.rooms.W1N1.lookAt(0, 0)]],
        ['lookAt bad', () => {
            const r = Game.rooms.W1N1, out = [];
            for (const a of [[50, 1], [-1, 3], [1, 50], [{x: 1, y: 1}], [], ['5', '5'], [NaN, 3], [5.5, 5]]) {
                try { out.push(r.lookAt(...a)); } catch (e) { out.push('throw ' + e.message); }
            }
            return out;
        }],
        ['lookForAt', () => {
            const r = Game.rooms.W1N1;
            return [LOOK_CREEPS, LOOK_ENERGY, LOOK_RESOURCES, LOOK_STRUCTURES, LOOK_TERRAIN, LOOK_FLAGS, LOOK_CONSTRUCTION_SITES,
                LOOK_SOURCES, LOOK_MINERALS, 'nope', 'toString', undefined].map(t => r.lookForAt(t, 27, 25))
                .concat([r.lookForAt(LOOK_SOURCES, Game.getObjectById('s1')), r.lookForAt(LOOK_CREEPS, Game.spawns.Spawn1.pos),
                    r.lookForAt(LOOK_TERRAIN, 50, 0), r.lookForAt(LOOK_TERRAIN, 0, 50), r.lookForAt(LOOK_TERRAIN, 1.5, 2)]);
        }],
        ['lookForAt oob', () => { try { return Game.rooms.W1N1.lookForAt(LOOK_CREEPS, 50, 0); } catch (e) { return e.message; } }],
        ['lookForAt shared', () => {
            const r = Game.rooms.W1N1, a = r.lookForAt(LOOK_ENERGY, 27, 25);
            a.pop();
            return [a.length, r.lookForAt(LOOK_ENERGY, 27, 25).length];
        }],
        ['pos.look', () => [new RoomPosition(25, 25, 'W1N1').look(), new RoomPosition(25, 25, 'W1N1').lookFor(LOOK_CREEPS),
            new RoomPosition(0, 0, 'W2N1').lookFor(LOOK_TERRAIN), new RoomPosition(22, 0, 'W1N1').lookFor('terrain'),
            new RoomPosition(25, 25, 'W1N1').lookFor('bogus')]],
        ['pos.look invisible', () => {
            const out = [];
            for (const f of [p => p.look(), p => p.lookFor(LOOK_CREEPS)]) {
                try { out.push(f(new RoomPosition(25, 25, 'W2N1'))); } catch (e) { out.push(e.message); }
            }
            return out;
        }],
    ] }],
}, {
    name: 'look-area',
    tags: ['game/rooms.js:707', 'game/rooms.js:808', 'game/rooms.js:847'],
    world: base({ objects: base().objects.concat(roads) }),
    ticks: [{ probes: [
        ['area nested', () => Game.rooms.W1N1.lookAtArea(24, 24, 26, 27)],
        ['area array', () => Game.rooms.W1N1.lookAtArea(24, 24, 26, 27, true)],
        ['area big', () => Game.rooms.W1N1.lookAtArea(18, 18, 31, 31, true).filter(i => i.type != 'terrain')],
        ['area roads small', () => [Game.rooms.W1N1.lookAtArea(29, 22, 31, 24, true), Game.rooms.W1N1.lookAtArea(29, 22, 31, 24)]],
        ['area edge', () => {
            const r = Game.rooms.W1N1, out = [];
            for (const a of [[-1, -1, 1, 1], [29, 48, 31, 50], [48, 0, 50, 1]]) {
                try { out.push(r.lookAtArea(...a, true).length); } catch (e) { out.push(e.message); }
                try { out.push(Object.keys(r.lookAtArea(...a))); } catch (e) { out.push(e.message); }
            }
            const r2 = Game.rooms.W1N2;
            out.push(r2.lookAtArea(-1, -1, 1, 1, true), r2.lookAtArea(48, 48, 50, 50), r2.lookForAtArea(LOOK_STRUCTURES, -5, -5, 60, 60, true));
            return out;
        }],
        ['for area', () => {
            const r = Game.rooms.W1N1;
            return [r.lookForAtArea(LOOK_CREEPS, 24, 24, 26, 27), r.lookForAtArea(LOOK_CREEPS, 24, 24, 26, 27, true),
                r.lookForAtArea(LOOK_STRUCTURES, 29, 22, 31, 24), r.lookForAtArea(LOOK_STRUCTURES, 29, 22, 31, 24, true),
                r.lookForAtArea(LOOK_TERRAIN, 0, 20, 2, 23), r.lookForAtArea(LOOK_TERRAIN, 0, 20, 2, 23, true),
                r.lookForAtArea(LOOK_ENERGY, 20, 20, 30, 30, true), r.lookForAtArea(LOOK_FLAGS, 0, 0, 49, 49)];
        }],
        ['for area bad type', () => { try { return Game.rooms.W1N1.lookForAtArea('nope', 1, 1, 2, 2); } catch (e) { return e.constructor.name; } }],
        ['for area single', () => [Game.rooms.W1N1.lookForAtArea(LOOK_SOURCES, 24, 22, 24, 22, true), Game.rooms.W1N1.lookForAtArea(LOOK_SOURCES, 25, 25, 24, 24)]],
    ] }],
}];
