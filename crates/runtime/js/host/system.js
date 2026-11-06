// not a mirror: system isolate __store: make.js:192 storeIntents + intentsCount (§4.2). Owner: WP02.
// Runs on the ValueDeserializer copy of the player's outMessage, the equivalent of vanilla's ExternalCopy into the
// runner's node realm, so bot prototype patches never reach it. Sloppy, as utils.js and make.js.
// Natives: objectRoom(id) -> room name at tick start | undefined (§4.3). A throw out of __store is vanilla's
// make.js catch: the run is a total failure with `error.stack` as its error.
var system = require('../common/system');

// config.engine.customIntentTypes (driver/lib/index.js:28); the host sets the mods' merged types once.
var customIntentTypes = {};
globalThis.__setCustomIntentTypes = function(types) {
    customIntentTypes = types;
};

// vanilla: utils.js:405 storeIntents. `userRuntimeData.userObjects[i] || userRuntimeData.roomObjects[i]` are
// plain id maps: a real id gives its doc (objectRoom), any other key the value it inherits from Object.prototype
// ("constructor", "__proto__", ...), which is truthy and has no `room` unless this call put one there.
function storeIntents(userId, userIntents, customIntentTypes) {
    var intents = {};

    for(var i in userIntents) {

        if(i == 'notify') {
            intents.notify = system.sanitizeUserIntents({notify: userIntents.notify}).notify;
            continue;
        }

        if(i == 'room') {
            system.sanitizeUserRoomIntents(userIntents.room, intents, customIntentTypes);
            continue;
        }

        if(i == 'global') {
            intents.global = system.sanitizeUserIntents(userIntents.global, customIntentTypes);
            continue;
        }

        const room = objectRoom(i);
        const object = room !== undefined ? {room: room} : ({})[i];
        if(!object) {
            continue;
        }

        intents[object.room] = intents[object.room] || {};
        intents[object.room][i] = system.sanitizeUserIntents(userIntents[i], customIntentTypes);
    }

    return intents;
}

// vanilla: make.js:98 - counted over the raw list: free say/pull, arrays per element
const intentsCount = function(intentsList) {
    const freeMethods = {say: true, pull: true};

    let count = 0;
    for(const entity in intentsList) {
        for(const intentName in intentsList[entity]) {
            if(!freeMethods[intentName]) {
                count += Array.isArray(intentsList[entity][intentName]) ? intentsList[entity][intentName].length : 1;
            }
        }
    }

    return count;
};

// Room grouping writes `.room` onto whatever `intents[roomName]` inherits (roomName "__proto__" -> Object.prototype,
// "constructor"/"toString"/... -> a builtin function). Vanilla keeps that pollution for the life of the runner
// process, breaking every later user; it is undone after each call, so every call behaves like vanilla's first.
const polluted = [Object.prototype].concat(Object.getOwnPropertyNames(Object.prototype)
    .map(k => Object.getOwnPropertyDescriptor(Object.prototype, k).value)
    .filter(v => typeof v === 'function'));

// JSON.stringify output with non-ASCII escaped, so the host reads a one-byte string.
function ascii(json) {
    return json.replace(/[\u0080-\uffff]/g, c => '\\u' + ('000' + c.charCodeAt(0).toString(16)).slice(-4));
}

// vanilla: make.js:192-193 (storeIntents, then intentsCount) plus the outMessage fields make.js/runner.js read.
// memory, memorySegments and interShardSegment stay with the host (read as JsStr, §4.2).
// ponytail: activeForeignSegment.username is unchecked bot data (runtime.js setActiveForeignSegment) and goes out as
// JSON, i.e. as vanilla's storage RPC sends it. A BigInt or a cycle in it makes this stringify throw (a total
// failure, unbilled), while vanilla bills and saves its $set first and only then throws in the RPC's
// JSON.stringify (make.js:214-233), or never throws when `username == stored username` and `id` is set. Carry the
// raw value out of band if a bot ever depends on that.
globalThis.__store = function __store(out, userId) {
    try {
        const intents = storeIntents(userId, out.intentsList, customIntentTypes);
        const count = intentsCount(out.intentsList);
        return ascii(JSON.stringify({
            type: out.type,
            error: out.error === undefined ? null : out.error,
            usedTime: out.usedTime,
            usedCleanTime: out.usedCleanTime,
            usedDirtyTime: out.usedDirtyTime,
            intents,
            count,
            console: out.console,
            visual: out.visual,
            activeSegments: out.activeSegments,
            activeForeignSegment: out.activeForeignSegment,
            defaultPublicSegment: out.defaultPublicSegment,
            publicSegments: out.publicSegments
        }));
    }
    finally {
        for(const o of polluted) {
            delete o.room;
        }
    }
};
