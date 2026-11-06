// mirror: reference/engine/src/game/game.js
// parity: FAITHFUL
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
// J3: spatial register writes are queued for their first read (rooms.js queueSpatial).
// FAITHFUL on the game server. A match also has the object list path (runner.setupList): ARCHITECTURE.md "What differs in a match".
'use strict';
(function() {
    var _ = require('lodash'),
        utils = require('../utils'),
        driver = utils.getRuntimeDriver(),
        C = driver.constants,
        map = require('./map'),
        market = require('./market'),
        customPrototypes = require('./custom-prototypes'),
        rooms = require('./rooms'),
        bindFunction = Function.call.bind(Function.bind),
        objectCreate = Object.create,
        jsonStringify = JSON.stringify,
        jsonParse = JSON.parse,
        copyConstants;

    // chitin, match only (runner.setupList, ARCHITECTURE.md "What differs in a match"): what the object list path below
    // needs. On the game server none of this is set and that path cannot run.
    var setupList = driver.natives.setupList === true, objectKeys, objectDefine, objectSetPrototype, objectPrototype, ProxyConstructor;
    if(setupList) {
        objectKeys = Object.keys;
        objectDefine = Object.defineProperty;
        objectSetPrototype = Object.setPrototypeOf;
        objectPrototype = Object.prototype;
        ProxyConstructor = Proxy;
    }

    var findCacheFn = {
        [C.FIND_CREEPS]: (i) => !i.spawning,
        [C.FIND_MY_CREEPS]: (i) => !i.spawning && i.my,
        [C.FIND_HOSTILE_CREEPS]: (i) => !i.spawning && !i.my,
        [C.FIND_MY_POWER_CREEPS]: (i) => i.my,
        [C.FIND_HOSTILE_POWER_CREEPS]: (i) => !i.my,
        [C.FIND_MY_SPAWNS]: (i) =>  i.my === true,
        [C.FIND_HOSTILE_SPAWNS]: (i) =>  i.my === false,
        [C.FIND_SOURCES_ACTIVE]: (i) => i.energy > 0,
        [C.FIND_MY_STRUCTURES]: (i) =>  i.my === true,
        [C.FIND_HOSTILE_STRUCTURES]: (i) =>  i.my === false && i.owner,
        [C.FIND_MY_CONSTRUCTION_SITES]: (i) =>  i.my,
        [C.FIND_HOSTILE_CONSTRUCTION_SITES]: (i) =>  i.my === false
    };

    // chitin: the engine-internal tables below (register, its byRoom/findCache, _objects and the per-type maps) reach
    // player code only through an accessor on Object.prototype named after a key they lack (ARCHITECTURE.md "Open, older
    // than this work"), so makeGameObject reads each entry once where vanilla reads it again (and writes back what
    // it read); the raw docs' room/x/y are own data properties. All else a player can observe is unchanged: each
    // constructor, `.id`/`.name` read, find predicate (getters), `instanceof` and `push` call, in vanilla's order.

    // vanilla: game.js:30
    function addObjectToFindCache(register, type, objectInstance, objectRaw) {
        if(!findCacheFn[type] || findCacheFn[type](objectInstance)) {
            var byType = register.findCache[type], room = objectRaw.room, list;
            if(!byType) {
                byType = register.findCache[type] = {};
            }
            list = byType[room];
            if(!list) {
                list = byType[room] = [];
            }
            list.push(objectInstance);
        }
    }

    // vanilla: game.js:38
    function addObjectToRegister(register, type, objectInstance, objectRaw) {
        register[type][objectInstance.id] = objectInstance;
        let byRoom = register.byRoom[objectRaw.room];
        byRoom[type][objectInstance.id] = objectInstance;
        let index = objectRaw.x * 50 + objectRaw.y;
        let spatial = byRoom.spatial[type];
        // chitin (J3): vanilla writes spatial[index] here; queued for the room's first look (rooms.js queueSpatial).
        rooms.queueSpatial(objectRaw.room, spatial, index, objectInstance);
    }

    // chitin, match only: addObjectToFindCache and addObjectToRegister for addRoomObject (the object list path), which
    // has read the doc's room and need not know it is a doc
    function addToFindCache(register, type, objectInstance, room) {
        if(!findCacheFn[type] || findCacheFn[type](objectInstance)) {
            var byType = register.findCache[type], list;
            if(!byType) {
                byType = register.findCache[type] = {};
            }
            list = byType[room];
            if(!list) {
                list = byType[room] = [];
            }
            list.push(objectInstance);
        }
    }

    function addToRegister(register, type, objectInstance, objectRaw, room) {
        register[type][objectInstance.id] = objectInstance;
        let byRoom = register.byRoom[room];
        byRoom[type][objectInstance.id] = objectInstance;
        let index = objectRaw.x * 50 + objectRaw.y;
        let spatial = byRoom.spatial[type];
        // chitin (J3): as addObjectToRegister
        rooms.queueSpatial(room, spatial, index, objectInstance);
    }

    // chitin, match only: type 'structures', most of a late tick's objects, with named loads. True when the table now
    // holds the instance under its id: nothing on Object.prototype of that name took the first write, so the caller need
    // not read it back.
    function addStructureToRegister(register, objectInstance, objectRaw, room) {
        var id = objectInstance.id, own = typeof id === 'string' && !(id in objectPrototype);
        register.structures[id] = objectInstance;
        let byRoom = register.byRoom[room];
        byRoom.structures[objectInstance.id] = objectInstance;
        rooms.queueSpatial(room, byRoom.spatial.structures, objectRaw.x * 50 + objectRaw.y, objectInstance);
        return own ? id : undefined;
    }

    // chitin, match only: the engine never reads the ownedStructures tables, so one is written only when Object.prototype
    // holds that name and vanilla's write would show there. A bot can still find them empty, and their spatial tables are
    // not written at all: ARCHITECTURE.md, "What differs in a match".
    function addToOwnedTable(table, id, objectInstance) {
        if(typeof id !== 'string' || id in objectPrototype) {
            table[id] = objectInstance;
        }
    }

    // vanilla: game.js:50
    function makeGameObject ({runtimeData, intents, memory, getUsedCpu, globals, sandboxedFunctionWrapper, getHeapStatistics, cpuHalt}) {

        var customObjectsInfo = {};

        if(runtimeData.customObjectPrototypes) {
            runtimeData.customObjectPrototypes.forEach(i => {
                i.opts = i.opts || {};
                customObjectsInfo[i.objectType] = {
                    name: i.name,
                    make: customPrototypes(i.name, i.opts.parent, i.opts.properties, i.opts.prototypeExtender,
                        !!i.opts.userOwned),
                    findConstant: i.opts.findConstant,
                    lookConstant: i.opts.lookConstant
                };
            });
        }

        var register = {
            _useNewPathFinder: true,
            _objects: {},
            byRoom: {},
            findCache: {},
            rooms: {},
            roomEventLogCache: {},
            wrapFn: sandboxedFunctionWrapper || function(fn) { return fn }
        };

        // vanilla: game.js:77
        function populateRegister(reg, spatial) {
            _.extend(reg, {
                creeps: {},
                structures: {},
                ownedStructures: {},
                spawns: {},
                sources: {},
                energy: {},
                flags: {},
                constructionSites: {},
                minerals: {},
                deposits: {},
                tombstones: {},
                nukes: {},
                powerCreeps: {},
                ruins: {},
                customObjects: {}
            });

            if(runtimeData.customObjectPrototypes) {
                runtimeData.customObjectPrototypes.forEach(i => {
                    if(i.opts.lookConstant) {
                        reg[i.opts.lookConstant] = {};
                    }
                });
            }

            if(spatial) {
                var keys = Object.keys(reg);
                reg.spatial = {};
                keys.forEach((i) => {
                    // chitin: vanilla `new Array(2500)`. The arrays are only read and written at x*50+y
                    // (addObjectToRegister, lookAt), never iterated or measured, so an empty array answers the
                    // same without a 20 KB backing store per type, per room, per tick.
                    reg.spatial[i] = [];
                });
            }
        }

        var deprecatedShown = [];

        register.deprecated = (msg) => {
            if (!_.contains(deprecatedShown, msg)) {
                deprecatedShown.push(msg);
                globals.console.log(msg);
            }
        };

        register.assertTargetObject = (obj) => {
            if(obj && _.isPlainObject(obj) && _.isString(obj.id) && obj.id.length == 24) {
                throw new Error("It seems you're trying to use a serialized game object stored in Memory which is not allowed. Please use `Game.getObjectById` to retrieve a live object reference instead.");
            }
        };

        populateRegister(register);

        var gclLevel = Math.floor(Math.pow((runtimeData.user.gcl || 0) / C.GCL_MULTIPLY, 1 / C.GCL_POW)) + 1,
        gclBaseProgress = Math.pow(gclLevel - 1, C.GCL_POW) * C.GCL_MULTIPLY;

        var gplLevel = Math.floor(Math.pow((runtimeData.user.power || 0) / C.POWER_LEVEL_MULTIPLY, 1 / C.POWER_LEVEL_POW)),
            gplBaseProgress = Math.pow(gplLevel, C.POWER_LEVEL_POW) * C.POWER_LEVEL_MULTIPLY;

        var game = {
            creeps: {},
            powerCreeps: {},
            spawns: {},
            structures: {},
            flags: {},
            constructionSites: {},
            rooms: {},
            time: runtimeData.time,
            cpuLimit: runtimeData.cpu,
            cpu: {
                getUsed(){
                    return getUsedCpu();
                },
                tickLimit: runtimeData.cpu,
                limit: runtimeData.user.cpu,
                bucket: runtimeData.cpuBucket,
                getHeapStatistics: getHeapStatistics ? function() {
                    return getHeapStatistics();
                } : undefined,
                halt: cpuHalt,
            },
            map: {},
            gcl: {
                level: gclLevel,
                progress: (runtimeData.user.gcl || 0) - gclBaseProgress,
                progressTotal: Math.pow(gclLevel, C.GCL_POW) * C.GCL_MULTIPLY - gclBaseProgress
            },
            gpl: {
                level: gplLevel,
                progress: (runtimeData.user.power || 0) - gplBaseProgress,
                progressTotal: Math.pow(gplLevel+1, 2) * 1000 - gplBaseProgress
            },
            market: {},
            resources: jsonParse(jsonStringify(runtimeData.user.resources||{})),
            getObjectById(id) {
                return register._objects[id] || null;
            },
            notify(message, groupInterval) {
                if (intents.push('notify', {message, groupInterval}, 20)) {
                    return C.OK;
                }
                else {
                    return C.ERR_FULL;
                }
            }
        };

        // chitin, match only (runner.setupList): runtimeData.objectList (prelude.js) holds the raw docs in roomObjects' key
        // order on a tick that qualifies, and the object list path runs. On any other tick, and always on the game server,
        // vanilla's statements run and nothing below reads the list. The two maps serve only some API calls, usually for
        // a room or two, so here the docs are grouped by room, before any constructor runs, and a room's entries are
        // built on the first read: the docs never change during a tick. The engine reads both by room name only, but a
        // getter on Object.prototype named after a room with no entry gets the table as `this`, and there it holds only the
        // rooms read so far. Vanilla builds them before player code runs, while a first read can come after it, so the build must
        // not consult anything a player can change by then: the table entries are defined rather than assigned, and a
        // room's table is filled with no prototype and given its usual one at the end. The list is dropped from
        // roomLists only once both tables hold the room, so a stack overflow in that build leaves the next read to build it.
        var objectList = setupList ? runtimeData.objectList : null;
        if (objectList) {
            var byRoomObjects = {}, byRoomKeys = {}, roomLists = objectCreate(null);
            for (var n = 0; n < objectList.length; n++) {
                var listed = objectList[n];
                if (listed.temp) {
                    continue;
                }
                var roomList = roomLists[listed.room];
                if (roomList === undefined) {
                    roomList = roomLists[listed.room] = [];
                }
                roomList[roomList.length] = listed;
            }
            var tableEntry = {__proto__: null, value: undefined, writable: true, enumerable: true, configurable: true};
            var setTableEntry = function(table, room, value) {
                tableEntry.value = value;
                objectDefine(table, room, tableEntry);
            };
            var roomEntries = {
                get(table, room) {
                    var list = roomLists[room];
                    if (list) {
                        var objects = objectCreate(null);
                        for (var n = 0; n < list.length; n++) {
                            objects[list[n]._id] = list[n];
                        }
                        objectSetPrototype(objects, objectPrototype);
                        setTableEntry(byRoomObjects, room, objects);
                        setTableEntry(byRoomKeys, room, objectKeys(objects));
                        roomLists[room] = undefined;
                    }
                    return table[room];
                }
            };
            register.objectsByRoom = new ProxyConstructor(byRoomObjects, roomEntries);
            register.objectsByRoomKeys = new ProxyConstructor(byRoomKeys, roomEntries);
            for (var i in runtimeData.rooms) {
                // the table populateRegister builds, in one literal: there are no custom object types on this path
                register.byRoom[i] = {
                    creeps: {}, structures: {}, ownedStructures: {}, spawns: {}, sources: {}, energy: {}, flags: {},
                    constructionSites: {}, minerals: {}, deposits: {}, tombstones: {}, nukes: {}, powerCreeps: {},
                    ruins: {}, customObjects: {},
                    spatial: {
                        creeps: [], structures: [], ownedStructures: [], spawns: [], sources: [], energy: [], flags: [],
                        constructionSites: [], minerals: [], deposits: [], tombstones: [], nukes: [], powerCreeps: [],
                        ruins: [], customObjects: []
                    }
                };
            }
        }
        else {
            register.objectsByRoom = {};
            register.objectsByRoomKeys = {};
            _.forEach(runtimeData.roomObjects, (i, key) => {
                if (i.temp) {
                    return;
                }
                var objects = register.objectsByRoom[i.room], keys = register.objectsByRoomKeys[i.room];
                if (!objects) {
                    objects = register.objectsByRoom[i.room] = {};
                }
                objects[key] = i;
                if (!keys) {
                    keys = register.objectsByRoomKeys[i.room] = [];
                }
                keys.push(key);
            });

            for (var i in runtimeData.rooms) {
                register.byRoom[i] = {};
                populateRegister(register.byRoom[i], true);
            }
        }

        require('./rooms').make(runtimeData, intents, register, globals);
        require('./rooms').makePos(register, globals);
        require('./creeps').make(runtimeData, intents, register, globals);
        require('./structures').make(runtimeData, intents, register, globals);
        require('./sources').make(runtimeData, intents, register, globals);
        require('./minerals').make(runtimeData, intents, register, globals);
        require('./deposits').make(runtimeData, intents, register, globals);
        require('./nukes').make(runtimeData, intents, register, globals);
        require('./resources').make(runtimeData, intents, register, globals);
        require('./flags').make(runtimeData, intents, register, globals);
        require('./tombstones').make(runtimeData, intents, register, globals);
        require('./construction-sites').make(runtimeData, intents, register, globals);
        require('./path-finder').make(runtimeData, intents, register, globals);
        require('./power-creeps').make(runtimeData, intents, register, globals);
        require('./ruins').make(runtimeData, intents, register, globals);
        require('./store').make(runtimeData, intents, register, globals);

        for (var i in runtimeData.rooms) {
            register.rooms[i] = new Room(i);
        }

        for(var i in customObjectsInfo) {
            customObjectsInfo[i].make(runtimeData, intents, register, globals);
        }


        game.rooms = _.clone(register.rooms);

        var structureTypes = {
            rampart: StructureRampart,
            road: StructureRoad,
            extension: StructureExtension,
            constructedWall: StructureWall,
            keeperLair: StructureKeeperLair,
            controller: StructureController,
            link: StructureLink,
            storage: StructureStorage,
            tower: StructureTower,
            observer: StructureObserver,
            powerBank: StructurePowerBank,
            powerSpawn: StructurePowerSpawn,
            lab: StructureLab,
            extractor: StructureExtractor,
            terminal: StructureTerminal,
            container: StructureContainer,
            spawn: StructureSpawn,
            nuker: StructureNuker,
            portal: StructurePortal,
            factory: StructureFactory,
            invaderCore: StructureInvaderCore
        };

        for(var i in runtimeData.userPowerCreeps) {
            register.powerCreeps[i] = new PowerCreep(i);
            game.powerCreeps[register.powerCreeps[i].name] = register.powerCreeps[i];
        }

        // chitin, match only: the object loop for the object list. A raw doc's fields are own data properties, read once;
        // `.id` reads, constructors and getters stay in vanilla's order; a structure's table write is not read back and
        // its chain of `type` tests ends early. What this leaves out could show through Object.prototype, clean when the
        // tick starts: an accessor on a game class can run in this loop and change that, so these skips test the name
        // they would have consulted. The ownedStructures spatial writes are simply gone (addToOwnedTable).
        function addRoomObject(i, object) {
            var obj, registered, type, room;

            if (object.temp) {
                return;
            }
            type = object.type;
            room = object.room;

            // chitin: vanilla also counts the objects per type into a local `c` nothing reads.

            if (type == 'creep') {
                obj = register._objects[i] = new Creep(i);
                addToRegister(register, 'creeps', obj, object, room);
                registered = register.creeps[i];
                if (runtimeData.userObjects[i]) {
                    if (game.creeps[registered.name]) {
                        registered.suicide();
                    }
                    else {
                        game.creeps[registered.name] = registered;
                    }
                }

                addToFindCache(register, C.FIND_CREEPS, registered, room);
                addToFindCache(register, C.FIND_MY_CREEPS, registered, room);
                addToFindCache(register, C.FIND_HOSTILE_CREEPS, registered, room);
            }
            if (type == 'powerCreep') {
                if(register.powerCreeps[i]) {
                    obj = register._objects[i] = register.powerCreeps[i];
                }
                else {
                    obj = register._objects[i] = new PowerCreep(i);
                }
                addToRegister(register, 'powerCreeps', obj, object, room);
                registered = register.powerCreeps[i];
                addToFindCache(register, C.FIND_POWER_CREEPS, registered, room);
                addToFindCache(register, C.FIND_MY_POWER_CREEPS, registered, room);
                addToFindCache(register, C.FIND_HOSTILE_POWER_CREEPS, registered, room);
            }
            var structureType = structureTypes[type];
            if (structureType) {
                obj = register._objects[i] = type == 'road' ? new structureType(i) : type == 'rampart' ? new structureType(i) : new structureTypes[type](i);
                var inStructures = addStructureToRegister(register, obj, object, room) === i;

                if (obj instanceof OwnedStructure) {
                    if (runtimeData.userObjects[i]) {
                        registered = inStructures ? obj : register.structures[i];
                        game.structures[registered.id] = registered;
                    }
                    addToOwnedTable(register.ownedStructures, obj.id, obj);
                    addToOwnedTable(register.byRoom[room].ownedStructures, obj.id, obj);
                }

                registered = inStructures ? obj : register.structures[i];
                addToFindCache(register, C.FIND_STRUCTURES, registered, room);
                addToFindCache(register, C.FIND_MY_STRUCTURES, registered, room);
                addToFindCache(register, C.FIND_HOSTILE_STRUCTURES, registered, room);

                if(type == 'spawn') {
                    addToRegister(register, 'spawns', obj, object, room);
                    registered = register.spawns[i];

                    if (runtimeData.userObjects[i]) {
                        game.spawns[registered.name] = registered;
                    }

                    addToFindCache(register, C.FIND_MY_SPAWNS, registered, room);
                    addToFindCache(register, C.FIND_HOSTILE_SPAWNS, registered, room);
                }

            }
            // chitin: `off` is on spawns and extensions; elsewhere the read shows only to an accessor on Object.prototype
            var energyType = type == 'extension' || type == 'spawn';
            if ((energyType || 'off' in objectPrototype) && !object.off && energyType && (object.user == runtimeData.user._id)) {
                registered = register.rooms[room];
                registered.energyAvailable += object.store.energy;
                registered.energyCapacityAvailable += object.storeCapacityResource.energy;
            }
            // chitin: none of these is a structure's type, and customObjectsInfo is empty but for Object.prototype
            if (!structureType || type in objectPrototype) {
                if (type == 'source') {
                    obj = register._objects[i] = new Source(i);
                    addToRegister(register, 'sources', obj, object, room);
                    registered = register.sources[i];
                    addToFindCache(register, C.FIND_SOURCES, registered, room);
                    addToFindCache(register, C.FIND_SOURCES_ACTIVE, registered, room);
                }
                if (type == 'mineral') {
                    obj = register._objects[i] = new Mineral(i);
                    addToRegister(register, 'minerals', obj, object, room);
                    addToFindCache(register, C.FIND_MINERALS, register.minerals[i], room);
                }
                if(type == 'deposit') {
                    obj = register._objects[i] = new Deposit(i);
                    addToRegister(register, 'deposits', obj, object, room)
                    addToFindCache(register, C.FIND_DEPOSITS, register.deposits[i], room)
                }
                if (type == 'energy') {
                    obj = register._objects[i] = new Energy(i);
                    addToRegister(register, 'energy', obj, object, room);
                    addToFindCache(register, C.FIND_DROPPED_RESOURCES, register.energy[i], room);
                }
                if (type == 'nuke') {
                    obj = register._objects[i] = new Nuke(i);
                    addToRegister(register, 'nukes', obj, object, room);
                    addToFindCache(register, C.FIND_NUKES, obj, room);
                }
                if (type == 'tombstone') {
                    obj = register._objects[i] = new Tombstone(i);
                    addToRegister(register, 'tombstones', obj, object, room);
                    addToFindCache(register, C.FIND_TOMBSTONES, obj, room);
                }
                if(type == 'ruin') {
                    obj = register._objects[i] = new Ruin(i);
                    addToRegister(register, 'ruins', obj, object, room);
                    addToFindCache(register, C.FIND_RUINS, obj, room)
                }

                if (type == 'constructionSite') {
                    obj = register._objects[i] = new ConstructionSite(i);
                    if (runtimeData.userObjects[i]) {
                        game.constructionSites[obj.id] = obj;
                        if (!register.byRoom[runtimeData.userObjects[i].room]) {
                            register.byRoom[runtimeData.userObjects[i].room] = {};
                            populateRegister(register.byRoom[runtimeData.userObjects[i].room], true);
                        }
                    }

                    addToRegister(register, 'constructionSites', obj, object, room);
                    if(runtimeData.rooms[object.room]) {
                        registered = register.constructionSites[i];
                        addToFindCache(register, C.FIND_CONSTRUCTION_SITES, registered, room);
                        addToFindCache(register, C.FIND_MY_CONSTRUCTION_SITES, registered, room);
                        addToFindCache(register, C.FIND_HOSTILE_CONSTRUCTION_SITES, registered, room);
                    }
                }

                if(customObjectsInfo[type]) {
                    obj = register._objects[i] = new globals[customObjectsInfo[type].name](i);
                    addToRegister(register, 'customObjects', obj, object, room);
                    if(customObjectsInfo[type].findConstant) {
                        addToFindCache(register, customObjectsInfo[type].findConstant, obj, room);
                    }
                    if(customObjectsInfo[type].lookConstant) {
                        addToRegister(register, customObjectsInfo[type].lookConstant, obj, object, room);
                    }
                }
            }
        }


        if (objectList) {
            for (var n = 0; n < objectList.length; n++) {
                addRoomObject(objectList[n]._id, objectList[n]);
            }
        }
        else {
            for(var i in runtimeData.roomObjects) {
                var object = runtimeData.roomObjects[i], obj, registered;

                if (object.temp) {
                    continue;
                }

                // chitin: vanilla also counts the objects per type into a local `c` nothing reads.

                if (object.type == 'creep') {
                    obj = register._objects[i] = new Creep(i);
                    addObjectToRegister(register, 'creeps', obj, object);
                    registered = register.creeps[i];
                    if (runtimeData.userObjects[i]) {
                        if (game.creeps[registered.name]) {
                            registered.suicide();
                        }
                        else {
                            game.creeps[registered.name] = registered;
                        }
                    }

                    addObjectToFindCache(register, C.FIND_CREEPS, registered, object);
                    addObjectToFindCache(register, C.FIND_MY_CREEPS, registered, object);
                    addObjectToFindCache(register, C.FIND_HOSTILE_CREEPS, registered, object);
                }
                if (object.type == 'powerCreep') {
                    if(register.powerCreeps[i]) {
                        obj = register._objects[i] = register.powerCreeps[i];
                    }
                    else {
                        obj = register._objects[i] = new PowerCreep(i);
                    }
                    addObjectToRegister(register, 'powerCreeps', obj, object);
                    registered = register.powerCreeps[i];
                    addObjectToFindCache(register, C.FIND_POWER_CREEPS, registered, object);
                    addObjectToFindCache(register, C.FIND_MY_POWER_CREEPS, registered, object);
                    addObjectToFindCache(register, C.FIND_HOSTILE_POWER_CREEPS, registered, object);
                }
                if (structureTypes[object.type]) {
                    obj = register._objects[i] = new structureTypes[object.type](i);
                    addObjectToRegister(register, 'structures', obj, object);

                    if (obj instanceof OwnedStructure) {
                        if (runtimeData.userObjects[i]) {
                            registered = register.structures[i];
                            game.structures[registered.id] = registered;
                        }
                        addObjectToRegister(register, 'ownedStructures', obj, object);
                    }

                    registered = register.structures[i];
                    addObjectToFindCache(register, C.FIND_STRUCTURES, registered, object);
                    addObjectToFindCache(register, C.FIND_MY_STRUCTURES, registered, object);
                    addObjectToFindCache(register, C.FIND_HOSTILE_STRUCTURES, registered, object);

                    if(object.type == 'spawn') {
                        addObjectToRegister(register, 'spawns', obj, object);
                        registered = register.spawns[i];

                        if (runtimeData.userObjects[i]) {
                            game.spawns[registered.name] = registered;
                        }

                        addObjectToFindCache(register, C.FIND_MY_SPAWNS, registered, object);
                        addObjectToFindCache(register, C.FIND_HOSTILE_SPAWNS, registered, object);
                    }

                }
                if (!object.off && (object.type == 'extension' || object.type == 'spawn') && (object.user == runtimeData.user._id)) {
                    registered = register.rooms[object.room];
                    registered.energyAvailable += object.store.energy;
                    registered.energyCapacityAvailable += object.storeCapacityResource.energy;
                }
                if (object.type == 'source') {
                    obj = register._objects[i] = new Source(i);
                    addObjectToRegister(register, 'sources', obj, object);
                    registered = register.sources[i];
                    addObjectToFindCache(register, C.FIND_SOURCES, registered, object);
                    addObjectToFindCache(register, C.FIND_SOURCES_ACTIVE, registered, object);
                }
                if (object.type == 'mineral') {
                    obj = register._objects[i] = new Mineral(i);
                    addObjectToRegister(register, 'minerals', obj, object);
                    addObjectToFindCache(register, C.FIND_MINERALS, register.minerals[i], object);
                }
                if(object.type == 'deposit') {
                    obj = register._objects[i] = new Deposit(i);
                    addObjectToRegister(register, 'deposits', obj, object)
                    addObjectToFindCache(register, C.FIND_DEPOSITS, register.deposits[i], object)
                }
                if (object.type == 'energy') {
                    obj = register._objects[i] = new Energy(i);
                    addObjectToRegister(register, 'energy', obj, object);
                    addObjectToFindCache(register, C.FIND_DROPPED_RESOURCES, register.energy[i], object);
                }
                if (object.type == 'nuke') {
                    obj = register._objects[i] = new Nuke(i);
                    addObjectToRegister(register, 'nukes', obj, object);
                    addObjectToFindCache(register, C.FIND_NUKES, obj, object);
                }
                if (object.type == 'tombstone') {
                    obj = register._objects[i] = new Tombstone(i);
                    addObjectToRegister(register, 'tombstones', obj, object);
                    addObjectToFindCache(register, C.FIND_TOMBSTONES, obj, object);
                }
                if(object.type == 'ruin') {
                    obj = register._objects[i] = new Ruin(i);
                    addObjectToRegister(register, 'ruins', obj, object);
                    addObjectToFindCache(register, C.FIND_RUINS, obj, object)
                }

                if (object.type == 'constructionSite') {
                    obj = register._objects[i] = new ConstructionSite(i);
                    if (runtimeData.userObjects[i]) {
                        game.constructionSites[obj.id] = obj;
                        if (!register.byRoom[runtimeData.userObjects[i].room]) {
                            register.byRoom[runtimeData.userObjects[i].room] = {};
                            populateRegister(register.byRoom[runtimeData.userObjects[i].room], true);
                        }
                    }

                    addObjectToRegister(register, 'constructionSites', obj, object);
                    if(runtimeData.rooms[object.room]) {
                        registered = register.constructionSites[i];
                        addObjectToFindCache(register, C.FIND_CONSTRUCTION_SITES, registered, object);
                        addObjectToFindCache(register, C.FIND_MY_CONSTRUCTION_SITES, registered, object);
                        addObjectToFindCache(register, C.FIND_HOSTILE_CONSTRUCTION_SITES, registered, object);
                    }
                }

                if(customObjectsInfo[object.type]) {
                    obj = register._objects[i] = new globals[customObjectsInfo[object.type].name](i);
                    addObjectToRegister(register, 'customObjects', obj, object);
                    if(customObjectsInfo[object.type].findConstant) {
                        addObjectToFindCache(register, customObjectsInfo[object.type].findConstant, obj, object);
                    }
                    if(customObjectsInfo[object.type].lookConstant) {
                        addObjectToRegister(register, customObjectsInfo[object.type].lookConstant, obj, object);
                    }
                }

            }
        }

        runtimeData.flags.forEach(flagRoomData => {

            var data = flagRoomData.data.split("|");
            data.forEach(flagData => {
                if(!flagData) {
                    return;
                }
                var info = flagData.split("~");
                info[0] = info[0].replace(/\$VLINE\$/g,"|").replace(/\$TILDE\$/g,"~");
                var id = 'flag_'+info[0];
                var flag = register._objects[id] = new Flag(info[0], info[1], info[2], flagRoomData.room, info[3], info[4]);

                register.flags[id] = flag;
                if(register.byRoom[flagRoomData.room]) {
                    register.byRoom[flagRoomData.room].flags[id] = flag;
                    let index = (+info[3]) * 50 + (+info[4]);
                    let spatial = register.byRoom[flagRoomData.room].spatial.flags;
                    // chitin (J3): queued like addObjectToRegister's spatial write.
                    rooms.queueSpatial(flagRoomData.room, spatial, index, flag);
                }

                game.flags[info[0]] = flag;

                if(!findCacheFn[C.FIND_FLAGS] || findCacheFn[C.FIND_FLAGS](flag)) {
                    register.findCache[C.FIND_FLAGS] = register.findCache[C.FIND_FLAGS] || {};
                    register.findCache[C.FIND_FLAGS][flagRoomData.room] = register.findCache[C.FIND_FLAGS][flagRoomData.room] || [];
                    register.findCache[C.FIND_FLAGS][flagRoomData.room].push(flag);
                }
            })
        });

        game.map = register.map = map.makeMap(runtimeData, register, globals);

        game.market = register.market = market.make(runtimeData, intents, register);

        // chitin: vanilla `_.extend(globals, jsonParse(jsonStringify(C)))`: every key of the parsed copy, in order, is
        // assigned (lodash 3 assign: `object[key] = source[key]`) a fresh plain copy. C is never written after
        // startup, so its JSON is the same every tick: compiled once per VM into one assignment per key, each of an
        // object literal, which V8 copies from its boilerplate instead of parsing the text and looping in lodash.
        // Same keys, order, values and [[Set]]s; a "__proto__" key is the one place a literal differs from JSON.parse.
        if (!copyConstants) {
            var constantsJson = jsonStringify(C), parsed = jsonParse(constantsJson);
            copyConstants = constantsJson.indexOf('"__proto__"') === -1 ?
                driver.natives.compileAndRun('(function (g) {\n' + Object.keys(parsed).map(function (k) {
                    return 'g[' + jsonStringify(k) + '] = ' + jsonStringify(parsed[k]) + ';\n';
                }).join('') + '})', 'constants', undefined, 'sandbox') :
                function (g) { _.extend(g, jsonParse(constantsJson)); };
        }
        copyConstants(globals);

        return game;
    };

    (function() {

        var runCodeCache = objectCreate(null);

        // vanilla: game.js:444
        exports.init = function (
            _globals, _codeModules, _runtimeData,
            _intents, _memory,
            _fakeConsole, _consoleCommands,
            _timeout, _getUsedCpu,
            _scriptCachedData, _sandboxedFunctionWrapper,
            _getHeapStatistics, _cpuHalt
        ) {

            var userId = _runtimeData.user._id;

            runCodeCache[userId] = runCodeCache[userId] || objectCreate(null);
            runCodeCache[userId].globals = _globals;
            runCodeCache[userId].codeModules = _codeModules;
            runCodeCache[userId].runtimeData = _runtimeData;
            runCodeCache[userId].intents = _intents;
            runCodeCache[userId].memory = _memory;
            runCodeCache[userId].fakeConsole = _fakeConsole;
            runCodeCache[userId].consoleCommands = _consoleCommands;
            runCodeCache[userId].timeout = _timeout;
            runCodeCache[userId].getUsedCpu = _getUsedCpu;
            runCodeCache[userId].scriptCachedData = _scriptCachedData;
            runCodeCache[userId].getHeapStatistics = _getHeapStatistics;
            runCodeCache[userId].cpuHalt = _cpuHalt;
            runCodeCache[userId].sandboxedFunctionWrapper = _sandboxedFunctionWrapper;

            _.extend(runCodeCache[userId].globals, {
                RawMemory: runCodeCache[userId].memory,
                console: runCodeCache[userId].fakeConsole
            });

            if (!runCodeCache[userId].globals._) {
                runCodeCache[userId].globals._ = _.runInContext();
            }

            Object.defineProperty(runCodeCache[userId].globals, 'Memory', {
                configurable: true,
                enumerable: true,
                get() {

                    try {
                        runCodeCache[userId].memory._parsed = JSON.parse(runCodeCache[userId].memory.get() || "{}");
                        runCodeCache[userId].memory._parsed.__proto__ = null;
                    }
                    catch (e) {
                        runCodeCache[userId].memory._parsed = null;
                    }

                    Object.defineProperty(runCodeCache[userId].globals, 'Memory', {
                        configurable: true,
                        enumerable: true,
                        value: runCodeCache[userId].memory._parsed
                    });

                    return runCodeCache[userId].memory._parsed;
                }
            });

            runCodeCache[userId].globals.Game = makeGameObject(runCodeCache[userId]);

            if (!driver.natives.globalRequire() ||
                runCodeCache[userId].runtimeData.userCodeTimestamp != driver.natives.globalRequire().timestamp ||
                !_.isObject(driver.natives.globalRequire().cache.main) || !_.isFunction(
                    driver.natives.globalRequire().cache.main.loop)) {

                runCodeCache[userId].globals.require = bindFunction(requireFn, runCodeCache[userId]);
                runCodeCache[userId].globals.require.cache = {lodash: runCodeCache[userId].globals._};
                runCodeCache[userId].globals.require.timestamp = runCodeCache[userId].runtimeData.userCodeTimestamp;
            }

            return runCodeCache[userId];
        };

        // vanilla: game.js:517
        exports.run = function(userId) {

            var mainExports = runCodeCache[userId].globals.require('main');
            if(_.isObject(mainExports) && _.isFunction(mainExports.loop)) {

                if(driver.natives.globalRequire().initGlobals) {
                    _.forEach(driver.natives.globalRequire().initGlobals, (i) => i());
                }

                driver.evalCode({
                    exports: mainExports,
                    user: runCodeCache[userId].runtimeData.user._id,
                    timestamp: runCodeCache[userId].runtimeData.userCodeTimestamp,
                    name: '__mainLoop',
                    code: 'module.exports.loop();'
                }, runCodeCache[userId].globals, false, runCodeCache[userId].timeout);
            }

            if (runCodeCache[userId].consoleCommands) {
                for (var i = 0; i < runCodeCache[userId].consoleCommands.length; i++) {
                    var result = driver.evalCode({
                        exports: {},
                        user: runCodeCache[userId].runtimeData.user._id,
                        name: '_console' + new Date().getTime() + '_' + i,
                        code: runCodeCache[userId].consoleCommands[i].expression
                    }, runCodeCache[userId].globals, true);
                    if (!runCodeCache[userId].consoleCommands[i].hidden) {
                        runCodeCache[userId].fakeConsole.commandResult(result);
                    }
                }
            }
        };
    })();

    // vanilla: game.js:551
    function requireFn(moduleName){

        moduleName = moduleName.replace(/^\.\//,'');

        if (!(moduleName in driver.natives.globalRequire().cache)) {

            if (_.isUndefined(this.codeModules[moduleName])) {
                throw new Error(`Unknown module '${moduleName}'`);
            }

            if(_.isObject(this.codeModules[moduleName]) && this.codeModules[moduleName].binary !== undefined) {
                driver.natives.globalRequire().cache[moduleName] = driver.bufferFromBase64(this.codeModules[moduleName].binary);
            }
            else {
                driver.natives.globalRequire().cache[moduleName] = -1;

                var moduleObject = {
                    exports: {},
                    user: this.runtimeData.user._id,
                    timestamp: this.runtimeData.userCodeTimestamp,
                    name: moduleName,
                    code: this.codeModules[moduleName]
                };

                try {
                    driver.evalCode(moduleObject, this.globals, false, this.timeout, this.scriptCachedData);
                }
                catch (e) {
                    delete driver.natives.globalRequire().cache[moduleName];
                    throw e;
                }

                driver.natives.globalRequire().cache[moduleName] = moduleObject.exports;
                if (moduleObject.__initGlobals) {
                    driver.natives.globalRequire().initGlobals = driver.natives.globalRequire().initGlobals || {};
                    driver.natives.globalRequire().initGlobals[moduleName] = moduleObject.__initGlobals;
                }
            }
        }
        else if (driver.natives.globalRequire().cache[moduleName] === -1) {
            throw new Error(`Circular reference to module '${moduleName}'`)
        }
        return driver.natives.globalRequire().cache[moduleName];
    }
})();
