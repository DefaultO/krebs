// mirror: reference/engine/src/game/console.js
// parity: FAITHFUL
var _ = require('lodash'),
    messages = {},
    commandResults = {},
    visual = {};

// vanilla: console.js:6 (CON-OBJECT) - rebuilt every tick; log/logUnsafe writable+configurable, the rest
// non-writable, non-configurable; all non-enumerable.
exports.makeConsole = function(id, sandboxedFunctionWrapper) {
    messages[id] = [];
    commandResults[id] = [];
    visual[id] = {};
    return Object.create(null, {
        log: {
            writable: true,
            configurable: true,
            value: sandboxedFunctionWrapper(function() {

                if(typeof self != 'undefined' && self.navigator.userAgent) {
                    self['console']['log'].apply(console, arguments);
                }

                messages[id].push({
                    message: _.map(arguments, (i) => {
                        if(i && i.toString) return i.toString();
                        if(typeof i === 'undefined') return 'undefined';
                        return JSON.stringify(i);
                    }).join(' '),
                    escape: true
                });
            })
        },
        logUnsafe: {
            writable: true,
            configurable: true,
            value: sandboxedFunctionWrapper(function() {

                if(typeof self != 'undefined' && self.navigator.userAgent) {
                    self['console']['log'].apply(console, arguments);
                }

                messages[id].push({
                    message: _.map(arguments, (i) => {
                        if(i && i.toString) return i.toString();
                        if(typeof i === 'undefined') return 'undefined';
                        return JSON.stringify(i);
                    }).join(' '),
                    escape: false
                });
            })
        },
        commandResult: {
            value: sandboxedFunctionWrapper(function(message) {
                if(typeof self != 'undefined' && self.navigator.userAgent) {
                    self['console']['log'].call(console, message);
                }
                commandResults[id].push(String(message));
            })
        },
        // vanilla: console.js:57 (CON-VISUAL)
        addVisual: {
            value: sandboxedFunctionWrapper(function(roomName, data) {
                roomName = roomName || "";
                if(!data) {
                    return;
                }
                const sizeLimit = roomName == "map" ? 1000 : 500;
                visual[id][roomName] = visual[id][roomName] || "";
                const dataString = _.isString(data) ? data : JSON.stringify(data)+"\n";
                if((visual[id][roomName].length + dataString.length) > sizeLimit*1024) {
                    if(roomName == "map") {
                        throw new Error( `MapVisual size has exceeded ${sizeLimit} KB limit`);
                    }
                    throw new Error( `RoomVisual size in room ${roomName} has exceeded ${sizeLimit} KB limit`);
                }

                visual[id][roomName] += dataString;
            })
        },
        getVisualSize: {
            value: sandboxedFunctionWrapper(function(roomName) {
                roomName = roomName || "";
                if(!visual[id][roomName]) {
                    return 0;
                }
                return visual[id][roomName].length;
            })
        },
        clearVisual: {
            value: sandboxedFunctionWrapper(function(roomName) {
                roomName = roomName || "";
                visual[id][roomName] = "";
            })
        },
        getVisual: {
            value: sandboxedFunctionWrapper(function(roomName){
                return visual[id][roomName || ""];
            })
        }
    });
};

// vanilla: console.js:99
exports.getMessages = function(id) {
    var result = messages[id];
    messages[id] = [];
    return result;
};

// vanilla: console.js:105
exports.getCommandResults = function(id) {
    var result = commandResults[id];
    commandResults[id] = [];
    return result;
};

// vanilla: console.js:111 - resets to an array, not an object (the next makeConsole replaces it).
exports.getVisual = function(id) {
    var result = visual[id];
    visual[id] = [];
    return result;
};
