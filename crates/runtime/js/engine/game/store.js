// mirror: reference/engine/src/game/store.js
// parity: FAITHFUL
// FAITHFUL on the game server. A match shares one Proxy handler (runner.setupList): ARCHITECTURE.md "What differs in a match".
var _ = require('lodash'),
    utils = require('./../utils'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants;

var runtimeData, intents, register, globals;

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.Store)` in make(), where `Store` is make()'s own local;
// out here it is the global, read through its property cell.
function hasStore() {
    return typeof Store !== 'undefined' && !!Store;
}

// vanilla: the handler literal of `new Proxy` in store.js
function makeStoreHandler() {
    return {
        get(target, name) {
            if(target[name] !== undefined) {
                return target[name];
            }
            if(C.RESOURCES_ALL.indexOf(name) !== -1) {
                return 0;
            }
        }
    };
}

// chitin, match only (runner.setupList, ARCHITECTURE.md "What differs in a match"): one handler for every Store while
// `Proxy` is the original, and a player's replacement gets its own per Store. A player can reach the shared one: a trap
// a Proxy finds on Object.prototype (`Object.prototype.has`, say) runs with `this` = handler, and a change to it then
// reaches every Store of the VM. On the game server storeHandler stays null and vanilla's statement runs.
var originalProxy, storeHandler = null;
if(driver.natives.setupList === true) {
    originalProxy = Proxy;
    storeHandler = makeStoreHandler();
}

// vanilla: store.js:8
exports.make = function(_runtimeData, _intents, _register, _globals) {

    runtimeData = _runtimeData;
    intents = _intents;
    register = _register;
    globals = _globals;

    if (hasStore()) {
        return;
    }

    // vanilla: store.js:19 - truthy amounts become own props; the instance is wrapped in a Proxy whose `get`
    // answers 0 for any unset RESOURCES_ALL key and undefined otherwise.
    var Store = register.wrapFn(function(object) {

        Object.entries(object.store).forEach(([resourceType, resourceAmount]) => {
            if(resourceAmount) {
                this[resourceType] = resourceAmount;
            }
        });

        Object.defineProperties(this, {
            getCapacity: {
                value: function getCapacity(resource) {
                    if(!resource) {
                        return object.storeCapacityResource ? null : object.storeCapacity || null;
                    }
                    return utils.capacityForResource(object, resource) || null;
                }
            },
            getUsedCapacity: {
                value: function getUsedCapacity(resource) {
                    if(!resource) {
                        if(!!object.storeCapacityResource && (utils.capacityForResource(object, resource) === 0)) {
                            return null;
                        }
                        if(this._sum === undefined) {
                            Object.defineProperty(this, '_sum', {
                                value: _.sum(object.store)
                            });
                        }
                        return this._sum;
                    }
                    return object.store[resource] || (!!object.storeCapacityResource && utils.capacityForResource(object, resource) === 0 ? null : 0);
                }
            },
            getFreeCapacity: {
                value: function getFreeCapacity(resource) {
                    if(utils.capacityForResource(object, resource) === 0) {
                        return null;
                    }

                    if(!object.storeCapacity) {
                        return this.getCapacity(resource) - this.getUsedCapacity(resource);
                    }

                    const capacity = this.getCapacity(resource);
                    if(!capacity) {
                        return null;
                    }

                    if(object.storeCapacityResource) {
                        return capacity - this.getUsedCapacity(resource);
                    }

                    if(this._sum === undefined) {
                        Object.defineProperty(this, '_sum', {
                            value: _.sum(object.store)
                        });
                    }

                    return capacity - this._sum;
                }
            },
            toString: {
                value: function toString() {
                    return `[store]`;
                }
            }
        });

        if(storeHandler) {
            var StoreProxy = Proxy;
            return new StoreProxy(this, StoreProxy === originalProxy ? storeHandler : makeStoreHandler());
        }

        return new Proxy(this, {
            get(target, name) {
                if(target[name] !== undefined) {
                    return target[name];
                }
                if(C.RESOURCES_ALL.indexOf(name) !== -1) {
                    return 0;
                }
            }
        });
    });

    Object.defineProperty(globals, 'Store', {enumerable: true, value: Store});

};
