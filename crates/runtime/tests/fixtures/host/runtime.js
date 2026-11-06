// Host-contract fixture for the runtime conformance tests (not the game runtime), loaded by the fixture
// prelude's _init. A vanilla-shaped miniature of the driver runtime, its evalCode and the game run(): _init(natives) returns
// __tick; modules evaluate through natives.compileAndRun with the MOD-TIMEOUT rule and the CON-ERR-FORMAT
// rewrite; outMessage is built as the driver runtime builds it, except that Memory is only returned when it
// changed (ARCHITECTURE.md 6.8). Test bots reach the host through the global `H`.
// (No file paths in these comments: tools/parity-report counts them as fixture tags.)
var _ = require('lodash');

var natives;
var memory = '';
var code = {};
var currentTimeout;

function EvalCodeError(message) {
    this.toString = function () { return message; };
}

// runtime-driver.js:21-122, with the compile+run pair as one native.
function evalCode(module, timeout, kind) {
    var oldModule = global.__module || {};
    global.__module = module;
    var t;
    if (timeout !== undefined && timeout !== null && timeout !== 0 && timeout != Infinity) {
        t = timeout + 5;
        if (t < 30) t = 30;
    }
    var src = kind === 'console'
        ? '(function(code,module,exports) { return "" + eval(code); })(' + JSON.stringify(module.code) + ', __module, __module.exports)'
        : '(function __module(module,exports){ ' + module.code + '\n})(__module, __module.exports)';
    try {
        var result = natives.compileAndRun(src, module.name, t, kind);
        global.module = oldModule;
        return result;
    } catch (e) {
        if (e instanceof EvalCodeError) throw e;
        if (e.message === 'Script execution timed out.') {
            e.message = 'Script execution timed out: CPU time limit reached';
        }
        var message = '';
        if (e.stack) {
            message = e.stack.replace(/</g, '&lt;').replace(/ *at.*?$/, '').replace(/_console\d+:\d+/, 'console')
                .replace(/at __module \((.*)\)/g, 'at $1');
        } else {
            message = e.message;
        }
        throw new EvalCodeError(message);
    }
}

// game.js:551-594
function requireFn(name) {
    name = name.replace(/^\.\//, '');
    var cache = global.require.cache;
    if (!(name in cache)) {
        if (code[name] === undefined) throw new Error("Unknown module '" + name + "'");
        if (typeof code[name] !== 'string') {
            cache[name] = code[name];
        } else {
            cache[name] = -1;
            var mod = {exports: {}, name: name, code: code[name]};
            try {
                evalCode(mod, currentTimeout, 'module');
            } catch (e) {
                delete cache[name];
                throw e;
            }
            cache[name] = mod.exports;
        }
    } else if (cache[name] === -1) {
        throw new Error("Circular reference to module '" + name + "'");
    }
    return cache[name];
}

function tick(meta, blobs, eventLogs, mem, segments, foreign, modules) {
    var data = JSON.parse(meta);
    var memoryPassed = mem !== undefined;
    if (memoryPassed) memory = mem;
    if (modules !== undefined) {
        code = modules;
        global.require = undefined;
    }
    var startDirtyTime = natives.cpuTime(), startTime;
    var intents = {
        list: Object.create(null),
        cpu: 0,
        set: function (id, name, value) {
            this.list[id] = this.list[id] || Object.create(null);
            if (name !== 'say' && name !== 'pull' && !this.list[id][name]) this.cpu += 0.2;
            this.list[id][name] = value;
        }
    };
    var parsed;
    var rawMemory = {segments: Object.create(null)};
    for (var k in segments) rawMemory.segments[k] = segments[k];
    var logs = [], results = [];
    Object.defineProperty(global, 'Memory', {
        configurable: true,
        enumerable: true,
        get: function () {
            try {
                parsed = JSON.parse(memory || '{}');
                parsed.__proto__ = null;
            } catch (e) {
                parsed = null;
            }
            Object.defineProperty(global, 'Memory', {configurable: true, enumerable: true, value: parsed});
            return parsed;
        }
    });
    global.H = {
        natives: natives, intents: intents, data: data, blobs: blobs, eventLogs: eventLogs, foreign: foreign,
        raw: rawMemory, memoryPassed: memoryPassed, bundleRequire: require, modules: code,
        log: function (m) { logs.push('' + m); }
    };
    currentTimeout = data.cpu;
    if (!global.require) {
        global.require = requireFn;
        global.require.cache = {lodash: _};
    }
    var outMessage = Object.create(null);
    startTime = natives.cpuTime();
    try {
        var main = global.require('main');
        if (main && typeof main.loop === 'function') {
            evalCode({exports: main, name: '__mainLoop', code: 'module.exports.loop();'}, data.cpu, 'loop');
        }
        var commands = data.consoleCommands || [];
        for (var i = 0; i < commands.length; i++) {
            results.push(evalCode({exports: {}, name: '_console' + natives.now() + '_' + i, code: commands[i]}, undefined, 'console'));
        }
        outMessage.type = 'done';
    } catch (e) {
        outMessage.type = 'error';
        outMessage.error = _.isObject(e) && e.stack || e.toString();
    }
    if (parsed) {
        var s = JSON.stringify(parsed);
        if (s !== memory) {
            memory = s;
            outMessage.memory = s;
        }
    }
    var segmentKeys = Object.keys(rawMemory.segments);
    if (segmentKeys.length > 0) {
        if (segmentKeys.length > 10) throw 'Cannot save more than 10 memory segments on the same tick';
        outMessage.memorySegments = {};
        for (var j = 0; j < segmentKeys.length; j++) {
            outMessage.memorySegments[parseInt(segmentKeys[j])] = '' + rawMemory.segments[segmentKeys[j]];
        }
    }
    outMessage.usedTime = Math.ceil(natives.cpuTime() - startTime + intents.cpu);
    outMessage.usedCleanTime = natives.cpuTime() - startTime;
    outMessage.usedDirtyTime = Math.ceil(natives.cpuTime() - startDirtyTime);
    outMessage.intentsList = intents.list;
    outMessage.intentsCpu = intents.cpu;
    outMessage.console = {log: logs, results: results};
    outMessage.activeSegments = data.activeSegments;
    return outMessage;
}

global._init = function (n) {
    natives = n;
    return tick;
};
