# 11 — Persistent world data model, world generation, room naming, constants

Status: implementation spec for **chitin** (Rust, std-only reimplementation of the Screeps: World server).
Scope: everything that is *stored* (DB collections, env keys, room-object documents and their fields), how the
world is *named and laid out* (room coordinates, sectors, highways, center/SK rooms, room status), how rooms are
*generated* (`map.generateRoom` and the cron jobs that later spawn power banks / deposits / strongholds / invaders),
and the `constants.js` table that will be ported wholesale.

Reference versions: engine @8097782, driver @cf63d8a, common @2fb779b, storage @3d6c3d0, backend 3.3.0,
launcher 4.2.0, lodash 3.10.1 (every package depends on lodash 3.x — `reference/<pkg>/package.json`).
Citations are `reference/<pkg>/<path>:<line>`. `db.original.json` / `init_dist/db.json` are single-line JSON
files; they are cited without a line. LokiJS is not under `reference/`: storage pins `lokijs ^1.5.6`
`reference/storage/package.json:16`; the local copy used for this spec is **1.5.9**
(`E:\screeps\server\node_modules\lokijs\src\lokijs.js`), cited as `lokijs.js:<line>`. The server requires
Node ≥ 22.9 (`E:\screeps\server\package.json` `engines`), so `Array.prototype.sort` is V8 TimSort, i.e. **stable**.

Conventions used below:

* "doc" = a JSON document in a collection. All docs carry LokiJS bookkeeping `meta` `{revision, created, version, updated}`
  and `$loki` (integer). These are **not** game data (see STORE-BACKEND) but they are physically present and are sent
  over the websocket (WIRE-ROOM-DIFF).
* `null` vs *absent* matters for several fields; it is called out explicitly.
* "ms" = JavaScript `Date.now()` milliseconds; "tick" = game time.
* **GOTCHA** = 1:1 pitfall; **PERF** = performance-relevant structure.

---

## 1. Inventory

| ID | Item | Section |
|---|---|---|
| STORE-BACKEND | LokiJS in-memory DB in one storage process, RPC access | 2.1 |
| STORE-COLLECTION-LIST | The 20 registered collections + `env` | 2.2 |
| STORE-INDEXES | Unique `_id` + binary indexes per collection | 2.3 |
| STORE-ID-GEN | `_id` generation (`genId`, Math.random + time) | 2.4 |
| STORE-QUERY-NE-NULL | `{$ne:null}` / `{$eq:null}` query rewriting | 2.5 |
| STORE-QUERY-OPS | LokiJS `$ne` (strict, NaN case), `$gt`, `$in` operator semantics | 2.5 |
| STORE-UPDATE-OPERATORS | `$set $merge $inc $unset $addToSet $pull` semantics | 2.6 |
| STORE-UPDATE-UPSERT | update fast path by `_id`, upsert doc construction | 2.7 |
| STORE-BULK-RPC | `dbBulk` op list (update/insert/remove) | 2.8 |
| STORE-FINDEX | `findEx` sort/offset/limit | 2.9 |
| STORE-BULK-WRITER | driver `bulk.js` deep-merge write buffer | 2.10 |
| STORE-HIDDEN-FIELDS | `_`-prefixed fields never persist | 2.11 |
| STORE-PERSISTENCE | autosave every 10 s, whole-DB JSON | 2.12 |
| STORE-QUEUES | `usersIvm` / `usersLegacy` / `rooms` work queues (LIFO pop) | 2.13 |
| STORE-PUBSUB | pubsub channels | 2.14 |
| STORE-RESET | `resetAllData` reload of `db.original.json` | 2.15 |
| STORE-MIGRATIONS | DB upgrade v4..v9 | 2.16 |
| STORE-RPC-SERIALIZATION | JSON over RPC: `undefined` dropped, `Date`→string | 2.17 |
| STORE-INDEX-MODE | adaptive vs non-adaptive binary indices, per collection | 2.18 |
| STORE-ORDER-INDEXED | `find({k:v})` on a non-adaptive index = data[] (creation) order | 2.19 |
| STORE-ORDER-ADAPTIVE | adaptive index = most recently written first; batch-remove rebuild | 2.20 |
| STORE-ORDER-IN | `$in` on an indexed field groups rows by `$in` array order | 2.21 |
| STORE-ORDER-SCAN | unindexed queries, `$and`, `findEx` sorts: data[] order, stable | 2.22 |
| STORE-ORDER-RELOAD | saved index state on load (shipped DBs, reset) | 2.23 |
| ORDER-ROOM-OBJECTS | canonical per-room object order = creation order (processor, backend) | 2.24 |
| ORDER-RUNTIME-OBJECTS | runtime `roomObjects`: foreign grouped by `roomIds`, then own | 2.25 |
| ORDER-RUNTIME-ROOMIDS | `roomIds` construction (first appearance, observer targets) | 2.25 |
| ORDER-GAME-ROOMS | `Game.rooms` key order = `rooms` collection insertion order | 2.26 |
| ORDER-CONSOLE-COMMANDS | queued console / `/memory` expressions run newest-first | 2.27 |
| ORDER-POWER-CREEPS | `Game.powerCreeps` key order (adaptive `users.power_creeps`) | 2.28 |
| ORDER-MISC | users, flags, transactions, market and other result orders | 2.29 |
| ENV-LAYOUT | env = Loki collection with doc 1 (values) and doc 2 (expirations) | 3.1 |
| ENV-OPS | Redis-like env API semantics | 3.2 |
| ENV-EXPIRY | expiration sweep every 10 s | 3.3 |
| ENV-GAMETIME | `gameTime` | 3.4 |
| ENV-ACCESSIBLE-ROOMS | `accessibleRooms` (JSON string) | 3.5 |
| ENV-ROOM-STATUS-DATA | `roomStatusData` (JSON string) | 3.6 |
| ENV-TERRAIN-DATA | `terrainData` (base64 deflate JSON) | 3.7 |
| ENV-ACTIVE-ROOMS | `activeRooms` set (rooms to process next tick) | 3.8 |
| ENV-MEMORY | `memory:<userId>` | 3.9 |
| ENV-MEMORY-SEGMENTS | `memorySegments:<userId>` hash | 3.10 |
| ENV-PUBLIC-MEMORY-SEGMENTS | `publicMemorySegments:<userId>` | 3.11 |
| ENV-MAP-VIEW | `mapView:<room>` | 3.12 |
| ENV-ROOM-HISTORY | `roomHistory:<room>` hash | 3.13 |
| ENV-ROOM-VISUAL | `roomVisual:<user>,<room>,<tick>` | 3.14 |
| ENV-ROOM-EVENT-LOG | `roomEventLog:` single hash | 3.15 |
| ENV-USER-ONLINE | `userOnline:<userId>` | 3.16 |
| ENV-MAIN-LOOP-PAUSED | `mainLoopPaused` | 3.17 |
| ENV-TICK-RATE | `tickRate` (MAIN_LOOP_MIN_DURATION) | 3.18 |
| ENV-AUTH-TOKENS | `auth_<token>` | 3.19 |
| ENV-LEGACY-KEYS | `scriptCachedData:`, `scrUserMemory:`, `scrScriptCachedData:` | 3.20 |
| COL-ROOMS | `rooms` doc schema | 4.1 |
| COL-ROOMS-TERRAIN | `rooms.terrain` doc schema | 4.2 |
| COL-ROOMS-OBJECTS | `rooms.objects` envelope (details in §6) | 4.3 |
| COL-ROOMS-FLAGS | `rooms.flags` doc + packed flag string | 4.4 |
| COL-ROOMS-INTENTS | `rooms.intents` doc | 4.5 |
| COL-USERS | `users` doc schema (all fields) | 4.6 |
| COL-USERS-CODE | `users.code` | 4.7 |
| COL-USERS-CONSOLE | `users.console` | 4.8 |
| COL-USERS-MESSAGES | `users.messages` | 4.9 |
| COL-USERS-NOTIFICATIONS | `users.notifications` | 4.10 |
| COL-USERS-MONEY | `users.money` | 4.11 |
| COL-USERS-RESOURCES | `users.resources` | 4.12 |
| COL-USERS-POWER-CREEPS | `users.power_creeps` | 4.13 |
| COL-USERS-INTENTS | `users.intents` (global intents) | 4.14 |
| COL-MARKET-ORDERS | `market.orders` | 4.15 |
| COL-MARKET-STATS | `market.stats` | 4.16 |
| COL-TRANSACTIONS | `transactions` | 4.17 |
| COL-LEADERBOARD | `leaderboard.*` (registered, unused) | 4.18 |
| COL-MARKET-INTENTS-LEGACY | `market.intents` (in default DB only) | 4.19 |
| USER-SPECIAL-IDS | `'2'` Invader, `'3'` Source Keeper, system user | 5.1 |
| USER-CPU-FIELDS | `cpu`, `cpuAvailable`, `lastUsedCpu`, … | 5.2 |
| USER-GCL-POWER-MONEY | `gcl`, `power`, `money` units | 5.3 |
| USER-BADGE | badge object | 5.4 |
| USER-ROOMS-LIST | `rooms[]` maintenance | 5.5 |
| USER-ACTIVE-FLAG | `active` and who runs each tick | 5.6 |
| USER-SEGMENT-FIELDS | `activeSegments`, `defaultPublicSegment`, `activeForeignSegment` | 5.7 |
| USER-CREATE-PATHS | every code path that inserts a user doc | 5.8 |
| OBJ-ENVELOPE | fields common to all room objects | 6.1 |
| OBJ-STORE-MODEL | `store` / `storeCapacity` / `storeCapacityResource` | 6.2 |
| OBJ-EFFECTS | `effects[]` entries | 6.3 |
| OBJ-ACTIONLOG | `actionLog` per type | 6.4 |
| OBJ-NOTIFY | `notifyWhenAttacked` | 6.5 |
| OBJ-OWNERSHIP | `user` field conventions | 6.6 |
| OBJ-CUSTOM-TYPES | schemaless docs / mod object types | 6.7 |
| OBJ-CREEP | creep | 6.8 |
| OBJ-CREEP-BIRTH | spawn ticks before its creep; `ageTime` set in the birth tick | 6.8 |
| OBJ-CREEP-KEEPER | source keeper creep | 6.9 |
| OBJ-CREEP-INVADER | invader creep (cron / summoned) | 6.10 |
| OBJ-CREEP-STRONGHOLD | stronghold defender creep | 6.11 |
| OBJ-POWERCREEP | power creep in room | 6.12 |
| OBJ-SOURCE | source | 6.13 |
| OBJ-MINERAL | mineral | 6.14 |
| OBJ-DEPOSIT | deposit | 6.15 |
| OBJ-CONTROLLER | controller | 6.16 |
| OBJ-KEEPERLAIR | keeper lair | 6.17 |
| OBJ-POWERBANK | power bank | 6.18 |
| OBJ-PORTAL | portal | 6.19 |
| OBJ-INVADERCORE | invader core | 6.20 |
| OBJ-SPAWN | spawn | 6.21 |
| OBJ-EXTENSION | extension | 6.22 |
| OBJ-ROAD | road | 6.23 |
| OBJ-CONSTRUCTEDWALL | constructed wall (incl. newbie/portal walls) | 6.24 |
| OBJ-RAMPART | rampart (incl. PWR_SHIELD and stronghold ramparts) | 6.25 |
| OBJ-LINK | link | 6.26 |
| OBJ-STORAGE | storage | 6.27 |
| OBJ-TOWER | tower | 6.28 |
| OBJ-OBSERVER | observer | 6.29 |
| OBJ-POWERSPAWN | power spawn | 6.30 |
| OBJ-EXTRACTOR | extractor | 6.31 |
| OBJ-LAB | lab | 6.32 |
| OBJ-TERMINAL | terminal | 6.33 |
| OBJ-CONTAINER | container | 6.34 |
| OBJ-NUKER | nuker | 6.35 |
| OBJ-FACTORY | factory | 6.36 |
| OBJ-CONSTRUCTIONSITE | construction site | 6.37 |
| OBJ-ENERGY | dropped resource (`type:'energy'` for every resource) | 6.38 |
| OBJ-TOMBSTONE | tombstone (creep and power creep variants) | 6.39 |
| OBJ-RUIN | ruin | 6.40 |
| OBJ-NUKE | nuke in flight | 6.41 |
| OBJ-FLAG | flags as stored (rooms.flags) + legacy `flag` objects | 6.42 |
| WIRE-ROOM-DIFF | `common.getDiff` semantics for `room:` websocket channel | 7.1 |
| WIRE-ROOM-EVENT | `room:<name>` event payload shape | 7.2 |
| WIRE-MAP-VIEW | `roomMap2:<name>` payload | 7.3 |
| TERRAIN-STRING | 2500-char terrain string, codes 0..3 | 8.1 |
| TERRAIN-CODE3 | code 3 = wall+swamp, how each consumer reads it | 8.2 |
| TERRAIN-ENCODE-DECODE | encode/decode helpers | 8.3 |
| TERRAIN-CHECK | `checkTerrain` helpers | 8.4 |
| TERRAIN-WORLD-BLOB | `updateTerrainData` world blob incl. walled ring | 8.5 |
| TERRAIN-RUNTIME-BUFFER | shared Uint8Array per process | 8.6 |
| TERRAIN-API | `Room.Terrain.get/getRawBuffer`, `getTerrainAt` | 8.7 |
| TERRAIN-PF-PACK | 2-bit column-major pack for native pathfinder | 8.8 |
| TERRAIN-EXIT-GRID | `WorldMapGrid.gridData` exit counts (plain only) | 8.9 |
| TERRAIN-PNG | map image assets | 8.10 |
| ROOMNAME-COORDS | room name ↔ integer coordinates, W0/E0 seam | 9.1 |
| ROOMNAME-PARSE | the 5 parser variants and their differences | 9.2 |
| ROOMNAME-FORMAT | `getRoomNameFromXY` + cached variant (`'sim'`) | 9.3 |
| ROOMNAME-VALIDATE | validation regexes in use | 9.4 |
| ROOMNAME-POS-PACK | `RoomPosition.__packedPos` limits (W127..E127) | 9.5 |
| ROOMNAME-PF-COORDS | pathfinder room coordinates | 9.6 |
| ROOMNAME-WORLD-SIZE | `calcWorldSize` / `getWorldSize` | 9.7 |
| ROOMNAME-DISTANCE | `calcRoomsDistance` (incl. continuous wrap) | 9.8 |
| ROOMNAME-EDGE-CROSSING | creep/power creep crossing to a neighbour room | 9.9 |
| ROOMNAME-DESCRIBE-EXITS | `Game.map.describeExits` from grid data | 9.10 |
| SECTOR-LAYOUT | 10×10 sectors | 10.1 |
| SECTOR-HIGHWAY | highway (“bus”) rooms, `isBus`, `rooms.bus` | 10.2 |
| SECTOR-CENTER | center / SK rooms, `isCenter`, `isVeryCenter` | 10.3 |
| SECTOR-DEPOSIT-TYPE | `depositType` assignment formula | 10.4 |
| SECTOR-REGEX | sector regexes used by cron jobs | 10.5 |
| STATUS-FIELDS | `status`, `openTime`, `novice`, `respawnArea` | 11.1 |
| STATUS-OPEN-CLOSE | CLI `openRoom` / `closeRoom` | 11.2 |
| STATUS-ACCESSIBLE | accessible-room list computation | 11.3 |
| STATUS-DATA | status-data computation | 11.4 |
| STATUS-API | `Game.map.getRoomStatus` / `isRoomAvailable` | 11.5 |
| STATUS-GATES | where status changes game behaviour | 11.6 |
| GEN-ROOM-ENTRY | `map.generateRoom` entry, validation, defaults | 12.1 |
| GEN-EXIT-MATCH | neighbour exit matching | 12.2 |
| GEN-EXIT-RANDOM | `_genExit` random exits | 12.3 |
| GEN-TERRAIN-TABLES | wall (1..28) and swamp (1..14) type tables | 12.4 |
| GEN-TERRAIN-FILL | random fill + forced exits | 12.5 |
| GEN-SMOOTH | cellular smoothing | 12.6 |
| GEN-FLOOD | 8-connectivity check | 12.7 |
| GEN-SOURCES | source placement | 12.8 |
| GEN-KEEPER-LAIRS | keeper lair placement | 12.9 |
| GEN-CONTROLLER | controller placement | 12.10 |
| GEN-EMIT | grid → terrain string + objects (swamp filter) | 12.11 |
| GEN-MINERAL | mineral type, position, density | 12.12 |
| GEN-INSERT | documents written by generateRoom | 12.13 |
| GEN-RNG-ORDER | exact Math.random consumption order | 12.14 |
| GEN-POST | post-steps (PNG, terrain blob) and what they do *not* do | 12.15 |
| GEN-REMOVE-ROOM | `map.removeRoom` | 12.16 |
| GEN-DEFAULT-WORLD | contents of the shipped `db.original.json` / `init_dist/db.json` | 12.17 |
| GEN-BOT-SPAWN | CLI `bots.spawn` (user + spawn + controller) | 12.18 |
| GEN-PLACE-SPAWN | HTTP `place-spawn` (first spawn of a player) | 12.19 |
| GEN-RESPAWN | `respawnUser` | 12.20 |
| GEN-USER-REGISTER | steam registration user doc | 12.21 |
| CRON-SCHEDULER | backend cron runner | 13.1 |
| CRON-ROOMS-FORCE-UPDATE | periodic activation of idle rooms | 13.2 |
| CRON-POWER-BANKS | power bank spawning | 13.3 |
| CRON-DEPOSITS | deposit spawning | 13.4 |
| CRON-INVADERS | invader raids | 13.5 |
| CRON-STRONGHOLD-GEN | stronghold spawning | 13.6 |
| CRON-STRONGHOLD-EXPAND | stronghold expansion (level-0 cores) | 13.7 |
| CRON-NPC-ORDERS | NPC terminal market orders | 13.8 |
| CRON-DELETE-POWER-CREEPS | delayed power creep deletion | 13.9 |
| CRON-HOUSEKEEPING | notifications, transaction purge, market stats | 13.10 |
| CONST-PORT-RULES | how to port constants.js (structure, ordering, mutability) | 14.1 |
| CONST-EXPOSURE | constants as player globals (JSON round trip) | 14.2 |
| CONST-SYSTEM-INTENT-SCHEMA | `system.js` intent field transforms | 14.3 |
| CONST-RETURN-CODES | `OK`, `ERR_*` | 14.4 |
| CONST-FIND | `FIND_*` | 14.4 |
| CONST-DIRECTIONS | `TOP`..`TOP_LEFT` | 14.4 |
| CONST-COLORS | `COLOR_*` | 14.4 |
| CONST-LOOK | `LOOK_*` | 14.4 |
| CONST-OBSTACLES | `OBSTACLE_OBJECT_TYPES` | 14.4 |
| CONST-BODYPARTS | body part names, `BODYPART_COST` | 14.4 |
| CONST-CREEP-ACTIONS | creep life/action powers | 14.4 |
| CONST-STRUCTURE-TYPES | `STRUCTURE_*` | 14.4 |
| CONST-STRUCTURE-STATS | hits/capacities/decay per structure | 14.4 |
| CONST-CONSTRUCTION-COST | `CONSTRUCTION_COST` + road ratios | 14.4 |
| CONST-CONTROLLER | `CONTROLLER_*`, `SAFE_MODE_*`, `GCL_*` | 14.4 |
| CONST-MINERALS-DEPOSITS | `MINERAL_*`, `DENSITY_*`, `DEPOSIT_*` | 14.4 |
| CONST-MARKET | `ORDER_*`, `MARKET_*`, intershard names | 14.4 |
| CONST-RESOURCES | `RESOURCE_*` (84) | 14.4 |
| CONST-REACTIONS | `REACTIONS` | 14.5 |
| CONST-BOOSTS | `BOOSTS` | 14.5 |
| CONST-REACTION-TIME | `REACTION_TIME` | 14.5 |
| CONST-POWER | `POWER_*`, `PWR_*`, `EFFECT_*` | 14.4 |
| CONST-POWER-INFO | `POWER_INFO` | 14.5 |
| CONST-COMMODITIES | `COMMODITIES` | 14.5 |
| CONST-EVENTS | `EVENT_*` | 14.4 |
| CONST-NPC | invader core / stronghold / invader constants | 14.4 |
| CONST-LISTS | `BODYPARTS_ALL`, `RESOURCES_ALL`, `COLORS_ALL`, `INTERSHARD_RESOURCES` | 14.5 |
| CONST-UNUSED | constants not referenced by server code | 14.6 |
| CONST-STRONGHOLD-TABLES | `common/lib/strongholds.js` templates & reward tables | 14.7 |
| CONST-GEN-TABLES | constants embedded in generator/cron code (not in constants.js) | 14.8 |

---

## 2. Storage layer

### 2.1 STORE-BACKEND
* One storage process hosts a LokiJS database (`new loki(DB_PATH, {autosave:true, autosaveInterval:10000})`)
  `reference/storage/lib/db.js:14-24`; all other processes (main loop, runners, processors, backend) talk to it
  over a TCP RPC socket (`STORAGE_PORT`) `reference/common/lib/storage.js:62-110`, `reference/storage/lib/index.js:13-58`.
* Each collection proxy exposes `find findOne by clear count ensureIndex removeWhere insert` (raw Loki calls),
  `update(query, update, params)`, `bulk(ops)`, `findEx(query, opts)` `reference/common/lib/storage.js:91-108`.
* `find` returns Loki docs; over RPC they are JSON-serialized (so `meta` and `$loki` travel with every doc).
* **PERF**: every query is a cross-process JSON round trip; there is no transaction. The original server is
  bottlenecked here (single-threaded Loki + JSON). chitin should keep the *semantics* (doc shapes, update operators)
  but can hold room-local state in-process. `meta`/`$loki` need not exist in chitin's store unless a client depends
  on them (see Open questions).

### 2.2 STORE-COLLECTION-LIST
Registered (`config.common.dbCollections`) `reference/common/lib/storage.js:6-27`:
`leaderboard.power, leaderboard.seasons, leaderboard.world, users.intents, market.orders, market.stats, rooms,
rooms.objects, rooms.flags, rooms.intents, rooms.terrain, transactions, users, users.code, users.console,
users.messages, users.money, users.notifications, users.resources, users.power_creeps` (20).
Plus `env` (not in the list; accessed only through the env API, §3). Collections are created lazily on first
access `reference/storage/lib/db.js:330-335`. The default DB additionally contains `market.intents` (legacy, unused).

### 2.3 STORE-INDEXES
`getOrAddCollection` `reference/storage/lib/db.js:330-389`: unique index `_id` on every collection; binary indexes:
`rooms.objects`: `room`, `user`; `rooms.intents`: `room`; `users`: `username`; `rooms.flags`: `room`, `user`;
`rooms.terrain`: `room`; `transactions, users.code, users.console, users.money, users.notifications,
users.resources, users.power_creeps`: `user`.
`rooms`, `users.messages`, `users.intents`, `market.orders`, `market.stats`, `leaderboard.*` have **no** binary
index (the unique `_id` index is only used by `by('_id')`, never by `find`). Whether an index is maintained
adaptively or lazily differs per collection and decides result order: STORE-INDEX-MODE (2.18).
**PERF**: these are the only access paths the original optimises; chitin's equivalents are: objects by room
(room-local, hot), objects by user (runner, cross-room), users by id/username.
**PERF**: on the non-adaptive `rooms.objects`, *every* insert/update/remove marks both indices dirty, and the
next storage request of any kind on that collection re-sorts both indices over **all objects in the world**
(O(N log N)): `getOrAddCollection` calls `ensureIndex('room')`/`ensureIndex('user')` on every request
`reference/storage/lib/db.js:330-341`, `lokijs.js:5405-5452`. With processors committing between other
processors' reads this happens many times per tick. chitin keeps
per-room and per-user lists ordered by a creation sequence instead (ORDER-ROOM-OBJECTS).

### 2.4 STORE-ID-GEN
`reference/storage/lib/db.js:317-328`:
```
rand4()  = Math.floor(Math.random()*0x10000).toString(16) left-padded with '0' to 4 chars
genId(o) = rand4() + Date.now().toString(16).slice(4) + rand4();  if o && !o._id: o._id = id
```
* Today `Date.now().toString(16)` is 11 hex chars → ids are 15 lowercase hex chars (matches every id in
  `db.original.json`). Two `Math.random()` calls per id.
* Applied on `insert` (each element of an array insert, in order) `reference/storage/lib/db.js:414-421`, on bulk
  `insert` `:493-496`, and on upsert `:466`. An explicit `_id` in the doc is kept (power creeps, §6.12).
* **GOTCHA**: ids are assigned by the storage process at *commit* time. Objects created during a processor tick
  have no real id until the bulk executes; processors key them in `roomObjects` under temporary keys
  (`'createdStructure'+n` `reference/engine/src/processor/intents/creeps/build.js:289`,
  `'_createdConstructionSite'+n` `.../room/create-construction-site.js:73`, `'deployedStructure'+n`
  `.../invader-core/stronghold/stronghold.js:129`, or the bulk insert counter for dropped resources
  `reference/engine/src/processor/intents/_create-energy.js:50`).
* chitin may use any id scheme that is a string, unique, and stable; bots treat ids as opaque strings. Keep them
  short (15–24 chars) because Memory sizes depend on it.

### 2.5 STORE-QUERY-NE-NULL
Before `find/findOne/count/removeWhere/update/findEx`, `recursReplaceNeNull` rewrites `{f:{$ne:null}}` into
`$and:[{f:{$ne:null}},{f:{$ne:undefined}}]` and `{f:{$eq:null}}` into `$or:[{f:{$eq:null}},{f:{$eq:undefined}}]`
(only if the containing object has no `$and`/`$or` already) `reference/storage/lib/db.js:391-407,423-425,438,517`.
Semantics to reproduce: **`null` and absent are equivalent in `$ne:null` / `$eq:null` queries.**
(`insert` and `by` are not rewritten.)

**STORE-QUERY-OPS** (LokiJS 1.5.9 operators used by server code):
* `$ne`: `if (b !== b) return a === a; return a !== b` `lokijs.js:473-481` — strict inequality; a `NaN` query
  value matches every non-NaN field. No type coercion. Hence `getAllUsers`
  (`{$and:[{active:{$ne:0}},{cpu:{$gt:0}}]}` `reference/driver/lib/index.js:144-151`) **keeps** users with
  `active:false`, `active:'0'`, `active:null` and absent `active`; only the number `0` is excluded.
* `$eq` (implicit `{k:v}`): strict `===` (on an indexed field the index narrows the range first, then `===`
  filters) `lokijs.js:675-685,3615-3638`.
* `$gt`/`$gte`/`$lt`/`$lte`: Loki comparators `gtHelper`/`ltHelper` `lokijs.js:222-355`: when either side is
  falsy, `true`, or NaN, values are ranked `undefined = null (1) < false (3) < true (4) < "" (5) < other (9)` and
  compared by rank; two "other" values compare as `Number()` if both numeric, numbers before non-numeric
  strings, else by string. So `cpu:{$gt:0}` keeps numbers > 0 and numeric strings > 0, and drops
  `undefined/null/false/true/""`.
* `$in`: `b.indexOf(a) !== -1` (strict) `lokijs.js:532-534`; on an indexed field it is answered from the index
  (STORE-ORDER-IN).

### 2.6 STORE-UPDATE-OPERATORS
`updateDocument(doc, update)` `reference/storage/lib/db.js:257-293`, applied in this fixed order:
1. `$set`: shallow `_.extend(doc, $set)` (top-level replace; nested objects replaced wholesale).
2. `$merge`: lodash-3 deep `_.merge(doc, $merge)` (used for `rooms.intents`, `activeForeignSegment`, `steam`).
3. `$inc`: `doc[key] = (doc[key]||0) + val` — **GOTCHA**: `key` is used literally; a dotted key such as
   `'resources.pixel'` (market direct deals `reference/engine/src/processor/global-intents/market.js:435,465`)
   creates a top-level property named `"resources.pixel"`, it does **not** touch `doc.resources.pixel`.
4. `$unset`: `delete doc[key]` for each key.
5. `$addToSet`: create array if missing; push if `indexOf == -1` (strict equality).
6. `$pull`: remove the first occurrence only (`indexOf` + `splice(idx,1)`).

### 2.7 STORE-UPDATE-UPSERT
`dbUpdate` `reference/storage/lib/db.js:436-478`:
* If the query has exactly one key `_id` with a string value → direct `by('_id')` lookup; else `find(query)`.
* Every matched doc is updated; result `{modified:n}`.
* No match and `params.upsert` → new doc = union of `query.$and[*]` objects (or the plain query) then
  `updateDocument`, `genId`, insert; result `{inserted:1}`. **GOTCHA**: query operators become literal fields of the
  upserted doc (e.g. `sendNotification` upserts with `date:{$lte:now}` in the query, then `$set` overwrites `date`)
  `reference/driver/lib/index.js:455-471`.

### 2.8 STORE-BULK-RPC
`dbBulk(collection, ops)` `reference/storage/lib/db.js:480-513` executes ops in array order:
`{op:'update', id, update}` (by `_id`, silently skipped if missing), `{op:'insert', data}` (genId),
`{op:'remove', id}`. Not atomic across ops; errors abort the remaining ops.

### 2.9 STORE-FINDEX
`findEx(query,{sort,offset,limit})`: `simplesort(field, desc)` for each sort key in object order, then offset, then
limit `reference/storage/lib/db.js:515-537`. Used for transactions (last 100) and money history.

### 2.10 STORE-BULK-WRITER
The driver's per-collection write buffer `reference/driver/lib/bulk.js:24-131` is how all game processing writes:
* `update(idOrDoc, data)` `:33-61`: deep-clones `data`; for every key whose value is an object/array, the stored
  in-memory value `doc[key]` (or `{}`) is `_.merge`d with it and `data[key]` becomes the merged whole; then
  `_.merge(doc, data)` (the caller's in-memory object is mutated); `removeHidden(data)`; accumulated per id
  with `_.extend`. Updates to an id that was inserted in the same bulk are folded into the insert.
* `execute()` `:120-129` emits, in this order: the op list built by `remove/inc/addToSet/pull` (in call order), then
  one `{op:'update', update:{$set: accumulated}}` per updated id, then one insert per inserted doc.
* `insert(data, id?)` `:62-75` deep-clones, `removeHidden`, optional explicit `_id`, returns an integer counter
  (temporary id). `remove(id)` of a pending insert cancels it.
* `inc(id,key,amount)` mutates the in-memory object and queues `$inc` `:88-99`.
* **GOTCHA (data shape)**: because object-valued fields are *merged*, a key can never be removed from a nested
  object through `bulk.update`. The engine therefore writes `0` or `null` instead of deleting:
  `store` keys stay at `0` after being emptied; lab `storeCapacityResource[mineral] = null`
  `reference/engine/src/processor/intents/labs/run-reaction.js:71-75`; effects are replaced by writing
  `effects:null` then `effects:[...]` `reference/engine/src/processor/intents/power-creeps/usePower.js:287-288`.
  Arrays are merged index-wise (lodash merge), so shrinking an array via `bulk.update` leaves trailing elements.
  chitin must reproduce the *visible* consequences: zero-valued `store` keys persist in the doc (hidden by the
  `Store` API but visible to the websocket diff and to `_.sum(object.store)` style code).
* **GOTCHA**: `bulk.update(obj, {x: undefined})` is equivalent to "no change" after JSON transport; `null` is a
  real value stored in the doc.
* **GOTCHA (pending inserts)**: `update(doc)` resolves the target by `doc._id`. A doc inserted earlier in the same
  tick has no `_id` (unless the caller stored the temporary counter, as `_create-energy` does
  `reference/engine/src/processor/intents/_create-energy.js:50`), so later `bulk.update`s of it go to key
  `'undefined'` and are silently dropped at commit (the in-memory object still changes for the rest of the tick).
* **GOTCHA (counter reuse)**: `remove(tempId)` of a pending insert deletes it **and decrements** the counter
  `reference/driver/lib/bulk.js:76-87`; if it was not the newest pending insert, the next `insert` reuses the counter
  of a still-pending doc and overwrites it (that object is lost). Reachable e.g. when a 1-unit drop created in the
  intent phase (`_create-energy` registers it in `roomObjects` under its counter id) decays to 0 in the same tick's
  object phase and is removed `reference/engine/src/processor/intents/energy/tick.js:14-19` after other inserts
  (spawned creep, construction site…) were queued; the next insert (e.g. a tombstone) then overwrites the newest one.
  See Open questions.

### 2.11 STORE-HIDDEN-FIELDS
`removeHidden` deletes every property whose name starts with `_` (recursively, including inside arrays) and
`$loki` before insert/update `reference/driver/lib/bulk.js:6-22`. Hence all `_xxx` fields
(`_actionLog`, `_ticksToLive`, `_oldHits`, `_damageToApply`, `_cooldown`, `_upgradeBlocked`, `_skip`, `_off`,
`_justSpawned`, `_parsed`, `_modified`, …) are **per-tick transient** and never stored. Note `_id` also starts with
`_` and is stripped from update payloads; inserts re-add it only when an explicit id is passed `:66-68`.
Rule for chitin: never persist transient processor state; persisted names never start with `_` (except `_id`).

### 2.12 STORE-PERSISTENCE
LokiJS `autosave` every 10 000 ms writes the whole DB as one JSON file (`db.json`) `reference/storage/lib/db.js:15`.
A crash loses up to 10 s. On load `upgradeDb` runs (STORE-MIGRATIONS). **PERF**: whole-world serialization every
10 s; O(world). chitin should use incremental/append-only persistence.

### 2.13 STORE-QUEUES
In-memory queues `usersLegacy`, `usersIvm`, `rooms` `reference/storage/lib/queue.js:6-22`; the driver maps the
name `'users'` to `'usersIvm'` `reference/driver/lib/queue.js:21-23`.
* `fetch` takes `pending.pop()` — **LIFO** — and moves the item to `processing` `reference/storage/lib/queue.js:25-42`.
* `addMulti` concatenates `reference/storage/lib/queue.js:65-75`; driver `addMulti` returns immediately for an
  empty array but **throws on `undefined`** (`array.length`) `reference/driver/lib/queue.js:44-49`.
* `whenAllDone` resolves when pending and processing are both empty and publishes `queueDone:<name>` `:76-92`.
Processing order of rooms across processors is therefore not deterministic; chitin must not make results depend on
it (rooms are processed independently; cross-room effects go through global processing).

### 2.14 STORE-PUBSUB
`publish(channel, data)` emits to listeners of `channel` and of `'*'` `reference/storage/lib/pubsub.js:9-13`.
Keys `queueDone:`, `runtimeRestart`, `tickStarted`, `roomsDone` `reference/common/lib/storage.js:53-60`; other
channels in use: `setTickRate`, `serverMessage`, `user:<id>/code`, `user:<id>/console`, `user:<id>/cpu`,
`user:<id>/set-active-branch`, `user:<id>/message:<id2>`, `user:<id>/newMessage`
(`reference/backend/lib/cli/system.js:69`, `reference/backend/lib/game/api/user.js:111-119`,
`reference/driver/lib/index.js:368,409`, `reference/driver/lib/runtime/make.js:262-273`).

### 2.15 STORE-RESET
`resetAllData` reloads `storage/db.original.json` wholesale `reference/storage/lib/db.js:766-775`
(not the launcher's `init_dist/db.json`; they differ, see GEN-DEFAULT-WORLD).

### 2.16 STORE-MIGRATIONS
`upgradeDb` keyed on `env` doc 1 `databaseVersion` `reference/storage/lib/db.js:32-254` (only needed to import
legacy DBs; the shipped DBs are at 4.1 and 7):
* v4 `:46-66`: rooms matching `^[WE][1-9][NS][1-9]$` get `bus:false`; rooms matching `^[WE]\d*0` or `0$` get
  `depositType` (SECTOR-DEPOSIT-TYPE). **GOTCHA**: `^[WE]\d*0` is not anchored to the end of the number, so
  `W105N3` matches; and `bus:true` is *not* set here.
* v5 `:68-169`: legacy `energy/energyCapacity/<resource>` fields → `store`/`storeCapacity`/`storeCapacityResource`
  for spawn, extension, tower, link (energy-only), storage/terminal/container/factory/creep/powerCreep/tombstone
  (store-only), nuker (`energy`,`G`), powerSpawn (`energy`,`power`), lab (energy + mineral; if no mineral then
  `storeCapacity = energyCapacity + mineralCapacity`); market orders get `createdTimestamp = now`.
* v6 `:171-184`: powerBank `power` → `store:{power}`.
* v7 `:186-204`: Invader user (`_id:'2'`) gets a fixed SVG-path badge.
* v8 `:206-223`: spawn/invaderCore `spawning.remainingTime` → `spawning.spawnTime = gameTime + remainingTime`.
* v9 `:225-251`: `rooms.active` booleans move into env set `activeRooms`; `active` deleted from room docs.

### 2.17 STORE-RPC-SERIALIZATION
All values cross processes as JSON: `undefined` properties vanish, `Date` objects become ISO strings
(`registeredDate`, `users.money.date`, `users.messages.date`), `Infinity` becomes `null`
(used deliberately by STATUS-DATA). Doc field types in this spec are the *post-JSON* types.
JSON transport preserves array order and object key order, so every order rule below survives the RPC hop.

### Iteration order (2.18–2.29)

The base order of `rooms.objects` and the other collections decides every tie-break in the processor
(`_.find`/`_.forEach` over `roomObjects`) and in the player API (`room.find`, `look*`, `findClosestBy*`,
`Game.*` key order). It is fully determined by LokiJS 1.5.9 and the code below. **These are the canonical 1:1
rules for chitin; specs 01, 03, 04, 05, 06 and 07 defer to them.** Superseded statements elsewhere: spec 01
§20 Q1 ("ties keep their order through ... adaptive index updates" — wrong: adaptive updates move the written
doc to the *front* of its key range, and `rooms.objects` is not adaptive anyway); spec 03 CORE-OBJECT-ORDER
("DB return order" — now pinned by ORDER-RUNTIME-OBJECTS) and spec 03's `Game.rooms` line (the result of
`rooms.find({_id:{$in}})` is `rooms`-collection order, **not** `roomIds` order: ORDER-GAME-ROOMS). The open
questions on base order in 01, 03, 04, 05, 06, 07 and 11, and on `ticksToLive` at birth in 04 and 06
(OBJ-CREEP-BIRTH), are answered here.

### 2.18 STORE-INDEX-MODE
A Loki collection maintains its binary indices either **lazily** (`adaptiveBinaryIndices:false`) or
**adaptively** (`true`).
* On load, the flag is copied from the saved file; missing ⇒ `false` `lokijs.js:1741`.
* A collection created at runtime by `getOrAddCollection` → `db.addCollection(name)` (no options)
  `reference/storage/lib/db.js:330-334` gets Loki's default **`true`** `lokijs.js:4978`.
* Shipped databases store `adaptiveBinaryIndices:false` for every collection they contain (`env`, `users`,
  `rooms`, `rooms.objects`, `rooms.flags`, `rooms.terrain`, `rooms.intents`, `users.code`, `users.notifications`,
  `transactions`, `market.orders`, `market.intents`) — `reference/launcher/init_dist/db.json`,
  `reference/storage/db.original.json`. The only exception: `users.power_creeps` in `db.original.json` is `true`.
* Resulting mode on a server started from either shipped DB (the flag is re-saved with the DB, so it is permanent):

| collection | indexed on | mode |
|---|---|---|
| `rooms.objects` | `room`, `user` | lazy |
| `rooms.flags` | `room`, `user` | lazy |
| `rooms.terrain`, `rooms.intents` | `room` | lazy |
| `users` | `username` | lazy |
| `users.code`, `users.notifications`, `transactions` | `user` | lazy |
| `users.console` | `user` | **adaptive** (absent from both shipped DBs ⇒ created at runtime) |
| `users.power_creeps` | `user` | **adaptive** (runtime-created in launcher installs; stored `true` in `db.original`) |
| `users.money`, `users.resources` | `user` | **adaptive** (runtime-created; backend-only readers) |
| `rooms`, `market.orders`, `market.stats`, `users.messages`, `users.intents`, `leaderboard.*` | — | no index |

### 2.19 STORE-ORDER-INDEXED (lazy indices)
* `data[]` order: insert appends (`this.data.push(obj)` `lokijs.js:6142`), update replaces in place
  (`this.data[position] = newInternal` `:6031`), remove splices (`:6407`, batch: `filter` `:6288-6290`). So
  `data[]` is **insertion (creation) order**, never changed by updates, and saved/loaded as-is (persists across
  restarts).
* Every insert/update/remove on a lazy collection only calls `flagBinaryIndexesDirty` `lokijs.js:5653-5660`
  (call sites `:6052,6161,6262,6404`).
* A `find` whose **first** filter is on an indexed field with an indexed operator (`$eq $aeq $dteq $gt $gte $lt
  $lte $in $between` `lokijs.js:675-685`) calls `ensureIndex(property)` `lokijs.js:3519-3532` (storage also calls
  it on every request `reference/storage/lib/db.js:335-386`); a dirty index is
  rebuilt as `prepareFullDocIndex()` = `[0..n-1]` `:5372-5379` sorted by the field `:5405-5452`. The sort is
  stable (Node ≥ 22.9), so rows with equal keys stay in `data[]` order.
* **Rule:** `find({room: R})` on `rooms.objects` returns the room's objects in **creation order** (order of
  insertion into the collection). Same for `find({user: U})` (all of U's objects, across all rooms, in global
  creation order), `rooms.flags`, `users.code`, `transactions` by `user`.
* **GOTCHA**: "creation order" is the order of the *document's* insertion, not of arrival in the room. A creep or
  power creep crossing into a room is an in-place update (`bulkObjects.update(creep, {room, x, y, interRoom:null})`
  `reference/engine/src/processor/global.js:42`), so it keeps its original position: an old creep entering a room
  sorts before structures built there after it was spawned.
* **GOTCHA**: within one room-processing commit, inserts reach storage in bulk-insert order (after that bulk's
  removes and updates) `reference/driver/lib/bulk.js:120-129`, `reference/storage/lib/db.js:480-513`. Commits of
  different rooms in the same tick interleave in nondeterministic processor order (STORE-QUEUES), and backend/cron
  inserts arrive at wall-clock times. The original therefore has no fixed order between objects created in the
  same tick in *different* rooms (visible only in cross-room lists such as the runtime's own-object list); any
  deterministic choice is 1:1-compatible.

### 2.20 STORE-ORDER-ADAPTIVE (adaptive indices)
* Insert: `adaptiveBinaryIndexInsert` splices the new data position at `calculateRangeStart(..., adaptive=true)`,
  which is the **lower bound** of the equal-key range `lokijs.js:6153-6162,6523-6539,6658-6690`.
* Update: `adaptiveBinaryIndexUpdate` removes the position by linear scan and re-inserts it at the lower bound
  `lokijs.js:6044-6050,6546-6563` — even when the key did not change.
* Single remove: splice out, shift higher positions down; relative order of the rest kept `lokijs.js:6396-6401,6569-6642`.
* **Batch remove** (`removeWhere`, `Resultset.remove`, `remove(array)` → `removeBatchByPositions`): when the
  collection is adaptive and has any binary index, the indices are flagged dirty and, after the removal,
  **force-rebuilt** with adaptive temporarily off (`ensureAllIndexes(true)`) `lokijs.js:6205-6213,3865-3877,
  6223-6320` (`adaptiveOverride` `:6229`, rebuild `:6304-6308`). The rebuild is the stable sort of `[0..n-1]`, i.e.
  equal keys revert to `data[]` (insertion) order. This happens **even when zero rows match** (the call still goes
  through `removeBatchByPositions([])`).
* Array insert via the storage RPC passes no `overrideAdaptiveIndices`, so each element is inserted adaptively
  `lokijs.js:5801-5833`. Storage never sends array updates (`dbUpdate`/`dbBulk` update one doc at a time
  `reference/storage/lib/db.js:436-478,480-513`).
* **Rule:** within one key, an adaptive index lists = [docs inserted or updated since the last batch remove on
  that collection, **most recently written first**] followed by [the remaining docs in insertion order].

### 2.21 STORE-ORDER-IN
* `$in` on an indexed field: `calculateRange('$in')` runs a `$eq` range per element of the `$in` array, in array
  order, de-duplicating positions, and returns the concatenation `lokijs.js:6853-6867`; rows are emitted in that
  order `:3639-3648`. **Rows are grouped by the order of the `$in` array**, each group in index order.
* `$in` on an unindexed field (including `_id`): full scan, `data[]` order (STORE-ORDER-SCAN); the `$in` array order
  is irrelevant.

### 2.22 STORE-ORDER-SCAN
* A query whose first filter cannot use a binary index scans `data[]` in order `lokijs.js:3586-3612`.
* Multi-key queries become `$and` of single-key filters in object-key order; `findAnd` applies them in sequence,
  each later filter preserving the current row order `lokijs.js:3451-3466,3405-3415`. Only the first filter can
  use an index (`doIndexCheck = !filterInitialized` `:3520-3522`).
* `findEx` sort (`simplesort(field, desc)` `reference/storage/lib/db.js:519-524`) on an unindexed field sorts
  with `sortHelper`, which implements `desc` inside the comparator (no reverse) `lokijs.js:3248-3275,357-371`;
  stable ⇒ **ties keep the prior (data) order** in both directions. (Sorting on an *indexed* field takes an
  index-intersection path and reverses for `desc` `:3209-3245`; no server `findEx` sorts on an indexed field.)

### 2.23 STORE-ORDER-RELOAD
* Saved binary indices (values and dirty flag) are restored verbatim on load `lokijs.js:1784-1786`; a clean
  saved index is used as-is until the next write dirties it.
* `init_dist/db.json` saves `rooms.objects` indices **dirty** ⇒ rebuilt (creation order) on first use.
* **GOTCHA**: `db.original.json` (used by `resetAllData` → `db.loadJSON` `reference/storage/lib/db.js:766-775`)
  saves `rooms.objects` `room`/`user` indices **clean but unstably sorted** (140 equal-key inversions in `room`,
  6 in `user`, from an older V8). Until the first write to `rooms.objects` after a reset, `find({room})` returns
  that scrambled order. Not bot-visible: no user code runs before a spawn is placed, and placing it is a write.
  chitin: ignore (always creation order).

### 2.24 ORDER-ROOM-OBJECTS (canonical)
* Processor: `getRoomObjects` = `rooms.objects.find({room})` → `mapById` (`_.reduce` into an object keyed by
  `_id`) `reference/driver/lib/index.js:249-271,331-337`. The processor iterates that object (`_.forEach`
  `reference/engine/src/processor.js:43,342`), so **per-room processing order = creation order**, with objects
  created during the tick appended under temporary keys (STORE-ID-GEN).
* The same order is seen by the backend room websocket and every other `find({room})` reader.
* JS key order caveat: integer-like keys (canonical array indices < 2³²−1) enumerate first, ascending. A
  canonical array index has at most 10 digits, so 15-char genId ids and 24-char ObjectIds never qualify; the user ids
  `'2'`/`'3'` do (relevant only to maps keyed by user id, e.g. `getRoomObjects().users`), as would custom mod
  `_id`s. chitin's JS engine must implement standard property order; chitin's store should keep ids that are
  never integer-like.
* chitin rule: each object gets a monotonically increasing creation sequence when it is inserted; a room's objects
  iterate by ascending sequence; moves between rooms keep the sequence; removal keeps the others' order.
  **PERF**: room-local; an insertion-ordered vector/slab per room plus a per-user ordered set replaces Loki's
  whole-world re-sort.

### 2.25 ORDER-RUNTIME-OBJECTS / ORDER-RUNTIME-ROOMIDS
`reference/driver/lib/runtime/data.js:99-220`:
1. Own objects: `rooms.objects.find({user})` (`:103`) — all of the user's objects in global creation order
   (lazy `user` index).
2. `roomIds` (`:113-127`): iterate own objects in that order, skipping `flag` and `constructionSite`; for each
   set `roomIdsHash[obj.room] = true`, and for an `observer` immediately `roomIdsHash[obj.observeRoom] = true`.
   `roomIds = Object.keys(roomIdsHash)` = **first-appearance order** of the sequence
   (room₁, [observeRoom₁], room₂, …). An observer's target therefore lands right after the observer's room when
   both first appear at that observer; a key already present keeps its earlier position.
   **GOTCHA**: rooms where the user has only construction sites (or legacy `flag` objects) are not in `roomIds`:
   no foreign objects and no `Game.rooms` entry for them. **GOTCHA**: an idle observer has `observeRoom:null`
   (reset each processing `reference/engine/src/processor.js:116-118,403-405`), a never-processed one has it
   absent, so `roomIds` can contain the strings `"null"`/`"undefined"`; they match nothing (also passed to the
   event-log `hmget` `:141`).
3. Foreign objects: `rooms.objects.find({$and:[{room:{$in: roomIds}}, {user:{$ne: userId}}]})` (`:136`) —
   indexed `$in` ⇒ **grouped by `roomIds` order**, each room in creation order; `$ne` then filters (objects with no
   `user` are kept).
4. `runtimeData.roomObjects = _.extend(mapById(foreign), userObjects)` (`:209`): **all foreign objects (grouped
   by `roomIds`), then all own objects (global creation order)**. This is the base order of every `room.find`,
   `look*`, `Game.creeps/structures/...` built from it (spec 03 CORE-OBJECT-ORDER).

### 2.26 ORDER-GAME-ROOMS
`rooms` has no binary index (2.18), so `db.rooms.find({_id:{$in: roomIds}})` (`data.js:135`) is a full scan:
`runtimeData.rooms = mapById(result)` (`:208`) is in **`rooms` collection insertion order** (world-generation
order), filtered to `roomIds` — *not* `roomIds` order. `Game.rooms` is built by `for (i in runtimeData.rooms)`
and `_.clone` `reference/engine/src/game/game.js:196-199,218-220,227`. Default world order (both shipped DBs):
`W10N10, W10N9, …, W10N0, W9N10, …, W0N0`; rooms added by `map.generateRoom` are appended, `map.removeRoom`
splices. chitin: keep a room creation sequence.

### 2.27 ORDER-CONSOLE-COMMANDS
`users.console` is adaptive (2.18). The runtime reads `find({user})` (`data.js:133`), evaluates
`consoleCommands` in array order `reference/engine/src/game/game.js:535-546`, and afterwards calls
`users.console.removeWhere({_id:{$in: ids}})` (`data.js:149`) — for **every user's fetch, every tick, even with
an empty id list**, which force-rebuilds the whole `user` index into insertion order (2.20).
* Backend `/console` and `/memory` each insert one doc `reference/backend/lib/game/api/user.js:285-351`, spliced
  at the front of the user's range.
* **Rule:** commands queued since the last rebuild run **newest-first**; commands that survived a rebuild
  (e.g. inserted between this user's `find` and `removeWhere`, or present when another user's fetch rebuilt
  the index) follow, in insertion order. With one active user the last rebuild is that user's own previous
  fetch, so all commands queued for a tick run exactly newest-first. With several users, another user's fetch
  landing between the inserts and this user's `find` reverts the earlier inserts to insertion order, so the
  original is timing-dependent there. **chitin canonical: newest-first** (the single-user result).
* **GOTCHA**: `/memory` writes are hidden console expressions, so two writes to the same Memory path queued for the
  same tick apply newest-first — **the older write wins**.

### 2.28 ORDER-POWER-CREEPS
`users.power_creeps` is adaptive (2.18). The runtime's `find({user})` (`data.js:142`) → `_.indexBy(..., '_id')`
(`:220`) → `Game.powerCreeps[name]` in that order `reference/engine/src/game/game.js:255-258`.
* Every write to a power-creep doc moves it to the front of the user's range: create (insert), spawn, death,
  upgrade, rename, delete-mark/cancel-delete (updates) `reference/engine/src/processor/global-intents/power/*.js`
  (`createPowerCreep.js:27`, `spawnPowerCreep.js:24`, `_diePowerCreep.js:55`, `upgradePowerCreep.js:60`,
  `renamePowerCreep.js:22`, `deletePowerCreep.js:16,27`), backend create/delete-mark/cancel-delete
  `reference/backend/lib/game/api/power-creeps.js:52,84,101`. Single removes (`deletePowerCreep.js:21`) keep the
  others' order.
* The delete cron's `removeWhere({_id})` `reference/backend/lib/cronjobs.js:550` force-rebuilds the index:
  all users' power creeps revert to insertion order.
* **Rule:** `Game.powerCreeps` key order = power creeps written since the last delete-cron removal, most recently
  written first, then the rest in creation order. Key order changes e.g. on the tick after a power creep
  spawns or dies. (Integer-like names enumerate first, per JS rules.)
* Global processing reads `users.power_creeps.find()` (no filter ⇒ `data[]` order) `reference/driver/lib/index.js:438`.

### 2.29 ORDER-MISC
* `users.find({_id:{$in}})` (runtime `data.js:224`, processor `index.js:261`) and `getAllUsers` are full scans ⇒
  `users` insertion order; `getAllUsers` then sorts by `lastUsedDirtyTime` desc (stable; ties keep insertion
  order) `reference/driver/lib/index.js:144-151`.
* `rooms.flags.find({user})` (`data.js:140`) and `find({room})`: creation order of the per-(user,room) docs.
* Transactions (`findEx({sender|recipient},{sort:{time:-1},limit:100})` `data.js:138-139`): unindexed field ⇒
  scan, stable desc sort ⇒ newest tick first, same-tick rows in insertion order.
* `market.orders`, `rooms.intents`, `users.intents`: `data[]` order.

---

## 3. env (key/value) store

### 3.1 ENV-LAYOUT
`env` is a Loki collection with two docs: `$loki 1` = `{data:{key: value}}` (all values) and `$loki 2` =
`{data:{key: expiryMs}}` `reference/storage/lib/db.js:539-764`. Keys are listed in
`reference/common/lib/storage.js:33-52`. **PERF**: every key (including all users' Memory strings, every room's
`mapView`, the whole compressed terrain blob, room history) lives in **one** document; chitin should shard this.

### 3.2 ENV-OPS
`reference/storage/lib/db.js:539-764`:
* `get(k)`, `mget([k])` → values (undefined if missing). `set(k,v)` creates doc 1 if needed.
* `setex(k,s,v)` = `set` + `expire`. `expire(k,s)` stores `Date.now()+s*1000` in doc 2. `ttl(k)` → seconds
  remaining (float) or `-1` if none/expired.
* `del(k)` → 1/0. **GOTCHA**: does not call `env.update`; persisted on the next write.
* `hget(k,f)`, `hset(k,f,v)`, `hmget(k,[f])`, `hmset(k,obj)` (`_.extend`): hashes are plain objects stored
  under the key. `hmget` on a missing key creates an empty hash in memory.
* `sadd(k, v|[v])`: array-backed set, de-duplicated with `_.includes`, insertion ordered.
  `smembers(k)` → array, or **`undefined`** when the key is missing.

### 3.3 ENV-EXPIRY
Every 10 s, keys in doc 2 with `Date.now() > expiry` are deleted from both docs `reference/storage/lib/db.js:295-315`.
Expiry is wall-clock based (not tick based).

### 3.4 ENV-GAMETIME
Key `gameTime`. Stored as a JSON number (default DB: `1`), read with `parseInt` `reference/common/index.js:65-67`;
`incrementGameTime` sets `gameTime+1` once per tick after global processing `reference/driver/lib/index.js:416-419`,
`reference/engine/src/main.js:72-76`.

### 3.5 ENV-ACCESSIBLE-ROOMS
Key `accessibleRooms`: a **JSON string** (double encoded) of the array of room names with `status=='normal'` and
(`!openTime || openTime < Date.now()`) `reference/driver/lib/index.js:490-496`. Recomputed by the main loop when
`gameTime > lastAccessibleRoomsUpdate + 20` (`lastAccessibleRoomsUpdate` starts at 0 per process start)
`reference/engine/src/main.js:79-83`. Runtime caches it 60 s wall-clock `reference/driver/lib/runtime/data.js:76-86`
and `JSON.parse`s it per player per tick `reference/engine/src/game/map.js:18`.

### 3.6 ENV-ROOM-STATUS-DATA
Key `roomStatusData`: JSON string `{novice:{room:ms}, respawn:{room:ms}, closed:{room:ms|null}}`, recomputed together
with ENV-ACCESSIBLE-ROOMS (STATUS-DATA) `reference/driver/lib/index.js:498-524`; runtime cache 60 s
`reference/driver/lib/runtime/data.js:88-97`.
**GOTCHA**: the shipped DBs do not contain this key. On a fresh DB (gameTime starts at 1) it is first written at
tick 21; until then `JSON.parse(undefined)` throws inside the runtime data fetch
`reference/driver/lib/runtime/data.js:91-93`. chitin should write it at startup (deviation with no bot-visible
downside other than avoiding broken early ticks).

### 3.7 ENV-TERRAIN-DATA
Key `terrainData` = base64( zlib-deflate( JSON.stringify(arrayOf terrain docs) ) ) where each element has at least
`{room, terrain}` (real docs also carry `_id,type,meta,$loki`) `reference/backend/lib/cli/map.js:661-695`.
Consumers inflate it once per process: runner (`staticTerrainData`) `reference/driver/lib/runtime/make.js:20-51`,
processor pathfinder `reference/driver/lib/index.js:113-116`, stronghold expansion
`reference/backend/lib/strongholds.js:163-168`. See TERRAIN-WORLD-BLOB for the walled ring.
**GOTCHA**: consumers cache it for the process lifetime; rooms generated later are invisible to runtimes and the
pathfinder until restart. chitin should update incrementally.

### 3.8 ENV-ACTIVE-ROOMS
Key `activeRooms` (a set). `activateRoom(room|[rooms])` = `sadd` `reference/driver/lib/index.js:176-178`,
`reference/backend/lib/utils.js:311-317`. Each tick the main loop reads it and deletes it
(`getAllRoomsNames`) `reference/driver/lib/index.js:170-174`, then queues exactly those rooms for processing
`reference/engine/src/main.js:42-56`. Rooms are activated by: user intents in the room
`reference/driver/lib/index.js:234-240`, the processor's `activateRoom` rules (owned objects, energy drops,
tombstones, nukes, portals, invader activity, power banks/deposits within 500 ticks of decay, `roomInfo.active`)
`reference/engine/src/processor.js:107-140,499-506`, inter-room movement `reference/engine/src/processor/global.js:33-43`,
cron jobs (§13) and backend actions (place-spawn, construction, flags).
**PERF**: this is the original's main scalability lever — only active rooms are processed each tick; idle rooms are
"force-updated" every 90–109 ticks by CRON-ROOMS-FORCE-UPDATE. chitin needs the same set (bots can observe e.g.
source regeneration only being applied when a room is processed — see Open questions).
**GOTCHA**: if no room was activated during a tick, `smembers` returns `undefined`, `addMulti(undefined)` throws and
that main-loop iteration aborts before `incrementGameTime` (game time stalls until something activates a room)
`reference/driver/lib/queue.js:44-46`, `reference/engine/src/main.js:94-99`. Do not replicate.

### 3.9 ENV-MEMORY
Key `memory:<userId>` = raw Memory JSON string (default `"{}"` for bots `reference/backend/lib/cli/bots.js:94`).
Saved after each run; rejected if `> 2*1024*1024` chars `reference/driver/lib/index.js:153-160`. Backend
`/memory` writes go through `users.console` hidden expressions instead (COL-USERS-CONSOLE).

### 3.10 ENV-MEMORY-SEGMENTS
Key `memorySegments:<userId>` = hash `{segmentId: string}`; written with `hmset` of the segments changed this tick
`reference/driver/lib/index.js:162-168`; read with `hmget(activeSegments)` `reference/driver/lib/runtime/data.js:228-231`.
Backend limits: id 0..99, 100 KiB per segment `reference/backend/lib/game/api/user.js:318-339`.

### 3.11 ENV-PUBLIC-MEMORY-SEGMENTS
Key `publicMemorySegments:<userId>` = comma-separated id string as returned by the runtime
`reference/driver/lib/runtime/make.js:249-251`; read for foreign segment access
`reference/driver/lib/runtime/data.js:233-237,255`.

### 3.12 ENV-MAP-VIEW
Key `mapView:<room>` = JSON string written every time the room is processed
`reference/driver/lib/index.js:530-532`: `{w:[], r:[], pb:[], p:[], s:[], c:[], m:[], k:[], <userId>:[]...}`,
each entry `[x,y]` `reference/engine/src/processor.js:328-481`:
* owned objects (`object.user` truthy) → `mapView[userId]`, except `constructionSite`, `newbieWall`, `ruin`,
  `tombstone`, public ramparts;
* otherwise by type: `constructedWall`→`w`, `road`→`r`, `powerBank`→`pb`, `portal`→`p`, `source`→`s`,
  `mineral`→`m`, **`deposit`→`m`**, `controller`→`c`, `keeperLair`→`k`, dropped `power` (`type:'energy'`,
  `resourceType:'power'`)→`pb`. Flags never appear.
The default DB ships a `mapView:` for each of its 121 rooms.

### 3.13 ENV-ROOM-HISTORY
Key `roomHistory:<room>` = hash `{tick: JSON string of all non-flag objects}`; on ticks with
`gameTime % historyChunkSize(20) == 0` the chunk starting at `floor((gameTime-1)/20)*20` is diffed (WIRE-ROOM-DIFF
algorithm), emitted via the `saveRoomHistory` engine event and the key deleted
`reference/driver/lib/history.js:7-43`, `reference/engine/src/processor.js:522-536`, `reference/driver/lib/index.js:29`.
Only saved for rooms that were active this tick.

### 3.14 ENV-ROOM-VISUAL
Key `roomVisual:<userId>,<roomName>,<tick>` (`roomName` empty string for map visuals) set with TTL
`mainLoopResetInterval/1000` = 5 s `reference/driver/lib/runtime/make.js:253-260`; read by the websocket for
`gameTime-1` `reference/backend/lib/game/socket/rooms.js:80-92`.

### 3.15 ENV-ROOM-EVENT-LOG
**One** hash under the literal key `roomEventLog:` with field = roomId, value = JSON event array of the last
processed tick `reference/driver/lib/index.js:583-585`; read with `hmget(ROOM_EVENT_LOG, roomIds)`
`reference/driver/lib/runtime/data.js:141`. **GOTCHA**: rooms not processed this tick keep their *old* event log
(no clearing), so `Room.getEventLog()` in an inactive room returns stale events.

### 3.16 ENV-USER-ONLINE
`userOnline:<userId>` = `Date.now()` on every authenticated request `reference/backend/lib/authlib.js:37`.

### 3.17 ENV-MAIN-LOOP-PAUSED
`mainLoopPaused` = `'0'|'1'` string; `notifyTickStarted` rejects with `'Simulation paused'` when `+value` truthy
`reference/driver/lib/index.js:339-347`, CLI `reference/backend/lib/cli/system.js:29-41`.

### 3.18 ENV-TICK-RATE
`tickRate` = minimal tick duration in ms (`config.engine.mainLoopMinDuration`, default 1000)
`reference/driver/lib/index.js:19-21,118-132`, CLI `reference/backend/lib/cli/system.js:54-72`.

### 3.19 ENV-AUTH-TOKENS
`auth_<token>` → userId with 60 s TTL, extended on use `reference/backend/lib/authlib.js:7-36`.

### 3.20 ENV-LEGACY-KEYS
`scriptCachedData:` (declared, unused) `reference/common/lib/storage.js:41`; steam registration writes
`scrUserMemory:<id>` instead of `memory:<id>` `reference/backend/lib/game/api/auth.js:66`; code upload deletes
`scrScriptCachedData:<id>` `reference/backend/lib/game/api/user.js:113`. None is read by the engine.

---

## 4. Collections

All docs also have `_id` (string) unless noted.

### 4.1 COL-ROOMS
| field | type | meaning / writer |
|---|---|---|
| `_id` | string | room name (`W5N5`) — the primary key |
| `status` | `'normal'` \| `'out of borders'` | STATUS-FIELDS |
| `openTime` | ms number | room becomes accessible after this time (`map.openRoom`) |
| `novice` | ms number | novice area until this time (read-only in OSS code; no writer) |
| `respawnArea` | ms number | respawn area until this time (no writer) |
| `sourceKeepers` | boolean | set by `generateRoom` if any keeper lair was emitted; informational |
| `bus` | boolean | highway flag; **only** source of truth for power banks/deposits (§10.2) |
| `depositType` | resource string | deposit kind for highway rooms (§10.4) |
| `powerBankTime` | tick | next power bank spawn tick (CRON-POWER-BANKS) |
| `nextForceUpdateTime` | tick | CRON-ROOMS-FORCE-UPDATE |
| `invaderGoal` | number | harvest threshold for invader raids (CRON-INVADERS); `1000000` after a player spawns |
| `nextNpcMarketOrder` | tick | CRON-NPC-ORDERS |
| `lastPvpTime` | tick | set when damage between players happens `reference/engine/src/processor/intents/_damage.js:82` |
| `active` | boolean | legacy (pre-v9), deleted by migration |

The processor passes the room doc as `roomInfo`; if any field changed (`roomInfo.active` is consumed and deleted
first) the *whole* doc is `$set` back `reference/engine/src/processor.js:29,499-511`, `reference/driver/lib/index.js:425-427`.

### 4.2 COL-ROOMS-TERRAIN
`{_id, room, terrain: string(2500), type:'terrain'}` (default DB). **GOTCHA**: `generateRoom` inserts
`{room, terrain}` *without* `type` `reference/backend/lib/cli/map.js:558`; consumers that filter on
`type=='terrain'` (`utils.decodeTerrain`, `decodeTerrainByRoom` `reference/engine/src/utils.js:280-331`; backend
`/room-terrain` decoding `reference/backend/lib/game/api/game.js:263-278`) ignore/return raw generated rooms.
chitin: always store `type:'terrain'`.

### 4.3 COL-ROOMS-OBJECTS
One doc per room object; schemaless; indexed by `room`, `user` (lazy indices). Shapes in §6.
Result order of `find({room})` / `find({user})` = creation order (STORE-ORDER-INDEXED, ORDER-ROOM-OBJECTS).
**PERF**: room-local except: runner reads all objects of a user + all foreign objects in that user's rooms
`reference/driver/lib/runtime/data.js:102-142`; global processing reads all creeps/powerCreeps with `interRoom`,
all terminals, power spawns and power creeps `reference/driver/lib/index.js:429-449`.

### 4.4 COL-ROOMS-FLAGS
One doc per (user, room): `{_id, user, room, data}` where `data` is
`name~color~secondaryColor~x~y` joined by `|` `reference/engine/src/processor/intents/room/intents.js:42-53`,
`reference/backend/lib/game/api/game.js:139-184`.
* name escaping: `|`→`$VLINE$`, `~`→`$TILDE$` on write, reversed on read.
* After `split('~')` all fields are strings; created flags are written from numbers so stored text is e.g.
  `Flag1~1~1~25~25`. Runtime converts `Number(x)`, `Number(color)`, `Number(secondaryColor || color)`
  `reference/engine/src/game/flags.js:20-26`, skipping empty entries `reference/engine/src/game/game.js:399-403`.
* **GOTCHA**: removing the last flag leaves `data:""`; the processor then parses `""` into `[[""]]` and a later
  create yields `"|Name~..."`, i.e. a leading empty entry. Readers must skip empty entries.
* **GOTCHA**: processor `createFlag` checks name uniqueness only among *this room's* flag docs of the user
  `reference/engine/src/processor/intents/room/create-flag.js:10-14`; global uniqueness is enforced by the runtime API.
* Limit `FLAGS_LIMIT` (10000) is enforced by the backend HTTP path `reference/backend/lib/game/api/game.js:442-446`.

### 4.5 COL-ROOMS-INTENTS
`{_id, room, users:{<userId>:{objects:{<objectId>|'room':{<intentName>: intent | intent[]}}, objectsManual:{…}}}}`
written with `$merge` + upsert `reference/driver/lib/index.js:234-236`, `reference/backend/lib/game/api/game.js:522-524`;
removed after the room is processed `reference/driver/lib/index.js:322-324`. `objectsManual` (from the web client)
overrides `objects` per object `reference/engine/src/processor.js:231-234`. Intent payloads are sanitized
(CONST-SYSTEM-INTENT-SCHEMA).

### 4.6 COL-USERS
| field | type | meaning |
|---|---|---|
| `_id` | string | `'2'` Invader, `'3'` Source Keeper, else genId |
| `username`, `usernameLower` | string | |
| `email`, `emailDirty`, `password` | | backend auth only |
| `cpu` | number | CPU limit; `0`/falsy ⇒ never queued (USER-ACTIVE-FLAG) |
| `cpuAvailable` | number | bucket; may be negative |
| `lastUsedCpu`, `lastUsedDirtyTime` | number | runner stats `reference/driver/lib/runtime/make.js:196-199` |
| `skipTicksPenalty` | number | runner hard-reset penalty `reference/driver/lib/runtime/make.js:61-71` |
| `gcl` | number | accumulated control points (not level) |
| `power` | number | accumulated processed power |
| `money` | number | credits × 1000 (integer "millis") |
| `resources` | object | intershard resources `{token, cpuUnlock, pixel, accessKey}` |
| `rooms` | string[] | owned room names (USER-ROOMS-LIST) |
| `active` | number/boolean | USER-ACTIVE-FLAG |
| `bot` | string | bot AI name for CLI bots |
| `badge` | object | USER-BADGE; `customBadge` for custom path badges |
| `registeredDate` | ISO string | |
| `steam` | `{id, displayName?, ownership?, steamProfileLinkHidden?}` | |
| `credits` | number | set 0 at steam registration, unused |
| `powerExperimentations`, `powerExperimentationTime` | number, ms | |
| `lastRespawnDate`, `blocked`, `lastChargeTime` | | backend only |
| `notifyPrefs` | `{disabled, disabledOnMessages, sendOnline, interval, errorsInterval}` | |
| `lastNotifyDate` | ms | |
| `activeSegments`, `defaultPublicSegment`, `activeForeignSegment` | | USER-SEGMENT-FIELDS |
| `shardAccess` | boolean | `false` blocks `claimController` `reference/engine/src/processor/intents/creeps/claimController.js:37-39` |
| `"resources.<type>"` | number | artefact of STORE-UPDATE-OPERATORS `$inc` GOTCHA |

### 4.7 COL-USERS-CODE
`{_id, user, branch, modules:{<name>:code}, timestamp, activeWorld:bool, activeSim:bool}`.
Module names are stored with `.`→`$DOT$`, `/`→`$SLASH$`, `\`→`$BACKSLASH$`; names starting with `$` after
translation are dropped; `main` is forced to exist `reference/backend/lib/utils.js:47-86`; runtime reverses the
mapping `reference/driver/lib/runtime/data.js:168-176`. The active world branch is `{user, activeWorld:true}`.

### 4.8 COL-USERS-CONSOLE
`{_id, user, expression, hidden?:true}`; consumed (deleted) by the next runtime data fetch
`reference/driver/lib/runtime/data.js:133,149`, inserted by `/console` and `/memory`
`reference/backend/lib/game/api/user.js:285-351`. Adaptive `user` index ⇒ commands queued for one tick run
**newest-first** (ORDER-CONSOLE-COMMANDS).

### 4.9 COL-USERS-MESSAGES
`{_id, user, respondent, date, type:'in'|'out', text, unread, outMessage?}` `reference/backend/lib/game/api/user-messages.js:44-72`.

### 4.10 COL-USERS-NOTIFICATIONS
`{_id, user, message, date(ms), type:'msg'|'error', count}`; grouped per `groupInterval`/`errorsInterval`
buckets via upsert+`$inc` `reference/driver/lib/index.js:185-227,397-405,455-471`. Notify intents: max 20 per tick,
message cut to 500 chars, `groupInterval` clamped 0..1440 minutes.

### 4.11 COL-USERS-MONEY
`{_id, date, tick, user, type:'market.fee'|'market.sell'|'market.buy', balance, change, market:{...}}`
(balance/change in credits, i.e. /1000) `reference/engine/src/processor/global-intents/market.js:154-164,347-385,438-480,512-529`.

### 4.12 COL-USERS-RESOURCES
`{_id, date, resourceType, user, change, balance, marketOrderId?, market:{orderId, anotherUser}}`
`reference/engine/src/processor/global-intents/market.js:451-494`.

### 4.13 COL-USERS-POWER-CREEPS
`{_id, user, name(≤50), className:'operator', level, hitsMax, store:{}, storeCapacity, spawnCooldownTime,
powers:{<PWR_id>:{level, cooldownTime?}}, shard?, deleteTime?}`
`reference/engine/src/processor/global-intents/power/createPowerCreep.js:27-37`,
`reference/backend/lib/game/api/power-creeps.js:52-62`.
* `spawnCooldownTime`: ms timestamp (0 initially); **`null` while spawned** (`spawnPowerCreep`
  `reference/engine/src/processor/global-intents/power/spawnPowerCreep.js:24-28`); set to
  `Date.now()+POWER_CREEP_SPAWN_COOLDOWN` on death `.../_diePowerCreep.js:55-58`.
* `deleteTime`: ms; `null` when cancelled/spawned.
* Upgrade: `level+1`, `hitsMax+1000`, `storeCapacity+100`, `powers[p].level+1` `.../upgradePowerCreep.js:45-65`.
* Adaptive `user` index ⇒ `Game.powerCreeps` key order changes whenever a power-creep doc is written
  (ORDER-POWER-CREEPS).
* **GOTCHA**: backend `/power-creeps/delete` during `powerExperimentationTime` calls
  `db['users.power_creeps'].remove(...)` `reference/backend/lib/game/api/power-creeps.js:79-81`, but the storage
  proxy has no `remove` method (`find findOne by clear count ensureIndex removeWhere insert update bulk findEx`
  `reference/common/lib/storage.js:91-108`), so that request always fails (TypeError → rejected promise).

### 4.14 COL-USERS-INTENTS
`{_id, user, intents:{<globalIntentName>: [intent...]}}` (market + power creep management), inserted by runners
`reference/driver/lib/index.js:229-232` or HTTP `reference/backend/lib/game/api/game.js:714-720`; cleared each tick
after global processing `reference/driver/lib/index.js:326-328`.

### 4.15 COL-MARKET-ORDERS
`{_id, created(tick), createdTimestamp(ms), user?, active, type:'sell'|'buy', amount, remainingAmount,
totalAmount, resourceType, price(credits×1000 integer), roomName}`
`reference/engine/src/processor/global-intents/market.js:135-150`. NPC orders have no `user`
`reference/backend/lib/cronjobs.js:75-115`. Runtime exposes `id` and price/1000.

### 4.16 COL-MARKET-STATS
`{_id, resourceType, date:'YYYY-MM-DD', transactions, volume, avgPrice, stddevPrice}` recomputed hourly from 14 days
of `users.money` `market.sell` rows `reference/backend/lib/cronjobs.js:472-523`.

### 4.17 COL-TRANSACTIONS
`{_id, time(tick), sender?, recipient?, resourceType, amount, from, to, description?, order?:{id,type,price}}`
`reference/engine/src/processor/global-intents/market.js:58-66,339-343`; runtime loads last 100 sent and received
`reference/driver/lib/runtime/data.js:138-139`.

### 4.18 COL-LEADERBOARD
`leaderboard.power`, `leaderboard.seasons`, `leaderboard.world` are registered but never written; HTTP returns
static empty data `reference/backend/lib/game/api/leaderboard.js:14-32`.

### 4.19 COL-MARKET-INTENTS-LEGACY
Present (empty) in `db.original.json` only; not registered, never used.

---

## 5. Users

### 5.1 USER-SPECIAL-IDS
* `'2'` = Invader, `'3'` = Source Keeper (hard-coded in processor `reference/engine/src/processor.js:13-14`,
  websocket `reference/backend/lib/game/socket/rooms.js:11-14`); both `cpu:0` so they never run user code.
* System user `username == SYSTEM_USERNAME ('Screeps')` (default `_id 'f4b532d08c3952a'`) owns ruins created by
  respawn `reference/backend/lib/utils.js:115-142`; its name appears in `hardSign`.

### 5.2 USER-CPU-FIELDS
Runner budget: if `cpu` truthy: `bucket = max(0, cpuAvailable||0)`, `limit = min(bucket + cpu, cpuMaxPerTick(500))`;
else `cpu = bucket = Infinity` `reference/driver/lib/runtime/data.js:150-166`. After the run
`cpuAvailable = min(cpuAvailable + cpu - ceil(usedCleanTime + 0.2*intentCount), cpuBucketSize(10000))`
(`say` and `pull` are free) `reference/driver/lib/runtime/make.js:98-111,193-214`. Negative bucket ⇒ the run is
skipped and `cpuAvailable = (cpuAvailable < -2*cpu) ? -2*cpu : cpuAvailable + cpu`, `lastUsedCpu = lastUsedDirtyTime = 0`
`:72-83`. (Detailed CPU rules belong to the runner spec.)

### 5.3 USER-GCL-POWER-MONEY
* `gcl` += upgrade effect `reference/engine/src/processor/intents/creeps/upgradeController.js:84-86`;
  GCL level = `floor((gcl/GCL_MULTIPLY)^(1/GCL_POW)) + 1`; points needed for level L: `calcNeededGcl(L) =
  GCL_MULTIPLY * (L-1)^GCL_POW` `reference/engine/src/utils.js:661-663`.
* `power` += processed power `reference/engine/src/processor/intents/power-spawns/process-power.js:36-38`;
  power level = `floor((power/POWER_LEVEL_MULTIPLY)^(1/POWER_LEVEL_POW))`
  `reference/engine/src/processor/global-intents/power/createPowerCreep.js:11`.
* `money` is credits×1000; `/me` returns `money/1000` `reference/backend/lib/game/api/auth.js:128`.

### 5.4 USER-BADGE
`{type: 1..24 | {path1, path2}, color1, color2, color3: '#rrggbb', param: -100..100, flip: bool}`;
validation `reference/backend/lib/game/api/user.js:146-176`. CLI bot badges: `type = floor(r*24)+1`, colors
`'#'+floor(r*0xffffff).toString(16)` (**not zero-padded**, may be < 6 hex digits), `flip = r>0.5`,
`param = floor(r*200)-100` `reference/backend/lib/cli/bots.js:37-46`.

### 5.5 USER-ROOMS-LIST
`rooms[]` maintained by `bulkUsers.addToSet/pull` when a controller is claimed/owned/downgraded/unclaimed
`reference/driver/lib/index.js:542-552`, `reference/engine/src/processor/intents/controllers/tick.js:18,53`,
`.../controllers/unclaim.js:16`, `.../creeps/claimController.js:55`. `controllers/tick` re-adds the room every tick
the controller is owned (idempotent). Used for the GCL claim check (`rooms.length`).

### 5.6 USER-ACTIVE-FLAG
Users queued each tick: `active != 0` **and** `cpu > 0`, sorted by `lastUsedDirtyTime` desc
`reference/driver/lib/index.js:144-151`. `active` is set to `10000` on code upload/spawn placement
`reference/backend/lib/game/api/user.js:102`, `reference/backend/lib/game/api/game.js:405`, to `0` when the user has
no room objects `reference/driver/lib/runtime/data.js:106-108`. Values seen: `1`, `true`, `false`, `10000`, `0`.

### 5.7 USER-SEGMENT-FIELDS
`activeSegments: number[]`, `defaultPublicSegment: number|null`,
`activeForeignSegment: {username, id?, user_id}` (unset via `$unset` when cleared)
`reference/driver/lib/runtime/make.js:200-247`.

### 5.8 USER-CREATE-PATHS
* steam: `{steam:{id}, cpu:100, cpuAvailable:0, registeredDate, credits:0, gcl:0, powerExperimentations:30}` +
  code branch `default` with `{main:''}` `reference/backend/lib/game/api/auth.js:20-70`; username set later via
  `/set-username` (`^[a-zA-Z0-9_-]{3,}$`) `reference/backend/lib/game/api/register.js:44-73`.
* CLI bot: GEN-BOT-SPAWN.

---

## 6. Room object documents (`rooms.objects`)

These field sets are exactly what the room websocket diff transmits (WIRE-ROOM-DIFF) and what the runtime reads.
"Written by" cites the creating code. Fields set to `null` remain present with `null`.

### 6.1 OBJ-ENVELOPE
`{_id, type, room, x, y}` always; `user` (string) for owned objects; optional `effects`, `notifyWhenAttacked`,
`hits`, `hitsMax`, store fields (6.2). `x,y` integers 0..49. `room` is the room name.

### 6.2 OBJ-STORE-MODEL
* `store: {resource: amount}` — may contain zero entries; `Store` API hides falsy amounts
  `reference/engine/src/game/store.js:19-25` and returns 0 for unknown resources via a Proxy `:87-96`.
* `storeCapacity: number|null` — total capacity for "open" stores.
* `storeCapacityResource: {resource: number|null}` — per-resource caps.
* `capacityForResource(o, r) = o.storeCapacityResource?.[r] || max(0, (o.storeCapacity||0) - sum(o.storeCapacityResource))`
  `reference/engine/src/utils.js:671-675` (lodash `_.sum` treats `null` as 0).
* `calcResources(o) = sum(store)` `reference/engine/src/utils.js:615-621`.
* Type usage: single-resource (`storeCapacityResource` only): spawn, extension, link, tower, nuker (`energy`,`G`),
  powerSpawn (`energy`,`power`); open (`storeCapacity` only): storage, terminal, container, factory, creep,
  power creep; lab: both (see OBJ-LAB); tombstone/ruin: `store` only.

### 6.3 OBJ-EFFECTS
`effects: [{effect, power, level?, endTime, duration?}]`:
* power effects `{effect:pwr, power:pwr, level, endTime}` `reference/engine/src/processor/intents/power-creeps/usePower.js:278-288`;
* PWR_SHIELD rampart `{power:PWR_SHIELD, level, endTime}` (**no `effect` key**) `:248-254`;
* `EFFECT_INVULNERABILITY` / `EFFECT_COLLAPSE_TIMER` `{effect, power(=effect), endTime, duration}`
  `reference/backend/lib/strongholds.js:83-88`, `reference/engine/src/processor/intents/invader-core/stronghold/stronghold.js:30-35`.
An object whose `EFFECT_COLLAPSE_TIMER.endTime <= gameTime` is removed at tick end `reference/engine/src/processor.js:412-419`.

### 6.4 OBJ-ACTIONLOG
Reset to all-null keys at the start of each room processing and written back only if changed
`reference/engine/src/processor.js:48-167`:
creep `{attacked, healed, attack, rangedAttack, rangedMassAttack, rangedHeal, harvest, heal, repair, build, say,
upgradeController, reserveController}`; invaderCore `{transferEnergy, reserveController, attackController,
upgradeController}`; link `{transferEnergy}`; lab `{runReaction, reverseReaction}`; tower `{attack, heal, repair}`;
powerCreep `{spawned, attack, attacked, healed, power, say}`; factory `{produce}`. Values are `{x,y}` (or
`{message,isPublic}` for `say`, `{x,y,resourceType}` for `produce`).

### 6.5 OBJ-NOTIFY
`notifyWhenAttacked: bool` — set `true` on built structures `reference/engine/src/processor/intents/creeps/build.js:111-117`,
spawned creeps, power creeps; `false` for CLI bot spawn, stronghold containers/roads/creeps. Toggled by intent
for structures in `CONSTRUCTION_COST`, creeps and power creeps `reference/engine/src/processor.js:313-315`.

### 6.6 OBJ-OWNERSHIP
`user` is a string user id; NPC `'2'`/`'3'`; `null` on an unowned controller after downgrade/unclaim; absent on
unowned structures (roads, walls, containers, extractors in SK rooms, power banks...).

### 6.7 OBJ-CUSTOM-TYPES
Docs are schemaless; mods insert arbitrary `type`s and fields (`reference/launcher/init_dist/example-mods/custom-objects/simple.js`)
and register runtime prototypes `reference/driver/lib/index.js:56-73`. chitin's object store must allow unknown types
and unknown fields (preserved verbatim, sent over the wire).

### 6.8 OBJ-CREEP
Written by `reference/engine/src/processor/intents/spawns/create-creep.js:79-100`:
`{type:'creep', name, x, y (spawn's tile while spawning), body:[{type, hits(100), boost?}], store:{energy:0},
storeCapacity (50 × CARRY parts, recomputed with boosts), room, user, hits, hitsMax (100×parts), spawning:true,
fatigue:0, notifyWhenAttacked:true, tutorial?}`.
Later fields: `spawning:false` + new x,y on birth `.../spawns/_born-creep.js:44-48`; `ageTime` (tick of death) set on
the first non-spawning tick = `gameTime + CREEP_LIFE_TIME (or CREEP_CLAIM_LIFE_TIME if any CLAIM part)`
`.../creeps/tick.js:87-91` (`ticksToLive = ageTime - time` `reference/engine/src/game/creeps.js:77`);
`actionLog`; `interRoom:{room,x,y}` (pending room change, cleared to `null` by global processing);
`userSummoned`, `strongholdId`, `tombstoneDecay` (optional NPC fields).

**OBJ-CREEP-BIRTH.** A spawning creep's doc is inserted by its spawn's `createCreep` (bulk insert
`.../spawns/create-creep.js:100`), so it is always created *after* its spawn and, by ORDER-ROOM-OBJECTS, the
processor's per-object tick loop (`reference/engine/src/processor.js:342-409`) reaches the spawn first.
In the tick T processing where `gameTime >= spawning.spawnTime-1`:
1. `spawns/tick` finds the creep and calls `_born-creep` `.../spawns/tick.js:12-27`; on success
   `bulk.update(creep, {x, y, spawning:false})` `.../spawns/_born-creep.js:44-49` — `bulk.update` merges into the
   live object (`_.merge(id, data)` `reference/driver/lib/bulk.js:50-53`, STORE-BULK-WRITER), so the in-memory
   creep is already non-spawning.
2. Later in the same loop `creeps/tick` sees `spawning` falsy, takes the normal branch and, because `ageTime` is
   unset, sets `ageTime = T + (any CLAIM ? CREEP_CLAIM_LIFE_TIME : CREEP_LIFE_TIME)` `.../creeps/tick.js:87-92`.
* Result: on the creep's first visible non-spawning tick (T+1), `ticksToLive = ageTime - Game.time`
  (`reference/engine/src/game/creeps.js:77`) is defined — `CREEP_LIFE_TIME-1` (or `CREEP_CLAIM_LIFE_TIME-1`);
  it is **never `undefined`** for a spawn-born creep. While `spawning` is true `ticksToLive` is `undefined`
  (no `ageTime`).
* If birth fails (all tiles blocked, no spawn-stomp), the spawn sets `spawning.spawnTime = T+1` (lodash merge keeps
  `spawning.name`), the creep tick sees the name still matching and does nothing `.../creeps/tick.js:37-47`.
* The creep tick's own birth fallback (`!spawn.spawning || spawn.spawning.name != object.name`
  `.../creeps/tick.js:44-46`) does **not** set `ageTime` that tick (if/else), so a creep born through it would show
  `ticksToLive undefined` for one tick; with the canonical order it is reached only for an orphaned spawning creep
  whose spawn is no longer spawning it.
* NPC creeps differ: keepers (OBJ-CREEP-KEEPER, `spawning:false`) and cron invaders (OBJ-CREEP-INVADER, no
  `spawning` field) are inserted already born but without `ageTime`, so their first visible tick has
  `ticksToLive undefined` (the invader's stored `ticksToLive:1500` field is not what the getter reads); stronghold creeps
  (OBJ-CREEP-STRONGHOLD) are born by `invader-core/tick.js:29-38` but already carry `ageTime`.

### 6.9 OBJ-CREEP-KEEPER
`reference/engine/src/processor/intents/keeper-lairs/tick.js:50-64`: `{name:'Keeper'+lairId, x,y (lair tile),
body: 17×TOUGH, 13×MOVE, then 10×(ATTACK, RANGED_ATTACK) interleaved, store:{energy:0}, storeCapacity:0,
type:'creep', room, user:'3', hits:5000, hitsMax:5000, spawning:false, fatigue:0}` (no `ageTime` until first tick).

### 6.10 OBJ-CREEP-INVADER
`reference/backend/lib/cronjobs.js:275-310`: `{type:'creep', user:'2', body:[{type,hits:100,boost?}],
hits, hitsMax (100×parts), ticksToLive:1500, x,y (exit tile), room, fatigue:0, store:{}, storeCapacity:0,
name:'invader_<room>_<floor(r*1000)>'}`; HTTP-summoned adds `userSummoned` `reference/backend/lib/game/api/game.js:659-678`.
**GOTCHA**: invaders carry a legacy `ticksToLive` field (stored, ignored by engine life logic which uses `ageTime`).

### 6.11 OBJ-CREEP-STRONGHOLD
`reference/engine/src/processor/intents/invader-core/create-creep.js:38-55`: `{strongholdId, type:'creep', name,
x,y (core), body (with boosts), store:{energy:0}, storeCapacity, room, user (core user), hits, hitsMax,
spawning:true, fatigue:0, notifyWhenAttacked:false, ageTime: core.decayTime}`.

### 6.12 OBJ-POWERCREEP
In-room doc = copy of the `users.power_creeps` doc (same `_id`) plus `{type:'powerCreep', room, x, y (power
spawn tile), hits:hitsMax, ageTime: gameTime+POWER_CREEP_LIFE_TIME, actionLog:{spawned:true},
notifyWhenAttacked:true}` `reference/engine/src/processor/global-intents/power/spawnPowerCreep.js:30-39`.
Carries `name, className, user, level, hitsMax, store, storeCapacity, powers, spawnCooldownTime:null` etc.
**GOTCHA**: since it is an `Object.assign` copy of the Loki doc, the users.power_creeps `meta` object is copied too.

### 6.13 OBJ-SOURCE
`{type:'source', x,y, room, energy, energyCapacity, ticksToRegeneration?, nextRegenerationTime?, invaderHarvested?, effects?}`.
* generated with `energy = energyCapacity = SOURCE_ENERGY_NEUTRAL_CAPACITY (1500)`, `ticksToRegeneration =
  ENERGY_REGEN_TIME (300)` `reference/backend/lib/cli/map.js:475-483`. `ticksToRegeneration` is a **legacy stored
  field** ignored by the engine (API derives `ticksToRegeneration` from `nextRegenerationTime`
  `reference/engine/src/game/sources.js:38`).
* capacity is re-derived by the processor each time the room is processed: room with controller ⇒ 3000 if owned
  or reserved, else 1500 (energy clamped down); room **without controller ⇒ `SOURCE_ENERGY_KEEPER_CAPACITY` 4000**
  (applies to SK rooms *and* sector centers) `reference/engine/src/processor/intents/sources/tick.js:46-59`.
* `nextRegenerationTime` set to `gameTime+300` when first below capacity, then `null` + refill at
  `gameTime >= nextRegenerationTime-1` `:10-29`.
* `invaderHarvested` += harvested amount `reference/engine/src/processor/intents/creeps/harvest.js:45-48`; reset to
  0 by place-spawn and invader cron.

### 6.14 OBJ-MINERAL
`{type:'mineral', mineralType, density(1..4), mineralAmount, x,y, room, nextRegenerationTime?, effects?}`
`reference/backend/lib/cli/map.js:545-553`. Regeneration after `MINERAL_REGEN_TIME` with density re-roll
`reference/engine/src/processor/intents/minerals/tick.js:8-37` (density always re-rolled from LOW/ULTRA, else with
`MINERAL_DENSITY_CHANGE`; re-roll repeats until different). **GOTCHA**: the default DB's minerals have *fractional*
`mineralAmount` (e.g. 40372.87…) from an older formula; they persist until first depletion.

### 6.15 OBJ-DEPOSIT
`{type:'deposit', depositType, x,y, room, harvested:0, decayTime, cooldownTime?}`
`reference/backend/lib/cronjobs.js:638`. Each harvest: `harvested += amount`, `cooldownTime = gameTime +
ceil(DEPOSIT_EXHAUST_MULTIPLY*harvested^DEPOSIT_EXHAUST_POW)` (only if > 1), `decayTime = gameTime +
DEPOSIT_DECAY_TIME` `reference/engine/src/processor/intents/creeps/harvest.js:114-139`,
`reference/engine/src/processor/intents/deposits/tick.js:7-11`. Removed when `gameTime >= decayTime-1`
`reference/engine/src/processor.js:421-426` (and `> decayTime` in deposits/tick).

### 6.16 OBJ-CONTROLLER
Generated: `{type:'controller', room, x, y, level:0}` `reference/backend/lib/cli/map.js:486`. Owned/live fields:
| field | meaning |
|---|---|
| `user` | owner id, `null` when lost |
| `level` | 0..8 |
| `progress` | points toward next level (API hides at level 0) |
| `progressTotal` | written `0` by respawn only; API computes from `CONTROLLER_LEVELS` |
| `downgradeTime` | absolute tick; claim writes `null`, and the same tick's object phase (`controllers/tick`) immediately re-sets it to `gameTime + CONTROLLER_DOWNGRADE[level] + 1`; backend/CLI spawn placement leaves `null` until the room is next processed `reference/engine/src/processor/intents/controllers/tick.js:33-36` |
| `reservation` | `{user, endTime}` or `null` |
| `sign` | `{user, text, time, datetime(ms)}` or `null` `reference/engine/src/processor/intents/creeps/signController.js:23-28` |
| `hardSign` | `{text, time, datetime}` (system sign; overrides `sign` in API; no OSS writer) |
| `safeMode` | absolute tick end; `safeModeAvailable` count; `safeModeCooldown` absolute tick |
| `upgradeBlocked` | absolute tick |
| `isPowerEnabled` | bool |
| `effects` | e.g. stronghold invulnerability |
| `hits`, `hitsMax` | written `0` by respawn only |
| `tutorial`, `bindUser`, `promoPeriodUntil`… | official-only flags read by OSS (`tutorial` `controllers/tick.js:33`, `bindUser` `reference/backend/lib/game/api/game.js:110`) |

### 6.17 OBJ-KEEPERLAIR
`{type:'keeperLair', room, x, y, nextSpawnTime?}` `reference/backend/lib/cli/map.js:488-491`; `nextSpawnTime`
set to `gameTime + ENERGY_REGEN_TIME` when the keeper is missing or below 5000 hits, keeper (re)spawned at
`gameTime >= nextSpawnTime-1` `reference/engine/src/processor/intents/keeper-lairs/tick.js:12-67`. No `user`
(API reports owner "Source Keeper").

### 6.18 OBJ-POWERBANK
`{type:'powerBank', x,y, room, store:{power}, hits:POWER_BANK_HITS, hitsMax, decayTime: gameTime+POWER_BANK_DECAY}`
`reference/backend/lib/cronjobs.js:231-239`. Removed at `gameTime >= decayTime-1`. Ruin decay 10
(`RUIN_DECAY_STRUCTURES`).

### 6.19 OBJ-PORTAL
`{type:'portal', x,y, room, destination:{x,y,room} | {shard, room}, decayTime?, unstableDate?}`.
No OSS code creates portals; semantics from readers: `unstableDate` (ms) passed ⇒ `decayTime = gameTime +
PORTAL_DECAY`, `unstableDate:null`; at `gameTime > decayTime` the portal and a `constructedWall` at
`(x+1, y+1)` are removed `reference/engine/src/processor/intents/portals/tick.js:8-24`. Creeps standing on a portal
get `interRoom = destination` `reference/engine/src/processor/intents/creeps/tick.js:80-85`.

### 6.20 OBJ-INVADERCORE
Spawned stronghold core `reference/backend/lib/strongholds.js:71-92`: template fields (`type, level,
strongholdBehavior`) + `{room, x, y, user:'2', templateName, hits:INVADER_CORE_HITS, hitsMax, nextExpandTime,
depositType, deployTime, strongholdId:'<room>_<gameTime>', effects:[invulnerability]}`. After deployment:
`deployTime:null, decayTime, effects += collapse timer` `reference/engine/src/processor/intents/invader-core/stronghold/stronghold.js:26-42`;
also `spawning:{name,needTime,spawnTime}`, `population:[{body,behavior}]`, `actionLog`.
Expansion cores: `{type:'invaderCore', level:0, room, x, y, user:'2', hits, hitsMax, strongholdId,
effects (copied from parent)}` `reference/backend/lib/strongholds.js:317-327`.

### 6.21 OBJ-SPAWN
Built: `{name, user, store:{energy:0}, storeCapacityResource:{energy:SPAWN_ENERGY_CAPACITY}, hits:SPAWN_HITS,
hitsMax}` + envelope `reference/engine/src/processor/intents/creeps/build.js:119-128`. First spawn:
`store.energy = SPAWN_ENERGY_START`, `spawning:null`, `notifyWhenAttacked` true (HTTP) / false (bots)
`reference/backend/lib/game/api/game.js:344-357`. `spawning: null | {name, needTime, spawnTime, directions?}`
`reference/engine/src/processor/intents/spawns/create-creep.js:56-63`. `off: bool` = beyond RCL allowance
(nearest-to-controller kept) `reference/engine/src/processor/intents/_calc_spawns.js:6-41`.

### 6.22 OBJ-EXTENSION
`{user, store:{energy:0}, storeCapacityResource:{energy:0}, hits:EXTENSION_HITS, hitsMax, off?}`; capacity
re-derived from `EXTENSION_ENERGY_CAPACITY[controller.level]` whenever processed
`reference/engine/src/processor/intents/extensions/tick.js:10-17`.

### 6.23 OBJ-ROAD
`{hits, hitsMax, nextDecayTime}`; `hits = ROAD_HITS × 5 (swamp) × 150 (wall)` multipliers by terrain mask
`reference/engine/src/processor/intents/creeps/build.js:171-187`; decay `ROAD_DECAY_AMOUNT` with the same
multipliers every `ROAD_DECAY_TIME` `.../roads/tick.js:10-31`. No `user`.

### 6.24 OBJ-CONSTRUCTEDWALL
`{hits:WALL_HITS, hitsMax:WALL_HITS_MAX}`; `hitsMax` forced to 0 when the controller level disallows walls
`reference/engine/src/processor/intents/constructedWalls/tick.js:10-15`. Newbie/temporary walls (official only):
`{user, decayTime: tick | {timestamp}, newbieWall?, ticksToLive?}` handled `:17-44`.

### 6.25 OBJ-RAMPART
`{user, hits:RAMPART_HITS, hitsMax: RAMPART_HITS_MAX[level] (owner) else 0, nextDecayTime, isPublic?}`
`reference/engine/src/processor/intents/creeps/build.js:161-169`; stronghold ramparts add `strongholdId`, `isPublic:true`,
`hitsTarget`, `decayTime`, `effects`; PWR_SHIELD ramparts have `hitsMax:0` + shield effect.

### 6.26 OBJ-LINK
`{user, store:{energy:0}, storeCapacityResource:{energy:LINK_CAPACITY}, cooldown:0, hits:LINK_HITS,
hitsMax:LINK_HITS_MAX}`; `cooldown` is a **relative countdown** decremented each processed tick
`reference/engine/src/processor/intents/links/tick.js:11-22`.

### 6.27 OBJ-STORAGE
`{user, store:{energy:0}, storeCapacity:STORAGE_CAPACITY, hits, hitsMax}`; `storeCapacity` re-derived (0 if the
controller does not allow it; + PWR_OPERATE_STORAGE effect) `reference/engine/src/processor/intents/storages/tick.js:6-19`.

### 6.28 OBJ-TOWER
`{user, store:{energy:0}, storeCapacityResource:{energy:TOWER_CAPACITY}, hits:TOWER_HITS, hitsMax, actionLog}`.

### 6.29 OBJ-OBSERVER
`{user, hits:OBSERVER_HITS, hitsMax, observeRoom}`; `observeRoom` reset to `null` each processed tick and set
from the intent `reference/engine/src/processor.js:116-118,284-288,403-406`.

### 6.30 OBJ-POWERSPAWN
`{user, store:{energy:0}, storeCapacityResource:{energy:5000, power:100}, hits, hitsMax}`.

### 6.31 OBJ-EXTRACTOR
`{user?, hits:EXTRACTOR_HITS, hitsMax, cooldown?}`; `cooldown` relative countdown set to `EXTRACTOR_COOLDOWN`
after a harvest `reference/engine/src/processor/intents/extractors/tick.js:9-25`. **Unowned extractors** exist in
SK/center rooms of the default world (GEN-DEFAULT-WORLD).

### 6.32 OBJ-LAB
Built: `{user, hits, hitsMax, mineralAmount:0 (legacy, unused), cooldown:0 (legacy), store:{energy:0},
storeCapacity: LAB_ENERGY_CAPACITY+LAB_MINERAL_CAPACITY, storeCapacityResource:{energy:LAB_ENERGY_CAPACITY}}`
`reference/engine/src/processor/intents/creeps/build.js:222-233`. With a mineral: `storeCapacityResource[mineral]
= LAB_MINERAL_CAPACITY`, `storeCapacity:null`; when emptied: `storeCapacityResource[mineral]=null`,
`storeCapacity` restored `reference/engine/src/processor/intents/labs/run-reaction.js:54-85`. Cooldown is the
absolute `cooldownTime` `:56` (API `cooldown` derived `reference/engine/src/game/structures.js:310`).

### 6.33 OBJ-TERMINAL
`{user, store:{energy:0}, storeCapacity:TERMINAL_CAPACITY, hits, hitsMax, cooldownTime?, send?}`.
**GOTCHA**: a pending `send` intent is *stored on the terminal doc* (`send: intent`)
`reference/engine/src/processor/intents/terminal/send.js:32` and cleared (`send:null`) by global processing
`reference/engine/src/processor/global-intents/market.js:71-75`.

### 6.34 OBJ-CONTAINER
`{store:{energy:0}, storeCapacity:CONTAINER_CAPACITY, hits:CONTAINER_HITS, hitsMax, nextDecayTime}` (no user);
decay period 500 if the room controller level > 0 else 100 `reference/engine/src/processor/intents/containers/tick.js:10-31`.

### 6.35 OBJ-NUKER
`{user, store:{energy:0}, storeCapacityResource:{energy:NUKER_ENERGY_CAPACITY, G:NUKER_GHODIUM_CAPACITY},
hits, hitsMax, cooldownTime: gameTime+NUKER_COOLDOWN}` (built on cooldown) `reference/engine/src/processor/intents/creeps/build.js:265-274`.

### 6.36 OBJ-FACTORY
`{user, store:{energy:0}, storeCapacity:FACTORY_CAPACITY, hits, hitsMax, cooldown:0 (legacy), cooldownTime?, level?}`;
capacity 0 when the controller disallows `reference/engine/src/processor/intents/factories/tick.js:7-12`.

### 6.37 OBJ-CONSTRUCTIONSITE
`{structureType, x, y, type:'constructionSite', room, user, progress:0, progressTotal, name? (spawn only)}`
`reference/engine/src/processor/intents/room/create-construction-site.js:56-69`; road `progressTotal` × 5 swamp × 150
wall `:37-46`.

### 6.38 OBJ-ENERGY
Every dropped resource: `{type:'energy', x, y, room, [resourceType]: amount, resourceType}`
`reference/engine/src/processor/intents/_create-energy.js:42-51`. **GOTCHA**: the amount lives under a key named after
the resource (`{type:'energy', resourceType:'H', H:120}`); API `amount = o[o.resourceType||'energy']`
`reference/engine/src/game/resources.js:35-39`; `LOOK_RESOURCES = "resource"` differs from the stored type. Drops merge
into an existing drop of the same resource on the tile; decay `ceil(amount/ENERGY_DECAY)` per processed tick
`reference/engine/src/processor/intents/energy/tick.js:10-23`.

### 6.39 OBJ-TOMBSTONE
Creep: `{type:'tombstone', room, x, y, user, deathTime, decayTime, creepId, creepName, creepTicksToLive,
creepBody:[types], creepSaying?, store}` `reference/engine/src/processor/intents/creeps/_die.js:21-35`
(`decayTime = gameTime + body.length*TOMBSTONE_DECAY_PER_PART` or `tombstoneDecay`).
Power creep: `{…, powerCreepId, powerCreepName, powerCreepTicksToLive, powerCreepClassName, powerCreepLevel,
powerCreepPowers:{p:{level}}, powerCreepSaying?}` `.../global-intents/power/_diePowerCreep.js:8-24`.

### 6.40 OBJ-RUIN
`{type:'ruin', room, x, y, user?, structure:{id, type, hits:0, hitsMax, user}, store, destroyTime, decayTime}`
`reference/engine/src/processor/intents/structures/_destroy.js:21-49`; `decayTime = gameTime +
(RUIN_DECAY_STRUCTURES[type] || RUIN_DECAY)`, extended to the collapse timer if any. Backend-created ruins use
`decayTime = gameTime+100000` (place-spawn, original owner kept) or `gameTime+500000` (respawn, system user)
`reference/backend/lib/game/api/game.js:379-395`, `reference/backend/lib/utils.js:126-142`.

### 6.41 OBJ-NUKE
`{type:'nuke', room (target), x, y, landTime, launchRoomName}` `reference/engine/src/processor/intents/nukers/launch-nuke.js:42-49`.

### 6.42 OBJ-FLAG
Flags are **not** room objects; they live in `rooms.flags` (COL-ROOMS-FLAGS). A legacy `type:'flag'` room object
(`color`, `secondaryColor` as strings) is still handled by `reference/engine/src/processor/intents/flags/intents.js:3-23`
and skipped by history/mapView/visibility code; no current writer.

---

## 7. Wire representation

### 7.1 WIRE-ROOM-DIFF
`common.getDiff(old, new)` `reference/common/index.js:69-136` (the websocket uses this one; the engine copy
`reference/engine/src/utils.js:191-260` additionally skips `_id`):
```
result = {}
for o in old: if new has o._id:
    d = {}
    for k in o: if new[k] === undefined: d[k]=null
                elif typeof differs or (o[k] truthy and new[k] falsy): d[k]=new[k]
                elif o[k] is object (incl. arrays): d[k] = {sub: new[k][sub] for sub in o[k] if !isEqual} ∪ {sub new in new[k]}; drop if empty
                elif !isEqual: d[k]=new[k]
    for k in new not in o: d[k]=new[k]
    if d non-empty: result[id]=d
  else result[id]=null
for n in new not in old: result[n._id]=n
```
**GOTCHA**: nested keys removed from an object are sent as `undefined` (dropped by JSON) → clients keep stale
sub-keys; arrays are diffed as objects keyed by index. This is why the engine zeroes instead of deleting (STORE-BULK-WRITER).

### 7.2 WIRE-ROOM-EVENT
`room:<name>` → `{objects: diff, flags: <this user's rooms.flags data string>, gameTime, visual, users?, info:{mode:'world'}}`
`reference/backend/lib/game/socket/rooms.js:69-140`; max 2 subscribed rooms per user per tick (`USER_LIMIT`)
`:9,35-46`. **PERF**: each tick re-reads all objects of each subscribed room.

### 7.3 WIRE-MAP-VIEW
`roomMap2:<name>` → raw `mapView:` string `reference/backend/lib/game/socket/map.js:14-41`.

---

## 8. Terrain

### 8.1 TERRAIN-STRING
`rooms.terrain.terrain` is a 2500-char string, index `y*50 + x`, each char a decimal digit = bit mask
`TERRAIN_MASK_WALL(1) | TERRAIN_MASK_SWAMP(2)`; `0` plain `reference/common/index.js:25-41`,
`reference/common/lib/constants.js:291-293`. Every char is in `'0'..'3'`.

### 8.2 TERRAIN-CODE3
`3` = wall+swamp. It is *produced by the generator* (GEN-EMIT) and present in the default world (4749 tiles,
including 40 border tiles). Consumers:
* mask checks (`checkTerrain`, movement, building) treat it as wall (and as swamp for masks).
* `Room.Terrain.get` returns `(v&1) || (v&2) || 0` ⇒ **1** `reference/engine/src/game/rooms.js:1230-1233`;
  `getTerrainAt` ⇒ `'wall'` `reference/engine/src/game/map.js:251-258`.
* `getRawBuffer()` returns the raw byte **3** `reference/engine/src/game/rooms.js:1235-1241`. **GOTCHA**: bots that
  test `buf[i] === TERRAIN_MASK_WALL` see these tiles as non-wall; must be preserved.
* native pathfinder: `look_table = {0:plain_cost, 2:swamp_cost, 1:obstacle, 3:obstacle}`
  `reference/driver/native/src/pf.h:351`, `reference/driver/native/src/pf.cc:495-496`.
* exit grid: only `== 0` counts as an exit tile (TERRAIN-EXIT-GRID).
* road on a code-3 tile: cost/hits/decay multiplied by **both** ratios (5 × 150 = 750): construction 225 000,
  hits 3 750 000 `reference/engine/src/processor/intents/room/create-construction-site.js:37-46`,
  `reference/engine/src/processor/intents/creeps/build.js:171-187`.
* xxscreeps normalizes 3→1 on import `reference/xxscreeps/packages/xxscreeps/scripts/scrape-world.ts:172-176`
  (a deliberate 1:1 deviation; chitin should keep 3).

### 8.3 TERRAIN-ENCODE-DECODE
`encodeTerrain(list of {type:'wall'|'swamp',x,y})` → string; `decodeTerrain(str, room)` → list
`reference/common/index.js:25-59`; engine copies `reference/engine/src/utils.js:262-331`.
**PERF**: `encodeTerrain` does `_.filter(terrain,{x,y})` per tile (O(2500·n)); trivial to replace.

### 8.4 TERRAIN-CHECK
`common.checkTerrain(str,x,y,mask) = (parseInt(str[y*50+x]) & mask) > 0` `reference/common/index.js:61-63`;
engine accepts string or Uint8Array `reference/engine/src/utils.js:333-336`. Same row-wrap as 8.7 for x outside
0..49; indexes outside the string read `''` → `NaN & mask` (common) / `Number('') = 0` (engine) → not wall.

### 8.5 TERRAIN-WORLD-BLOB
`updateTerrainData()` `reference/backend/lib/cli/map.js:661-695`: loads all rooms and terrains; for each room:
* `status == 'out of borders'` → its terrain in the blob is replaced by 2500×`'1'` (the DB is untouched);
* with `m = /(W|E)(\d+)(N|S)(\d+)/`, rooms `m1+(m2+1)+m3+m4` and `m1+m2+m3+(m4+1)` (one step *further from the
  origin* in x and in y, same hemisphere) are appended as all-wall terrain if absent.
Result: the blob covers every room + an outer ring of wall rooms (default world: 121 + 22 = 143 entries; `W11N11`
is not added). **GOTCHA**: the ring is only on the "numerically larger" side; rooms across the W/E or N/S seam
(e.g. `E0N5` for a W-only world) are absent, so `new Room.Terrain('E0N5')` throws `Could not access room`.

### 8.6 TERRAIN-RUNTIME-BUFFER
Runner: one `SharedArrayBuffer` of `rooms*2500` bytes, room offsets `roomIndex*2500` in blob order, byte =
`Number(char)` `reference/driver/lib/runtime/make.js:20-51`; each isolate gets views
`reference/driver/lib/runtime/runtime.js:45-49` and its memory limit is raised by the terrain size
`reference/driver/lib/runtime/user-vm.js:30`. **PERF**: global (all rooms in every isolate); chitin can share a single
read-only arena (2500 B/room; 1 B/tile or 2-bit packed = 625 B/room as xxscreeps does
`reference/xxscreeps/packages/xxscreeps/game/terrain.ts:52-58`).

### 8.7 TERRAIN-API
`Room.Terrain(roomName)` throws `Could not access room <name>` if absent from the blob; `get(x,y)` as in 8.2
with **no bounds check**: index `y*50+x` is used directly, so `x` outside 0..49 wraps into the neighbouring row
(e.g. `get(50,0)` = tile (0,1)) and indexes outside 0..2499 read `undefined` → 0 (plain); `getRawBuffer(dest?)` copies all 2500 raw bytes
`reference/engine/src/game/rooms.js:1223-1242`. `Game.map.getTerrainAt` (deprecated) bounds-checks 0..49 and
returns `undefined` for unknown rooms `reference/engine/src/game/map.js:235-259`.

### 8.8 TERRAIN-PF-PACK
Native pathfinder load: per room, 625 bytes, index `ii = x*50 + y` (**column-major**), 2 bits per tile at
`(ii%4)*2` `reference/driver/lib/path-finder.js:23-46`.

### 8.9 TERRAIN-EXIT-GRID
For every accessible room, count border tiles with terrain byte `== 0` on each side (`t` y=0, `r` x=49, `b` y=49,
`l` x=0, scanning all 50 tiles incl. corners); store only non-zero counts, keyed `"x,y"`
`reference/driver/lib/runtime/mapgrid.js:68-120`; same algorithm in `reference/backend/lib/strongholds.js:163-223`.
**GOTCHA**: swamp exit tiles (`2`) do not count; a side whose only open border tiles are swamps has no exit for
`describeExits`/`findRoute` (the default world has 2 swamp border tiles). Rooms not in `accessibleRooms` have no grid
entry.

### 8.10 TERRAIN-PNG
`updateRoomImageAssets`: `map/<room>.png` 150×150 (3× zoom) and a 200×200 `map/zoom2/<firstRoom>.png` for the 4×4
block at `floor(x/4)*4, floor(y/4)*4`; colors wall (0,0,0), swamp (35,37,19), border plain (50,50,50), plain
(43,43,43) `reference/backend/lib/cli/map.js:611-659`, `reference/backend/lib/utils.js:180-248`. Client asset only.

---

## 9. Room names and coordinates

### 9.1 ROOMNAME-COORDS
Integer room coordinates (engine/common convention): `W<n>` → `x = -n-1`, `E<n>` → `x = n`; `N<n>` → `y = -n-1`,
`S<n>` → `y = n` `reference/common/index.js:142-165`, `reference/engine/src/utils.js:374-392`.
So `W0`/`E0` are adjacent columns `x=-1`/`x=0` (likewise `N0`/`S0`): there is no "0" room column — the
**W0/E0 seam** sits between two rooms that both have number 0. North is negative y (screen-down = south).
World position of a tile for direction math: `(roomX*50 + x, roomY*50 + y)` `reference/engine/src/game/rooms.js:1390-1393`.

### 9.2 ROOMNAME-PARSE
Five parsers with different edge behaviour (port the engine one for the runtime, others where used):
1. common `roomNameToXY`: uppercases, regex `^(\w)(\d+)(\w)(\d+)$`, returns `[undefined,undefined]` on mismatch;
   any letter other than `W` is treated as east, other than `N` as south `reference/common/index.js:142-165`.
2. backend `roomNameToXY`: same regex **without** uppercasing; **throws** on mismatch (destructuring `null`)
   `reference/backend/lib/utils.js:28-45`.
3. engine `roomNameToXY`: `xx = parseInt(name.substr(1))`; vertical letter index = 2, 3 (xx ≥ 10) or 4 (xx ≥ 100);
   accepts lowercase `w`/`n`; no validation (garbage → NaN) `reference/engine/src/utils.js:374-392`.
   **GOTCHA**: supports at most 3 x-digits (W999); leading zeros (`W05N5`) mis-parse.
4. mapgrid copy of (1) `reference/driver/lib/runtime/mapgrid.js:5-31`.
5. pathfinder `parseRoomName` (ROOMNAME-PF-COORDS).
xxscreeps uses (3)'s algorithm `reference/xxscreeps/packages/xxscreeps/game/room/name.ts:29-49`.

### 9.3 ROOMNAME-FORMAT
`getRoomNameFromXY(x,y)`: `x<0 ? 'W'+(-x-1) : 'E'+x` + `y<0 ? 'N'+(-y-1) : 'S'+y` `reference/engine/src/utils.js:358-372`,
`reference/common/index.js:167-181`, `reference/backend/lib/utils.js:12-26`. In the runtime it is wrapped by a
cache keyed `id = (x+128)<<8 | (y+128)` with `roomNames[0] = 'sim'` `reference/engine/src/game/rooms.js:1260-1274`.
**GOTCHA**: `getRoomNameFromXY(-128,-128)` (W127N127) returns `'sim'`; coordinates outside −128..127 alias other
cache slots.

### 9.4 ROOMNAME-VALIDATE
Regexes in use: `^[WE]\d+[NS]\d+$` (generateRoom) `reference/backend/lib/cli/map.js:28`; `^(W|E)\d+(N|S)\d+$`
(describeExits/getRoomStatus/isRoomAvailable) `reference/engine/src/game/map.js:21,203,210`; unanchored-start
`(W|E)\d+(N|S)\d+$` (findRoute) `:80`; `^(W|E)\d+(S|N)\d+$` (launchNuke, flag setPosition)
`reference/engine/src/processor/intents/nukers/launch-nuke.js:26`; websocket channel `[a-zA-Z0-9_-]+`;
backend `checkGame` accepts any name starting with `W|E` `reference/backend/lib/game/api/game.js:20-37`.

### 9.5 ROOMNAME-POS-PACK
`RoomPosition` stores `__packedPos = (rx+128)<<24 | (ry+128)<<16 | x<<8 | y` (`'sim'` → rx=ry=−128)
`reference/engine/src/game/rooms.js:1276-1347`. Constructor check allows `rx+128` in `0..256` (the 256 case
overflows) and x,y in 0..49, else `Invalid arguments in RoomPosition constructor`.
**GOTCHA / PERF**: the official API therefore supports rooms `W127..E127` × `N127..S127` (255×255). A
`RoomPosition` in `W127N127` reads back `roomName === 'sim'`. Bots that read `__packedPos` depend on this layout.
chitin's "worlds far larger than official shards" cannot exceed this range without breaking `RoomPosition`
compatibility (see Open questions).

### 9.6 ROOMNAME-PF-COORDS
Pathfinder: `rx = 127 + (W ? -n : n+1)`, `ry = 127 + (N ? -n : n+1)`, valid 0..255 (`kWorldSize = 255`)
`reference/driver/lib/path-finder.js:9-21` — i.e. internal = engine coordinate + 128. Same W127..E127 bound.

### 9.7 ROOMNAME-WORLD-SIZE
`calcWorldSize(rooms) = max(maxX-minX+1, maxY-minY+1)` with **`maxX = maxY = 0` initial values**
`reference/common/index.js:183-193`; computed once at driver connect `reference/driver/lib/index.js:135-137` and by
backend `/world-size` (cached forever) `reference/backend/lib/game/api/game.js:706-712`; exposed as
`Game.map.getWorldSize()` `reference/engine/src/game/map.js:269-271`.
**GOTCHA**: for an all-W/N world the max stays 0, e.g. the default world (x,y ∈ −11..−1) yields **12**, not 11.

### 9.8 ROOMNAME-DISTANCE
`calcRoomsDistance(a,b,continuous)`: Chebyshev of room coords; if `continuous`, each axis
`min(worldSize - d, d)` (torus wrap with GOTCHA-9.7 size) `reference/engine/src/utils.js:644-655`. Used with
`continuous=true` for terminal costs and market deal ordering.

### 9.9 ROOMNAME-EDGE-CROSSING
A non-NPC creep/power creep on a border tile gets `interRoom = {room: neighbour, x: 49|0, y: 49|0}` (x=0 checked
first, then y=0, x=49, y=49) `reference/engine/src/processor/intents/creeps/tick.js:52-78`,
`.../power-creeps/tick.js:16-42`; global processing moves it if the target room is in the "normal & open" map,
otherwise `interRoom` stays set and the creep stays on the border tile `reference/engine/src/processor/global.js:33-43`,
`reference/driver/lib/index.js:433-434`.

### 9.10 ROOMNAME-DESCRIBE-EXITS
`describeExits(name)` → `{1: top, 3: right, 5: bottom, 7: left}` for sides with grid exits, `null` for invalid
names or rooms without grid data `reference/engine/src/game/map.js:20-46`. Neighbour names from coordinate ±1
(seam-safe).

---

## 10. Sectors

### 10.1 SECTOR-LAYOUT
A sector is the 10×10 block of room numbers `n = 10k+1 .. 10k+9` per axis per hemisphere, bounded by highway
rows/columns at `n % 10 == 0` (per hemisphere). Because both `W0` and `E0` are highways, the seam is a
**double highway** (two adjacent highway columns). Sector center = room with both numbers `≡ 5 (mod 10)`.

### 10.2 SECTOR-HIGHWAY
`isBus(coord)` on engine coordinates: `coord<0 && (coord+1)%10==0 || coord>0 && coord%10==0 || coord==0`
(true for display number `n%10==0`) `reference/backend/lib/utils.js:297-299` — defined but **unused**.
The authoritative marker is the room doc field `bus:true`, queried by power banks and deposits
(`reference/backend/lib/cronjobs.js:196,594`). `generateRoom` never sets `bus`/`depositType`
(**GOTCHA** for world generation: chitin's generator must set them for highway rooms). Crossroads (both coordinates
highways) are ordinary bus rooms in OSS.

### 10.3 SECTOR-CENTER
`isCenter(x,y)`: for each axis, display number `n` (`|x+1|` for negative, `|x|` otherwise) satisfies
`4 <= n%10 <= 6` `reference/backend/lib/utils.js:301-304`. `isVeryCenter`: both `n%10 == 5` `:306-309`.
Center 3×3 minus the very center = source-keeper rooms (keeper lairs, 3 sources, 1 mineral, unowned extractor, no
controller); the very center (e.g. `W5N5`) = 3 sources, mineral, unowned extractor, no lairs, no controller (default
world). Room doc flag `sourceKeepers` marks rooms with lairs. Center rooms (no controller) have 4000-energy sources
(OBJ-SOURCE). Used by invader boosts/raid size and stronghold room selection.

### 10.4 SECTOR-DEPOSIT-TYPE
Migration v4: `depositTypes = [silicon, metal, biomass, mist]`, index `(longitude + latitude) % 4` where both are
**strings** from the regex → string concatenation, e.g. `W10N5`: `"10"+"5" = "105"`, `105 % 4 = 1` → metal
`reference/storage/lib/db.js:55-63`. Matches every bus room in the default DB. chitin's generator should use this
formula for new highway rooms.

### 10.5 SECTOR-REGEX
* Invaders: sector of room R = `R._id.replace(/^([WE]\d*)\d([NS]\d*)\d$/, '^$1\\d$2\\d$')`
  (same tens digits) `reference/backend/lib/cronjobs.js:393`.
* Strongholds: center rooms `'^[WE]\\d*5[NS]\\d*5$'`; sector regex from center `reference/backend/lib/strongholds.js:132,227`.
* **GOTCHA**: genDeposits uses `'^[WE]\d*5[NS]\d*5$'` in a JS string, where `\d` collapses to `d`, so the effective
  regex is `^[WE]d*5[NS]d*5$` — it only matches `W5N5`, `E5N5`, `W5S5`, `E5S5`; deposits are therefore only
  generated around the four origin sectors `reference/backend/lib/cronjobs.js:574`.

---

## 11. Room status

### 11.1 STATUS-FIELDS
`status`: `'normal'` | `'out of borders'`; `openTime` (ms), `novice` (ms), `respawnArea` (ms) on the room doc
(COL-ROOMS). `out of borders` rooms are walled in the terrain blob (TERRAIN-WORLD-BLOB) and closed.

### 11.2 STATUS-OPEN-CLOSE
`map.openRoom(room, [timestamp])`: `$set {status:'normal', openTime?}` `reference/backend/lib/cli/map.js:568-580`;
`map.closeRoom(room)`: only rooms currently `normal` → `out of borders` `:582-587`. Neither updates the terrain blob
(call `updateTerrainData`), and neither activates the room.

### 11.3 STATUS-ACCESSIBLE
`accessibleRooms` = rooms with `status=='normal'` and `(!openTime || openTime < now)` (ENV-ACCESSIBLE-ROOMS).
The global inter-room step uses the same filter live each tick `reference/driver/lib/index.js:433-434`.

### 11.4 STATUS-DATA
`updateRoomStatusData` `reference/driver/lib/index.js:498-524`: select rooms with `novice>now || respawnArea>now ||
openTime>now || status=='out of borders'`; per room, first matching rule wins:
`novice>now` → `novice[room]=novice`; else `respawnArea>now` → `respawn[room]`; else `openTime>now` →
`closed[room]=openTime`; else → `closed[room]=Infinity` (serialized as `null`).
**GOTCHA**: a novice room that is also `out of borders` reports `novice`.

### 11.5 STATUS-API
`Game.map.getRoomStatus(name)` `reference/engine/src/game/map.js:209-233`: invalid name → `undefined`; missing data
→ throws `No runtime status data`; then `closed`, `novice`, `respawn` maps in that order (`{status, timestamp}`);
else `normal/null` if in `accessibleRooms`; else `closed/null` (non-existent rooms are closed).
`isRoomAvailable(name)` (deprecated, logs deprecation) = `_.contains(accessibleRooms, name)` `:201-207`.
**PERF**: both do a linear scan of the accessible list per call; data is at most 60 s stale.

### 11.6 STATUS-GATES
* creeps cannot enter non-normal/not-yet-open rooms (ROOMNAME-EDGE-CROSSING);
* nukes: launch refused and in-flight nukes deleted in novice/respawn rooms
  `reference/engine/src/processor/intents/nukers/launch-nuke.js:22-24`, `.../nukes/tick.js:10-14`;
  runtime `launchNuke` checks `roomStatusData` `reference/engine/src/game/structures.js:1362-1368`;
* claim limit `GCL_NOVICE` in novice rooms `reference/engine/src/game/creeps.js:853`;
* safe mode cooldown not applied in novice rooms `reference/engine/src/processor/intents/controllers/tick.js:29,61,68`;
* backend refuses spawn placement/flags/intents in `out of borders` or not-yet-open rooms
  `reference/backend/lib/game/api/game.js:20-37`;
* strongholds do not expand into novice/respawn rooms `reference/backend/lib/strongholds.js:250-252,284-286`.

---

## 12. World generation

### 12.1 GEN-ROOM-ENTRY
`map.generateRoom(roomName, opts)` `reference/backend/lib/cli/map.js:14-566`. Steps, in order:
1. `opts.exits ||= {}`; reject `"Invalid room name"` unless `^[WE]\d+[NS]\d+$` `:24-30`.
2. reject `'This room already exists'` if a `rooms` doc exists `:417-418`.
3. fetch neighbour terrains: top `(x,y-1)`, right `(x+1,y)`, bottom `(x,y+1)`, left `(x-1,y)` `:419-427`.
4. GEN-EXIT-MATCH for top, right, bottom, left (in that order) `:428-440`.
5. missing exit arrays default to `[]` `:442-445`.
6. defaults (Math.random only if undefined, in this order) `:447-461`:
   `terrainType = floor(r*27)+1` (1..27; **type 28 is never chosen by default**),
   `swampType = floor(r*14)` (0..13; 0 = no swamp; **14 never by default**),
   `sources = r > 0.5 ? 1 : 2`, `controller = true`, `keeperLairs = false`.
7. `_genTerrain` (12.5–12.10) `:463`; 8. GEN-EMIT `:465-512`; 9. GEN-MINERAL `:514-554`; 10. GEN-INSERT `:556-560`;
11. GEN-POST `:562-564`; returns `'OK'`.

### 12.2 GEN-EXIT-MATCH
`_matchExitWithNeighbors(exits, dir, neighbor)` `reference/backend/lib/cli/map.js:81-113`; the neighbour edge is
`top`→neighbour row y=49, `right`→column x=0, `bottom`→row y=0, `left`→column x=49; `_exitsArray` lists indexes
0..49 whose tile is not wall (mask 1) `:32-40`.
* `exits[dir]` given: if neighbour exists it must have the same length and full intersection, else reject
  `` `Exits in room ${neighbor.room} don't match` ``; no neighbour → accepted.
* not given: neighbour → copy neighbour's open indexes; no neighbour → `_genExit()`.
Neighbour terrain codes 2/3 count as open/closed by the wall bit only.

### 12.3 GEN-EXIT-RANDOM
`_genExit()` `reference/backend/lib/cli/map.js:42-79`:
```
L  = floor(r*43)+1                      // 1..43
ic = [0,0,1,1,2][floor(r*5)]            // gap count
s  = floor(r*(46-L))+2                  // start 2..47-L
cur = s
for j in 0..ic-1:
    cur += floor(r*(L/(ic*2))) + 5
    len  = floor(r*(L/(ic*2))) + 5
    if len + cur >= s + L - 5: len = s + L - cur - 5
    gap[j] = {start: cur, length: len};  cur += len + 1
exit = [ t for t in s..s+L (inclusive) if not in an active gap (length>0, start<=t<=start+length) and 2<=t<=47 ]
```
Gaps with `length <= 0` are ignored. An exit side always yields 1..44 tiles within 2..47.

### 12.4 GEN-TERRAIN-TABLES
Wall types `{fill, smooth, factor}` `reference/backend/lib/cli/map.js:204-236`:

| t | fill | smooth | factor | | t | fill | smooth | factor |
|---|---|---|---|---|---|---|---|---|
| 1 | 0.4 | 10 | 5 | | 15 | 0.5 | 3 | 5 |
| 2 | 0.2 | 20 | 4 | | 16 | 0.45 | 4 | 5 |
| 3 | 0.2 | 20 | 4 | | 17 | 0.45 | 6 | 5 |
| 4 | 0.3 | 18 | 4 | | 18 | 0.45 | 10 | 5 |
| 5 | 0.3 | 10 | 4 | | 19 | 0.5 | 10 | 5 |
| 6 | 0.3 | 10 | 4 | | 20 | 0.4 | 3 | 5 |
| 7 | 0.3 | 10 | 4 | | 21 | 0.5 | 2 | 5 |
| 8 | 0.35 | 15 | 4 | | 22 | 0.45 | 4 | 5 |
| 9 | 0.3 | 2 | 4 | | 23 | 0.45 | 6 | 5 |
| 10 | 0.35 | 2 | 4 | | 24 | 0.45 | 10 | 5 |
| 11 | 0.35 | 5 | 4 | | 25 | 0.5 | 10 | 5 |
| 12 | 0.35 | 5 | 4 | | 26 | 0.45 | 10 | 5 |
| 13 | 0.25 | 5 | 4 | | 27 | 0.45 | 6 | 5 |
| 14 | 0.4 | 3 | 5 | | 28 | 0.2 | 20 | 4 |

Swamp types `:238-254`: 1 (0.3,3,5), 2 (0.35,3,5), 3 (0.45,3,5), 4 (0.25,1,5), 5 (0.25,30,4), 6 (0.52,30,5),
7 (0.45,3,5), 8 (0.3,1,5), 9 (0.3,1,4), 10 (0.3,3,5), 11 (0.3,3,5), 12 (0.3,1,5), 13 (0.25,1,5), 14 (0.35,3,5).

### 12.5 GEN-TERRAIN-FILL
`_genTerrain(wallType, swampType, exits, sources, controller, keeperLair)` `reference/backend/lib/cli/map.js:202-415`.
Outer loop `do { … } while(!_checkFlood(terrain))` `:258-309`:
1. `tries++`; if `tries > 100`: `wallType = floor(r*27)+1`, `tries = 0` `:260-265`.
2. fresh 50×50 grid of empty cells; then row-major `y` 0..49, `x` 0..49 `:267-303`:
   * `y==0 && top∋x` → cell `forceOpen, exit`, and cell `(x,1)` `forceOpen`; skip randomization.
   * `y==49 && bottom∋x` → same with `(x,48)`; `x==0 && left∋y` → `(1,y)`; `x==49 && right∋y` → `(48,y)`.
     (checked in this order; first match wins).
   * else `wall = r < fill[wallType]`; `swamp = swampType ? r < fill[swampType] : false` (**no Math.random call when
     swampType is 0**).
3. apply `_smoothTerrain(…, factor, 'wall')` `smooth[wallType]` times `:305-307`.
After a passing flood check: if `swampType`, apply `_smoothTerrain(…, swampFactor, 'swamp')` `swampSmooth` times
`:311-315`.

### 12.6 GEN-SMOOTH
`_smoothTerrain(terrain, factor, key)` `reference/backend/lib/cli/map.js:171-200` (double-buffered; each new cell is a
shallow clone, so `forceOpen`/`exit`/other flags carry over):
* `cnt` = number of cells in the 3×3 block **including the cell itself** with `key` truthy; for `key=='wall'`
  out-of-bounds positions count as walls; for swamp they don't.
* `new[key] = cnt >= factor`.
* for `wall` only: border cells (x or y ∈ {0,49}) forced `wall=true`, then `forceOpen` forces `wall=false`.
Consequences: exit tiles and the tile inward of each are always open; all other border tiles are walls; swamp
smoothing may put swamp on exit tiles.

### 12.7 GEN-FLOOD
`_checkFlood` `reference/backend/lib/cli/map.js:115-169`: all non-wall cells (`!cell.wall`, so exit cells whose
`wall` is undefined count as open) must form **one 8-connected component** (diagonal steps allowed, also between
two walls). The start-cell search has a truthiness quirk (`if(startX && startY) break`, so a start at x=0 or y=0
keeps scanning) that does not change the result as long as an open cell exists; with no open cell the JS crashes.
No randomness.

### 12.8 GEN-SOURCES
For `i in 0..sources-1` `reference/backend/lib/cli/map.js:317-390`:
```
tries = 0
do { tries++
     x = floor(r*44)+3; y = floor(r*44)+3          // 3..46
     passNearby = any in-bounds cell of the 3x3 (incl. self) is !wall
     if tries > 1000: return _genTerrain(floor(r*27)+1, swampType, exits, sources, controller, keeperLair)
} while(!cell.wall || !passNearby)
cell.source = true
if keeperLair: GEN-KEEPER-LAIRS
```
Restarts regenerate the whole terrain with a new random wall type (RNG continues). **GOTCHA**: no check that the
tile is not already a source (two sources may coincide → one source object) or a previously placed lair.

### 12.9 GEN-KEEPER-LAIRS
Per source (only when `keeperLairs`) `reference/backend/lib/cli/map.js:350-389`:
```
visited = {"sx,sy": 0}; list = [[sx,sy]]; spots = []
do { [cx,cy] = list.pop()                                   // LIFO: DFS-like order
     for dx in -1..1: for dy in -1..1:                      // dx outer, dy inner
        skip (0,0) or already visited; skip out of bounds
        d = visited[cx,cy] + 1; visited[n] = d
        if 3 <= d <= 5 and n.wall and !n.source and 1<=nx<=48 and 1<=ny<=48: spots.push(n)
        if !n.wall and d < 5: list.push(n)
} while list non-empty
if spots empty: return _genTerrain(floor(r*27)+1, ...)       // full restart
spots = _.shuffle(spots); cell(spots[0]).keeperLair = true
```
**GOTCHA**: "distance" is first-discovery depth in a LIFO traversal (not BFS distance) and walls are discovered
but not expanded. `_.shuffle` (lodash 3.10.1) = Fisher–Yates forward: for `i = 0..n-1`:
`j = i + floor(r*(n-i))`, swap(i,j) — consumes exactly `n` randoms (including the last, trivial one)
`reference/lodash-3.10.1/lodash.js:2501-2503,6928-6967`.

### 12.10 GEN-CONTROLLER
If `controller` `reference/backend/lib/cli/map.js:392-412`: repeat `x = floor(r*40)+5; y = floor(r*40)+5` (5..44)
until cell is wall, has an open 3×3 neighbour, and is not a source or lair. No retry limit.

### 12.11 GEN-EMIT
`reference/backend/lib/cli/map.js:465-512`: iterate `y` then `x` ascending:
* `wall` → terrain wall entry;
* `source` → `{room, type:'source', x, y, energy:1500, energyCapacity:1500, ticksToRegeneration:300}`;
* `controller` → `{room, type:'controller', x, y, level:0}`;
* `keeperLair` → `{room, type:'keeperLair', x, y}`; `sourceKeepers = true`;
* `swamp` → swamp entry **only if** some in-bounds cell of the 3×3 (incl. self) is not wall.
Then `encodeTerrain` → string. A wall tile with a kept swamp flag becomes code **3** (TERRAIN-CODE3).
Object order (and thus insertion/id order) = tile order, per tile source → controller → lair.

### 12.12 GEN-MINERAL
`reference/backend/lib/cli/map.js:514-554`:
* default type: `['H'×6,'O'×6,'Z'×3,'K'×3,'U'×3,'L'×3,'X'×1][floor(r*25)]`; `opts.mineral === false` ⇒ none.
* position loop (no retry limit): `mx = 4+floor(r*42)`, `my = 4+floor(r*42)` (4..45); accept when the tile is wall
  (from the encoded string), some 3×3 cell is non-wall, and **no source or controller** with `|dx|<5 && |dy|<5`.
  **GOTCHA**: keeper lairs are not checked, so a mineral can be placed on a lair tile.
* density: `v = r`; first `d` in `"1","2","3","4"` with `v <= MINERAL_DENSITY_PROBABILITY[d]` (0.1/0.5/0.9/1.0) ⇒
  `density = +d`, `mineralAmount = MINERAL_DENSITY[density]`.
* object `{type:'mineral', mineralType, density, mineralAmount, x, y, room}` appended after GEN-EMIT objects.
No extractor is created.

### 12.13 GEN-INSERT
In parallel `reference/backend/lib/cli/map.js:556-560`: `rooms` `{_id, status:'normal', sourceKeepers}` (no `bus`,
`depositType`, `active`, `openTime`); `rooms.terrain` `{room, terrain}` (no `type`, see COL-ROOMS-TERRAIN);
`rooms.objects` array insert (ids generated in array order). The room is **not** added to `activeRooms`.

### 12.14 GEN-RNG-ORDER
For reproducibility with a seeded RNG, Math.random is consumed in this order:
1. `_genExit` for each side lacking both an explicit exit array and a neighbour, sides in order top, right, bottom,
   left: 3 calls (length, gap count, start) + 2 per gap (offset, length).
2. defaults: terrainType, swampType, sources (only those undefined).
3. terrain attempts: per attempt (after the optional `wallType` re-roll on the 101st), row-major for each non-exit
   cell: 1 wall call (+1 swamp call if swampType ≠ 0). Smoothing and flood: none.
4. sources: 2 per try; on >1000 tries one re-roll then full recursion; lairs: n shuffle calls (or recursion).
5. controller: 2 per try. 6. mineral type (if undefined), 2 per position try, 1 density.
7. `genId`: 2 per inserted doc (storage process).
chitin's world generator need not be bit-identical to JS (no bot can observe the generator), but should implement the
same distributions and constraints; keep a seedable RNG for reproducible large worlds.

### 12.15 GEN-POST
`updateRoomImageAssets(roomName)` then `updateTerrainData()` `reference/backend/lib/cli/map.js:562-564`.
Not done: no `bus/depositType`, no extractor in SK rooms, no `mapView`, no activation, no accessible-room refresh
(happens within 20 ticks), and running runners/processors keep their cached terrain (ENV-TERRAIN-DATA GOTCHA).

### 12.16 GEN-REMOVE-ROOM
`map.removeRoom(room)` deletes the room doc, its objects, flags, terrain, intents, `market.stats` rows with
`{room}`, `mapView:<room>`; then `updateTerrainData()` and unlinks the PNG `reference/backend/lib/cli/map.js:589-609`.

### 12.17 GEN-DEFAULT-WORLD
`storage/db.original.json` (env `databaseVersion 4.1`) and `launcher/init_dist/db.json` (v7; copied by `screeps init`
`reference/launcher/lib/init.js:48-66`) both contain an 11×11 world `W0..W10 × N0..N10` (121 rooms):
* 40 bus rooms (`x` or `y` number is 0 or 10), `bus:true`, `depositType` per SECTOR-DEPOSIT-TYPE (31 rooms have it in
  `db.original.json`, all 40 in the launcher DB), `powerBankTime` on all 40 in `db.original.json` and on none in the
  launcher DB;
* 8 SK rooms (W4–W6 × N4–N6 minus W5N5): 3 sources (`energyCapacity` 4000), 4 lairs (`nextSpawnTime: 303`),
  1 mineral, 1 **unowned extractor**, `sourceKeepers:true`;
* 1 center room W5N5: 3 sources, 1 mineral, 1 unowned extractor, no controller;
* 72 controller rooms: 42 with 2 sources, 30 with 1, each with a mineral (densities 1:6, 2:32, 3:34, 4:9 over all 81
  minerals; fractional `mineralAmount`s);
* users: Invader `'2'`, Source Keeper `'3'`, system `Screeps`, 4 `simplebot` bots owning W9N9, W9N1, W1N9, W1N1
  (controller level 1, `safeMode: 20001`, one spawn each, `invaderGoal: 1000000`), their `users.code` and
  `memory:<id> = "{}"`;
* env: `gameTime:1`, `accessibleRooms`, `terrainData` (143 rooms), `mapView:` per room, `mainLoopPaused:'0'`.
* rooms still carry the legacy `active` boolean (true for the 4 bot rooms), converted into `activeRooms` by the v9
  migration on first load.
* Differences: `db.original.json` has a deposit in W3N0 and W3N0 `active:true`; the launcher DB has the v7 Invader
  badge and no `users.power_creeps` collection.

### 12.18 GEN-BOT-SPAWN
CLI `bots.spawn(botAiName, roomName, opts)` `reference/backend/lib/cli/bots.js:48-130`:
controller must exist and be unowned; username `opts.username` or random `<Name>Bot` from boy/girl lists
(`r>0.5` picks list, then index; after 4 collisions two names are concatenated) `:15-35`; user doc
`{username, usernameLower, cpu: opts.cpu||100, gcl: opts.gcl ? GCL_MULTIPLY*(gcl-1)^GCL_POW : 0, cpuAvailable:0,
registeredDate, bot, active:10000, badge: random}`; `users.code` `{user, modules, branch:'default', activeWorld:true,
activeSim:true}`; `memory:<id> = "{}"`; spawn at `opts.x || floor(3+r*46)` (**3..48**, re-rolled while wall; `x=0`
falls back to random) named `Spawn1`, `notifyWhenAttacked:false`; controller `$set {user, level:1, progress:0,
downgradeTime:null, safeMode: gameTime+20000}`; room `invaderGoal:1000000`; activate room.

### 12.19 GEN-PLACE-SPAWN
HTTP `place-spawn` `reference/backend/lib/game/api/game.js:328-410`: validation (0..49, name ≤ 50, not blocked,
`cpu` truthy, 180 s since `lastRespawnDate`, room open, user owns no objects, controller unowned/unreserved/not bound
to another user, construction-spot rules); removes foreign creeps/sites in the room, converts other players'
structures to ruins (`decayTime +100000`, owner kept) and deletes them; controller (only if `level:0`) `$set {user,
level:1, progress:0, downgradeTime:null, safeMode: gameTime+20000}`; sources `invaderHarvested:0`; spawn insert
(`store.energy 300`, `notifyWhenAttacked:true`); room `invaderGoal:1000000`; activate; user `active:10000`.

### 12.20 GEN-RESPAWN
`respawnUser(userId)` `reference/backend/lib/utils.js:115-168`: delete the user's creeps, power creeps (room and
`users.power_creeps`) and construction sites; structures → ruins owned by the system user (`decayTime +500000`);
reassign remaining ruins to the system user; delete all user objects except controllers; delete flags; controllers
`$set {level:0, hits:0, hitsMax:0, progress:0, progressTotal:0, user:null, downgradeTime:null, safeMode:null,
safeModeAvailable:0, safeModeCooldown:null, isPowerEnabled:false}`; user `rooms: []`.
**GOTCHA**: the loop meant to reset room status iterates a Promise (missing `await`) and does nothing `:147-150`.

### 12.21 GEN-USER-REGISTER
See USER-CREATE-PATHS.

---

## 13. Cron jobs that create world objects

### 13.1 CRON-SCHEDULER
`config.cronjobs = {name: [intervalSeconds, fn]}`; `run()` executes each job whose last run is older than its
interval (wall clock) `reference/backend/lib/cronjobs.js:14-36`: sendNotifications 60 s, roomsForceUpdate 20 s,
genPowerBanks 300 s, genInvaders 300 s, purgeTransactions 3600 s, recreateNpcOrders 300 s, calcMarketStats 3600 s,
deletePowerCreeps 600 s, genDeposits 300 s, genStrongholds 300 s, expandStrongholds 900 s. **GOTCHA**: wall-clock
scheduled, so at high tick rates world events become rarer per tick; chitin should offer a tick-based mode (and the
1:1 wall-clock mode).

### 13.2 CRON-ROOMS-FORCE-UPDATE
For rooms not `out of borders` and not currently in `activeRooms`: if `!nextForceUpdateTime || gameTime >=
nextForceUpdateTime` → activate and set `nextForceUpdateTime = gameTime + 90 + floor(r*20)`
`reference/backend/lib/cronjobs.js:178-192`.

### 13.3 CRON-POWER-BANKS
`reference/backend/lib/cronjobs.js:194-243`: for rooms `{bus:true, status:'normal'}`:
`respawnTime = round(r*POWER_BANK_RESPAWN_TIME/2 + POWER_BANK_RESPAWN_TIME*0.75)` (computed every visit); first
visit only sets `powerBankTime`. When due: position loop `x = floor(r*40+5)`, `y = floor(r*40+5)` until wall with a
non-wall 3×3 neighbour (no retry limit, objects not checked); `power = floor(r*(MAX-MIN)+MIN)`, `+MAX` with
probability `POWER_BANK_CAPACITY_CRIT`; insert OBJ-POWERBANK; update `powerBankTime`; activate.

### 13.4 CRON-DEPOSITS
`reference/backend/lib/cronjobs.js:556-643`: for center rooms (regex GOTCHA 10.5) not out of borders: sector edge
rooms = the 40 distinct highway rooms framing the sector (4 lists of 11, `_.unique`d) `:557-571`; existing deposits
counted if `|Δ| < 250` on both axes in world tile coordinates (`50*roomXY + xy`, center tile `24 + 50*centerXY`); `throughput = Σ 20/max(1, DEPOSIT_EXHAUST_MULTIPLY*harvested^DEPOSIT_EXHAUST_POW)`;
skip if `>= 2.5`. Candidate rooms: edge rooms `{bus:true, status:'normal', depositType exists}` not already holding a
counted deposit; `_.sample` one; position loop (max 1000) `x,y = floor(r*40+5)`: wall, non-wall 3×3 neighbour,
within the 250-tile sector box, no object within Chebyshev 2; insert OBJ-DEPOSIT (`decayTime = gameTime +
DEPOSIT_DECAY_TIME`); activate. Several early `return`s abort the whole job (no candidate rooms, missing terrain).

### 13.5 CRON-INVADERS
`reference/backend/lib/cronjobs.js:245-441`: for `normal` rooms not active this tick: skip if invader creeps exist;
`goal = room.invaderGoal || INVADERS_ENERGY_GOAL`; skip unless `goal == 1` or `Σ sources.invaderHarvested >= goal`;
skip unless the sector has an `invaderCore` with `level > 0`; exits = sides with any non-wall border tile among
indexes 0..48, minus sides whose neighbour controller is owned/reserved; random side; raid composition
(`createRaid`, small/big by controller level, boost chances, counts) and creep docs (OBJ-CREEP-INVADER); new
`invaderGoal = floor(INVADERS_ENERGY_GOAL*(r*0.6+0.7))`, with 10% chance `*= floor(r>0.5 ? 2 : 0.5)` (**`floor(0.5)=0`**
so it becomes 0 half of those times); sources `invaderHarvested:0`; activate.

### 13.6 CRON-STRONGHOLD-GEN
`reference/backend/lib/strongholds.js:225-236,130-161,14-128`: for each sector center room (not out of borders)
without an invader core or invader-core ruin in its sector: choose a random sector room that is not bus, is center
(`isCenter`), not very center, not a highway by name, and has no owned/reserved controller nor claim creeps; spawn a
random template at a random fitting spot (x,y in 4..44, all template tiles non-wall and object-free); objects:
core + ramparts (`hits 1`, `isPublic`, `nextDecayTime = deployTime`), controller (if any) set to user `'2'`, level 8,
invulnerable until `deployTime = gameTime + 5000`; core `depositType = _.sample(keys(coreRewards))`.

### 13.7 CRON-STRONGHOLD-EXPAND
`reference/backend/lib/strongholds.js:238-349`: cores with `deployTime:null, level>0, nextExpandTime<gameTime`:
set `nextExpandTime = gameTime + INVADER_CORE_EXPAND_TIME[level]`; BFS over grid exits (shuffled order) skipping
highway names and novice/respawn rooms, passing through rooms without controller or with a core; first room with an
unowned controller and no core receives a level-0 core at `findFreePos` within ±5 of the controller (500 tries;
rect 1..48) `reference/backend/lib/utils.js:319-359`.

### 13.8 CRON-NPC-ORDERS
For unowned terminals in normal rooms whose `nextNpcMarketOrder` has passed: replace that room's orders with a sell
order (mineral from a weighted list, prices H/O 3000, Z/K/U/L 6000, X 18000 millis, amount 200 000), 50% an energy
sell order (200 000 @1000), 25% a buy order (100 000 @1000); `nextNpcMarketOrder = gameTime +
round(5000*(0.8+0.4r))` `reference/backend/lib/cronjobs.js:38-122`.

### 13.9 CRON-DELETE-POWER-CREEPS
Power creeps with `deleteTime < now` are removed and the user's `power` reduced by an interpolated level refund
`reference/backend/lib/cronjobs.js:525-554`.

### 13.10 CRON-HOUSEKEEPING
`sendNotifications` (emits `sendUserNotifications`, deletes sent rows, honours `notifyPrefs`) `:124-176`;
`purgeTransactions` keeps the newest 100 per sender and per recipient `:443-470`; `calcMarketStats` (COL-MARKET-STATS).

---

## 14. Constants

### 14.1 CONST-PORT-RULES
`reference/common/lib/constants.js` = two `Object.assign(exports, …)` blocks: scalars/tables `:1-851`, then tables
that reference earlier exports (`POWER_INFO`, `BODYPARTS_ALL`, `RESOURCES_ALL`, `COLORS_ALL`,
`INTERSHARD_RESOURCES`, `COMMODITIES`) `:853-1591`. 423 keys: 258 numbers, 137 strings, 5 arrays, 21 objects, 2 nulls.
* Port as an **ordered** map from name to a JSON-like value (keys must enumerate in source order for
  `for…in`/`Object.keys` parity; numeric-like object keys enumerate ascending in JS, e.g. `MINERAL_DENSITY_PROBABILITY`
  iterated `1,2,3,4` by the generator/mineral regen).
* The engine/processor/runtime all use the *same mutable object* `config.common.constants` (`driver.constants`)
  `reference/common/lib/config-manager.js:4-9`, `reference/driver/lib/index.js:593`; mods mutate it at load time
  (`reference/launcher/init_dist/example-mods/change-constant.js`). chitin: constants are mod-overridable at startup
  and then immutable.
* Runtime sandboxes receive a structured copy (`_constants`) `reference/driver/lib/runtime/user-vm.js:65`,
  `reference/driver/lib/runtime/runtime.js:6`.
* Computed values: `MARKET_ORDER_LIFE_TIME = 1000*60*60*24*30` (2 592 000 000 ms), `PORTAL_UNSTABLE = 10*24*3600*1000`,
  `PORTAL_MIN_TIMEOUT = 12*…`, `PORTAL_MAX_TIMEOUT = 22*…`, `POWER_CREEP_SPAWN_COOLDOWN = 8*3600*1000`,
  `POWER_CREEP_DELETE_COOLDOWN = 24*3600*1000` (ms, wall clock); `POWER_INFO` keys are the numeric `PWR_*` values.
* Non-integer numbers: `CREEP_CORPSE_RATE 0.2, REPAIR_COST 0.01, DISMANTLE_COST 0.005, SPAWN_RENEW_RATIO 1.2,
  LINK_LOSS_RATIO 0.03, TOWER_FALLOFF 0.75, POWER_BANK_CAPACITY_CRIT 0.3, POWER_BANK_HIT_BACK 0.5, GCL_POW 2.4,
  MINERAL_DENSITY_CHANGE 0.05, DEPOSIT_EXHAUST_MULTIPLY 0.001, DEPOSIT_EXHAUST_POW 1.2, TERMINAL_SEND_COST 0.1,
  MARKET_FEE 0.05`; plus floats inside tables (`BOOSTS.tough.*.damage` 0.7/0.5/0.3, boost multipliers 1.5/1.8,
  `POWER_INFO.*.effect` arrays like 0.9/0.7/…). All arithmetic on them must use f64 with JS rounding semantics.
* Sparse tables (missing keys are `undefined`, not 0): `RAMPART_HITS_MAX` has only 2..8; `CONTROLLER_LEVELS` 1..7;
  `CONTROLLER_DOWNGRADE` 1..8; `CONTROLLER_STRUCTURES` lacks key `0` for link, constructedWall, rampart, storage,
  tower, observer, powerSpawn, extractor, terminal, lab, nuker, factory (code compares `count < undefined` ⇒ false);
  `NUKE_DAMAGE` has keys 0 and 2 only; `INVADER_CORE_EXPAND_TIME` 1..5. Model as `Option`.
* Aliases: `ERR_NOT_ENOUGH_ENERGY = ERR_NOT_ENOUGH_RESOURCES = ERR_NOT_ENOUGH_EXTENSIONS = -6`; there is no -13.
* `null` values: `MODE_SIMULATION`, `MODE_WORLD`.

### 14.2 CONST-EXPOSURE
Each tick the runtime does `_.extend(globals, JSON.parse(JSON.stringify(C)))` `reference/engine/src/game/game.js:435`:
every constant is a fresh deep copy in player globals (players may mutate their copy within a tick; changes do not
leak to the engine; numeric object keys become strings — no observable change in JS). chitin's JS engine must expose
constants as plain data properties with these exact names/values (bots iterate `RESOURCES_ALL`, `REACTIONS`, etc.).

### 14.3 CONST-SYSTEM-INTENT-SCHEMA
`reference/common/lib/system.js:4-140`: per-intent field whitelist with transforms
`string ("" + v)`, `number (parseInt(v))`, `boolean (!!v)`, `price (parseInt((1000*v).toFixed(0)))`,
`string[]`, `number[] (parseInt each)`, `bodypart[] (filter ∈ BODYPARTS_ALL)`, `userString (substring 0..100)`,
`userText (substring 0..1000)`. Only listed fields survive; intent names not in the table (or custom types) are
dropped; falsy intent values are dropped. **GOTCHA**: `number` is `parseInt`, so `amount: 12.7` becomes 12, `'5e3'`
becomes 5, non-numeric becomes `NaN` (stored as `null` after JSON). Intent table: 65 entries `:16-82`.

### 14.4 Scalar constants by category (line ranges in `reference/common/lib/constants.js`)
| ID | Contents | Lines |
|---|---|---|
| CONST-RETURN-CODES | `OK 0` + 17 `ERR_*` (−1..−16, with three −6 aliases, no −13) | 2-19 |
| CONST-FIND | `FIND_EXIT_TOP/RIGHT/BOTTOM/LEFT = 1/3/5/7`, `FIND_EXIT 10`, `FIND_* 101..123` (28 keys) | 21-48 |
| CONST-DIRECTIONS | `TOP 1` … `TOP_LEFT 8` | 50-57 |
| CONST-COLORS | `COLOR_RED 1` … `COLOR_WHITE 10` | 59-68 |
| CONST-LOOK | 14 `LOOK_*` strings (`LOOK_RESOURCES = "resource"`, `LOOK_ENERGY = "energy"`) | 70-83 |
| CONST-OBSTACLES | `OBSTACLE_OBJECT_TYPES` (20 type strings; not road/rampart/container/portal) | 85 |
| CONST-BODYPARTS | 8 part names; `BODYPART_COST` | 87-105 |
| CONST-CREEP-ACTIONS | `WORLD_WIDTH/HEIGHT 202` (deprecated), `CREEP_LIFE_TIME 1500`, `CREEP_CLAIM_LIFE_TIME 600`, corpse rate, `CARRY_CAPACITY`, `*_POWER`, `REPAIR_COST`, `DISMANTLE_COST` | 107-129 |
| CONST-STRUCTURE-STATS | rampart, `ENERGY_REGEN_TIME`, `ENERGY_DECAY`, spawn, source capacities, wall, extension, road, link, storage, tower, observer, power bank, power spawn, extractor, lab, terminal, container, nuker/nuke, factory, tombstone, ruin, portal decay | 131-168, 246-283, 333-367 |
| CONST-STRUCTURE-TYPES | 21 `STRUCTURE_*` (`STRUCTURE_WALL = "constructedWall"`) | 170-190 |
| CONST-CONSTRUCTION-COST | `CONSTRUCTION_COST` (16 buildable types), `*_ROAD_SWAMP_RATIO 5`, `*_ROAD_WALL_RATIO 150` | 192-211 |
| CONST-CONTROLLER | `CONTROLLER_LEVELS`, `CONTROLLER_STRUCTURES`, `CONTROLLER_DOWNGRADE*`, reserve/attack/nuke blocks, `SAFE_MODE_*`, `GCL_*` | 213-244, 284-286 |
| CONST-MINERALS-DEPOSITS | `TERRAIN_MASK_*`, `MAX_CONSTRUCTION_SITES 100`, `MAX_CREEP_SIZE 50`, `MINERAL_*`, `DENSITY_*`, `DEPOSIT_*` | 291-331 |
| CONST-MARKET | `ORDER_*`, `MARKET_FEE`, `MARKET_MAX_ORDERS 300`, `MARKET_ORDER_LIFE_TIME`, `FLAGS_LIMIT`, intershard names, `PIXEL_CPU_COST` | 369-384 |
| CONST-RESOURCES | 84 `RESOURCE_*` (energy, power, 8 base minerals, 4 deposit raws, compounds, ops, commodities) | 386-482 |
| CONST-NPC | `PORTAL_*` timeouts, `POWER_BANK_RESPAWN_TIME`, `INVADERS_ENERGY_GOAL`, `SYSTEM_USERNAME`, `SIGN_*` (3 identical strings), `INVADER_CORE_*`, `STRONGHOLD_*` | 770-783, 842-850 |
| CONST-EVENTS | `EVENT_*` 1..12, attack types 1..6, heal types 1..2 | 785-806 |
| CONST-POWER | `POWER_LEVEL_*`, `POWER_CREEP_*`, `POWER_CLASS`, `PWR_* 1..19`, `EFFECT_INVULNERABILITY 1001`, `EFFECT_COLLAPSE_TIMER 1002` | 808-840 |

### 14.5 Notable tables
* CONST-REACTIONS `:484-615` — symmetric map `REACTIONS[a][b] = product` (31 first-level keys, 68 pairs).
* CONST-BOOSTS `:617-731` — `BOOSTS[part][compound] = {action: multiplier}` (30 compounds; tough uses `damage`).
* CONST-REACTION-TIME `:733-768` — 34 products.
* CONST-POWER-INFO `:854-1016` — 19 powers; fields `className, level[5], cooldown, duration (number|array),
  range?, ops (number|array)?, effect[]?, period?, energy?` — PWR_SHIELD uses `energy` instead of `ops`;
  PWR_DISRUPT_TERMINAL has array `ops`; PWR_REGEN_* have `period`.
* CONST-COMMODITIES `:1144-1590` — 45 recipes `{level?, amount, cooldown, components}` including reverse recipes
  (`U ← utrium_bar`, `energy ← battery`).
* CONST-LISTS `:1018-1143` — `BODYPARTS_ALL` (8), `RESOURCES_ALL` (84, in the listed order), `COLORS_ALL` (10),
  `INTERSHARD_RESOURCES` (4).
* CONTROLLER_STRUCTURES, EXTENSION_ENERGY_CAPACITY, RAMPART_HITS_MAX, CONTROLLER_LEVELS, CONTROLLER_DOWNGRADE,
  NUKE_DAMAGE, MINERAL_DENSITY(_PROBABILITY), RUIN_DECAY_STRUCTURES, INVADER_CORE_CREEP_SPAWN_TIME,
  INVADER_CORE_EXPAND_TIME, STRONGHOLD_RAMPART_HITS — see 14.1 for sparseness.

### 14.6 CONST-UNUSED
Exposed to players but not referenced by OSS server code (verified by grep of engine/driver/backend):
`MINERAL_MIN_AMOUNT`, `MINERAL_RANDOM_FACTOR`, `TERRAIN_MASK_LAVA`, `PORTAL_UNSTABLE`, `PORTAL_MIN_TIMEOUT`,
`PORTAL_MAX_TIMEOUT`, `LAB_COOLDOWN` ("not used" `:279`), `PIXEL_CPU_COST`, `WORLD_WIDTH/HEIGHT`,
`TERMINAL_SEND_COST`, `TERMINAL_MIN_SEND`, `MODE_SIMULATION/MODE_WORLD`. Keep them for bot compatibility.

### 14.7 CONST-STRONGHOLD-TABLES
`reference/common/lib/strongholds.js`: `templates` bunker1..bunker5 (`description, rewardLevel, structures:[{type, dx,
dy, level?, strongholdBehavior?}]`) `:3-246`; `coreRewards` (deposit type → 6 commodities), `coreAmounts`,
`coreDensities`, `containerRewards`, `containerAmounts` `:248-272`. Exposed as `driver.strongholds`
`reference/driver/lib/index.js:595`. Port wholesale (stronghold spec covers semantics).

### 14.8 CONST-GEN-TABLES
Constants that live in code, not constants.js: terrain/swamp type tables (12.4), mineral weight list (12.12),
`_genExit` parameters (12.3), bot name lists `reference/backend/lib/cli/bots.js:12-13`, invader bodies
`reference/backend/lib/cronjobs.js:266-273`, NPC order prices (13.8), stronghold deploy time 5000
`reference/backend/lib/strongholds.js:12`, `historyChunkSize 20`, `cpuMaxPerTick 500`, `cpuBucketSize 10000`,
`mainLoopMinDuration 1000`, `mainLoopResetInterval 5000` `reference/driver/lib/index.js:19-33`.

---

## 15. PERF summary (data-model level)

* Room-local hot state: `rooms.objects` of one room, its terrain, its flags, its intents, its `rooms` doc. Everything
  the room processor needs; all writes are buffered and committed at the end of the room (STORE-BULK-WRITER).
* Cross-room/global state: `users` (gcl/power/money/rooms updated from many rooms via `$inc`/`$addToSet`, which
  commute), market orders, transactions, power creeps, inter-room moves (global step reads all `interRoom` objects),
  `accessibleRooms`/`roomStatusData` (computed every 20 ticks from all rooms), terrain blob (all rooms).
* Original slow spots: whole-DB JSON autosave; single env doc; RPC JSON per query; per-tick full re-read of room
  objects for each websocket subscriber; `getInterRoom` loading all rooms each tick; linear `_.contains` on the
  accessible list; `encodeTerrain` O(n²); whole-world re-sort of both `rooms.objects` indices after every write
  (2.3).
* Iteration order (2.18–2.29) is reproducible without Loki: a global creation sequence per object (room-local
  ordered lists, per-user ordered sets), a creation sequence per room doc, and two small adaptive-order lists
  (console commands, power creeps) per user. None of it needs cross-room coordination at tick time except the
  runtime's own-object list, whose same-tick cross-room ties are free (2.19).
* Only *active* rooms are processed (ENV-ACTIVE-ROOMS); chitin's scheduler should preserve which rooms are processed
  each tick if bot-observable effects depend on it (Open questions), otherwise it can process all rooms in parallel.

---

## 16. Open questions

1. Room activation parity: several regenerations (`sources/tick`, `minerals/tick`, decay of roads/ramparts/containers)
   only run when a room is processed; idle rooms are processed every ~90–109 ticks (wall clock + RNG). E.g. a source
   in an unvisited room regenerates when `gameTime >= nextRegenerationTime-1` *at its next processing*, possibly late.
   Must chitin reproduce the active-room schedule exactly, or process every room every tick (deviation visible to
   observers/remote miners)? Needs a decision by the processing-order spec.
2. Does the official web client rely on `meta`/`$loki` in room diffs (WIRE-ROOM-DIFF)? Not observed in backend code.
3. World size beyond W127..E127: RoomPosition packing (9.5) and pathfinder (9.6) cap the official API at 255×255
   rooms. Options: keep the cap (still far larger than official shards ~W60..E60) or extend `__packedPos` (breaks bots
   that decode it). Decision needed.
4. `novice`, `respawnArea`, `hardSign`, `bindUser`, `tutorial`, `newbieWall`, `promoPeriodUntil`, portal creation:
   read but never written by OSS code; the official server's writers are not in `reference/`. chitin should support
   the fields as data; generation of novice/respawn areas and portals is unspecified here.
5. `calcWorldSize` with `maxX = maxY = 0` initial values (9.7) changes `getWorldSize()` and the continuous distance
   for worlds not containing the origin; bots may hard-code results. Replicate exactly (recommended) — confirm.
6. Bulk-writer data loss (STORE-BULK-WRITER pending-insert/counter-reuse GOTCHAs): replicate (strict 1:1, objects can
   vanish in rare same-tick create/remove sequences) or fix? The processor specs should list the concrete code paths
   that can trigger it before deciding.
