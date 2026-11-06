// mirror: reference/engine/src/game/rooms.js
// parity: DIVERGENT(LEGACY-PF)
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
// LEGACY-PF: the PathFinder.use(false) paths (findPath legacy, findClosestByPath legacy, getEndNodes grids) need
//   @screeps/pathfinding, which the chitin bundle does not vendor yet; they are ported statement by statement and
//   load the library lazily, so they work wherever require('@screeps/pathfinding') resolves (the node oracle).
// Deviations (ARCHITECTURE.md §7.2): J1 staticTerrainData is the host's per-VM natives.terrain cache; J2 Uint8Array
//   and its set captured at bundle evaluation; J3 spatial register writes replayed on first read (queueSpatial);
//   J10 RoomPosition keeps roomNameToXY per room name (roomXYOf).
//   Sloppy mode like vanilla; Room.Terrain keeps its own "use strict".
var _ = require('lodash'),
    utils = require('./../utils'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants,
    pathfinding;   // vanilla: rooms.js:5 - required on first legacy use (LEGACY-PF)

function legacyPathfinding() {
    return pathfinding || (pathfinding = require('@screeps/pathfinding'));
}

var abs = Math.abs, min = Math.min, max = Math.max;

// chitin (J10): RoomPosition's utils.roomNameToXY(roomName). While the builtins it calls (String.prototype.substr
// and charAt, the global parseInt) are these originals, captured at bundle evaluation, its [x, y] is a pure function
// of the room-name string, so it is kept per name (up to 4096 names, never mutated): no substr strings and no array
// per position. Otherwise, and for anything but a string, vanilla's call runs.
var roomXY = {__proto__: null}, roomXYSize = 0, stringPrototype = String.prototype,
    originalSubstr = stringPrototype.substr, originalCharAt = stringPrototype.charAt, originalParseInt = parseInt;

function roomXYOf(roomName) {
    if(typeof roomName !== 'string' || stringPrototype.substr !== originalSubstr ||
        stringPrototype.charAt !== originalCharAt || typeof parseInt !== 'function' || parseInt !== originalParseInt) {
        return utils.roomNameToXY(roomName);
    }
    var xy = roomXY[roomName];
    if(xy === undefined) {
        xy = utils.roomNameToXY(roomName);
        if(roomXYSize < 4096) {
            roomXY[roomName] = xy;
            roomXYSize++;
        }
    }
    return xy;
}

var runtimeData, intents, register, globals;

var positionsSetCacheCounter, createdFlagNames, createdSpawnNames, privateStore, createdConstructionSites;

// chitin (J3): game.js's spatial register writes (addObjectToRegister, flags) run vanilla's statements on a Map
// beside the array: the per-index arrays are made and pushed to when vanilla does it (a patched push is called the
// same), and only their stores into the spatial array wait for _lookSpatialRegister, its only reader (the arrays
// never escape), to read it. An array written at x*50+y >= 1024 holds dictionary elements, which V8 adds through
// the runtime (~300 ns a store); arrays nobody reads skip that, among them every ownedStructures and spawns array,
// which no look type maps to. spatialQueues[roomName] = [spatial, lists, ...]: lists is the Map (index -> per-index
// array) until the array is stored, then null and later writes go to the array itself, as does any write whose
// index is not a number (a Map key is not converted to a property key). Null prototype: no inherited room names;
// arrays are matched by identity, so the reader touches nothing a user can reach.
var spatialQueues;

// Captured at bundle evaluation (J5): own get/set/forEach, so a user patch of Map.prototype is not called.
class SpatialLists extends Map {
    constructor() {
        super();
    }
}
SpatialLists.prototype.get = Map.prototype.get;
SpatialLists.prototype.set = Map.prototype.set;
SpatialLists.prototype.forEach = Map.prototype.forEach;

exports.queueSpatial = function(roomName, spatial, index, objectInstance) {
    var queues = spatialQueues[roomName], lists;
    if(queues === undefined) {
        queues = spatialQueues[roomName] = [];
    }
    for(var k = 0; k < queues.length; k += 2) {
        if(queues[k] === spatial) {
            lists = queues[k + 1];
            break;
        }
    }
    if(lists === undefined) {
        lists = new SpatialLists();
        queues[k] = spatial;
        queues[k + 1] = lists;
    }
    if(lists !== null && typeof index !== 'number') {
        storeSpatialLists(spatial, lists);
        queues[k + 1] = lists = null;
    }
    if(lists === null) {
        // vanilla: game.js:43-47 (addObjectToRegister), game.js:414-418 (flags)
        if (spatial[index] === undefined) {
            spatial[index] = [ objectInstance ];
        } else {
            spatial[index].push(objectInstance);
        }
        return;
    }
    // the same, with the array's slot in the Map
    var list = lists.get(index);
    if (list === undefined) {
        lists.set(index, [ objectInstance ]);
    } else {
        list.push(objectInstance);
    }
};

// Presized to the highest index, so the stores stay fast elements (V8 sizes an empty array's backing store to
// exactly the length set). The length itself is never read.
function storeSpatialLists(spatial, lists) {
    var top = -1;
    lists.forEach(function(list, index) {
        if((index | 0) === index && index > top && index < 2500) {
            top = index;
        }
    });
    if(top >= spatial.length) {
        spatial.length = top + 1;
    }
    lists.forEach(function(list, index) {
        spatial[index] = list;
    });
}

function flushSpatialQueue(queues, spatial) {
    for(var k = 0; k < queues.length; k += 2) {
        if(queues[k] === spatial) {
            if(queues[k + 1] !== null) {
                storeSpatialLists(spatial, queues[k + 1]);
                queues[k + 1] = null;
            }
            return;
        }
    }
}

// A name that is not a string (a hand-made `this`) is not converted here: every room's queues are searched.
function flushSpatial(roomName, spatial) {
    if(typeof roomName === 'string') {
        var queues = spatialQueues[roomName];
        if(queues !== undefined) {
            flushSpatialQueue(queues, spatial);
        }
        return;
    }
    for(var queuedRoom in spatialQueues) {
        flushSpatialQueue(spatialQueues[queuedRoom], spatial);
    }
}

// vanilla: rooms.js:13-14, 389-399 - J2: vanilla takes the constructor of the first staticTerrainData entry at the
// first make of the VM (before any user code), i.e. the isolate's own Uint8Array; the J1 cache may still be empty
// then, so the same pristine Uint8Array and %TypedArray%.prototype.set are captured at bundle evaluation instead.
let TerrainConstructor    = Uint8Array;
let TerrainConstructorSet = Uint8Array.prototype.set;

// vanilla: rooms.js:16
function getPathfinder(id, opts) {
    opts = opts || {};
    _.defaults(opts, {maxOps: 2000, heuristicWeight: 1});
    var key = `${opts.maxOps},${opts.heuristicWeight}`;

    if(!privateStore[id].pfFinders[key]) {
        privateStore[id].pfFinders[key] = new (legacyPathfinding().AStarFinder)({
            diagonalMovement: 1,
            maxOpsLimit: opts.maxOps,
            heuristic: legacyPathfinding().Heuristic.chebyshev,
            weight: opts.heuristicWeight
        });
    }
    return privateStore[id].pfFinders[key];
}

// vanilla: rooms.js:32
function makePathfindingGrid(id, opts, endNodesKey) {

    opts = opts || {};

    var rows = new Array(50),
    obstacleTypes = _.clone(C.OBSTACLE_OBJECT_TYPES);

    if(opts.ignoreDestructibleStructures) {
        obstacleTypes = _.without(obstacleTypes, 'constructedWall','spawn','extension', 'link','storage','observer','tower','powerBank','powerSpawn','lab','terminal');
    }
    if(opts.ignoreCreeps) {
        obstacleTypes = _.without(obstacleTypes, 'creep', 'powerCreep');
    }

    for(var y=0; y<50; y++) {
        rows[y] = new Array(50);
        for(var x=0; x<50; x++) {
            rows[y][x] = x == 0 || y == 0 || x == 49 || y == 49 ? 11 : 2;
            var terrainCode = runtimeData.staticTerrainData[id][y*50+x];
            if(terrainCode & C.TERRAIN_MASK_WALL) {
                rows[y][x] = 0;
            }
            if ((terrainCode & C.TERRAIN_MASK_SWAMP) && rows[y][x] == 2) {
                rows[y][x] = 10;
            }
        }
    }

    register.objectsByRoomKeys[id].forEach((key) => {
        var object = register.objectsByRoom[id][key];

        if (_.contains(obstacleTypes, object.type) ||
        !opts.ignoreDestructibleStructures && object.type == 'rampart' && !object.isPublic && object.user != runtimeData.user._id ||
        !opts.ignoreDestructibleStructures && object.type == 'constructionSite' && object.user == runtimeData.user._id && _.contains(C.OBSTACLE_OBJECT_TYPES, object.structureType)) {

            rows[object.y][object.x] = 0;
        }

        if (object.type == 'road' && rows[object.y][object.x] > 0) {
            rows[object.y][object.x] = 1;
        }
    });


    if(opts.ignore) {
        if(!_.isArray(opts.ignore)) {
            throw new Error('option `ignore` is not an array');
        }
        _.forEach(opts.ignore, (i, key) => {
            if(!i) {
                return;
            }
            if(i.pos) {
                rows[i.pos.y][i.pos.x] = rows[i.pos.y][i.pos.x] > 2 ? 2 : rows[i.pos.y][i.pos.x];
            }
            if(_.isObject(i) && !_.isUndefined(i.x) && !(i instanceof RoomPosition)) {
                opts.ignore[key] = new RoomPosition(i.x, i.y, id);
            }
            if(!_.isUndefined(i.x)) {
                rows[i.y][i.x] = rows[i.y][i.x] > 2 ? 2 : rows[i.y][i.x];
            }
        });
    }

    if(opts.avoid) {
        if(!_.isArray(opts.avoid)) {
            throw new Error('option `avoid` is not an array');
        }
        _.forEach(opts.avoid, (i, key) => {
            if(!i) {
                return;
            }
            if(i.pos) {
                rows[i.pos.y][i.pos.x] = 0;
            }
            if(_.isObject(i) && !_.isUndefined(i.x) && !(i instanceof RoomPosition)) {
                opts.avoid[key] = new RoomPosition(i.x, i.y, id);
            }
            if(!_.isUndefined(i.x)) {
                rows[i.y][i.x] = 0;
            }
        });
    }
    if(endNodesKey) {
        _.forEach(privateStore[id].pfEndNodes[endNodesKey], (i) => {
            if(!_.isUndefined(i.x)) {
                rows[i.y][i.x] = 999;
            }
            else if(!_.isUndefined(i.pos)) {
                rows[i.pos.y][i.pos.x] = 999;
            }
        });
    }

    return new (legacyPathfinding().Grid)(50, 50, rows);
}

// vanilla: rooms.js:130
function getPathfindingGrid(id, opts, endNodesKey) {

    var gridName = 'grid';

    opts = opts || {};

    if(opts.ignoreCreeps) {
        gridName += '_ignoreCreeps'
    }
    if(opts.ignoreDestructibleStructures) {
        gridName += '_ignoreDestructibleStructures'
    }
    if(_.isNumber(endNodesKey)) {
        gridName += '_endNodes'+endNodesKey;
    }
    if(opts.avoid) {
        gridName += '_avoid'+privateStore[id].positionsSetCache.key(opts.avoid);
    }
    if(opts.ignore) {
        gridName += '_ignore'+privateStore[id].positionsSetCache.key(opts.ignore);
    }

    if(!privateStore[id].pfGrid[gridName]) privateStore[id].pfGrid[gridName] = makePathfindingGrid(id, opts, endNodesKey);

    return privateStore[id].pfGrid[gridName].clone();
}

// vanilla: rooms.js:157
function makePathfindingGrid2(id, opts) {

    opts = opts || {};

    var costs = new PathFinder.CostMatrix();

    var obstacleTypes = _.clone(C.OBSTACLE_OBJECT_TYPES);
    obstacleTypes.push('portal');

    if(opts.ignoreDestructibleStructures) {
        obstacleTypes = _.without(obstacleTypes, 'constructedWall','spawn','extension', 'link','storage','observer','tower','powerBank','powerSpawn','lab','terminal');
    }
    if(opts.ignoreCreeps || register.rooms[id].controller && register.rooms[id].controller.safeMode && register.rooms[id].controller.my) {
        obstacleTypes = _.without(obstacleTypes, 'creep', 'powerCreep');
    }

    if(register.objectsByRoomKeys[id]) {
        register.objectsByRoomKeys[id].forEach((key) => {
            var object = register.objectsByRoom[id][key];

            if (_.contains(obstacleTypes, object.type) ||
            !opts.ignoreCreeps && register.rooms[id].controller && register.rooms[id].controller.safeMode && register.rooms[id].controller.my && (object.type == 'creep' || object.type == 'powerCreep') && object.user == runtimeData.user._id ||
            !opts.ignoreDestructibleStructures && object.type == 'rampart' && !object.isPublic && object.user != runtimeData.user._id ||
            !opts.ignoreDestructibleStructures && object.type == 'constructionSite' && object.user == runtimeData.user._id && _.contains(C.OBSTACLE_OBJECT_TYPES, object.structureType)) {

                costs.set(object.x, object.y, 0xFF);
            }

            if (object.type == 'swamp' && costs.get(object.x, object.y) == 0) {
                costs.set(object.x, object.y, opts.ignoreRoads ? 5 : 10);
            }

            if (!opts.ignoreRoads && object.type == 'road' && costs.get(object.x, object.y) < 0xFF) {
                costs.set(object.x, object.y, 1);
            }
        });
    }

    return costs;
}

// vanilla: rooms.js:198
function getPathfindingGrid2(id, opts) {

    if(!privateStore[id]) {
        return new PathFinder.CostMatrix();
    }

    var gridName = 'grid2';

    opts = opts || {};

    if(opts.ignoreCreeps) {
        gridName += '_ignoreCreeps';
    }
    if(opts.ignoreDestructibleStructures) {
        gridName += '_ignoreDestructibleStructures';
    }
    if(opts.ignoreRoads) {
        gridName += '_ignoreRoads';
    }

    if(!privateStore[id].pfGrid[gridName]) privateStore[id].pfGrid[gridName] = makePathfindingGrid2(id, opts);

    return privateStore[id].pfGrid[gridName];
}

// vanilla: rooms.js:223
function _findPath2(id, fromPos, toPos, opts) {

    opts = opts || {};

    if(fromPos.isEqualTo(toPos)) {
        return opts.serialize ? '' : [];
    }

    if(opts.avoid) {
        register.deprecated('`avoid` option cannot be used when `PathFinder.use()` is enabled. Use `costCallback` instead.');
        opts.avoid = undefined;
    }
    if(opts.ignore) {
        register.deprecated('`ignore` option cannot be used when `PathFinder.use()` is enabled. Use `costCallback` instead.');
        opts.ignore = undefined;
    }
    if(opts.maxOps === undefined && (opts.maxRooms === undefined || opts.maxRooms > 1) && fromPos.roomName != toPos.roomName) {
        opts.maxOps = 20000;
    }
    var searchOpts = {
        roomCallback: function(roomName) {
            var costMatrix = getPathfindingGrid2(roomName, opts);
            if(typeof opts.costCallback == 'function') {
                costMatrix = costMatrix.clone();
                var resultMatrix = opts.costCallback(roomName, costMatrix);
                if(resultMatrix instanceof PathFinder.CostMatrix) {
                    costMatrix = resultMatrix;
                }
            }
            return costMatrix;
        },
        maxOps: opts.maxOps,
        maxRooms: opts.maxRooms
    };
    if(!opts.ignoreRoads) {
        searchOpts.plainCost = 2;
        searchOpts.swampCost = 10;
    }
    if(opts.plainCost) {
        searchOpts.plainCost = opts.plainCost;
    }
    if(opts.swampCost) {
        searchOpts.swampCost = opts.swampCost;
    }

    var ret = PathFinder.search(fromPos, {range: Math.max(1,opts.range || 0), pos: toPos}, searchOpts);

    if(!opts.range &&
            (ret.path.length && ret.path[ret.path.length-1].isNearTo(toPos) && !ret.path[ret.path.length-1].isEqualTo(toPos) ||
            !ret.path.length && fromPos.isNearTo(toPos))) {
        ret.path.push(toPos);
    }
    var curX = fromPos.x, curY = fromPos.y;

    var resultPath = [];

    for(let i=0; i<ret.path.length; i++) {
        let pos = ret.path[i];
        if(pos.roomName != id) {
            break;
        }
        let result = {
            x: pos.x,
            y: pos.y,
            dx: pos.x - curX,
            dy: pos.y - curY,
            direction: utils.getDirection(pos.x - curX, pos.y - curY)
        };

        curX = result.x;
        curY = result.y;
        resultPath.push(result);
    }

    if(opts.serialize) {
        return utils.serializePath(resultPath);
    }

    return resultPath;
}

// vanilla: rooms.js:304
function _findClosestByPath2(fromPos, objects, opts) {

    opts = opts || {};

    if(_.isNumber(objects)) {
        objects = register.rooms[fromPos.roomName].find(objects, {filter: opts.filter});
    }
    else if(opts.filter) {
        objects = _.filter(objects, opts.filter);
    }

    if(!objects.length) {
        return null;
    }

    var objectOnSquare = _.find(objects, obj => fromPos.isEqualTo(obj));
    if(objectOnSquare) {
        return objectOnSquare;
    }

    var goals = _.map(objects, i => {
        if(i.pos) {
            i = i.pos;
        }
        return {range: 1, pos: i};
    });

    if(opts.avoid) {
        register.deprecated('`avoid` option cannot be used when `PathFinder.use()` is enabled. Use `costCallback` instead.');
    }
    if(opts.ignore) {
        register.deprecated('`ignore` option cannot be used when `PathFinder.use()` is enabled. Use `costCallback` instead.');
    }
    var searchOpts = {
        roomCallback: function(roomName) {
            if(register.objectsByRoom[roomName]) {
                var costMatrix = getPathfindingGrid2(roomName, opts);
                if(typeof opts.costCallback == 'function') {
                    costMatrix = costMatrix.clone();
                    var resultMatrix = opts.costCallback(roomName, costMatrix);
                    if(resultMatrix instanceof PathFinder.CostMatrix) {
                        costMatrix = resultMatrix;
                    }
                }
                return costMatrix;
            }
        },
        maxOps: opts.maxOps,
        maxRooms: 1
    };
    if(!opts.ignoreRoads) {
        searchOpts.plainCost = 2;
        searchOpts.swampCost = 10;
    }
    var ret = PathFinder.search(fromPos, goals, searchOpts);

    var result = null;
    var lastPos = fromPos;

    if(ret.path.length) {
        lastPos = ret.path[ret.path.length-1];
    }

    objects.forEach(obj => {
        if(lastPos.isNearTo(obj)) {
            result = obj;
        }
    });

    return result;
}

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.Room)` in make(), where `Room` is make()'s own local;
// out here it is the global, read through its property cell.
function hasRoom() {
    return typeof Room !== 'undefined' && !!Room;
}

// vanilla: rooms.js:376
exports.make = function(_runtimeData, _intents, _register, _globals) {

    runtimeData = _runtimeData;
    intents = _intents;
    register = _register;
    globals = _globals;

    positionsSetCacheCounter = 1;
    createdFlagNames = [];
    createdSpawnNames = [];
    privateStore = {};
    createdConstructionSites = 0;
    spatialQueues = {__proto__: null};

    // vanilla: rooms.js:389-399 (TerrainConstructor discovery) - J2, see the declarations above

    if(hasRoom()) {
        return;
    }

    var data = (id) => {
        if(!runtimeData.rooms[id]) {
            throw new Error("Could not find a room with name "+id);
        }
        return runtimeData.rooms[id];
    };


    // vanilla: rooms.js:419
    var Room = register.wrapFn(function(id) {

        var gameInfo, gameId = id, match = id.match(/survival_(.*)$/);
        if(match) {
            gameId = match[1];
        }
        if(runtimeData.games && gameId in runtimeData.games) {
            gameInfo = runtimeData.games[gameId];
        }

        Object.defineProperty(this, 'name', {
            value: id,
            enumerable: true
        });

        this.energyAvailable = 0;
        this.energyCapacityAvailable = 0;

        this.survivalInfo = gameInfo;

        privateStore[id] = {
            pfGrid: {},
            pfFinders: {},
            pfEndNodes: {},
            pfDijkstraFinder: null,   // vanilla: rooms.js:443 - created on first legacy use (LEGACY-PF)
            pathCache: {},
            positionsSetCache: {

                cache: {},

                key(array) {

                    if (!_.isArray(array)) {
                        return 0;
                    }

                    var positionsArray = _.map(array, (i) => {
                        if (i && i.pos) {
                            return i.pos;
                        }
                        if(_.isObject(i) && !_.isUndefined(i.x) && !(i instanceof RoomPosition)) {
                            return new RoomPosition(i.x, i.y, id);
                        }
                        return i;
                    });

                    var key = _.findKey(this.cache, (objects) => {
                        return positionsArray.length == objects.length && _.every(positionsArray, (j) => _.any(objects, (object) => {
                            if(!_.isObject(j) || !j.isEqualTo) {
                                throw new Error('Invalid position '+j+', check your `opts` property');
                            }
                            return j.isEqualTo(object);
                        }));
                    });

                    if (key === undefined) {
                        key = positionsSetCacheCounter++;
                        this.cache[key] = _.clone(array);
                    }
                    else {
                        key = parseInt(key);
                    }

                    return key;
                }
            },
            lookTypeRegisters: {
                creep: register.byRoom[id].creeps,
                energy: register.byRoom[id].energy,
                resource: register.byRoom[id].energy,
                source: register.byRoom[id].sources,
                mineral: register.byRoom[id].minerals,
                deposit: register.byRoom[id].deposits,
                structure: register.byRoom[id].structures,
                flag: register.byRoom[id].flags,
                constructionSite: register.byRoom[id].constructionSites,
                tombstone: register.byRoom[id].tombstones,
                ruin: register.byRoom[id].ruins,
                nuke: register.byRoom[id].nukes,
                powerCreep: register.byRoom[id].powerCreeps
            },
            lookTypeSpatialRegisters: {
                creep: register.byRoom[id].spatial.creeps,
                energy: register.byRoom[id].spatial.energy,
                resource: register.byRoom[id].spatial.energy,
                source: register.byRoom[id].spatial.sources,
                mineral: register.byRoom[id].spatial.minerals,
                deposit: register.byRoom[id].spatial.deposits,
                structure: register.byRoom[id].spatial.structures,
                flag: register.byRoom[id].spatial.flags,
                constructionSite: register.byRoom[id].spatial.constructionSites,
                tombstone: register.byRoom[id].spatial.tombstones,
                ruin: register.byRoom[id].spatial.ruins,
                nuke: register.byRoom[id].spatial.nukes,
                powerCreep: register.byRoom[id].spatial.powerCreeps
            }
        };
        if(runtimeData.customObjectPrototypes) {
            runtimeData.customObjectPrototypes.forEach(i => {
                if(i.opts.lookConstant) {
                    privateStore[id].lookTypeRegisters[i.opts.lookConstant] = register.byRoom[id][i.opts.lookConstant];
                    privateStore[id].lookTypeSpatialRegisters[i.opts.lookConstant] = register.byRoom[id].spatial[i.opts.lookConstant];
                }
            });
        }

        this.visual = new RoomVisual(id);
    });

    // vanilla: rooms.js:528
    Room.serializePath = register.wrapFn(function(path) {
        return utils.serializePath(path);
    });

    // vanilla: rooms.js:532
    Room.deserializePath = register.wrapFn(function(str) {
        return utils.deserializePath(str);
    });

    // vanilla: rooms.js:536
    Room.prototype.toString = register.wrapFn(function() {
        return `[room ${this.name}]`;
    });

    // vanilla: rooms.js:540
    Room.prototype.toJSON = register.wrapFn(function() {
        var result = {};
        for(var i in this) {
            if(i[0] == '_' || _.contains(['toJSON','toString','controller','storage','terminal'],i)) {
                continue;
            }
            result[i] = this[i];
        }
        return result;
    });

    // vanilla: rooms.js:551
    Object.defineProperty(Room.prototype, 'memory', {
        get: function() {
            if(_.isUndefined(globals.Memory.rooms) || globals.Memory.rooms === 'undefined') {
                globals.Memory.rooms = {};
            }
            if(!_.isObject(globals.Memory.rooms)) {
                return undefined;
            }
            return globals.Memory.rooms[this.name] = globals.Memory.rooms[this.name] || {};
        },

        set: function(value) {
            if(_.isUndefined(globals.Memory.rooms) || globals.Memory.rooms === 'undefined') {
                globals.Memory.rooms = {};
            }
            if(!_.isObject(globals.Memory.rooms)) {
                throw new Error('Could not set room memory');
            }
            globals.Memory.rooms[this.name] = value;
        }
    });

    // vanilla: rooms.js:573
    Room.prototype.getEventLog = register.wrapFn(function(raw) {
        if(raw) {
            return runtimeData.roomEventLog[this.name] || '[]';
        }
        let {roomEventLogCache} = register;
        if(!roomEventLogCache[this.name]) {
            roomEventLogCache[this.name] = JSON.parse(runtimeData.roomEventLog[this.name] || '[]');
        }
        return roomEventLogCache[this.name];
    });

    // vanilla: rooms.js:584
    Room.prototype.find = register.wrapFn(function(type, opts) {
        var result = [];
        opts = opts || {};
        if(register.findCache[type] && register.findCache[type][this.name]) {
            result = register.findCache[type][this.name];
        }
        else {
            switch (type) {
                case C.FIND_EXIT:
                    register.findCache[type] = register.findCache[type] || {};
                    register.findCache[type][this.name] = this.find(C.FIND_EXIT_TOP, opts)
                    .concat(this.find(C.FIND_EXIT_BOTTOM, opts))
                    .concat(this.find(C.FIND_EXIT_RIGHT, opts))
                    .concat(this.find(C.FIND_EXIT_LEFT, opts));
                    return _.clone(register.findCache[type][this.name]);
                case C.FIND_EXIT_TOP:
                case C.FIND_EXIT_RIGHT:
                case C.FIND_EXIT_BOTTOM:
                case C.FIND_EXIT_LEFT:

                    register.findCache[type] = register.findCache[type] || {};

                    var exits = [];
                    for (var i = 0; i < 50; i++) {
                        var x=0,y=0;
                        if(type == C.FIND_EXIT_LEFT || type == C.FIND_EXIT_RIGHT) {
                            y = i;
                        }
                        else  {
                            x = i;
                        }
                        if(type == C.FIND_EXIT_RIGHT) {
                            x = 49;
                        }
                        if(type == C.FIND_EXIT_BOTTOM) {
                            y = 49;
                        }
                        exits.push(!(runtimeData.staticTerrainData[this.name][y*50+x] & C.TERRAIN_MASK_WALL));
                    }

                    result = _.reduce(exits, (accum, i, key) => {
                        if (i) {
                            if (type == C.FIND_EXIT_TOP) {
                                accum.push(this.getPositionAt(key, 0));
                            }
                            if (type == C.FIND_EXIT_BOTTOM) {
                                accum.push(this.getPositionAt(key, 49));
                            }
                            if (type == C.FIND_EXIT_LEFT) {
                                accum.push(this.getPositionAt(0, key));
                            }
                            if (type == C.FIND_EXIT_RIGHT) {
                                accum.push(this.getPositionAt(49, key));
                            }
                        }
                        return accum;
                    }, []);

                    register.findCache[type][this.name] = result;

                    break;
            }
        }

        if(opts.filter) {
            result = _.filter(result, opts.filter);
        }
        else {
            result = _.clone(result);
        }

        return result;

    });

    // vanilla: rooms.js:659
    function _lookSpatialRegister (id, typeName, x,y, outArray, withCoords) {

        var item;

        if(typeName == 'terrain') {
            var result = 'plain';
            var terrainCode = runtimeData.staticTerrainData[id][y*50+x];
            if(terrainCode & C.TERRAIN_MASK_SWAMP) {
                result = 'swamp';
            }
            if(terrainCode & C.TERRAIN_MASK_WALL) {
                result = 'wall';
            }
            if(outArray) {
                item = {type: 'terrain', terrain: result};
                if(withCoords) {
                    item.x = x;
                    item.y = y;
                }
                outArray.push(item);
                return;
            }
            return [result];
        }

        if(x < 0 || y < 0 || x > 49 || y > 49) {
            throw new Error('look coords are out of bounds');
        }

        // chitin (J3): vanilla's single expression, split so the array's queued writes land before the read.
        var spatialRegister = privateStore[id].lookTypeSpatialRegisters[typeName];
        flushSpatial(id, spatialRegister);
        var typeResult = spatialRegister[x * 50 + y];
        if(typeResult) {
            if(outArray) {
                typeResult.forEach((i) => {
                    item = {type: typeName};
                    item[typeName] = i;
                    if(withCoords) {
                        item.x = x;
                        item.y = y;
                    }
                    outArray.push(item);
                });
                return;
            }
            return _.clone(typeResult);
        }
        return [];
    }

    // vanilla: rooms.js:707
    function _lookAreaMixedRegister(id, type, top, left, bottom, right, withType, asArray, result) {
        var typeRegister = privateStore[id].lookTypeRegisters[type],
        keys = typeRegister && Object.keys(typeRegister);

        if(type != 'terrain' && keys.length < (bottom-top+1)*(right-left+1)) {

            // by objects

            var checkInside = (i) => {
                return (!i.pos && i.roomName == id || i.pos && i.pos.roomName == id) &&
                i.pos && i.pos.y >= top && i.pos.y <= bottom && i.pos.x >= left && i.pos.x <= right ||
                !i.pos && i.y >= top && i.y <= bottom && i.x >= left && i.x <= right;
            };
            var item;
            keys.forEach((key) => {
                var obj = typeRegister[key];
                if(checkInside(obj)) {
                    if(withType) {
                        item = {type: type};
                        item[type] = obj;
                        if(asArray) {
                            result.push({x: obj.x || obj.pos.x, y: obj.y || obj.pos.y, type, [type]: obj});
                        }
                        else {
                            result[obj.y || obj.pos.y][obj.x || obj.pos.x].push(item);
                        }
                    }
                    else {
                        if(asArray) {
                            result.push({x: obj.x || obj.pos.x, y: obj.y || obj.pos.y, [type]: obj});
                        }
                        else {
                            result[obj.y || obj.pos.y][obj.x || obj.pos.x] = result[obj.y || obj.pos.y][obj.x || obj.pos.x] || [];
                            result[obj.y || obj.pos.y][obj.x || obj.pos.x].push(obj);
                        }
                    }
                }
            });
        }
        else {

            // spatial

            for (var y = top; y <= bottom; y++) {
                for (var x = left; x <= right; x++) {
                    if(asArray) {
                        _lookSpatialRegister(id, type, x, y, result, true);
                    }
                    else {
                        if (result[y][x]) {
                            _lookSpatialRegister(id, type, x, y, result[y][x]);
                        }
                        else {
                            result[y][x] = _lookSpatialRegister(id, type, x, y, undefined);
                        }
                    }
                }
            }
        }
    }

    // vanilla: rooms.js:768
    Room.prototype.lookAt = register.wrapFn(function(firstArg, secondArg) {
        var [x,y] = utils.fetchXYArguments(firstArg, secondArg, globals),
            result = [];

        _lookSpatialRegister(this.name, C.LOOK_CREEPS, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_ENERGY, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_RESOURCES, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_SOURCES, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_MINERALS, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_DEPOSITS, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_STRUCTURES, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_FLAGS, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_CONSTRUCTION_SITES, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_TERRAIN, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_NUKES, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_TOMBSTONES, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_RUINS, x,y, result);
        _lookSpatialRegister(this.name, C.LOOK_POWER_CREEPS, x,y, result);

        if(runtimeData.customObjectPrototypes) {
            runtimeData.customObjectPrototypes.forEach(i => {
                if(i.opts.lookConstant) {
                    _lookSpatialRegister(this.name, i.opts.lookConstant, x,y, result);
                }
            });
        }

        return result;
    });

    // vanilla: rooms.js:798
    Room.prototype.lookForAt = register.wrapFn(function(type, firstArg, secondArg) {
        var [x,y] = utils.fetchXYArguments(firstArg, secondArg, globals);

        if(type != 'terrain' && !(type in privateStore[this.name].lookTypeSpatialRegisters)) {
            return C.ERR_INVALID_ARGS;
        }

        return _lookSpatialRegister(this.name, type, x,y);
    });

    // vanilla: rooms.js:808
    Room.prototype.lookAtArea = register.wrapFn(function(top, left, bottom, right, asArray) {

        var result = asArray ? [] : {};

        if(!asArray) {
            for (var y = top; y <= bottom; y++) {
                result[y] = {};
                for (var x = left; x <= right; x++) {
                    result[y][x] = [];
                }
            }
        }

        _lookAreaMixedRegister(this.name, C.LOOK_CREEPS, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_ENERGY, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_RESOURCES, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_SOURCES, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_MINERALS, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_DEPOSITS, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_STRUCTURES, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_FLAGS, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_CONSTRUCTION_SITES, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_TERRAIN, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_NUKES, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_TOMBSTONES, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_RUINS, top, left, bottom, right, true, asArray, result);
        _lookAreaMixedRegister(this.name, C.LOOK_POWER_CREEPS, top, left, bottom, right, true, asArray, result);

        if(runtimeData.customObjectPrototypes) {
            runtimeData.customObjectPrototypes.forEach(i => {
                if(i.opts.lookConstant) {
                    _lookAreaMixedRegister(this.name, i.opts.lookConstant, top, left, bottom, right, true, asArray, result);
                }
            });
        }

        return result;
    });

    // vanilla: rooms.js:847
    Room.prototype.lookForAtArea = register.wrapFn(function(type, top, left, bottom, right, asArray) {

        var result = asArray ? [] : {};

        if(!asArray) {
            for (var y = top; y <= bottom; y++) {
                result[y] = {};
            }
        }

        _lookAreaMixedRegister(this.name, type, top, left, bottom, right, false, asArray, result);

        return result;
    });


    // vanilla: rooms.js:863
    Room.prototype.findPath = register.wrapFn(function(fromPos, toPos, opts) {

        if(fromPos.roomName != this.name) {
            return opts.serialize ? '' : [];
        }

        if(register._useNewPathFinder) {
            return _findPath2(this.name, fromPos, toPos, opts);
        }

        var fromX = fromPos.x, fromY = fromPos.y,
            path,
            cacheKeySuffix = '';

        opts = _.clone(opts || {});

        if(opts.ignoreCreeps) {
            cacheKeySuffix += '_ignoreCreeps'
        }
        if(opts.ignoreDestructibleStructures) {
            cacheKeySuffix += '_ignoreDestructibleStructures'
        }
        if(opts.avoid) {
            cacheKeySuffix += '_avoid'+privateStore[this.name].positionsSetCache.key(opts.avoid);
        }
        if(opts.ignore) {
            cacheKeySuffix += '_ignore'+privateStore[this.name].positionsSetCache.key(opts.ignore);
        }

        if(_.isNumber(toPos)) {
            if(!privateStore[this.name].pfEndNodes[toPos]) {
                return opts.serialize ? '' : [];
            }

            var grid = getPathfindingGrid(this.name, opts, toPos);

            // vanilla: rooms.js:443 creates the finder in the Room constructor (LEGACY-PF: created here, stateless)
            if(!privateStore[this.name].pfDijkstraFinder) {
                privateStore[this.name].pfDijkstraFinder = new (legacyPathfinding().DijkstraFinder)({diagonalMovement: 1});
            }
            path = privateStore[this.name].pfDijkstraFinder.findPath(fromX, fromY, -999, -999, grid);
        }
        else {

            if(toPos.roomName != this.name) {
                return opts.serialize ? '' : [];
            }

            var toX = toPos.x, toY = toPos.y,
                cacheKey = `${fromX},${fromY},${toX},${toY}${cacheKeySuffix}`;

            if(privateStore[this.name].pathCache[cacheKey]) {
                return opts.serialize ? utils.serializePath(privateStore[this.name].pathCache[cacheKey]) : _.cloneDeep(privateStore[this.name].pathCache[cacheKey]);
            }

            if (fromX == toX && fromY == toY) {
                return opts.serialize ? '' : [];
            }
            if (fromX < 0 || fromY < 0 || toX < 0 || toY < 0 ||
                fromX >= 50 || fromY >= 50 || toX >= 50 || toY >= 50) {
                return opts.serialize ? '' : [];
            }

            if (abs(fromX - toX) < 2 && abs(fromY - toY) < 2) {
                var result = [{
                    x: toX,
                    y: toY,
                    dx: toX - fromX,
                    dy: toY - fromY,
                    direction: utils.getDirection(toX - fromX, toY - fromY)
                }];
                return opts.serialize ? utils.serializePath(result) : result;
            }

            var grid = getPathfindingGrid(this.name, opts),
                finder = getPathfinder(this.name, opts);

            grid.setWalkableAt(toX, toY, true);
            path = finder.findPath(fromX, fromY, toX, toY, grid);
        }

        path.splice(0,1);

        var curX = fromX, curY = fromY;

        var resultPath = _.map(path, (step) => {
            var result = {
                x: step[0],
                y: step[1],
                dx: step[0] - curX,
                dy: step[1] - curY,
                direction: utils.getDirection(step[0] - curX, step[1] - curY)
            };

            curX = result.x;
            curY = result.y;
            return result;
        });

        if(resultPath.length > 0) {
            var lastStep = resultPath[resultPath.length-1],
                cacheKey = `${fromX},${fromY},${lastStep.x},${lastStep.y}${cacheKeySuffix}`;
            privateStore[this.name].pathCache[cacheKey] = _.cloneDeep(resultPath);
        }

        if(opts.serialize) {
            return utils.serializePath(resultPath);
        }

        return resultPath;
    });

    // vanilla: rooms.js:971
    Room.prototype.getPositionAt = register.wrapFn(function(x,y) {
        if(x < 0 || x > 49 || y < 0 || y > 49) {
            return null;
        }
        return new RoomPosition(x,y,this.name);
    });

    // vanilla: rooms.js:978
    Room.prototype.createFlag = register.wrapFn(function(firstArg, secondArg, name, color, secondaryColor) {
        var [x,y] = utils.fetchXYArguments(firstArg, secondArg, globals);

        if(_.isUndefined(x) || _.isUndefined(y) || x < 0 || x > 49 || y < 0 || y > 49) {
            return C.ERR_INVALID_ARGS;
        }
        if(_.size(globals.Game.flags) >= C.FLAGS_LIMIT) {
            return C.ERR_FULL;
        }
        if(_.isObject(firstArg)) {
            secondaryColor = color;
            color = name;
            name = secondArg;
        }
        if(!color) {
            color = C.COLOR_WHITE;
        }
        if(!secondaryColor) {
            secondaryColor = color;
        }
        if(!_.contains(C.COLORS_ALL, color)) {
            return C.ERR_INVALID_ARGS;
        }
        if(!_.contains(C.COLORS_ALL, secondaryColor)) {
            return C.ERR_INVALID_ARGS;
        }
        if(!name) {
            var cnt = 1;
            do {
                name = 'Flag'+cnt;
                cnt++;
            }
            while(_.any(register.flags, {name}) || createdFlagNames.indexOf(name) != -1);
        }
        if(_.any(register.flags, {name}) || createdFlagNames.indexOf(name) != -1) {
            return C.ERR_NAME_EXISTS;
        }
        if(name.length > 100) {
            return C.ERR_INVALID_ARGS;
        }

        createdFlagNames.push(name);

        const roomName = ""+this.name;
        globals.Game.flags[name] = new Flag(name, color, secondaryColor, roomName, x, y);

        intents.pushByName('room', 'createFlag', {roomName, x, y, name, color, secondaryColor});

        return name;
    });

    // vanilla: rooms.js:1029
    Room.prototype.createConstructionSite = register.wrapFn(function(firstArg, secondArg, structureType, name) {
        var [x,y] = utils.fetchXYArguments(firstArg, secondArg, globals);

        if(_.isUndefined(x) || _.isUndefined(y) || x < 0 || x > 49 || y < 0 || y > 49) {
            return C.ERR_INVALID_ARGS;
        }
        if(_.isString(secondArg) && _.isUndefined(structureType)) {
            structureType = secondArg;
        }
        if(!C.CONSTRUCTION_COST[structureType]) {
            return C.ERR_INVALID_ARGS;
        }
        if(structureType == 'spawn' && typeof name == 'string') {
            if(name.length > 100) {
                return C.ERR_INVALID_ARGS;
            }
            if(createdSpawnNames.indexOf(name) != -1) {
                return C.ERR_INVALID_ARGS;
            }
            if(_.any(register.spawns, {name}) || _.any(register.constructionSites, {structureType: 'spawn', name})) {
                return C.ERR_INVALID_ARGS;
            }
        }
        if(this.controller && this.controller.level > 0 && !this.controller.my) {
            return C.ERR_NOT_OWNER;
        }
        if(this.controller &&
            this.controller.reservation &&
            register.objectsByRoom[this.name] &&
            register.objectsByRoom[this.name][this.controller.id] &&
            (register.objectsByRoom[this.name][this.controller.id].reservation.user != runtimeData.user._id)) {
            return C.ERR_NOT_OWNER;
        }
        const roomName = ""+this.name;
        if(!utils.checkControllerAvailability(structureType, register.objectsByRoom[this.name], this.controller)) {
            return C.ERR_RCL_NOT_ENOUGH;
        }
        if(!utils.checkConstructionSite(register.objectsByRoom[roomName], structureType, x, y) ||
            !utils.checkConstructionSite(runtimeData.staticTerrainData[roomName], structureType, x, y)) {
            return C.ERR_INVALID_TARGET;
        }

        if(_(runtimeData.userObjects).filter({type: 'constructionSite'}).size() + createdConstructionSites >= C.MAX_CONSTRUCTION_SITES) {
            return C.ERR_FULL;
        }

        var intent = {roomName, x, y, structureType};

        if(structureType == 'spawn') {
            if(typeof name !== 'string') {
                var cnt = 1;
                do {
                    name = "Spawn" + cnt;
                    cnt++;
                }
                while (_.any(register.spawns, {name}) ||
                _.any(register.constructionSites, {structureType: 'spawn', name}) ||
                createdSpawnNames.indexOf(name) != -1);
            }
            createdSpawnNames.push(name);
            intent.name = name;
        }

        createdConstructionSites++;

        intents.pushByName('room', 'createConstructionSite', intent);

        return C.OK;
    });

    // vanilla: rooms.js:1099
    Room.prototype.getEndNodes = register.wrapFn(function(type, opts) {
        var key;

        opts = opts || {};

        if(_.isUndefined(type)) {
            throw new Error('Find type cannot be undefined');
        }

        if(!opts.filter && _.isNumber(type)) {
            key = type;
        }
        else {
            if(_.isNumber(type)) {
                type = this.find(type, opts);
            }

            key = privateStore[this.name].positionsSetCache.key(type);

            privateStore[this.name].pfEndNodes[key] = privateStore[this.name].positionsSetCache.cache[key];
        }

        if(!privateStore[this.name].pfEndNodes[key]) {
            privateStore[this.name].pfEndNodes[key] = _.clone(type);
            if (_.isNumber(type)) {
                privateStore[this.name].pfEndNodes[key] = this.find(type, opts);
            }
        }
        return {key, objects: privateStore[this.name].pfEndNodes[key]};
    });

    // vanilla: rooms.js:1130
    Room.prototype.findExitTo = register.wrapFn(function(room) {
        return register.map.findExit(this.name, room);
    });

    // vanilla: rooms.js:1134
    Room.prototype.getTerrain = register.wrapFn(function() {
        return new Room.Terrain(this.name);
    });

    Object.defineProperty(globals, 'Room', {enumerable: true, value: Room});

    // vanilla: rooms.js:1146
    var RoomVisual = register.wrapFn(function(roomName) {
        this.roomName = roomName;
    });

    // vanilla: rooms.js:1150
    RoomVisual.prototype.circle = register.wrapFn(function(x,y,style) {
        if(typeof x == 'object') {
            style = y;
            y = x.y;
            x = x.x;
        }
        globals.console.addVisual(this.roomName, {t: 'c', x,y,s:style});
        return this;
    });

    // vanilla: rooms.js:1160
    RoomVisual.prototype.line = register.wrapFn(function(x1,y1,x2,y2,style) {
        if(typeof x1 == 'object' && typeof y1 == 'object') {
            style = x2;
            x2 = y1.x;
            y2 = y1.y;
            y1 = x1.y;
            x1 = x1.x;
        }
        globals.console.addVisual(this.roomName, {t: 'l', x1,y1,x2,y2,s:style});
        return this;
    });

    // vanilla: rooms.js:1172
    RoomVisual.prototype.rect = register.wrapFn(function(x,y,w,h,style) {
        if(typeof x == 'object') {
            style = h;
            h = w;
            w = y;
            y = x.y;
            x = x.x;
        }
        globals.console.addVisual(this.roomName, {t: 'r', x,y,w,h,s:style});
        return this;
    });

    // vanilla: rooms.js:1184
    RoomVisual.prototype.poly = register.wrapFn(function(points,style) {
        if(_.isArray(points) && _.some(points)) {
            points = points.map(i => i.x !== undefined ? [i.x, i.y] : i);
            globals.console.addVisual(this.roomName, {t: 'p', points,s:style});
        }
        return this;
    });

    // vanilla: rooms.js:1192
    RoomVisual.prototype.text = register.wrapFn(function(text,x,y,style) {
        if(typeof x == 'object') {
            style = y;
            y = x.y;
            x = x.x;
        }
        globals.console.addVisual(this.roomName, {t: 't', text,x,y,s:style});
        return this;
    });

    // vanilla: rooms.js:1202
    RoomVisual.prototype.getSize = register.wrapFn(function() {
        return globals.console.getVisualSize(this.roomName);
    });

    // vanilla: rooms.js:1206
    RoomVisual.prototype.clear = register.wrapFn(function() {
        globals.console.clearVisual(this.roomName);
        return this;
    });

    // vanilla: rooms.js:1211
    RoomVisual.prototype.export = register.wrapFn(function() {
        return globals.console.getVisual(this.roomName);
    });

    // vanilla: rooms.js:1215
    RoomVisual.prototype.import = register.wrapFn(function(data) {
        globals.console.addVisual(this.roomName, "" + data);
        return this;
    });

    Object.defineProperty(globals, 'RoomVisual', {enumerable: true, value: RoomVisual});


    // vanilla: rooms.js:1223
    Room.Terrain = register.wrapFn(function(roomName){ "use strict";
        roomName = "" + roomName;

        const array = (runtimeData.staticTerrainData || {})[roomName];
        if(!array)
            throw new Error(`Could not access room ${roomName}`);

        this.get = register.wrapFn(function(x,y){
            const value = array[y * 50 + x];
            return (value & C.TERRAIN_MASK_WALL) || (value & C.TERRAIN_MASK_SWAMP) || 0;
        });

        this.getRawBuffer = register.wrapFn(function(destinationArray){
            if(!!destinationArray) {
                TerrainConstructorSet.call(destinationArray, array);
                return destinationArray;
            }
            return new TerrainConstructor(array);
        });
    });
};

// J9 (ARCHITECTURE.md §7.2): vanilla's `if(globals.RoomPosition)` in make(), where `RoomPosition` is make()'s own local;
// out here it is the global, read through its property cell.
function hasRoomPosition() {
    return typeof RoomPosition !== 'undefined' && !!RoomPosition;
}

// vanilla: rooms.js:1245
exports.makePos = function(_register) {

    register = _register;

    if(hasRoomPosition()) {
        return;
    }

    const kMaxWorldSize = 256;
    const kMaxWorldSize2 = kMaxWorldSize >> 1;
    const roomNames = [];
    utils.getRoomNameFromXY = function(slowFn) {
        return function(xx, yy) {
            let id = (xx + kMaxWorldSize2) << 8 | (yy + kMaxWorldSize2);
            let roomName = roomNames[id];
            if (roomName === undefined) {
                return roomNames[id] = slowFn(xx, yy);
            } else {
                return roomName;
            }
        };
    }(utils.getRoomNameFromXY);
    roomNames[0] = 'sim';

    // vanilla: rooms.js:1276
    let RoomPosition = register.wrapFn(function RoomPosition(xx, yy, roomName) {
        // chitin (J10): the room's [x, y] may be shared (roomXYOf), so the offsets go to locals, not into the array.
        let xy = roomName === 'sim' ? [-kMaxWorldSize2, -kMaxWorldSize2] : roomXYOf(roomName);
        let xy0 = xy[0] + kMaxWorldSize2;
        let xy1 = xy[1] + kMaxWorldSize2;
        if (
            xy0 < 0 || xy0 > kMaxWorldSize || xy0 !== xy0 ||
            xy1 < 0 || xy1 > kMaxWorldSize || xy1 !== xy1 ||
            xx < 0 || xx > 49 || xx !== xx ||
            yy < 0 || yy > 49 || yy !== yy
        ) {
            throw new Error('Invalid arguments in RoomPosition constructor');
        }
        Object.defineProperty(this, '__packedPos', {
            enumerable: false,
            value: xy0 << 24 | xy1 << 16 | xx << 8 | yy,
            writable: true,
        });
    });

    // vanilla: rooms.js:1295
    Object.defineProperties(RoomPosition.prototype, {
        x: {
            enumerable: true,
            get() {
                return (this.__packedPos >> 8) & 0xff;
            },
            set(val) {
                if (val < 0 || val > 49 || val !== val) {
                    throw new Error('Invalid coordinate');
                }
                this.__packedPos = this.__packedPos & ~(0xff << 8) | val << 8;
            },
        },

        y: {
            enumerable: true,
            get() {
                return this.__packedPos & 0xff;
            },
            set(val) {
                if (val < 0 || val > 49 || val !== val) {
                    throw new Error('Invalid coordinate');
                }
                this.__packedPos = this.__packedPos & ~0xff | val;
            },
        },

        roomName: {
            enumerable: true,
            get() {
                let roomName = roomNames[this.__packedPos >>> 16];
                if (roomName === undefined) {
                    return utils.getRoomNameFromXY(
                        (this.__packedPos >>> 24) - kMaxWorldSize2,
                        (this.__packedPos >>> 16 & 0xff) - kMaxWorldSize2
                    );
                } else {
                    return roomName;
                }
            },
            set(val) {
                let xy = val === 'sim' ? [-kMaxWorldSize2, -kMaxWorldSize2] : utils.roomNameToXY(val);
                xy[0] += kMaxWorldSize2;
                xy[1] += kMaxWorldSize2;
                if (
                    xy[0] < 0 || xy[0] > kMaxWorldSize || xy[0] !== xy[0] ||
                    xy[1] < 0 || xy[1] > kMaxWorldSize || xy[1] !== xy[1]
                ) {
                    throw new Error('Invalid roomName');
                }
                this.__packedPos = this.__packedPos & ~(0xffff << 16) | xy[0] << 24 | xy[1] << 16;
            },
        },
    });

    // vanilla: rooms.js:1350
    RoomPosition.prototype.toJSON = register.wrapFn(function() {
        return Object.assign({
            x: this.x,
            y: this.y,
            roomName: this.roomName,
        }, this);
    });

    // vanilla: rooms.js:1358
    RoomPosition.prototype.toString = register.wrapFn(function() {
        return `[room ${this.roomName} pos ${this.x},${this.y}]`;
    });

    // vanilla: rooms.js:1362
    RoomPosition.prototype.inRangeTo = register.wrapFn(function(firstArg, secondArg, thirdArg) {
        var x = firstArg, y = secondArg, range = thirdArg, roomName = this.roomName;
        if(_.isUndefined(thirdArg)) {
            var pos = firstArg;
            if(pos.pos) {
                pos = pos.pos;
            }
            x = pos.x;
            y = pos.y;
            roomName = pos.roomName;
            range = secondArg;
        }

        return abs(x - this.x) <= range && abs(y - this.y) <= range && roomName == this.roomName;
    });

    // vanilla: rooms.js:1378
    RoomPosition.prototype.isNearTo = register.wrapFn(function(firstArg, secondArg) {
        var [x,y,roomName] = utils.fetchXYArguments(firstArg, secondArg, globals);
        return abs(x - this.x) <= 1 && abs(y - this.y) <= 1 && (!roomName || roomName == this.roomName);
    });

    // vanilla: rooms.js:1383
    RoomPosition.prototype.getDirectionTo = register.wrapFn(function(firstArg, secondArg) {
        var [x,y,roomName] = utils.fetchXYArguments(firstArg, secondArg, globals);

        if(!roomName || roomName == this.roomName) {
            return utils.getDirection(x - this.x, y - this.y);
        }

        var [thisRoomX, thisRoomY] = utils.roomNameToXY(this.roomName);
        var [thatRoomX, thatRoomY] = utils.roomNameToXY(roomName);

        return utils.getDirection(thatRoomX*50 + x - thisRoomX*50 - this.x, thatRoomY*50 + y - thisRoomY*50 - this.y);
    });

    // vanilla: rooms.js:1396
    RoomPosition.prototype.findPathTo = register.wrapFn(function(firstArg, secondArg, opts) {

        var [x,y,roomName] = utils.fetchXYArguments(firstArg, secondArg, globals),
            room = register.rooms[this.roomName];

        if(_.isObject(secondArg)) {
            opts = _.clone(secondArg);
        }
        opts = opts || {};

        roomName = roomName || this.roomName;

        if(!room) {
            throw new Error(`Could not access room ${this.roomName}`);
        }

        if(roomName == this.roomName || register._useNewPathFinder) {
            return room.findPath(this, new RoomPosition(x,y,roomName), opts);
        }
        else {
            var exitDir = room.findExitTo(roomName);
            if(exitDir < 0) {
                return [];
            }
            var exit = this.findClosestByPath(exitDir, opts);
            if(!exit) {
                return [];
            }
            return room.findPath(this, exit, opts);
        }

    });

    // vanilla: rooms.js:1429
    RoomPosition.prototype.findClosestByPath = register.wrapFn(function(type, opts) {

        opts = _.clone(opts || {});

        var room = register.rooms[this.roomName];

        if(!room) {
            throw new Error(`Could not access room ${this.roomName}`);
        }

        if(_.isUndefined(type)) {
            return null;
        }

        if(register._useNewPathFinder) {
            return _findClosestByPath2(this, type, opts);
        }

        opts.serialize = false;

        var result = null,
            isNear,
            endNodes = room.getEndNodes(type, opts);

        if(!opts.algorithm) {

            var minH, sumH = 0;

            endNodes.objects.forEach((i) => {
                var x = i.x, y = i.y, roomName = i.roomName;
                if(i.pos) {
                    x = i.pos.x;
                    y = i.pos.y;
                    roomName = i.pos.roomName;
                }
                var h = max(abs(this.x - x), abs(this.y - y));
                if(_.isUndefined(minH) || minH > h) {
                    minH = h;
                }
                sumH += h;
            });

            opts.algorithm = sumH > minH*10 ? 'dijkstra' : 'astar';
        }

        if(opts.algorithm == 'dijkstra') {

            isNear = 1;

            endNodes.objects.forEach((i) => {
                var distance = this.isEqualTo(i) ? -1 :
                    this.isNearTo(i) ? 0 : 1;
                if(distance < isNear) {
                    result = i;
                    isNear = distance;
                }
            });

            if(isNear == 1) {
                var path = room.findPath(this, endNodes.key, opts);
                if(path.length > 0) {
                    var lastStep = path[path.length-1],
                        lastStepPos = room.getPositionAt(lastStep.x, lastStep.y);
                    result = _.find(endNodes.objects, (i) => lastStepPos.isEqualTo(i));
                }
            }
        }

        if(opts.algorithm == 'astar') {

            endNodes.objects.forEach((i) => {

                var path,
                    distance = this.isEqualTo(i) ? -1 :
                        this.isNearTo(i) ? 0 :
                            (path = this.findPathTo(i, opts)) && path.length > 0 &&
                            room.getPositionAt(path[path.length - 1].x, path[path.length - 1].y).isNearTo(i) ?
                                path.length : undefined;

                if ((_.isUndefined(isNear) || distance <= isNear) && !_.isUndefined(distance)) {
                    isNear = distance;
                    result = i;
                }
            });
        }

        return result;
    });

    // vanilla: rooms.js:1518
    RoomPosition.prototype.findInRange = register.wrapFn(function(type, range, opts) {
        var room = register.rooms[this.roomName];

        if(!room) {
            throw new Error(`Could not access room ${this.roomName}`);
        }

        opts = _.clone(opts || {});

        var objects = [],
            result = [];

        if(_.isNumber(type)) {
            objects = room.find(type, opts);
        }
        if(_.isArray(type)) {
            objects = opts.filter ? _.filter(type, opts.filter) : type;
        }

        objects.forEach((i) => {
            if(this.inRangeTo(i, range)) {
                result.push(i);
            }
        });

        return result;
    });

    // vanilla: rooms.js:1546
    RoomPosition.prototype.findClosestByRange = register.wrapFn(function(type, opts) {
        var room = register.rooms[this.roomName];

        if(!room) {
            throw new Error(`Could not access room ${this.roomName}`);
        }

        opts = _.clone(opts || {});

        var objects = [],
        result = [];

        if(_.isNumber(type)) {
            objects = room.find(type, opts);
        }
        if(_.isArray(type)) {
            objects = opts.filter ? _.filter(type, opts.filter) : type;
        }

        var closest = null, minRange = Infinity;

        objects.forEach((i) => {
            var range = this.getRangeTo(i);
            if(range < minRange) {
                minRange = range;
                closest = i;
            }
        });

        return closest;
    });

    // vanilla: rooms.js:1578
    RoomPosition.prototype.isEqualTo = register.wrapFn(function(firstArg, secondArg) {
        if (firstArg.__packedPos !== undefined) {
            return firstArg.__packedPos === this.__packedPos;
        }
        var [x,y,roomName] = utils.fetchXYArguments(firstArg, secondArg, globals);
        return x == this.x && y == this.y && (!roomName || roomName == this.roomName);
    });

    // vanilla: rooms.js:1586
    RoomPosition.prototype.getRangeTo = register.wrapFn(function(firstArg, secondArg) {
        var [x,y,roomName] = utils.fetchXYArguments(firstArg, secondArg, globals);
        if(roomName && roomName != this.roomName) {
            return Infinity;
        }
        return max(abs(this.x - x), abs(this.y - y));
    });

    // vanilla: rooms.js:1594
    RoomPosition.prototype.look = register.wrapFn(function() {
        var room = register.rooms[this.roomName];
        if(!room) {
            throw new Error(`Could not access room ${this.roomName}`);
        }
        return room.lookAt(this);
    });

    // vanilla: rooms.js:1602
    RoomPosition.prototype.lookFor = register.wrapFn(function(type) {
        if(type == 'terrain') {
            var terrainCode = runtimeData.staticTerrainData[this.roomName][this.y*50+this.x];
            if(terrainCode & C.TERRAIN_MASK_WALL) {
                return ['wall'];
            }
            else if(terrainCode & C.TERRAIN_MASK_SWAMP) {
                return ['swamp'];
            }
            else {
                return ['plain'];
            }
        }
        var room = register.rooms[this.roomName];
        if(!room) {
            throw new Error(`Could not access room ${this.roomName}`);
        }
        return room.lookForAt(type, this);
    });

    // vanilla: rooms.js:1622
    RoomPosition.prototype.createFlag = register.wrapFn(function(name, color, secondaryColor) {
        var room = register.rooms[this.roomName];
        if(!room) {
            throw new Error(`Could not access room ${this.roomName}`);
        }
        return room.createFlag(this, name, color, secondaryColor);
    });

    // vanilla: rooms.js:1630
    RoomPosition.prototype.createConstructionSite = register.wrapFn(function(structureType, name) {
        var room = register.rooms[this.roomName];
        if(!room) {
            throw new Error(`Could not access room ${this.roomName}`);
        }
        return room.createConstructionSite(this.x, this.y, structureType, name);
    });

    Object.defineProperty(globals, 'RoomPosition', {enumerable: true, value: RoomPosition});


    // vanilla: rooms.js:1648
    var RoomObject = register.wrapFn(function(x, y, room, effects) {
        this.room = register.rooms[room];
        this.pos = new RoomPosition(x,y,room);
        if(effects) {
            this.effects = _(effects).map(i => ({
                power: i.power,
                effect: i.effect,
                level: i.level,
                ticksRemaining: i.endTime - runtimeData.time
            })).filter(i => i.ticksRemaining > 0).value();
        }
    });

    Object.defineProperty(globals, 'RoomObject', {enumerable: true, value: RoomObject});
};
