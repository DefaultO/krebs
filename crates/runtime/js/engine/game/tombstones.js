// mirror: reference/engine/src/game/tombstones.js
// parity: FAITHFUL
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
// Statement-by-statement port (sloppy mode, like the original); token-identical to vanilla apart from comments
// (tools/oracle/api-wp07/normdiff.js). `// vanilla: tombstones.js:N` anchors use the vanilla line numbers.
var utils = require('./../utils'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants,
    _ = require('lodash');

var runtimeData, intents, register, globals;

// vanilla: tombstones.js:8
function _storeGetter(o) {
    if(!o) {
        o = {store: {}, storeCapacity: this.carryCapacity};
    }
    return new Store(o);
}

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.Tombstone)` in make(), where `Tombstone` is make()'s own local;
// out here it is the global, read through its property cell.
function hasTombstone() {
    return typeof Tombstone !== 'undefined' && !!Tombstone;
}

// vanilla: tombstones.js:15
exports.make = function(_runtimeData, _intents, _register, _globals) {

    runtimeData = _runtimeData;
    intents = _intents;
    register = _register;
    globals = _globals;

    if(hasTombstone()) {
        return;
    }

    // vanilla: tombstones.js:26
    var data = (id) => {
        if(!runtimeData.roomObjects[id]) {
            throw new Error("Could not find an object with ID "+id);
        }
        return runtimeData.roomObjects[id];
    };

    // vanilla: tombstones.js:33
    var Tombstone = register.wrapFn(function(id) {
        var _data = data(id);
        RoomObject.call(this, _data.x, _data.y, _data.room, _data.effects);
        this.id = id;
    });

    Tombstone.prototype = Object.create(RoomObject.prototype);
    Tombstone.prototype.constructor = Tombstone;
    // vanilla: tombstones.js:41
    utils.defineGameObjectProperties(Tombstone.prototype, data, {
        deathTime: (o) => o.deathTime,
        store: _storeGetter,
        ticksToDecay: (o) => o.decayTime - runtimeData.time,
        creep: (o) => {
            if(o.creepId) {
                let creep = new Creep();
                RoomObject.call(creep, o.x, o.y, o.room);
                Object.defineProperties(creep, {
                    id: {
                        enumerable: true,
                        get() {
                            return o.creepId;
                        }
                    },
                    name: {
                        enumerable: true,
                        get() {
                            return o.creepName;
                        }
                    },
                    spawning: {
                        enumerable: true,
                        get() {
                            return false;
                        }
                    },
                    my: {
                        enumerable: true,
                        get() {
                            return o.user == runtimeData.user._id;
                        }
                    },
                    body: {
                        enumerable: true,
                        get() {
                            return _.map(o.creepBody, type => ({type, hits: 0}))
                        }
                    },
                    owner: {
                        enumerable: true,
                        get() {
                            return _.isUndefined(o.user) || o.user === null ? undefined : {
                                username: runtimeData.users[o.user].username
                            }
                        }
                    },
                    ticksToLive: {
                        enumerable: true,
                        get() {
                            return o.creepTicksToLive;
                        }
                    },
                    carryCapacity: {
                        enumerable: true,
                        get() {
                            return _.reduce(o.creepBody,
                                (result, type) => result += type === C.CARRY ? C.CARRY_CAPACITY : 0, 0);
                        }
                    },
                    carry: {
                        enumerable: true,
                        get: _storeGetter
                    },
                    store: {
                        enumerable: true,
                        get: _storeGetter
                    },
                    fatigue: {
                        enumerable: true,
                        get() {
                            return 0;
                        }
                    },
                    hits: {
                        enumerable: true,
                        get() {
                            return 0;
                        }
                    },
                    hitsMax: {
                        enumerable: true,
                        get() {
                            return o.creepBody.length * 100;
                        }
                    },
                    saying: {
                        enumerable: true,
                        get() {
                            return o.creepSaying;
                        }
                    }
                });
                return creep;
            }

            if(o.powerCreepId) {

                let powerCreep = new PowerCreep();
                RoomObject.call(powerCreep, o.x, o.y, o.room);
                Object.defineProperties(powerCreep, {
                    id: {
                        enumerable: true,
                        get() {
                            return o.powerCreepId;
                        }
                    },
                    name: {
                        enumerable: true,
                        get() {
                            return o.powerCreepName;
                        }
                    },
                    className: {
                        enumerable: true,
                        get() {
                            return o.powerCreepClassName;
                        }
                    },
                    level: {
                        enumerable: true,
                        get() {
                            return o.powerCreepLevel;
                        }
                    },
                    my: {
                        enumerable: true,
                        get() {
                            return o.user == runtimeData.user._id;
                        }
                    },
                    body: {
                        enumerable: true,
                        get() {
                            return _.map(o.creepBody, type => ({type, hits: 0}))
                        }
                    },
                    owner: {
                        enumerable: true,
                        get() {
                            return _.isUndefined(o.user) || o.user === null ? undefined : {
                                username: runtimeData.users[o.user].username
                            }
                        }
                    },
                    ticksToLive: {
                        enumerable: true,
                        get() {
                            return o.powerCreepTicksToLive;
                        }
                    },
                    carryCapacity: {
                        enumerable: true,
                        get() {
                            return o.powerCreepLevel * 100;
                        }
                    },
                    carry: {
                        enumerable: true,
                        get: _storeGetter
                    },
                    store: {
                        enumerable: true,
                        get: _storeGetter
                    },
                    hits: {
                        enumerable: true,
                        get() {
                            return 0;
                        }
                    },
                    hitsMax: {
                        enumerable: true,
                        get() {
                            return o.powerCreepLevel * 1000;
                        }
                    },
                    saying: {
                        enumerable: true,
                        get() {
                            return o.powerCreepSaying;
                        }
                    },
                    powers: {
                        enumerable: true,
                        get() {
                            return o.powerCreepPowers;
                        }
                    }
                });
                return powerCreep;
            }
        },
    });

    // vanilla: tombstones.js:236
    Tombstone.prototype.toString = register.wrapFn(function() {
        return `[Tombstone #${this.id}]`;
    });

    Object.defineProperty(globals, 'Tombstone', {enumerable: true, value: Tombstone});
};
