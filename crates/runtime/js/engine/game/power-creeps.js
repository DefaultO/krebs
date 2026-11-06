// mirror: reference/engine/src/game/power-creeps.js
// parity: FAITHFUL
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
// Statement-by-statement port (sloppy mode, like the original); token-identical to vanilla apart from comments
// (tools/oracle/api-wp07/normdiff.js). `// vanilla: power-creeps.js:N` anchors use the vanilla line numbers.
var utils = require('./../utils'),
    rooms = require('./rooms'),
    driver = utils.getRuntimeDriver(),
    _ = require('lodash'),
    C = driver.constants;

var runtimeData, intents, register, globals;

// vanilla: power-creeps.js:9
function calcFreePowerLevels() {
    var level = Math.floor(Math.pow((runtimeData.user.power || 0) / C.POWER_LEVEL_MULTIPLY, 1 / C.POWER_LEVEL_POW));
    var used = Object.keys(runtimeData.userPowerCreeps).length + _.sum(runtimeData.userPowerCreeps, 'level');
    return level - used;
}

// vanilla: power-creeps.js:15
function data(id) {
    return Object.assign({}, runtimeData.userPowerCreeps[id], runtimeData.roomObjects[id]);
}

// vanilla: power-creeps.js:19
function _storeGetter(o) {
    return new Store(o);
}

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.PowerCreep)` in make(), where `PowerCreep` is make()'s own local;
// out here it is the global, read through its property cell.
function hasPowerCreep() {
    return typeof PowerCreep !== 'undefined' && !!PowerCreep;
}

// vanilla: power-creeps.js:23
exports.make = function(_runtimeData, _intents, _register, _globals) {

    runtimeData = _runtimeData;
    intents = _intents;
    register = _register;
    globals = _globals;

    if(hasPowerCreep()) {
        return;
    }

    // vanilla: power-creeps.js:34
    var PowerCreep = register.wrapFn(function(id) {
        var _data = data(id);
        if(_data.room) {
            RoomObject.call(this, _data.x, _data.y, _data.room);
        }
        this.id = id;
    });

    PowerCreep.prototype = Object.create(RoomObject.prototype);
    PowerCreep.prototype.constructor = PowerCreep;

    // vanilla: power-creeps.js:45
    utils.defineGameObjectProperties(PowerCreep.prototype, data, {
        name: (o) => o.name,
        my: (o) => o.user == runtimeData.user._id,
        owner: (o) => new Object({username: runtimeData.users[o.user].username}),
        level: (o) => o.level,
        className: (o) => o.className,
        hitsMax: (o) => o.hitsMax,
        hits: (o) => o.hits,
        shard: (o) => o.shard || undefined,
        spawnCooldownTime: (o) => o.spawnCooldownTime !== null && o.spawnCooldownTime > Date.now() ? o.spawnCooldownTime : undefined,
        deleteTime: (o) => o.deleteTime || undefined,
        powers: (o) => _.mapValues(o.powers, i => ({
            level: i.level,
            cooldown: Math.max(0, (i.cooldownTime || 0) - runtimeData.time)
        })),
        saying: o => {
            if(!o.actionLog || !o.actionLog.say) {
                return undefined;
            }
            if(o.user == runtimeData.user._id) {
                return o.actionLog.say.message;
            }
            return o.actionLog.say.isPublic ? o.actionLog.say.message : undefined;
        },
        carry: _storeGetter,
        store: _storeGetter,
        carryCapacity: o => o.storeCapacity,
        ticksToLive: (o) => o.ageTime - runtimeData.time,
    });

    // vanilla: power-creeps.js:75
    Object.defineProperty(PowerCreep.prototype, 'memory', {
        get: function() {
            if(this.id && !this.my) {
                return undefined;
            }
            if(_.isUndefined(globals.Memory.powerCreeps) || globals.Memory.powerCreeps === 'undefined') {
                globals.Memory.powerCreeps = {};
            }
            if(!_.isObject(globals.Memory.powerCreeps)) {
                return undefined;
            }
            return globals.Memory.powerCreeps[this.name] = globals.Memory.powerCreeps[this.name] || {};
        },
        set: function(value) {
            if(this.id && !this.my) {
                throw new Error('Could not set other player\'s creep memory');
            }
            if(_.isUndefined(globals.Memory.powerCreeps) || globals.Memory.powerCreeps === 'undefined') {
                globals.Memory.powerCreeps = {};
            }
            if(!_.isObject(globals.Memory.powerCreeps)) {
                throw new Error('Could not set creep memory');
            }
            globals.Memory.powerCreeps[this.name] = value;
        }
    });

    // vanilla: power-creeps.js:102
    PowerCreep.prototype.toString = register.wrapFn(function() {
        return `[powerCreep ${this.name}]`;
    });

    // vanilla: power-creeps.js:106
    PowerCreep.prototype.move = register.wrapFn(function(target) {
        if(!this.room) {
            return C.ERR_BUSY;
        }
        return Creep.prototype.move.call(this, target);
    });

    // vanilla: power-creeps.js:113
    PowerCreep.prototype.moveTo = register.wrapFn(function(firstArg, secondArg, opts) {
        if(!this.room) {
            return C.ERR_BUSY;
        }
        return Creep.prototype.moveTo.call(this, firstArg, secondArg, opts);
    });

    // vanilla: power-creeps.js:120
    PowerCreep.prototype.moveByPath = register.wrapFn(function(path) {
        if(!this.room) {
            return C.ERR_BUSY;
        }
        return Creep.prototype.moveByPath.call(this, path);
    });

    // vanilla: power-creeps.js:127
    PowerCreep.prototype.transfer = register.wrapFn(function(target, resourceType, amount) {
        if(!this.room) {
            return C.ERR_BUSY;
        }
        return Creep.prototype.transfer.call(this, target, resourceType, amount);
    });

    // vanilla: power-creeps.js:134
    PowerCreep.prototype.withdraw = register.wrapFn(function(target, resourceType, amount) {
        if(!this.room) {
            return C.ERR_BUSY;
        }
        return Creep.prototype.withdraw.call(this, target, resourceType, amount);
    });

    // vanilla: power-creeps.js:141
    PowerCreep.prototype.drop = register.wrapFn(function(resourceType, amount) {
        if(!this.room) {
            return C.ERR_BUSY;
        }
        return Creep.prototype.drop.call(this, resourceType, amount);
    });

    // vanilla: power-creeps.js:148
    PowerCreep.prototype.pickup = register.wrapFn(function(target) {
        if(!this.room) {
            return C.ERR_BUSY;
        }
        return Creep.prototype.pickup.call(this, target);
    });

    // vanilla: power-creeps.js:155
    PowerCreep.prototype.say = register.wrapFn(function(message, isPublic) {
        if(!this.room) {
            return C.ERR_BUSY;
        }
        return Creep.prototype.say.call(this, message, isPublic);
    });

    // vanilla: power-creeps.js:162
    PowerCreep.prototype.spawn = register.wrapFn(function(powerSpawn) {
        if(this.room) {
            return C.ERR_BUSY;
        }
        if(!(powerSpawn instanceof StructurePowerSpawn)) {
            return C.ERR_INVALID_TARGET;
        }
        if(!this.my || !powerSpawn.my) {
            return C.ERR_NOT_OWNER;
        }
        if(!utils.checkStructureAgainstController(data(powerSpawn.id), register.objectsByRoom[data(powerSpawn.id).room], data(powerSpawn.room.controller.id))) {
            return C.ERR_RCL_NOT_ENOUGH;
        }

        if(this.spawnCooldownTime) {
            return C.ERR_TIRED;
        }

        if(_.isUndefined(globals.Memory.powerCreeps)) {
            globals.Memory.powerCreeps = {};
        }
        if(_.isObject(globals.Memory.powerCreeps) && _.isUndefined(globals.Memory.powerCreeps[this.name])) {
            globals.Memory.powerCreeps[this.name] = {};
        }

        intents.pushByName('global', 'spawnPowerCreep', {id: powerSpawn.id, name: this.name}, 50);
        return C.OK;
    });

    // vanilla: power-creeps.js:191
    PowerCreep.prototype.suicide = register.wrapFn(function() {

        if(!this.room) {
            return C.ERR_BUSY;
        }
        if(!this.my) {
            return C.ERR_NOT_OWNER;
        }

        intents.pushByName('global', 'suicidePowerCreep', {id: this.id}, 50);
        return C.OK;
    });

    // vanilla: power-creeps.js:204
    PowerCreep.prototype.delete = register.wrapFn(function(cancel) {

        if(this.room) {
            return C.ERR_BUSY;
        }
        if(!this.my) {
            return C.ERR_NOT_OWNER;
        }

        intents.pushByName('global', 'deletePowerCreep', {id: this.id, cancel}, 50);
        return C.OK;
    });

    // vanilla: power-creeps.js:217
    PowerCreep.prototype.upgrade = register.wrapFn(function(power) {

        if(!this.my) {
            return C.ERR_NOT_OWNER;
        }
        if(calcFreePowerLevels() <= 0) {
            return C.ERR_NOT_ENOUGH_RESOURCES;
        }
        if(this.level >= C.POWER_CREEP_MAX_LEVEL) {
            return C.ERR_FULL;
        }
        var powerInfo = C.POWER_INFO[power];
        if(!powerInfo || powerInfo.className !== this.className) {
            return C.ERR_INVALID_ARGS;
        }
        var powerData = data(this.id).powers[power];
        var powerLevel = powerData ? powerData.level : 0;
        if(powerLevel == 5) {
            return C.ERR_FULL;
        }

        if(this.level < powerInfo.level[powerLevel]) {
            return C.ERR_FULL;
        }

        intents.pushByName('global', 'upgradePowerCreep', {id: this.id, power}, 50);
        return C.OK;
    });

    // vanilla: power-creeps.js:246
    PowerCreep.prototype.usePower = register.wrapFn(function(power, target) {

        if(!this.my) {
            return C.ERR_NOT_OWNER;
        }
        if(!this.room) {
            return C.ERR_BUSY;
        }
        if(this.room.controller) {
            if (!this.room.controller.isPowerEnabled) {
                return C.ERR_INVALID_ARGS;
            }
            if (!this.room.controller.my && this.room.controller.safeMode) {
                return C.ERR_INVALID_ARGS;
            }
        }

        var powerData = data(this.id).powers[power];
        var powerInfo = C.POWER_INFO[power];
        if(!powerData || !powerData.level || !powerInfo) {
            return C.ERR_NO_BODYPART;
        }
        if(powerData.cooldownTime > runtimeData.time) {
            return C.ERR_TIRED;
        }
        var ops = powerInfo.ops || 0;
        if(_.isArray(ops)) {
            ops = ops[powerData.level-1];
        }
        if((data(this.id).store.ops || 0) < ops) {
            return C.ERR_NOT_ENOUGH_RESOURCES;
        }
        if(powerInfo.range) {
            if(!target) {
                return C.ERR_INVALID_TARGET;
            }
            if(!this.pos.inRangeTo(target, powerInfo.range)) {
                return C.ERR_NOT_IN_RANGE;
            }
            var currentEffect = _.find(target.effects, i => i.power == power);
            if(currentEffect && currentEffect.level > powerData.level && currentEffect.ticksRemaining > 0) {
                return C.ERR_FULL;
            }
        }

        intents.set(this.id, 'usePower', {power, id: target ? target.id : undefined});
        return C.OK;
    });

    // vanilla: power-creeps.js:295
    PowerCreep.prototype.enableRoom = register.wrapFn(function(target) {

        if(!this.my) {
            return C.ERR_NOT_OWNER;
        }
        if(!this.room) {
            return C.ERR_BUSY;
        }

        if(!target || !target.id || !register.structures[target.id] || !(target instanceof Structure)) {
            register.assertTargetObject(target);
            return C.ERR_INVALID_TARGET;
        }
        if(!target.pos.isNearTo(this.pos)) {
            return C.ERR_NOT_IN_RANGE;
        }
        if(target.structureType != 'controller' || target.safeMode && !target.my) {
            return C.ERR_INVALID_TARGET;
        }

        intents.set(this.id, 'enableRoom', {id: target.id});
        return C.OK;
    });

    // vanilla: power-creeps.js:319
    PowerCreep.prototype.renew = register.wrapFn(function(target) {

        if(!this.my) {
            return C.ERR_NOT_OWNER;
        }
        if(!this.room) {
            return C.ERR_BUSY;
        }

        if(!target || !target.id || !register.structures[target.id] || !(target instanceof StructurePowerBank) && !(target instanceof StructurePowerSpawn)) {
            register.assertTargetObject(target);
            return C.ERR_INVALID_TARGET;
        }
        if((target instanceof StructurePowerSpawn) && !utils.checkStructureAgainstController(data(target.id), register.objectsByRoom[data(target.id).room], data(target.room.controller.id))) {
            return C.ERR_RCL_NOT_ENOUGH;
        }
        if(!target.pos.isNearTo(this.pos)) {
            return C.ERR_NOT_IN_RANGE;
        }
        intents.set(this.id, 'renew', {id: target.id});
        return C.OK;
    });

    // vanilla: power-creeps.js:342
    PowerCreep.prototype.cancelOrder = register.wrapFn(function(name) {

        if(!this.room) {
            return C.ERR_BUSY;
        }
        if(!this.my) {
            return C.ERR_NOT_OWNER;
        }
        if(intents.remove(this.id, name)) {
            return C.OK;
        }
        return C.ERR_NOT_FOUND;
    });

    // vanilla: power-creeps.js:356
    PowerCreep.prototype.rename = register.wrapFn(function(name) {

        if(this.room) {
            return C.ERR_BUSY;
        }
        if(!this.my) {
            return C.ERR_NOT_OWNER;
        }
        if(!name || !_.isString(name) || (name.length > 100)) {
            return C.ERR_INVALID_ARGS;
        }
        if(_.any(runtimeData.userPowerCreeps, {name})) {
            return C.ERR_NAME_EXISTS;
        }

        intents.pushByName('global', 'renamePowerCreep', {id: this.id, name}, 50);
        return C.OK;
    });

    // vanilla: power-creeps.js:375
    PowerCreep.prototype.notifyWhenAttacked = register.wrapFn(function(enabled) {

        if(!this.room) {
            return C.ERR_BUSY;
        }
        if(!this.my) {
            return C.ERR_NOT_OWNER;
        }
        if(!_.isBoolean(enabled)) {
            return C.ERR_INVALID_ARGS;
        }

        if(enabled != data(this.id).notifyWhenAttacked) {

            intents.set(this.id, 'notifyWhenAttacked', {enabled});
        }

        return C.OK;
    });

    // vanilla: power-creeps.js:395
    PowerCreep.create = register.wrapFn(function(name, className) {
        if(!name || !_.isString(name) || (name.length > 100)) {
            return C.ERR_INVALID_ARGS;
        }
        if(calcFreePowerLevels() <= 0) {
            return C.ERR_NOT_ENOUGH_RESOURCES;
        }
        if(_.any(runtimeData.userPowerCreeps, {name})) {
            return C.ERR_NAME_EXISTS;
        }
        if(Object.values(C.POWER_CLASS).indexOf(className) === -1) {
            return C.ERR_INVALID_ARGS;
        }
        intents.pushByName('global', 'createPowerCreep', {name, className}, 50);
        return C.OK;
    });



    Object.defineProperty(globals, 'PowerCreep', {enumerable: true, value: PowerCreep});
};

