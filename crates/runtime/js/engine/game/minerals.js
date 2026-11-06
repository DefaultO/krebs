// mirror: reference/engine/src/game/minerals.js
// parity: FAITHFUL
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
var utils = require('./../utils'),
    rooms = require('./rooms'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants;

var runtimeData, intents, register, globals;

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.Mineral)` in make(), where `Mineral` is make()'s own local;
// out here it is the global, read through its property cell.
function hasMineral() {
    return typeof Mineral !== 'undefined' && !!Mineral;
}

// vanilla: minerals.js:8
exports.make = function(_runtimeData, _intents, _register, _globals) {

    runtimeData = _runtimeData;
    intents = _intents;
    register = _register;
    globals = _globals;

    if(hasMineral()) {
        return;
    }

    var data = (id) => {
        if(!runtimeData.roomObjects[id]) {
            throw new Error("Could not find an object with ID "+id);
        }
        return runtimeData.roomObjects[id];
    };

    // vanilla: minerals.js:26
    var Mineral = register.wrapFn(function(id) {
        var _data = data(id);
        RoomObject.call(this, _data.x, _data.y, _data.room, _data.effects);
        this.id = id;
    });

    Mineral.prototype = Object.create(RoomObject.prototype);
    Mineral.prototype.constructor = Mineral;

    // vanilla: minerals.js:35
    utils.defineGameObjectProperties(Mineral.prototype, data, {
        mineralType: (o) => o.mineralType,
        mineralAmount: (o) => o.mineralAmount,
        density: o => o.density,
        ticksToRegeneration: (o) => o.nextRegenerationTime ? o.nextRegenerationTime - runtimeData.time : undefined
    });

    // vanilla: minerals.js:42
    Mineral.prototype.toString = register.wrapFn(function() {
        return `[mineral (${this.mineralType}) #${this.id}]`;
    });

    Object.defineProperty(globals, 'Mineral', {enumerable: true, value: Mineral});
};
