// Room.find: every FIND_* constant, filters (function / matches object / property string), unknown types,
// clone-vs-cache, FIND_EXIT cache pollution (spec 03 ROOM-FIND, ROOM-FIND-EXIT, FIND-*).
'use strict';
const { base } = require('../world');

module.exports = [{
    name: 'find-all-types',
    tags: ['game/rooms.js:584', 'game/rooms.js:592'],
    world: base(),
    ticks: [{ probes: [
        ['all', () => {
            const r = Game.rooms.W1N1, out = {};
            for (const k of Object.keys(global).filter(k => /^FIND_/.test(k)).sort()) out[k] = r.find(global[k]);
            return out;
        }],
        ['other room', () => [FIND_STRUCTURES, FIND_HOSTILE_STRUCTURES, FIND_MY_CREEPS, FIND_SOURCES, FIND_FLAGS].map(t => Game.rooms.W1N2.find(t))],
        ['unknown', () => [Game.rooms.W1N1.find(), Game.rooms.W1N1.find('creeps'), Game.rooms.W1N1.find(999), Game.rooms.W1N1.find(null, {filter: () => true})]],
        ['filter fn', () => Game.rooms.W1N1.find(FIND_STRUCTURES, {filter: (s, i, coll) => s.structureType == 'road' || i === 0 && Array.isArray(coll)})],
        ['filter obj', () => [
            Game.rooms.W1N1.find(FIND_CREEPS, {filter: {my: true}}),
            Game.rooms.W1N1.find(FIND_STRUCTURES, {filter: {structureType: 'extension', my: true}}),
            Game.rooms.W1N1.find(FIND_CREEPS, {filter: {owner: {username: 'Bob'}}}),
            Game.rooms.W1N1.find(FIND_CREEPS, {filter: {pos: {x: 25}}}),
        ]],
        ['filter string', () => [Game.rooms.W1N1.find(FIND_SOURCES, {filter: 'energy'}), Game.rooms.W1N1.find(FIND_CREEPS, {filter: 'my'})]],
        ['clone', () => {
            const r = Game.rooms.W1N1, a = r.find(FIND_SOURCES), b = r.find(FIND_SOURCES);
            a.push(1);
            return [a === b, a.length, b.length, r.find(FIND_SOURCES).length, a[0] === b[0]];
        }],
        ['exit sides', () => {
            const r = Game.rooms.W1N1;
            return [FIND_EXIT_TOP, FIND_EXIT_RIGHT, FIND_EXIT_BOTTOM, FIND_EXIT_LEFT].map(t => r.find(t));
        }],
        ['exit pollution', () => {
            const r = Game.rooms.W1N2;
            const first = r.find(FIND_EXIT, {filter: p => p.x % 2 == 0});
            return [first, r.find(FIND_EXIT), r.find(FIND_EXIT_TOP), r.find(FIND_EXIT, {filter: p => p.y == 0})];
        }],
        ['filter mutates cache', () => {
            const r = Game.rooms.W1N1;
            r.find(FIND_MINERALS, {filter: (m, i, coll) => { coll.push('junk'); return true; }});
            return r.find(FIND_MINERALS);
        }],
        ['instances', () => {
            const r = Game.rooms.W1N1;
            return r.find(FIND_MY_SPAWNS)[0] === Game.spawns.Spawn1 && r.find(FIND_SOURCES)[0] === Game.getObjectById('s1');
        }],
        ['no room', () => Object.keys(Game.rooms)],
    ] }],
}];
