'use strict';
// Shared case helpers (files starting with '_' are not case modules).

// Code module from a real function: its source text becomes the loop body (no closures allowed).
const code = (loop) => ({ main: `var P = require('probe');
var G = function(id) { return Game.getObjectById(id) || Game.creeps[id] || Game.spawns[id] || Game.powerCreeps[id]; };
module.exports.loop = ${loop.toString()};` });

// Code module probing a list of expressions, each labelled by its own text.
const probe = (exprs, pre) => ({ main: `var P = require('probe');
var G = function(id) { return Game.getObjectById(id) || Game.creeps[id] || Game.spawns[id] || Game.powerCreeps[id]; };
module.exports.loop = function() {
${pre || ''}
${exprs.map(e => `P(${JSON.stringify(e)}, () => ${e});`).join('\n')}
};` });

// actor x target x extra-argument expressions for one method.
const matrix = (method, actors, targets, extra = ['']) => {
    const out = [];
    for (const a of actors) for (const t of targets) for (const x of extra) out.push(`G('${a}').${method}(${t}${x})`);
    return out;
};

// Invalid targets every method sees: absent, plain objects (incl. a 24-char id: assertTargetObject), strings.
const JUNK = ['undefined', 'null', '{}', "{id: 'src1'}", "{id: 'abcdefabcdefabcdefabcdef'}", "'src1'", '5'];

module.exports = { code, probe, matrix, JUNK };
