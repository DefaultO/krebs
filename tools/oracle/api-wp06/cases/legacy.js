// PathFinder.use(false): the legacy @screeps/pathfinding paths of rooms.js (grid build, avoid/ignore position-set
// cache, path cache, Dijkstra end nodes, findClosestByPath algorithm choice) (spec 03 ROOM-FINDPATH-LEGACY,
// POS-FINDCLOSESTBYPATH-LEGACY, ROOM-GETENDNODES).
'use strict';
const { base } = require('../world');

module.exports = [{
    name: 'legacy-findpath',
    tags: ['game/rooms.js:16', 'game/rooms.js:32', 'game/rooms.js:130', 'game/rooms.js:873', 'game/rooms.js:1099', 'game/rooms.js:1447'],
    world: base(),
    ticks: [{ probes: [
        ['use', () => { PathFinder.use(false); return 1; }],
        ['findPath', () => {
            const r = Game.rooms.W1N1, from = new RoomPosition(25, 25, 'W1N1');
            const a = r.findPath(from, new RoomPosition(30, 30, 'W1N1'));
            const b = r.findPath(from, new RoomPosition(30, 30, 'W1N1'));
            a.push('junk');
            return [a, b === a, r.findPath(from, new RoomPosition(30, 30, 'W1N1'), {serialize: true}), r.findPath(from, new RoomPosition(26, 26, 'W1N1')),
                r.findPath(from, new RoomPosition(25, 25, 'W1N1')), r.findPath(from, new RoomPosition(25, 25, 'W1N2')),
                r.findPath(from, new RoomPosition(21, 21, 'W1N1')), r.findPath(from, new RoomPosition(28, 25, 'W1N1'), {ignoreCreeps: true}),
                r.findPath(from, new RoomPosition(29, 21, 'W1N1'), {maxOps: 3}), r.findPath(from, new RoomPosition(29, 21, 'W1N1'), {heuristicWeight: 5}),
                r.findPath(from, new RoomPosition(25, 29, 'W1N1'), {ignoreDestructibleStructures: true})];
        }],
        ['avoid/ignore', () => {
            const r = Game.rooms.W1N1, from = new RoomPosition(25, 25, 'W1N1'), to = new RoomPosition(30, 25, 'W1N1');
            const o = {avoid: [new RoomPosition(26, 25, 'W1N1'), {x: 26, y: 24}, Game.creeps.Harv1], ignore: [Game.getObjectById('cr2'), {x: 27, y: 25}]};
            const res = [r.findPath(from, to, o), o.avoid, o.ignore, r.findPath(from, to, {avoid: [{x: 26, y: 24}, new RoomPosition(26, 25, 'W1N1')]})];
            for (const bad of [{avoid: 'x'}, {ignore: 5}, {avoid: [5]}, {avoid: [null]}, {ignore: [null, 0]}]) { try { res.push(r.findPath(from, to, bad)); } catch (e) { res.push(e.message); } }
            return res;
        }],
        ['findPathTo', () => {
            const p = Game.creeps.Harv1.pos;
            return [p.findPathTo(30, 30), p.findPathTo(new RoomPosition(10, 10, 'W1N2')), p.findPathTo(new RoomPosition(10, 10, 'W0N1'), {serialize: true}),
                p.findPathTo(new RoomPosition(10, 10, 'W2N2')), p.findPathTo(Game.creeps.Scout)];
        }],
        ['closest', () => {
            const p = Game.creeps.Harv1.pos;
            return [p.findClosestByPath(FIND_SOURCES), p.findClosestByPath(FIND_STRUCTURES), p.findClosestByPath(FIND_EXIT_TOP),
                p.findClosestByPath(FIND_SOURCES, {algorithm: 'dijkstra'}), p.findClosestByPath(FIND_SOURCES, {algorithm: 'astar'}),
                p.findClosestByPath(FIND_SOURCES, {filter: s => s.energy > 0}), p.findClosestByPath([new RoomPosition(30, 30, 'W1N1'), new RoomPosition(20, 22, 'W1N1')]),
                p.findClosestByPath([Game.getObjectById('m1'), Game.getObjectById('s2')], {algorithm: 'dijkstra'}), p.findClosestByPath(FIND_CREEPS),
                p.findClosestByPath([{x: 25, y: 26}]), p.findClosestByPath(FIND_NUKES), p.findClosestByPath(FIND_EXIT, {algorithm: 'other'})];
        }],
        ['endNodes', () => {
            const r = Game.rooms.W1N1, out = [];
            out.push(r.getEndNodes(FIND_SOURCES), r.getEndNodes(FIND_SOURCES, {filter: s => s.energy}), r.getEndNodes([new RoomPosition(1, 1, 'W1N1')]),
                r.getEndNodes([new RoomPosition(1, 1, 'W1N1')]), r.getEndNodes('str'));
            try { r.getEndNodes(); } catch (e) { out.push(e.message); }
            out.push(r.findPath(new RoomPosition(25, 25, 'W1N1'), 105), r.findPath(new RoomPosition(25, 25, 'W1N1'), 3), r.findPath(new RoomPosition(25, 25, 'W1N1'), 1));
            return out;
        }],
    ] }],
}];
