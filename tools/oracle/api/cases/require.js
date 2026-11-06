// Modules (02 MOD-*): flat namespace, cache quirks, wrapper (+36 columns on line 1), errors, legacy mode.
'use strict';
const { basicWorld } = require('./_lib');

const T = ['game/game.js:504', 'game/game.js:517', 'game/game.js:551', 'runtime/runtime-driver.js:21', 'runtime/runtime-driver.js:101'];

module.exports = [
    {
        name: 'require-namespace',
        tags: T,
        world: basicWorld(`
var a = require('./role.harvester'), b = require('role.harvester');
console.log('same', a === b, require.cache['role.harvester'] === a, require('lodash') === _);
console.log('proto', typeof require('toString'), require('__proto__') === Object.prototype, require('constructor') === Object);
try { require('nope') } catch (e) { console.log('unknown', e.message, e instanceof Error) }
try { require('../role.harvester') } catch (e) { console.log('dotdot', e.message) }
console.log('xjs', require('x.js').me, require('lib/util').me);
console.log('module', module.name, typeof module.code, module.user, module.timestamp, Object.keys(module), Object.getOwnPropertyDescriptor(module, '__initGlobals'));
console.log('wrapper', typeof this, this === global, arguments.length, typeof __filename, typeof exports, exports === module.exports);
console.log('requireProps', Object.keys(require), typeof require.timestamp, Object.getOwnPropertyDescriptor(require, 'initGlobals'), require.name);
module.exports.loop = function () {
    console.log('loop', this === module.exports, typeof global.module, global.__module && global.__module.name, Object.keys(require.cache));
    console.log('leak', JSON.stringify(Object.keys(global.module)));
};`, {
            code: { timestamp: 1, modules: {
                main: null,
                'role.harvester': "module.exports = {role: 'h', n: Math.random()};",
                'x.js': "exports.me = 'x.js';",
                'lib/util': "'use strict'; exports.me = typeof this;",
            } },
        }),
        ticks: 2,
    },
    {
        name: 'require-errors',
        tags: T,
        world: basicWorld(`
try { require('a') } catch (e) { console.log('circular', e.message, e instanceof Error) }
try { require('broken') } catch (e) { console.log('broken', e.message, e instanceof Error, typeof e.stack, String(e)) }
try { require('broken') } catch (e) { console.log('broken again', String(e)) }
try { require('throwsString') } catch (e) { console.log('string', String(e), e.message) }
try { require('minusOne'); require('minusOne') } catch (e) { console.log('minusOne', e.message) }
module.exports.loop = function () { console.log('cache', Object.keys(require.cache)); };`, {
            code: { timestamp: 1, modules: {
                main: null,
                a: "require('b');",
                b: "require('a');",
                broken: "var x = 1; throw new Error('boom ' + x);\n// trailing comment",
                throwsString: "throw 'plain';",
                minusOne: 'module.exports = -1;',
            } },
        }),
        ticks: 2,
    },
    {
        name: 'require-column-offset',
        tags: T,
        world: basicWorld(`module.exports.loop = function () { require('oneLine'); };`, {
            code: { timestamp: 1, modules: {
                main: null,
                oneLine: "var a = 1; function f() { null.x; } f();",
            } },
        }),
        ticks: 1,
    },
    {
        name: 'loop-errors',
        tags: T.concat(['runtime/runtime.js:237']),
        world: basicWorld(`
function thrower(kind) {
    if (kind === 0) throw new Error('plain error');
    if (kind === 1) throw 'a string';
    if (kind === 2) throw {stack: 'custom stack'};
    if (kind === 3) throw null;
    if (kind === 4) undefinedFunction();
    if (kind === 5) throw {};
}
module.exports.loop = function () {
    Game.notify('before ' + Game.time);
    Memory.t = Game.time;
    thrower(Game.time % 6);
};`, { consoleCommands: [{ expression: '"never"' }] }),
        ticks: [{ time: 0 }, { consoleCommands: [{ expression: '1' }] }, {}, {}, {}, {}],
    },
    {
        name: 'main-throws-toplevel',
        tags: T,
        world: basicWorld(null, {
            code: { timestamp: 1, modules: {
                main: "global.evals = (global.evals || 0) + 1; var lib = require('lib'); if (Game.time % 2) throw new Error('top ' + Game.time);\nmodule.exports.loop = function () { console.log('loop', global.evals, global.libEvals); };",
                lib: 'global.libEvals = (global.libEvals || 0) + 1;',
            } },
            consoleCommands: [{ expression: '"lost"' }],
        }),
        ticks: 4,
    },
    {
        name: 'legacy-no-loop',
        tags: T,
        world: basicWorld(null, {
            code: { timestamp: 1, modules: {
                main: "global.n = (global.n || 0) + 1; require('lib'); console.log('main', global.n, global.m, typeof require.cache.main);",
                lib: 'global.m = (global.m || 0) + 1;',
            } },
        }),
        ticks: 3,
    },
    {
        name: 'loop-swap-and-async',
        tags: T,
        world: basicWorld(`
module.exports.loop = function () {
    console.log('first');
    module.exports.loop = async function () { console.log('async never runs'); };
};`),
        ticks: 3,
    },
    {
        name: 'require-binary',
        tags: T.concat(['runtime/runtime-driver.js:13']),
        world: basicWorld(`
module.exports.loop = function () {
    const b = require('blob');
    console.log(b instanceof Uint8Array, b.length, Array.from(b).join(','), Object.prototype.toString.call(b), require('blob') === b);
};`, {
            code: { timestamp: 1, modules: { main: null, blob: { binary: Buffer.from([1, 2, 3, 250]).toString('base64') } } },
        }),
        ticks: 2,
    },
];
