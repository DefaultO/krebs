// not a mirror: module table, _init(natives), __tick (JS half of data.js + runtime.js _start/run). Owner: WP05.
//
// BUNDLE (runtime/src/host.rs builds it; tools/oracle/api/bundle.js builds the same text for the oracle):
//     <this file>
//     __chitinDefine("<path under js/>", function(module, exports, require) {
//     <file source>
//     });
// once per bundle.rs FILES entry except host/* and cli/*, in FILES order, then
//     __chitinDefine("common/constants.js", function(module, exports, require) {
//     module.exports = <engine::constants::constants_json()>;
//     });
// compiled as ONE sloppy script named `<runtime>` (ENV-SNAPSHOT). The wrapper adds no directive, so every file
// keeps its own strictness (ENV-ENGINE-STRICTNESS, webpack 3 does the same). Top-level evaluation only
// registers functions: a module runs on its first require (inside _init), as in vanilla's webpack bundle.
// `require` inside a module resolves vanilla's own specifiers ('lodash', '../utils', '@screeps/engine/src/utils',
// '~runtime-driver', ...) against the module's vanilla path; an unknown one throws `Cannot find module "x"`.
//
// HOST CONTRACT (§6.3, §6.6, §6.7):
//   tick = _init(natives)   once per isolate, before any player code. Installs vanilla's bootstrap globals as
//                           adapters over the natives, runs runtime.js's _init, deletes every bootstrap global
//                           (user-vm.js:42-56; only `global` survives) and returns the __tick function.
//   out = tick(meta, blobs, eventLogs, memory, segments, foreignSegment, modules, plans)
//     meta            per-user JSON string (§6.6 FROZEN shape; cpu/cpuBucket null = Infinity). `codeVersion` becomes
//                     vanilla's userCodeTimestamp, which bots read as module.timestamp / require.timestamp: it
//                     must be the active branch's `timestamp` (data.js:197, `|| 0`), not CodeState.version.
//     blobs           array parallel to meta.rooms: room blob JSON string, or null/undefined (not a room)
//     eventLogs       array parallel to the VISION rooms of meta.rooms: event-log string or null
//     memory          Memory string, or undefined = the string this VM saw last (passed in, or produced by the
//                     last run that returned). After a run whose Memory the host did not commit (total error,
//                     clone failure, Memory over 2 MiB) the host passes the stored string again.
//     segments        {id: string} of the active segments ({} or undefined when none)
//     foreignSegment  {username, id, data} or undefined. The `data` key is always present; its value is undefined
//                     for a public segment that was never written (vanilla's env.hget answer, which the storage
//                     RPC drops; fixtures omit it).
//     modules         {name: string | Uint8Array} when the code version changed (always on a new VM), else
//                     undefined (the VM keeps the last set)
//     plans           doc reuse (J11, runtime doc_reuse.rs): JSON [generation, [blob index, rooms doc, ops, changed
//                     docs...], ...] building some rooms' docs from the ones this VM held after its last run; the
//                     blobs stay complete. undefined once the VM called natives.docsOff().
//   `out` is vanilla's outMessage (runtime.js:271-296) except `out.memory`: undefined when the serialized Memory
//   equals the input string, else that string (§6.8). A throw out of tick = vanilla's rejected run (MEM-SERIALIZE-
//   THROW, SEG-SAVE-ERRORS): nothing saved, unbilled; the error text is `e.stack || e`.
//   JS-mod game slots: `__chitinDefine("mods/<name>.js", ...)` after the files above (see _init).
//   System isolate: `exports = _initSystem(natives)` instead of _init (see below).
(function (G) {
    var defs = Object.create(null), installed = Object.create(null), natives;

    // path under js/ -> the vanilla module id webpack would have resolved it to
    function canonical(path) {
        path = path.replace(/\.js$/, '');
        if (path === 'vendor/lodash') return 'lodash';
        if (path.indexOf('common/') === 0) return '@screeps/common/lib/' + path.slice(7);
        if (path.indexOf('engine/') === 0) return '@screeps/engine/src/' + path.slice(7);
        if (path === 'driver/path-finder') return '@screeps/driver/lib/path-finder';
        if (path.indexOf('driver/') === 0) return '@screeps/driver/lib/runtime/' + path.slice(7);
        return path;
    }

    function resolve(from, spec) {
        if (spec === '~runtime-driver') return '@screeps/driver/lib/runtime/runtime-driver';
        if (spec.charAt(0) !== '.') return spec;
        var parts = from.split('/'), segs = spec.split('/');
        parts.pop();
        for (var i = 0; i < segs.length; i++) {
            if (segs[i] === '..') parts.pop();
            else if (segs[i] !== '.') parts[parts.length] = segs[i];
        }
        // host/* files sit under js/, so their relative requires land on js/ paths; map those to vanilla ids too
        return canonical(parts.join('/'));
    }

    // webpack 3 __webpack_require__: a module that throws stays installed with its partial exports.
    function load(id, spec) {
        if (installed[id]) return installed[id].exports;
        if (!defs[id]) throw new Error('Cannot find module "' + spec + '"');
        var module = installed[id] = {i: id, l: false, exports: {}}, ids = Object.create(null);
        // webpack resolves a specifier once, at build time; makeGameObject requires 16 modules on every tick
        defs[id].call(module.exports, module, module.exports, function require(s) {
            if (typeof s !== 'string') return load(resolve(id, s), s);
            return load(ids[s] || (ids[s] = resolve(id, s)), s);
        });
        module.l = true;
        if (id === 'lodash' && webpackAmd) {
            // The player runtime is webpack's bundle, which compiles lodash's AMD test (lodash.js:12323) to `true`:
            // lodash runs `root._ = _` and exports `_` without the CommonJS `_._ = _` (:12340). So the runtime's own
            // lodash is the global `_`, and game.js:475 never makes a runInContext copy (bot patches to `_` reach
            // engine code). The system isolate is node's realm (make.js), where lodash takes the CommonJS branch.
            delete module.exports._;
            G._ = module.exports;
        }
        return module.exports;
    }
    var webpackAmd = false;

    G.__chitinDefine = function (path, fn) {
        defs[canonical(path)] = fn;
    };

    // J9 (ARCHITECTURE.md §7.2): vanilla's `globals.require`, the same single [[Get]] on the global, from one load site
    // instead of a dozen (each misses once a tick, after the Memory redefinition).
    function globalRequire() {
        return G.require;
    }

    G._init = function (_natives) {
        natives = _natives;
        natives.globalRequire = globalRequire;
        webpackAmd = true;
        // user-vm.js:59-71, same order (`_natives` is chitin's: runtime.js/runtime-driver.js read the §6.7 natives).
        G.global = G;
        G._ivm = {
            Reference: Reference,
            ExternalCopy: ExternalCopy
        };
        G._isolate = {   // compileScriptSync: runtime-driver calls natives.compileAndRun directly
            get cpuTime() {
                return natives.cpuTime();
            },
            getHeapStatisticsSync: function () {
                return natives.heapStats();
            }
        };
        G._context = {};
        G._worldSize = natives.worldSize();
        G._nativeMod = natives.nativeMod;
        G._constants = JSON.parse(JSON.stringify(load('@screeps/common/lib/constants', 'constants')));
        G._customObjectPrototypes = [];
        G._customIntentTypes = {};
        G._halt = {
            applySync: function () {
                natives.halt();
            }
        };
        G._natives = natives;
        // JS-mod game slots (§10.2): every bundle module under mods/ runs before the runtime, in registration
        // order, once every bootstrap global is in place (a mod may require engine modules); its
        // exports may carry customObjectPrototypes [[objectType, name, opts], ...] (registered exactly like
        // driver/lib/index.js:56-73 registerCustomObjectPrototype: functions become source strings that
        // runtime.js evals) and customIntentTypes {intent: {field: type}} (driver/lib/index.js:28).
        for (var id in defs) {
            if (id.indexOf('mods/') !== 0) continue;
            var mod = load(id, id), regs = mod.customObjectPrototypes || [];
            for (var r = 0; r < regs.length; r++) registerCustomObjectPrototype(regs[r][0], regs[r][1], regs[r][2]);
            for (var it in mod.customIntentTypes) G._customIntentTypes[it] = mod.customIntentTypes[it];
        }

        load('@screeps/driver/lib/runtime/runtime', 'runtime');   // defines global._init (runtime.js:1)
        G._init();
        var start = G._start, docsOn = G._customObjectPrototypes.length === 0;

        // user-vm.js:42-56
        delete G._ivm;
        delete G._isolate;
        delete G._context;
        delete G._init;
        delete G._evalFn;
        delete G._start;
        delete G._setStaticTerrainData;
        delete G._worldSize;
        delete G._nativeMod;
        delete G._constants;
        delete G._halt;
        delete G._customObjectPrototypes;
        delete G._customIntentTypes;
        delete G._natives;
        delete G._initSystem;
        delete G.__chitinDefine;

        var jsonParse = JSON.parse, shardName = natives.shardName, codeModules = {}, vmMemory = '';

        // chitin (J11, ARCHITECTURE.md §6.6): room docs unchanged since the VM's last run are taken over from it
        // (runtime doc_reuse.rs) instead of parsed again. Vanilla parses every doc each tick, so a doc the player
        // can reach must not be one it saw before: the host never takes over docs whose getters return a piece of
        // the doc (creep.body, spawning.directions, a tombstone's powers), and reuse stops for good when a mod
        // defines object types or the player has replaced a builtin that engine code hands raw docs or their
        // parts to (prototype methods, accessors on Object.prototype, lodash, Object.entries/assign, ...), as
        // left by _init. Checked with functions captured here, so the check itself calls no player code.
        var ownKeys = Reflect.ownKeys, ownDesc = Object.getOwnPropertyDescriptor, protoOf = Object.getPrototypeOf;
        var lodash = installed.lodash.exports, arrayIterator = protoOf([][Symbol.iterator]());
        var exact = [Object.prototype, Array.prototype, Function.prototype, arrayIterator, protoOf(arrayIterator), protoOf(lodash([]))].map(function (o) {
            var keys = ownKeys(o);
            return [o, protoOf(o), keys, keys.map(function (k) { return ownDesc(o, k); })];
        });
        var kept = [[Object], [Array], [JSON], [Reflect], [lodash], [lodash.prototype], [G, ['Object', 'Array', 'JSON', 'Reflect', 'Symbol', 'Function']]].map(function (e) {
            var keys = e[1] || ownKeys(e[0]);
            return [e[0], keys, keys.map(function (k) { return ownDesc(e[0], k); })];
        });
        var heldRooms = null, heldGen = -1;

        function same(d, was) {
            return d !== undefined && d.value === was.value && d.get === was.get && d.set === was.set &&
                d.enumerable === was.enumerable && d.writable === was.writable && d.configurable === was.configurable;
        }

        // chitin, match only (runner.setupList, ARCHITECTURE.md "What differs in a match"): the docs of this tick in
        // roomObjects' key order for Game setup (game.js), when that is what for-in and `_.forEach` would visit: pristine()
        // held, no id sorts as an array index (those are 10 digits at most) and no room is listed twice. Null on any other
        // tick and always on the game server. It is filled only after pristine() held, with no player code run since,
        // so no setter on Object.prototype or Array.prototype can see a doc go in.
        var setupList = natives.setupList === true, objectList = null, listedRooms = null;

        function listDoc(doc) {
            var id = doc._id;
            if (typeof id === 'string' && id.length >= 11) objectList[objectList.length] = doc;
            else objectList = null;
        }

        // exact: same prototype and own properties; kept: the listed properties unchanged, others may be added
        function pristine() {
            var i, j, e;
            for (i = 0; i < exact.length; i++) {
                e = exact[i];
                var keys = ownKeys(e[0]);
                if (protoOf(e[0]) !== e[1] || keys.length !== e[2].length) return false;
                for (j = 0; j < keys.length; j++) if (keys[j] !== e[2][j] || !same(ownDesc(e[0], keys[j]), e[3][j])) return false;
            }
            for (i = 0; i < kept.length; i++) {
                e = kept[i];
                for (j = 0; j < e[1].length; j++) if (!same(ownDesc(e[0], e[1][j]), e[2][j])) return false;
            }
            return true;
        }

        // own docs of every blob are ascending by seq; merge the runs pairwise (ORDER-RUNTIME-OBJECTS)
        function merge(a, b) {
            var out = [], i = 0, j = 0;
            while (i < a.length && j < b.length) out[out.length] = a[i][0] <= b[j][0] ? a[i++] : b[j++];
            while (i < a.length) out[out.length] = a[i++];
            while (j < b.length) out[out.length] = b[j++];
            return out;
        }

        return function __tick(metaJson, blobs, eventLogs, memory, segments, foreignSegment, modules, plans) {
            natives.mark(-1);   // chitin bench timing (runtime timing::mark), here and below
            var meta = jsonParse(metaJson), me = meta.user._id, i, j;

            if (modules !== undefined) {
                codeModules = {};
                for (var name in modules) {
                    // game.js:561 expects {binary} for binary modules; runtime-driver turns the bytes into a Buffer
                    codeModules[name] = typeof modules[name] === 'string' ? modules[name] : {binary: modules[name]};
                }
            }
            if (memory !== undefined) {
                vmMemory = memory;
            }

            // data.js:113-221, JS half: foreign docs of vision rooms in meta.rooms order (each blob in seq order),
            // then every own doc merged by seq; rooms docs of vision rooms in RoomIdx order.
            var roomObjects = {}, userObjects = {}, rooms = {}, roomEventLog = {}, roomDocs = [], runs = [], v = 0;
            if (docsOn && !pristine()) {
                docsOn = false;
                heldRooms = null;
            }
            if (!docsOn && plans !== undefined) natives.docsOff();
            if (setupList) {
                objectList = docsOn ? [] : null;
                listedRooms = {__proto__: null};
            }
            // J11: a plan for another generation (the VM did not take the last run's input) is dropped
            var plan = docsOn && plans !== undefined ? jsonParse(plans) : null, p = 1;
            var planned = plan && plan[0] === heldGen ? plan.length : 0, nextRooms = plan ? {} : null;
            for (i = 0; i < meta.rooms.length; i++) {
                var entry = meta.rooms[i], vision = entry[1], room = null, docs = null, seqs = null;
                var held = p < planned && plan[p][0] === i ? heldRooms[entry[0]] : undefined;
                if (held) {
                    var item = plan[p++], ops = item[2], f = 3;
                    room = item[1];
                    docs = [];
                    seqs = [];
                    for (j = 0; j < ops.length; j++) {
                        var k = ops[j];
                        if (k < 0) {
                            docs[docs.length] = item[f++];
                            seqs[seqs.length] = -1 - k;
                        } else {
                            for (var end = k + ops[++j]; k < end; k++) {
                                docs[docs.length] = held[0][k];
                                seqs[seqs.length] = held[1][k];
                            }
                        }
                    }
                } else if (blobs[i] != null) {
                    var blob = jsonParse(blobs[i]);
                    room = blob.room;
                    docs = blob.objects;
                    seqs = blob.seq;
                }
                if (nextRooms && docs) nextRooms[entry[0]] = [docs, seqs];
                if (vision) {
                    roomEventLog[entry[0]] = eventLogs[v] == null ? null : eventLogs[v];
                    v++;
                    if (docs) roomDocs[roomDocs.length] = [entry[2], room];
                }
                if (!docs) continue;
                if (objectList) {
                    if (listedRooms[entry[0]] === true) objectList = null;
                    listedRooms[entry[0]] = true;
                }
                var own = [];
                for (j = 0; j < docs.length; j++) {
                    var doc = docs[j];
                    if (doc.user === me) own[own.length] = [seqs[j], doc];
                    else if (vision) {
                        roomObjects[doc._id] = doc;
                        if (objectList) listDoc(doc);
                    }
                }
                if (own.length) runs[runs.length] = own;
            }
            if (nextRooms) {
                heldRooms = nextRooms;
                heldGen = plan[0] + 1;
            }
            while (runs.length > 1) {
                var next = [];
                for (i = 0; i < runs.length; i += 2) next[next.length] = i + 1 < runs.length ? merge(runs[i], runs[i + 1]) : runs[i];
                runs = next;
            }
            if (runs.length) {
                for (i = 0; i < runs[0].length; i++) {
                    userObjects[runs[0][i][1]._id] = runs[0][i][1];
                    roomObjects[runs[0][i][1]._id] = runs[0][i][1];
                    if (objectList) listDoc(runs[0][i][1]);
                }
            }
            for (i = 1; i < roomDocs.length; i++) {          // insertion sort by RoomIdx (few rooms, stable)
                var item = roomDocs[i];
                for (j = i - 1; j >= 0 && roomDocs[j][0] > item[0]; j--) roomDocs[j + 1] = roomDocs[j];
                roomDocs[j + 1] = item;
            }
            for (i = 0; i < roomDocs.length; i++) rooms[roomDocs[i][1]._id] = roomDocs[i][1];

            var data = {
                userObjects: userObjects,
                user: meta.user,
                userCode: codeModules,
                userCodeTimestamp: meta.codeVersion,
                userMemory: {data: vmMemory || "", userId: me},
                consoleCommands: meta.consoleCommands,
                time: meta.time,
                rooms: rooms,
                roomObjects: roomObjects,
                flags: meta.flags,
                transactions: meta.transactions,
                cpu: meta.cpu == null ? Infinity : meta.cpu,
                cpuBucket: meta.cpuBucket == null ? Infinity : meta.cpuBucket,
                roomEventLog: roomEventLog,
                userPowerCreeps: meta.powerCreeps,
                users: meta.users,
                market: {myOrders: meta.myOrders}
            };
            if (setupList) {
                data.objectList = objectList;
                objectList = listedRooms = null;
            }
            if (segments) data.memorySegments = segments;
            if (foreignSegment) data.foreignMemorySegment = foreignSegment;

            natives.mark(0);
            var run = start(data);

            // make.js:166 playerSandbox, driver/lib/index.js:35-52 (GAME-SHARD); chitin: configured shard name.
            G.Game.shard = G.Object.create(null, {
                name: {value: shardName, writable: true, enumerable: true},
                type: {value: 'normal', writable: true, enumerable: true},
                ptr: {value: false, enumerable: true}
            });

            natives.mark(1);
            var out = run.applySync();
            natives.mark(3);
            var saved = out.memory.data;
            if (typeof saved !== 'string') {
                // a truthy RawMemory._parsed that JSON.stringify turns into undefined (toJSON returning nothing, a
                // function): vanilla's run returns, then make.js:254-263 reads `runResult.memory.data.length` and
                // rejects the whole run (nothing saved). The VM keeps its last committed Memory.
                throw new TypeError("Cannot read properties of undefined (reading 'length')");
            }
            out.memory = saved === vmMemory ? undefined : saved;
            vmMemory = saved;
            return out;
        };
    };

    // SYSTEM ISOLATE (§4.2, §6.5): same bundle format with vendor/lodash.js, common/system.js, common/constants.js,
    // engine/utils.js and host/system.js. `exports = _initSystem(natives)` evaluates host/system.js (natives are
    // `global._natives` while it loads) and returns its exports. engine/utils.js reaches the driver through
    // '~runtime-driver'; here that is the node-realm driver make.js:192 storeIntents runs against: system + constants.
    G._initSystem = function (_natives) {
        natives = _natives;
        G.global = G;   // lodash's root (lodash.js:250-274), as in the node realm
        G._natives = natives;
        defs['@screeps/driver/lib/runtime/runtime-driver'] = defs['@screeps/driver/lib/runtime/runtime-driver'] || function (module, exports, require) {
            exports.system = require('@screeps/common/lib/system');
            exports.constants = require('@screeps/common/lib/constants');
        };
        var exports = load('host/system', 'host/system');
        delete G._natives;
        delete G._initSystem;
        delete G._init;
        delete G.__chitinDefine;
        return exports;
    };

    // driver/lib/index.js:56-73
    function registerCustomObjectPrototype(objectType, name, opts) {
        if (!objectType) {
            throw new Error('No object type provided!');
        }
        if (!name) {
            throw new Error('No prototype name provided!');
        }
        opts.properties = opts.properties || {};
        for (var prop in opts.properties) {
            opts.properties[prop] = '' + opts.properties[prop];
        }
        if (opts.prototypeExtender) {
            opts.prototypeExtender = '' + opts.prototypeExtender;
        }
        G._customObjectPrototypes.push({objectType: objectType, name: name, opts: opts});
    }

    // isolated-vm pieces runtime.js uses (Reference#applySync, ExternalCopy#copyInto). The host copies the
    // returned outMessage itself (ValueSerializer, §4.2), so copyInto hands it over as is.
    function Reference(fn) {
        this.fn = fn;
    }
    Reference.prototype.applySync = function () {
        var fn = this.fn;
        return fn();
    };
    function ExternalCopy(value) {
        this.value = value;
    }
    ExternalCopy.prototype.copyInto = function () {
        return this.value;
    };
})(this);
