// Seeded random worlds for the __tick assembly (host/prelude.js vs data.js): rooms in a shuffled collection order,
// objects of random types/owners/rooms in random creation order, observers aimed at real, missing, null and absent
// rooms, own sites in otherwise unseen rooms, flags (incl. in rooms that are not visible). The probe dumps every order-sensitive view of runtimeData.
'use strict';
const { prng } = require('../../lib/common');
const { users, ME, OTHER, doc } = require('./_lib');

const probe = `
module.exports.loop = function () {
    var ids = function (h) { return Object.keys(h).map(function (k) { return k + ':' + (h[k].id || h[k].name); }).join(); };
    console.log('rooms', Object.keys(Game.rooms).join());
    console.log('creeps', ids(Game.creeps));
    console.log('structures', Object.keys(Game.structures).join());
    console.log('spawns', ids(Game.spawns));
    console.log('sites', Object.keys(Game.constructionSites).join());
    console.log('flags', Object.keys(Game.flags).join());
    for (var name in Game.rooms) {
        var r = Game.rooms[name];
        [FIND_CREEPS, FIND_STRUCTURES, FIND_SOURCES, FIND_CONSTRUCTION_SITES, FIND_DROPPED_RESOURCES, FIND_FLAGS, FIND_MY_STRUCTURES, FIND_HOSTILE_CREEPS].forEach(function (t) {
            console.log(name, t, r.find(t).map(function (o) { return o.id || o.name; }).join());
        });
        console.log(name, 'energy', r.energyAvailable, r.energyCapacityAvailable, JSON.stringify(r.getEventLog(true)));
    }
};`;

function world(seed) {
    const rnd = prng(seed);
    const grid = [];
    for (let x = 0; x < 4; x++) for (let y = 0; y < 4; y++) grid.push(`W${x}N${y}`);
    const roomNames = grid.filter(() => rnd.chance(0.7));
    for (let i = roomNames.length - 1; i > 0; i--) {
        const j = rnd.int(i + 1);
        [roomNames[i], roomNames[j]] = [roomNames[j], roomNames[i]];
    }
    // chitin keeps every object inside an existing room (RoomSnapshot), so objects never sit in a non-room; only
    // observer targets may name one
    const room = () => rnd.pick(roomNames);
    const owner = () => rnd.pick([ME, ME, OTHER, undefined, '2']);
    const objects = [];
    let n = 0;
    const count = 10 + rnd.int(40);
    for (let i = 0; i < count; i++) {
        const r = room(), x = 1 + rnd.int(48), y = 1 + rnd.int(48), u = owner();
        const k = rnd.int(8);
        let o;
        if (k === 0) o = doc.creep(r, x, y, `c${n++}`, u || ME, { spawning: rnd.chance(0.2) });
        else if (k === 1) o = doc.spawn(r, x, y, `s${n++}`, u || OTHER, { off: rnd.chance(0.2) });
        else if (k === 2) o = doc.extension(r, x, y, u || ME, { off: rnd.chance(0.3) });
        else if (k === 3) o = doc.source(r, x, y);
        else if (k === 4) o = doc.site(r, x, y, rnd.pick(['road', 'extension', 'spawn']), u || ME);
        else if (k === 5) o = doc.observer(r, x, y, u || ME, rnd.chance(0.25) ? {} : { observeRoom: rnd.pick([...roomNames, 'W9N9', null]) });
        else if (k === 6) o = doc.energy(r, x, y, rnd.int(500));
        else o = doc.controller(r, x, y, u ? { user: u } : {});
        if (o.user === undefined) delete o.user;
        objects.push(o);
    }
    if (!objects.some((o) => o.user === ME)) objects.push(doc.creep(roomNames[0] || 'W0N0', 5, 5, 'anchor', ME));
    const eventLogs = {};
    for (const r of roomNames) if (rnd.chance(0.5)) eventLogs[r] = `[{"event":${rnd.int(10)}}]`;
    const flags = roomNames.filter(() => rnd.chance(0.3)).map((r, i) => ({ _id: `fl${i}`, user: ME, room: r, data: `F${i}a~1~2~3~4|F${i}b~5~5~6~7` }));
    return {
        me: ME, time: 1000 + seed, users: users(),
        rooms: roomNames.concat(roomNames.includes('W0N0') ? [] : ['W0N0']).map((r) => ({ _id: r, status: 'normal' })),
        objects, flags, eventLogs,
        code: { modules: { main: probe }, timestamp: 1 },
    };
}

module.exports = Array.from({ length: 12 }, (_x, i) => ({
    name: `assembly-fuzz-${String(i).padStart(2, '0')}`,
    tags: ['runtime/data.js:113', 'game/game.js:260'],
    world: world(1000 + i),
    ticks: 1,
}));
