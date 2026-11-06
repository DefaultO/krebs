// mirror: reference/driver/lib/runtime/runtime.js
// parity: DIVERGENT(J1)
// Evaluated by host/prelude.js inside `_init(natives)`, after it installed vanilla's bootstrap globals
// (`_ivm`, `_isolate`, ... as adapters over the natives, user-vm.js:59-71) - so this stays vanilla's file.
// chitin deviations: `_isolate.cpuTime` is already milliseconds (the configured meter, §6.10); terrain is
// a lazy per-VM view over `natives.terrain` and `_setStaticTerrainData` is gone (J1); the world map grid is
// built from natives (J3); compileScriptSync is not exported (runtime-driver compiles through natives).

// vanilla: webpack 3 hands this module the global object as a wrapper parameter (`WEBPACK VAR INJECTION`,
// module 6 `g = (function() { return this; })()`), so `game.init(global, ...)` below keeps the real global even
// after a player reassigns or deletes the `global` property.
var global = (function() { return this; })();

global._init = (function() {
    const systemFunctions = require('./system-functions');
    const utils = require('@screeps/engine/src/utils');
    const driver = utils.getRuntimeDriver();

    driver.constants = _constants;

    const game = require('@screeps/engine/src/game/game');
    const fakeConsole = require('@screeps/engine/src/game/console');
    const WorldMapGrid = require('./mapgrid');
    const _ = require('lodash');

    const isolate = _isolate;
    const context = _context;
    const ivm = _ivm;
    const natives = _natives;
    const customObjectPrototypes = _customObjectPrototypes;
    const customIntentTypes = _customIntentTypes;
    let mapGrid, staticTerrainData, scope;
    const halt = _halt;
    // vanilla: runtime.js:20 (CPU-HALT)
    const cpuHalt = halt ? function() {
        halt.applySync();
        throw new Error("No one should ever see this message.");
    } : undefined;

    // vanilla: runtime.js:25 - `Number(isolate.cpuTime) / 1e6`; the chitin meter already returns ms.
    function nowCpuTime() {
        return isolate.cpuTime;
    }

    module.exports.context = context;
    // vanilla: runtime.js:30 module.exports.compileScriptSync - chitin: runtime-driver's evalCode calls
    // natives.compileAndRun directly, nothing else used it.

    // vanilla: runtime.js:34 (HOOK-CUSTOM-OBJECT) - mod functions arrive as source strings.
    _.each(customObjectPrototypes, cop => {
        _.each(cop.opts.properties, (fn, key) => {
            cop.opts.properties[key] = eval(`(${fn})`)
        })
        if (cop.opts.prototypeExtender) {
            if(cop.opts.prototypeExtender.startsWith('prototypeExtender')) {
              cop.opts.prototypeExtender = `function ${cop.opts.prototypeExtender}`
            }
            cop.opts.prototypeExtender = eval(`(${cop.opts.prototypeExtender})`)
        }
    })

    // vanilla: runtime.js:18,45 - staticTerrainData[room]: chitin loads each room's copy on first read
    // (natives.terrain) and keeps it for the VM's lifetime; unknown rooms stay undefined (J1). The cache is a plain
    // `{}` like vanilla's, so Object.prototype names ('constructor', '__proto__', ...) resolve to its members.
    const terrainMissing = Object.create(null);
    staticTerrainData = new Proxy({}, {
        get(cache, roomName) {
            if(typeof roomName !== 'string' || roomName in cache || roomName in terrainMissing) {
                return cache[roomName];
            }
            const terrain = natives.terrain(roomName);
            if(terrain === undefined) {
                terrainMissing[roomName] = true;
            }
            else {
                cache[roomName] = terrain;
            }
            return terrain;
        }
    });

    // vanilla: runtime.js:51
    global._evalFn = function(fnString) {
        eval('('+fnString+')(scope)');
    };

    // vanilla: runtime.js:55
    global._start = function (data) {
        systemFunctions.Object.setPrototypeOf(data, null);

        let activeSegments, publicSegments, defaultPublicSegment, activeForeignSegment;
        let startTime, startDirtyTime = nowCpuTime();
        let intentCpu = 0.2,
            freeMethods = systemFunctions.Object.create(null, {say: {value: true}, pull:{value:true}});

        // vanilla: runtime.js:63 (INT-STORE, CPU-INTENT-COST)
        let intents = {
            list: systemFunctions.Object.create(null),
            cpu: 0,
            set(id, name, data) {
                this.list[id] = this.list[id] || systemFunctions.Object.create(null);
                if (!freeMethods[name] && !this.list[id][name]) {
                    this.cpu += intentCpu;
                }
                this.list[id][name] = data;
            },
            push(name, data, maxLen) {
                this.list[name] = this.list[name] || [];
                if (maxLen && this.list[name].length >= maxLen) {
                    return false;
                }
                this.list[name].push(data);
                this.cpu += intentCpu;
                return true;
            },
            pushByName(id, name, data, maxLen) {
                this.list[id] = this.list[id] || systemFunctions.Object.create(null);
                this.list[id][name] = this.list[id][name] || [];
                if (maxLen && this.list[id][name].length >= maxLen) {
                    return false;
                }
                this.list[id][name].push(data);
                this.cpu += intentCpu;
                return true;
            },
            remove(id, name) {
                if (this.list[id] && this.list[id][name]) {
                    delete this.list[id][name];
                    this.cpu -= intentCpu;
                    return true;
                }
                return false;
            }
        };

        // vanilla: runtime.js:102 (MEM-RAW-OBJECT, SEG-*)
        var rawMemory = systemFunctions.Object.create(null, {
            get: {
                value: function () {
                    return data.userMemory.data;
                }
            },
            set: {
                value: function (value) {
                    if (!_.isString(value)) {
                        throw new Error('Raw memory value is not a string');
                    }
                    if (value.length > 2 * 1024 * 1024) {
                        throw new Error('Raw memory length exceeded 2 MB limit');
                    }
                    if (this._parsed) {
                        delete this._parsed;
                    }
                    data.userMemory.data = value;
                }
            },
            segments: {
                value: systemFunctions.Object.create(null)
            },
            interShardSegment: {
                value: '',
                writable: true
            },
            setActiveSegments: {
                value: function (ids) {
                    if (!_.isArray(ids)) {
                        throw new Error(`"${ids}" is not an array`);
                    }
                    if (ids.length > 10) {
                        throw new Error('Only 10 memory segments can be active at the same time');
                    }
                    activeSegments = [];
                    for (var i = 0; i < ids.length; i++) {
                        var id = parseInt(ids[i]);
                        if (_.isNaN(id) || id > 99 || id < 0) {
                            throw new Error(`"${ids[i]}" is not a valid segment ID`);
                        }
                        activeSegments.push(id);
                    }

                }
            },
            setPublicSegments: {
                value: function (ids) {
                    if (!_.isArray(ids)) {
                        throw new Error(`"${ids}" is not an array`);
                    }
                    publicSegments = [];
                    for (var i = 0; i < ids.length; i++) {
                        var id = parseInt(ids[i]);
                        if (_.isNaN(id) || id > 99 || id < 0) {
                            throw new Error(`"${ids[i]}" is not a valid segment ID`);
                        }
                        publicSegments.push(id);
                    }
                }
            },
            setDefaultPublicSegment: {
                value: function (id) {
                    if (id !== null) {
                        id = parseInt(id);
                        if (_.isNaN(id) || id > 99 || id < 0) {
                            throw new Error(`"${id}" is not a valid segment ID`);
                        }
                    }
                    defaultPublicSegment = id;
                }
            },
            setActiveForeignSegment: {
                value: function (username, id) {
                    if (username === null) {
                        activeForeignSegment = null;
                        return;
                    }
                    if (id !== undefined) {
                        id = parseInt(id);
                        if (_.isNaN(id) || id > 99 || id < 0) {
                            throw new Error(`"${id}" is not a valid segment ID`);
                        }
                    }
                    activeForeignSegment = {username, id};
                }
            }
        });

        if (data.memorySegments) {
            for (var i in data.memorySegments) {
                rawMemory.segments[i] = data.memorySegments[i];
            }
        }

        if (data.foreignMemorySegment) {
            rawMemory.foreignSegment = systemFunctions.Object.create(null);
            Object.assign(rawMemory.foreignSegment, data.foreignMemorySegment);
        }

        // vanilla: runtime.js:202 (CPU-GETUSED)
        var getUsedCpu = function () {
            return nowCpuTime() + intents.cpu - startTime;
        };

        // vanilla: runtime.js:206 - built once per VM (J3: from natives, not a parsed accessibleRooms list).
        if(!mapGrid) {
            mapGrid = new WorldMapGrid(undefined, staticTerrainData);
        }

        data.mapGrid = mapGrid;
        data.staticTerrainData = staticTerrainData;
        data.customObjectPrototypes = customObjectPrototypes;

        const outMessage = systemFunctions.Object.create(null);

        scope = game.init(
            global,
            data.userCode,
            data,
            intents,
            rawMemory,
            fakeConsole.makeConsole(data.user._id, function(fn) { return fn }),
            data.consoleCommands,
            data.cpu,
            getUsedCpu,
            function (name) {},
            undefined,
            function() {
                return isolate.getHeapStatisticsSync();
            },
            cpuHalt);

        // vanilla: runtime.js:233 (OUT-SUCCESS, MEM-SERIALIZE, SEG-SAVE) - Memory and segments are serialized
        // outside the try: a throw there rejects the whole run.
        return new ivm.Reference(function() {

            startTime = nowCpuTime();

            try {
                game.run(data.user._id);
                outMessage.type = 'done';
            }
            catch (e) {
                outMessage.type = 'error';
                outMessage.error = _.isObject(e) && e.stack || e.toString();
            }
            natives.mark(2);    // chitin bench timing

            if (rawMemory._parsed) {
                data.userMemory.data = systemFunctions.JSON.stringify(rawMemory._parsed);
            }

            var segmentKeys = Object.keys(rawMemory.segments);
            if (segmentKeys.length > 0) {
                if (segmentKeys.length > 10) {
                    throw 'Cannot save more than 10 memory segments on the same tick';
                }
                outMessage.memorySegments = {};
                for (var i = 0; i < segmentKeys.length; i++) {
                    var key = parseInt(segmentKeys[i]);
                    if (_.isNaN(key) || key < 0 || key > 99) {
                        throw `"${segmentKeys[i]}" is not a valid memory segment ID`;
                    }
                    if (typeof rawMemory.segments[segmentKeys[i]] != 'string') {
                        throw `Memory segment #${segmentKeys[i]} is not a string`;
                    }
                    if (rawMemory.segments[segmentKeys[i]].length > 100 * 1024) {
                        throw `Memory segment #${segmentKeys[i]} has exceeded 100 KB length limit`;
                    }
                    outMessage.memorySegments[key] = "" + rawMemory.segments[segmentKeys[i]];
                }
            }

            outMessage.usedTime = systemFunctions.Math.ceil(nowCpuTime() - startTime + intents.cpu);
            outMessage.usedCleanTime = nowCpuTime() - startTime;
            outMessage.usedDirtyTime = systemFunctions.Math.ceil(nowCpuTime() - startDirtyTime);
            outMessage.intentsList = intents.list;
            outMessage.intentsCpu = intents.cpu;
            outMessage.memory = data.userMemory;
            outMessage.console = {
                log: fakeConsole.getMessages(data.user._id),
                results: fakeConsole.getCommandResults(data.user._id)
            };

            let visual = fakeConsole.getVisual(data.user._id);
            if (Object.keys(visual).length > 0) {
                outMessage.visual = visual;
            }

            outMessage.activeSegments = activeSegments;
            outMessage.activeForeignSegment = activeForeignSegment;
            outMessage.defaultPublicSegment = defaultPublicSegment;
            if (publicSegments) {
                outMessage.publicSegments = publicSegments.join(',');
            }

            data = null;  // Important - preventing memory leak

            return new ivm.ExternalCopy(outMessage).copyInto();
        });
    }
});
