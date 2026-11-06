// Pathfinding through the PathFinder wrapper: Room.findPath, RoomPosition.findPathTo / findClosestByPath,
// PathFinder.search (driver option coercion, goals, callbacks, results) and CostMatrix
// (spec 03 ROOM-FINDPATH, ROOM-DEFAULT-COSTMATRIX, POS-FINDPATHTO, POS-FINDCLOSESTBYPATH, PF-*, CM-*).
'use strict';
const { base } = require('../world');

module.exports = [{
    name: 'path-findpath',
    tags: ['game/rooms.js:157', 'game/rooms.js:198', 'game/rooms.js:223', 'game/rooms.js:863', 'game/rooms.js:1396',
        'game/path-finder.js:57', 'lib/path-finder.js:86'],
    world: base(),
    ticks: [{ probes: [
        ['same room', () => {
            const r = Game.rooms.W1N1, from = new RoomPosition(25, 25, 'W1N1');
            return [r.findPath(from, new RoomPosition(30, 26, 'W1N1')), r.findPath(from, new RoomPosition(26, 26, 'W1N1')),
                r.findPath(from, new RoomPosition(25, 25, 'W1N1')), r.findPath(from, Game.getObjectById('s1').pos),
                r.findPath(from, new RoomPosition(21, 21, 'W1N1'), {serialize: true}), r.findPath(from, new RoomPosition(30, 26, 'W1N1'), {range: 2}),
                r.findPath(from, new RoomPosition(28, 25, 'W1N1'), {ignoreCreeps: true}), r.findPath(from, new RoomPosition(28, 25, 'W1N1')),
                r.findPath(from, new RoomPosition(24, 28, 'W1N1'), {ignoreRoads: true}), r.findPath(from, new RoomPosition(24, 28, 'W1N1'))];
        }],
        ['options', () => {
            const r = Game.rooms.W1N1, from = new RoomPosition(25, 25, 'W1N1'), to = new RoomPosition(29, 21, 'W1N1');
            const seen = [];
            const opts = {costCallback: (rn, m) => { seen.push(rn, m.get(25, 27), m.get(21, 21), m.get(24, 26), m.get(27, 24)); m.set(27, 23, 255); }};
            const res = [r.findPath(from, to, opts), seen, r.findPath(from, to, {plainCost: 3, swampCost: 3}),
                r.findPath(from, to, {costCallback: () => new PathFinder.CostMatrix()}), r.findPath(from, to, {maxOps: 5}),
                r.findPath(from, to, {ignoreDestructibleStructures: true}), r.findPath(from, to, {heuristicWeight: 9, flee: true, maxCost: 1})];
            const o2 = {avoid: [], ignore: [from], maxRooms: 1};
            res.push(r.findPath(from, to, o2), Object.keys(o2), o2.maxOps);
            return res;
        }],
        ['opts mutation', () => {
            const r = Game.rooms.W1N1, from = new RoomPosition(25, 25, 'W1N1');
            const o = {avoid: [new RoomPosition(26, 26, 'W1N1')]};
            r.findPath(from, new RoomPosition(25, 10, 'W1N2'), o);
            const o3 = {};
            from.findPathTo(new RoomPosition(25, 10, 'W1N2'), o3);
            const o4 = {};
            from.findPathTo(25, 10, o4);
            return [o, o3, o4];
        }],
        ['cross room', () => {
            const r = Game.rooms.W1N1, from = new RoomPosition(25, 25, 'W1N1');
            return [r.findPath(from, new RoomPosition(25, 25, 'W1N2')), r.findPath(from, new RoomPosition(25, 25, 'W0N1'), {maxRooms: 1}),
                r.findPath(from, new RoomPosition(25, 25, 'W2N1')), r.findPath(new RoomPosition(25, 25, 'W1N2'), from, {}),
                (() => { try { return r.findPath(new RoomPosition(25, 25, 'W1N2'), from); } catch (e) { return e.constructor.name; } })()];
        }],
        ['findPathTo', () => {
            const p = Game.creeps.Harv1.pos;
            return [p.findPathTo(30, 26), p.findPathTo(Game.getObjectById('s1')), p.findPathTo(new RoomPosition(30, 26, 'W1N1'), {serialize: true}),
                p.findPathTo({x: 30, y: 26}), p.findPathTo(30, 26, {ignoreCreeps: true}), p.findPathTo(Game.creeps.Scout),
                Game.creeps.Scout.pos.findPathTo(Game.creeps.Harv1, {maxRooms: 2})];
        }],
        ['callback rooms', () => {
            const seen = [];
            Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(25, 25, 'E0N1'), {costCallback: (rn, m) => { seen.push(rn); }});
            return seen;
        }],
        ['deprecated', () => { Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(28, 28, 'W1N1'), {avoid: [1], ignore: [2]}); return 1; }],
    ] }],
}, {
    name: 'path-closest',
    tags: ['game/rooms.js:304', 'game/rooms.js:1429'],
    world: base(),
    ticks: [{ probes: [
        ['by type', () => {
            const p = Game.creeps.Harv1.pos;
            return [p.findClosestByPath(FIND_SOURCES), p.findClosestByPath(FIND_STRUCTURES), p.findClosestByPath(FIND_MY_STRUCTURES, {filter: {structureType: 'extension'}}),
                p.findClosestByPath(FIND_DROPPED_RESOURCES), p.findClosestByPath(FIND_HOSTILE_CREEPS), p.findClosestByPath(FIND_NUKES),
                p.findClosestByPath(), p.findClosestByPath(FIND_EXIT), p.findClosestByPath(FIND_SOURCES_ACTIVE, {ignoreCreeps: true})];
        }],
        ['by array', () => {
            const p = Game.creeps.Harv1.pos;
            const a = [new RoomPosition(26, 24, 'W1N1'), new RoomPosition(24, 24, 'W1N1'), new RoomPosition(25, 24, 'W1N1')];
            return [p.findClosestByPath(a), p.findClosestByPath(a.slice().reverse()), p.findClosestByPath([new RoomPosition(25, 25, 'W1N1'), a[0]]),
                p.findClosestByPath([Game.getObjectById('s1'), Game.getObjectById('m1')]), p.findClosestByPath([]),
                p.findClosestByPath([Game.getObjectById('s1')], {filter: o => false}), p.findClosestByPath([new RoomPosition(30, 30, 'W1N2')]),
                p.findClosestByPath(a, {costCallback: (rn, m) => { m.set(24, 24, 255); m.set(26, 24, 255); }}),
                (() => { try { return p.findClosestByPath([null]); } catch (e) { return e.constructor.name; } })(),
                (() => { try { return p.findClosestByPath([{x: 1, y: 1}]); } catch (e) { return e.message; } })()];
        }],
    ] }],
}, {
    name: 'path-pathfinder',
    tags: ['game/path-finder.js:18', 'game/path-finder.js:57', 'game/path-finder.js:67', 'lib/path-finder.js:59', 'lib/path-finder.js:86'],
    world: base(),
    ticks: [{ probes: [
        ['search', () => {
            const from = new RoomPosition(25, 25, 'W1N1');
            const res = r => [r.path, r.ops, r.cost, r.incomplete, Object.keys(r)];
            return [res(PathFinder.search(from, new RoomPosition(30, 30, 'W1N1'))), res(PathFinder.search(from, {pos: new RoomPosition(30, 30, 'W1N1'), range: 3})),
                res(PathFinder.search(from, [{pos: new RoomPosition(30, 30, 'W1N1'), range: 1}, new RoomPosition(20, 22, 'W1N1')])),
                res(PathFinder.search(from, Game.getObjectById('s1'))), PathFinder.search(from, []), PathFinder.search(from), PathFinder.search(from, null),
                res(PathFinder.search(from, from)), res(PathFinder.search(from, {pos: new RoomPosition(30, 30, 'W1N1'), range: 5}, {flee: true})),
                res(PathFinder.search(from, new RoomPosition(25, 25, 'W1N2'), {maxOps: 50})), res(PathFinder.search(from, new RoomPosition(25, 25, 'W1N2'), {maxRooms: 1})),
                res(PathFinder.search(from, new RoomPosition(25, 25, 'W0N1'), {roomCallback: rn => rn == 'W1N1' ? undefined : false})),
                res(PathFinder.search(from, new RoomPosition(30, 30, 'W1N1'), {roomCallback: () => false})),
                res(PathFinder.search(from, new RoomPosition(30, 30, 'W1N1'), {plainCost: 300, swampCost: 0.5, heuristicWeight: 20, maxCost: 3e9})),
                res(PathFinder.search(from, new RoomPosition(30, 30, 'W1N1'), {maxCost: 4})),
                res(PathFinder.search(from, new RoomPosition(30, 30, 'W1N1'), {roomCallback: 5, maxOps: Infinity}))];
        }],
        ['search errors', () => {
            const from = new RoomPosition(25, 25, 'W1N1'), out = [];
            for (const f of [() => PathFinder.search(Game.creeps.Harv1, from), () => PathFinder.search({x: 1, y: 1, roomName: 'sim'}, from),
                () => PathFinder.search({x: 50, y: 1, roomName: 'W1N1'}, from), () => PathFinder.search(from, {x: 1, y: 1}),
                () => PathFinder.search(from, new RoomPosition(1, 1, 'W1N1'), {roomCallback: () => { throw new Error('cb boom'); }}),
                () => PathFinder.search(from, {range: 1}), () => PathFinder.search(from, [{pos: from, range: -3}]),
                () => PathFinder.search({x: 1, y: 1, roomName: 'W200N1'}, from)]) {
                try { const r = f(); out.push([r.path, r.ops, r.incomplete]); } catch (e) { out.push(e.message); }
            }
            return out;
        }],
        ['callback matrix', () => {
            const from = new RoomPosition(25, 25, 'W1N1'), m = new PathFinder.CostMatrix();
            for (let y = 0; y < 50; y++) m.set(27, y, 255);
            const r1 = PathFinder.search(from, new RoomPosition(30, 25, 'W1N1'), {roomCallback: () => m});
            const r2 = PathFinder.search(from, new RoomPosition(30, 25, 'W1N1'), {roomCallback: () => ({_bits: m._bits})});
            return [r1.path, r1.incomplete, r2.path.length];
        }],
        ['costmatrix', () => {
            const m = new PathFinder.CostMatrix();
            m.set(1, 2, 3); m.set(0, 50, 7); m.set(49, 49, 300); m.set(2, 2, -5); m.set(3, 3, 'x'); m.set(4, 4, 2.7); m.set(5, 5, '9');
            m.set(60, 60, 1); m.set(-1, 0, 4);
            const c = m.clone();
            c.set(1, 2, 9);
            const s = m.serialize();
            const d = PathFinder.CostMatrix.deserialize(s), d2 = PathFinder.CostMatrix.deserialize([1, 2, 256]);
            return [m.get(1, 2), m.get(1, 0), m.get(49, 49), m.get(2, 2), m.get(3, 3), m.get(4, 4), m.get(5, 5), m.get(60, 60), m.get(-1, 0), m.get(1.9, 2.2),
                c.get(1, 2), m.get(1, 2), s.length, s.slice(0, 4), s[624], d.get(1, 2), d instanceof PathFinder.CostMatrix, Object.keys(d), d2._bits,
                Object.keys(m), Object.keys(PathFinder), Object.keys(PathFinder.CostMatrix.prototype), m.set(1, 1, 1)];
        }],
        ['use', () => {
            PathFinder.use(true);
            const a = Game.rooms.W1N1.findPath(new RoomPosition(25, 25, 'W1N1'), new RoomPosition(28, 28, 'W1N1')).length;
            PathFinder.use(false);
            PathFinder.use(0);
            PathFinder.use(1);
            return a;
        }],
    ] }],
}];
