// mirror: reference/engine/src/game/resources.js
// parity: FAITHFUL
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
var utils = require('./../utils'),
    rooms = require('./rooms'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants;

var runtimeData, intents, register, globals;

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.Resource)` in make(), where `Resource` is make()'s own local;
// out here it is the global, read through its property cell.
function hasResource() {
    return typeof Resource !== 'undefined' && !!Resource;
}

// vanilla: resources.js:8 - class created once per VM (GLB-CLASSES-ONCE); module state reassigned every tick.
exports.make = function(_runtimeData, _intents, _register, _globals) {

    runtimeData = _runtimeData;
    intents = _intents;
    register = _register;
    globals = _globals;

    if(hasResource()) {
        return;
    }

    var data = (id) => {
        if(!runtimeData.roomObjects[id]) {
            throw new Error("Could not find an object with ID "+id);
        }
        return runtimeData.roomObjects[id];
    };

    var Resource = register.wrapFn(function(id) {
        var _data = data(id);
        RoomObject.call(this, _data.x, _data.y, _data.room, _data.effects);
        this.id = id;
    });

    Resource.prototype = Object.create(RoomObject.prototype);
    Resource.prototype.constructor = Resource;

    utils.defineGameObjectProperties(Resource.prototype, data, {
        energy: (o) => o.energy,
        amount: (o) => o[o.resourceType || C.RESOURCE_ENERGY],
        resourceType: (o) => o.resourceType || C.RESOURCE_ENERGY
    });

    Resource.prototype.toString = register.wrapFn(function() {
        return `[resource (${this.resourceType}) #${this.id}]`;
    });

    Object.defineProperty(globals, 'Resource', {enumerable: true, value: Resource});
    Object.defineProperty(globals, 'Energy', {enumerable: true, value: Resource});
};
