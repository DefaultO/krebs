// mirror: reference/engine/src/game/ruins.js
// parity: FAITHFUL
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
// Statement-by-statement port (sloppy mode, like the original); token-identical to vanilla apart from comments
// (tools/oracle/api-wp07/normdiff.js). `// vanilla: ruins.js:N` anchors use the vanilla line numbers.
var utils = require('./../utils'),
    rooms = require('./rooms'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants;

var runtimeData, intents, register, globals;

// vanilla: ruins.js:8
function _storeGetter(o) {
    return new Store(o);
}

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.Ruin)` in make(), where `Ruin` is make()'s own local;
// out here it is the global, read through its property cell.
function hasRuin() {
    return typeof Ruin !== 'undefined' && !!Ruin;
}

// vanilla: ruins.js:12
exports.make = function(_runtimeData, _intents, _register, _globals) {
    runtimeData = _runtimeData;
    intents = _intents;
    register = _register;
    globals = _globals;

    if(hasRuin()) {
        return;
    }

    // vanilla: ruins.js:22
    const data = (id) => {
        if(!runtimeData.roomObjects[id]) {
            throw new Error("Could not find an object with ID "+id);
        }
        return runtimeData.roomObjects[id];
    };

    // vanilla: ruins.js:29
    const Ruin = register.wrapFn(function(id) {
        const _data = data(id);
        RoomObject.call(this, _data.x, _data.y, _data.room, _data.effects);
        this.id = id;
    });

    Ruin.prototype = Object.create(RoomObject.prototype);
    Ruin.prototype.constructor = Ruin;

    // vanilla: ruins.js:38
    utils.defineGameObjectProperties(Ruin.prototype, data, {
        structureType: o => o.structureType,
        destroyTime: o => o.destroyTime,
        ticksToDecay: o => o.decayTime - runtimeData.time,
        store: _storeGetter,
        structure: o => {
            if(o.structure.user) {
                const structure = new OwnedStructure();
                Object.defineProperties(structure, {
                    id: { enumerable: true, get() { return o.structure.id; }},
                    hits: { enumerable: true, get() { return o.structure.hits; }},
                    hitsMax: { enumerable: true, get() { return o.structure.hitsMax; }},
                    structureType: { enumerable: true, get() { return o.structure.type }},
                    owner: { enumerable: true, get() { return { username: runtimeData.users[o.structure.user].username } }},
                    my: { enumerable: true, get() { return o.structure.user == runtimeData.user._id }}
                });
                return structure;
            }

            const structure = new Structure();
            Object.defineProperties(structure, {
                id: { enumerable: true, get() { return o.structure.id; }},
                hits: { enumerable: true, get() { return o.structure.hits }},
                hitsMax: { enumerable: true, get() { return o.structure.hitsMax }},
                structureType: { enumerable: true, get() { return o.structure.type }}
            });
            return structure;
        }
    });

    // vanilla: ruins.js:68
    Ruin.prototype.toString = register.wrapFn(function() {
        return `[ruin (${this.structure.structureType}) #${this.id}]`;
    });

    Object.defineProperty(globals, 'Ruin', {enumerable: true, value: Ruin});
};
