// mirror: reference/engine/src/game/map.js
// parity: FAITHFUL
// J9 (ARCHITECTURE.md §7.2): engine classes are read as plain globals (`RoomPosition`), vanilla `globals.RoomPosition`.
// Deviations (ARCHITECTURE.md §6.7, §7.2): J3 the world grid, accessibility and room status come from the natives the
//   runtime driver exposes (driver.gridData/isAccessible/roomStatus) instead of runtimeData.mapGrid.gridData, a
//   JSON.parse of runtimeData.accessibleRooms per tick and runtimeData.roomStatusData; J1 for getTerrainAt.
var utils = require('../utils'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants,
    _ = require('lodash'),
    pathUtils = require('./path-utils'),
    Heap = pathUtils.Heap,
    OpenClosed = pathUtils.OpenClosed;

const kRouteGrid = 30;

// vanilla: map.js:11
exports.makeMap = function(runtimeData, register, globals) {

    var heap, openClosed, parents;
    var originX, originY;
    var toX, toY;
    let _visual;

    // vanilla: map.js:18 `JSON.parse(runtimeData.accessibleRooms)` - J3: driver.isAccessible per query

    // vanilla: map.js:20
    function describeExits(roomName) {
        if(!/^(W|E)\d+(N|S)\d+$/.test(roomName)) {
            return null;
        }
        var [x,y] = utils.roomNameToXY(roomName);
        var gridItem = driver.gridData(x, y);   // J3: vanilla runtimeData.mapGrid.gridData[`${x},${y}`]
        if(!gridItem) {
            return null;
        }

        var exits = {};

        if(gridItem.t) {
            exits[C.TOP] = utils.getRoomNameFromXY(x,y-1);
        }
        if(gridItem.b) {
            exits[C.BOTTOM] = utils.getRoomNameFromXY(x,y+1);
        }
        if(gridItem.l) {
            exits[C.LEFT] = utils.getRoomNameFromXY(x-1,y);
        }
        if(gridItem.r) {
            exits[C.RIGHT] = utils.getRoomNameFromXY(x+1,y);
        }

        return exits;
    }

    // vanilla: map.js:48
    function xyToIndex(xx, yy) {
      let ox = originX - xx;
      let oy = originY - yy;
      if (ox < 0 || ox >= kRouteGrid * 2 || oy < 0 || oy >= kRouteGrid * 2) {
        return;
      }
      return ox * kRouteGrid * 2 + oy;
    }

    // vanilla: map.js:57
    function indexToXY(index) {
      return [ originX - Math.floor(index / (kRouteGrid * 2)), originY - index % (kRouteGrid * 2) ];
    }

    // vanilla: map.js:61
    function heuristic(xx, yy) {
      return Math.abs(xx - toX) + Math.abs(yy - toY);
    }

    // vanilla: map.js:67
    const map = {

        // vanilla: map.js:69
        findRoute(fromRoom, toRoom, opts) {
            if(_.isObject(fromRoom)) {
                fromRoom = fromRoom.name;
            }
            if(_.isObject(toRoom)) {
                toRoom = toRoom.name;
            }
            if(fromRoom == toRoom) {
                return [];
            }

            if(!/(W|E)\d+(N|S)\d+$/.test(fromRoom) || !/(W|E)\d+(N|S)\d+$/.test(toRoom)) {
                return C.ERR_NO_PATH;
            }

            var [fromX, fromY] = utils.roomNameToXY(fromRoom);
            [toX, toY] = utils.roomNameToXY(toRoom);

            if (fromX == toX && fromY == toY) {
                return [];
            }

            originX = fromX + kRouteGrid;
            originY = fromY + kRouteGrid;

            // Init path finding structures
            if (heap) {
              heap.clear();
              openClosed.clear();
            } else {
              heap = new Heap(Math.pow(kRouteGrid * 2, 2), Float64Array);
              openClosed = new OpenClosed(Math.pow(kRouteGrid * 2, 2));
            }
            if (!parents) {
              parents = new Uint16Array(Math.pow(kRouteGrid * 2, 2));
            }
            var fromIndex = xyToIndex(fromX, fromY);
            heap.push(fromIndex, heuristic(fromX, fromY));
            var routeCallback = (opts && opts.routeCallback) || function() { return 1; };

            // Astar
            while (heap.size()) {

              // Pull node off heap
              let index = heap.min();
              let fcost = heap.minPriority();

              // Close this node
              heap.pop();
              openClosed.close(index);

              // Calculate costs
              let [ xx, yy ] = indexToXY(index);
              let hcost = heuristic(xx, yy);
              let gcost = fcost - hcost;

              // Reached destination?
              if (hcost === 0) {
                let route = [];
                while (index !== fromIndex) {
                  let [ xx, yy ] = indexToXY(index);
                  index = parents[index];
                  let [ nx, ny ] = indexToXY(index);
                  let dir;
                  if (nx < xx) {
                    dir = C.FIND_EXIT_RIGHT;
                  } else if (nx > xx) {
                    dir = C.FIND_EXIT_LEFT;
                  } else if (ny < yy) {
                    dir = C.FIND_EXIT_BOTTOM;
                  } else {
                    dir = C.FIND_EXIT_TOP;
                  }
                  route.push({
                    exit: dir,
                    room: utils.getRoomNameFromXY(xx, yy),
                  });
                }
                route.reverse();
                return route;
              }

              // Add neighbors
              let fromRoomName = utils.getRoomNameFromXY(xx, yy);
              let exits = describeExits(fromRoomName);
              for (let dir in exits) {

                // Calculate costs and check if this node was already visited
                let roomName = exits[dir];
                let graphKey = fromRoomName+ ':'+ roomName;
                let [ xx, yy ] = utils.roomNameToXY(roomName);
                let neighborIndex = xyToIndex(xx, yy);
                if (neighborIndex === undefined || openClosed.isClosed(neighborIndex)) {
                  continue;
                }
                let cost = Number(routeCallback(roomName, fromRoomName)) || 1;
                if (cost === Infinity) {
                  continue;
                }

                let fcost = gcost + heuristic(xx, yy) + cost;

                // Add to or update heap
                if (openClosed.isOpen(neighborIndex)) {
                  if (heap.priority(neighborIndex) > fcost) {
                    heap.update(neighborIndex, fcost);
                    parents[neighborIndex] = index;
                  }
                } else {
                  heap.push(neighborIndex, fcost);
                  openClosed.open(neighborIndex);
                  parents[neighborIndex] = index;
                }
              }
            }

            return C.ERR_NO_PATH;
        },

        // vanilla: map.js:188
        findExit(fromRoom, toRoom, opts) {
            var route = this.findRoute(fromRoom, toRoom, opts);
            if(!_.isArray(route)) {
                return route;
            }
            if(!route.length) {
                return C.ERR_INVALID_ARGS;
            }
            return route[0].exit;
        },

        describeExits,

        // vanilla: map.js:201
        isRoomAvailable(roomName) {
            register.deprecated('Method `Game.map.isRoomAvailable` is deprecated and will be removed. Please use `Game.map.getRoomStatus` instead.');
            if(!/^(W|E)\d+(N|S)\d+$/.test(roomName)) {
                return false;
            }
            // J3: vanilla `_.contains(accessibleRooms, roomName)` (strict equality, so only a string can match)
            return typeof roomName == 'string' && !!driver.isAccessible(roomName);
        },

        // vanilla: map.js:209
        getRoomStatus(roomName) {
            if(!/^(W|E)\d+(N|S)\d+$/.test(roomName)) {
                return undefined;
            }

            // vanilla: map.js:214-226 - J3: the closed/novice/respawn lookups (property keys, hence String) are one
            // native query on exact stored names; vanilla's 'No runtime status data' throw (cold runner cache,
            // 01 CLK-RUNNER-CACHE: removed) cannot occur
            const status = driver.roomStatus(String(roomName));
            if(status) {
                return { status: status.status, timestamp: status.timestamp };
            }

            if(typeof roomName == 'string' && driver.isAccessible(roomName)) {
                return { status: 'normal', timestamp: null };
            }

            return { status: 'closed', timestamp: null };
        },

        // vanilla: map.js:235
        getTerrainAt(x, y, roomName) {
            register.deprecated('Method `Game.map.getTerrainAt` is deprecated and will be removed. Please use a faster method `Game.map.getRoomTerrain` instead.');
            if(_.isObject(x)) {
                y = x.y;
                roomName = x.roomName;
                x = x.x;
            }

            // check if coordinates are out of bounds
            if(x < 0 || x > 49 || y < 0 || y > 49) {
                return undefined;
            }

            if(!runtimeData.staticTerrainData || !runtimeData.staticTerrainData[roomName]) {
                return undefined;
            }
            var terrain = runtimeData.staticTerrainData[roomName][y*50+x];
            if(terrain & C.TERRAIN_MASK_WALL) {
                return 'wall'
            }
            if(terrain & C.TERRAIN_MASK_SWAMP) {
                return 'swamp';
            }
            return 'plain';
        },

        // vanilla: map.js:261
        getRoomTerrain(roomName) {
            return new Room.Terrain(roomName);
        },

        // vanilla: map.js:265
        getRoomLinearDistance(roomName1, roomName2, continuous) {
            return utils.calcRoomsDistance(roomName1, roomName2, continuous);
        },

        // vanilla: map.js:269
        getWorldSize() {
            return driver.getWorldSize();
        },
    };

    // vanilla: map.js:274
    Object.defineProperties(map, {
        visual: {
            enumerable: true,
            get() {
                if(!_visual) {
                    _visual = {};
                    Object.defineProperties(_visual, {
                        circle: {
                            value: function(pos, style) {
                                if(!(pos instanceof RoomPosition)) {
                                    throw new Error('Invalid pos, RoomPosition expected')
                                }
                                globals.console.addVisual("map", {
                                    t: 'c',
                                    x: pos.x, y: pos.y, n: pos.roomName,
                                    s: style || {}});
                                return this;
                            }
                        },
                        line: {
                            value: function(pos1, pos2, style) {
                                if(!(pos1 instanceof RoomPosition)) {
                                    throw new Error('Invalid pos1, RoomPosition expected')
                                }
                                if(!(pos2 instanceof RoomPosition)) {
                                    throw new Error('Invalid pos2, RoomPosition expected')
                                }
                                globals.console.addVisual("map", {
                                    t: 'l',
                                    x1: pos1.x, y1: pos1.y, n1: pos1.roomName,
                                    x2: pos2.x, y2: pos2.y, n2: pos2.roomName,
                                    s: style || {}});
                                return this;
                            }
                        },
                        rect: {
                            value: function(pos, w, h, style) {
                                if(!(pos instanceof RoomPosition)) {
                                    throw new Error('Invalid pos, RoomPosition expected')
                                }
                                globals.console.addVisual("map", {
                                    t: 'r',
                                    x: pos.x, y: pos.y, n: pos.roomName,
                                    w, h,
                                    s: style || {}});
                                return this;
                            }
                        },
                        poly: {
                            value: function(points, style) {
                                if(_.isArray(points) && _.some(points)) {
                                    points = points.map(i => {
                                        const p = i.pos || i;
                                        return {x: p.x, y: p.y, n: p.roomName};
                                    });
                                    globals.console.addVisual("map", {
                                        t: 'p',
                                        points,
                                        s: style || {}});
                                }
                                return this;
                            }
                        },
                        text: {
                            value: function(text, pos, style) {
                                if(!(pos instanceof RoomPosition)) {
                                    throw new Error('Invalid pos , RoomPosition expected')
                                }
                                globals.console.addVisual("map", {
                                    t: 't',
                                    text,
                                    x: pos.x, y: pos.y, n: pos.roomName,
                                    s: style || {}});
                                return this;
                            }
                        },
                        clear: {
                            value: function () {
                                globals.console.clearVisual("map");
                                return this;
                            }
                        },
                        getSize: {
                            value: function() {
                                return globals.console.getVisualSize("map");
                            }
                        },
                        export: {
                            value: function(){
                                return globals.console.getVisual("map");
                            }
                        },
                        import: {
                            value: function(data){
                                globals.console.addVisual("map", "" + data);
                                return this;
                            }
                        }
                    });
                }
                return _visual;
            }
        }
    });

    return map;
};
