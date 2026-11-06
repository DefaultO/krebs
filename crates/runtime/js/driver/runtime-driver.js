// mirror: reference/driver/lib/runtime/runtime-driver.js
// parity: DIVERGENT(ENV-BINARY-BUFFER)
// chitin deviations (host boundary only): bufferFromBase64 goes through natives (no buffer@4.9.1 polyfill,
// ENV-BINARY-BUFFER); evalCode compiles and runs through natives.compileAndRun; `exports.natives` exposes the §6.7
// world/market natives to engine modules (J1, J3) the way vanilla exposes getWorldSize/pathFinder.
const _ = require('lodash');
const runtime = require('./runtime');

// vanilla: runtime-driver.js:4 - no message, no stack: only an own toString (MOD-REQUIRE-ERRORS).
function EvalCodeError(message) {
    this.toString = () => message;
}

var runtimeCache = {};
var worldSize = global._worldSize;
var natives = global._natives;

exports.natives = natives;

// J3 (§6.7): the world and market natives map.js, structures.js and market.js read in place of runtimeData's
// accessibleRooms, mapGrid, roomStatusData and market maps; looked up on the natives object at call time.
// gridData's counts arrive packed (6 bits each, t r b l from bit 0); the object is made here, fresh per call.
exports.gridData = (x, y) => {
    var p = natives.gridData(x, y), o;
    if(p === undefined) {
        return undefined;
    }
    o = {};
    if(p & 63) o.t = p & 63;
    if(p >> 6 & 63) o.r = p >> 6 & 63;
    if(p >> 12 & 63) o.b = p >> 12 & 63;
    if(p >> 18) o.l = p >> 18;
    return o;
};
exports.isAccessible = (roomName) => natives.isAccessible(roomName);
exports.roomStatus = (roomName) => natives.roomStatus(roomName);
exports.marketOrders = (resourceType) => natives.marketOrders(resourceType);
exports.marketOrder = (id) => natives.marketOrder(id);
exports.marketHistory = (resourceType) => natives.marketHistory(resourceType);

exports.system = require('@screeps/common/lib/system');

// vanilla: runtime-driver.js:13 - `Buffer.from(base64, 'base64')`. chitin modules may arrive already decoded
// (Uint8Array); either way the result is a fresh buffer with the internal Buffer prototype.
exports.bufferFromBase64 = (base64) => {
    if(typeof base64 === 'string') {
        return natives.base64ToBuffer(base64);
    }
    return Object.setPrototypeOf(new Uint8Array(base64), Object.getPrototypeOf(natives.base64ToBuffer('')));
};

// vanilla: runtime-driver.js:17
exports.getWorldSize = function() {
    return worldSize;
};

// vanilla: runtime-driver.js:21 (MOD-WRAPPER, MOD-TIMEOUT, CON-ERR-FORMAT)
exports.evalCode = function(module, globals, returnValue, timeout, scriptCachedData) {

    var options = {filename: module.name};

    var oldModule = globals.__module || {};

    Object.defineProperty(module, '__initGlobals', {
        value: undefined,
        writable: false,
        configurable: false
    });
    Object.defineProperty(globals.require, 'initGlobals', {
        value: undefined,
        writable: false,
        configurable: false
    });

    globals.__module = module;

    if(!_.isUndefined(timeout) && timeout !== null && timeout !== 0 && timeout != Infinity) {
        options.timeout = timeout + 5;
        if(options.timeout < 30) {
            options.timeout = 30;
        }
    }

    // chitin: `compileScriptSync(code, options).runSync(context, options)` is one native call,
    // natives.compileAndRun(code, filename, timeoutMs, kind), so the stack between evalCode and the evaluated script
    // holds the same single host frame as isolated-vm's runSync; the host caches compiled code by source.
    try {

        var result;

        if(returnValue) {
            var code = '(function(code,module,exports) { return "" + eval(code); })(' + JSON.stringify(module.code) + ', __module, __module.exports)';
            result = natives.compileAndRun(code, options.filename, options.timeout, 'console');
        }
        else {

            if (!runtimeCache[module.user] || !runtimeCache[module.user].modules[module.name] ||
                runtimeCache[module.user].modules[module.name].timestamp != module.timestamp) {

                var code = '(function __module(module,exports){ ' + module.code + "\n})(__module, __module.exports)";

                var script = code;

                runtimeCache[module.user] = runtimeCache[module.user] || {};
                runtimeCache[module.user].modules = runtimeCache[module.user].modules || {};
                runtimeCache[module.user].modules[module.name] = {
                    timestamp: module.timestamp,
                    script
                };
            }

            result = natives.compileAndRun(runtimeCache[module.user].modules[module.name].script, options.filename,
                options.timeout, module.name === '__mainLoop' ? 'loop' : 'module');
        }

        globals.module = oldModule;

        return result;
    }
    catch(e) {

        if(e instanceof EvalCodeError) throw e;

        if(e.message === 'Script execution timed out.') {
            e.message = 'Script execution timed out: CPU time limit reached';
        }

        var message = '';
        if(e.stack) {
            message = e.stack;
            message = message.replace(/</g,'&lt;');
            message = message.replace(/ *at.*?$/, '');
            message = message.replace(/_console\d+:\d+/, 'console');
            message = message.replace(/at __module \((.*)\)/g, 'at $1');
        }
        else {
            message = e.message;
        }
        throw new EvalCodeError(message);
    }
};

// vanilla: runtime-driver.js:124
const pathFinderFactory = require('../path-finder');
exports.pathFinder = pathFinderFactory.create(global._nativeMod);
