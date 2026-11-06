// console (02 CON-*): object shape, argument formatting, logUnsafe, missing methods, console commands, visuals.
'use strict';
const { basicWorld } = require('./_lib');

const T = ['game/console.js:6', 'game/console.js:57', 'game/game.js:535', 'runtime/runtime-driver.js:58'];

module.exports = [
    {
        name: 'console-format',
        tags: T,
        world: basicWorld(`
var old;
module.exports.loop = function () {
    console.log(NaN, '', 0, false, null, undefined, 1.5, 'str', [1, [2, 3]], {a: 1}, Object.create(null), Symbol('s'), 7n);
    console.log(function f() { return 1; }, Game.creeps.Harvester1, Game.spawns.Spawn1, Game.rooms.W1N1);
    console.logUnsafe('<b>unsafe</b>', 1);
    console.log();
    try { console.log(0n); } catch (e) { console.log('0n', e.message); }
    try { console.log({toString: 5}); } catch (e) { console.log('toString', e.message); }
    try { console.error('x'); } catch (e) { console.log('error', e.message); }
    console.log('shape', Object.keys(console), Object.getOwnPropertyNames(console), Object.getPrototypeOf(console));
    var d = Object.getOwnPropertyDescriptor(console, 'log'), v = Object.getOwnPropertyDescriptor(console, 'addVisual');
    console.log('desc', d.writable, d.enumerable, d.configurable, v.writable, v.enumerable, v.configurable);
    if (old) old.log('old console object of the previous tick');
    old = console;
    console.log = function () {};
};`),
        ticks: 2,
    },
    {
        name: 'console-self-hook',
        tags: T,
        world: basicWorld(`
module.exports.loop = function () {
    console.log('before');
    if (Game.time === 100) global.self = {};
    if (Game.time === 101) global.self = {navigator: {userAgent: ''}};
    if (Game.time === 102) global.self = {navigator: {userAgent: 'x'}, console: {log: function (m) { global.forwarded = m; }}};
    console.log('after', global.forwarded);
};`),
        ticks: 3,
    },
    {
        name: 'console-commands',
        tags: T.concat(['game/game.js:540']),
        world: basicWorld(`module.exports.loop = function () { console.log('loop', typeof global.fromCmd, typeof x); };`, {
            consoleCommands: [
                { expression: '1 + 1' },
                { expression: 'var x = 5; x' },
                { expression: 'typeof x' },
                { expression: 'global.fromCmd = 1; Memory.cmd = true', hidden: true },
                { expression: '({})' },
                { expression: '[typeof module, typeof exports, typeof code, this === global].join()' },
                { expression: 'undefined' },
                { expression: 'Game.notify("from console")' },
            ],
        }),
        ticks: [{}, {}, { consoleCommands: [{ expression: '"ok"' }, { expression: 'throw new Error("cmd fails")' }, { expression: '"skipped"' }] },
            { consoleCommands: [{ expression: 'Symbol()' }] }, { consoleCommands: [{ expression: 'syntax error here' }] }],
    },
    {
        name: 'console-visual',
        tags: T.concat(['runtime/runtime.js:282']),
        world: basicWorld(`
module.exports.loop = function () {
    console.addVisual('W1N1', {t: 'c', x: 1});
    console.addVisual('W1N1', 'raw string');
    console.addVisual(undefined, {t: 'l'});
    console.addVisual('map', {t: 'm'});
    console.addVisual('W2N1', '');
    console.log('size', console.getVisualSize('W1N1'), console.getVisualSize(), console.getVisualSize('none'), console.getVisual('W1N1'));
    try { console.addVisual('W3N1', 'x'.repeat(500 * 1024 + 1)); } catch (e) { console.log(e.message); }
    try { console.addVisual('map', 'x'.repeat(1000 * 1024 + 1)); } catch (e) { console.log(e.message); }
    if (Game.time === 101) console.clearVisual('W1N1');
    if (Game.time === 101) console.clearVisual();
};`),
        ticks: 2,
    },
];
