// mirror: reference/engine/src/game/deposits.js
// parity: FAITHFUL
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
// Statement-by-statement port (sloppy mode, like the original); token-identical to vanilla apart from comments
// (tools/oracle/api-wp07/normdiff.js). `// vanilla: deposits.js:N` anchors use the vanilla line numbers.
const utils = require('./../utils'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants;

let runtimeData, intents, register, globals;

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.Deposit)` in make(), where `Deposit` is make()'s own local;
// out here it is the global, read through its property cell.
function hasDeposit() {
    return typeof Deposit !== 'undefined' && !!Deposit;
}

// vanilla: deposits.js:7
exports.make = function(_runtimeData, _intents, _register, _globals) {
    runtimeData = _runtimeData;
    intents = _intents;
    register = _register;
    globals = _globals;

    if(hasDeposit()) {
        return;
    }

    // vanilla: deposits.js:17
    const data = (id) => {
        if(!runtimeData.roomObjects[id]) {
            throw new Error("Could not find an object with ID "+id);
        }
        return runtimeData.roomObjects[id];
    };

    // vanilla: deposits.js:24
    const Deposit = register.wrapFn(function(id) {
        const _data = data(id);
        RoomObject.call(this, _data.x, _data.y, _data.room, _data.effects);
        this.id = id;
    });

    Deposit.prototype = Object.create(RoomObject.prototype);
    Deposit.prototype.constructor = Deposit;

    // vanilla: deposits.js:33
    utils.defineGameObjectProperties(Deposit.prototype, data, {
        depositType: o => o.depositType,
        cooldown: o => o.cooldownTime && o.cooldownTime > runtimeData.time ? o.cooldownTime - runtimeData.time : 0,
        lastCooldown: o => Math.ceil(C.DEPOSIT_EXHAUST_MULTIPLY*Math.pow(o.harvested,C.DEPOSIT_EXHAUST_POW)),
        ticksToDecay: o => o.decayTime ? o.decayTime - runtimeData.time : undefined
    });

    // vanilla: deposits.js:40
    Deposit.prototype.toString = register.wrapFn(function() {
        return `[deposit (${this.depositType}) #${this.id}]`;
    });

    Object.defineProperty(globals, 'Deposit', {enumerable: true, value: Deposit});
};
