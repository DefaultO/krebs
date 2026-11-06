# 09 — Pathfinding & Terrain

Status: implementation spec for chitin (1:1 port target).
Reference revisions: engine @8097782, driver @cf63d8a (native PathFinder v11), common @2fb779b, backend 3.3.0, `@screeps/pathfinding` 0.4.17, docs (screeps/docs), xxscreeps @d1e3bad.
Every rule cites `reference/<pkg>/<path>:<line>`. Where the docs and the code disagree, **the code wins** (bots were tuned against the code).

Conventions used below:

* **Local coords**: `x, y` in `0..49` inside one room.
* **Terrain index** (DB string, runtime buffer, `getRawBuffer`, `Room.Terrain.get`) is **row-major** `y*50 + x`.
* **CostMatrix index** and the **native PathFinder index** are **column-major** `x*50 + y`. **GOTCHA**: the two layouts are transposed; mixing them up produces paths that look plausible and are wrong.
* **Engine room XY** (`utils.roomNameToXY`): `E0S0 = (0,0)`, `W0N0 = (-1,-1)`, `En → n`, `Wn → -n-1`, `Sn → n`, `Nn → -n-1`.
* **PF map coords** (`rx, ry`, uint8): `rx = engineX + 128`, so `W127 → 0`, `W0 → 127`, `E0 → 128`, `E127 → 255`.
* **PF world coords** (`wx, wy`, uint32): `wx = rx*50 + x`, `wy = ry*50 + y`. These are continuous across room borders (x=49 of one room and x=0 of the next room are 1 apart).
* Directions: `TOP=1, TOP_RIGHT=2, RIGHT=3, BOTTOM_RIGHT=4, BOTTOM=5, BOTTOM_LEFT=6, LEFT=7, TOP_LEFT=8` (reference/common/lib/constants.js:50-57). `FIND_EXIT_TOP=1, _RIGHT=3, _BOTTOM=5, _LEFT=7, FIND_EXIT=10` (reference/common/lib/constants.js:21-25). `ERR_NO_PATH=-2`, `ERR_INVALID_ARGS=-10` (reference/common/lib/constants.js:4,13). `TERRAIN_MASK_WALL=1, TERRAIN_MASK_SWAMP=2, TERRAIN_MASK_LAVA=4` (reference/common/lib/constants.js:291-293). `OBSTACLE_OBJECT_TYPES = [spawn, creep, powerCreep, source, mineral, deposit, controller, constructedWall, extension, link, storage, tower, observer, powerSpawn, powerBank, lab, terminal, nuker, factory, invaderCore]` (reference/common/lib/constants.js:85).
* `OBST` = native `obstacle` = `u32::MAX` (reference/driver/native/src/pf.h:342).

---

## 1. Inventory

Scope column: **G** = global/static (shared read-only world data), **X** = cross-room per call, **R** = room-local, **P** = per-player per-tick cache.

| ID | Item | Entry point | Primary source | Scope |
|---|---|---|---|---|
| TERRAIN-CODES | Terrain code values 0..3 and how each consumer interprets `3` | — | reference/common/lib/constants.js:291-293 | G |
| TERRAIN-DB-FORMAT | `rooms.terrain` document string encoding | DB | reference/common/index.js:25-58 | G |
| TERRAIN-ENV-BLOB | `env.terrainData` compressed world blob + `map.updateTerrainData()` (synthetic walled rooms) | CLI / driver | reference/backend/lib/cli/map.js:660-695; reference/driver/lib/index.js:80-87 | G |
| TERRAIN-RUNTIME-BUFFER | Per-process unpacked terrain buffer shared into user isolates | runner | reference/driver/lib/runtime/make.js:20-51; reference/driver/lib/runtime/runtime.js:45-49 | G |
| TERRAIN-PF-PACKED | Native 2-bit packed, column-major terrain (625 B/room) + `loadTerrain` | driver | reference/driver/lib/path-finder.js:23-46; reference/driver/native/src/pf.cc:605-613 | G |
| TERRAIN-LIFECYCLE | When terrain/map data is loaded; no hot reload | driver | reference/driver/lib/runtime/make.js:20-27; reference/driver/lib/runtime/runtime.js:206-208 | G |
| TERRAIN-CTOR | `new Room.Terrain(roomName)` | API | reference/engine/src/game/rooms.js:1223-1228 | G |
| TERRAIN-GET | `Room.Terrain#get(x,y)` | API | reference/engine/src/game/rooms.js:1230-1233 | G |
| TERRAIN-GETRAWBUFFER | `Room.Terrain#getRawBuffer([dest])` | API | reference/engine/src/game/rooms.js:1235-1241, 389-399 | G |
| TERRAIN-ROOM-GETTERRAIN | `Room#getTerrain()`, `Game.map.getRoomTerrain()` | API | reference/engine/src/game/rooms.js:1134-1136; reference/engine/src/game/map.js:261-263 | G |
| TERRAIN-GETTERRAINAT | `Game.map.getTerrainAt` (deprecated) | API | reference/engine/src/game/map.js:235-259 | G |
| TERRAIN-POS-LOOKFOR | `RoomPosition#lookFor('terrain')` | API | reference/engine/src/game/rooms.js:1602-1614 | G |
| TERRAIN-ROOM-LOOK | terrain entries in `lookAt/lookForAt/lookAtArea/lookForAtArea` | API | reference/engine/src/game/rooms.js:663-682, 707-766 | G |
| TERRAIN-CHECK | `utils.checkTerrain` / `common.checkTerrain` (processor side) | engine | reference/engine/src/utils.js:333-336; reference/common/index.js:60-62 | R |
| TERRAIN-ENCODE-DECODE | `encodeTerrain / decodeTerrain / decodeTerrainByRoom` | engine/common | reference/engine/src/utils.js:262-331; reference/common/index.js:25-58 | G |
| TERRAIN-FIND-EXIT | `Room#find(FIND_EXIT*)` | API | reference/engine/src/game/rooms.js:592-645 | R |
| TERRAIN-EXIT-GRID | `WorldMapGrid.gridData` exit counts (feeds `describeExits`) | runtime | reference/driver/lib/runtime/mapgrid.js:68-121 | G |
| COORD-ROOMNAME-TO-XY | `utils.roomNameToXY` (lenient) | engine | reference/engine/src/utils.js:374-392 | — |
| COORD-XY-TO-ROOMNAME | `utils.getRoomNameFromXY` + cached override (`'sim'` quirk) | engine | reference/engine/src/utils.js:358-372; reference/engine/src/game/rooms.js:1260-1274 | — |
| COORD-PF-PARSE | PF `parseRoomName` / `generateRoomName` (kWorldSize 255) | driver | reference/driver/lib/path-finder.js:9-21, 51-56 | — |
| COORD-PF-WORLDPOS | `toWorldPosition` / `fromWorldPosition` | driver | reference/driver/lib/path-finder.js:59-80 | — |
| COORD-MAPGRID-PARSE | mapgrid's own `roomNameToXY` | runtime | reference/driver/lib/runtime/mapgrid.js:5-30 | — |
| COORD-ROOMPOS-PACK | `RoomPosition.__packedPos` (world-size limit) | API | reference/engine/src/game/rooms.js:1276-1348 | — |
| COORD-FETCHXY | `utils.fetchXYArguments` | engine | reference/engine/src/utils.js:34-63 | — |
| COORD-GETDIRECTION | `utils.getDirection(dx,dy)` | engine | reference/engine/src/utils.js:65-99 | — |
| COORD-POS-DIRECTIONTO | `RoomPosition#getDirectionTo` (cross-room) | API | reference/engine/src/game/rooms.js:1383-1394 | — |
| COORD-OFFSETS | `offsetsByDirection` table | engine | reference/engine/src/utils.js:3, 101-112 | — |
| COORD-LINEAR-DISTANCE | `Game.map.getRoomLinearDistance` | API | reference/engine/src/utils.js:644-655; reference/engine/src/game/map.js:265-267 | — |
| PATHSER-STEP | Path step object `{x,y,dx,dy,direction}` | engine | reference/engine/src/game/rooms.js:284-290, 944-956 | — |
| PATHSER-SERIALIZE | `utils.serializePath` | engine | reference/engine/src/utils.js:555-574 | — |
| PATHSER-DESERIALIZE | `utils.deserializePath` | engine | reference/engine/src/utils.js:576-613 | — |
| PATHSER-STATIC | `Room.serializePath / Room.deserializePath` | API | reference/engine/src/game/rooms.js:528-534 | — |
| CM-LAYOUT | CostMatrix `_bits` Uint8Array(2500), `x*50+y` | API | reference/engine/src/game/path-finder.js:18-20 | — |
| CM-SET | `CostMatrix#set` | API | reference/engine/src/game/path-finder.js:22-26 | — |
| CM-GET | `CostMatrix#get` | API | reference/engine/src/game/path-finder.js:28-32 | — |
| CM-CLONE | `CostMatrix#clone` | API | reference/engine/src/game/path-finder.js:34-38 | — |
| CM-SERIALIZE | `CostMatrix#serialize` | API | reference/engine/src/game/path-finder.js:40-42 | — |
| CM-DESERIALIZE | `CostMatrix.deserialize` | API | reference/engine/src/game/path-finder.js:44-48 | — |
| CM-TO-NATIVE | How a matrix reaches the native search (`_bits`, length 2500) | driver | reference/driver/lib/path-finder.js:120-128; reference/driver/native/src/pf.cc:47-58 | X |
| CM-VALUE-SEMANTICS | 0 = terrain, 1..254 = override, 255 = blocked | native | reference/driver/native/src/pf.cc:106-123 | X |
| PF-API-SEARCH | `PathFinder.search` guard for empty goal | API | reference/engine/src/game/path-finder.js:57-65 | X |
| PF-API-OPTIONS | option normalisation/clamping | driver | reference/driver/lib/path-finder.js:89-96 | X |
| PF-API-GOALS | goal/origin normalisation | driver | reference/driver/lib/path-finder.js:99-112, 134 | X |
| PF-API-ROOMCALLBACK | roomCallback wrapper (`false` / `_bits` / undefined) | driver | reference/driver/lib/path-finder.js:115-131 | X |
| PF-API-RESULT | result mapping (undefined / -1 / object), path reversal | driver | reference/driver/lib/path-finder.js:134-141 | X |
| PF-API-USE | `PathFinder.use` / `_useNewPathFinder` (stale-register quirk) | API | reference/engine/src/game/path-finder.js:8-14, 67-75; reference/engine/src/game/game.js:67-68 | P |
| PF-API-INIT | driver init, binary version 11, `loadTerrain` | driver | reference/driver/lib/path-finder.js:23-46; reference/driver/native/src/main.cc:53-57 | G |
| PF-API-INSTANCES | 2 thread-local finders, re-entrancy | native | reference/driver/native/src/main.cc:9-28 | — |
| PF-CORE-TYPES | world/map position, index, constants | native | reference/driver/native/src/pf.h:11-170 | X |
| PF-CORE-ROOMTABLE | lazy room loading, maxRooms, blocked rooms, callback order | native | reference/driver/native/src/pf.cc:22-64 | X |
| PF-CORE-INDEX | `index_from_pos / pos_from_index` | native | reference/driver/native/src/pf.cc:67-80 | X |
| PF-CORE-LOOK | per-tile cost lookup order | native | reference/driver/native/src/pf.cc:106-123; reference/driver/native/src/pf.h:228-234 | X |
| PF-CORE-HEURISTIC | non-flee / flee heuristic | native | reference/driver/native/src/pf.cc:126-148 | X |
| PF-CORE-HEAP | binary heap with exact tie-breaking | native | reference/driver/native/src/pf.h:251-335 | X |
| PF-CORE-OPENCLOSED | marker-based open/closed set | native | reference/driver/native/src/pf.h:174-209 | X |
| PF-CORE-PUSH | `push_node` (insert / decrease-key) | native | reference/driver/native/src/pf.cc:83-103 | X |
| PF-CORE-SEARCH | main loop, ops, maxCost, termination | native | reference/driver/native/src/pf.cc:456-567 | X |
| PF-CORE-ORIGIN-ASTAR | 8-neighbour expansion of the origin + border move rules | native | reference/driver/native/src/pf.cc:151-190 | X |
| PF-CORE-JPS | JPS expansion of popped nodes | native | reference/driver/native/src/pf.cc:304-437 | X |
| PF-CORE-JPS-BORDER | border-tile special neighbour sets | native | reference/driver/native/src/pf.cc:309-364 | X |
| PF-CORE-JUMP-NEIGHBOR | `jump_neighbor` cost accounting | native | reference/driver/native/src/pf.cc:439-454 | X |
| PF-CORE-JUMP | `jump_x / jump_y / jump_xy / jump` | native | reference/driver/native/src/pf.cc:193-302 | X |
| PF-CORE-RESULT | path reconstruction, `ops`, `cost`, `incomplete` | native | reference/driver/native/src/pf.cc:569-601 | X |
| PF-CORE-SPECIAL-RETURNS | `undefined` (already at goal), `-1` (origin room blocked), termination | native | reference/driver/native/src/pf.cc:506-519, 557-566 | X |
| PF-CORE-ERRORS | missing terrain, JS exceptions in callback, heap overflow | native | reference/driver/native/src/pf.cc:31-46, 563-567; reference/driver/native/src/pf.h:301-304 | X |
| PF-CORE-CONFORMANCE | golden checksum test `profile.js` | native | reference/driver/native/profile.js:76-105 | — |
| ROOM-FINDPATH | `Room#findPath` dispatch | API | reference/engine/src/game/rooms.js:863-871 | X |
| ROOM-FINDPATH-NEW | `_findPath2` (PathFinder-backed findPath) | engine | reference/engine/src/game/rooms.js:223-302 | X |
| ROOM-DEFAULT-MATRIX | `makePathfindingGrid2 / getPathfindingGrid2` default CostMatrix + per-tick cache | engine | reference/engine/src/game/rooms.js:157-221 | P |
| ROOM-FINDCLOSESTBYPATH-NEW | `_findClosestByPath2` | engine | reference/engine/src/game/rooms.js:304-374 | R |
| POS-FINDPATHTO | `RoomPosition#findPathTo` | API | reference/engine/src/game/rooms.js:1396-1427 | X |
| POS-FINDCLOSESTBYPATH | `RoomPosition#findClosestByPath` dispatch | API | reference/engine/src/game/rooms.js:1429-1445 | R |
| ROOM-FINDEXITTO | `Room#findExitTo` | API | reference/engine/src/game/rooms.js:1130-1132 | G |
| LEGACY-REACHABILITY | when the legacy (`@screeps/pathfinding`) path can actually run | engine | reference/engine/src/game/path-finder.js:67-75 | P |
| LEGACY-GRID | `makePathfindingGrid / getPathfindingGrid` weights + cache | engine | reference/engine/src/game/rooms.js:32-155 | P |
| LEGACY-POSSET-CACHE | `positionsSetCache.key` | engine | reference/engine/src/game/rooms.js:445-484 | P |
| LEGACY-FINDER-CACHE | `getPathfinder` AStarFinder cache | engine | reference/engine/src/game/rooms.js:16-30 | P |
| LEGACY-FINDPATH | legacy branch of `Room#findPath` + `pathCache` | engine | reference/engine/src/game/rooms.js:873-968 | R |
| LEGACY-ASTAR | `AStarFinder.findPath` (weights, 1.00000001 diagonal, closestNode) | lib | reference/pathfinding/src/finders/AStarFinder.js:19-164 | R |
| LEGACY-DIJKSTRA | `DijkstraFinder` (h=0, end nodes weight 999) | lib | reference/pathfinding/src/finders/DijkstraFinder.js:1-22 | R |
| LEGACY-GRID-NEIGHBORS | `Grid` walkability, neighbour order, `setWalkableAt` | lib | reference/pathfinding/src/core/Grid.js:39-239 | R |
| LEGACY-HEAP | `heap@0.2.5` (CPython-heapq port) `push` / `pop` tie-breaking | lib | E:\screeps\server\node_modules\heap\lib\heap.js:62-89, 231-270; reference/pathfinding/package.json:18-20 | R |
| LEGACY-HEAP-UPDATE | `heap@0.2.5` `updateItem` (siftdown + bottom-up siftup that permutes equal-f nodes) | lib | E:\screeps\server\node_modules\heap\lib\heap.js:160-171 | R |
| LEGACY-ENDNODES | `Room#getEndNodes` | engine | reference/engine/src/game/rooms.js:1099-1128 | R |
| LEGACY-FINDCLOSESTBYPATH | legacy `findClosestByPath` (astar/dijkstra choice) | engine | reference/engine/src/game/rooms.js:1447-1515 | R |
| LEGACY-FINDPATHTO-XROOM | legacy cross-room `findPathTo` via exits | engine | reference/engine/src/game/rooms.js:1415-1425 | R |
| LEGACY-UNUSED | other finders/heuristics in the package (unused by the game) | lib | reference/pathfinding/src/PathFinding.js:1-17 | — |
| MAP-DESCRIBEEXITS | `Game.map.describeExits` | API | reference/engine/src/game/map.js:20-46 | G |
| MAP-FINDROUTE | `Game.map.findRoute` A* over rooms | API | reference/engine/src/game/map.js:69-186 | G |
| MAP-ROUTE-HEAP | `path-utils` Heap/OpenClosed (JS) | engine | reference/engine/src/game/path-utils.js:5-122 | — |
| MAP-ROUTE-WINDOW | 60×60 room window (`kRouteGrid=30`) | engine | reference/engine/src/game/map.js:9, 48-59 | — |
| MAP-ROUTE-CALLBACK | `routeCallback` cost semantics | API | reference/engine/src/game/map.js:107, 164-167 | — |
| MAP-FINDEXIT | `Game.map.findExit` | API | reference/engine/src/game/map.js:188-197 | G |
| MAP-WORLDMAPGRID | `WorldMapGrid` build (once per VM, accessible rooms only) | runtime | reference/driver/lib/runtime/mapgrid.js:40-57; reference/driver/lib/index.js:490-496 | G |
| NPC-PF-SETUP | processor-side PathFinder (fake RoomPosition) | processor | reference/engine/src/processor.js:174; reference/driver/lib/index.js:113-116 | R |
| NPC-PF-COSTMATRIX | `fakeRuntime.defaultCostMatrix` | processor | reference/engine/src/processor/common/fake-runtime.js:104-141 | R |
| NPC-PF-FINDPATH | `fakeRuntime.findPath` | processor | reference/engine/src/processor/common/fake-runtime.js:143-182 | R |
| NPC-PF-FINDCLOSESTBYPATH | `fakeRuntime.findClosestByPath` | processor | reference/engine/src/processor/common/fake-runtime.js:265-309 | R |
| NPC-PF-MOVE | `fakeRuntime.moveTo/walkTo/flee` + packed local paths | processor | reference/engine/src/processor/common/fake-runtime.js:54-77, 185-263, 311-332 | R |
| NPC-PF-CM | fake `CostMatrix` | processor | reference/engine/src/processor/common/fake-runtime.js:79-99 | R |
| XREF-CREEP-MOVE | `Creep#moveTo / moveByPath` (path caching in Memory) — owned by the creep spec | API | reference/engine/src/game/creeps.js:180-333 | X |
| XREF-XXSCREEPS | xxscreeps pathfinder/terrain: where it is NOT 1:1 | — | reference/xxscreeps/packages/pathfinder/src/pf.cc:24-55; reference/xxscreeps/packages/xxscreeps/game/terrain.ts:52-83 | — |

---

## 2. Terrain

### TERRAIN-CODES
A terrain code is a small integer: bit 0 (`1`) = wall, bit 1 (`2`) = swamp. `0` = plain. **GOTCHA**: code `3` (wall+swamp) exists in real data — the shipped seed DB contains thousands of `'3'` tiles (reference/launcher/init_dist/db.json:1). `TERRAIN_MASK_LAVA = 4` is defined (reference/common/lib/constants.js:293) but no code path in this area reads it.

How each consumer treats code 3 (port each exactly):

| Consumer | Result for 3 | Source |
|---|---|---|
| native PathFinder | `look_table[3] = OBST` (impassable) | reference/driver/native/src/pf.h:351; reference/driver/native/src/pf.cc:495-496 |
| `Room.Terrain#get` | `1` (wall) | reference/engine/src/game/rooms.js:1232 |
| `getRawBuffer` | raw `3` | reference/engine/src/game/rooms.js:1240 |
| `lookFor/lookAt/getTerrainAt` | `'wall'` (wall wins) | reference/engine/src/game/rooms.js:666-671, 1605-1607; reference/engine/src/game/map.js:252-257 |
| `checkTerrain(mask)` | true for WALL **and** SWAMP (processor e.g. road decay applies both ratios) | reference/engine/src/utils.js:333-336; reference/engine/src/processor/intents/roads/tick.js:12-19 |
| legacy grid | 0 (unwalkable) | reference/engine/src/game/rooms.js:52-57 |
| `FIND_EXIT_*` | not an exit | reference/engine/src/game/rooms.js:621 |
| `WorldMapGrid` exits | not an exit (only `== 0` counts) | reference/driver/lib/runtime/mapgrid.js:108 |
| fake-runtime `lookFor` | `'wall'` | reference/engine/src/processor/common/fake-runtime.js:50-51 |

### TERRAIN-DB-FORMAT
`rooms.terrain` documents: `{room: "W10N10", terrain: "<2500 chars '0'..'3'>", type: "terrain"}` (reference/launcher/init_dist/db.json:1). Character index is `y*50+x` (reference/common/index.js:43-58). Indexed on `room` (reference/storage/lib/db.js:355-357).

### TERRAIN-ENV-BLOB
* `env` key `terrainData` (reference/common/lib/storage.js:40) holds `base64( zlib.deflate( JSON.stringify(terrainDocs[]) ) )` (reference/backend/lib/cli/map.js:691-693). Read with base64 → inflate → `JSON.parse` (reference/driver/lib/index.js:80-87).
* Built by CLI `map.updateTerrainData()` (reference/backend/lib/cli/map.js:660-695):
  1. rooms whose `status == 'out of borders'` get their terrain replaced by 2500 × `'1'` **in the blob only** (DB untouched).
  2. for every room `XnYm`, if missing, append all-wall rooms `X(n+1)Ym` and `XnY(m+1)` (the outward neighbours: W5→W6, E5→E6, N5→N6, S5→S6).
* **GOTCHA**: runtime terrain (`Room.Terrain`, PathFinder) comes from the blob; processor intents use the DB string (reference/engine/src/processor.js:33-35). For out-of-borders rooms they differ.
* **GOTCHA**: the synthetic walled rooms exist for terrain purposes (`new Room.Terrain('W11N10')` works for them), and they prevent the native search from throwing "Could not load terrain data" at the map edge.

### TERRAIN-RUNTIME-BUFFER
Runner process (reference/driver/lib/runtime/make.js:20-51): once per process, decode the blob, call `pathfinderFactory.init(native, rooms)` (loads PF terrain), allocate `(Shared)ArrayBuffer(rooms.length*2500)`, write `view[i + roomIndex*2500] = Number(terrain.charAt(i))` (row-major, raw 0..3), `roomOffsets[room] = roomIndex*2500`. Copied into each user isolate (reference/driver/lib/runtime/user-vm.js:80-85) and wrapped as `staticTerrainData[room] = new Uint8Array(buffer, offset, 2500)` (reference/driver/lib/runtime/runtime.js:45-49). The isolate memory limit is raised by the terrain size (reference/driver/lib/runtime/user-vm.js:30).

**PERF**: one immutable copy per world; chitin should keep a single `Arc` store shared by all player threads (unpacked 2500 B/room and/or packed 625 B/room).

### TERRAIN-PF-PACKED
`init(mod, rooms)` (reference/driver/lib/path-finder.js:23-46): for each room, `ii = x*50 + y` (column-major), `bit = Number(terrain[y*50 + x])`, `pack[ii>>2] = pack[ii>>2] & ~(3 << (ii%4)*2) | bit << (ii%4)*2`. Then version check `mod.version !== 11 → throw Error('Invalid pathfinder binary')` and `mod.loadTerrain([{room:{xx,yy}, bits}])`. Native stores one pointer per map position in a process-global table of 65536 entries (reference/driver/native/src/pf.h:359; reference/driver/native/src/pf.cc:605-613). Read back as `0x03 & terrain[i/4] >> (i%4*2)` with `i = x*50+y` (reference/driver/native/src/pf.h:228-234).

### TERRAIN-LIFECYCLE
* Runner loads terrain once per process and never reloads (reference/driver/lib/runtime/make.js:20-27). Processor loads PF terrain at startup (reference/driver/lib/index.js:113-116).
* `WorldMapGrid` (exits) is built once per user VM from the `accessibleRooms` snapshot of its first tick (reference/driver/lib/runtime/runtime.js:206-208).
* **PERF**: terrain is immutable during a run → lock-free sharing. Map edits require restart in the original.

### TERRAIN-CTOR — `new Room.Terrain(roomName)`
reference/engine/src/game/rooms.js:1223-1228. `roomName = "" + roomName`; `array = (runtimeData.staticTerrainData || {})[roomName]`; falsy → `throw new Error("Could not access room " + roomName)`. Works for any room in the blob (visibility irrelevant). **GOTCHA**: `get` and `getRawBuffer` are **own enumerable properties** (closures created per instance), not prototype methods (rooms.js:1230, 1235).

### TERRAIN-GET — `terrain.get(x, y)`
reference/engine/src/game/rooms.js:1230-1233. `value = array[y*50 + x]` with no coercion or bounds check; return `(value & 1) || (value & 2) || 0`.
* Out-of-range / non-integer index → `undefined` → `0`.
* **GOTCHA**: `x >= 50` wraps into the next row (`get(50,0) == get(0,1)`); string args concatenate (`0*50 + "1"` = `"01"`, not an index → `0`).
* Code 3 → `1`.

### TERRAIN-GETRAWBUFFER — `terrain.getRawBuffer([dest])`
reference/engine/src/game/rooms.js:1235-1241. `TerrainConstructor` / `TerrainConstructorSet` are captured once from the first runtime terrain array: `Uint8Array` and `%TypedArray%.prototype.set` (rooms.js:389-399).
* `dest` falsy → `new Uint8Array(array)`: a fresh 2500-byte copy, row-major, raw 0..3.
* `dest` truthy → `set.call(dest, array)`, return `dest`. Non-TypedArray → **TypeError thrown**. The docs promise `ERR_INVALID_ARGS` (reference/docs/api/source/Room.Terrain.md:157), but no code path returns it. **GOTCHA**. `dest.length < 2500` → RangeError. Other typed-array element types convert numerically.

### TERRAIN-ROOM-GETTERRAIN
`Room#getTerrain()` returns `new Room.Terrain(this.name)` (reference/engine/src/game/rooms.js:1134-1136). `Game.map.getRoomTerrain(name)` returns `new Room.Terrain(name)` (reference/engine/src/game/map.js:261-263).

### TERRAIN-GETTERRAINAT — `Game.map.getTerrainAt(x, y, roomName | pos)`
reference/engine/src/game/map.js:235-259. Logs the deprecation message once per tick per message through `console.log` (reference/engine/src/game/game.js:113-120). Object first arg → `x=a.x, y=a.y, roomName=a.roomName`. If `x<0||x>49||y<0||y>49` → `undefined`. Missing room data → `undefined`. Wall bit → `'wall'`, else swamp bit → `'swamp'`, else `'plain'`. A non-integer coordinate gives an index miss and returns `'plain'`.

### TERRAIN-POS-LOOKFOR — `pos.lookFor('terrain')`
reference/engine/src/game/rooms.js:1602-1614: `['wall']` / `['swamp']` / `['plain']` from `staticTerrainData[roomName][y*50+x]`. The room's existence is not checked; missing terrain → TypeError. Other look types require a visible room.

### TERRAIN-ROOM-LOOK
`_lookSpatialRegister(id,'terrain',x,y,out,withCoords)` (reference/engine/src/game/rooms.js:663-682): start with `'plain'`, set `'swamp'` if the swamp bit is set, then `'wall'` if the wall bit is set. Push `{type:'terrain', terrain}` (with `x,y` when `withCoords`), or return `[result]`. **GOTCHA**: the bounds check (`throw 'look coords are out of bounds'`) comes after the terrain branch (rooms.js:684-686), so terrain looks never throw on bad coordinates and wrap like TERRAIN-GET. Terrain is the 10th entry in `lookAt` order (rooms.js:772-785). Area lookups always use the spatial branch for terrain (rooms.js:711, 748-764). The full look API is owned by the room spec.

### TERRAIN-CHECK
`utils.checkTerrain(terrain, x, y, mask)`: if `terrain instanceof Uint8Array`, read `terrain[y*50+x]`, else `Number(terrain.charAt(y*50+x))`; return `(code & mask) > 0` (reference/engine/src/utils.js:333-336). `common.checkTerrain` uses `parseInt` (reference/common/index.js:60-62). In the processor `roomTerrain` is the DB string (reference/engine/src/processor.js:33-35).

### TERRAIN-ENCODE-DECODE
`encodeTerrain(list)`: for each `y`, then `x`, compute `code = (any wall obj at x,y ? 1 : 0) | (any swamp obj ? 2 : 0)` and append the digit (reference/engine/src/utils.js:262-278; reference/common/index.js:25-41). `decodeTerrain` emits `{room,x,y,type:'wall'}` then `{…'swamp'}` per tile (reference/common/index.js:43-58; engine variant takes doc arrays, reference/engine/src/utils.js:280-302). `decodeTerrainByRoom` → `spatial[room][y][x] = char` (reference/engine/src/utils.js:304-331). **PERF**: encode is O(2500·n) via `_.filter`; offline only.

### TERRAIN-FIND-EXIT — `room.find(FIND_EXIT_*)`
reference/engine/src/game/rooms.js:592-645.
* `FIND_EXIT_TOP`: `(i,0)`; `_BOTTOM`: `(i,49)`; `_LEFT`: `(0,i)`; `_RIGHT`: `(49,i)` for `i=0..49` ascending, where `!(terrain & WALL)` (swamp tiles count; corners included if not walls). Results are `RoomPosition`s cached per tick in `register.findCache[type][room]`.
* `FIND_EXIT` = concat of **TOP, BOTTOM, RIGHT, LEFT** in that order (rooms.js:594-597). **GOTCHA**: this is not numeric order.
* **GOTCHA (bug, keep)**: `FIND_EXIT` builds its cache by calling the sub-finds with the caller's `opts` (so with `opts.filter` applied) and returns `_.clone(cache)` immediately without applying `filter` again. If the first `FIND_EXIT` call in a tick has a filter, the filtered list is cached for the rest of the tick.
* Other find types: `opts.filter` → `_.filter(result)`, else `_.clone(result)` (rooms.js:648-653).

### TERRAIN-EXIT-GRID
`WorldMapGrid._buildGridData(accessibleRooms, staticTerrainData)` (reference/driver/lib/runtime/mapgrid.js:68-121): for each accessible room, scan the 50 tiles of each side — `t`: (0..49, 0); `r`: (49, 0..49); `b`: (0..49, 49); `l`: (0, 0..49) — and count `terrain == 0`. Store a side only if its count > 0. Key `"${x},${y}"` (engine XY). **GOTCHA**: only **plain** tiles count; a side whose passable tiles are all swamp counts as having no exit for `describeExits`/`findRoute`, while `FIND_EXIT` lists them.

---

## 3. Coordinates and room names

### COORD-ROOMNAME-TO-XY — `utils.roomNameToXY(name)`
reference/engine/src/utils.js:374-392. No validation:
```
xx = parseInt(name.substr(1), 10)
vp = xx >= 100 ? 4 : xx >= 10 ? 3 : 2
yy = parseInt(name.substr(vp + 1), 10)
if name[0] in {'W','w'}: xx = -xx - 1
if name[vp] in {'N','n'}: yy = -yy - 1
return [xx, yy]
```
`E1N1 → [1,-2]`, `W0N0 → [-1,-1]`, `E0S0 → [0,0]`. Garbage input gives NaN or garbage. It never throws on a string.

### COORD-XY-TO-ROOMNAME
Base: `x<0 ? 'W'+(-x-1) : 'E'+x`, `y<0 ? 'N'+(-y-1) : 'S'+y` (reference/engine/src/utils.js:358-372). `makePos` replaces it on the shared `utils` module with a memoised version keyed `(xx+128)<<8 | (yy+128)` and pre-seeds `roomNames[0] = 'sim'` (reference/engine/src/game/rooms.js:1260-1274). **GOTCHA**: `(-128,-128)` (= `W127N127`) maps to `'sim'`. So `RoomPosition(…,'W127N127').roomName === 'sim'` (the getter reads the same table, rooms.js:1325), and `findRoute` and `describeExits` produce `'sim'` for it.

### COORD-PF-PARSE (driver)
reference/driver/lib/path-finder.js:9-21, 51-56. `kWorldSize = 255`. The regex `^([WE])([0-9]+)([NS])([0-9]+)$` is uppercase-only. `rx = 127 + (W ? -n : n+1)` and `ry` likewise (N negative). Throw `'Invalid room name'` unless `0 <= rx,ry <= 255`. Inverse: `xx <= 127 ? 'W'+(127-xx) : 'E'+(xx-128)`, same for N/S.

### COORD-PF-WORLDPOS
`toWorldPosition(rp)`: `x = rp.x|0, y = rp.y|0`; outside 0..49 → throw `'Invalid room position'`; `{xx: x + rx*50, yy: y + ry*50}` (reference/driver/lib/path-finder.js:59-70). `fromWorldPosition([wx,wy])` = `new globals.RoomPosition(wx%50, wy%50, generateRoomName(floor(wx/50), floor(wy/50)))` (reference/driver/lib/path-finder.js:74-80). `globals` is whatever was last passed to `make()` (reference/driver/lib/path-finder.js:82-84): the user isolate's globals (reference/engine/src/game/path-finder.js:10) or the processor's fake `RoomPosition` (reference/engine/src/processor.js:174).

### COORD-MAPGRID-PARSE
mapgrid's private parser upper-cases the name and matches `^(\w)(\d+)(\w)(\d+)$`. Any first letter other than `W` counts as east and any other than `N` counts as south (reference/driver/lib/runtime/mapgrid.js:5-30). It agrees with `utils.roomNameToXY` for valid names.

### COORD-ROOMPOS-PACK
`__packedPos = (rx+128)<<24 | (ry+128)<<16 | x<<8 | y` (reference/engine/src/game/rooms.js:1276-1293), so room coordinates must fit in 8 bits. **PERF/limit**: the API layer supports at most 256×256 rooms (65 536 rooms), and the native PF has the same limit (`map_position_t` is `uint8` per axis, reference/driver/native/src/pf.h:26). Owned by the RoomPosition spec. Listed here because it caps "larger worlds".

### COORD-FETCHXY — `utils.fetchXYArguments(a, b)`
reference/engine/src/utils.js:34-63. If `b` is a number → `(a, b, undefined)`. Otherwise `a` must be an object: if `a instanceof RoomPosition` take its x,y,roomName; if `a.pos instanceof RoomPosition` take those (overrides). NaN → `undefined`. **GOTCHA**: a plain `{x,y,roomName}` object yields `[undefined, undefined, undefined]`.

### COORD-GETDIRECTION — `utils.getDirection(dx, dy)`
reference/engine/src/utils.js:65-99:
```
if |dx| > 2|dy|: dx>0 ? RIGHT : LEFT
elif |dy| > 2|dx|: dy>0 ? BOTTOM : TOP
else: BR if dx>0&&dy>0, TR if dx>0&&dy<0, BL if dx<0&&dy>0, TL if dx<0&&dy<0, else undefined
```
`(0,0)` → `undefined`. `(2,1)` → `BOTTOM_RIGHT`; `(3,1)` → `RIGHT`.

### COORD-POS-DIRECTIONTO
reference/engine/src/game/rooms.js:1383-1394: same room (or no room given) → `getDirection(x - this.x, y - this.y)`. Cross-room: offsets from `roomNameToXY(...) * 50`.

### COORD-OFFSETS
`offsetsByDirection = [ , [0,-1],[1,-1],[1,0],[1,1],[0,1],[-1,1],[-1,0],[-1,-1] ]`; index 0 is a hole (reference/engine/src/utils.js:3).

### COORD-LINEAR-DISTANCE
`Game.map.getRoomLinearDistance(a, b, continuous)` returns `max(|dx|, |dy|)` of the `roomNameToXY` coordinates. If `continuous`, each axis becomes `min(worldSize - d, d)` (reference/engine/src/utils.js:644-655), where `worldSize = max(spanX, spanY)` over all rooms in the DB (reference/common/index.js:183-193).

---

## 4. Path serialisation

### PATHSER-STEP
Step objects are `{x, y, dx, dy, direction}`: `x,y` is the tile entered, `dx,dy` the delta from the previous tile (the first step is relative to the start position), and `direction = getDirection(dx,dy)` (reference/engine/src/game/rooms.js:284-290).

### PATHSER-SERIALIZE
reference/engine/src/utils.js:555-574:
1. Not an array → throw `'path is not an array'`.
2. Empty → `''`.
3. `path[0].x < 0 || path[0].y < 0` → throw `'path coordinates cannot be negative'`.
4. `s = pad2(path[0].x) + pad2(path[0].y)`, where `pad2(v) = v > 9 ? v : '0' + v` (string concat).
5. For **every** step including the first, append `path[i].direction` (string coercion: `undefined` → `"undefined"`, **GOTCHA**).

Example: from (10,10), steps (11,10,R) and (12,11,BR) → `"111034"`.

### PATHSER-DESERIALIZE
reference/engine/src/utils.js:576-613:
1. Non-string → throw `` '`path` is not a string' ``. Empty → `[]`.
2. `x = parseInt(s.substring(0,2))`, `y = parseInt(s.substring(2,4))` (lenient). NaN → throw `` '`path` is not a valid serialized path string' ``.
3. For `i = 4 .. len-1`: `d = parseInt(s[i])`; if `!offsets[d]` (0, 9, non-digit) → same throw. For `i > 4` apply `x += dx, y += dy` (the first step keeps the stored x,y). Push `{x, y, dx, dy, direction: d}`.

A 4-char string → `[]`.

### PATHSER-STATIC
`Room.serializePath = utils.serializePath`, `Room.deserializePath = utils.deserializePath` (reference/engine/src/game/rooms.js:528-534).

---

## 5. CostMatrix

### CM-LAYOUT
`new PathFinder.CostMatrix()` → `this._bits = new Uint8Array(2500)` (reference/engine/src/game/path-finder.js:18-20). Index is `x*50 + y` (column-major). **GOTCHA**: `_bits` is a public, widely abused property (bots read and write it directly and pass foreign objects that have `_bits`). Chitin must expose a real `Uint8Array` named `_bits` with this layout. `PathFinder`/`CostMatrix` are created once per global (reference/engine/src/game/path-finder.js:12-14), so `instanceof` works across ticks for matrices cached on the global.

### CM-SET — `set(x, y, v)`
`x|0`, `y|0`; `_bits[x*50+y] = Math.min(Math.max(0, v), 255)` (reference/engine/src/game/path-finder.js:22-26). The typed-array store applies ToUint8 (truncation): `3.7→3`, `NaN/undefined/'abc'→0`, `'7'→7`, `true→1`, `Infinity→255`, `-5→0`. No bounds check: an index outside 0..2499 is silently dropped; **GOTCHA** `y >= 50` wraps into column `x+1` (e.g. `set(0,60,v)` writes (1,10)). Returns `undefined`.

### CM-GET — `get(x, y)`
`_bits[(x|0)*50 + (y|0)]`; out of range → `undefined` (reference/engine/src/game/path-finder.js:28-32).

### CM-CLONE
`new CostMatrix`, then `_bits = new Uint8Array(this._bits)` (copy, same length) (reference/engine/src/game/path-finder.js:34-38).

### CM-SERIALIZE
`Array.prototype.slice.apply(new Uint32Array(this._bits.buffer))` (reference/engine/src/game/path-finder.js:40-42). That is 625 plain numbers, each the little-endian composition `b[4k] | b[4k+1]<<8 | b[4k+2]<<16 | b[4k+3]<<24` (host order; x86 is LE). It uses the whole underlying buffer, not the view.

### CM-DESERIALIZE
`Object.create(CostMatrix.prototype)` (the constructor does not run); `_bits = new Uint8Array(new Uint32Array(data).buffer)` (reference/engine/src/game/path-finder.js:44-48). Elements go through ToUint32. If `data` is a number `n` → `n` zero words. **GOTCHA**: any length other than 625 words gives `_bits.length != 2500`, and the native search then silently ignores the matrix (CM-TO-NATIVE).

### CM-TO-NATIVE
The JS wrapper returns `ret._bits` for any truthy callback result (reference/driver/lib/path-finder.js:125-127). Native: if the value is `false` (boolean) → room blocked. Otherwise `Nan::TypedArrayContents<uint8_t>`; only `length == 2500` is used as the matrix (reference/driver/native/src/pf.cc:47-58). Anything else (undefined, wrong length, non-view) → no matrix (terrain costs), **but the room is still loaded and counts against maxRooms**. **GOTCHA**: returning a bare `Uint8Array` from `roomCallback` is ignored (`uint8array._bits` is undefined).

### CM-VALUE-SEMANTICS
In `look()` (reference/driver/native/src/pf.cc:106-123): matrix value `0` → terrain cost; `255` → `OBST`; `1..254` → that cost, **even on wall tiles** (a nonzero value makes a wall walkable, e.g. tunnels/roads). A missing matrix uses an all-zero static matrix (reference/driver/native/src/pf.h:221-223; reference/driver/native/src/main.cc:13).

---

## 6. PathFinder JS layer

### PF-API-SEARCH — `PathFinder.search(origin, goal, opts)`
reference/engine/src/game/path-finder.js:57-65: `if (!goal || Array.isArray(goal) && !goal.length) return {path: [], ops: 0}`. **GOTCHA**: this return has no `cost` and no `incomplete` keys. Otherwise → `driver.pathFinder.search(origin, goal, opts)`.

### PF-API-OPTIONS
reference/driver/lib/path-finder.js:89-96 (`options = options || {}`):

| option | formula | default |
|---|---|---|
| plainCost | `min(254, max(1, (o.plainCost\|0) \|\| 1))` | 1 |
| swampCost | `min(254, max(1, (o.swampCost\|0) \|\| 5))` | 5 |
| heuristicWeight | `min(9, max(1, o.heuristicWeight \|\| 1.2))` (float) | 1.2 |
| maxOps | `max(1, (o.maxOps\|0) \|\| 2000)` | 2000 |
| maxCost | `max(1, (o.maxCost\|0) \|\| 0xffffffff)` | 4294967295 |
| maxRooms | `min(64, max(1, (o.maxRooms\|0) \|\| 16))` | 16 |
| flee | `!!o.flee` | false |

**GOTCHA**s, all from `|0` (ToInt32): `maxCost >= 2^31` wraps negative and becomes `1`, so the search stops at its first pop; `maxCost: Infinity / 0 / NaN` means unlimited; `swampCost: 0` → 5; `maxOps: 0` → 2000; negative maxOps → 1; `plainCost 255` → 254 (never blocks). `heuristicWeight` is a float (no `|0`); `'2'` → 2. The native side converts with `Nan::To<uint32_t>` / `To<double>` and `max_rooms` as `uint8_t` (reference/driver/native/src/main.cc:31-37).

### PF-API-GOALS
reference/driver/lib/path-finder.js:99-112: `goal` may be an array or a single item. Each item:
* if `g.x !== undefined && g.y !== undefined && g.roomName !== undefined` → `{range: 0, pos: toWorldPosition(g)}`;
* else → `{range: max(0, g.range|0), pos: toWorldPosition(g.pos)}`. A RoomObject such as a creep is accepted (it has no `.x`; uses `.pos`, range 0). `g.pos` undefined → TypeError.

Goals are normalised **before** the origin (line 134), so a bad goal throws first. The origin needs `x, y, roomName` directly. **GOTCHA**: passing a Creep as origin throws `'Invalid room name'` (its `roomName` is undefined).

### PF-API-ROOMCALLBACK
reference/driver/lib/path-finder.js:115-131. A non-function truthy `roomCallback` is dropped (no callback). The wrapper is `(xx,yy) => { r = cb(generateRoomName(xx,yy)); return r === false ? false : (r ? r._bits : undefined) }`. Native calls it with receiver `undefined` (reference/driver/native/src/pf.cc:42). It runs **at most once per room per search**, lazily, at the moment the search first touches a tile of that room (PF-CORE-ROOMTABLE). A throw propagates out of `PathFinder.search`. Callbacks may re-enter `PathFinder.search` (PF-API-INSTANCES).

### PF-API-RESULT
reference/driver/lib/path-finder.js:134-141:
* native `undefined` → `{path: [], ops: 0, cost: 0, incomplete: false}`
* native `-1` → `{path: [], ops: 0, cost: 0, incomplete: true}`
* else the native object `{path, ops, cost, incomplete}` (keys in that order, reference/driver/native/src/pf.cc:595-599), with `path = path.map(fromWorldPosition).reverse()`: first step first, origin excluded, last element = reached/closest node.

### PF-API-USE / `_useNewPathFinder`
`register._useNewPathFinder` starts `true` every tick (reference/engine/src/game/game.js:67-68; register is rebuilt per tick at reference/engine/src/game/game.js:502). `PathFinder.use(v)` logs a deprecation if `!v` and sets `register._useNewPathFinder = !!v` (reference/engine/src/game/path-finder.js:67-75). **GOTCHA**: `use` closes over the `register` passed to the first `make()` for this global. `make` returns early once `globals.PathFinder` exists (reference/engine/src/game/path-finder.js:12-14), and the isolate global persists across ticks (reference/driver/lib/runtime/runtime.js:216-217). So `PathFinder.use(false)` affects only the first tick of a fresh global; on later ticks it writes to a stale register and the legacy pathfinder stays unreachable. The readers are `Room#findPath`, `RoomPosition#findPathTo` and `findClosestByPath` (reference/engine/src/game/rooms.js:869, 1412, 1443).

### PF-API-INIT
See TERRAIN-PF-PACKED. The binary exports `search`, `loadTerrain`, `version = 11` (reference/driver/native/src/main.cc:53-57).

### PF-API-INSTANCES
`thread_local std::array<path_finder_t, 2>`. Use the first one not `is_in_use`; if both are busy (callback recursion depth > 2), allocate a new one (~2.16 MB) for this call (reference/driver/native/src/main.cc:9-28). Behaviour is identical either way. Chitin needs re-entrant search state (a stack of scratch contexts per thread).

---

## 7. Native PathFinder core (must produce identical paths)

### PF-CORE-TYPES
reference/driver/native/src/pf.h:11-170.
* `cost_t = pos_index_t = room_index_t = u32`; `k_max_rooms = 64`.
* `world_position_t {u32 xx, yy}`; `null()` = `(0,0)` (id 0). `range_to` = Chebyshev in world coords. `direction_to(p)` = direction from the signs of `(p - this)`, ordered TOP..TOP_LEFT (enum 0..7). `map_position()` = `(xx/50, yy/50)` truncated to `u8`.
* Neighbour positions are computed with u32 arithmetic (`xx - 1` at `xx == 0` wraps). See the open questions.

Search state (reference/driver/native/src/pf.h:343-357): `room_table[64]` (terrain ptr, matrix ptr, map pos), `reverse_room_table[65536]` (map id → 1-based room index, 0 = not loaded), `blocked_rooms` set, `parents[160000]`, `open_closed[160000]`, `heap` (priorities[160000], heap slots[20000]), `goals`, `look_table[4] = {plainCost, OBST, swampCost, OBST}`, `heuristic_weight: f64`, `max_rooms`, `flee`.

### PF-CORE-ROOMTABLE — `room_index_from_pos(map)`
reference/driver/native/src/pf.cc:22-64. The order matters, because callback side effects are observable:
1. `reverse[map.id] != 0` → return it.
2. `room_table_size >= max_rooms` → return 0 (treated as `OBST`; **not** remembered, and no callback).
3. `map ∈ blocked_rooms` → return 0.
4. Terrain pointer null → throw JS `Error("Could not load terrain data")` (aborts the search).
5. If a callback exists: call it with `(xx, yy)`. Throw → propagate. Returns boolean `false` → insert into `blocked_rooms`, return 0 (**does not count toward maxRooms**). Otherwise matrix = typed-array contents if `length == 2500`, else none.
6. Append to `room_table`; `reverse[map.id] = ++size`; return it.

Rooms load in the order the search first `look()`s a tile in them. Only border crossings can touch new rooms (in-room lookahead never crosses, see PF-CORE-JUMP), so the callback order follows border-crossing expansion order.

### PF-CORE-INDEX
`index = (roomIndex-1)*2500 + (xx%50)*50 + (yy%50)`; the inverse uses `room_table[index/2500].pos` (reference/driver/native/src/pf.cc:67-80). `index_from_pos` throws a C++ `runtime_error` if the room is not loaded. That cannot happen on valid paths, because `push_node` is only called after a successful `look()`.

### PF-CORE-LOOK — `look(pos)`
reference/driver/native/src/pf.cc:106-123:
```
ri = room_index_from_pos(pos.map); if ri == 0: return OBST
m = matrix(ri)[x%50][y%50]           // x-major
if m != 0: return m == 255 ? OBST : m
return look_table[ terrain2bits(ri, x%50*50 + y%50) ]
```

### PF-CORE-HEURISTIC — `heuristic(pos)` (unweighted, u32)
reference/driver/native/src/pf.cc:126-148:
* non-flee: `min over goals of (dist > range ? dist - range : 0)`, starting from `u32::MAX`.
* flee: `max over goals with dist < range of (range - dist)`, starting from 0. **GOTCHA**: `dist == range` already counts as escaped.
* `dist` = Chebyshev in world coords, so it crosses rooms freely and ignores walls.

Weighted use: `(u32)(heuristic(pos) as f64 * heuristic_weight)` (C++ truncation; reference/driver/native/src/pf.cc:88, 535). **GOTCHA**: compute it exactly as an f64 multiply followed by truncation toward zero (Rust `(h as f64 * w) as u32`). Integer tricks like `h*6/5` change tie-breaks.

### PF-CORE-HEAP (tie-breaking is everything)
1-based binary min-heap of node indices; priorities are stored **per node index**, not per slot (reference/driver/native/src/pf.h:251-335).
```
insert(i, p): if size == 19999: throw "Max heap"; prio[i]=p; size+=1; heap[size]=i; bubble_up(size)
bubble_up(k): while k != 1: if prio[heap[k]] <= prio[heap[k/2]]: swap(k, k/2); k/=2 else return
pop(): ret = (heap[1], prio[heap[1]]); heap[1] = heap[size]; size -= 1
       v = 1; loop { u = v
         if 2u+1 <= size: if prio[heap[u]] >= prio[heap[2u]]: v = 2u
                          if prio[heap[v]] >= prio[heap[2u+1]]: v = 2u+1
         elif 2u <= size: if prio[heap[u]] >= prio[heap[2u]]: v = 2u
         if u != v: swap(u, v) else break }
update(i, p): for k in size down to 1: if heap[k] == i: prio[i] = p; bubble_up(k); return
```
**GOTCHA**: the `<=` in bubble_up makes a newly inserted node rise **above** equal-priority nodes (LIFO among ties). The `>=` in pop makes the sinking element go below equal children and prefer the right child on equal children. Port this sequence of comparisons verbatim; a `BinaryHeap` or any "equivalent" heap gives different paths. `update` does a linear scan from the end (**PERF**: O(open)).

### PF-CORE-OPENCLOSED
Marker array: `open` ⇔ `list[i] == marker`, `closed` ⇔ `list[i] == marker+1`; `clear()` adds 2, or zero-fills when near `u32::MAX` (reference/driver/native/src/pf.h:174-209). Gives an O(1) reset per search. **GOTCHA**: the origin is never marked open or closed (below).

### PF-CORE-PUSH — `push_node(parent, node, g)`
reference/driver/native/src/pf.cc:83-103:
```
i = index_from_pos(node); if closed(i): return
f = (u32)(heuristic(node) * w) + g
if open(i): if prio[i] > f: heap.update(i, f); parents[i] = parent     // strict >; equal f keeps old parent
else: heap.insert(i, f); open(i); parents[i] = parent
```

### PF-CORE-SEARCH
reference/driver/native/src/pf.cc:456-567:
```
reset: clear reverse entries of last search; room_table_size=0; blocked.clear(); goals.clear();
       open_closed.clear(); heap.clear()
goals = parse(goals_js); callback = (undefined ? none : cb)
look_table[0]=plainCost; look_table[2]=swampCost
ops = maxOps; min_h = U32MAX; min_g = U32MAX; min_node = 0
if heuristic(origin) == 0: return UNDEFINED        // before any callback (origin already satisfies goal)
if room_index_from_pos(origin.map) == 0: return -1 // origin room callback runs here; false → -1
min_node = index(origin)
astar(min_node, origin, 0)                          // origin expansion; NOT counted in ops; origin NOT closed
while !heap.empty() && ops > 0:
   (i, f) = heap.pop(); close(i)
   pos = pos(i); h = heuristic(pos); g = f - (u32)(h * w)
   if h == 0: min_node=i; min_h=0; min_g=g; break
   elif h < min_h: min_node=i; min_h=h; min_g=g     // strict: first popped among equal h wins
   if g + h > maxCost: break                        // unweighted h; node already recorded as min
   jps(i, pos, g)
   ops -= 1
   if isolate terminating: return UNDEFINED
```
**GOTCHA**s:
* `ops` counts only expansions in the loop; the goal pop and the maxCost-break pop are not counted.
* The goal test runs **before** the maxCost test. A goal popped with `g > maxCost` returns a *complete* path whose `cost > maxCost`.
* The "closest" node for incomplete results is the lowest **unweighted** `h` among **popped** nodes, with ties going to the earliest pop; `g` is ignored.
* The origin is pushed into neither open nor closed. Neighbours can push it again later (as a normal tile with a parent); reconstruction stops at the origin anyway. Port this exactly (it affects heap content and ops).
* If nothing is ever popped (all origin neighbours blocked), the result is `{path: [], ops: 0, cost: 4294967295, incomplete: true}`, because `min_g` keeps its initial `U32MAX` (reference/driver/native/src/pf.cc:503, 598).

### PF-CORE-ORIGIN-ASTAR — `astar(index, pos, g)` (used only for the origin)
reference/driver/native/src/pf.cc:151-190. Directions in order TOP, TOP_RIGHT, RIGHT, BOTTOM_RIGHT, BOTTOM, BOTTOM_LEFT, LEFT, TOP_LEFT; `n = pos+dir`; border rules (first matching branch wins):
```
lx==0 : skip if (n.x%50==49 && n.y!=pos.y) or n.x==pos.x
lx==49: skip if (n.x%50==0  && n.y!=pos.y) or n.x==pos.x
ly==0 : skip if (n.y%50==49 && n.x!=pos.x) or n.y==pos.y
ly==49: skip if (n.y%50==0  && n.x!=pos.x) or n.y==pos.y
c = look(n); if c == OBST: continue; push_node(index, n, g + c)
```
So from an exit tile you may step only inward (3 tiles) or straight across into the neighbouring room; moving along the edge or diagonally across the border is forbidden. **GOTCHA**: at a corner tile only the x-rule applies (corners are walls in standard terrain).

### PF-CORE-JPS — `jps(index, pos, g)` (every popped node)
reference/driver/native/src/pf.cc:304-437:
```
par = pos(parents[index]); dx = sign(pos.x - par.x); dy = sign(pos.y - par.y)
(lx, ly) = (pos.x%50, pos.y%50)
// PF-CORE-JPS-BORDER (if/else-if chain on lx first, then ly):
lx==0 : dx==-1 → [(x-1,y)];             dx==1  → [(x+1,y-1),(x+1,y),(x+1,y+1)]
lx==49: dx==1  → [(x+1,y)];             dx==-1 → [(x-1,y-1),(x-1,y),(x-1,y+1)]
ly==0 : dy==-1 → [(x,y-1)];             dy==1  → [(x-1,y+1),(x,y+1),(x+1,y+1)]
ly==49: dy==1  → [(x,y+1)];             dy==-1 → [(x-1,y-1),(x,y-1),(x+1,y-1)]
if list nonempty: for n in list: c=look(n); if c!=OBST: push_node(index, n, g+c); return
// (lx==0 with dx==0 etc. → empty list → falls through to regular JPS)
bdx = lx==1 ? -1 : lx==48 ? 1 : 0;  bdy = ly==1 ? -1 : ly==48 ? 1 : 0
cost = look(pos)
if dx!=0: n=(x+dx,y); c=look(n); if c!=OBST: (bdy==0 ? jump_neighbor(pos,index,n,g,cost,c) : push_node(index,n,g+c))
if dy!=0: n=(x,y+dy); c=look(n); if c!=OBST: (bdx==0 ? jump_neighbor(...)            : push_node(index,n,g+c))
if dx!=0 && dy!=0:
   n=(x+dx,y+dy); c=look(n); if c!=OBST: jump_neighbor(pos,index,n,g,cost,c)
   if look(x-dx,y) != cost: jump_neighbor(pos,index,(x-dx,y+dy),g,cost,look(x-dx,y+dy))
   if look(x,y-dy) != cost: jump_neighbor(pos,index,(x+dx,y-dy),g,cost,look(x+dx,y-dy))
elif dx!=0:
   if bdy==1  || look(x,y+1) != cost: jump_neighbor(pos,index,(x+dx,y+1),g,cost,look(x+dx,y+1))
   if bdy==-1 || look(x,y-1) != cost: jump_neighbor(pos,index,(x+dx,y-1),g,cost,look(x+dx,y-1))
else:
   if bdx==1  || look(x+1,y) != cost: jump_neighbor(pos,index,(x+1,y+dy),g,cost,look(x+1,y+dy))
   if bdx==-1 || look(x-1,y) != cost: jump_neighbor(pos,index,(x-1,y+dy),g,cost,look(x-1,y+dy))
```
**GOTCHA**: this is weighted JPS. "Forced neighbour" means a side tile whose cost **differs** from the current tile's cost (obstacle, swamp next to plain, or any CostMatrix value), not only an obstacle. Push order (straight-x, straight-y, diagonal, forced-a, forced-b) sets heap insertion order and therefore the tie-breaks.

### PF-CORE-JUMP-NEIGHBOR
reference/driver/native/src/pf.cc:439-454:
```
if nc != cost || is_border(n.x) || is_border(n.y):     // is_border(v): (v+1)%50 < 2  ⇔ v%50 ∈ {49,0}
    if nc == OBST: return
    g += nc
else:
    j = jump(nc, n, n.x-pos.x, n.y-pos.y); if j.is_null(): return
    g += nc * (range(pos, j) - 1) + look(j)
    n = j
push_node(index, n, g)
```
The cost of a jump is uniform-cost intermediate tiles plus the cost of the jump point itself, which may differ (cost change ends the jump).

### PF-CORE-JUMP
reference/driver/native/src/pf.cc:193-302. `near_border(v)`: `(v+2)%50 < 4` ⇔ `v%50 ∈ {48,49,0,1}`. `jump(cost,p,dx,dy)` = `jump_xy` if both nonzero, `jump_x` if `dx`, else `jump_y`.
```
jump_x(cost,p,dx):
  pu = look(x,y-1); pd = look(x,y+1)
  loop:
    if heuristic(p)==0 || near_border(p.x): break
    cu = look(x+dx,y-1); cd = look(x+dx,y+1)
    if (cu!=OBST && pu!=cost) || (cd!=OBST && pd!=cost): break
    pu = cu; pd = cd; p.x += dx
    jc = look(p); if jc==OBST: return NULL; if jc!=cost: break
  return p
jump_y: same with (x±1) sides, stepping y by dy, near_border(p.y).
jump_xy(cost,p,dx,dy):
  px = look(x-dx,y); py = look(x,y-dy)
  loop:
    if heuristic(p)==0 || near_border(p.x) || near_border(p.y): break
    if (look(x-dx,y+dy)!=OBST && px!=cost) || (look(x+dx,y-dy)!=OBST && py!=cost): break
    px = look(x,y+dy); py = look(x+dx,y)
    if (py!=OBST && jump_x(cost,(x+dx,y),dx)!=NULL) || (px!=OBST && jump_y(cost,(x,y+dy),dy)!=NULL): break   // short-circuit ||
    p.x += dx; p.y += dy
    jc = look(p); if jc==OBST: return NULL; if jc!=cost: break
  return p
```
`NULL` = world (0,0). Jumps start from non-border tiles and stop by the time they reach a tile with `x%50`/`y%50 ∈ {1,48}`, so they never cross rooms (no callbacks, no collision with NULL). **PERF**: `heuristic()` runs on every jump step, O(#goals) per step.

### PF-CORE-RESULT
reference/driver/native/src/pf.cc:569-601:
```
path=[]; i=min_node; p=pos(i)
while p != origin:
   path.push(p)
   i = parents[i]; nx = pos(i)
   if range(nx,p) > 1: d = p.direction_to(nx); do { p = p+d; path.push(p) } while range(p,nx) > 1
   p = nx
return {path, ops: maxOps - ops_remaining, cost: min_g, incomplete: min_h != 0}
```
Jump segments are interpolated along their straight or diagonal line. The array is goal→start (origin excluded); the JS layer reverses it.

### PF-CORE-SPECIAL-RETURNS
* `heuristic(origin) == 0` → `undefined` → `{path:[],ops:0,cost:0,incomplete:false}`; no callback runs (reference/driver/native/src/pf.cc:506-510).
* Origin room refused (callback `false` or `maxRooms` < 1, which cannot happen) → `-1` → `incomplete:true` (reference/driver/native/src/pf.cc:515-519).
* Isolate terminating (CPU timeout) → `undefined` (reference/driver/native/src/pf.cc:557-561).

### PF-CORE-ERRORS
* Missing terrain for a touched room (including the origin room) → JS `Error('Could not load terrain data')` (reference/driver/native/src/pf.cc:31-35).
* A callback exception is re-thrown to the caller (reference/driver/native/src/pf.cc:43-46, 563-567).
* Heap overflow (`size == 19999`) → C++ `runtime_error("Max heap")`, **not** caught by the `js_error` handler (reference/driver/native/src/pf.h:301-304; reference/driver/native/src/pf.cc:563). See the open questions.

### PF-CORE-CONFORMANCE (golden test)
reference/driver/native/profile.js:9-105 + reference/driver/native/sample-terrain.js:1-2 (625-byte packed rooms in a `Uint32Array` buffer, with map coords). For 5 rounds, every ordered pair `(i, j≠i)` of 28 positions runs `search(pos_i, [{range: i%3, pos: pos_j}], cb, 1, 5, 16, 100000, 100000, false, 1.2)`. `cb` returns `false` when `j%5==0 && i%2==0` (so the origin is blocked → `-1`). Otherwise it returns the **number 2** (the script's matrix loop overwrites the array with a number, profile.js:41-46), which counts as "no matrix". Sum `path.length + ops` over truthy results; the expected checksum is **11843305** (profile.js:102). Chitin's PF core must reproduce it bit-exactly. Port it as a unit test.

---

## 8. Room / RoomPosition wrappers (PathFinder-backed — the live path)

### ROOM-FINDPATH — `room.findPath(fromPos, toPos, opts)`
reference/engine/src/game/rooms.js:863-871: `if (fromPos.roomName != this.name) return opts.serialize ? '' : []`. **GOTCHA**: with `opts` undefined this throws a TypeError. Then, if `_useNewPathFinder`, → `_findPath2(this.name, fromPos, toPos, opts)` (opts **not cloned**), else LEGACY-FINDPATH.

### ROOM-FINDPATH-NEW — `_findPath2(id, from, to, opts)`
reference/engine/src/game/rooms.js:223-302:
1. `opts = opts || {}`. `from.isEqualTo(to)` → `''` or `[]`.
2. `opts.avoid` / `opts.ignore` → deprecation log, then set to `undefined` (**mutates the caller's object**).
3. If `opts.maxOps === undefined && (opts.maxRooms === undefined || opts.maxRooms > 1) && from.roomName != to.roomName` → `opts.maxOps = 20000` (also mutates).
4. `searchOpts = {roomCallback, maxOps: opts.maxOps, maxRooms: opts.maxRooms}`. If `!opts.ignoreRoads`: `plainCost = 2, swampCost = 10`. If `opts.plainCost` is truthy it overrides; same for `swampCost`. **GOTCHA**: `heuristicWeight`, `maxCost` and `flee` are **not forwarded** (the docs list heuristicWeight; the weight is always 1.2). The docs' "plainCost default 1 / swampCost 5" holds only with `ignoreRoads`.
5. `roomCallback(room)`: `m = getPathfindingGrid2(room, opts)`. If `opts.costCallback` is a function: `m = m.clone(); r = costCallback(room, m); if r instanceof PathFinder.CostMatrix: m = r`. Return `m`. **GOTCHA**: a matrix is always returned, so rooms are never blocked and `costCallback` cannot block a room (returning `false` uses the clone).
6. `ret = PathFinder.search(from, {range: max(1, opts.range || 0), pos: to}, searchOpts)`.
7. If `!opts.range` and (the last path pos `isNearTo(to)` and is not equal to `to`, OR the path is empty and `from.isNearTo(to)`) → `ret.path.push(to)`. **GOTCHA**: the target tile is appended even if it is a wall or obstacle. Range 0 searches to range 1, then appends the target.
8. Convert to steps, starting `cur = from`. **Stop at the first position whose `roomName != id`** (a cross-room path is truncated after this room's exit tile).
9. `opts.serialize` → `serializePath(steps)`.

### ROOM-DEFAULT-MATRIX — `getPathfindingGrid2(id, opts)`
reference/engine/src/game/rooms.js:157-221.
* `!privateStore[id]` (room not visible this tick) → a fresh all-zero `CostMatrix` (not cached).
* Cache key `'grid2' + ('_ignoreCreeps')? + ('_ignoreDestructibleStructures')? + ('_ignoreRoads')?` (truthiness). Cached in `privateStore[id].pfGrid`, which `make()` resets every tick (reference/engine/src/game/rooms.js:386). The cached matrix is returned **uncloned** when there is no costCallback.
* Build (`makePathfindingGrid2`):
  * `obstacleTypes = OBSTACLE_OBJECT_TYPES + ['portal']`.
  * `ignoreDestructibleStructures` removes `constructedWall, spawn, extension, link, storage, observer, tower, powerBank, powerSpawn, lab, terminal`. **GOTCHA**: `nuker`, `factory`, `invaderCore`, `source`, `mineral`, `deposit`, `controller` and `portal` stay obstacles.
  * Creeps: if `ignoreCreeps` **or** (the room controller exists and `safeMode` and `my`), remove `creep` and `powerCreep`.
  * For each raw object in `register.objectsByRoomKeys[id]` order (runtime room-object order, `temp` objects skipped; reference/engine/src/game/game.js:184-194), set 255 if any of:
    (a) `type ∈ obstacleTypes`;
    (b) `!ignoreCreeps` && my safe-mode room && type is `creep`/`powerCreep` && `object.user == me` (**GOTCHA**: in your own safe-mode room hostile creeps are passable, your own creeps are not);
    (c) `!ignoreDestructibleStructures` && `rampart` && `!isPublic` && `user != me`;
    (d) `!ignoreDestructibleStructures` && `constructionSite` && `user == me` && `structureType ∈ OBSTACLE_OBJECT_TYPES`.
  * Then: `type=='swamp' && get==0` → `ignoreRoads ? 5 : 10` (legacy object type, normally absent).
  * Then: `!ignoreRoads && type=='road' && get < 255` → 1.
  * Road vs obstacle on the same tile gives the same result in either order.
* Includes spawning creeps and all players' creeps present in `roomObjects`.

### ROOM-FINDCLOSESTBYPATH-NEW — `_findClosestByPath2(from, objects, opts)`
reference/engine/src/game/rooms.js:304-374:
1. `objects` number → `register.rooms[from.roomName].find(objects, {filter: opts.filter})`; else if `opts.filter` → `_.filter`.
2. Empty → `null`. If an object is on `from` (`from.isEqualTo(obj)`) → return the first such object.
3. Goals = `{range: 1, pos: obj.pos || obj}` for all objects. Deprecation logs for `avoid`/`ignore` (not cleared).
4. `searchOpts = {roomCallback: room => objectsByRoom[room] ? (default matrix + costCallback as above) : undefined, maxOps: opts.maxOps, maxRooms: 1}`, plus `plainCost 2 / swampCost 10` unless `ignoreRoads`. **GOTCHA**: `opts.plainCost/swampCost/range/algorithm` are ignored, and `maxRooms` is forced to 1.
5. `lastPos = last path pos || from`. **Return the last object in array order with `lastPos.isNearTo(obj)`** (**GOTCHA**: ties go to the later array element, not to the goal the search actually reached).

### POS-FINDPATHTO — `pos.findPathTo(x, y, [opts]) | (target, [opts])`
reference/engine/src/game/rooms.js:1396-1427: `[x,y,roomName] = fetchXYArguments(a, b)`; `room = register.rooms[this.roomName]`. If `b` is an object, `opts = _.clone(b)`; then `opts = opts || {}`. **GOTCHA**: the `(x, y, opts)` form does not clone, so `_findPath2` mutates the caller's opts. `roomName ||= this.roomName`. `!room` → throw `` `Could not access room ${this.roomName}` ``. New mode (or same room) → `room.findPath(this, new RoomPosition(x, y, roomName), opts)`. **GOTCHA**: an unrecognised target (e.g. a plain `{x,y,roomName}`) passes `undefined` coordinates, and the RoomPosition constructor packs them as (0,0).

### POS-FINDCLOSESTBYPATH — `pos.findClosestByPath(type|objects, opts)`
reference/engine/src/game/rooms.js:1429-1445: `opts = _.clone(opts || {})`; room not visible → throw; `type === undefined` → `null`; new mode → ROOM-FINDCLOSESTBYPATH-NEW.

### ROOM-FINDEXITTO
`register.map.findExit(this.name, room)` with no opts (reference/engine/src/game/rooms.js:1130-1132).

---

## 9. Legacy pathfinder (`@screeps/pathfinding`) — reachable only via LEGACY-REACHABILITY

### LEGACY-REACHABILITY
The legacy pathfinder runs only when `_useNewPathFinder` is false, which in practice means after `PathFinder.use(false)` during the first tick of a fresh global (PF-API-USE). `@screeps/pathfinding` is used only by `rooms.js` (reference/engine/src/game/rooms.js:5) and by `WorldMapGrid`, which uses only `pathfinding.Node` (reference/driver/lib/runtime/mapgrid.js:1, 139). Priority: low, but port it for completeness.

### LEGACY-GRID — `getPathfindingGrid(id, opts, endNodesKey)`
reference/engine/src/game/rooms.js:32-155. Weights `rows[y][x]`:
* Border tile (x or y ∈ {0,49}) → 11, else 2.
* Wall bit → 0.
* Swamp bit with value still 2 → 10 (border swamps stay 11).
* For each object (`register.objectsByRoomKeys[id].forEach` — **GOTCHA**: no guard, a room with no objects throws a TypeError): obstacle → 0 if any of: `type ∈ OBSTACLE_OBJECT_TYPES` (minus the ignore lists; **no portal**); a non-public foreign rampart (`!ignoreDestructibleStructures`); my obstacle-type construction site (`!ignoreDestructibleStructures`). Then a road with value `> 0` → 1 (**GOTCHA**: a road on a wall stays 0).
* `opts.ignore`: for each truthy `i`: if `i.pos` → `min(v, 2)` at pos (**GOTCHA**: `v > 2 ? 2 : v`, so obstacles (0) stay blocked); non-RoomPosition objects with `x` are replaced in `opts.ignore[key]` by a RoomPosition (mutation); `i.x` defined → same `min(v,2)`. A non-array → throw `` 'option `ignore` is not an array' ``.
* `opts.avoid`: same shape, set 0; error text `` 'option `avoid` is not an array' ``.
* End nodes (`pfEndNodes[key]`): `i.x` defined → `rows[i.y][i.x] = 999`, else `i.pos` → 999.
* Cache name `'grid' + _ignoreCreeps + _ignoreDestructibleStructures + (_endNodes<key> if number) + (_avoid<setKey>) + (_ignore<setKey>)`. Cached per tick, `.clone()` per call (fresh nodes).

### LEGACY-POSSET-CACHE
`positionsSetCache.key(array)` (reference/engine/src/game/rooms.js:449-483): non-array → 0. Map items to positions (`i.pos`, or `new RoomPosition(i.x,i.y,id)` for plain objects). Find an existing key whose cached set has the same length and where every new position `isEqualTo` some cached element; an element without `isEqualTo` → throw `'Invalid position …, check your `opts` property'`. Otherwise `key = positionsSetCacheCounter++` (a counter shared by all rooms, reset to 1 each tick at rooms.js:383) and cache `_.clone(array)`. Existing keys are returned via `parseInt`.

### LEGACY-FINDER-CACHE
`getPathfinder(id, opts)`: `_.defaults(opts, {maxOps: 2000, heuristicWeight: 1})`, key `"${maxOps},${heuristicWeight}"` → `new AStarFinder({diagonalMovement: 1 /*Always*/, maxOpsLimit: maxOps, heuristic: chebyshev, weight: heuristicWeight})` (reference/engine/src/game/rooms.js:16-30). **GOTCHA**: the legacy default weight is 1, not 1.2.

### LEGACY-FINDPATH (legacy branch of `Room#findPath`)
reference/engine/src/game/rooms.js:873-968:
1. `opts = _.clone(opts || {})`; suffix from ignoreCreeps / ignoreDestructibleStructures / avoid key / ignore key.
2. `toPos` is a number (end-nodes key): if `!pfEndNodes[toPos]` → empty; else Dijkstra on `getPathfindingGrid(id, opts, toPos)` from (fromX, fromY) to `(-999, -999)`.
3. Else: `toPos.roomName != this.name` → empty. If `pathCache["fx,fy,tx,ty"+suffix]` exists → return it (serialized, or deep clone). **GOTCHA**: the cache check runs before the equality/bounds checks and ignores maxOps and weight. `from == to` → empty; any coordinate outside 0..49 → empty. If Chebyshev ≤ 1 → a single step to the target regardless of walkability. Otherwise grid + finder; `grid.setWalkableAt(toX, toY)` (weight := 1) → `finder.findPath`.
4. `path.splice(0,1)` (drop start), map to steps. If non-empty, cache under `"fx,fy,lastX,lastY"+suffix` (`_.cloneDeep`). Serialize if `opts.serialize`.

### LEGACY-ASTAR — `AStarFinder.findPath(sx, sy, ex, ey, grid)`
reference/pathfinding/src/finders/AStarFinder.js:54-164:
```
open = Heap(cmp = (a,b) => a.f - b.f)
start.g = 0; start.f = 0; start.h = endNode ? w*H(|sx-ex|,|sy-ey|) : 0      // endNode = ex != -999 ? node : null
push(start); start.opened = true; closest = start; ops = 0
while !open.empty():
  n = open.pop(); n.closed = true
  if n === endNode || n.weight == 999: return backtrace(n)
  ops += 1; if maxOpsLimit && ops > maxOpsLimit: break
  for m in grid.getNeighbors(n, diagonalMovement):
     if m.closed: continue
     ng = n.g + m.weight * ((m.x==n.x || m.y==n.y) ? 1 : 1.00000001)
     if !m.opened || ng < m.g:
        m.g = ng; m.h = m.h || w*H(|m.x-ex|,|m.y-ey|); m.f = m.g + m.h; m.parent = n
        if m.h < closest.h || (m.h === closest.h && m.g < closest.g): closest = m
        if !m.opened: open.push(m); m.opened = true else open.updateItem(m)
return backtrace(closest)
```
**GOTCHA**s: diagonal steps cost ×1.00000001, so straight steps win ties. The fallback "closest" is the best **discovered** node (updated on relax), unlike the native "best popped". `h` is computed once per node (`||`). Backtrace includes the start (reference/pathfinding/src/core/Util.js:7-14). `H` = chebyshev `max(dx,dy)` (reference/pathfinding/src/core/Heuristic.js:44-46). `open` is the `heap@0.2.5` array heap: `push` / `pop` / `empty` at AStarFinder.js:77, 81, 83, 147 follow LEGACY-HEAP, and `updateItem` at AStarFinder.js:153 follows LEGACY-HEAP-UPDATE.

### LEGACY-DIJKSTRA
`DijkstraFinder` = AStarFinder with `heuristic = () => 0` (reference/pathfinding/src/finders/DijkstraFinder.js:14-17). Built per room with `{diagonalMovement: 1}` and no maxOps (unbounded) (reference/engine/src/game/rooms.js:443). It stops at the first popped node with `weight == 999`. With h ≡ 0 the "closest" never changes, so failure → `[start]` → `[]`.

### LEGACY-GRID-NEIGHBORS
reference/pathfinding/src/core/Grid.js. Node `weight = matrix[y][x]`; walkable ⇔ inside and `weight > 0` (Grid.js:68, 88-91). `setWalkableAt` sets `weight = 1` (Grid.js:115-118). Neighbour order: ↑ → ↓ ←, then ↖ ↗ ↘ ↙; with `Always` (=1) the diagonals need only the target tile to be walkable, so diagonal corner-cutting between walls is allowed (Grid.js:139-211; reference/pathfinding/src/core/DiagonalMovement.js:1-8). `clone` copies `walkable` and `weight` (Grid.js:218-239).

### LEGACY-HEAP
`@screeps/pathfinding` pins `"heap": "0.2.5"` exactly (reference/pathfinding/package.json:18-20) and requires it from AStarFinder (reference/pathfinding/src/finders/AStarFinder.js:1). The package is not in `reference/`; the installed copy is at E:\screeps\server\node_modules\heap (package.json `version` 0.2.5, hoisted next to `@screeps/pathfinding` 0.4.17, no nested copy). Rules below are verified against its `lib/heap.js` (CoffeeScript 1.8.0 output). The minified copy in reference/pathfinding/visual/lib/pathfinding-browser.min.js:1 has the same `_siftdown` / `_siftup` / `updateItem` logic.

Layout: a plain JS array `nodes`, 0-based, children of `p` at `2p+1` and `2p+2`, parent `(p-1) >> 1`. AStarFinder's comparator is `(a, b) => a.f - b.f` (reference/pathfinding/src/finders/AStarFinder.js:55-57); every decision below tests only `cmp(...) < 0`, so equal `f` is never "less".
```
_siftdown(a, start, pos):            // heap.js:231-248 — moves a[pos] UP
  item = a[pos]
  while pos > start:
    pp = (pos-1) >> 1
    if cmp(item, a[pp]) < 0: a[pos] = a[pp]; pos = pp      // strict
    else: break
  a[pos] = item

_siftup(a, pos):                     // heap.js:250-270 — bottom-up (CPython heapq)
  end = len(a); start = pos; item = a[pos]; c = 2*pos+1
  while c < end:                     // always descends to a leaf, never compares item
    r = c+1
    if r < end && !(cmp(a[c], a[r]) < 0): c = r       // ties pick the RIGHT child
    a[pos] = a[c]; pos = c; c = 2*pos+1
  a[pos] = item
  _siftdown(a, start, pos)           // climb back, strict <, never above start

push(x):  a.push(x); _siftdown(a, 0, len-1)                     // heap.js:62-68
pop():    last = a.pop()                                          // heap.js:75-89
          if len(a) > 0: ret = a[0]; a[0] = last; _siftup(a, 0); return ret
          else: return last                                       // undefined on empty
updateItem(x):                                                    // heap.js:160-171
          pos = a.indexOf(x)        // identity ===, O(n) linear scan
          if pos == -1: return
          _siftdown(a, 0, pos)
          _siftup(a, pos)           // on whatever element is NOW at pos
```
* `push`: a new node rises only past strictly greater parents, so it settles **below** equal-f nodes (FIFO-ish, **the opposite of the native heap's `<=` LIFO**, PF-CORE-HEAP).
* `pop`: because `_siftup` sinks the moved last element all the way to a leaf before climbing back with strict `<`, it can end up below equal-f nodes it would never have passed with a textbook sift-down. The order among ties is therefore neither FIFO nor LIFO. Only a verbatim port of these two loops reproduces it.
* `empty()` is `nodes.length === 0`, `clear()` replaces the array (heap.js:326-332). `peek`, `replace`, `pushpop`, `heapify`, `nlargest`, `nsmallest` and `insort` are not used by AStarFinder or DijkstraFinder.

### LEGACY-HEAP-UPDATE
AStarFinder calls `updateItem(m)` only when `m` is already open and `ng < m.g`, so `m.f` strictly decreased (LEGACY-ASTAR). heap.js:160-171 does **not** sift the item alone:
1. `_siftdown(a, 0, pos)` moves `m` up past strictly greater ancestors. Each passed ancestor shifts down one level; `m`'s original parent lands at `pos`.
2. `_siftup(a, pos)` then runs on the element **now at `pos`**: the displaced parent `P` if `m` moved, else `m` itself. That element is sunk to a leaf (right child on ties) and climbs back with strict `<`.

**GOTCHA**: when `m` moved, step 2 runs on a subtree that was already a valid heap (`P` ≤ all its new descendants) and still reorders it: `P` returns to `pos` only if it is strictly below every node on its sink path, otherwise it stays under the equal-f nodes it passed. Worked example: `[A5, M5, C7, D5, X5]`, `M.f` drops to 4 → `_siftdown` gives `[M4, A5, C7, D5, X5]` → `_siftup(1)` sinks `A` to index 4 via the right child `X` (tie) → `A5` is not `< X5`, so it stays → `[M4, X5, C7, D5, A5]` (`A` and `X` swapped). When `m` did not move, its new `f` is below its old `f` ≤ every descendant, so step 2 brings it back to `pos` (no change), unless the float sum `g + h` rounds to equal a descendant's `f`, in which case it sinks below it. A Rust port must run both calls in this order (not a "decrease-key" sift-up alone), or ties pop in a different order and legacy paths differ.

**PERF**: `indexOf` is O(open) per update, and the whole heap holds node objects. A position map would be faster, but it must give the same `pos` (`indexOf` returns the first match; nodes are unique, so any exact index map works) and must keep the same two sift calls.

### LEGACY-ENDNODES — `room.getEndNodes(type, opts)`
reference/engine/src/game/rooms.js:1099-1128: `type === undefined` → throw `'Find type cannot be undefined'`. If there is no filter and `type` is a number → `key = type`. Else run find if numeric; `key = positionsSetCache.key(type)`; `pfEndNodes[key] = cache[key]`. If unset → `pfEndNodes[key] = _.clone(type)` (or `find(type)` when numeric). **GOTCHA**: FIND constants and positions-set counter keys share one namespace (e.g. `FIND_EXIT_TOP=1` vs set key 1).

### LEGACY-FINDCLOSESTBYPATH
reference/engine/src/game/rooms.js:1447-1515: `opts.serialize = false`; `endNodes = getEndNodes`.
* If no `opts.algorithm`: `h_i = Chebyshev(this, obj)` (room ignored); `algorithm = sumH > minH*10 ? 'dijkstra' : 'astar'`.
* dijkstra: pick the first object with the smallest of {equal:-1, near:0, else 1} (strict `<`). If still 1 → `findPath(this, endNodes.key, opts)`; the result is the first object equal to the last step.
* astar: for each object, distance = -1 (equal) / 0 (near) / `path.length` if `findPathTo(obj)` ends near it / `undefined`. Take it if `isNear === undefined || distance <= isNear` (**the last object with minimal distance wins**).

### LEGACY-FINDPATHTO-XROOM
For a different room in legacy mode: `exitDir = room.findExitTo(roomName)`; `< 0` → `[]`; `exit = this.findClosestByPath(exitDir, opts)`; none → `[]`; else `room.findPath(this, exit, opts)` (reference/engine/src/game/rooms.js:1415-1425).

### LEGACY-UNUSED
BestFirst, BreadthFirst, Bi*, IDAStar and JumpPoint finders, plus the manhattan/euclidean/octile heuristics, are exported but never used by the engine (reference/pathfinding/src/PathFinding.js:1-17). Not in scope.

---

## 10. World-map routing

### MAP-WORLDMAPGRID
`accessibleRooms` = DB rooms with `status == 'normal'` and `openTime` passed (reference/driver/lib/index.js:490-496), cached ≤ 60 s by the runner (reference/driver/lib/runtime/data.js:78-85). Each user VM builds `WorldMapGrid` once (reference/driver/lib/runtime/runtime.js:206-208). **GOTCHA**: rooms opened later are unknown to `describeExits` and `findRoute` in an existing VM until it restarts. Only `gridData` (TERRAIN-EXIT-GRID) is used by the engine.

### MAP-DESCRIBEEXITS — `Game.map.describeExits(roomName)`
reference/engine/src/game/map.js:20-46: if `!/^(W|E)\d+(N|S)\d+$/.test(name)` → `null`. Look up `gridData["x,y"]`; missing → `null`. Result object: `t → exits[1] = name(x,y-1)`, `b → exits[5]`, `l → exits[7]`, `r → exits[3]`. Integer-like keys enumerate ascending (1,3,5,7). Whether the neighbour room exists is not checked.

### MAP-ROUTE-HEAP
`OpenClosed`: Uint8Array markers; `clear()` adds 2, or reallocates when `marker >= 253` (reference/engine/src/game/path-utils.js:5-35). `Heap(size, Float64Array)`: same algorithm and comparisons as PF-CORE-HEAP (`>=` in pop, `<=` in bubbleUp, linear `update` from the end), with `Uint16Array` slots (reference/engine/src/game/path-utils.js:39-122).

### MAP-ROUTE-WINDOW
`kRouteGrid = 30`, `originX = fromX + 30`, `index = (originX - x)*60 + (originY - y)`, valid only if `0 <= originX - x < 60` and the same for y (reference/engine/src/game/map.js:9, 48-59). **GOTCHA**: the reachable window is `x ∈ [fromX-29, fromX+30]` (asymmetric). Targets outside it → the window is exhausted → `ERR_NO_PATH`.

### MAP-FINDROUTE — `Game.map.findRoute(from, to, opts)`
reference/engine/src/game/map.js:69-186:
```
if isObject(from): from = from.name; same for to
if from == to: return []
if !/(W|E)\d+(N|S)\d+$/.test(from) || !…test(to): return ERR_NO_PATH        // NB: not anchored at start
[fx,fy] = roomNameToXY(from); [tx,ty] = roomNameToXY(to)                     // tx,ty stored in closure
if fx==tx && fy==ty: return []
originX = fx+30; originY = fy+30
heap/openClosed: created once per Game.map object (per tick), cleared per call; parents Uint16Array(3600) never cleared
H(x,y) = |x-tx| + |y-ty|                                                       // Manhattan, unweighted
fromIndex = idx(fx,fy); heap.push(fromIndex, H(fx,fy))                         // start not marked open
cb = (opts && opts.routeCallback) || (() => 1)
while heap.size():
   i = heap.min(); f = heap.minPriority(); heap.pop(); close(i)
   [x,y] = xy(i); h = H(x,y); g = f - h                                        // f64 arithmetic
   if h === 0: return reconstruct(i)
   fromName = getRoomNameFromXY(x,y)
   for dir in describeExits(fromName) (keys ascending):                       // null → none
      name = exits[dir]; [nx,ny] = roomNameToXY(name); ni = idx(nx,ny)
      if ni === undefined || closed(ni): continue
      cost = Number(cb(name, fromName)) || 1                                   // called before open-check
      if cost === Infinity: continue
      nf = g + H(nx,ny) + cost
      if open(ni): if heap.priority(ni) > nf: heap.update(ni, nf); parents[ni] = i
      else: heap.push(ni, nf); open(ni); parents[ni] = i
return ERR_NO_PATH
reconstruct(i): route=[]; while i !== fromIndex: [x,y]=xy(i); i=parents[i]; [px,py]=xy(i)
   exit = px<x ? FIND_EXIT_RIGHT : px>x ? FIND_EXIT_LEFT : py<y ? FIND_EXIT_BOTTOM : FIND_EXIT_TOP
   route.push({exit, room: getRoomNameFromXY(x,y)}); return route.reverse()
```
The route excludes the start room and ends with the target room. The target can be a room absent from `gridData` as long as some accessible neighbour has an exit toward it.

### MAP-ROUTE-CALLBACK
`Number(r) || 1`: `0`, `NaN`, `undefined`, `false`, `''` → 1 (**GOTCHA**: returning 0 does not make a room free); `Infinity` blocks; `-Infinity` and negatives are accepted as-is (they break A*, but must be ported literally); floats are allowed. It runs once per (expanded room, non-closed in-window neighbour) edge, so possibly several times per room.

### MAP-FINDEXIT
`route = findRoute(from, to, opts)`; non-array → return it (`ERR_NO_PATH`); `[]` → `ERR_INVALID_ARGS`; else `route[0].exit` (reference/engine/src/game/map.js:188-197).

---

## 11. Processor-side (NPC) pathfinding

### NPC-PF-SETUP
The processor loads the same native module and terrain blob at startup (reference/driver/lib/index.js:113-116). Each `processRoom` calls `driver.pathFinder.make({RoomPosition: fakeRuntime.RoomPosition})`, so results are fake positions `{x, y, roomName}` (reference/engine/src/processor.js:174; reference/engine/src/processor/common/fake-runtime.js:8-19). Used by invaders, source keepers and stronghold defenders (reference/engine/src/processor/intents/creeps/invaders/findAttack.js:8-15; keepers/pretick.js:46; invader-core/stronghold/creeps.js:29-88). **PERF**: these searches are strictly room-local (maxRooms 1, other rooms blocked), so they parallelise per room.

### NPC-PF-CM
The fake `CostMatrix` has the same set/get/clone semantics as CM-* and no serialize (reference/engine/src/processor/common/fake-runtime.js:79-99). Stronghold "safe matrix": every tile is set to `Infinity` (→255), then ramparts → 1, then non-creep obstacle objects → 255 (reference/engine/src/processor/intents/invader-core/stronghold/defence.js:15-31).

### NPC-PF-COSTMATRIX — `defaultCostMatrix(roomId, opts, creep, roomObjects)`
reference/engine/src/processor/common/fake-runtime.js:104-141:
* `roomId != creep.room` → `false` (blocked).
* `obstacleTypes = OBSTACLE_OBJECT_TYPES + ['portal']`.
* `ignoreDestructibleStructures` removes `constructedWall, rampart, spawn, extension, link, storage, observer, tower, powerBank, powerSpawn, lab, terminal`.
* `ignoreCreeps` removes only `'creep'` (**GOTCHA**: power creeps stay obstacles).
* For each object in `_.forEach(roomObjects)` order: 255 if any of: obstacle type; non-public rampart with `user != creep.user` (unless `ignoreDestructibleStructures`); own obstacle construction site. Then a swamp object → 5/10. Then `!ignoreRoads && road && get < Infinity` → 1. **GOTCHA**: `get < Infinity` is always true, so a road processed **after** a creep or other obstacle on the same tile resets it to 1. The result depends on `roomObjects` order. This differs from the runtime version (`< 0xFF`).

### NPC-PF-FINDPATH — `findPath(source, target, opts, scope)`
reference/engine/src/processor/common/fake-runtime.js:143-182: `terrains = {[source.room]: roomTerrain}`. The room callback is `defaultCostMatrix` plus `opts.costCallback` (clone → call → use the result if it is a fake `CostMatrix`). `searchOpts = _.clone(opts)` with `maxRooms = 1` and the callback; `!ignoreRoads` → plain 2 / swamp 10. Calls `driver.pathFinder.search(new RoomPosition(source.x, source.y, source.room), target, searchOpts)`; `range` inside `opts` is not a PF option. If `target instanceof RoomPosition && !opts.range` and (the last position is at range 1 from target, or the path is empty and the source is at range 1) → push `target`.

### NPC-PF-FINDCLOSESTBYPATH
reference/engine/src/processor/common/fake-runtime.js:265-309: `!_.some(objects)` → `null`; `opts.range ??= 0` (mutates); an object at distance 0 → the first such; goals `{range:1}`. The result is the **last** object with `utils.dist(lastPos, obj) <= 1`, where `utils.dist` ignores rooms.

### NPC-PF-MOVE
* `packLocal(x,y) = String.fromCharCode(32 + (x<<6 | y))`; unpack checks `>= 32` (reference/engine/src/processor/common/fake-runtime.js:54-77).
* `moveTo`: `reusePath` default 5, `range` default 0 (mutates opts). `dist <= range` → 0. Recompute when there is no memory, the dest differs, or `gameTime > time + reusePath`. Store `memory_move {dest, path: packPath(path), time}` via bulk (reference/engine/src/processor/common/fake-runtime.js:200-245).
* `nextDirectionByPath` (reference/engine/src/processor/common/fake-runtime.js:311-332).
* `walkTo` swaps with a same-user idle creep ahead (reference/engine/src/processor/common/fake-runtime.js:247-263).
* `flee(creep, hostiles, range)`: goals `{pos, range}`, `{flee:true}` → direction to `path[0]` or 0 (reference/engine/src/processor/common/fake-runtime.js:185-198).

Full NPC AI behaviour belongs to the NPC spec.

### XREF-CREEP-MOVE
`Creep#moveTo` calls `pos.findPathTo(targetPos, opts)`, which clones opts. It caches `memory._move = {dest, time, path: serializeMemory ? serializePath(path) : clone, room}` with `reusePath` default 5. `moveByPath` accepts RoomPosition arrays, serialized strings or step arrays (reference/engine/src/game/creeps.js:180-333). Owned by the creep spec; it consumes PATHSER-* and ROOM-FINDPATH-NEW.

---

## 12. PERF summary (what is local, what is global, what the original does slowly)

* **Global read-only**: terrain (both encodings), `gridData`. One immutable `Arc` store; no locks. Supports up to 256×256 rooms with identical encodings (COORD-ROOMPOS-PACK).
* **Per call, cross-room (X)**: a native search touches at most `maxRooms` ≤ 64 rooms. Rooms load lazily through the user callback. Scratch state per thread ≈ `parents` 640 KB + `open_closed` 640 KB + heap priorities 640 KB + slots 80 KB + `reverse_room_table` 256 KB ≈ 2.2 MB (reference/driver/native/src/main.cc:9-11). Keep a reusable stack of contexts per worker thread with marker-based O(1) reset (PF-CORE-OPENCLOSED). Searches from different players are independent: run them on the player's own worker thread.
* **Hot paths**: `look()` (room lookup via a 65536-entry table, then matrix, then terrain bits). `heuristic()` is O(#goals) on every push **and every jump step**, which is slow for `findClosestByPath` with hundreds of goals; an acceleration must not change any returned value. `heap.update` is a linear scan; a position-index map would be fine but must keep identical sift behaviour.
* **Per player per tick (P)**: the default room CostMatrix (ROOM-DEFAULT-MATRIX) is rebuilt per player, per room, per flag combination by iterating every room object (`_.contains` on an array per object). It depends on the player (`user == me` for ramparts and construction sites, safe-mode ownership). Chitin can build one user-agnostic base per room per tick and apply a per-user overlay of the user-dependent tiles; results must equal the original exactly.
* **CPU**: in the original, PathFinder time is isolate CPU time ("1 op ≈ 0.001 CPU", reference/docs/api/source/PathFinder.md:110). Chitin's CPU model must charge it to the player (see the CPU spec).
* **findRoute**: cheap (≤ 3600 nodes) but calls user JS per edge.
* **Legacy pathfinder**: allocates a Node object per tile and clones the grid on every call. Only reachable on a global's first tick.
* **Processor NPC searches**: room-local; safe to run inside the room-parallel processor.

## 13. xxscreeps notes (moddability reference, NOT the 1:1 reference)

* xxscreeps ships a C++ port of the same algorithm (reference/xxscreeps/packages/pathfinder/src/jps.cc, heap.cc). Differences that break 1:1:
  * a room with missing terrain is **blocked** instead of throwing (reference/xxscreeps/packages/pathfinder/src/pf.cc:34-37);
  * its JS option layer does not clamp (reference/xxscreeps/packages/pathfinder/pathfinder.ts:77-84);
  * `Terrain#get` returns raw 0..3 (code 3 not normalised) and `NaN` out of range, packs terrain row-major, and `getRawBuffer` re-expands (reference/xxscreeps/packages/xxscreeps/game/terrain.ts:52-83).
* Useful design idea to keep: the PF core as a pure library — inputs are a terrain provider, a room callback returning `Blocked | Matrix(&[u8;2500]) | None`, and goals; outputs are the path and stats. Mods can then supply cost layers without touching the core.

## 14. Open questions

1. (Resolved) LEGACY-HEAP / LEGACY-HEAP-UPDATE are now verified against the installed `heap@0.2.5` (E:\screeps\server\node_modules\heap\lib\heap.js). The bundled minified copy agrees. As before, the legacy path is live only when `PathFinder.use(false)` runs on the first tick of a fresh global (PF-API-USE, reference/engine/src/game/path-finder.js:8-14, 67-75).
2. Native heap overflow (`"Max heap"`, ≥ 19 999 open nodes) throws a C++ exception that the `js_error` handler does not catch, and leaves `_is_in_use = true`. The effect inside node/isolated-vm (process abort? propagated error?) is not determinable from source. Chitin needs a policy, probably to throw a JS Error.
3. At the world edge (`W127`/`N127`, rx or ry = 0), `xx - 1` underflows u32 and the truncated `map_position` aliases another room. This is unreachable on maps that keep a walled margin. Chitin should treat out-of-world tiles as `OBST`; confirm that no real map reaches it. Related: `W127N127` is renamed `'sim'` (COORD-XY-TO-ROOMNAME).
4. `Nan::TypedArrayContents<uint8_t>` on non-Uint8 views (`_bits` as a Uint16Array of length 1250, etc.) — exact length/alignment semantics are not visible in the reference tree.
5. Whether the official MMO driver matches the public driver @cf63d8a (native v11) bit-for-bit is not determinable from source.
6. A non-flee search with **zero** goals is reachable only through direct driver calls (processor). It gives `h = u32::MAX`, then a `u32::MAX * w` double→u32 conversion, which is UB in C++. No engine call site produces it today (verify in the NPC spec).
7. PF-API-USE assumes the isolate global persists across ticks (true for the reference driver). Confirm chitin's global lifecycle matches, because it decides whether the legacy path is ever live.
8. Timing: the processor loads the terrain blob asynchronously after connect (reference/driver/lib/index.js:113-116); a search before the load finishes would throw. Not relevant to chitin if terrain loads before the first tick.
