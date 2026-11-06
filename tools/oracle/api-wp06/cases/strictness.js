// Each port keeps its vanilla file's strictness (ENV-ENGINE-STRICTNESS): sloppy methods see the global object as
// `this` when called unbound, strict ones see undefined.
'use strict';
const { base } = require('../world');

module.exports = [{
    name: 'strictness',
    tags: ['game/rooms.js:536', 'game/rooms.js:1223', 'game/path-finder.js:18', 'game/flags.js:53', 'game/sources.js:41',
        'game/minerals.js:42', 'game/construction-sites.js:46', 'game/map.js:188'],
    world: base(),
    ticks: [{ probes: [
        ['unbound', () => {
            const out = [];
            for (const f of [() => Room.prototype.toString.call(undefined), () => RoomPosition.prototype.toString.call(undefined),
                () => Room.Terrain.call(undefined, 'W1N1'), () => (0, PathFinder.CostMatrix)(), () => PathFinder.CostMatrix.prototype.get.call(undefined, 1, 1),
                () => Flag.prototype.toString.call(undefined), () => Source.prototype.toString.call(undefined), () => Mineral.prototype.toString.call(undefined),
                () => ConstructionSite.prototype.toString.call(undefined), () => Game.map.findExit.call(undefined, 'W1N1', 'W1N2'),
                () => RoomVisual.prototype.getSize.call(undefined)]) {
                try { out.push(String(f())); } catch (e) { out.push(e.constructor.name); }
            }
            return out;
        }],
    ] }],
}];
