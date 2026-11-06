// Shared building blocks for API oracle cases. A case is {name, tags, world, ticks}:
//   name   fixture file name (crates/runtime/tests/fixtures/api/<name>.json)
//   tags   vanilla "dir/file.js:line" anchors the case covers (parity-report counts fixtures per file)
//   world  tools/oracle/api/world.js fields (defaults filled there)
//   ticks  number of ticks, or per-tick patches: an object merged into the world, or fn(world, tickIndex)
// The bot is world.code.modules; probes report through console.log, which is compared verbatim.
'use strict';

const ME = '5a1b2c3d4e5f601';
const OTHER = '6b2c3d4e5f60712';

const users = () => [
    { _id: '2', username: 'Invader', cpu: 0 },
    { _id: ME, username: 'Alice', cpu: 100, cpuAvailable: 9000, gcl: 1234567, power: 2500, active: 10000,
        badge: { type: 1, color1: '#ff0000', color2: '#00ff00', color3: '#0000ff', param: 0, flip: false },
        resources: { pixel: 3, cpuUnlock: 1 }, money: 1500000 },
    { _id: OTHER, username: 'Bob', cpu: 100, cpuAvailable: 10000, gcl: 0, active: 10000, badge: {} },
];

const rooms = (...names) => names.map((n) => ({ _id: n, status: 'normal' }));

let n = 0;
const id = (p) => `${p}${(++n).toString(16).padStart(15 - p.length, '0')}`;

const body = (...types) => types.map((type) => ({ type, hits: 100 }));

const doc = {
    controller: (room, x, y, o = {}) => Object.assign({ _id: id('c'), type: 'controller', room, x, y, level: 2, progress: 100,
        downgradeTime: 20000, safeModeAvailable: 1, isPowerEnabled: false }, o),
    spawn: (room, x, y, name, user, o = {}) => Object.assign({ _id: id('a'), type: 'spawn', room, x, y, name, user, hits: 5000,
        hitsMax: 5000, spawning: null, notifyWhenAttacked: true, store: { energy: 300 }, storeCapacityResource: { energy: 300 } }, o),
    extension: (room, x, y, user, o = {}) => Object.assign({ _id: id('e'), type: 'extension', room, x, y, user, hits: 1000,
        hitsMax: 1000, notifyWhenAttacked: true, store: { energy: 50 }, storeCapacityResource: { energy: 50 } }, o),
    source: (room, x, y, o = {}) => Object.assign({ _id: id('s'), type: 'source', room, x, y, energy: 3000,
        energyCapacity: 3000, ticksToRegeneration: 300 }, o),
    mineral: (room, x, y, o = {}) => Object.assign({ _id: id('m'), type: 'mineral', room, x, y, mineralType: 'H',
        mineralAmount: 70000, density: 3 }, o),
    creep: (room, x, y, name, user, o = {}) => Object.assign({ _id: id('k'), type: 'creep', room, x, y, name, user,
        body: body('work', 'carry', 'move'), hits: 300, hitsMax: 300, fatigue: 0, ageTime: 1500, spawning: false,
        notifyWhenAttacked: true, store: { energy: 0 }, storeCapacity: 50 }, o),
    site: (room, x, y, structureType, user, o = {}) => Object.assign({ _id: id('b'), type: 'constructionSite', room, x, y,
        structureType, user, progress: 0, progressTotal: 3000 }, o),
    observer: (room, x, y, user, o = {}) => Object.assign({ _id: id('o'), type: 'observer', room, x, y, user, hits: 500,
        hitsMax: 500, observeRoom: null, notifyWhenAttacked: true }, o),
    energy: (room, x, y, amount, o = {}) => Object.assign({ _id: id('r'), type: 'energy', room, x, y, energy: amount,
        resourceType: 'energy' }, o),
};

// one owned RCL2 room W1N1 with a spawn, a source, a creep; a foreign room W2N1 next to it
function basicWorld(main, extra = {}) {
    const objects = [
        doc.controller('W1N1', 20, 20, { user: ME }),
        doc.source('W1N1', 10, 10),
        doc.spawn('W1N1', 25, 25, 'Spawn1', ME),
        doc.creep('W1N1', 11, 11, 'Harvester1', ME),
        doc.extension('W1N1', 26, 26, ME),
        doc.creep('W1N1', 30, 30, 'Intruder', OTHER),
        doc.controller('W2N1', 20, 20, { user: OTHER }),
        doc.spawn('W2N1', 25, 25, 'BobSpawn', OTHER),
    ];
    const w = Object.assign({
        me: ME,
        time: 100,
        users: users(),
        rooms: rooms('W2N1', 'W1N1', 'W3N1'),
        objects,
        code: { modules: {}, timestamp: 1 },
    }, extra);
    if (w.code.modules.main == null) w.code.modules.main = main;   // `main: null` in extra.code = use the argument
    return w;
}

// a module whose loop logs JSON of each probe; probes = {label: '<js expression>'}
const probeMain = (probes, setup = '') => `${setup}
module.exports.loop = function () {
${Object.entries(probes).map(([k, e]) => `    try { console.log(${JSON.stringify(k)}, JSON.stringify(${e})); } catch (e) { console.log(${JSON.stringify(k)}, 'threw', String(e)); }`).join('\n')}
};`;

module.exports = { ME, OTHER, users, rooms, doc, body, basicWorld, probeMain, id };
