// RoomPosition: constructor coercion and packing, accessors, toJSON/toString, range/near/equal/direction helpers,
// findInRange / findClosestByRange, invisible-room throws (spec 03 POS-*, ROOMOBJ-*).
'use strict';
const { base } = require('../world');

module.exports = [{
    name: 'position-basics',
    tags: ['game/rooms.js:1245', 'game/rooms.js:1276', 'game/rooms.js:1295', 'game/rooms.js:1350', 'game/rooms.js:1358'],
    world: base(),
    ticks: [{ probes: [
        ['ctor', () => {
            const out = [];
            for (const a of [[1, 2, 'W1N1'], [undefined, undefined, 'W1N1'], [null, true, 'E0S0'], ['5', 1.7, 'w1n1'], [0, 0, 'E5S5junk'],
                [49, 49, 'W127N127'], [0, 0, 'E127S127'], [0, 0, 'E128S0'], [0, 0, 'W128N128'], [0, 0, 'W129N0'], [1, 1, 'sim'],
                [50, 1, 'W1N1'], [-1, 1, 'W1N1'], [NaN, 1, 'W1N1'], [1, 1, 'X'], [1, 1, ''], [1, 1, 5], [1, 1, undefined], [1, 1, {}]]) {
                try { const p = new RoomPosition(...a); out.push([p.x, p.y, p.roomName, p.__packedPos]); } catch (e) { out.push(e.constructor.name + ': ' + e.message); }
            }
            return out;
        }],
        ['props', () => {
            const p = new RoomPosition(10, 20, 'E3S7');
            const d = ['x', 'y', 'roomName'].map(k => Object.getOwnPropertyDescriptor(RoomPosition.prototype, k));
            return [Object.keys(p), Object.getOwnPropertyDescriptor(p, '__packedPos'), d.map(x => [x.enumerable, x.configurable, typeof x.get, typeof x.set]),
                JSON.stringify(p), String(p), p.toJSON(), Object.keys(RoomPosition.prototype)];
        }],
        ['setters', () => {
            const p = new RoomPosition(10, 20, 'E3S7'), out = [];
            for (const [k, v] of [['x', 5], ['y', '7'], ['x', 50], ['y', -1], ['x', NaN], ['roomName', 'W5N5'], ['roomName', 'bad'], ['roomName', 'sim'], ['roomName', 'E3S7'], ['x', 3.9]]) {
                try { p[k] = v; out.push([p.x, p.y, p.roomName, p.__packedPos]); } catch (e) { out.push(e.message); }
            }
            p.extra = 1;
            out.push(JSON.stringify(p), p.toJSON());
            return out;
        }],
        ['packed edits', () => {
            const p = new RoomPosition(1, 1, 'W1N1');
            p.__packedPos = 0;
            const q = new RoomPosition(1, 1, 'W1N1');
            q.__packedPos = -1;
            return [p.roomName, p.x, p.y, q.roomName, q.x, q.y, String(q)];
        }],
        ['room object', () => {
            const c = Game.creeps.Harv1, s = Game.getObjectById('s1');
            return [Object.keys(c).slice(0, 3), c.room === Game.rooms.W1N1, c.pos instanceof RoomPosition, Object.keys(s), s.effects,
                Game.flags.Far.room, Object.keys(Game.flags.Far), Game.getObjectById('cs3').room];
        }],
    ] }],
}, {
    name: 'position-relations',
    tags: ['game/rooms.js:1362', 'game/rooms.js:1378', 'game/rooms.js:1383', 'game/rooms.js:1578', 'game/rooms.js:1586',
        'game/rooms.js:1518', 'game/rooms.js:1546'],
    world: base(),
    ticks: [{ probes: [
        ['inRangeTo', () => {
            const p = new RoomPosition(25, 25, 'W1N1');
            return [p.inRangeTo(27, 27, 2), p.inRangeTo(27, 28, 2), p.inRangeTo(Game.creeps.Harv1, 0), p.inRangeTo({x: 26, y: 26, roomName: 'W1N1'}, 1),
                p.inRangeTo({x: 26, y: 26}, 1), p.inRangeTo(new RoomPosition(25, 25, 'W1N2'), 5), p.inRangeTo(25, 25), p.inRangeTo(Game.creeps.Harv1),
                p.inRangeTo(Game.creeps.Harv1, '1'), (() => { try { return p.inRangeTo(undefined, 1); } catch (e) { return e.constructor.name; } })()];
        }],
        ['isNearTo', () => {
            const p = new RoomPosition(25, 25, 'W1N1');
            return [p.isNearTo(26, 26), p.isNearTo(27, 26), p.isNearTo(25, 25), p.isNearTo(Game.getObjectById('cr2')), p.isNearTo(Game.getObjectById('sp1')),
                p.isNearTo({x: 25, y: 25, roomName: 'W1N1'}), p.isNearTo(new RoomPosition(25, 25, 'W1N2')), p.isNearTo('26', 26), p.isNearTo(26, '26'), p.isNearTo()];
        }],
        ['isEqualTo', () => {
            const p = new RoomPosition(25, 25, 'W1N1');
            return [p.isEqualTo(25, 25), p.isEqualTo(Game.creeps.Harv1), p.isEqualTo(Game.creeps.Harv1.pos), p.isEqualTo({__packedPos: p.__packedPos}),
                p.isEqualTo({x: 25, y: 25}), p.isEqualTo('25', 25), p.isEqualTo(new RoomPosition(25, 25, 'W1N2')),
                (() => { try { return p.isEqualTo(null); } catch (e) { return e.constructor.name; } })(),
                (() => { try { return p.isEqualTo(); } catch (e) { return e.constructor.name; } })()];
        }],
        ['getRangeTo', () => {
            const p = new RoomPosition(25, 25, 'W1N1');
            return [p.getRangeTo(30, 20), p.getRangeTo(Game.getObjectById('cr2')), p.getRangeTo(new RoomPosition(25, 25, 'W1N2')), p.getRangeTo({x: 1, y: 1}),
                p.getRangeTo('1', 1), p.getRangeTo(1, '1'), p.getRangeTo(NaN, 1)];
        }],
        ['getDirectionTo', () => {
            const p = new RoomPosition(25, 25, 'W1N1'), out = [];
            for (let dx = -2; dx <= 2; dx++) for (let dy = -2; dy <= 2; dy++) out.push(p.getDirectionTo(25 + dx, 25 + dy));
            out.push(p.getDirectionTo(new RoomPosition(0, 25, 'W0N1')), p.getDirectionTo(new RoomPosition(25, 49, 'W1N2')),
                p.getDirectionTo(new RoomPosition(49, 0, 'W2N2')), p.getDirectionTo(Game.creeps.Scout), p.getDirectionTo(Game.creeps.Harv1));
            return out;
        }],
        ['findInRange', () => {
            const p = new RoomPosition(25, 25, 'W1N1');
            return [p.findInRange(FIND_CREEPS, 2), p.findInRange(FIND_STRUCTURES, 3, {filter: {structureType: 'road'}}),
                p.findInRange([Game.getObjectById('cr2'), new RoomPosition(25, 26, 'W1N1'), {x: 25, y: 24, roomName: 'W1N1'}, Game.creeps.Scout], 2),
                p.findInRange([Game.getObjectById('cr2'), Game.creeps.Harv1], 5, {filter: 'my'}), p.findInRange('x', 5), p.findInRange(FIND_SOURCES)];
        }],
        ['findClosestByRange', () => {
            const p = new RoomPosition(25, 25, 'W1N1');
            return [p.findClosestByRange(FIND_SOURCES), p.findClosestByRange(FIND_STRUCTURES), p.findClosestByRange(FIND_DROPPED_RESOURCES),
                p.findClosestByRange([new RoomPosition(27, 27, 'W1N1'), new RoomPosition(23, 23, 'W1N1')]),
                p.findClosestByRange([Game.creeps.Scout]), p.findClosestByRange([{x: 1}, {}]), p.findClosestByRange(FIND_MY_CREEPS, {filter: c => c.name != 'Harv1'}),
                p.findClosestByRange(FIND_HOSTILE_CREEPS), p.findClosestByRange(FIND_NUKES)];
        }],
        ['invisible throws', () => {
            const p = new RoomPosition(25, 25, 'W2N1'), out = [];
            for (const f of [() => p.findInRange([], 1), () => p.findClosestByRange([]), () => p.findClosestByPath([]), () => p.findPathTo(1, 1),
                () => p.createFlag('x'), () => p.createConstructionSite('road')]) {
                try { out.push(f()); } catch (e) { out.push(e.message); }
            }
            return out;
        }],
    ] }],
}, {
    // cross-room getDirectionTo near the getDirection 2:1 thresholds, and inRangeTo's loose roomName ==
    name: 'position-direction-sweep',
    tags: ['game/rooms.js:1383', 'game/rooms.js:1362'],
    world: base(),
    ticks: [{ probes: [
        ['cross-room', () => {
            const p = new RoomPosition(25, 25, 'W1N1'), out = [];
            for (const [room, horizontal] of [['W0N1', true], ['W2N1', true], ['W1N0', false], ['W1N2', false], ['W0N0', true], ['W2N2', false]]) {
                const s = [];
                for (let i = 0; i < 50; i++) s.push(p.getDirectionTo(horizontal ? new RoomPosition(0, i, room) : new RoomPosition(i, 0, room)));
                out.push(room + ':' + s.join(''));
            }
            return out;
        }],
        ['loose roomName', () => {
            const p = new RoomPosition(25, 25, 'W1N1'), r = {toString: () => 'W1N1'};
            return [p.inRangeTo({x: 26, y: 26, roomName: r}, 1), p.inRangeTo({x: 26, y: 26, roomName: ['W1N1']}, 1), p.isNearTo({x: 26, y: 26, roomName: r}),
                p.getRangeTo({x: 26, y: 26, roomName: r}), p.isEqualTo({x: 25, y: 25, roomName: r}), p.getDirectionTo({x: 26, y: 26, roomName: r})];
        }],
    ] }],
}];
