// What the official runtime gets from being a webpack 3 bundle (ENV-SNAPSHOT, driver/webpack.config.js) rather
// than from its sources: lodash's AMD test compiles to `true`, so the runtime's own lodash becomes the global `_`
// (game.js:475 then never calls runInContext) and bot patches to `_` reach engine code; `global` is a wrapper
// parameter, so reassigning the global property does not reach runtime.js; the terrain map is a plain `{}`, so
// Object.prototype keys resolve. Cross-check: `run.js webpack --real-bundle` runs the installed official bundle.
'use strict';
const { basicWorld } = require('./_lib');

module.exports = [
    {
        name: 'webpack-shared-lodash',
        tags: ['runtime/runtime.js:11', 'game/game.js:475', 'game/game.js:504', 'runtime/runtime.js:109'],
        world: basicWorld(`
module.exports.loop = function () {
    console.log('lodash', require('lodash') === _, typeof _._, _.VERSION, _.persisted);
    var isString = _.isString, isArray = _.isArray;
    _.isString = function () { return false; };
    try { RawMemory.set('{}'); console.log('set ok'); } catch (e) { console.log('set threw', e.message); }
    _.isString = isString;
    _.isArray = function () { return false; };
    try { RawMemory.setActiveSegments([1]); console.log('segments ok'); } catch (e) { console.log('segments threw', e.message); }
    _.isArray = isArray;
    _.persisted = Game.time;
};`),
        ticks: 2,
    },
    {
        name: 'webpack-global-reassigned',
        tags: ['runtime/runtime.js:1', 'runtime/runtime.js:217'],
        world: basicWorld(`
module.exports.loop = function () {
    console.log('tick', typeof Game, Game.time, typeof global, global && global.fake, typeof Memory, typeof RawMemory);
    Memory.t = Game.time;
    if (Game.time === 100) global = {fake: true};
    if (Game.time === 101) delete globalThis.global;
};`),
        ticks: 3,
    },
    {
        name: 'webpack-terrain-proto-keys',
        tags: ['runtime/runtime.js:18', 'runtime/runtime.js:211', 'game/rooms.js:1226', 'game/map.js:248'],
        world: basicWorld(`
function t(f) { try { return JSON.stringify(f()); } catch (e) { return 'threw ' + e.message; } }
module.exports.loop = function () {
    console.log('at', t(function () { return [Game.map.getTerrainAt(5, 5, 'W1N1'), Game.map.getTerrainAt(5, 5, 'constructor'), Game.map.getTerrainAt(5, 5, 'W9N9')]; }));
    ['W1N1', 'W9N9', 'toString', '__proto__', 'hasOwnProperty'].forEach(function (name) {
        console.log(name, t(function () { var x = new Room.Terrain(name); return [x.get(1, 1), x.get(49, 49)]; }),
            t(function () { return Game.map.getRoomTerrain(name).get(0, 0); }));
    });
};`),
        ticks: 1,
    },
];
