// mirror: reference/engine/src/game/nukes.js
// parity: FAITHFUL
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
// Statement-by-statement port (sloppy mode, like the original); token-identical to vanilla apart from comments
// (tools/oracle/api-wp07/normdiff.js). `// vanilla: nukes.js:N` anchors use the vanilla line numbers.
var utils = require('./../utils'),
    rooms = require('./rooms'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants;

var runtimeData, intents, register, globals;

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.Nuke)` in make(), where `Nuke` is make()'s own local;
// out here it is the global, read through its property cell.
function hasNuke() {
    return typeof Nuke !== 'undefined' && !!Nuke;
}

// vanilla: nukes.js:8
exports.make = function(_runtimeData, _intents, _register, _globals) {

    runtimeData = _runtimeData;
    intents = _intents;
    register = _register;
    globals = _globals;

    if(hasNuke()) {
        return;
    }

    // vanilla: nukes.js:19
    var data = (id) => {
        if(!runtimeData.roomObjects[id]) {
            throw new Error("Could not find an object with ID "+id);
        }
        return runtimeData.roomObjects[id];
    };

    // vanilla: nukes.js:26
    var Nuke = register.wrapFn(function(id) {
        var _data = data(id);
        RoomObject.call(this, _data.x, _data.y, _data.room, _data.effects);
        this.id = id;
    });

    Nuke.prototype = Object.create(RoomObject.prototype);
    Nuke.prototype.constructor = Nuke;

    // vanilla: nukes.js:35
    utils.defineGameObjectProperties(Nuke.prototype, data, {
        timeToLand: (o) => o.landTime - runtimeData.time,
        launchRoomName: o => o.launchRoomName
    });

    // vanilla: nukes.js:40
    Nuke.prototype.toString = register.wrapFn(function() {
        return `[nuke #${this.id}]`;
    });

    Object.defineProperty(globals, 'Nuke', {enumerable: true, value: Nuke});
};