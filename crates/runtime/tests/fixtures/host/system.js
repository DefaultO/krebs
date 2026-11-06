// Host-contract fixture for the runtime conformance tests (not the real system bundle): a miniature
// __store with the driver's intentsCount and storeIntents routing through objectRoom, returning the
// ARCHITECTURE.md 4.2 JSON shape (no sanitizers).
var _ = require('lodash');

global.__store = function (out, userId) {
    var intents = {}, count = 0;
    for (var id in out.intentsList) {
        var entity = out.intentsList[id];
        for (var name in entity) {
            if (name !== 'say' && name !== 'pull') count += Array.isArray(entity[name]) ? entity[name].length : 1;
        }
        var room = objectRoom(id);
        if (room === undefined) continue;
        intents[room] = intents[room] || {};
        intents[room][id] = entity;
    }
    return JSON.stringify({
        type: out.type,
        error: out.error === undefined ? null : out.error,
        usedTime: out.usedTime,
        usedCleanTime: out.usedCleanTime,
        usedDirtyTime: out.usedDirtyTime,
        intents: intents,
        count: count,
        console: out.console,
        visual: out.visual,
        activeSegments: out.activeSegments,
        user: userId,
        isArray: _.isArray([])
    });
};
