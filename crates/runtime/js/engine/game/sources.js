// mirror: reference/engine/src/game/sources.js
// parity: FAITHFUL
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
var utils = require('./../utils'),
    rooms = require('./rooms'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants;

var runtimeData, intents, register, globals;

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.Source)` in make(), where `Source` is make()'s own local;
// out here it is the global, read through its property cell.
function hasSource() {
    return typeof Source !== 'undefined' && !!Source;
}

// vanilla: sources.js:8
exports.make = function(_runtimeData, _intents, _register, _globals) {

    runtimeData = _runtimeData;
    intents = _intents;
    register = _register;
    globals = _globals;

    if(hasSource()) {
        return;
    }

    var data = (id) => {
        if(!runtimeData.roomObjects[id]) {
            throw new Error("Could not find an object with ID "+id);
        }
        return runtimeData.roomObjects[id];
    };

    // vanilla: sources.js:26
    var Source = register.wrapFn(function(id) {
        var _data = data(id);
        RoomObject.call(this, _data.x, _data.y, _data.room, _data.effects);
        this.id = id;
    });

    Source.prototype = Object.create(RoomObject.prototype);
    Source.prototype.constructor = Source;

    // vanilla: sources.js:35
    utils.defineGameObjectProperties(Source.prototype, data, {
        energy: (o) => o.energy,
        energyCapacity: (o) => o.energyCapacity,
        ticksToRegeneration: (o) => o.nextRegenerationTime ? o.nextRegenerationTime - runtimeData.time : undefined
    });

    // vanilla: sources.js:41
    Source.prototype.toString = register.wrapFn(function() {
        return `[source #${this.id}]`;
    });

    Object.defineProperty(globals, 'Source', {enumerable: true, value: Source});
};
