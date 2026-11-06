// mirror: reference/engine/src/game/construction-sites.js
// parity: FAITHFUL
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
var utils = require('./../utils'),
    rooms = require('./rooms'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants,
    _ = require('lodash');

var runtimeData, intents, register, globals;

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.ConstructionSite)` in make(), where `ConstructionSite` is make()'s own local;
// out here it is the global, read through its property cell.
function hasConstructionSite() {
    return typeof ConstructionSite !== 'undefined' && !!ConstructionSite;
}

// vanilla: construction-sites.js:9
exports.make = function(_runtimeData, _intents, _register, _globals) {

    runtimeData = _runtimeData;
    intents = _intents;
    register = _register;
    globals = _globals;

    if(hasConstructionSite()) {
        return;
    }

    var data = (id) => {
        if(!runtimeData.roomObjects[id]) {
            throw new Error("Could not find an object with ID "+id);
        }
        return runtimeData.roomObjects[id];
    };

    // vanilla: construction-sites.js:27
    var ConstructionSite = register.wrapFn(function(id) {
        var _data = data(id);
        RoomObject.call(this, _data.x, _data.y, _data.room, _data.effects);
        this.id = id;
    });

    ConstructionSite.prototype = Object.create(RoomObject.prototype);
    ConstructionSite.prototype.constructor = ConstructionSite;

    // vanilla: construction-sites.js:36
    utils.defineGameObjectProperties(ConstructionSite.prototype, data, {
        progress: (o) => o.progress,
        progressTotal: (o) => o.progressTotal,
        structureType: (o) => o.structureType,
        name: (o) => o.name,
        owner: (o) => new Object({username: runtimeData.users[o.user].username}),
        my: (o) => _.isUndefined(o.user) ? undefined : o.user == runtimeData.user._id
    });


    // vanilla: construction-sites.js:46
    ConstructionSite.prototype.toString = register.wrapFn(function() {
        return `[construction site (${data(this.id).structureType}) #${this.id}]`;
    });

    // vanilla: construction-sites.js:50
    ConstructionSite.prototype.remove = register.wrapFn(function() {

        if(!this.my && !(this.room && this.room.controller && this.room.controller.my)) {
            return C.ERR_NOT_OWNER;
        }
        intents.pushByName('room', 'removeConstructionSite', {roomName: data(this.id).room, id: this.id});
        return C.OK;
    });

    Object.defineProperty(globals, 'ConstructionSite', {enumerable: true, value: ConstructionSite});
};
