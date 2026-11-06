# 03 — Player API: Room, RoomPosition, RoomObject & friends

Scope: the game-side (player sandbox) surface for `Room`, `Room.Terrain`, `RoomPosition`, `RoomObject`, `Flag`,
`ConstructionSite`, `Source`, `Mineral`, `Deposit`, `Nuke`, `Resource`, `Tombstone`, `Ruin`, `Store`,
`PathFinder` / `PathFinder.CostMatrix` (JS wrapper only; the native A*/JPS core is a separate spec),
`Game.map`, `RoomVisual` / `MapVisual`, and the `utils.defineGameObjectProperties` getter/caching mechanism.
Creep/Structure/PowerCreep methods, the processor, and the native pathfinder are out of scope except where
their behaviour is directly observable through this API (cited where used).

Reference commit: engine @8097782. All paths below are relative to `E:\chitin\` and written as
`reference/<pkg>/<path>:<line>`. Lodash is the engine's own `lodash@3.10.1` (`reference/engine/package.json:18`),
NOT the user's `_` (the user gets a separate `_.runInContext()` copy, `reference/engine/src/game/game.js:475-477`).

Legend: **GOTCHA** = subtle 1:1 pitfall a reimplementation will likely get wrong. **PERF** = performance-relevant
structure (room-local vs cross-room/global, hot paths, what the original does slowly).

---

## 0. INVENTORY

Scope column: `R` = room-local, `U` = user-global (all of one player's objects), `W` = world-global/static, `T` = tick-local cache, `-` = pure.

### 0.1 Core mechanisms

| ID | Item | Source | Scope |
|---|---|---|---|
| CORE-GAMEOBJ-GETTER | `utils.defineGameObjectProperties` lazy getter + truthy-only per-instance cache | reference/engine/src/utils.js:509 | - |
| CORE-GAMEOBJ-TOJSON | `toJSON` installed on prototypes by `defineGameObjectProperties` | reference/engine/src/utils.js:535 | - |
| CORE-REGISTER | Per-tick register / `Game` construction, eager instantiation of every visible object | reference/engine/src/game/game.js:50 | U |
| CORE-OBJECT-ORDER | Iteration order of `runtimeData.roomObjects` (drives every find/look order) | reference/driver/lib/runtime/data.js:209 | U |
| CORE-FIND-CACHE | `register.findCache[FIND_*][room]` population + predicates | reference/engine/src/game/game.js:15 | R,T |
| CORE-SPATIAL | Per-room spatial index `spatial[type][x*50+y]` | reference/engine/src/game/game.js:38 | R,T |
| CORE-FETCHXY | `utils.fetchXYArguments` overload resolution `(x,y)` vs `(target)` | reference/engine/src/utils.js:34 | - |
| CORE-DIRECTION | `utils.getDirection(dx,dy)` and direction offset table | reference/engine/src/utils.js:65 | - |
| CORE-ROOMNAME | `utils.roomNameToXY` / `utils.getRoomNameFromXY` (+ memoised override, `sim`) | reference/engine/src/utils.js:358 | - |
| CORE-INTENT-CPU | `intents.pushByName` / `set` and the 0.2 CPU intent surcharge | reference/driver/lib/runtime/runtime.js:59 | U |
| CORE-ROOM-INTENTS | Room-level intent shapes, sanitisation, grouping, processor order | reference/common/lib/system.js:18 | R |
| CORE-DEPRECATED | `register.deprecated(msg)` once-per-tick console line | reference/engine/src/game/game.js:115 | T |
| CORE-FILTER | lodash 3.10.1 `_.filter` predicate semantics (function / object / string) | reference/lodash-3.10.1/lodash.js:1654 | - |
| CORE-STALE | Behaviour of game objects kept across ticks (global cache) | reference/engine/src/game/rooms.js:376 | - |
| CORE-TERRAIN-DATA | `staticTerrainData` layout (row-major Uint8 per room) and lifetime | reference/driver/lib/runtime/make.js:41 | W |
| CORE-VISIBILITY | Which rooms get a `Room` object (`runtimeData.rooms`) | reference/driver/lib/runtime/data.js:113 | U |

### 0.2 Room

| ID | Item | Source | Scope |
|---|---|---|---|
| ROOM-CTOR | `Room` constructor (internal), per-room private store | reference/engine/src/game/rooms.js:419 | R |
| ROOM-NAME | `room.name` | reference/engine/src/game/rooms.js:429 | - |
| ROOM-ENERGY | `room.energyAvailable`, `room.energyCapacityAvailable` | reference/engine/src/game/game.js:325 | R |
| ROOM-SINGLETONS | `room.controller`, `room.storage`, `room.terminal` | reference/engine/src/game/structures.js:45 | R |
| ROOM-VISUAL-PROP | `room.visual` | reference/engine/src/game/rooms.js:525 | - |
| ROOM-SURVIVALINFO | `room.survivalInfo` (undocumented) | reference/engine/src/game/rooms.js:421 | - |
| ROOM-MEMORY | `room.memory` getter/setter | reference/engine/src/game/rooms.js:551 | U |
| ROOM-TOSTRING | `room.toString()` | reference/engine/src/game/rooms.js:536 | - |
| ROOM-TOJSON | `room.toJSON()` | reference/engine/src/game/rooms.js:540 | - |
| ROOM-FIND | `room.find(type, [opts])` | reference/engine/src/game/rooms.js:584 | R,T |
| ROOM-FIND-EXIT | Exit-position computation & the FIND_EXIT cache-pollution bug | reference/engine/src/game/rooms.js:592 | R,T |
| ROOM-LOOKAT | `room.lookAt(x,y \| target)` | reference/engine/src/game/rooms.js:768 | R |
| ROOM-LOOKFORAT | `room.lookForAt(type, x,y \| target)` | reference/engine/src/game/rooms.js:798 | R |
| ROOM-LOOKATAREA | `room.lookAtArea(top,left,bottom,right,[asArray])` | reference/engine/src/game/rooms.js:808 | R |
| ROOM-LOOKFORATAREA | `room.lookForAtArea(type,top,left,bottom,right,[asArray])` | reference/engine/src/game/rooms.js:847 | R |
| ROOM-FINDPATH | `room.findPath(from,to,[opts])` (PathFinder mode, default) | reference/engine/src/game/rooms.js:863 | R (cross-room capable) |
| ROOM-FINDPATH-LEGACY | `room.findPath` with `PathFinder.use(false)` | reference/engine/src/game/rooms.js:873 | R |
| ROOM-DEFAULT-COSTMATRIX | Built-in per-room CostMatrix (`getPathfindingGrid2`) | reference/engine/src/game/rooms.js:157 | R,T |
| ROOM-GETPOSITIONAT | `room.getPositionAt(x,y)` | reference/engine/src/game/rooms.js:971 | - |
| ROOM-CREATEFLAG | `room.createFlag(...)` | reference/engine/src/game/rooms.js:978 | U |
| ROOM-CREATECS | `room.createConstructionSite(...)` | reference/engine/src/game/rooms.js:1029 | R+U |
| ROOM-GETENDNODES | `room.getEndNodes(type,opts)` (undocumented, legacy helper) | reference/engine/src/game/rooms.js:1099 | R,T |
| ROOM-FINDEXITTO | `room.findExitTo(room)` | reference/engine/src/game/rooms.js:1130 | W |
| ROOM-GETTERRAIN | `room.getTerrain()` | reference/engine/src/game/rooms.js:1134 | W |
| ROOM-GETEVENTLOG | `room.getEventLog([raw])` | reference/engine/src/game/rooms.js:573 | R,T |
| ROOM-SERIALIZEPATH | `Room.serializePath(path)` | reference/engine/src/utils.js:555 | - |
| ROOM-DESERIALIZEPATH | `Room.deserializePath(str)` | reference/engine/src/utils.js:576 | - |
| TERRAIN-CTOR | `new Room.Terrain(roomName)` | reference/engine/src/game/rooms.js:1223 | W |
| TERRAIN-GET | `terrain.get(x,y)` | reference/engine/src/game/rooms.js:1230 | W |
| TERRAIN-GETRAWBUFFER | `terrain.getRawBuffer([dest])` | reference/engine/src/game/rooms.js:1235 | W |

### 0.3 FIND_* constants (each row in §3 is the spec)

| ID | Constant = value | Source (predicate / population) |
|---|---|---|
| FIND-EXIT-TOP | `FIND_EXIT_TOP = 1` | reference/engine/src/game/rooms.js:599 |
| FIND-EXIT-RIGHT | `FIND_EXIT_RIGHT = 3` | reference/engine/src/game/rooms.js:600 |
| FIND-EXIT-BOTTOM | `FIND_EXIT_BOTTOM = 5` | reference/engine/src/game/rooms.js:601 |
| FIND-EXIT-LEFT | `FIND_EXIT_LEFT = 7` | reference/engine/src/game/rooms.js:602 |
| FIND-EXIT | `FIND_EXIT = 10` | reference/engine/src/game/rooms.js:592 |
| FIND-CREEPS | `FIND_CREEPS = 101` | reference/engine/src/game/game.js:16, :282 |
| FIND-MY-CREEPS | `FIND_MY_CREEPS = 102` | reference/engine/src/game/game.js:17, :283 |
| FIND-HOSTILE-CREEPS | `FIND_HOSTILE_CREEPS = 103` | reference/engine/src/game/game.js:18, :284 |
| FIND-SOURCES-ACTIVE | `FIND_SOURCES_ACTIVE = 104` | reference/engine/src/game/game.js:23, :333 |
| FIND-SOURCES | `FIND_SOURCES = 105` | reference/engine/src/game/game.js:332 |
| FIND-DROPPED-RESOURCES | `FIND_DROPPED_RESOURCES = 106` | reference/engine/src/game/game.js:348 |
| FIND-STRUCTURES | `FIND_STRUCTURES = 107` | reference/engine/src/game/game.js:309 |
| FIND-MY-STRUCTURES | `FIND_MY_STRUCTURES = 108` | reference/engine/src/game/game.js:24, :310 |
| FIND-HOSTILE-STRUCTURES | `FIND_HOSTILE_STRUCTURES = 109` | reference/engine/src/game/game.js:25, :311 |
| FIND-FLAGS | `FIND_FLAGS = 110` | reference/engine/src/game/game.js:423 |
| FIND-CONSTRUCTION-SITES | `FIND_CONSTRUCTION_SITES = 111` | reference/engine/src/game/game.js:378 |
| FIND-MY-SPAWNS | `FIND_MY_SPAWNS = 112` | reference/engine/src/game/game.js:21, :320 |
| FIND-HOSTILE-SPAWNS | `FIND_HOSTILE_SPAWNS = 113` | reference/engine/src/game/game.js:22, :321 |
| FIND-MY-CONSTRUCTION-SITES | `FIND_MY_CONSTRUCTION_SITES = 114` | reference/engine/src/game/game.js:26, :379 |
| FIND-HOSTILE-CONSTRUCTION-SITES | `FIND_HOSTILE_CONSTRUCTION_SITES = 115` | reference/engine/src/game/game.js:27, :380 |
| FIND-MINERALS | `FIND_MINERALS = 116` | reference/engine/src/game/game.js:338 |
| FIND-NUKES | `FIND_NUKES = 117` | reference/engine/src/game/game.js:353 |
| FIND-TOMBSTONES | `FIND_TOMBSTONES = 118` | reference/engine/src/game/game.js:358 |
| FIND-POWER-CREEPS | `FIND_POWER_CREEPS = 119` | reference/engine/src/game/game.js:294 |
| FIND-MY-POWER-CREEPS | `FIND_MY_POWER_CREEPS = 120` | reference/engine/src/game/game.js:19, :295 |
| FIND-HOSTILE-POWER-CREEPS | `FIND_HOSTILE_POWER_CREEPS = 121` | reference/engine/src/game/game.js:20, :296 |
| FIND-DEPOSITS | `FIND_DEPOSITS = 122` | reference/engine/src/game/game.js:343 |
| FIND-RUINS | `FIND_RUINS = 123` | reference/engine/src/game/game.js:363 |

Constant values: reference/common/lib/constants.js:21-48.

### 0.4 LOOK_* constants

| ID | Constant = value | Register | Source |
|---|---|---|---|
| LOOK-CREEPS | `LOOK_CREEPS = "creep"` | `byRoom.creeps` | reference/engine/src/game/rooms.js:486 |
| LOOK-ENERGY | `LOOK_ENERGY = "energy"` | `byRoom.energy` | reference/engine/src/game/rooms.js:487 |
| LOOK-RESOURCES | `LOOK_RESOURCES = "resource"` | `byRoom.energy` (same!) | reference/engine/src/game/rooms.js:488 |
| LOOK-SOURCES | `LOOK_SOURCES = "source"` | `byRoom.sources` | reference/engine/src/game/rooms.js:489 |
| LOOK-MINERALS | `LOOK_MINERALS = "mineral"` | `byRoom.minerals` | reference/engine/src/game/rooms.js:490 |
| LOOK-DEPOSITS | `LOOK_DEPOSITS = "deposit"` | `byRoom.deposits` | reference/engine/src/game/rooms.js:491 |
| LOOK-STRUCTURES | `LOOK_STRUCTURES = "structure"` | `byRoom.structures` | reference/engine/src/game/rooms.js:492 |
| LOOK-FLAGS | `LOOK_FLAGS = "flag"` | `byRoom.flags` | reference/engine/src/game/rooms.js:493 |
| LOOK-CONSTRUCTION-SITES | `LOOK_CONSTRUCTION_SITES = "constructionSite"` | `byRoom.constructionSites` | reference/engine/src/game/rooms.js:494 |
| LOOK-NUKES | `LOOK_NUKES = "nuke"` | `byRoom.nukes` | reference/engine/src/game/rooms.js:497 |
| LOOK-TERRAIN | `LOOK_TERRAIN = "terrain"` | static terrain | reference/engine/src/game/rooms.js:663 |
| LOOK-TOMBSTONES | `LOOK_TOMBSTONES = "tombstone"` | `byRoom.tombstones` | reference/engine/src/game/rooms.js:495 |
| LOOK-POWER-CREEPS | `LOOK_POWER_CREEPS = "powerCreep"` | `byRoom.powerCreeps` | reference/engine/src/game/rooms.js:498 |
| LOOK-RUINS | `LOOK_RUINS = "ruin"` | `byRoom.ruins` | reference/engine/src/game/rooms.js:496 |

Constant values: reference/common/lib/constants.js:70-83.

### 0.5 RoomPosition / RoomObject

| ID | Item | Source | Scope |
|---|---|---|---|
| POS-CTOR | `new RoomPosition(x,y,roomName)` | reference/engine/src/game/rooms.js:1276 | - |
| POS-PACKED | `__packedPos` int32 packing, world-size limits | reference/engine/src/game/rooms.js:1288 | - |
| POS-X | `pos.x` getter/setter | reference/engine/src/game/rooms.js:1296 | - |
| POS-Y | `pos.y` getter/setter | reference/engine/src/game/rooms.js:1309 | - |
| POS-ROOMNAME | `pos.roomName` getter/setter | reference/engine/src/game/rooms.js:1322 | - |
| POS-TOJSON | `pos.toJSON()` | reference/engine/src/game/rooms.js:1350 | - |
| POS-TOSTRING | `pos.toString()` | reference/engine/src/game/rooms.js:1358 | - |
| POS-INRANGETO | `pos.inRangeTo(x,y,range \| target,range)` | reference/engine/src/game/rooms.js:1362 | - |
| POS-ISNEARTO | `pos.isNearTo(x,y \| target)` | reference/engine/src/game/rooms.js:1378 | - |
| POS-GETDIRECTIONTO | `pos.getDirectionTo(x,y \| target)` | reference/engine/src/game/rooms.js:1383 | - |
| POS-FINDPATHTO | `pos.findPathTo(x,y,[opts] \| target,[opts])` | reference/engine/src/game/rooms.js:1396 | R (cross-room capable) |
| POS-FINDCLOSESTBYPATH | `pos.findClosestByPath(type\|objects,[opts])` (PathFinder mode) | reference/engine/src/game/rooms.js:304, :1429 | R |
| POS-FINDCLOSESTBYPATH-LEGACY | same, `PathFinder.use(false)` | reference/engine/src/game/rooms.js:1447 | R |
| POS-FINDINRANGE | `pos.findInRange(type\|objects, range, [opts])` | reference/engine/src/game/rooms.js:1518 | R |
| POS-FINDCLOSESTBYRANGE | `pos.findClosestByRange(type\|objects,[opts])` | reference/engine/src/game/rooms.js:1546 | R |
| POS-ISEQUALTO | `pos.isEqualTo(x,y \| target)` | reference/engine/src/game/rooms.js:1578 | - |
| POS-GETRANGETO | `pos.getRangeTo(x,y \| target)` | reference/engine/src/game/rooms.js:1586 | - |
| POS-LOOK | `pos.look()` | reference/engine/src/game/rooms.js:1594 | R |
| POS-LOOKFOR | `pos.lookFor(type)` | reference/engine/src/game/rooms.js:1602 | R |
| POS-CREATEFLAG | `pos.createFlag([name],[color],[secondaryColor])` | reference/engine/src/game/rooms.js:1622 | U |
| POS-CREATECS | `pos.createConstructionSite(structureType,[name])` | reference/engine/src/game/rooms.js:1630 | R+U |
| ROOMOBJ-POS | `roomObject.pos` | reference/engine/src/game/rooms.js:1650 | - |
| ROOMOBJ-ROOM | `roomObject.room` | reference/engine/src/game/rooms.js:1649 | - |
| ROOMOBJ-EFFECTS | `roomObject.effects` | reference/engine/src/game/rooms.js:1651 | - |

### 0.6 Simple room objects & Store

| ID | Item | Source |
|---|---|---|
| FLAG-LOAD | Building `Game.flags` / flag registers from `rooms.flags` data strings | reference/engine/src/game/game.js:397 |
| FLAG-PROPS | `Flag` ctor; `name`, `color`, `secondaryColor` | reference/engine/src/game/flags.js:20 |
| FLAG-MEMORY | `flag.memory` | reference/engine/src/game/flags.js:31 |
| FLAG-TOSTRING | `flag.toString()` | reference/engine/src/game/flags.js:53 |
| FLAG-REMOVE | `flag.remove()` | reference/engine/src/game/flags.js:57 |
| FLAG-SETPOSITION | `flag.setPosition(x,y \| pos)` | reference/engine/src/game/flags.js:63 |
| FLAG-SETCOLOR | `flag.setColor(color,[secondaryColor])` | reference/engine/src/game/flags.js:76 |
| CS-PROPS | `ConstructionSite` id/progress/progressTotal/structureType/name | reference/engine/src/game/construction-sites.js:27 |
| CS-OWNER-MY | `constructionSite.owner`, `.my` | reference/engine/src/game/construction-sites.js:41 |
| CS-REMOVE | `constructionSite.remove()` | reference/engine/src/game/construction-sites.js:50 |
| CS-TOSTRING | `constructionSite.toString()` | reference/engine/src/game/construction-sites.js:46 |
| SOURCE-PROPS | `Source` energy/energyCapacity/ticksToRegeneration/toString | reference/engine/src/game/sources.js:26 |
| MINERAL-PROPS | `Mineral` mineralType/mineralAmount/density/ticksToRegeneration/toString | reference/engine/src/game/minerals.js:26 |
| DEPOSIT-PROPS | `Deposit` depositType/cooldown/lastCooldown/ticksToDecay/toString | reference/engine/src/game/deposits.js:24 |
| NUKE-PROPS | `Nuke` timeToLand/launchRoomName/toString | reference/engine/src/game/nukes.js:26 |
| RESOURCE-PROPS | `Resource` amount/resourceType/energy/toString | reference/engine/src/game/resources.js:26 |
| RESOURCE-ENERGY-ALIAS | global `Energy === Resource` | reference/engine/src/game/resources.js:46 |
| TOMB-PROPS | `Tombstone` deathTime/store/ticksToDecay/toString | reference/engine/src/game/tombstones.js:33 |
| TOMB-CREEP | `tombstone.creep` synthetic Creep/PowerCreep | reference/engine/src/game/tombstones.js:45 |
| RUIN-PROPS | `Ruin` destroyTime/ticksToDecay/store/structureType/toString | reference/engine/src/game/ruins.js:29 |
| RUIN-STRUCTURE | `ruin.structure` synthetic Structure/OwnedStructure | reference/engine/src/game/ruins.js:43 |
| STORE-CTOR | `new Store(rawObject)` (global), own resource keys | reference/engine/src/game/store.js:19 |
| STORE-INDEX | Proxy `store[resource]` (0 for known resources) | reference/engine/src/game/store.js:87 |
| STORE-GETCAPACITY | `store.getCapacity([resource])` | reference/engine/src/game/store.js:28 |
| STORE-GETUSEDCAPACITY | `store.getUsedCapacity([resource])` | reference/engine/src/game/store.js:36 |
| STORE-GETFREECAPACITY | `store.getFreeCapacity([resource])` | reference/engine/src/game/store.js:52 |
| STORE-CAPFORRES | `utils.capacityForResource(raw, resource)` | reference/engine/src/utils.js:671 |

### 0.7 PathFinder, Game.map, visuals, mod hooks

| ID | Item | Source | Scope |
|---|---|---|---|
| PF-SEARCH | `PathFinder.search(origin, goal, [opts])` game wrapper | reference/engine/src/game/path-finder.js:57 | W |
| PF-SEARCH-OPTS | Driver-side option coercion & goal normalisation | reference/driver/lib/path-finder.js:86 | - |
| PF-SEARCH-RESULT | Result object shapes (`path/ops/cost/incomplete`) | reference/driver/lib/path-finder.js:134 | - |
| PF-USE | `PathFinder.use(isActive)` | reference/engine/src/game/path-finder.js:67 | T |
| CM-CTOR | `new PathFinder.CostMatrix()` | reference/engine/src/game/path-finder.js:18 | - |
| CM-SET | `costMatrix.set(x,y,cost)` | reference/engine/src/game/path-finder.js:22 | - |
| CM-GET | `costMatrix.get(x,y)` | reference/engine/src/game/path-finder.js:28 | - |
| CM-CLONE | `costMatrix.clone()` | reference/engine/src/game/path-finder.js:34 | - |
| CM-SERIALIZE | `costMatrix.serialize()` | reference/engine/src/game/path-finder.js:40 | - |
| CM-DESERIALIZE | `PathFinder.CostMatrix.deserialize(data)` | reference/engine/src/game/path-finder.js:44 | - |
| MAP-GRID | World map grid (`gridData` exit counts) built once per runtime | reference/driver/lib/runtime/mapgrid.js:68 | W |
| MAP-DESCRIBEEXITS | `Game.map.describeExits(roomName)` | reference/engine/src/game/map.js:20 | W |
| MAP-FINDROUTE | `Game.map.findRoute(from,to,[opts])` | reference/engine/src/game/map.js:69 | W |
| MAP-FINDEXIT | `Game.map.findExit(from,to,[opts])` | reference/engine/src/game/map.js:188 | W |
| MAP-ISROOMAVAILABLE | `Game.map.isRoomAvailable(roomName)` (deprecated) | reference/engine/src/game/map.js:201 | W |
| MAP-GETROOMSTATUS | `Game.map.getRoomStatus(roomName)` | reference/engine/src/game/map.js:209 | W |
| MAP-GETTERRAINAT | `Game.map.getTerrainAt(x,y,room \| pos)` (deprecated) | reference/engine/src/game/map.js:235 | W |
| MAP-GETROOMTERRAIN | `Game.map.getRoomTerrain(roomName)` | reference/engine/src/game/map.js:261 | W |
| MAP-GETROOMLINEARDISTANCE | `Game.map.getRoomLinearDistance(r1,r2,[continuous])` | reference/engine/src/game/map.js:265 | - |
| MAP-GETWORLDSIZE | `Game.map.getWorldSize()` | reference/engine/src/game/map.js:269 | W |
| MAP-VISUAL-PROP | `Game.map.visual` lazy getter | reference/engine/src/game/map.js:274 | T |
| VIS-BUFFER | Per-user visual string buffers, size limits, flush | reference/engine/src/game/console.js:57 | U,T |
| RV-CTOR | `new RoomVisual([roomName])`, `.roomName` | reference/engine/src/game/rooms.js:1146 | - |
| RV-CIRCLE | `visual.circle(x,y,[style] \| pos,[style])` | reference/engine/src/game/rooms.js:1150 | - |
| RV-LINE | `visual.line(x1,y1,x2,y2,[style] \| p1,p2,[style])` | reference/engine/src/game/rooms.js:1160 | - |
| RV-RECT | `visual.rect(x,y,w,h,[style] \| pos,w,h,[style])` | reference/engine/src/game/rooms.js:1172 | - |
| RV-POLY | `visual.poly(points,[style])` | reference/engine/src/game/rooms.js:1184 | - |
| RV-TEXT | `visual.text(text,x,y,[style] \| text,pos,[style])` | reference/engine/src/game/rooms.js:1192 | - |
| RV-GETSIZE | `visual.getSize()` | reference/engine/src/game/rooms.js:1202 | - |
| RV-CLEAR | `visual.clear()` | reference/engine/src/game/rooms.js:1206 | - |
| RV-EXPORT | `visual.export()` | reference/engine/src/game/rooms.js:1211 | - |
| RV-IMPORT | `visual.import(str)` | reference/engine/src/game/rooms.js:1215 | - |
| MV-CIRCLE | `Game.map.visual.circle(pos,[style])` | reference/engine/src/game/map.js:281 | - |
| MV-LINE | `Game.map.visual.line(pos1,pos2,[style])` | reference/engine/src/game/map.js:293 | - |
| MV-RECT | `Game.map.visual.rect(pos,w,h,[style])` | reference/engine/src/game/map.js:309 | - |
| MV-POLY | `Game.map.visual.poly(points,[style])` | reference/engine/src/game/map.js:322 | - |
| MV-TEXT | `Game.map.visual.text(text,pos,[style])` | reference/engine/src/game/map.js:337 | - |
| MV-CLEAR | `Game.map.visual.clear()` | reference/engine/src/game/map.js:350 | - |
| MV-GETSIZE | `Game.map.visual.getSize()` | reference/engine/src/game/map.js:356 | - |
| MV-EXPORT | `Game.map.visual.export()` | reference/engine/src/game/map.js:361 | - |
| MV-IMPORT | `Game.map.visual.import(str)` | reference/engine/src/game/map.js:366 | - |
| MOD-CUSTOM-PROTO | `config.engine.registerCustomObjectPrototype` → custom classes, find/look constants | reference/driver/lib/index.js:56; reference/engine/src/game/custom-prototypes.js:8 | R |
| MOD-CUSTOM-INTENTS | `customIntentTypes` sanitisation of mod intents | reference/common/lib/system.js:105 | - |

Inventory total: 182 items.

---

## 1. Core mechanisms

### CORE-GAMEOBJ-GETTER — `defineGameObjectProperties(proto, dataFn, properties, opts)`
Source: reference/engine/src/utils.js:509-545.

For each `name` in `properties`, defines an accessor on `proto` (the *prototype*, not instances):
```
get():  if (!this['_' + name]) this['_' + name] = properties[name](dataFn(this.id), this.id)
        return this['_' + name]
set:    opts.canSet ? (v => this['_' + name] = v) : undefined
enumerable:   opts.enumerable === undefined ? true : !!opts.enumerable
configurable: !!opts.configurable   (default false)
```
- `dataFn(id)` returns the raw DB object `runtimeData.roomObjects[id]` or **throws** `Error("Could not find an object with ID "+id)` (e.g. reference/engine/src/game/sources.js:19-24).
- `properties[name]` is invoked as a method of the `properties` object (so `this` inside it is the properties literal, not the game object) with `(raw, id)`.
- **GOTCHA (cache only when truthy):** the cache test is `!this['_'+name]`; values `0`, `false`, `''`, `null`, `undefined`, `NaN` are recomputed on *every* access (re-calling `dataFn`). Truthy values are frozen for the life of the instance.
- **GOTCHA (visible cache keys):** the cache is a plain own enumerable data property named `_<name>` created by assignment on the instance. `Object.keys(obj)`/`for..in` show e.g. `_energy` after it was read with a truthy value. Several are read during register construction (see CORE-FIND-CACHE: `my`, `spawning`, `energy`, `owner`, `name`), so they may already exist before user code runs.
- **GOTCHA (assignment):** without `canSet` the accessor has no setter: `source.energy = 5` is silently ignored in sloppy code and throws `TypeError` in strict-mode user code. No call site in this area passes `canSet`.
- Property getters cannot be redefined by user code (`configurable:false`) but can be shadowed by an own property via `Object.defineProperty(instance, ...)` (the engine itself does that in TOMB-CREEP / RUIN-STRUCTURE).
- The original builds each accessor with `eval` of a template string; semantics are as above.

### CORE-GAMEOBJ-TOJSON
Source: reference/engine/src/utils.js:535-544. `defineGameObjectProperties` also assigns `proto.toJSON = function(){...}` (assignment → enumerable, overwritten each call with an identical function):
```
result = {}
for (i in this)                     // own + inherited enumerable, incl. prototype getters & methods
    if (i[0] == '_' || i == 'toJSON' || i == 'toString') continue
    result[i] = this[i]             // triggers getters (and their caching)
return result
```
Methods assigned as `Proto.prototype.m = fn` and `constructor` (assigned at e.g. reference/engine/src/game/sources.js:33) are enumerable, so they land in `result` as functions and are dropped by `JSON.stringify`. Order of keys = for..in order (own props in creation order: `room`, `pos`, [`effects`], `id`; then prototype chain). Objects without `defineGameObjectProperties` (Flag) use default JSON (own enumerable props).

### CORE-REGISTER — per-tick object graph
Source: reference/engine/src/game/game.js:50-438.

Every tick, per user, a fresh `register` and `Game` are built (`makeGameObject`). Steps in order:
1. `register = {_useNewPathFinder: true, _objects:{}, byRoom:{}, findCache:{}, rooms:{}, roomEventLogCache:{}, ...}` (:67-75).
2. `register.objectsByRoom[room][id] = raw` and `objectsByRoomKeys[room]` (array of ids) for every raw object without `temp` (:184-194). `temp` is never set anywhere in the reference tree (dead compatibility check).
3. `register.byRoom[room]` with empty per-type maps + `spatial[type] = new Array(2500)` for each room in `runtimeData.rooms` (:196-199, :77-111).
4. Module `make()` calls (rooms, pos, creeps, structures, ...) (:201-216). Each `make()` resets its per-tick module state (e.g. `privateStore`, `createdFlagNames`, `createdSpawnNames`, `createdConstructionSites` at reference/engine/src/game/rooms.js:383-387) then returns early if the global class already exists (classes persist in the isolate across ticks; closures read the module-level variables re-bound each tick).
5. `register.rooms[name] = new Room(name)` for each visible room (:218-220); `Game.rooms = _.clone(register.rooms)` (:227).
6. Power creeps of the user (:255-258), then **one pass over `runtimeData.roomObjects` in key order** (:260-395), instantiating every object and calling `addObjectToRegister` (maps + spatial) and `addObjectToFindCache`.
7. Flags (:397-429), then `Game.map` (:431), `Game.market`, then all constants copied onto globals (:435).

**PERF:** This is eager: every visible object of every visible room is instantiated and indexed every tick for every user, plus predicate getters are evaluated. With 1000 players this is the dominant API cost of the original. A lazy implementation is allowed only if it preserves (a) ordering (CORE-OBJECT-ORDER), (b) the `_name` cache side effects that are observable (CORE-GAMEOBJ-GETTER), (c) `Game.getObjectById` identity (same instance returned by `find`, `look*`, `Game.*`).

### CORE-OBJECT-ORDER — order of everything
Source: reference/driver/lib/runtime/data.js:103-111, :136, :209; reference/driver/lib/index.js:331-337.

`runtimeData.roomObjects = _.extend(mapById(foreignObjects), userObjects)` where
- `foreignObjects` = DB query `{room ∈ roomIds, user ≠ me}` (neutral objects have no user → included here),
- `userObjects` = DB query `{user: me}` (all rooms, including construction sites in non-visible rooms).

So iteration order is: **all objects not owned by the player (neutral + other players), in DB return order, then all objects owned by the player (`user == me`), in DB return order.** Ids are 15-hex-char strings on the private server (reference/storage/lib/db.js:317-327) or 24-hex ObjectIds on the MMO; neither can be a JS array-index key, so object key order equals insertion order.

**GOTCHA:** this order is the order of `room.find(...)` results, of objects within a `lookAt` tile, of `lookAtArea` results, `findInRange` results, and decides ties in `findClosestByRange` (first wins) and `findClosestByPath` (last wins). Bots commonly do `find(...)[0]`. The DB order itself is storage-dependent (see Open questions); chitin must pick one deterministic order. Recommended: foreign-then-own as above, each group in object creation order (monotonic id/sequence), which matches MongoDB natural order in the common case.

`Game.rooms` key order = `runtimeData.rooms` = order of `db.rooms.find({_id: {$in: roomIds}})` (reference/driver/lib/runtime/data.js:135, :208).

### CORE-FIND-CACHE
Source: reference/engine/src/game/game.js:15-36, :260-429.

`addObjectToFindCache(register, type, inst, raw)` appends `inst` to `register.findCache[type][raw.room]` iff `findCacheFn[type]` is absent or returns truthy. Predicates (evaluated once at build time, on the instance → triggers getter caching):

| FIND | predicate |
|---|---|
| CREEPS | `!i.spawning` |
| MY_CREEPS | `!i.spawning && i.my` |
| HOSTILE_CREEPS | `!i.spawning && !i.my` |
| MY_POWER_CREEPS | `i.my` |
| HOSTILE_POWER_CREEPS | `!i.my` |
| MY_SPAWNS | `i.my === true` |
| HOSTILE_SPAWNS | `i.my === false` |
| SOURCES_ACTIVE | `i.energy > 0` |
| MY_STRUCTURES | `i.my === true` |
| HOSTILE_STRUCTURES | `i.my === false && i.owner` |
| MY_CONSTRUCTION_SITES | `i.my` |
| HOSTILE_CONSTRUCTION_SITES | `i.my === false` |
| all others | none (always added) |

Construction sites are added to find caches only if `runtimeData.rooms[raw.room]` exists (visible room) (:377-381). Flags use the same append logic keyed by the flag-data room (:423-427), but are only reachable through `Room` objects, which exist only for visible rooms.

The cache reflects tick-start state; objects created/removed during the tick by user intents are never reflected.

### CORE-SPATIAL
Source: reference/engine/src/game/game.js:38-48, :410-419; reference/engine/src/game/rooms.js:688.

`byRoom[room].spatial[type][x*50 + y]` = array of instances in insertion order (**column-major index `x*50+y`**, unlike terrain which is `y*50+x`). Types: `creeps, structures, ownedStructures, spawns, sources, energy, flags, constructionSites, minerals, deposits, tombstones, nukes, powerCreeps, ruins, customObjects` (+ custom `lookConstant` keys). Flags index with `(+x)*50 + (+y)` from the flag string.

### CORE-FETCHXY — `fetchXYArguments(firstArg, secondArg)`
Source: reference/engine/src/utils.js:34-63.
```
if secondArg === undefined || typeof secondArg is not number:        // lodash _.isNumber: NaN counts as number
    if firstArg is not an object (lodash _.isObject): return [undefined, undefined, undefined]
    if firstArg instanceof RoomPosition: x,y,roomName = firstArg.*
    if firstArg.pos instanceof RoomPosition: x,y,roomName = firstArg.pos.*     // overrides
else:
    x = firstArg; y = secondArg                                          // no type check on x
if x is NaN: x = undefined;  if y is NaN: y = undefined
return [x, y, roomName]
```
**GOTCHA:** plain objects `{x, y, roomName}` (e.g. from Memory) are NOT accepted (all undefined). Everything built on this (`isNearTo`, `isEqualTo` non-packed path, `getRangeTo`, `getDirectionTo`, `lookAt`, `lookForAt`, `createFlag`, `createConstructionSite`, `findPathTo`, `setPosition`) inherits that. `inRangeTo` does NOT use it (see POS-INRANGETO).

### CORE-DIRECTION — `getDirection(dx, dy)`
Source: reference/engine/src/utils.js:65-99; offsets reference/engine/src/utils.js:3.
```
adx=|dx|, ady=|dy|
if adx > 2*ady:  dx>0 ? RIGHT : LEFT
elif ady > 2*adx: dy>0 ? BOTTOM : TOP
else: dx>0&&dy>0 → BOTTOM_RIGHT; dx>0&&dy<0 → TOP_RIGHT; dx<0&&dy>0 → BOTTOM_LEFT; dx<0&&dy<0 → TOP_LEFT
      otherwise (dx=dy=0) → undefined
```
Offsets: `[_, [0,-1],[1,-1],[1,0],[1,1],[0,1],[-1,1],[-1,0],[-1,-1]]` for TOP(1)..TOP_LEFT(8). **GOTCHA:** `getDirection(0,0)` returns `undefined`; NaN inputs also fall through to `undefined`.

### CORE-ROOMNAME
Source: reference/engine/src/utils.js:358-392; override reference/engine/src/game/rooms.js:1260-1274.

`roomNameToXY(name)`:
```
xx = parseInt(name.substr(1), 10)
verticalPos = xx >= 100 ? 4 : xx >= 10 ? 3 : 2
yy = parseInt(name.substr(verticalPos + 1), 10)
if name[0] in {'W','w'}: xx = -xx - 1
if name[verticalPos] in {'N','n'}: yy = -yy - 1
return [xx, yy]
```
**GOTCHA (lenient):** any first letter other than W/w means E, any vertical letter other than N/n means S; trailing junk is ignored by `parseInt`; lowercase accepted; non-strings throw `TypeError` (`name.substr`). Invalid names yield `NaN`s.
`getRoomNameFromXY(x,y)`: `(x<0 ? 'W'+(-x-1) : 'E'+x) + (y<0 ? 'N'+(-y-1) : 'S'+y)`. After `makePos`, it is replaced by a memoised wrapper keyed `id = (x+128)<<8 | (y+128)` in a shared `roomNames[]` array with `roomNames[0] = 'sim'`. **GOTCHA:** `getRoomNameFromXY(-128,-128)` (= W127N127) returns `'sim'`.

### CORE-INTENT-CPU
Source: reference/driver/lib/runtime/runtime.js:59-100.
- `intents.pushByName(id, name, data, maxLen)` appends to `list[id][name]` and adds **0.2 CPU** (`intentCpu`) to `intents.cpu`, which is included in `Game.cpu.getUsed()` (:202-204). Used by all room-level intents here (`createFlag`, `removeFlag`, `createConstructionSite`, `removeConstructionSite`, `destroyStructure`).
- `intents.set(id, name, data)` (object intents) adds 0.2 only when `list[id][name]` was not already set and `name` is not `say`/`pull`.
- **GOTCHA:** `flag.setPosition` / `flag.setColor` push two intents each → 0.4 CPU.

### CORE-ROOM-INTENTS — shapes, sanitisation, processing order
Sources: reference/common/lib/system.js:15-25 (types), :84-93, :116-140; reference/engine/src/utils.js:405-436; reference/engine/src/processor/intents/room/intents.js:12-36.

Intent payloads pushed under `list.room[name]` (array). At end of tick they are sanitised field-by-field (`string` → `""+v`; `number` → `parseInt(v)`; `userString` → `(""+(v||"")).substring(0,100)`) and grouped by `roomName`:

| intent | fields (type) |
|---|---|
| `createFlag` | roomName(string), x(number), y(number), name(userString), color(number), secondaryColor(number) |
| `removeFlag` | roomName(string), name(userString) |
| `createConstructionSite` | roomName(string), x, y (number), structureType(string), name(userString) |
| `removeConstructionSite` | roomName(string), id(string) |
| `destroyStructure` | roomName(string), id(string) |

Processor applies a room's user intents in this fixed order: **all `removeFlag`, then all `createFlag`, then `createConstructionSite`, then `removeConstructionSite`, then `destroyStructure`** (reference/engine/src/processor/intents/room/intents.js:12-36). Consequences are listed under FLAG-* and ROOM-CREATECS. The processor re-validates (reference/engine/src/processor/intents/room/create-flag.js:6, create-construction-site.js:9) — that re-validation belongs to the processor spec.

### CORE-DEPRECATED
Source: reference/engine/src/game/game.js:113-120. `register.deprecated(msg)` prints `msg` via `console.log` the first time per tick (list lives in the per-tick Game object). Triggered by: `PathFinder.use(false)`, `avoid`/`ignore` opts in PathFinder mode, `Game.map.isRoomAvailable`, `Game.map.getTerrainAt`.

### CORE-FILTER — lodash 3.10.1 `_.filter(collection, predicate)`
Sources: reference/lodash-3.10.1/lodash.js:6306-6310 (filter), :1654-1670 (baseCallback), :2287-2303 (baseMatches), :2223-2259 (baseIsMatch), :3924-3932 (getMatchData), :3798 (equalObjects), :11434 (property).

`opts.filter` is passed as `predicate` with no `thisArg`:
- `function` → called `(value, index, collection)`; truthiness of the return value. **GOTCHA:** for `room.find`, `collection` is the *internal cached array* (CORE-FIND-CACHE); a predicate that mutates its third argument corrupts the cache for the rest of the tick.
- `null`/`undefined` → identity (but callers only call filter when `opts.filter` is truthy).
- `object` → `_.matches(source)`: pairs = own enumerable `[key, value, isStrictComparable(value)]` of `source`.
  - Exactly one pair and strict-comparable (primitive, not NaN/object): `obj[key] === value && (value !== undefined || key in obj)`.
  - Otherwise: every key must satisfy `key in obj` (prototype getters count) / `===` for strict pairs; non-strict pairs compared with `baseIsEqual(src, objValue, undefined, isLoose=true)` = recursive *partial* deep match (source keys only, `key in other`, constructor check skipped in loose mode). So `{owner: {username: 'Invader'}}` and `{pos: {x: 5}}` work.
- `string`/`number` → `_.property(path)`: truthiness of `obj[path]` (deep path for `'a.b'` / array paths).
Result is a new array in original order.

### CORE-STALE — objects kept across ticks
Classes and prototypes persist across ticks in the isolate; `runtimeData`, `register`, `privateStore` are module-level variables re-bound each tick (reference/engine/src/game/rooms.js:376-399). A game object stored in a global from tick N, read at tick N+1: cached (truthy, already-read) properties return tick-N values; uncached/falsy properties call `data(this.id)` against tick N+1's `runtimeData` (current values) or throw `Could not find an object with ID ...` if the object is gone/invisible. `obj.room` is the tick-N `Room`. **GOTCHA:** mixed-tick reads are observable; chitin must keep "getter reads current tick raw data by id" semantics rather than snapshotting.

### CORE-TERRAIN-DATA
Source: reference/driver/lib/runtime/make.js:20-51; reference/driver/lib/runtime/runtime.js:45-49.
All rooms' terrain is loaded once per runner process into one (Shared)ArrayBuffer; each room is a `Uint8Array(buffer, offset, 2500)` view, **row-major `y*50+x`**, values = the DB terrain digit (0 plain, 1 wall, 2 swamp, 3 wall+swamp). Static for the process lifetime. **PERF:** world-global, read-only, shared by all users; 2.5 KB per room.

### CORE-VISIBILITY
Source: reference/driver/lib/runtime/data.js:113-127.
Visible rooms (`runtimeData.rooms` → `Room` objects) = rooms of every user object except `flag` and `constructionSite`, plus each observer's `observeRoom`. Everything that requires `register.rooms[roomName]` throws `Error("Could not access room "+roomName)` for other rooms (see POS-*).

---

## 2. Room

### ROOM-CTOR
Source: reference/engine/src/game/rooms.js:419-526. Internal; user code never needs `new Room`. Sets own properties in this order: `name` (via `defineProperty`, enumerable, non-writable, non-configurable), `energyAvailable = 0`, `energyCapacityAvailable = 0`, `survivalInfo`, then `visual = new RoomVisual(name)`. Creates `privateStore[name]` holding: `pfGrid` (cost-matrix cache), `pfFinders`, `pfEndNodes`, `pfDijkstraFinder`, `pathCache`, `positionsSetCache`, `lookTypeRegisters` / `lookTypeSpatialRegisters` (LOOK_* → register maps, incl. custom `lookConstant`s, :516-523).
Other properties (`controller`, `storage`, `terminal`) are assigned later (ROOM-SINGLETONS).

### ROOM-NAME
`room.name` — own, enumerable, read-only (assignment ignored / TypeError in strict). Source :429-432.

### ROOM-ENERGY
Source: reference/engine/src/game/game.js:325-328. During the register pass, for each raw object with `!off && (type=='extension' || type=='spawn') && user == me`: `energyAvailable += raw.store.energy`, `energyCapacityAvailable += raw.storeCapacityResource.energy`. **GOTCHA:** only the *player's own* spawns/extensions count (0 in rooms of others), and `off` (inactive due to RCL) ones are excluded. Plain own data properties (writable).

### ROOM-SINGLETONS
Source: reference/engine/src/game/structures.js:45-55. Set as plain own properties by the `Structure` constructor when a structure of type `controller`/`storage`/`terminal` is instantiated in that room. Absent (undefined, key not present) otherwise. If several existed, the last instantiated wins. Ownership is not checked (an enemy storage is `room.storage`).

### ROOM-VISUAL-PROP
`room.visual = new RoomVisual(room.name)` (own, enumerable, writable). Source :525.

### ROOM-SURVIVALINFO
Source :421-437. `runtimeData.games[gameId]` where `gameId` is the room name or the suffix of `survival_(.*)`; `undefined` on normal worlds (own enumerable key with value undefined).

### ROOM-MEMORY
Source: reference/engine/src/game/rooms.js:551-571 (prototype accessor, non-enumerable).
```
get: if Memory.rooms === undefined || Memory.rooms === 'undefined': Memory.rooms = {}
     if !_.isObject(Memory.rooms): return undefined
     return Memory.rooms[name] = Memory.rooms[name] || {}        // writes {} if falsy
set: same init; if !_.isObject(Memory.rooms): throw Error('Could not set room memory')
     Memory.rooms[name] = value
```
Touching `Memory` triggers the lazy JSON parse of raw memory (reference/engine/src/game/game.js:479-500). **GOTCHA:** reading `room.memory` creates `Memory.rooms[name] = {}` when missing/falsy (including when it was `0`/`''`).

### ROOM-TOSTRING
`[room <name>]`. Source :536-538.

### ROOM-TOJSON
Source :540-549: for..in over the room (own + prototype enumerable), skipping keys starting with `_` and `toJSON`, `toString`, `controller`, `storage`, `terminal`. Prototype methods are enumerable (dropped by JSON); `memory` is non-enumerable. Effective JSON: `{"name","energyAvailable","energyCapacityAvailable","visual":{"roomName"}}` (`survivalInfo` dropped when undefined).

### ROOM-FIND — `room.find(type, [opts])`
Source: reference/engine/src/game/rooms.js:584-657. Docs: reference/docs/api/source/Room.md:196-272.
```
opts = opts || {}
if findCache[type] && findCache[type][name]: result = that array (shared)
else switch type:
    FIND_EXIT: see ROOM-FIND-EXIT (returns early, no filter applied!)
    FIND_EXIT_TOP/RIGHT/BOTTOM/LEFT: compute + cache (ROOM-FIND-EXIT)
    default: result = []                  // unknown/empty types: [] — never ERR_INVALID_ARGS
if opts.filter: result = _.filter(result, opts.filter)     // CORE-FILTER
else: result = _.clone(result)                              // shallow copy
return result
```
- Only `opts.filter` is read. Any `type` value is accepted (strings, undefined → `[]`).
- Results: tick-start snapshot, order per CORE-OBJECT-ORDER (FIND_FLAGS: FLAG-LOAD order).
- **PERF:** O(n) copy per call; the cache is per (type, room) per tick, built eagerly in CORE-REGISTER. Room-local.

### ROOM-FIND-EXIT
Source :592-645.
- `FIND_EXIT_{TOP,RIGHT,BOTTOM,LEFT}`: for i in 0..49 take edge tile (TOP: (i,0); BOTTOM: (i,49); LEFT: (0,i); RIGHT: (49,i)); include `room.getPositionAt(x,y)` iff `!(terrain & TERRAIN_MASK_WALL)` (plain *and swamp* count). Ascending i. The unfiltered array is cached in `findCache[type][room]`, then the normal filter/clone step runs.
- `FIND_EXIT` = `find(TOP,opts) ++ find(BOTTOM,opts) ++ find(RIGHT,opts) ++ find(LEFT,opts)` — order **TOP, BOTTOM, RIGHT, LEFT** — stored in `findCache[FIND_EXIT][room]`, returned as `_.clone` **without** applying `opts.filter` again.
- **GOTCHA (cache pollution):** the sub-finds receive `opts`, so the stored FIND_EXIT array is the *filtered* concatenation of the first call. Every later `find(FIND_EXIT)` in the same tick (with or without a filter) starts from that filtered list. Corner tiles appear in two sides' lists if not walls.

### ROOM-LOOKAT — `room.lookAt(x, y | target)`
Source: reference/engine/src/game/rooms.js:768-796, helper :659-705.
`[x,y] = fetchXYArguments(...)` (roomName of a target is **ignored** — **GOTCHA:** looking at a position of another room returns this room's tile). Then appends, in this fixed order: `creep, energy, resource, source, mineral, deposit, structure, flag, constructionSite, terrain, nuke, tombstone, ruin, powerCreep`, then custom `lookConstant`s in registration order. Each object → `{type: T, [T]: obj}`; terrain → `{type:'terrain', terrain:'plain'|'swamp'|'wall'}` (wall wins over swamp).
- `_lookSpatialRegister` for non-terrain types throws `Error('look coords are out of bounds')` if `x<0||y<0||x>49||y>49` (checked on the first type, `creep`). `undefined` coordinates pass the check (comparisons false) and find nothing; terrain for undefined → `'plain'`.
- **GOTCHA (duplicate resources):** `energy` and `resource` both read the same register, so every dropped resource appears **twice**: `{type:'energy', energy:r}` then `{type:'resource', resource:r}`.
- Includes spawning creeps (spatial index is unfiltered), unlike FIND_CREEPS.
- **PERF:** 14+ O(1) spatial lookups; room-local.

### ROOM-LOOKFORAT — `room.lookForAt(type, x, y | target)`
Source :798-806.
```
[x,y] = fetchXYArguments(firstArg, secondArg)
if type != 'terrain' && !(type in lookTypeSpatialRegisters): return ERR_INVALID_ARGS
return _lookSpatialRegister(name, type, x, y)   // terrain: [str]; else _.clone(tile) or []
```
- **GOTCHA:** invalid type returns the number `ERR_INVALID_ARGS` (-10), not an array; `in` also accepts inherited names (`'toString'`, `'constructor'`) which then yield `[]`.
- `LOOK_ENERGY` and `LOOK_RESOURCES` return identical lists.
- Terrain path has no bounds check: index `y*50+x` can alias (x=50,y=0 → tile (0,1)) or be out of range (→ `'plain'`). Non-terrain out of bounds → throws.

### ROOM-LOOKATAREA — `room.lookAtArea(top, left, bottom, right, [asArray])`
Source :808-845, helper `_lookAreaMixedRegister` :707-766.
```
result = asArray ? [] : {}
if !asArray: for y in top..bottom: result[y] = {}; for x in left..right: result[y][x] = []
for T in [creep, energy, resource, source, mineral, deposit, structure, flag, constructionSite,
          terrain, nuke, tombstone, ruin, powerCreep, ...custom]:
    _lookAreaMixedRegister(name, T, top, left, bottom, right, withType=true, asArray, result)
```
`_lookAreaMixedRegister` chooses a strategy **per type**:
- **by-objects** if `T != 'terrain' && count(objects of T in room) < (bottom-top+1)*(right-left+1)`: iterate the type's room register in insertion order; for objects inside the box (room name check + inclusive bounds): asArray → push `{x, y, type:T, [T]:obj}`; else push `{type:T,[T]:obj}` into `result[y][x]`. No bounds validation.
- **spatial** otherwise: for y in top..bottom, x in left..right (row-major): asArray → push `{type:T, [T]:obj, x, y}` per object (terrain: `{type:'terrain', terrain, x, y}`); else append `{type,[T]}` items into the pre-created `result[y][x]`.
- **GOTCHA (ordering):** asArray output is **type-major** (all creeps, then all energy, ..., all terrain in the middle, ...), not position-major as the docs example suggests (reference/docs/api/source/Room.md:756-768). Within a type it is object order (by-objects) or row-major position order (spatial) — the switch depends on how many objects of that type the room has.
- **GOTCHA (exceptions):** spatial mode throws `'look coords are out of bounds'` for any out-of-range coordinate; by-objects never throws. So `lookAtArea(y-1, x-1, y+1, x+1)` at a room edge throws or not depending on per-type object counts (e.g. ≥9 structures in the room ⇒ spatial ⇒ throws). Terrain never throws: out-of-range coordinates read aliased/undefined terrain (`'plain'` for undefined).
- **GOTCHA (key order):** by-objects asArray entries have keys `x, y, type, <T>`; spatial ones `type, <T>, x, y` (visible in JSON).
- Dropped resources appear twice (energy + resource), as in lookAt.
- **PERF:** room-local; by-objects is O(objects of type), spatial O(area).

### ROOM-LOOKFORATAREA — `room.lookForAtArea(type, top, left, bottom, right, [asArray])`
Source :847-860.
```
result = asArray ? [] : {}; if !asArray: for y in top..bottom: result[y] = {}
_lookAreaMixedRegister(name, type, top, left, bottom, right, withType=false, asArray, result)
```
- No type validation: an unknown non-terrain `type` throws `TypeError` (reads `.length` of undefined keys at :711).
- by-objects mode: asArray → `{x, y, [type]: obj}` (no `type` key); nested → `result[y][x]` created lazily as `[]` and objects pushed (only tiles with objects get keys).
- spatial mode: asArray → `{type, [type]: obj, x, y}` (**has** a `type` key); nested → `result[y][x] = _.clone(tile) || []` for **every** tile in the box (empty tiles get `[]`).
- terrain: always spatial: asArray `{type:'terrain', terrain, x, y}`; nested `result[y][x] = ['plain'|...]`.
- **GOTCHA:** shape differences between the two modes (presence of `type` key; presence of empty-tile keys) are observable and depend on the per-room object count vs area size. Docs show `undefined` for empty cells (reference/docs/api/source/Room.md:836-850), which matches by-objects mode only.

### ROOM-FINDPATH — `room.findPath(fromPos, toPos, [opts])` (PathFinder mode)
Source: reference/engine/src/game/rooms.js:863-871, :223-302. Docs: reference/docs/api/source/Room.md:311-440.
```
if fromPos.roomName != this.name: return opts.serialize ? '' : []    // TypeError if opts undefined!
if register._useNewPathFinder (default true): return _findPath2(name, fromPos, toPos, opts)

_findPath2(id, fromPos, toPos, opts):
  opts = opts || {}                                   // the caller's object is MUTATED below
  if fromPos.isEqualTo(toPos): return serialize ? '' : []
  if opts.avoid:  deprecated('`avoid` option cannot be used when `PathFinder.use()` is enabled. Use `costCallback` instead.'); opts.avoid = undefined
  if opts.ignore: deprecated(... '`ignore` ...'); opts.ignore = undefined
  if opts.maxOps === undefined && (opts.maxRooms === undefined || opts.maxRooms > 1)
     && fromPos.roomName != toPos.roomName: opts.maxOps = 20000
  searchOpts = { roomCallback(roomName): m = getPathfindingGrid2(roomName, opts)   // ROOM-DEFAULT-COSTMATRIX
                     if typeof opts.costCallback == 'function':
                         m = m.clone(); r = opts.costCallback(roomName, m)
                         if r instanceof PathFinder.CostMatrix: m = r
                     return m,
                 maxOps: opts.maxOps, maxRooms: opts.maxRooms }
  if !opts.ignoreRoads: searchOpts.plainCost = 2; searchOpts.swampCost = 10
  if opts.plainCost: searchOpts.plainCost = opts.plainCost      // truthy only
  if opts.swampCost: searchOpts.swampCost = opts.swampCost
  ret = PathFinder.search(fromPos, {range: max(1, opts.range || 0), pos: toPos}, searchOpts)
  if !opts.range && ( (ret.path.length && last(ret.path).isNearTo(toPos) && !last.isEqualTo(toPos))
                      || (!ret.path.length && fromPos.isNearTo(toPos)) ):
      ret.path.push(toPos)
  steps = []; (cx,cy) = fromPos
  for pos in ret.path: if pos.roomName != id: break
      steps.push({x:pos.x, y:pos.y, dx:pos.x-cx, dy:pos.y-cy, direction: getDirection(dx,dy)}); (cx,cy)=pos
  return opts.serialize ? serializePath(steps) : steps
```
- **GOTCHA (range ≥ 1 then append):** the search always uses range ≥ 1 and then appends `toPos` itself when `opts.range` is falsy — so the returned path ends ON the target even if the target tile is an obstacle (wall, structure, creep). With `range: n>0` no append.
- **GOTCHA (opts mutation):** `room.findPath` does not clone `opts` in this mode: `avoid`/`ignore` are set to `undefined` and `maxOps` may be set to 20000 on the caller's object (persists if reused). `pos.findPathTo(target, opts)` clones; `pos.findPathTo(x, y, opts)` does not (POS-FINDPATHTO).
- **GOTCHA (ignored options):** `heuristicWeight`, `flee`, `maxCost` are not forwarded (native default weight 1.2 applies). `plainCost`/`swampCost` only override when truthy. With `ignoreRoads`, base costs fall to the native defaults 1/5.
- Multi-room: the path is truncated at the first position outside this room (so it ends at the exit tile). `toPos` may be in another room.
- `costCallback` is called for every room the search opens, including non-visible rooms (which get a fresh empty matrix), at most once per room per search (native contract).
- Empty-goal / same-tile handling: see PF-SEARCH-RESULT.
- **PERF:** cost-matrix cache per (room, ignoreCreeps, ignoreDestructibleStructures, ignoreRoads) per tick; `costCallback` forces a 2500-byte clone per room per call.

### ROOM-DEFAULT-COSTMATRIX — `getPathfindingGrid2(roomName, opts)`
Source: reference/engine/src/game/rooms.js:157-221.
- If `privateStore[roomName]` missing (room not visible): return `new CostMatrix()` (not cached).
- Cache key `'grid2' + (ignoreCreeps?'_ignoreCreeps':'') + (ignoreDestructibleStructures?'_ignoreDestructibleStructures':'') + (ignoreRoads?'_ignoreRoads':'')`; cached instance is returned directly (not cloned).
- Build (`makePathfindingGrid2`): obstacle types = `OBSTACLE_OBJECT_TYPES` (reference/common/lib/constants.js:85) + `'portal'`.
  - `ignoreDestructibleStructures` removes `constructedWall, spawn, extension, link, storage, observer, tower, powerBank, powerSpawn, lab, terminal` (**GOTCHA:** `nuker`, `factory`, `invaderCore`, `controller`, sources/minerals/deposits, portals stay obstacles).
  - `ignoreCreeps` OR (room controller `safeMode` truthy AND controller `my`) removes `creep, powerCreep`.
  - For each raw object in `objectsByRoomKeys[room]` order: set **0xFF** if type is an obstacle type, OR (`!ignoreCreeps` && own safe-mode room && creep/powerCreep && `user == me`) — own creeps still block in own safe mode, hostile don't —, OR (`!ignoreDestructibleStructures` && rampart && `!isPublic` && `user != me`), OR (`!ignoreDestructibleStructures` && constructionSite && `user == me` && `structureType ∈ OBSTACLE_OBJECT_TYPES`).
  - `type == 'swamp'` branch (cost 5/10) is dead code (no such room objects).
  - If `!ignoreRoads` && type `road` && current cost `< 0xFF`: set 1. Order-independent result.
- Terrain is NOT in the matrix; plain/swamp costs come from `plainCost/swampCost`.

### ROOM-FINDPATH-LEGACY — `PathFinder.use(false)` mode
Source: reference/engine/src/game/rooms.js:873-968, grid :32-155, finder :16-30; algorithm reference/pathfinding/src/finders/AStarFinder.js:48, DijkstraFinder.
Only reachable after `PathFinder.use(false)` (resets every tick). Summary for a full port:
- `opts = _.clone(opts||{})`; cache-key suffix from `ignoreCreeps`, `ignoreDestructibleStructures`, `_avoid<key>`, `_ignore<key>` (`positionsSetCache.key`, an O(n²) set-equality search assigning global incrementing ids).
- `toPos` number ⇒ end-node key from `getEndNodes` (Dijkstra to any node with weight 999). Else: other room → `[]`; path cache hit (`pathCache["fx,fy,tx,ty"+suffix]`) → cloned/serialized cached path; equal → `[]`; out of 0..49 → `[]`; adjacent → single step; else A* (`AStarFinder`, diagonal, Chebyshev heuristic, `weight = opts.heuristicWeight || 1`, `maxOpsLimit = opts.maxOps || 2000`) on a grid where edge tiles cost 11, plain 2, swamp 10, road 1, obstacles 0 (walkable target forced), `ignore` positions capped at 2, `avoid` set to 0; incomplete searches return the path to the closest node.
- Result path cached under its actual last step.
- Depends on the npm `heap` package's ordering (not in reference — see Open questions). Low priority for parity.

### ROOM-GETPOSITIONAT — `room.getPositionAt(x, y)`
Source :971-976. `if (x<0 || x>49 || y<0 || y>49) return null; return new RoomPosition(x, y, name)`. No type coercion: `undefined` passes and becomes (0,0) via POS-CTOR; `NaN` passes here then makes the constructor throw.

### ROOM-CREATEFLAG — `room.createFlag(x, y, [name], [color], [secondaryColor] | pos, [name], [color], [secondaryColor])`
Source: reference/engine/src/game/rooms.js:978-1027. Docs: reference/docs/api/source/Room.md:156-192.
Validation order:
1. `[x,y] = fetchXYArguments(firstArg, secondArg)`; if x or y undefined or outside 0..49 → `ERR_INVALID_ARGS`.
2. `_.size(Game.flags) >= FLAGS_LIMIT (10000)` → `ERR_FULL` (counts flags created earlier this tick; user edits to `Game.flags` affect it).
3. If `_.isObject(firstArg)`: shift args (`secondaryColor=color; color=name; name=secondArg`).
4. `color` falsy → `COLOR_WHITE`; `secondaryColor` falsy → `color`.
5. `color ∉ COLORS_ALL` (strict `===`, so `'1'` fails) → `ERR_INVALID_ARGS`; same for `secondaryColor`.
6. `name` falsy → generate `'Flag'+n` for n=1,2,... until no existing flag (tick-start `register.flags`, any room) and no name created this tick has it.
7. Name exists (tick-start flags or created this tick) → `ERR_NAME_EXISTS`.
8. `name.length > 100` → `ERR_INVALID_ARGS` (non-strings have no length → pass).
9. Effects: record name; `Game.flags[name] = new Flag(name, color, secondaryColor, ""+room.name, x, y)`; `pushByName('room','createFlag', {roomName, x, y, name, color, secondaryColor})`; return `name`.
- **GOTCHA:** the new Flag is visible in `Game.flags` immediately but NOT in `room.find(FIND_FLAGS)`, `lookAt`, or `Game.getObjectById` this tick.
- **GOTCHA:** a flag removed this tick (`flag.remove()`) still blocks its name (`ERR_NAME_EXISTS`).
- **GOTCHA:** create + `remove()` of the same flag in one tick → flag still exists next tick (processor runs `removeFlag` before `createFlag`).
- Requires a visible room (the method lives on `Room`); see POS-CREATEFLAG for the throw.
- **PERF:** `_.any(register.flags, {name})` scans all of the player's flags (up to 10 000) per call; auto-naming is O(n²). User-global.

### ROOM-CREATECS — `room.createConstructionSite(x, y, structureType, [name] | pos, structureType, [name])`
Source: reference/engine/src/game/rooms.js:1029-1097; checks reference/engine/src/utils.js:128-189, :338-354. Docs: reference/docs/api/source/Room.md:111-152.
Validation order:
1. `[x,y] = fetchXYArguments(firstArg, secondArg)`; undefined / outside 0..49 → `ERR_INVALID_ARGS`.
2. If `secondArg` is a string and `structureType === undefined`: `structureType = secondArg`.
   **GOTCHA:** the pos form with a name, `createConstructionSite(pos, STRUCTURE_SPAWN, 'Name')`, leaves `structureType = 'Name'` (third param) → step 3 fails → `ERR_INVALID_ARGS`. The pos-form `name` can never be passed. `pos.createConstructionSite` is unaffected (it calls the x,y form).
3. `!CONSTRUCTION_COST[structureType]` → `ERR_INVALID_ARGS`.
4. If `structureType == 'spawn'` and `typeof name == 'string'`: `name.length > 100`, or name already created this tick, or any spawn in `register.spawns` (all visible rooms, any owner) or spawn construction site in `register.constructionSites` (visible rooms, any owner, plus the player's own sites anywhere) with that name → `ERR_INVALID_ARGS` (not ERR_NAME_EXISTS).
5. `controller && controller.level > 0 && !controller.my` → `ERR_NOT_OWNER`.
6. `controller && controller.reservation && raw controller.reservation.user != me` → `ERR_NOT_OWNER`.
7. `!checkControllerAvailability(type, objectsByRoom[room], controller)` → `ERR_RCL_NOT_ENOUGH`: `rcl = controller.level` if controller has `level` and (`user` or `owner`) else 0; count raw objects with `type == T` or (`constructionSite` with `structureType == T`) **of any owner** in the room; OK iff `count < CONTROLLER_STRUCTURES[T][rcl]`. Sites created earlier this tick are not counted (processor re-checks).
8. `!checkConstructionSite(objectsByRoom[room], T, x, y) || !checkConstructionSite(terrain, T, x, y)` → `ERR_INVALID_TARGET`:
   - objects: any object at (x,y) with `type == T` → false; any `constructionSite` at (x,y) → false; `extractor` → true iff a `mineral` and no `extractor` at (x,y); if T ∉ {rampart, road}: any object at (x,y) whose type ∉ {rampart, road} and has a `CONSTRUCTION_COST` → false; `x<=0||y<=0||x>=49||y>=49` → false.
   - terrain: if T ∉ {road, container} and (x∈{1,48} or y∈{1,48}): the 3 adjacent edge tiles must all be walls. **GOTCHA:** `borderTiles` is assigned by sequential `if`s (x==1, x==48, y==1, y==48), so at corners the *last* matching rule wins (e.g. at (1,1) only the top edge (0..2,0) is checked). Then `extractor` → true; `T != road` on a wall → false. Roads may be built on walls.
9. `count(userObjects of type constructionSite) + createdThisTick >= MAX_CONSTRUCTION_SITES (100)` → `ERR_FULL` (all rooms, including invisible).
10. Effects: intent `{roomName, x, y, structureType}`; for spawns: `name` = given string or auto `'Spawn'+n` (first n not used by visible spawns, visible spawn sites, or names created this tick); `createdConstructionSites++`; `pushByName('room','createConstructionSite', intent)`; return `OK`.
- **PERF:** steps 7–9 are linear scans: room objects (×3) and all of the player's objects (step 9) per call.

### ROOM-GETENDNODES — `room.getEndNodes(type, [opts])` (undocumented)
Source :1099-1128. Legacy helper exposed on the prototype. Throws `'Find type cannot be undefined'` for undefined `type`. Returns `{key, objects}`; with a numeric type and no filter, `key = type` and objects = `this.find(type, opts)`; otherwise objects are keyed through `positionsSetCache`. Mutates `privateStore.pfEndNodes`. Only used by POS-FINDCLOSESTBYPATH-LEGACY.

### ROOM-FINDEXITTO — `room.findExitTo(room)`
Source :1130-1132 → `Game.map.findExit(this.name, room)` with no opts (MAP-FINDEXIT). Returns FIND_EXIT_* constant, `ERR_NO_PATH`, or `ERR_INVALID_ARGS` (same room).

### ROOM-GETTERRAIN
Source :1134-1136. `return new Room.Terrain(this.name)` (new object each call).

### ROOM-GETEVENTLOG — `room.getEventLog([raw])`
Source :573-582; data reference/driver/lib/runtime/data.js:141, written by reference/driver/lib/index.js:584.
```
if raw (truthy): return runtimeData.roomEventLog[name] || '[]'      // JSON string
if !register.roomEventLogCache[name]: cache[name] = JSON.parse(runtimeData.roomEventLog[name] || '[]')
return cache[name]
```
- **GOTCHA:** the parsed array is the *same object* on every call within the tick; user mutations persist for later callers that tick.
- Events are the previous tick's processor output for the room: `[{event: EVENT_*, objectId, data:{...}}]` (shapes per reference/docs/api/source/Room.md:473-620; production is the processor spec).
- **PERF:** parse cost paid once per room per tick per user, lazily.

### ROOM-SERIALIZEPATH — `Room.serializePath(path)`
Source: reference/engine/src/utils.js:555-574.
```
if !Array.isArray(path): throw Error('path is not an array')
if path.length == 0: return ''
if path[0].x < 0 || path[0].y < 0: throw Error('path coordinates cannot be negative')
s = (path[0].x > 9 ? path[0].x : '0'+path[0].x) + (path[0].y > 9 ? path[0].y : '0'+path[0].y)
for step in path: s += step.direction            // string concatenation of each direction
```
Format: `XXYY` of the **first step** (not the start position) + one digit per step (incl. the first). **GOTCHA:** no validation of directions (`undefined` becomes the text `"undefined"`), x/y ≥ 100 or fractional values produce malformed strings.

### ROOM-DESERIALIZEPATH — `Room.deserializePath(str)`
Source: reference/engine/src/utils.js:576-613.
```
if typeof str != string (lodash _.isString): throw Error('`path` is not a string')
if str == '': return []
x = parseInt(str.substring(0,2)); y = parseInt(str.substring(2,4))
if NaN(x) || NaN(y): throw Error('`path` is not a valid serialized path string')
for i in 4..len-1:
    d = parseInt(str[i]); if !offsets[d]: throw same error      // '0', '9', letters invalid
    (dx,dy) = offsets[d]; if i > 4: x += dx; y += dy
    push {x, y, dx, dy, direction: d}
```
First step keeps the stored x,y; dx/dy of the first step come from its direction. `"1010"` → `[]`.

### TERRAIN-CTOR — `new Room.Terrain(roomName)`
Source: reference/engine/src/game/rooms.js:1223-1242 (strict-mode function).
`roomName = ""+roomName`; `array = staticTerrainData[roomName]`; if missing → throw `Error("Could not access room "+roomName)`. Works for every room in the world's terrain data, visible or not. `get` and `getRawBuffer` are **own properties created per instance** (not on the prototype). Calling without `new` throws (strict `this` undefined).

### TERRAIN-GET — `terrain.get(x, y)`
Source :1230-1233. `v = array[y*50+x]; return (v & TERRAIN_MASK_WALL) || (v & TERRAIN_MASK_SWAMP) || 0`. Returns 0/1/2 (raw 3 → 1). No bounds or type check: out-of-range indices return 0; x≥50 aliases into the next row; fractional indices read `undefined` → 0.

### TERRAIN-GETRAWBUFFER — `terrain.getRawBuffer([destinationArray])`
Source :1235-1241.
- Falsy argument → `new Uint8Array(array)` (fresh copy, raw codes 0..3, row-major).
- Truthy → `Uint8Array.prototype.set.call(dest, array)`; returns `dest`. **GOTCHA:** docs promise `ERR_INVALID_ARGS` for incompatible arrays (reference/docs/api/source/Room.Terrain.md:154-158) but the engine **throws** (`TypeError` for non-typed-arrays, `RangeError` if `dest.length < 2500`); any TypedArray type is accepted (values converted).

---

## 3. FIND_* semantics (one row per inventory item)

All: snapshot at tick start, room-local, order per CORE-OBJECT-ORDER unless stated. "Instances" are the same objects as `Game.getObjectById`.

| ID | Contents | Notes / GOTCHA |
|---|---|---|
| FIND-EXIT-TOP | `RoomPosition`s (x,0), x ascending, non-wall | swamp edge tiles included |
| FIND-EXIT-RIGHT | (49,y), y ascending | |
| FIND-EXIT-BOTTOM | (x,49), x ascending | |
| FIND-EXIT-LEFT | (0,y), y ascending | |
| FIND-EXIT | TOP ++ BOTTOM ++ RIGHT ++ LEFT | order is not numeric order of the constants; cache pollution bug (ROOM-FIND-EXIT) |
| FIND-CREEPS | creeps with `!spawning` | spawning creeps excluded but visible to look*, `Game.creeps` |
| FIND-MY-CREEPS | `!spawning && my` | |
| FIND-HOSTILE-CREEPS | `!spawning && !my` | includes NPCs (Invader, Source Keeper) |
| FIND-SOURCES-ACTIVE | sources with `energy > 0` | |
| FIND-SOURCES | all sources | |
| FIND-DROPPED-RESOURCES | all raw `type:'energy'` objects (every resource type) | |
| FIND-STRUCTURES | every structure instance (all owners, neutral, portals, controllers, keeper lairs, power banks, invader cores) | not construction sites |
| FIND-MY-STRUCTURES | `my === true` | |
| FIND-HOSTILE-STRUCTURES | `my === false && owner` | **GOTCHA:** includes keeper lairs (`owner.username 'Source Keeper'`) and power banks (`'Power Bank'`) whose `my` is hard-coded `false` (reference/engine/src/game/structures.js:286, :588); neutral roads/walls/containers (no `my`) and unowned controllers (`my` undefined) excluded |
| FIND-FLAGS | the **player's own** flags in this room | order: FLAG-LOAD; flags created this tick excluded |
| FIND-CONSTRUCTION-SITES | all sites in the room (all owners) | |
| FIND-MY-SPAWNS | spawns with `my === true` | |
| FIND-HOSTILE-SPAWNS | spawns with `my === false` | |
| FIND-MY-CONSTRUCTION-SITES | `my` truthy | |
| FIND-HOSTILE-CONSTRUCTION-SITES | `my === false` | |
| FIND-MINERALS | all minerals | |
| FIND-NUKES | all landing nukes in the room | |
| FIND-TOMBSTONES | all tombstones | |
| FIND-POWER-CREEPS | all spawned power creeps | no `spawning` concept |
| FIND-MY-POWER-CREEPS | `my` | |
| FIND-HOSTILE-POWER-CREEPS | `!my` | |
| FIND-DEPOSITS | all deposits | |
| FIND-RUINS | all ruins | |

## 3b. LOOK_* semantics

| ID | Result element (lookAt / lookAtArea) | lookForAt element | Notes |
|---|---|---|---|
| LOOK-CREEPS | `{type:'creep', creep}` | Creep | includes spawning creeps |
| LOOK-ENERGY | `{type:'energy', energy}` | Resource (any type) | same set as LOOK_RESOURCES |
| LOOK-RESOURCES | `{type:'resource', resource}` | Resource | duplicates LOOK_ENERGY entries in lookAt/lookAtArea |
| LOOK-SOURCES | `{type:'source', source}` | Source | |
| LOOK-MINERALS | `{type:'mineral', mineral}` | Mineral | |
| LOOK-DEPOSITS | `{type:'deposit', deposit}` | Deposit | |
| LOOK-STRUCTURES | `{type:'structure', structure}` | Structure | all owners |
| LOOK-FLAGS | `{type:'flag', flag}` | Flag | own flags only |
| LOOK-CONSTRUCTION-SITES | `{type:'constructionSite', constructionSite}` | ConstructionSite | |
| LOOK-NUKES | `{type:'nuke', nuke}` | Nuke | |
| LOOK-TERRAIN | `{type:'terrain', terrain:'plain'|'swamp'|'wall'}` | `'plain'|'swamp'|'wall'` (array of 1) | wall wins over swamp; no bounds check |
| LOOK-TOMBSTONES | `{type:'tombstone', tombstone}` | Tombstone | |
| LOOK-POWER-CREEPS | `{type:'powerCreep', powerCreep}` | PowerCreep | |
| LOOK-RUINS | `{type:'ruin', ruin}` | Ruin | |

---

## 4. RoomPosition

### POS-CTOR — `new RoomPosition(x, y, roomName)`
Source: reference/engine/src/game/rooms.js:1276-1293.
```
xy = roomName === 'sim' ? [-128,-128] : roomNameToXY(roomName)     // non-string → TypeError
xy[0] += 128; xy[1] += 128
if xy[0] <0 || xy[0] > 256 || xy[0] is NaN || xy[1] <0 || xy[1] > 256 || xy[1] is NaN
   || x < 0 || x > 49 || x !== x || y < 0 || y > 49 || y !== y:
    throw Error('Invalid arguments in RoomPosition constructor')
this.__packedPos = xy[0]<<24 | xy[1]<<16 | x<<8 | y     // own, non-enumerable, writable
```
- **GOTCHA (coercion):** checks are relational, so `undefined`, `null`, `true`, `'5'`, `1.7` pass and are coerced by `<<`/`|` (ToInt32): `undefined`/`null` → 0, `'5'` → 5, `1.7` → 1. Only NaN, <0, >49 are rejected. `new RoomPosition(undefined, undefined, 'W1N1')` is (0,0).
- **GOTCHA (canonicalisation):** `roomName` is not stored; it is re-derived, so `new RoomPosition(1,1,'w1n1').roomName === 'W1N1'` and `'E5S5junk'` → `'E5S5'`.

### POS-PACKED — `__packedPos`
Bits: `[31..24] = roomX+128`, `[23..16] = roomY+128`, `[15..8] = x`, `[7..0] = y`; int32 (negative when roomX ≥ 0, i.e. bit 31 set); read with `>>>`. Some bots read/write `__packedPos` directly (e.g. for fast serialisation) — keep the exact layout.
- **GOTCHA (world limits):** room coords must satisfy −128 ≤ X,Y ≤ 128; `X = 128` (E128) gives `256<<24 == 0` → aliases to W128/`'sim'`. `W127N127` packs to room id 0 and reads back as `'sim'`. The driver pathfinder additionally rejects anything outside W127..E127 / N127..S127 (`kWorldSize = 255`, reference/driver/lib/path-finder.js:9-21). **PERF/scale:** chitin's "larger worlds" goal collides with this 8-bit room-coordinate packing; any extension must keep W127..E127 behaviour bit-identical for bots that use `__packedPos`.

### POS-X / POS-Y / POS-ROOMNAME
Source :1295-1348. Prototype accessors, enumerable.
- `x` get `(p >> 8) & 0xff`; set: `val<0 || val>49 || val!==val` → throw `Error('Invalid coordinate')`; else replace byte with `val << 8` (coercions as in POS-CTOR).
- `y` get `p & 0xff`; set analogous.
- `roomName` get: `roomNames[p >>> 16]` memo or `getRoomNameFromXY((p>>>24)-128, ((p>>>16)&0xff)-128)`; set: parse like the ctor, invalid → throw `Error('Invalid roomName')`.

### POS-TOJSON / POS-TOSTRING
`toJSON()` → `Object.assign({x, y, roomName}, this)` (own enumerable props of the instance appended; `__packedPos` is non-enumerable) (:1350-1356). `toString()` → `[room <roomName> pos <x>,<y>]` (:1358-1360).

### POS-INRANGETO — `inRangeTo(x, y, range)` / `inRangeTo(target, range)`
Source :1362-1376.
```
if thirdArg === undefined: pos = firstArg.pos || firstArg; x,y,roomName = pos.x, pos.y, pos.roomName; range = secondArg
else: x = firstArg; y = secondArg; range = thirdArg; roomName = this.roomName
return |x-this.x| <= range && |y-this.y| <= range && roomName == this.roomName
```
**GOTCHA:** unlike the fetchXY family, plain `{x,y,roomName}` objects work here; plain `{x,y}` without roomName → false; `undefined` target → TypeError.

### POS-ISNEARTO — `isNearTo(x, y | target)`
Source :1378-1381. `[x,y,rn] = fetchXY(...)`; `|x-this.x|<=1 && |y-this.y|<=1 && (!rn || rn == this.roomName)`. Unparseable target → false (NaN comparisons). Same tile counts as near.

### POS-GETDIRECTIONTO — `getDirectionTo(x, y | target)`
Source :1383-1394. Same room (or no roomName) → `getDirection(x-this.x, y-this.y)`. Other room → world-coordinate deltas `getDirection(tX*50 + x - sX*50 - this.x, tY*50 + y - sY*50 - this.y)` with `[sX,sY]`/`[tX,tY]` from `roomNameToXY`. Same position → `undefined`.

### POS-FINDPATHTO — `findPathTo(x, y, [opts])` / `findPathTo(target, [opts])`
Source :1396-1427.
```
[x,y,roomName] = fetchXY(firstArg, secondArg); room = register.rooms[this.roomName]
if _.isObject(secondArg): opts = _.clone(secondArg)        // target form clones; (x,y,opts) form does NOT
opts = opts || {}; roomName = roomName || this.roomName
if !room: throw Error('Could not access room ' + this.roomName)
if roomName == this.roomName || _useNewPathFinder:
    return room.findPath(this, new RoomPosition(x, y, roomName), opts)
else (legacy only): exitDir = room.findExitTo(roomName); exitDir < 0 → []
    exit = this.findClosestByPath(exitDir, opts); !exit → []; return room.findPath(this, exit, opts)
```
**GOTCHA:** an unparseable target gives `x=y=undefined` → `new RoomPosition(undefined, undefined, room)` = (0,0) — a path to the room corner, not an error.

### POS-FINDCLOSESTBYPATH — `findClosestByPath(type | objects, [opts])` (PathFinder mode)
Source :1429-1445, :304-374. Docs: reference/docs/api/source/RoomPosition.md:117-186.
```
opts = _.clone(opts || {}); room = register.rooms[this.roomName]
if !room: throw Error('Could not access room ' + this.roomName)
if type === undefined: return null
-- _findClosestByPath2(this, type, opts):
if number(type): objects = register.rooms[this.roomName].find(type, {filter: opts.filter})
elif opts.filter: objects = _.filter(type, opts.filter)  else objects = type
if !objects.length: return null
hit = first obj in objects with this.isEqualTo(obj); if hit: return hit
goals = objects.map(o => ({range: 1, pos: o.pos || o}))
avoid/ignore → deprecated() message only
searchOpts = { roomCallback(rn): if objectsByRoom[rn]: m = getPathfindingGrid2(rn, opts) (+costCallback clone/replace as in findPath); return m
                                  (else undefined → terrain only),
               maxOps: opts.maxOps, maxRooms: 1 }
if !opts.ignoreRoads: plainCost = 2, swampCost = 10
ret = PathFinder.search(this, goals, searchOpts)
lastPos = ret.path.length ? last(ret.path) : this
result = null; for obj in objects: if lastPos.isNearTo(obj): result = obj     // LAST match wins
return result
```
- **GOTCHA (ties):** among objects adjacent to the path end, the **last** in array order wins (contrast findClosestByRange: first wins). If the origin is already adjacent to objects the native search returns an empty path (PF-SEARCH-RESULT) and the last adjacent object wins.
- **GOTCHA (ignored opts):** `plainCost`, `swampCost`, `heuristicWeight`, `range`, `maxRooms` ignored; always `range: 1` goals, single room.
- Objects arrays may contain RoomPositions; a `null` element throws (`isEqualTo(null)`), plain `{x,y}` throws `'Invalid room name'` in the driver.
- `algorithm` option ignored in this mode.

### POS-FINDCLOSESTBYPATH-LEGACY
Source :1447-1515 (`PathFinder.use(false)` only). `opts.serialize=false`; `endNodes = room.getEndNodes(type, opts)`; if no `opts.algorithm`: `minH`/`sumH` of Chebyshev distances, `algorithm = sumH > minH*10 ? 'dijkstra' : 'astar'`. Dijkstra: equal → that object; adjacent → that object; else `findPath(this, endNodes.key)` and pick the object at the last step. A*: for each object, distance −1 (equal), 0 (adjacent), else `findPathTo(obj).length` if the path ends adjacent; picks the object with `distance <= best` (**last** among equals).

### POS-FINDINRANGE — `findInRange(type | objects, range, [opts])`
Source :1518-1544. Throws `Could not access room` if this room is invisible (even for an array argument — **GOTCHA**). `opts = _.clone(opts||{})`; number → `room.find(type, opts)`; array → `opts.filter ? _.filter(arr, opts.filter) : arr`; anything else → `[]`. Returns objects with `this.inRangeTo(o, range)`, preserving order.

### POS-FINDCLOSESTBYRANGE — `findClosestByRange(type | objects, [opts])`
Source :1546-1576. Same room/argument handling as findInRange. Returns the **first** object with strictly minimal `getRangeTo` (`<`), or `null`. Objects in other rooms have range `Infinity` and are never chosen; unparseable entries give NaN and are never chosen.

### POS-ISEQUALTO — `isEqualTo(x, y | target)`
Source :1578-1584.
```
if firstArg.__packedPos !== undefined: return firstArg.__packedPos === this.__packedPos   // TypeError if firstArg null/undefined
[x,y,rn] = fetchXY(firstArg, secondArg); return x == this.x && y == this.y && (!rn || rn == this.roomName)
```

### POS-GETRANGETO — `getRangeTo(x, y | target)`
Source :1586-1592. `rn && rn != this.roomName` → `Infinity`; else Chebyshev `max(|dx|,|dy|)` (NaN for unparseable input). No cross-room distance.

### POS-LOOK / POS-LOOKFOR
Source :1594-1620.
- `look()`: invisible room → throw `Could not access room`; else `room.lookAt(this)`.
- `lookFor(type)`: `type == 'terrain'` → `['wall'|'swamp'|'plain']` straight from terrain data, **works without visibility**; otherwise invisible room → throw; else `room.lookForAt(type, this)` (invalid type → `ERR_INVALID_ARGS` number).

### POS-CREATEFLAG / POS-CREATECS
Source :1622-1636. Invisible room → throw `Error('Could not access room ...')` (**GOTCHA:** throws rather than returning an error code). Otherwise `room.createFlag(this, name, color, secondaryColor)` / `room.createConstructionSite(this.x, this.y, structureType, name)` (x,y form, so the spawn name works here).

### ROOMOBJ-POS / ROOMOBJ-ROOM / ROOMOBJ-EFFECTS
Source: reference/engine/src/game/rooms.js:1648-1659.
`RoomObject(x, y, roomName, effects)`: `this.room = register.rooms[roomName]` (undefined for invisible rooms — flags and own construction sites only), `this.pos = new RoomPosition(x, y, roomName)`; if `effects` truthy: `this.effects = effects.map(e => ({power: e.power, effect: e.effect, level: e.level, ticksRemaining: e.endTime - Game.time})).filter(e => e.ticksRemaining > 0)`.
- **GOTCHA:** `effects` is **absent** (undefined) on objects whose raw record has no `effects` field, `[]` when the raw array exists but all expired. Natural effects have `power: undefined`, `level: undefined`; power effects carry `effect == power == PWR_*` (reference/engine/src/processor/intents/power-creeps/usePower.js:280-283).
- Own enumerable props in order `room`, `pos`, [`effects`]; no prototype members.

---

## 5. Flag, ConstructionSite, simple objects

### FLAG-LOAD
Source: reference/engine/src/game/game.js:397-429; data reference/driver/lib/runtime/data.js:140.
`runtimeData.flags` = the player's `rooms.flags` documents (one per room). Each `data` string is `name~color~secondaryColor~x~y` entries joined by `|`; names are unescaped (`$VLINE$`→`|`, `$TILDE$`→`~`). Per entry: `flag = new Flag(name, color, secondary, room, x, y)`; `register._objects['flag_'+name] = flag`; `register.flags['flag_'+name]`; spatial/byRoom registration only if the room is visible; `Game.flags[name] = flag`; appended to `findCache[FIND_FLAGS][room]`.
- **GOTCHA:** `Game.getObjectById('flag_' + name)` returns the flag, although flags have no `id` property.
- Order within a room = order in the data string; the processor appends new flags at the end and `setColor`/`setPosition` are remove+append, so they move a flag to the end (reference/engine/src/processor/intents/room/create-flag.js:26-33, remove-flag.js:6).

### FLAG-PROPS
Source: reference/engine/src/game/flags.js:20-28. `RoomObject.call(this, Number(x), Number(y), roomName)`; own `name` (as given, not coerced), `color = Number(color)`, `secondaryColor = Number(secondaryColor || color)`. Flags have no `id`, no `effects`. JSON = default (own enumerable: room, pos, name, color, secondaryColor).

### FLAG-MEMORY
Source :31-51. Identical to ROOM-MEMORY with `Memory.flags` and error text `'Could not set flag memory'`.

### FLAG-TOSTRING
`[flag <name>]` (:53-55).

### FLAG-REMOVE — `flag.remove()`
Source :57-61. Always `pushByName('room','removeFlag',{roomName: pos.roomName, name})`, return `OK`. No dedupe, no checks.

### FLAG-SETPOSITION — `flag.setPosition(x, y | pos)`
Source :63-74.
```
[x,y,roomName] = fetchXY(firstArg, secondArg); roomName = roomName || this.pos.roomName
if x === undefined || y === undefined: return ERR_INVALID_TARGET
push removeFlag {roomName: this.pos.roomName, name}
push createFlag {roomName, x, y, name, color: this.color, secondaryColor: this.secondaryColor}
return OK
```
- **GOTCHA:** no API bounds check; out-of-range x/y are rejected by the processor's createFlag *after* removeFlag ran → the flag is deleted.
- Cross-room moves split into intents for two rooms.
- Changes are not reflected in the current tick.

### FLAG-SETCOLOR — `flag.setColor(color, [secondaryColor])`
Source :76-91. `color ∉ COLORS_ALL` → `ERR_INVALID_ARGS`; `secondaryColor = secondaryColor || color`; `∉ COLORS_ALL` → `ERR_INVALID_ARGS`; push removeFlag + createFlag (same position); `OK`.
- **GOTCHA:** two `setColor`/`setPosition` calls on one flag in a tick → the processor removes once, the *first* createFlag succeeds and later ones fail the duplicate-name check: **first call wins** (reference/engine/src/processor/intents/room/create-flag.js:10-14).

### CS-PROPS / CS-OWNER-MY / CS-REMOVE / CS-TOSTRING
Source: reference/engine/src/game/construction-sites.js:27-57.
Getters (CORE-GAMEOBJ-GETTER): `progress`, `progressTotal`, `structureType`, `name` (spawn sites only), `owner = new Object({username: users[user].username})`, `my = user === undefined ? undefined : user == me`. Own `id`.
`remove()`: `if (!this.my && !(this.room && this.room.controller && this.room.controller.my)) return ERR_NOT_OWNER`; push `removeConstructionSite {roomName: raw.room, id}`; `OK`. (Room owners may remove hostile sites.) Processor refunds `floor(progress/2)` energy when `progress > 1` (reference/engine/src/processor/intents/room/remove-construction-site.js:6).
`toString()` → `[construction site (<structureType>) #<id>]`.

### SOURCE-PROPS
Source: reference/engine/src/game/sources.js:26-43. `energy`, `energyCapacity`, `ticksToRegeneration = raw.nextRegenerationTime ? nextRegenerationTime - Game.time : undefined`; own `id`; `toString` → `[source #<id>]`.

### MINERAL-PROPS
Source: reference/engine/src/game/minerals.js:26-44. `mineralType`, `mineralAmount`, `density`, `ticksToRegeneration` (as Source); `toString` → `[mineral (<mineralType>) #<id>]`.

### DEPOSIT-PROPS
Source: reference/engine/src/game/deposits.js:24-42. `depositType`; `cooldown = cooldownTime && cooldownTime > time ? cooldownTime - time : 0`; `lastCooldown = ceil(DEPOSIT_EXHAUST_MULTIPLY * harvested ^ DEPOSIT_EXHAUST_POW)` (0.001, 1.2; reference/common/lib/constants.js:329-330); `ticksToDecay = decayTime ? decayTime - time : undefined`; `toString` → `[deposit (<depositType>) #<id>]`.

### NUKE-PROPS
Source: reference/engine/src/game/nukes.js:26-42. `timeToLand = landTime - time`, `launchRoomName`; `toString` → `[nuke #<id>]`.

### RESOURCE-PROPS / RESOURCE-ENERGY-ALIAS
Source: reference/engine/src/game/resources.js:26-46. `resourceType = raw.resourceType || 'energy'`; `amount = raw[resourceType]`; `energy = raw.energy` (legacy; set only for energy drops); `toString` → `[resource (<resourceType>) #<id>]`. Globals `Resource` and `Energy` are the same constructor.

### TOMB-PROPS
Source: reference/engine/src/game/tombstones.js:33-44, :236-238. `deathTime`, `store` (STORE-CTOR over the raw tombstone; raw has only `store`, so capacities are null — see STORE-*), `ticksToDecay = decayTime - time`; `toString` → `[Tombstone #<id>]` (**capital T**).

### TOMB-CREEP
Source :45-233. Lazily-built, cached (it is truthy) synthetic object:
- raw `creepId` → `new Creep()` (no id ⇒ ctor does nothing) + `RoomObject.call(...)` (room/pos of the tombstone) + own enumerable getters: `id=creepId`, `name=creepName`, `spawning=false`, `my = user==me`, `body = creepBody.map(t => ({type:t, hits:0}))`, `owner` (`{username}` or undefined when user null/undefined), `ticksToLive = creepTicksToLive`, `carryCapacity = 50 × #CARRY`, `carry`/`store` = **a new, empty Store on every access** (`_storeGetter` called as a getter with no argument builds `{store:{}, storeCapacity: this.carryCapacity}`, reference/engine/src/game/tombstones.js:8-13 — the deceased creep's cargo is NOT shown here, it is in `tombstone.store`), `fatigue=0`, `hits=0`, `hitsMax = 100×body length`, `saying = creepSaying`.
- raw `powerCreepId` → `new PowerCreep()` with `id, name, className, level, my, body, owner, ticksToLive, carryCapacity = level×100, carry, store, hits=0, hitsMax = level×1000, saying, powers = powerCreepPowers`.
- Neither → `undefined` (re-evaluated each access).
Inherited prototype methods/getters of Creep/PowerCreep remain reachable (e.g. `memory` via `name`).

### RUIN-PROPS
Source: reference/engine/src/game/ruins.js:29-42, :68-70. `structureType = raw.structureType` — **GOTCHA:** ruins created by the processor have no top-level `structureType` (reference/engine/src/processor/intents/structures/_destroy.js:23-36), so this undocumented getter is always `undefined`; use `ruin.structure.structureType`. `destroyTime`, `ticksToDecay = decayTime - time`, `store` (raw ruin has no capacities → capacity methods null). `toString` → `[ruin (<structure.structureType>) #<id>]`.

### RUIN-STRUCTURE
Source :43-66. If `raw.structure.user`: `new OwnedStructure()` + own getters `id, hits, hitsMax, structureType (= raw.structure.type), owner {username}, my`. Else `new Structure()` + `id, hits, hitsMax, structureType`. No `pos`/`room` (Structure ctor without id skips RoomObject). Cached (truthy).

---

## 6. Store

### STORE-CTOR / STORE-INDEX
Source: reference/engine/src/game/store.js:19-97.
`new Store(raw)` (global constructor, user-callable): for each `[k, v]` of `Object.entries(raw.store)` with truthy `v`: own enumerable `this[k] = v`. Methods `getCapacity`, `getUsedCapacity`, `getFreeCapacity`, `toString` are own, **non-enumerable**. Returns `new Proxy(this, {get})`:
```
get(target, name): if target[name] !== undefined: return target[name]
                   if RESOURCES_ALL includes name: return 0
                   return undefined
```
- Only a `get` trap: `Object.keys(store)`, `JSON.stringify(store)` (→ `{"energy":50}`), `'energy' in store`, `hasOwnProperty` see only non-zero resources. Writes go to the target.
- `toString()` → `[store]`.

### STORE-CAPFORRES — `capacityForResource(raw, r)`
Source: reference/engine/src/utils.js:671-675.
`(raw.storeCapacityResource && raw.storeCapacityResource[r]) || max(0, (raw.storeCapacity || 0) - _.sum(raw.storeCapacityResource))` (`_.sum(undefined) = 0`).

### STORE-GETCAPACITY — `getCapacity([resource])`
Source :28-35.
- `!resource` → `raw.storeCapacityResource ? null : (raw.storeCapacity || null)`.
- else `capacityForResource(raw, resource) || null`.
Examples: lab raw `{storeCapacity: 5000, storeCapacityResource:{energy:2000}}` (reference/engine/src/processor/intents/creeps/build.js:229-231) → `getCapacity()` null, `getCapacity('energy')` 2000, `getCapacity('H')` 3000. Tombstone/ruin (no capacities) → null.

### STORE-GETUSEDCAPACITY — `getUsedCapacity([resource])`
Source :36-50.
- `!resource`: if `raw.storeCapacityResource && capacityForResource(raw, undefined) === 0` → `null` (pure limited stores: spawn, extension, tower, link, nuker, powerSpawn); else `_sum = _.sum(raw.store)` cached on the target as a non-enumerable `_sum`.
- `resource`: `raw.store[resource] || (raw.storeCapacityResource && capacityForResource(raw, resource) === 0 ? null : 0)`.

### STORE-GETFREECAPACITY — `getFreeCapacity([resource])`
Source :52-78.
```
if capacityForResource(raw, resource) === 0: return null
if !raw.storeCapacity: return getCapacity(resource) - getUsedCapacity(resource)
cap = getCapacity(resource); if !cap: return null
if raw.storeCapacityResource: return cap - getUsedCapacity(resource)
return cap - _sum (cached sum of store)
```
**GOTCHA:** for labs `getFreeCapacity('H')` = 3000 − amount of H, even if the lab holds another mineral (formula-level only). Tombstones/ruins: `capacityForResource(...) === 0` → `null` for every argument.

All values are computed from the tick-start raw record; intents earlier in the tick do not change them.

---

## 7. PathFinder & CostMatrix (game side)

### PF-SEARCH — `PathFinder.search(origin, goal, [opts])`
Source: reference/engine/src/game/path-finder.js:57-65 → reference/driver/lib/path-finder.js:86-142.
`if (!goal || Array.isArray(goal) && !goal.length) return {path: [], ops: 0}` — **GOTCHA:** this early return lacks `cost` and `incomplete`. Otherwise calls the driver/native search.

### PF-SEARCH-OPTS (driver coercion)
Source: reference/driver/lib/path-finder.js:88-131.
```
plainCost       = min(254, max(1, (opts.plainCost|0) || 1))
swampCost       = min(254, max(1, (opts.swampCost|0) || 5))
heuristicWeight = min(9,   max(1, opts.heuristicWeight || 1.2))      // not truncated
maxOps          = max(1, (opts.maxOps|0) || 2000)
maxCost         = max(1, (opts.maxCost|0) || 0xffffffff)
maxRooms        = min(64, max(1, (opts.maxRooms|0) || 16))
flee            = !!opts.flee
goals: each g: if g.x, g.y, g.roomName all !== undefined → {range: 0, pos: g}
               else {range: max(0, g.range|0), pos: g.pos}
roomCallback: ignored if not a function; wrapper returns false if cb returned exactly false,
              ret._bits if ret truthy, else undefined (terrain only)
positions → world coords via toWorldPosition: x|0, y|0 must be 0..49 else throw Error('Invalid room position');
             roomName must match /^([WE])([0-9]+)([NS])([0-9]+)$/ within W127..E127/N127..S127 else throw Error('Invalid room name')
```
- **GOTCHA:** `|0` truncates to int32: `maxCost: Infinity` → default; `maxCost` > 2³¹−1 (e.g. 3e9) wraps negative → clamped to **1**. `maxOps: Infinity` → 2000.
- **GOTCHA:** `origin` must have `x,y,roomName` (a RoomObject throws `'Invalid room name'`), but goals may be RoomObjects (`g.pos` used, range 0). `'sim'` is rejected.
- Any truthy callback return is used as `._bits` (duck-typed; not required to be a CostMatrix).

### PF-SEARCH-RESULT
Source: reference/driver/lib/path-finder.js:134-141; reference/driver/native/src/pf.cc:506-519, :595-599.
- Origin already satisfies a goal (heuristic 0) → native returns `undefined` → `{path: [], ops: 0, cost: 0, incomplete: false}`.
- Origin room inaccessible (callback `false` / no terrain) → `-1` → `{path: [], ops: 0, cost: 0, incomplete: true}`.
- Otherwise `{path: RoomPosition[] (origin excluded, goal-side last), ops, cost, incomplete}`; `incomplete` iff the best node is not within goal range; the path then leads to the node with the lowest heuristic.
- An exception thrown inside `roomCallback` propagates out of `search`.

### PF-USE — `PathFinder.use(isActive)`
Source: reference/engine/src/game/path-finder.js:67-75. Falsy → `deprecated('`PathFinder.use` is considered deprecated and will be removed soon.')`; `register._useNewPathFinder = !!isActive`. Per tick (register recreated each tick with `true`). Affects ROOM-FINDPATH, POS-FINDPATHTO, POS-FINDCLOSESTBYPATH (and Creep.moveTo).

### CM-CTOR — `new PathFinder.CostMatrix()`
Source :18-20. Own enumerable `_bits = new Uint8Array(2500)`, **column-major index `x*50+y`** (unlike terrain). Some bots read `_bits` directly; keep layout.

### CM-SET — `set(x, y, val)`
Source :22-26. `x|=0; y|=0; _bits[x*50+y] = min(max(0, val), 255)` stored with Uint8 conversion (NaN → 0, fractions truncated, strings numeric-coerced). **GOTCHA:** no bounds check — `(0,50)` writes tile (1,0); indices outside 0..2499 are silently dropped. Returns `undefined` (not chainable).

### CM-GET — `get(x, y)`
Source :28-32. `_bits[(x|0)*50 + (y|0)]` — `undefined` outside 0..2499, aliasing inside.

### CM-CLONE
Source :34-38. New `CostMatrix` whose `_bits` is a copy.

### CM-SERIALIZE
Source :40-42. `Array.prototype.slice.apply(new Uint32Array(this._bits.buffer))` → 625 numbers (little-endian packing of 4 bytes per number on x86/ARM). chitin must reproduce the little-endian packing exactly for Memory compatibility.

### CM-DESERIALIZE — `PathFinder.CostMatrix.deserialize(data)`
Source :44-48. `Object.create(CostMatrix.prototype)` (ctor not run) with `_bits = new Uint8Array(new Uint32Array(data).buffer)`; no length validation (shorter input → shorter `_bits`).

---

## 8. Game.map

### MAP-GRID
Source: reference/driver/lib/runtime/mapgrid.js:40-121; built once per runtime isolate (reference/driver/lib/runtime/runtime.js:206-211) from the accessible-room list at that moment.
For each accessible room, for each side `t` (y=0), `r` (x=49), `b` (y=49), `l` (x=0): count edge tiles whose raw terrain `== 0` (**plain only** — swamp tiles (2) do not count); store the count if > 0 in `gridData["x,y"]` (`roomNameToXY` of that file upper-cases names).
- **GOTCHA:** a side whose only passable edge tiles are swamps is not an exit for `describeExits`/`findRoute`, although `FIND_EXIT_*` returns those tiles.
- **GOTCHA:** static for the isolate's lifetime (rooms opened later are missing until the runtime restarts).

### MAP-DESCRIBEEXITS — `Game.map.describeExits(roomName)`
Source: reference/engine/src/game/map.js:20-46.
```
if !/^(W|E)\d+(N|S)\d+$/.test(roomName): return null        // uppercase only
item = gridData[roomNameToXY(roomName)]; if !item: return null
exits = {}; if item.t: exits[TOP] = name(x,y-1); if item.b: exits[BOTTOM] = ...(x,y+1)
            if item.l: exits[LEFT] = ...(x-1,y); if item.r: exits[RIGHT] = ...(x+1,y)
return exits
```
Keys are the strings `"1","3","5","7"`; JS enumerates integer-like keys ascending → **TOP, RIGHT, BOTTOM, LEFT** regardless of insertion order. Neighbour existence/accessibility is not checked. A room with no exits returns `{}` (not null).

### MAP-FINDROUTE — `Game.map.findRoute(fromRoom, toRoom, [opts])`
Source: reference/engine/src/game/map.js:9-186; heap/open-closed reference/engine/src/game/path-utils.js:5-122.
```
if _.isObject(from): from = from.name; same for to
if from == to: return []
if !/(W|E)\d+(N|S)\d+$/.test(from) || !...(to): return ERR_NO_PATH     // NOT anchored at start
[fx,fy] = roomNameToXY(from); [tx,ty] = roomNameToXY(to)
if fx==tx && fy==ty: return []
originX = fx+30; originY = fy+30           // kRouteGrid = 30
xyToIndex(x,y): ox=originX-x, oy=originY-y; if ox∉[0,60) || oy∉[0,60): undefined; else ox*60+oy
indexToXY(i) = [originX - floor(i/60), originY - i%60]
h(x,y) = |x-tx| + |y-ty|                    // Manhattan, weight 1
reset heap (Float64 priorities, capacity 3600) and open/closed
heap.push(xyToIndex(fx,fy), h(fx,fy)); cb = (opts && opts.routeCallback) || (() => 1)
while heap.size():
    i = heap.min(); f = heap.minPriority(); heap.pop(); close(i)
    [x,y] = indexToXY(i); hc = h(x,y); g = f - hc
    if hc === 0: return reconstruct(i)
    fromName = getRoomNameFromXY(x,y)
    for dir in describeExits(fromName) (TOP,RIGHT,BOTTOM,LEFT; null → none):
        rn = exits[dir]; [nx,ny] = roomNameToXY(rn); ni = xyToIndex(nx,ny)
        if ni === undefined || closed(ni): continue
        cost = Number(cb(rn, fromName)) || 1
        if cost === Infinity: continue
        nf = g + h(nx,ny) + cost
        if open(ni): if heap.priority(ni) > nf: heap.update(ni, nf); parents[ni] = i
        else: heap.push(ni, nf); open(ni); parents[ni] = i
return ERR_NO_PATH
reconstruct(i): route = []; while i != fromIndex: [x,y]=indexToXY(i); i = parents[i]; [px,py]=indexToXY(i)
    exit = px<x ? FIND_EXIT_RIGHT : px>x ? FIND_EXIT_LEFT : py<y ? FIND_EXIT_BOTTOM : FIND_EXIT_TOP
    route.push({exit, room: getRoomNameFromXY(x,y)}); return route.reverse()
```
Heap (must be ported exactly for tie-breaking; reference/engine/src/game/path-utils.js:39-122):
```
push(i,p): pri[i]=p; heap[++size]=i; bubbleUp(size)
bubbleUp(k): while k != 1: if pri[heap[k]] <= pri[heap[k>>>1]]: swap(k, k>>>1); k >>>= 1  else return
pop(): heap[1]=heap[size]; size--; v=1
   loop: u=v
         if 2u+1 <= size: if pri[heap[u]] >= pri[heap[2u]]: v=2u
                          if pri[heap[v]] >= pri[heap[2u+1]]: v=2u+1
         elif 2u <= size: if pri[heap[u]] >= pri[heap[2u]]: v=2u
         if u != v: swap(u,v) else return
update(i,p): find position k scanning from size down to 1; pri[i]=p; bubbleUp(k)
```
(Equal priorities move: `<=` on bubble-up, `>=` on sift-down, right child preferred on ties.)
- **GOTCHA (cost coercion):** callback returning `0`, `NaN`, `undefined`, `false` → cost **1**; `Infinity` blocks; negative/fractional values allowed; strings numeric-coerced. Called per relaxation (possibly several times per room).
- **GOTCHA (search window):** only rooms with `x ∈ (fx−30, fx+30]` and `y ∈ (fy−30, fy+30]` are reachable (asymmetric: 29 west/north, 30 east/south) → farther targets return `ERR_NO_PATH`.
- **GOTCHA (closed rooms):** expansion requires `describeExits(room)` of the *current* room only; a target that is not accessible is still reached if a neighbour has an exit towards it.
- Lowercase room names fail the regex → `ERR_NO_PATH`. Route `room` names are canonical uppercase.
- **PERF:** world-global, ≤ 3600 nodes, `update` is O(heap). Runs per call; no caching.

### MAP-FINDEXIT — `Game.map.findExit(fromRoom, toRoom, [opts])`
Source :188-197. `route = this.findRoute(from, to, opts)`; non-array → return it (`ERR_NO_PATH`); empty → `ERR_INVALID_ARGS`; else `route[0].exit`.

### MAP-ISROOMAVAILABLE (deprecated)
Source :201-207. Deprecation message (CORE-DEPRECATED); regex `^(W|E)\d+(N|S)\d+$` else `false`; `_.contains(accessibleRooms, roomName)`.

### MAP-GETROOMSTATUS — `Game.map.getRoomStatus(roomName)`
Source :209-233.
```
if !/^(W|E)\d+(N|S)\d+$/.test(roomName): return undefined          // non-strings coerced by .test
if !runtimeData.roomStatusData: throw Error('No runtime status data')
closed[room] defined  → {status:'closed',  timestamp: closed[room]}
novice[room] defined  → {status:'novice',  timestamp: ...}
respawn[room] defined → {status:'respawn', timestamp: ...}
roomName ∈ accessibleRooms → {status:'normal', timestamp: null}
else → {status:'closed', timestamp: null}
```
`roomStatusData` and `accessibleRooms` are cached by the driver for 60 s (reference/driver/lib/runtime/data.js:77-97).

### MAP-GETTERRAINAT (deprecated)
Source :235-259. Deprecation message; if `_.isObject(x)`: `(x, y, roomName) = (x.x, x.y, x.roomName)`; out of 0..49 → `undefined`; no terrain for room → `undefined`; else `'wall'` (wall bit first), `'swamp'`, `'plain'`.

### MAP-GETROOMTERRAIN
Source :261-263. `new Room.Terrain(roomName)` (throws for unknown rooms).

### MAP-GETROOMLINEARDISTANCE — `getRoomLinearDistance(r1, r2, [continuous])`
Source :265-267 → reference/engine/src/utils.js:644-655.
`dx = |x2−x1|, dy = |y2−y1|` via `roomNameToXY`; if `continuous`: `dx = min(worldSize − dx, dx)`, same for dy; return `max(dx, dy)`. No validation (invalid names → NaN).

### MAP-GETWORLDSIZE
Source :269-271; value from reference/common/index.js:183-193: `max(maxX−minX+1, maxY−minY+1)` over all rooms, with `maxX`/`maxY` initialised to **0** (not −∞) — computed once at driver start.

### MAP-VISUAL-PROP
Source :274-377. `Game.map.visual` is an enumerable getter creating one object per `Game.map` (per tick) with non-enumerable methods MV-*.

---

## 9. Visuals

### VIS-BUFFER
Source: reference/engine/src/game/console.js:57-115.
Per user, per tick (re-created by `makeConsole` each tick, reference/driver/lib/runtime/runtime.js:222): `visual[roomKey]` = string. `addVisual(roomName, data)`:
```
roomName = roomName || ""                 // "" = all rooms
if !data: return
limit = roomName == "map" ? 1000*1024 : 500*1024          // UTF-16 code units
s = typeof data == string ? data : JSON.stringify(data) + "\n"
if current.length + s.length > limit:
    throw Error(roomName=="map" ? `MapVisual size has exceeded 1000 KB limit`
                                : `RoomVisual size in room ${roomName} has exceeded 500 KB limit`)
current += s
```
`getVisualSize(room)` → length or 0; `clearVisual(room)` → `""`; `getVisual(room)` → string or `undefined`. At tick end the whole map is shipped as `outMessage.visual` if non-empty (reference/driver/lib/runtime/runtime.js:282-285).
- **GOTCHA:** exceeding the limit throws (an uncaught user error), it does not truncate. Styles are passed through unvalidated (`JSON.stringify` drops `undefined` fields). Rendering defaults (radius 0.15, opacity 0.5, ...) are client-side (reference/docs/api/source/RoomVisual.md:91-390).
- **GOTCHA:** `new RoomVisual('map')` writes into the map-visual buffer.

### RV-CTOR
Source: reference/engine/src/game/rooms.js:1146-1148. `this.roomName = roomName` (own; `undefined` ⇒ all rooms). No validation.

### RV-CIRCLE / RV-LINE / RV-RECT / RV-POLY / RV-TEXT
Source :1150-1200. All return `this`. Records (`s` omitted from JSON when style undefined):
- circle: if `typeof x == 'object'`: `style = y; y = x.y; x = x.x` → `{t:'c', x, y, s}`.
- line: if `typeof x1 == 'object' && typeof y1 == 'object'`: `(style, x2, y2, y1, x1) = (x2, y1.x, y1.y, x1.y, x1.x)` → `{t:'l', x1, y1, x2, y2, s}`.
- rect: if `typeof x == 'object'`: `style=h; h=w; w=y; y=x.y; x=x.x` → `{t:'r', x, y, w, h, s}`.
- poly: only if `Array.isArray(points) && _.some(points)`; `points = points.map(i => i.x !== undefined ? [i.x, i.y] : i)` → `{t:'p', points, s}`; otherwise no-op.
- text: if `typeof x == 'object'`: `style = y; y = x.y; x = x.x` → `{t:'t', text, x, y, s}` (`text` not coerced).
- **GOTCHA:** the "object" forms read `.x/.y` directly: a RoomObject (which has `.pos`, not `.x`) yields records without coordinates; in `poly` a RoomObject element is serialised whole. `null` as x throws TypeError (`typeof null == 'object'`).

### RV-GETSIZE / RV-CLEAR / RV-EXPORT / RV-IMPORT
Source :1202-1218. `getSize()` → buffer length (code units); `clear()` → empties this room's buffer, returns `this`; `export()` → buffer string or `undefined`; `import(data)` → `addVisual(roomName, ""+data)` appended verbatim (no newline added; empty string ignored), returns `this`.

### MV-CIRCLE / MV-LINE / MV-RECT / MV-POLY / MV-TEXT
Source: reference/engine/src/game/map.js:281-349. Positions must be `instanceof RoomPosition` else throw `'Invalid pos, RoomPosition expected'` (`'Invalid pos1, ...'`, `'Invalid pos2, ...'`; text: `'Invalid pos , RoomPosition expected'` with the stray space). Records, `s: style || {}` always present:
- circle `{t:'c', x, y, n: roomName, s}`; line `{t:'l', x1, y1, n1, x2, y2, n2, s}`; rect `{t:'r', x, y, n, w, h, s}`; text `{t:'t', text, x, y, n, s}`;
- poly: if array with a truthy element: `points.map(i => {p = i.pos || i; return {x:p.x, y:p.y, n:p.roomName}})` (no instanceof check) → `{t:'p', points, s}`.
All return `this` (the visual object).

### MV-CLEAR / MV-GETSIZE / MV-EXPORT / MV-IMPORT
Source :350-371. Same as RV-* on the `"map"` buffer (1000 KB limit).

---

## 10. Mod hooks present in vanilla

### MOD-CUSTOM-PROTO
Sources: reference/driver/lib/index.js:54-73; reference/driver/lib/runtime/runtime.js:34-44; reference/engine/src/game/custom-prototypes.js:8-58; reference/engine/src/game/game.js:52-65, :384-393; reference/engine/src/game/rooms.js:516-523, :787-793, :836-841.
`config.engine.registerCustomObjectPrototype(objectType, name, {parent, properties, prototypeExtender, userOwned, findConstant, lookConstant})`. Property fns are stringified and `eval`ed inside the isolate. Per tick:
- class `globals[name]` extends `globals[parent]` (ctor `parent.call(this, id)`) or `RoomObject` (no effects passed); `properties` via `defineGameObjectProperties`; `userOwned` adds `my` (`user == me`) and `owner`; `prototypeExtender(proto, scope, {utils})`.
- raw objects with `type == objectType` are instantiated in the register pass (after built-in handling for that object), added to `customObjects`, to `findCache[findConstant]` (no predicate) and to the `lookConstant` register/spatial index; `lookAt`/`lookAtArea` append custom look types after the built-ins in registration order; `lookForAt` accepts them.
chitin's mod system should be a superset of this hook so existing server mods that only use it keep working.

### MOD-CUSTOM-INTENTS
Source: reference/common/lib/system.js:95-140; reference/backend/lib/game/server.js:38. `config.engine.customIntentTypes` / `config.backend.customIntentTypes` field-type maps extend sanitisation for object, room and global intents (same transforms as CORE-ROOM-INTENTS).

---

## 11. PERF summary for chitin

- **Room-local (parallelisable per room):** `find`, all `look*`, default cost matrices, `findPath` (single-room portion), construction-site placement checks against room objects, event-log parse, exit lists.
- **User-global:** `createFlag` name checks (all of the player's flags), construction-site cap (all of the player's objects), `Game.flags`/`Game.creeps` construction, CPU intent accounting, visuals buffer.
- **World-global, static:** terrain (shared read-only), map grid / `describeExits` / `findRoute`, room status and accessible-room list (the original re-`JSON.parse`s the whole accessible-room list per user per tick in `makeMap`, reference/engine/src/game/map.js:18, and uses linear `_.contains` scans — use a hash set).
- **Original hot spots to beat:** eager per-user instantiation of every visible object (CORE-REGISTER); `_.clone` on every `find`; `_.filter` with object matchers (generic deep compare); linear scans in `createConstructionSite`; O(n²) flag auto-naming; per-call `Room.Terrain` closures; `costCallback` matrix clones; `findRoute`'s O(heap) `update`.
- **Caches that are observable and must keep tick scope:** find cache (tick-start snapshot, exit cache bug), parsed event log identity, cost-matrix cache (returned uncloned to native only), getter `_name` caches.

---

## Open questions

1. **DB return order.** The exact order of `runtimeData.roomObjects` depends on the storage backend: MongoDB natural order on the MMO (usually but not guaranteed insertion order), LokiJS with binary indices on `room`/`user` on the private server (reference/storage/lib/db.js:333-340), where an indexed `$in` query may group foreign objects by room. The reference does not pin this down; chitin needs a documented deterministic rule (recommendation in CORE-OBJECT-ORDER) and a decision whether to emulate Loki or Mongo.
2. **Legacy pathfinder tie-breaking.** `@screeps/pathfinding`'s `AStarFinder`/`DijkstraFinder` use the npm `heap` package, which is not in `reference/`; exact legacy (`PathFinder.use(false)`) results cannot be reproduced without it.
3. **Native search details** (JPS neighbour order, heap ties, `ops` counting, behaviour for goals in rooms without terrain) are in reference/driver/native/src/pf.cc and belong to the native pathfinder spec; this spec only fixes the JS wrapper contract.
4. **MMO vs this commit.** Whether the official MMO uses the same `kMaxWorldSize = 256` packing, the same `kRouteGrid = 30` route window, and the same `describeExits` plain-only edge counting cannot be verified from the reference.
5. **Accessible-room list freshness.** `accessibleRooms`/`roomStatusData` are cached 60 s by the driver and the map grid is built once per isolate; whether chitin should reproduce these staleness windows (observable around room openings) is a product decision.
