// __tick assembly (host/prelude.js = the JS half of data.js, §6.6): runtimeData orders seen through the API —
// ORDER-RUNTIME-ROOMIDS (first appearance, observer targets incl. "null"), ORDER-RUNTIME-OBJECTS (foreign grouped
// by roomIds, then own in global creation order), ORDER-GAME-ROOMS (rooms collection order), load-only rooms
// (own construction sites only), shared identity of own docs, event logs.
'use strict';
const { basicWorld, doc, ME, OTHER, rooms } = require('./_lib');

const T = ['runtime/data.js:113', 'runtime/data.js:200', 'game/game.js:186', 'game/game.js:196', 'game/game.js:218'];

const main = `
module.exports.loop = function () {
    console.log('rooms', Object.keys(Game.rooms));
    console.log('structures', Object.keys(Game.structures).map(function (id) { var s = Game.structures[id]; return s.structureType + '@' + s.pos.roomName; }));
    console.log('creeps', Object.keys(Game.creeps));
    console.log('sites', Object.keys(Game.constructionSites).map(function (id) { return Game.constructionSites[id].pos.roomName; }));
    for (var name in Game.rooms) {
        var r = Game.rooms[name];
        console.log(name, r.find(FIND_STRUCTURES).map(function (s) { return s.structureType + (s.my ? '*' : ''); }).join(),
            r.find(FIND_CREEPS).map(function (c) { return c.name; }).join(), r.find(FIND_SOURCES).length, JSON.stringify(r.getEventLog(true)));
    }
    console.log('hidden', Game.getObjectById(${JSON.stringify('HIDDEN')}));
    console.log('observers', Object.keys(Game.structures).filter(function (id) { return Game.structures[id].structureType == 'observer'; }).length);
};`;

const w = basicWorld(main, {
    rooms: rooms('W9N9', 'W5N5', 'W3N1', 'W2N1', 'W1N1', 'W4N4', 'W7N7'),
    eventLogs: { W1N1: '[{"event":1,"objectId":"x"}]', W3N1: '[]', null: 'never' },
});
w.objects.push(
    doc.creep('W3N1', 5, 5, 'Scout', ME),                                // W3N1 first appears after W1N1
    doc.source('W3N1', 6, 6),                                            // foreign in W3N1, created after my scout
    doc.observer('W4N4', 1, 1, ME, { observeRoom: 'W9N9' }),            // W4N4, then its target W9N9
    doc.observer('W4N4', 2, 2, ME, { observeRoom: null }),              // "null" room id
    doc.observer('W4N4', 3, 3, ME),                                      // observeRoom absent: "undefined"
    doc.site('W5N5', 9, 9, 'road', ME),                                  // load-only room (no vision)
    doc.creep('W5N5', 10, 10, 'Ghost', OTHER),                           // foreign, in a load-only room: invisible
    doc.source('W9N9', 25, 25),                                          // seen through the observer
    doc.creep('W2N1', 1, 1, 'Visitor', ME),                              // my creep in Bob's room, created last
    doc.spawn('W7N7', 4, 4, 'Unseen', OTHER),
);
w.objects.find((o) => o.name === 'Ghost')._id = 'HIDDEN';

module.exports = [
    { name: 'assembly-orders', tags: T, world: w, ticks: 2 },
];
