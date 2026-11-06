# 05 — Player API: Structures, Store, Game.market

Scope: every `Structure*` class exposed to player code (properties + methods), `StructureSpawn.Spawning`, the `Store` class, and `Game.market`; plus the server-side handling of every intent those methods emit (room processor and global stage), because 1:1 behaviour is only observable through the processor result.

Sources (all read-only, cited as `reference/<pkg>/<path>:<line>`):
engine @8097782, driver @cf63d8a, common @2fb779b, storage @3d6c3d0, backend 3.3.0, docs (`docs/api/source/*.md`), lodash 3.10.1, xxscreeps @d1e3bad (comparison only).

Conventions used below
- **API** = validation done inside the player sandbox (returns a code synchronously). **PROC** = what the server does with the intent later in the same tick.
- `raw(o)` = the raw DB document behind a wrapper (`data(id)` in `reference/engine/src/game/structures.js:9-14`). `time` = `Game.time` = processor `gameTime` of the same tick.
- Return codes (`reference/common/lib/constants.js:2-19`): `OK 0`, `ERR_NOT_OWNER -1`, `ERR_NAME_EXISTS -3`, `ERR_BUSY -4`, `ERR_NOT_FOUND -5`, `ERR_NOT_ENOUGH_ENERGY = ERR_NOT_ENOUGH_RESOURCES -6`, `ERR_INVALID_TARGET -7`, `ERR_FULL -8`, `ERR_NOT_IN_RANGE -9`, `ERR_INVALID_ARGS -10`, `ERR_TIRED -11`, `ERR_RCL_NOT_ENOUGH -14`.
- "Validation order" lists are exhaustive and ordered: the first failing check returns. A step that "throws" means a JS exception escapes to player code.
- Locality tags: **R** room-local, **U** per-player but cross-room (needs the player's whole runtime view), **G** global server stage / cross-room write.

## 0. Tick pipeline (only what this area depends on)

1. All player runners execute (API calls, intents collected) — `reference/engine/src/main.js:32-41`.
2. Intents are sanitized and routed per room / `global` — `reference/driver/lib/runtime/make.js:192`, `reference/engine/src/utils.js:405-436`.
3. Every room is processed independently — `reference/engine/src/main.js:42-56`, `reference/engine/src/processor.js:16-520`.
4. Global stage: terminal `send` execution, all market intents, power-creep global intents — `reference/engine/src/main.js:62-66`, `reference/engine/src/processor/global.js:9-64`.
5. `gameTime++`; then, if the new `gameTime > lastAccessibleRoomsUpdate + 20` (module variable of the engine_main process, starts at `0` on process start, set to `gameTime` after each refresh — so every 21 ticks), `accessibleRooms` and `roomStatusData` are rebuilt fire-and-forget (not awaited) — `reference/engine/src/main.js:8,72-83`. See RUNTIME-ROOM-STATUS-DATA for the fresh-world gap this leaves.

**PERF**: everything in this spec is room-local (step 3) except: terminal sends/deals and all market state (step 4, single serial global stage), `launchNuke` (inserts an object into another room, visible next tick), `observeRoom` (changes next tick's runtime visibility), `activateSafeMode` ERR_BUSY check and `createOrder`/`deal` terminal lookups (scan the player's whole object set in the runtime).

---

## 1. Inventory

| ID | Item | Kind | Loc | Primary source |
|---|---|---|---|---|
| COMMON-INTENT-SET | `intents.set` one-per-(object,name), overwrite, 0.2 CPU | rule | U | reference/driver/lib/runtime/runtime.js:63-72 |
| COMMON-INTENT-PUSH | `pushByName` lists with maxLen, 0.2 CPU each; `remove` refund | rule | U | reference/driver/lib/runtime/runtime.js:82-99 |
| COMMON-INTENT-SANITIZE | per-intent field whitelist + type transforms | rule | U | reference/common/lib/system.js:4-114 |
| COMMON-INTENT-ROUTING | object intents → object's room; `room` → by roomName; `global` | rule | U | reference/engine/src/utils.js:405-436 |
| COMMON-PROC-DISPATCH | processor per-object dispatch + owner filter + mod hooks | rule | R | reference/engine/src/processor.js:229-321 |
| COMMON-ASSERT-TARGET | serialized-object detection throw | rule | U | reference/engine/src/game/game.js:122-126 |
| COMMON-GETTER-CACHE | lazy getters cached in `_name`, `toJSON` | rule | R | reference/engine/src/utils.js:509-545 |
| COMMON-WRAPPERS | wrapper construction, `Game.structures`, `Game.spawns`, `room.controller/storage/terminal` | rule | U | reference/engine/src/game/game.js:260-328 |
| COMMON-RCL-CHECK | `checkStructureAgainstController` (active rank by distance) | rule | R | reference/engine/src/utils.js:456-507 |
| COMMON-OFF-FLAG | `off` flag for spawns/extensions (`_calc_spawns`) | proc | R | reference/engine/src/processor/intents/_calc_spawns.js:6-41 |
| COMMON-EFFECT-TIMING | `effects[].ticksRemaining` vs raw `endTime` comparisons | rule | R | reference/engine/src/game/rooms.js:1648-1659 |
| COMMON-ROOM-DISTANCE | `roomNameToXY`, `calcRoomsDistance` (wrap), world size | rule | — | reference/engine/src/utils.js:374-392,644-655 |
| COMMON-JS-COERCION | validations rely on JS loose semantics | rule | — | (throughout) |
| COMMON-MOD-HOOKS | processor events / custom intent types | rule | R | reference/engine/src/processor.js:169,246,318 |
| STRUCT-PROPS | `hits`, `hitsMax`, `structureType` (+ RoomObject `id/pos/room/effects`) | prop | R | reference/engine/src/game/structures.js:62-66 |
| STRUCT-TOSTRING | `toString()` | method | — | reference/engine/src/game/structures.js:68-70 |
| STRUCT-DESTROY | `destroy()` | method | R | reference/engine/src/game/structures.js:72-87 |
| PROC-DESTROY-STRUCTURE | `destroyStructure` room intent | proc | R | reference/engine/src/processor/intents/room/destroy-structure.js:6-26 |
| STRUCT-NOTIFY | `notifyWhenAttacked(enabled)` | method | R | reference/engine/src/game/structures.js:89-106 |
| PROC-NOTIFY | generic `notifyWhenAttacked` handling | proc | R | reference/engine/src/processor.js:313-315 |
| STRUCT-ISACTIVE | `isActive()` | method | R | reference/engine/src/game/structures.js:108-119 |
| OWNED-OWNER | `owner` | prop | R | reference/engine/src/game/structures.js:136-138 |
| OWNED-MY | `my` | prop | R | reference/engine/src/game/structures.js:139 |
| STORE-CTOR | `Store` construction, Proxy, enumeration | class | R | reference/engine/src/game/store.js:19-97 |
| STORE-CAPACITY-MODEL | `capacityForResource`, per-structure capacity fields | rule | R | reference/engine/src/utils.js:671-675 |
| STORE-GETCAPACITY | `getCapacity([r])` | method | R | reference/engine/src/game/store.js:28-35 |
| STORE-GETUSED | `getUsedCapacity([r])` | method | R | reference/engine/src/game/store.js:36-51 |
| STORE-GETFREE | `getFreeCapacity([r])` | method | R | reference/engine/src/game/store.js:52-79 |
| STORE-TOSTRING | `toString()` → `[store]` | method | — | reference/engine/src/game/store.js:80-84 |
| CONTAINER-PROPS | `store`, `storeCapacity`, `ticksToDecay` | prop | R | reference/engine/src/game/structures.js:155-159 |
| CONTROLLER-PROPS | all controller getters | prop | R | reference/engine/src/game/structures.js:174-199 |
| CONTROLLER-UNCLAIM | `unclaim()` | method | R | reference/engine/src/game/structures.js:201-209 |
| PROC-CONTROLLER-UNCLAIM | `unclaim` processing | proc | R | reference/engine/src/processor/intents/controllers/unclaim.js:6-28 |
| CONTROLLER-SAFEMODE | `activateSafeMode()` | method | U | reference/engine/src/game/structures.js:211-234 |
| PROC-CONTROLLER-SAFEMODE | `activateSafeMode` processing + controller tick apply | proc | R | reference/engine/src/processor/intents/controllers/activateSafeMode.js:6-25 |
| EXTENSION-PROPS | `energy`, `energyCapacity`, `store` | prop | R | reference/engine/src/game/structures.js:249-253 |
| PROC-EXTENSION-CAPACITY | extension capacity from RCL | proc | R | reference/engine/src/processor/intents/extensions/tick.js:6-20 |
| EXTRACTOR-PROPS | `cooldown` | prop | R | reference/engine/src/game/structures.js:268-270 |
| FACTORY-PROPS | `level`, `store`, `storeCapacity`, `cooldown` | prop | R | reference/engine/src/game/structures.js:1427-1432 |
| FACTORY-PRODUCE | `produce(resourceType)` | method | R | reference/engine/src/game/structures.js:1434-1470 |
| PROC-FACTORY-PRODUCE | `produce` processing | proc | R | reference/engine/src/processor/intents/factories/produce.js:6-44 |
| PROC-FACTORY-CAPACITY | factory `storeCapacity` from RCL | proc | R | reference/engine/src/processor/intents/factories/tick.js:6-19 |
| INVCORE-PROPS | `level`, `spawning`, `ticksToDeploy`, `toString` | prop | R | reference/engine/src/game/structures.js:1480-1488 |
| LAIR-PROPS | `my`, `owner`, `ticksToSpawn` | prop | R | reference/engine/src/game/structures.js:285-289 |
| LAB-PROPS | `energy`, `energyCapacity`, `cooldown`, `mineralAmount`, `mineralCapacity`, `mineralType`, `store` | prop | R | reference/engine/src/game/structures.js:304-315 |
| LAB-STORE-MODEL | lab `storeCapacity`/`storeCapacityResource` transitions | rule | R | reference/engine/src/processor/intents/labs/run-reaction.js:53-86 |
| LAB-RUN-REACTION | `runReaction(lab1, lab2)` | method | R | reference/engine/src/game/structures.js:317-358 |
| LAB-REVERSE-REACTION | `reverseReaction(lab1, lab2)` | method | R | reference/engine/src/game/structures.js:360-410 |
| LAB-BOOST-CREEP | `boostCreep(creep, [bodyPartsCount])` | method | R | reference/engine/src/game/structures.js:412-441 |
| LAB-UNBOOST-CREEP | `unboostCreep(creep)` | method | R | reference/engine/src/game/structures.js:443-466 |
| PROC-LAB-ORDER | lab intent dispatch order | proc | R | reference/engine/src/processor/intents/labs/intents.js:2-14 |
| PROC-LAB-RUN | `runReaction` processing | proc | R | reference/engine/src/processor/intents/labs/run-reaction.js:6-89 |
| PROC-LAB-REVERSE | `reverseReaction` processing | proc | R | reference/engine/src/processor/intents/labs/reverse-reaction.js:6-91 |
| PROC-LAB-BOOST | `boostCreep` processing | proc | R | reference/engine/src/processor/intents/labs/boost-creep.js:6-62 |
| PROC-LAB-UNBOOST | `unboostCreep` processing | proc | R | reference/engine/src/processor/intents/labs/unboost-creep.js:6-53 |
| LINK-PROPS | `energy`, `energyCapacity`, `cooldown`, `store` | prop | R | reference/engine/src/game/structures.js:481-486 |
| LINK-TRANSFER | `transferEnergy(target, [amount])` | method | R | reference/engine/src/game/structures.js:488-533 |
| PROC-LINK-TRANSFER | link `transfer` processing | proc | R | reference/engine/src/processor/intents/links/transfer.js:6-54 |
| PROC-LINK-COOLDOWN | link cooldown countdown | proc | R | reference/engine/src/processor/intents/links/tick.js:7-31 |
| NUKER-PROPS | `energy`, `energyCapacity`, `ghodium`, `ghodiumCapacity`, `cooldown`, `store` | prop | R | reference/engine/src/game/structures.js:1347-1354 |
| NUKER-LAUNCH | `launchNuke(pos)` | method | R | reference/engine/src/game/structures.js:1356-1388 |
| PROC-NUKER-LAUNCH | `launchNuke` processing (cross-room insert) | proc | G | reference/engine/src/processor/intents/nukers/launch-nuke.js:8-51 |
| RUNTIME-ROOM-STATUS-DATA | `roomStatusData` build, 60 s runner cache, fresh-world absence (affects `launchNuke`, `Game.map.getRoomStatus`), `Infinity`→`null` | rule | G | reference/driver/lib/index.js:498-524; reference/driver/lib/runtime/data.js:18-21,88-97 |
| OBSERVER-OBSERVE | `observeRoom(roomName)` | method | R | reference/engine/src/game/structures.js:548-570 |
| PROC-OBSERVER | observe intent → next-tick visibility | proc | G | reference/engine/src/processor.js:116-118,284-288,403-406 |
| PBANK-PROPS | `power`, `ticksToDecay`, `my`, `owner` | prop | R | reference/engine/src/game/structures.js:585-590 |
| PSPAWN-PROPS | `energy`, `energyCapacity`, `power`, `powerCapacity`, `store` | prop | R | reference/engine/src/game/structures.js:605-611 |
| PSPAWN-PROCESS | `processPower()` | method | R | reference/engine/src/game/structures.js:613-631 |
| PROC-PSPAWN-PROCESS | `processPower` processing | proc | R | reference/engine/src/processor/intents/power-spawns/process-power.js:7-39 |
| PORTAL-PROPS | `destination`, `ticksToDecay` | prop | R | reference/engine/src/game/structures.js:1404-1417 |
| RAMPART-PROPS | `ticksToDecay`, `isPublic` | prop | R | reference/engine/src/game/structures.js:646-649 |
| RAMPART-SETPUBLIC | `setPublic(isPublic)` | method | R | reference/engine/src/game/structures.js:651-657 |
| PROC-RAMPART-SETPUBLIC | `setPublic` processing | proc | R | reference/engine/src/processor/intents/ramparts/set-public.js:6-11 |
| ROAD-PROPS | `ticksToDecay` | prop | R | reference/engine/src/game/structures.js:672-674 |
| SPAWN-PROPS | `name`, `energy`, `energyCapacity`, `spawning`, `store` | prop | R | reference/engine/src/game/structures.js:857-863 |
| SPAWN-MEMORY | `memory` get/set | prop | U | reference/engine/src/game/structures.js:865-891 |
| SPAWN-TOSTRING | `toString()` | method | — | reference/engine/src/game/structures.js:893-895 |
| SPAWN-GLOBAL-ALIAS | `Spawn === StructureSpawn` | rule | — | reference/engine/src/game/structures.js:1292-1293 |
| SPAWN-ROOM-ENERGY | `Room.energyAvailable/energyCapacityAvailable` used by spawning | rule | R | reference/engine/src/game/game.js:325-328 |
| SPAWN-COST | `calcCreepCost(body)` | rule | — | reference/engine/src/utils.js:114-126 |
| SPAWN-NAME-RULES | creep name validity / uniqueness | rule | U | reference/engine/src/game/structures.js:1065-1075 |
| SPAWN-SPAWNCREEP | `spawnCreep(body, name, [opts])` | method | U | reference/engine/src/game/structures.js:1063-1218 |
| SPAWN-OPT-MEMORY | `opts.memory` | option | U | reference/engine/src/game/structures.js:1124-1130 |
| SPAWN-OPT-ENERGYSTRUCTURES | `opts.energyStructures` | option | R | reference/engine/src/game/structures.js:1053-1061,1077,1113 |
| SPAWN-OPT-DRYRUN | `opts.dryRun` | option | — | reference/engine/src/game/structures.js:1118-1120 |
| SPAWN-OPT-DIRECTIONS | `opts.directions` | option | R | reference/engine/src/game/structures.js:1079-1090 |
| SPAWN-STUB-CREEP | placeholder `Game.creeps[name]` object | rule | U | reference/engine/src/game/structures.js:1132-1213 |
| SPAWN-CANCREATECREEP | `canCreateCreep(body, [name])` (deprecated) | method | U | reference/engine/src/game/structures.js:897-931 |
| SPAWN-CREATECREEP | `createCreep(body, [name], [memory])` (deprecated) | method | U | reference/engine/src/game/structures.js:933-1051 |
| SPAWN-UNIQUE-NAME | random creep name generator | rule | U | reference/engine/src/game/names.js:4-23 |
| SPAWN-NOTIFY | spawn override of `notifyWhenAttacked` | method | R | reference/engine/src/game/structures.js:1220-1235 |
| SPAWN-RENEW | `renewCreep(target)` | method | R | reference/engine/src/game/structures.js:1237-1267 |
| SPAWN-RECYCLE | `recycleCreep(target)` | method | R | reference/engine/src/game/structures.js:1269-1290 |
| PROC-SPAWN-ORDER | spawn intent dispatch order | proc | R | reference/engine/src/processor/intents/spawns/intents.js:1-17 |
| PROC-SPAWN-CREATE | `createCreep` processing | proc | R | reference/engine/src/processor/intents/spawns/create-creep.js:7-101 |
| PROC-SPAWN-CHARGE | energy withdrawal order (old/new handling) | proc | R | reference/engine/src/processor/intents/spawns/_charge-energy.js:6-78 |
| PROC-SPAWN-RENEW | `renewCreep` processing | proc | R | reference/engine/src/processor/intents/spawns/renew-creep.js:6-55 |
| PROC-SPAWN-RECYCLE | `recycleCreep` processing (drop formula) | proc | R | reference/engine/src/processor/intents/spawns/recycle-creep.js:6-23 |
| PROC-SPAWN-SET-DIRECTIONS | `setSpawnDirections` processing | proc | R | reference/engine/src/processor/intents/spawns/set-spawn-directions.js:6-21 |
| PROC-SPAWN-CANCEL | `cancelSpawning` processing | proc | R | reference/engine/src/processor/intents/spawns/cancel-spawning.js:6-13 |
| PROC-SPAWN-TICK | birth, spawn-stomp, energy regen | proc | R | reference/engine/src/processor/intents/spawns/tick.js:6-49 |
| SPAWNING-PROPS | `name`, `needTime`, `remainingTime`, `directions`, `spawn` | prop | R | reference/engine/src/game/structures.js:1301-1310 |
| SPAWNING-SETDIRECTIONS | `Spawning.setDirections(directions)` | method | R | reference/engine/src/game/structures.js:1312-1326 |
| SPAWNING-CANCEL | `Spawning.cancel()` | method | R | reference/engine/src/game/structures.js:1328-1334 |
| STORAGE-PROPS | `store`, `storeCapacity` | prop | R | reference/engine/src/game/structures.js:690-693 |
| PROC-STORAGE-CAPACITY | storage capacity from RCL + PWR_OPERATE_STORAGE | proc | R | reference/engine/src/processor/intents/storages/tick.js:6-19 |
| TERMINAL-PROPS | `store`, `storeCapacity`, `cooldown` | prop | R | reference/engine/src/game/structures.js:708-712 |
| TERMINAL-SEND | `send(resourceType, amount, destination, [description])` | method | R | reference/engine/src/game/structures.js:714-745 |
| PROC-TERMINAL-SEND-ROOM | room-stage validation, stores `send` on terminal | proc | R | reference/engine/src/processor/intents/terminal/send.js:6-33 |
| PROC-TERMINAL-SEND-GLOBAL | global-stage execution of sends | proc | G | reference/engine/src/processor/global-intents/market.js:71-95 |
| PROC-TERMINAL-CAPACITY | terminal capacity from RCL | proc | R | reference/engine/src/processor/intents/terminal/tick.js:6-15 |
| TOWER-PROPS | `energy`, `energyCapacity`, `store` | prop | R | reference/engine/src/game/structures.js:760-764 |
| TOWER-ATTACK | `attack(target)` | method | R | reference/engine/src/game/structures.js:766-784 |
| TOWER-HEAL | `heal(target)` | method | R | reference/engine/src/game/structures.js:786-804 |
| TOWER-REPAIR | `repair(target)` | method | R | reference/engine/src/game/structures.js:806-824 |
| PROC-TOWER-PRIORITY | heal > repair > attack, one action/tick | proc | R | reference/engine/src/processor/intents/towers/intents.js:2-10 |
| PROC-TOWER-FALLOFF | range falloff + power multipliers | proc | R | reference/engine/src/processor/intents/towers/attack.js:32-46 |
| PROC-TOWER-ATTACK | tower attack processing | proc | R | reference/engine/src/processor/intents/towers/attack.js:6-70 |
| PROC-TOWER-HEAL | tower heal processing | proc | R | reference/engine/src/processor/intents/towers/heal.js:6-53 |
| PROC-TOWER-REPAIR | tower repair processing | proc | R | reference/engine/src/processor/intents/towers/repair.js:6-56 |
| WALL-PROPS | `ticksToLive` (undocumented) | prop | R | reference/engine/src/game/structures.js:839-841 |
| MARKET-UNITS | credits vs milli-credits representation | rule | G | reference/common/lib/system.js:8 |
| MARKET-DATA-SOURCES | what runtime data the market API sees | rule | G | reference/driver/lib/runtime/data.js:31-75,138-139,243-247 |
| MARKET-CREDITS | `Game.market.credits` | prop | U | reference/engine/src/game/market.js:196-201 |
| MARKET-INCOMING-TX | `Game.market.incomingTransactions` | prop | U | reference/engine/src/game/market.js:203-217 |
| MARKET-OUTGOING-TX | `Game.market.outgoingTransactions` | prop | U | reference/engine/src/game/market.js:219-233 |
| MARKET-ORDERS | `Game.market.orders` | prop | U | reference/engine/src/game/market.js:235-249 |
| MARKET-CALC-COST | `calcTransactionCost(amount, r1, r2)` | method | — | reference/engine/src/game/market.js:31-34 |
| MARKET-GET-ALL-ORDERS | `getAllOrders([filter])` | method | G | reference/engine/src/game/market.js:13-39 |
| MARKET-GET-HISTORY | `getHistory([resourceType])` | method | G | reference/engine/src/game/market.js:41-53 |
| MARKET-GET-ORDER-BY-ID | `getOrderById(id)` | method | G | reference/engine/src/game/market.js:55-66 |
| MARKET-CREATE-ORDER | `createOrder(params)` / positional form | method | U | reference/engine/src/game/market.js:68-98 |
| MARKET-CANCEL-ORDER | `cancelOrder(orderId)` | method | U | reference/engine/src/game/market.js:100-106 |
| MARKET-DEAL | `deal(orderId, amount, [yourRoomName])` | method | U | reference/engine/src/game/market.js:108-153 |
| MARKET-CHANGE-PRICE | `changeOrderPrice(orderId, newPrice)` | method | U | reference/engine/src/game/market.js:155-172 |
| MARKET-EXTEND-ORDER | `extendOrder(orderId, addAmount)` | method | U | reference/engine/src/game/market.js:174-191 |
| PROC-MARKET-STAGE-ORDER | global-stage ordering of market work | proc | G | reference/engine/src/processor/global-intents/market.js:71-589 |
| PROC-MARKET-EXECUTE-TRANSFER | shared terminal transfer routine | proc | G | reference/engine/src/processor/global-intents/market.js:15-69 |
| PROC-MARKET-CREATE | `createOrder` processing | proc | G | reference/engine/src/processor/global-intents/market.js:109-166 |
| PROC-MARKET-CHANGE-PRICE | `changeOrderPrice` processing | proc | G | reference/engine/src/processor/global-intents/market.js:168-213 |
| PROC-MARKET-EXTEND | `extendOrder` processing | proc | G | reference/engine/src/processor/global-intents/market.js:215-253 |
| PROC-MARKET-CANCEL | `cancelOrder` processing | proc | G | reference/engine/src/processor/global-intents/market.js:256-263,502-505 |
| PROC-MARKET-DEAL-TERMINAL | terminal deals (sorted by distance) | proc | G | reference/engine/src/processor/global-intents/market.js:265-398 |
| PROC-MARKET-DEAL-DIRECT | intershard (account-resource) deals | proc | G | reference/engine/src/processor/global-intents/market.js:400-495 |
| PROC-MARKET-EXPIRY | 30-day order expiry with fee refund | proc | G | reference/engine/src/processor/global-intents/market.js:507-534 |
| PROC-MARKET-MAINTENANCE | order activation / `amount` recompute | proc | G | reference/engine/src/processor/global-intents/market.js:536-588 |
| MARKET-HISTORY-STATS | hourly `market.stats` computation | cron | G | reference/backend/lib/cronjobs.js:472-523 |
| MARKET-TX-PURGE | hourly transaction purge (keep 100/sender, 100/recipient) | cron | G | reference/backend/lib/cronjobs.js:443-470 |
| MARKET-NPC-ORDERS | private-server NPC terminal orders | cron | G | reference/backend/lib/cronjobs.js:38-122 |

Inventory count: 148 items.

---

## 2. Common mechanics

### COMMON-INTENT-SET
`intents.set(objectId, name, payload)` — `reference/driver/lib/runtime/runtime.js:66-72`.
- Stores `list[objectId][name] = payload`. A second call with the same `(objectId, name)` in the same tick **overwrites** the first; only the last payload reaches the processor.
- CPU: adds `intentCpu = 0.2` to the tick's used CPU only the first time a `(objectId, name)` pair is set (`!this.list[id][name]`), so overwrites are free (`reference/driver/lib/runtime/runtime.js:60,68-70`). `say`/`pull` are free methods (irrelevant here).
- **GOTCHA**: different intent names on the same object coexist (e.g. tower `attack` + `heal` + `repair`, lab `runReaction` + `boostCreep`), each costs 0.2 CPU, and the processor decides which ones run (see PROC-TOWER-PRIORITY, PROC-LAB-ORDER).
- **GOTCHA**: all structure methods return `OK` when the intent is *scheduled*; most processor rejections are silent. The runtime never mutates `raw()` state, so every API check in one tick sees the start-of-tick state (e.g. energy is not "reserved" by an earlier `spawnCreep` in the same tick).

### COMMON-INTENT-PUSH
`intents.pushByName(id, name, payload, maxLen)` — `reference/driver/lib/runtime/runtime.js:82-91`: appends to `list[id][name]` (array). If `maxLen` and length `>= maxLen`, returns `false` (nothing stored, no CPU); else pushes, +0.2 CPU, returns `true`. `intents.remove(id, name)` deletes the whole entry and refunds 0.2 CPU (`:92-99`). Used with `id = 'room'` (`destroyStructure`) and `id = 'global'` (market).

### COMMON-INTENT-SANITIZE
Before storage every intent payload is rebuilt from a per-name field whitelist with transforms — `reference/common/lib/system.js:4-14` (transforms), `:16-82` (types), `:84-114`:
- `string`: `"" + v` (so `undefined` → `"undefined"`); `number`: `parseInt(v)` (floats truncate, `undefined` → `NaN`); `boolean`: `!!v`; `price`: `parseInt((1000*v).toFixed(0))` (rounds to milli-credits); `string[]`/`number[]`: map if array else `undefined`; `bodypart[]`: filter to `BODYPARTS_ALL`; `userString`: `("" + (v||"")).substring(0,100)`.
- Fields not in the whitelist are dropped; unknown intent names are dropped (unless registered as mod `customIntentTypes`).
- Relevant shapes: `createCreep {name:userString, body:bodypart[], energyStructures:string[], directions:number[]}`, `transfer {id, amount:number, resourceType}`, `send {targetRoomName, resourceType, amount:number, description:userString}`, `boostCreep {id, bodyPartsCount:number}`, `launchNuke {x:number, y:number, roomName}`, `attack/heal/repair {id, x:number, y:number}` (x/y become `NaN` for towers, unused), `produce {resourceType, amount:number}` (amount `NaN`, unused), `createOrder {type, resourceType, price:price, totalAmount:number, roomName:string}`, `changeOrderPrice {orderId, newPrice:price}`, `deal {orderId, amount:number, targetRoomName:string}`.
- Conformance vectors: `reference/engine/spec/engine/utilsSpec.js:120-455`.
- **GOTCHA**: The processor therefore sees *integers* for amounts even when the API validated a float (e.g. `terminal.send(R, 0.5, …)` passes API, becomes `amount 0`, silently dropped by `!intent.amount`).

### COMMON-INTENT-ROUTING
`reference/engine/src/utils.js:405-436`: keys `notify`/`room`/`global` are special; any other key is an object id, looked up in `userObjects` then `roomObjects`; the intents go to `intents[object.room][id]`. Unknown ids are dropped. `room` intents are grouped by their sanitized `roomName` field (`reference/common/lib/system.js:116-140`). `global` goes to the `users.intents` collection (`reference/driver/lib/index.js:229-232`).

### COMMON-PROC-DISPATCH
`reference/engine/src/processor.js:229-321`. For each user in the room's intent document, for each object id in insertion order:
- `room` pseudo-object → room intents (`destroyStructure` etc.).
- Skip if object missing, `_skip`, or `object.user && object.user != userId` (**ownerless objects — roads, walls, containers — accept intents from anyone**, which matters only for `notifyWhenAttacked`).
- Per type: creep, powerCreep, link, tower, lab, spawn handlers; then rampart `setPublic`, terminal `send`, nuker `launchNuke`, observer `observeRoom`, powerSpawn, invaderCore, factory `produce`, controller `unclaim` then `activateSafeMode`, then generic `notifyWhenAttacked` (PROC-NOTIFY).
- Order across objects = order of the first `intents.set` per object during the runner tick (JS object insertion order survives JSON/DB). Order across users = order of keys in the room intent document (runner completion order) → **GOTCHA**: cross-user ordering in one room is not deterministic in the original; chitin must pick a deterministic order (recommend user id order) and document it.
- In-memory mutation: `bulk.update(obj, data)` deep-merges `data` into the in-memory object and `bulk.inc` increments it (`reference/driver/lib/bulk.js:33-63,90-100`), so later handlers in the same tick see earlier effects.

### COMMON-ASSERT-TARGET
`register.assertTargetObject(obj)` — `reference/engine/src/game/game.js:122-126`: throws `Error("It seems you're trying to use a serialized game object stored in Memory …")` iff `obj` is a plain object (`_.isPlainObject`) with a string `id` of length exactly 24. Called just before returning `ERR_INVALID_TARGET` in target validations.
- **GOTCHA**: private-server LokiJS ids are 15 hex chars (`reference/storage/lib/db.js:317-328`), so this never fires there; the official MMO uses 24-char Mongo ObjectIds so it does. Chitin should use 24-hex-char ids to match MMO bots' expectations.

### COMMON-GETTER-CACHE
`utils.defineGameObjectProperties(proto, data, props)` — `reference/engine/src/utils.js:509-545`: each property is an enumerable prototype getter that computes `props[name](raw(this.id), this.id)` and caches the result in an own enumerable property `this['_'+name]` **only if the value is truthy** (falsy results are recomputed every access). `toJSON` iterates `for…in` (own + inherited enumerable) skipping keys starting with `_` and `toJSON`/`toString`.
- **GOTCHA**: after reading e.g. `s.store`, `Object.keys(s)` contains `"_store"`; JSON output omits it. Getters return the same object on repeated access within the tick (`s.store === s.store`, `spawn.spawning === spawn.spawning`).
- Wrappers are rebuilt every tick; old wrappers kept in `global` read the *current* tick's raw data through the module-level `runtimeData` (`reference/engine/src/game/structures.js:20-25`) but keep stale cached `_x` values.

### COMMON-WRAPPERS
`reference/engine/src/game/game.js:229-328`:
- Type → class map: `rampart, road, extension, constructedWall→StructureWall, keeperLair, controller, link, storage, tower, observer, powerBank, powerSpawn, lab, extractor, terminal, container, spawn, nuker, portal, factory, invaderCore` (`:229-251`).
- `Game.structures[id]`: only `OwnedStructure` instances that are the player's own objects (`:302-307`), including inactive ones. Never containers/roads/walls (they extend `Structure`).
- `Game.spawns[name]`: own spawns keyed by `name` (`:313-318`). `Game.structures`, `Game.spawns`, `Game.creeps` are plain `{}` objects (inherit `Object.prototype`; see SPAWN-NAME-RULES).
- The `Structure` constructor sets `room.controller`, `room.storage`, `room.terminal` to the wrapper of that type found in the room (last one wins) (`reference/engine/src/game/structures.js:45-55`).
- Classes that extend `Structure` directly: Container, Road, Wall, Portal. All others extend `OwnedStructure` (incl. Controller, KeeperLair, PowerBank, InvaderCore).

### COMMON-RCL-CHECK (`checkStructureAgainstController`)
`reference/engine/src/utils.js:456-507`. Inputs: raw object `s`, the room's raw objects (iteration order matters), raw controller `c`.
```
if !s.user → true
if !c || c.level < 1 || c.user !== s.user → false
allowed = CONTROLLER_STRUCTURES[s.type][c.level]
if allowed == 0 → false
if CONTROLLER_STRUCTURES[s.type][8] === 1 → true          // storage, observer, powerSpawn, extractor, terminal, nuker, factory
d = cheb(s, c); foundSelf = false
for o in roomObjects (iteration order):
   if o.type != s.type || o.user != s.user: continue
   od = cheb(o, c)
   if od < d:            allowed--; if allowed == 0 → false
   elif !foundSelf && od == d:
        if o is s: foundSelf = true
        else: allowed--; if allowed == 0 → false
→ true
```
`cheb` = Chebyshev distance `max(|dx|,|dy|)` to the controller tile (not path length).
Equivalent closed form (use this for PERF): `rank(s)` = number of same-type same-owner structures strictly closer + those at equal distance that precede `s` in iteration order; active ⇔ `rank(s) < CONTROLLER_STRUCTURES[type][level]`. Precompute per `(room, user, type)` once per tick by a stable sort on distance.
- **GOTCHA**: only the structure owner's objects are counted; construction sites are not counted; enemy structures do not consume the allowance (contrast COMMON-OFF-FLAG).
- **GOTCHA**: types whose RCL-8 limit is 1 short-circuit: if two terminals exist (e.g. via map editing) both are active.
- **GOTCHA**: tie-break = room-object iteration order. Runtime iterates `register.objectsByRoom[room]` built from `runtimeData.roomObjects` (`reference/engine/src/game/game.js:184-194`; foreign objects first, then own objects in `find({user})` order — `reference/driver/lib/runtime/data.js:209`); the processor iterates `find({room})` order. Only own objects are compared, so the relative order of the player's objects decides; see Open questions.
- Throws (TypeError) when the caller dereferences `this.room.controller.id` in a room with no controller (all lab/tower/observer/powerSpawn/terminal/nuker/factory API methods do this; `transferEnergy` guards it).
- **PERF**: O(objects in room) per call; called by most structure methods every tick and by player `isActive()` loops → O(n²) per room in the original. Cache per tick.
- Not a 1:1 model: xxscreeps' `checkActiveStructures` also counts other users' structures in the rank (`reference/xxscreeps/packages/xxscreeps/mods/structure/structure.ts:182-208`) — do not copy.

### COMMON-OFF-FLAG (spawns & extensions)
`reference/engine/src/processor/intents/_calc_spawns.js:6-41`, invoked at the start of room processing before any intent (`reference/engine/src/processor.js:221-223`) when the room has any spawn or extension.
- `spawns` = **all** spawns in the room regardless of owner; `N = CONTROLLER_STRUCTURES.spawn[controller.level|0]` (controller owner is not checked; unowned controller → level 0 → N=0 → all off).
- If `count > N`: stable-sort by Chebyshev distance to the controller (`reference/engine/src/utils.js:394-403`), first N are on, rest `off = true`; else all `off = false`. Same for extensions with `CONTROLLER_STRUCTURES.extension`.
- Writes `off` only when changed. The runtime reads `off` next tick.
- Consumers of `off`: `spawnCreep`/`canCreateCreep`/`renewCreep`/`recycleCreep` (ERR_RCL_NOT_ENOUGH), `Room.energyAvailable`/`energyCapacityAvailable` (SPAWN-ROOM-ENERGY), spawn energy charging (PROC-SPAWN-CHARGE), processor `energyAvailable` (`reference/engine/src/processor.js:326`), PWR_OPERATE_EXTENSION.
- **GOTCHA**: `off` ≠ `!isActive()`: (a) `off` counts enemy spawns/extensions in the room, `isActive` doesn't; (b) `off` is computed from the controller level at the *start* of the previous processing pass, so after an RCL change it lags `isActive()` by a tick; (c) `isActive()` returns false for a structure whose owner is not the controller owner, `off` may still be false. Both must be implemented separately.
- `PROC-SPAWN-CREATE` additionally re-checks `checkStructureAgainstController`, so a spawn with `off=false` but inactive by rank silently fails to spawn after returning OK.

### COMMON-EFFECT-TIMING
`RoomObject.effects` = raw `effects` mapped to `{power, effect, level, ticksRemaining: endTime - time}` filtered to `ticksRemaining > 0` (`reference/engine/src/game/rooms.js:1651-1658`). API code uses this filtered list (effect active ⇔ `endTime > time`). Processor code compares raw `endTime` with `gameTime`, inconsistently:

| Effect use | API | PROC |
|---|---|---|
| PWR_OPERATE_LAB amount | `endTime > t` | `endTime > t` |
| PWR_OPERATE_SPAWN time | — | `endTime > t` |
| PWR_OPERATE_TOWER / DISRUPT_TOWER | — | `endTime > t` |
| PWR_OPERATE_POWER | `endTime > t` | `endTime >= t` |
| PWR_OPERATE_FACTORY (ERR_BUSY) | raw `endTime >= t` | `endTime >= t` |
| PWR_OPERATE_TERMINAL cost | not applied | room stage `>=`, global stage `>` |
| PWR_OPERATE_OBSERVER range | `endTime > t` | not checked |
| PWR_DISRUPT_SPAWN | — | `endTime > t` |

**GOTCHA**: reproduce each comparison exactly; at `endTime == t` several API/PROC pairs disagree.

### COMMON-ROOM-DISTANCE
- `roomNameToXY(name)` — `reference/engine/src/utils.js:374-392`: `W n → x = -n-1`, `E n → x = n`, `N n → y = -n-1`, `S n → y = n`; parses with `parseInt` (lowercase `w`/`n` accepted). Non-string input throws (`name.substr`).
- `calcRoomsDistance(a, b, continuous)` — `:644-655`: `dx=|x2-x1|, dy=|y2-y1|`; if `continuous`: `dx = min(worldSize-dx, dx)` (same for dy); returns `max(dx,dy)`. All terminal/market costs use `continuous = true` (the world wraps for cost purposes).
- `worldSize` is computed once per process at startup: `max(maxX-minX+1, maxY-minY+1)` over all rooms, **with `maxX`/`maxY` initialised to 0, not -∞** (`reference/common/index.js:183-193`) — **GOTCHA**: a world entirely in W/N quadrants gets size +1. Runner and processor both read it (`reference/driver/lib/index.js:135-137,538-540`, `reference/driver/lib/runtime/runtime-driver.js:9,17-19`).
- `calcTerminalEnergyCost(amount, range) = ceil(amount * (1 - exp(-range/30)))` — `reference/engine/src/utils.js:657-659`. (`TERMINAL_SEND_COST` and `TERMINAL_MIN_SEND` constants are unused.)
- Observer and nuke range checks use raw `|dx|`,`|dy|` without wrap.

### COMMON-JS-COERCION
Validations are written against JS loose semantics and lodash 3; if chitin implements the API natively it must reproduce them. Examples that change return codes: string `amount` in `link.transferEnergy` (`"100" + energy` string concat in the FULL check), `undefined < 0 === false`, `undefined > x === false`, `regex.test(nonString)` coerces via `ToString`, `name.toString()` on numbers/objects, `_.isObject` true for arrays/functions/boxed primitives, `_.map(str, 'id')` maps characters, `_.sum` treats non-numeric as 0 and sums values **from last to first** (`reference/lodash-3.10.1/lodash.js:1509-1517,11809-11817`). Simplest 1:1 route: implement these API methods in chitin's JS engine using the original algorithms, or carefully port with explicit ToNumber/ToString rules.

### COMMON-MOD-HOOKS
The original processor emits `processObject` (`reference/engine/src/processor.js:169`), `preProcessObjectIntents` (`:246`), `processObjectIntents` (`:318`), `postProcessObject` (`:482`), `processRoom` (`:489`) and accepts `customIntentTypes` in the sanitizer (`reference/common/lib/system.js:95-140`). Chitin's mod API should offer equivalent points around every PROC-* item below.

---

## 3. Structure / OwnedStructure

### STRUCT-PROPS
`hits = raw.hits`, `hitsMax = raw.hitsMax`, `structureType = raw.type` (`reference/engine/src/game/structures.js:62-66`). Controller, keeper lair, portal have no `hits` (undefined). RoomObject supplies `id`, `pos`, `room`, `effects` (`reference/engine/src/game/structures.js:39-44`, `reference/engine/src/game/rooms.js:1648-1659`).

### STRUCT-TOSTRING
`[structure (${structureType}) #${id}]` (`reference/engine/src/game/structures.js:68-70`). Overridden by spawn (SPAWN-TOSTRING) and invader core (`[invaderCore #id]`, `:1486-1488`).

### STRUCT-DESTROY — `destroy()`
API (`reference/engine/src/game/structures.js:72-87`):
1. `!this.room` → `ERR_INVALID_TARGET`.
2. `!room.controller || !room.controller.my` → `ERR_NOT_OWNER`.
3. `room.find(FIND_HOSTILE_CREEPS).length > 0 || room.find(FIND_HOSTILE_POWER_CREEPS).length > 0` → `ERR_BUSY`. (`FIND_HOSTILE_CREEPS` = `!spawning && !my`, includes NPCs; `FIND_HOSTILE_POWER_CREEPS` = `!my` — `reference/engine/src/game/game.js:18,20`.)
4. `pushByName('room', 'destroyStructure', {roomName: room.name, id})` (no limit) → `OK`.

PROC-DESTROY-STRUCTURE (`reference/engine/src/processor/intents/room/destroy-structure.js:6-26`): ignore if object missing or `!CONSTRUCTION_COST[type]` (controller, keeperLair, powerBank, portal, invaderCore are indestructible this way); ignore if `controller.user != userId`; ignore unowned newbie walls (`constructedWall && decayTime && !user`); ignore if **any** creep/powerCreep of another user is in the room (**includes spawning creeps** — **GOTCHA**: API returns OK while only a hostile spawning creep is present, processor refuses). Then `_destroy` (`reference/engine/src/processor/intents/structures/_destroy.js:6-54`): a spawning spawn also removes its spawning creep; creates a ruin `{structure:{id,type,hits:0,hitsMax,user}, destroyTime, decayTime: time + (RUIN_DECAY_STRUCTURES[type] || RUIN_DECAY), store: object.store||{}}`; removes the object. Owned newbie walls also trigger clearing of all newbie walls.
- **GOTCHA**: does not check structure ownership — you can destroy hostile ramparts/structures, roads, containers in your own room. Destroy emits **no** `EVENT_OBJECT_DESTROYED` (only `_damage` does).

### STRUCT-NOTIFY — `notifyWhenAttacked(enabled)`
API (`reference/engine/src/game/structures.js:89-106`):
1. `!this.room` → `ERR_INVALID_TARGET`.
2. `this.my === false || (room.controller && room.controller.owner && !room.controller.my)` → `ERR_NOT_OWNER` (unowned structures are allowed anywhere except in rooms owned by someone else; KeeperLair/PowerBank have `my === false`).
3. `!_.isBoolean(enabled)` → `ERR_INVALID_ARGS` (boxed `Boolean` objects pass lodash `isBoolean`).
4. If `enabled != raw.notifyWhenAttacked` → `intents.set(id, 'notifyWhenAttacked', {enabled})`. → `OK` either way.

PROC-NOTIFY (`reference/engine/src/processor.js:313-315`): applied only if `CONSTRUCTION_COST[type]` (or creep/powerCreep): `notifyWhenAttacked = !!enabled`. Controller/lair/bank intents are no-ops (API still OK). Built structures start with `notifyWhenAttacked: true` (`reference/engine/src/processor/intents/creeps/build.js:116`). Notifications are sent only when the flag is truthy (`reference/engine/src/processor/intents/_damage.js:77-80`, `reference/engine/src/processor/intents/towers/attack.js:63-65`).
- **GOTCHA**: a tower hit notifies twice (once in `_damage`, once in tower attack).

### STRUCT-ISACTIVE — `isActive()`
`reference/engine/src/game/structures.js:108-119`:
1. `!this.owner` → `true` (all `Structure`-only classes, unowned controllers, ownerless owned-type objects).
2. `!CONTROLLER_STRUCTURES[raw.type]` → `true` (controller, keeperLair, powerBank, invaderCore, portal).
3. `!this.room || !this.room.controller` → `false`.
4. `checkStructureAgainstController(raw, objectsByRoom[room], raw(controller))` (COMMON-RCL-CHECK).
- **GOTCHA (big)**: which N structures are active is decided by Chebyshev distance to the controller, ties by object iteration order. Example: RCL 7 allows 3 towers; towers at distances 1,4,4,4 → the distance-1 tower plus the first two distance-4 towers in iteration order are active. After a downgrade the *farthest* structures go inactive regardless of build order.
- isActive is not used by spawn methods (they use `off`), see COMMON-OFF-FLAG.

### OWNED-OWNER / OWNED-MY
`owner = (raw.user === undefined || raw.user === null) ? undefined : {username: users[raw.user].username}`; `my = raw.user === undefined ? undefined : raw.user == me` (`reference/engine/src/game/structures.js:135-140`). Overrides: KeeperLair `my=false`, `owner={username:'Source Keeper'}`; PowerBank `my=false`, `owner={username:'Power Bank'}` (`:286-287,588-589`). InvaderCore uses the generic getter (user `'2'` → that user's username).
- **GOTCHA**: a never-claimed controller (no `user` field) has `my === undefined`; an unclaimed/downgraded one (`user: null`) has `my === false`. This changes `notifyWhenAttacked` (step 2 `my === false`) results.
- `FIND_MY_STRUCTURES` = `my === true`; `FIND_HOSTILE_STRUCTURES` = `my === false && owner` (`reference/engine/src/game/game.js:24-25`).

---

## 4. Store

### STORE-CAPACITY-MODEL
Raw objects carry `store` (resource→amount), optionally `storeCapacity` (general capacity) and/or `storeCapacityResource` (per-resource caps). `capacityForResource(o, r) = (o.storeCapacityResource && o.storeCapacityResource[r]) || max(0, (o.storeCapacity||0) - _.sum(o.storeCapacityResource))` (`reference/engine/src/utils.js:671-675`; nulls count 0).

| Structure | storeCapacity | storeCapacityResource | Source |
|---|---|---|---|
| spawn | — | `{energy: SPAWN_ENERGY_CAPACITY}` | reference/engine/src/processor/intents/creeps/build.js:119-128 |
| extension | — | `{energy: EXTENSION_ENERGY_CAPACITY[rcl]}` (0 at build, set by tick) | reference/engine/src/processor/intents/creeps/build.js:130-138; reference/engine/src/processor/intents/extensions/tick.js:10-17 |
| link | — | `{energy: LINK_CAPACITY}` | reference/engine/src/processor/intents/creeps/build.js:140-149 |
| tower | — | `{energy: TOWER_CAPACITY}` | reference/engine/src/processor/intents/creeps/build.js:196-204 |
| powerSpawn | — | `{energy: 5000, power: 100}` | reference/engine/src/processor/intents/creeps/build.js:235-243 |
| nuker | — | `{energy: NUKER_ENERGY_CAPACITY, G: NUKER_GHODIUM_CAPACITY}` | reference/engine/src/processor/intents/creeps/build.js:265-274 |
| lab (no mineral) | `LAB_ENERGY_CAPACITY + LAB_MINERAL_CAPACITY` (5000) | `{energy: 2000, <old minerals>: null}` | reference/engine/src/processor/intents/creeps/build.js:222-233 |
| lab (with mineral M) | `null` | `{energy: 2000, M: 3000}` | LAB-STORE-MODEL |
| storage | `STORAGE_CAPACITY` (+PWR_OPERATE_STORAGE) or 0 if inactive | — | reference/engine/src/processor/intents/storages/tick.js:7-18 |
| terminal | `TERMINAL_CAPACITY` or 0 if inactive | — | reference/engine/src/processor/intents/terminal/tick.js:8-13 |
| factory | `FACTORY_CAPACITY` or 0 if inactive | — | reference/engine/src/processor/intents/factories/tick.js:7-12 |
| container | `CONTAINER_CAPACITY` | — | reference/engine/src/processor/intents/creeps/build.js:255-263 |

Observer, extractor, rampart, road, wall, controller, keeper lair, portal, power bank, invader core expose **no** `store` getter (power bank exposes only `power`).

### STORE-CTOR
`new Store(rawObject)` — `reference/engine/src/game/store.js:19-97`:
- Copies every `store` entry whose value is **truthy** as an own enumerable property (zero, NaN, null entries are absent; any other value, including negatives, is present).
- Defines non-enumerable `getCapacity`, `getUsedCapacity`, `getFreeCapacity`, `toString` (STORE-TOSTRING: returns `[store]`).
- Returns a `Proxy` with only a `get` trap: own value if `!== undefined`; else `0` if the key is in `RESOURCES_ALL`; else `undefined`. No `has`/`ownKeys` traps → `'energy' in store` is false when empty, `Object.keys`/`for…in`/`JSON.stringify` list only non-zero resources (conformance: `reference/engine/spec/engine/game/storeSpec.js:13-57`).
- Lazily defines non-enumerable `_sum = _.sum(raw.store)` on first total computation.
- Structure `store` getter: `new globals.Store(raw)` cached per wrapper (`reference/engine/src/game/structures.js:16-18`).

### STORE-GETCAPACITY — `getCapacity([resource])`
`reference/engine/src/game/store.js:28-35`: if `!resource` → `storeCapacityResource ? null : (storeCapacity || null)`; else `capacityForResource(raw, resource) || null`.

### STORE-GETUSED — `getUsedCapacity([resource])`
`:36-51`: if `!resource`: if `storeCapacityResource && capacityForResource(raw, undefined) === 0` → `null`, else `_sum`. With resource: `raw.store[r] || (storeCapacityResource && capacityForResource(raw, r) === 0 ? null : 0)`.
- **GOTCHA**: lab without mineral → `getUsedCapacity()` returns the total; lab holding a mineral → `null` (`storeSpec.js:337-428`). Inactive storage (`storeCapacity 0`) still returns amounts (`storeSpec.js:208-255`).

### STORE-GETFREE — `getFreeCapacity([resource])`
`:52-79`, in order: `capacityForResource(raw, r) === 0` → `null`; `!raw.storeCapacity` → `getCapacity(r) - getUsedCapacity(r)`; `cap = getCapacity(r)`, `!cap` → `null`; `storeCapacityResource` → `cap - getUsedCapacity(r)`; else `cap - _sum`.
- **GOTCHA**: any general store with capacity 0 (inactive storage/terminal/factory, creep without CARRY) returns `null`, not 0. Tombstone/ruin (no capacity fields) → `null`.
- Full test vectors: `reference/engine/spec/engine/game/storeSpec.js:59-442`, `reference/engine/spec/engine/utilsSpec.js:20-118`.

---

## 5. Simple structure classes (properties only)

### CONTAINER-PROPS
`store` (Store), `storeCapacity = raw.storeCapacity`, `ticksToDecay = nextDecayTime ? nextDecayTime - time : decayTime ? decayTime - time : undefined` (`reference/engine/src/game/structures.js:155-159`). Unowned (`Structure`). Decay: `CONTAINER_DECAY` hits every `CONTAINER_DECAY_TIME_OWNED` (500) ticks if the room controller level > 0, else `CONTAINER_DECAY_TIME` (100); when destroyed by decay its contents are dropped (`reference/engine/src/processor/intents/containers/tick.js:10-32`).

### EXTENSION-PROPS / PROC-EXTENSION-CAPACITY
`energy = store ? store.energy : 0`, `energyCapacity = storeCapacityResource ? (storeCapacityResource.energy || 0) : 0`, `store` (`reference/engine/src/game/structures.js:249-253`). Each tick, if a controller exists, `storeCapacityResource.energy = EXTENSION_ENERGY_CAPACITY[controller.level] || 0` (owner not checked) (`reference/engine/src/processor/intents/extensions/tick.js:10-17`).

### EXTRACTOR-PROPS
`cooldown = raw.cooldown || 0` (`reference/engine/src/game/structures.js:268-270`). Countdown decremented each tick, `_cooldown` from harvest written at tick end (`reference/engine/src/processor/intents/extractors/tick.js:9-25`). No methods, no store.

### INVCORE-PROPS
`level`, `spawning = raw.spawning ? new StructureSpawn.Spawning(id) : null`, `ticksToDeploy = deployTime ? deployTime - time : undefined`, `toString() = [invaderCore #id]` (`reference/engine/src/game/structures.js:1480-1488`). `Spawning.setDirections/cancel` on it return `ERR_NOT_OWNER` (not `my`).

### LAIR-PROPS
`my = false`, `owner = {username:'Source Keeper'}`, `ticksToSpawn = nextSpawnTime ? nextSpawnTime - time : undefined` (`reference/engine/src/game/structures.js:285-289`).

### PBANK-PROPS
`power = raw.store.power`, `ticksToDecay` (same formula as container), `my = false`, `owner = {username:'Power Bank'}` (`reference/engine/src/game/structures.js:585-590`). **GOTCHA**: no `store` getter on power banks.

### PORTAL-PROPS
`destination = raw.destination.shard ? {shard, room} : new RoomPosition(x, y, room)`; `ticksToDecay = decayTime ? decayTime - time : undefined` (no `nextDecayTime` branch) (`reference/engine/src/game/structures.js:1404-1417`). Unowned.

### RAMPART-PROPS
`ticksToDecay` (container formula), `isPublic = !!raw.isPublic` (`reference/engine/src/game/structures.js:646-649`).

### ROAD-PROPS
`ticksToDecay` (container formula) (`reference/engine/src/game/structures.js:672-674`). Unowned.

### STORAGE-PROPS / PROC-STORAGE-CAPACITY
`store`, `storeCapacity = raw.storeCapacity` (`reference/engine/src/game/structures.js:690-693`). Each tick: capacity = `STORAGE_CAPACITY` if controller level > 0, `controller.user == storage.user` and `CONTROLLER_STRUCTURES.storage[level] > 0`, plus `POWER_INFO[PWR_OPERATE_STORAGE].effect[level-1]` while the effect `endTime > time`; else 0 (`reference/engine/src/processor/intents/storages/tick.js:6-19`).

### WALL-PROPS
`ticksToLive = raw.ticksToLive` (raw transient field, normally `undefined`; undocumented) (`reference/engine/src/game/structures.js:839-841`). Unowned unless a newbie wall; `isActive()` always true.

### TERMINAL-PROPS / PROC-TERMINAL-CAPACITY
`store`, `storeCapacity = raw.storeCapacity`, `cooldown = cooldownTime > time ? cooldownTime - time : 0` (`reference/engine/src/game/structures.js:708-712`). Tick: `storeCapacity = TERMINAL_CAPACITY` if `level > 0 && controller.user == terminal.user && CONTROLLER_STRUCTURES.terminal[level]` else 0 (`reference/engine/src/processor/intents/terminal/tick.js:6-15`).

### FACTORY-PROPS / PROC-FACTORY-CAPACITY
`level = raw.level` (undefined until set by PWR_OPERATE_FACTORY), `store`, `storeCapacity`, `cooldown` (cooldownTime form) (`reference/engine/src/game/structures.js:1427-1432`). Tick capacity rule as terminal with `FACTORY_CAPACITY` (`reference/engine/src/processor/intents/factories/tick.js:6-13`).

### TOWER-PROPS
`energy`, `energyCapacity`, `store` (extension formulas) (`reference/engine/src/game/structures.js:760-764`).

### LINK-PROPS
`energy`, `energyCapacity`, `cooldown = raw.cooldown || 0` (countdown field), `store` (`reference/engine/src/game/structures.js:481-486`).

### PSPAWN-PROPS
`energy = store ? store.energy||0 : 0`, `energyCapacity = storeCapacityResource ? storeCapacityResource.energy : 0`, `power = store ? store.power||0 : 0`, `powerCapacity = storeCapacityResource ? storeCapacityResource.power : 0`, `store` (`reference/engine/src/game/structures.js:605-611`).

### NUKER-PROPS
`energy`, `energyCapacity`, `ghodium = store.G||0`, `ghodiumCapacity = storeCapacityResource.G`, `cooldown` (cooldownTime form), `store` (`reference/engine/src/game/structures.js:1347-1354`). **GOTCHA**: a newly built nuker starts with `cooldownTime = time + NUKER_COOLDOWN` (100 000; 100 on PTR) (`reference/engine/src/processor/intents/creeps/build.js:272`).

### LAB-PROPS
(`reference/engine/src/game/structures.js:304-315`)
- `energy = store ? store.energy : 0`; `energyCapacity = storeCapacityResource ? storeCapacityResource.energy : 0`.
- `cooldown = cooldownTime > time ? cooldownTime - time : 0`.
- `mineralAmount = _.sum(store) - (store.energy||0)` (sum of *all* non-energy keys).
- `mineralType` = first key in `store` insertion order that is not `energy` and has a truthy amount, else `undefined`.
- `mineralCapacity = LAB_MINERAL_CAPACITY` (constant 3000, even when a different mineral is loaded).

### LAB-STORE-MODEL
Empty-of-mineral lab: `storeCapacity = 5000`, `storeCapacityResource = {energy: 2000, …nulls}` → accepts any one mineral up to 3000. When a mineral is added by a reaction: `storeCapacityResource[M] = 3000`, `storeCapacity = null` (`reference/engine/src/processor/intents/labs/run-reaction.js:53-65`). When a mineral reaches 0 (reaction input, reverse, boost): `storeCapacityResource[M] = null`, `storeCapacity = 5000` (`reference/engine/src/processor/intents/labs/run-reaction.js:67-86`, `reference/engine/src/processor/intents/labs/reverse-reaction.js:52-64`, `reference/engine/src/processor/intents/labs/boost-creep.js:48-56`). Creep transfer/withdraw maintain the same invariant (creep spec). Implement as a typed state (`LabStore { energy, mineral: Option<(Resource, u32)> }`) but expose raw-equivalent capacity answers.

### CONTROLLER-PROPS
(`reference/engine/src/game/structures.js:174-199`)
- `ticksToDowngrade = downgradeTime ? downgradeTime - time : undefined`
- `reservation = raw.reservation ? {username, ticksToEnd: endTime - time} : undefined`
- `level = raw.level`; `progress = level > 0 ? raw.progress : undefined`; `progressTotal = (level > 0 && level < 8) ? CONTROLLER_LEVELS[level] : undefined`
- `upgradeBlocked`, `safeMode`, `safeModeCooldown`: `v > time ? v - time : undefined`
- `safeModeAvailable = raw.safeModeAvailable || 0`
- `sign`: `hardSign` wins → `{username: SYSTEM_USERNAME ('Screeps'), text, time, datetime: new Date(hardSign.datetime)}`; else `sign` → `{username: users[sign.user].username, text, time, datetime: new Date(…)}`; else `undefined`.
- `isPowerEnabled = !!raw.isPowerEnabled`
- **GOTCHA**: `sign.datetime` is a fresh `Date` per wrapper; `sign` is cached per wrapper (getter cache).

---

## 6. Controller methods

### CONTROLLER-UNCLAIM — `unclaim()`
API (`reference/engine/src/game/structures.js:201-209`): `!my` → `ERR_NOT_OWNER`; `intents.set(id, 'unclaim', {})` → `OK`.
PROC-CONTROLLER-UNCLAIM (`reference/engine/src/processor/intents/controllers/unclaim.js:6-28`): requires `type == controller`, `user` and `level`; removes room from user's room list; sets `user:null, level:0, progress:0, downgradeTime:null, safeMode:null, safeModeAvailable:0, safeModeCooldown: (room novice > Date.now()) ? null : time + SAFE_MODE_COOLDOWN, isPowerEnabled:false`. Reservation, sign and all structures (still owned by the player, now inactive) are untouched. Processed before `activateSafeMode` on the same controller (which then no-ops because `user` is null in memory).

### CONTROLLER-SAFEMODE — `activateSafeMode()`
API (`reference/engine/src/game/structures.js:211-234`):
1. `!my` → `ERR_NOT_OWNER`.
2. `safeModeAvailable <= 0` → `ERR_NOT_ENOUGH_RESOURCES`.
3. `safeModeCooldown` (truthy remaining ticks) `|| upgradeBlocked > 0 || ticksToDowngrade < CONTROLLER_DOWNGRADE[level]/2 - CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD` → `ERR_TIRED`.
4. any wrapper in `register.structures` (all visible rooms) with `structureType == 'controller' && my && safeMode` → `ERR_BUSY` (includes this controller already in safe mode).
5. If an `activateSafeMode` intent was already set this tick on another (or the same) controller, `intents.remove` it (CPU refunded); remember this id (`lastActivateSafeMode`, reset each tick at `:28`).
6. `intents.set(id, 'activateSafeMode', {})` → `OK`.
- **GOTCHA**: only the *last* successful call per tick survives; earlier calls returned OK but are cancelled.

PROC-CONTROLLER-SAFEMODE (`reference/engine/src/processor/intents/controllers/activateSafeMode.js:6-25`): requires `user && level`, `safeModeAvailable > 0`, `!(safeModeCooldown >= time)`, `!(upgradeBlocked > time)`, `!(downgradeTime < time + CONTROLLER_DOWNGRADE[level]/2 - THRESHOLD)`; then marks `_safeModeActivated`. Applied in the controller tick (`reference/engine/src/processor/intents/controllers/tick.js:25-31`): unless `upgradeBlocked > time`, `safeModeAvailable -= 1`, `safeMode = time + SAFE_MODE_DURATION` (20 000), `safeModeCooldown = novice ? null : time + SAFE_MODE_COOLDOWN` (50 000).
- **GOTCHA**: at `time == safeModeCooldown` the API passes (getter yields `undefined`) but the processor rejects (`>=`). No server-side "one room in safe mode" check exists.
- Novice check uses wall-clock `roomInfo.novice > Date.now()`.

---

## 7. Lab methods

### LAB-RUN-REACTION — `runReaction(lab1, lab2)`
API (`reference/engine/src/game/structures.js:317-358`):
1. `!my` → `ERR_NOT_OWNER`.
2. `cooldown > 0` → `ERR_TIRED`.
3. `!checkStructureAgainstController(...)` → `ERR_RCL_NOT_ENOUGH` (throws if room has no controller).
4. `lab1` invalid (falsy, no `id`, not in `register.structures`, not `instanceof Structure`, `structureType != lab`, or `lab1.id == this.id`) → `assertTargetObject`, `ERR_INVALID_TARGET`.
5. same for `lab2`.
6. `pos.getRangeTo(lab1) > 2 || pos.getRangeTo(lab2) > 2` → `ERR_NOT_IN_RANGE`.
7. `amount = LAB_REACTION_AMOUNT (5) + (PWR_OPERATE_LAB active ? POWER_INFO[PWR_OPERATE_LAB].effect[level-1] (2,4,6,8,10) : 0)`.
8. `this.mineralAmount > LAB_MINERAL_CAPACITY - amount` → `ERR_FULL`.
9. `lab1.mineralAmount < amount || lab2.mineralAmount < amount` → `ERR_NOT_ENOUGH_RESOURCES`.
10. `!(lab1.mineralType in REACTIONS) || !REACTIONS[m1][m2] || (this.mineralType && this.mineralType != product)` → `ERR_INVALID_ARGS`.
11. `intents.set(id, 'runReaction', {lab1: lab1.id, lab2: lab2.id})` → `OK`.
- **GOTCHA**: input labs need not be yours (no ownership check here or in PROC). `lab1 === lab2` is not rejected explicitly (fails at step 10 since no `X+X` reaction).

PROC-LAB-RUN (`reference/engine/src/processor/intents/labs/run-reaction.js:6-89`), all silent: `cooldownTime > time` → stop; amount as above (`endTime > time`); each input must exist, be a lab, hold a mineral (first truthy non-energy key) with `>= amount`, and be within Chebyshev 2; output `(store[mt]||0) + amount <= LAB_MINERAL_CAPACITY`; `product = REACTIONS[m1][m2]`, must equal output mineral if any. Effects: output `store[product] += amount`, `cooldownTime = time + REACTION_TIME[product]` (not reduced by powers), store-model transition; inputs `-= amount` with transition at 0; `actionLog.runReaction = {x1,y1,x2,y2}`. No RCL/owner checks in PROC.

### LAB-REVERSE-REACTION — `reverseReaction(lab1, lab2)`
API (`reference/engine/src/game/structures.js:360-410`): steps 1–6 as runReaction; then `lab1 == lab2` (same wrapper) → `ERR_INVALID_ARGS`; amount as step 7; `!this.mineralType || this.mineralAmount < amount` → `ERR_NOT_ENOUGH_RESOURCES`; `variant` = first `[r1,r2]` from `getReactionVariants(this.mineralType)` with `(!lab1.mineralType || lab1.mineralType == r1) && (!lab2.mineralType || lab2.mineralType == r2)`, none → `ERR_INVALID_ARGS`; `lab1.mineralAmount + amount > 3000 || lab2… ` → `ERR_FULL`; `intents.set(id, 'reverseReaction', {lab1, lab2})` → `OK`.
- `getReactionVariants(c)` iterates `REACTIONS` in key order (`H, O, Z, L, K, G, U, OH, X, ZK, UL, LH, …`) then inner key order, collecting every `[r1,r2]` with `REACTIONS[r1][r2] == c` (`reference/engine/src/utils.js:700-710`, `reference/common/lib/constants.js:484`). **GOTCHA**: with both output labs empty, `lab1` receives the reagent whose `REACTIONS` key comes first (e.g. reversing `OH` → lab1 gets `H`, lab2 gets `O`).

PROC-LAB-REVERSE (`reference/engine/src/processor/intents/labs/reverse-reaction.js:6-91`): stop if not a lab or `cooldownTime > time`; `lab1 id == lab2 id` → stop; source mineral `>= amount`; reads `lab1.store` **before** null-checking `lab1` — **GOTCHA**: if `lab1`/`lab2` was destroyed earlier in the same tick's processing, the original throws and the whole room's processing for that tick is lost (`reference/engine/src/processor.js:579`); chitin should treat as no-op (documented divergence). Output-lab full checks only when the lab already has a mineral (`store[m] + amount > storeCapacityResource[m]`); variant selection as API. Effects: source `-= amount`, `cooldownTime = time + REACTION_TIME[sourceMineral]`; outputs `+= amount` each with store-model transition; `actionLog.reverseReaction`. **No range, RCL or owner check in PROC**.

### LAB-BOOST-CREEP — `boostCreep(creep, [bodyPartsCount])`
API (`reference/engine/src/game/structures.js:412-441`):
1. `!my` → `ERR_NOT_OWNER`.
2. RCL check → `ERR_RCL_NOT_ENOUGH`.
3. target not a registered `Creep` or `target.spawning` → `ERR_INVALID_TARGET`.
4. `!pos.isNearTo(target)` → `ERR_NOT_IN_RANGE`.
5. `raw.store.energy < LAB_BOOST_ENERGY (20)` → `ERR_NOT_ENOUGH_RESOURCES`.
6. `mineralAmount < LAB_BOOST_MINERAL (30)` → `ERR_NOT_ENOUGH_RESOURCES`.
7. `n = count(parts with !boost && BOOSTS[type]?.[mineralType])` (dead 0-hit parts count); `bodyPartsCount = bodyPartsCount || 0`; `!n || (bodyPartsCount && bodyPartsCount > n)` → `ERR_NOT_FOUND`.
8. `intents.set(id, 'boostCreep', {id: target.id, bodyPartsCount})` → `OK`.
- **GOTCHA**: no cooldown check (boosting ignores lab cooldown); target ownership not checked (hostile adjacent creeps can be boosted); negative `bodyPartsCount` passes and becomes `slice(0, negative)` in PROC.

PROC-LAB-BOOST (`reference/engine/src/processor/intents/labs/boost-creep.js:6-62`): mineral = first truthy non-energy key; needs `>= 30` mineral and `>= 20` energy; target creep, not spawning, adjacent; eligible parts in body order; if the first eligible part is not `TOUGH` the list is reversed (non-TOUGH parts boost **right-to-left**, TOUGH left-to-right); if `bodyPartsCount` truthy take `slice(0, bodyPartsCount)`; loop while resources suffice: set `boost`, `-30` mineral, `-20` energy (partial boosting when resources run out). Store-model transition on empty mineral; `_recalc-body` (capacity) then save body/storeCapacity; `target.actionLog.healed = lab pos`. No RCL/owner check, no event-log entry.

### LAB-UNBOOST-CREEP — `unboostCreep(creep)`
API (`reference/engine/src/game/structures.js:443-466`) — **note the different order**:
1. target not a registered `Creep` → `ERR_INVALID_TARGET` (before ownership).
2. `!my || !target.my` → `ERR_NOT_OWNER`.
3. RCL → `ERR_RCL_NOT_ENOUGH`.
4. `cooldown > 0` → `ERR_TIRED`.
5. no boosted part → `ERR_NOT_FOUND`.
6. not adjacent → `ERR_NOT_IN_RANGE`.
7. `intents.set(id, 'unboostCreep', {id})` → `OK`.

PROC-LAB-UNBOOST (`reference/engine/src/processor/intents/labs/unboost-creep.js:6-53`): stop if `cooldownTime > time`; target same user; RCL; adjacent; count boosted parts per compound; clear all boosts; recalc body; for each `r` in `RESOURCES_ALL` order with `k` parts: drop `k*LAB_UNBOOST_ENERGY` (0 → skipped) energy and `k*LAB_UNBOOST_MINERAL` (15) of `r` via `_create-energy` at the creep tile (container first, then pile; amounts `Math.round`ed — `reference/engine/src/processor/intents/_create-energy.js:46-94`); `cooldown = Σ k * calcTotalReactionsTime(r) * LAB_UNBOOST_MINERAL / LAB_REACTION_AMOUNT` (= 3 × total chain reaction time per part, `reference/engine/src/utils.js:665-669`); set `cooldownTime = time + cooldown` if > 0.
- **GOTCHA**: unlike renew, unboost does not drop resources exceeding the new (lower) carry capacity — a creep can end up with `store > storeCapacity`.

### PROC-LAB-ORDER
Per lab (`reference/engine/src/processor/intents/labs/intents.js:2-14`): `boostCreep`, then `unboostCreep`, then `runReaction` **else** `reverseReaction` (never both). Labs are processed in object order; a later lab sees inputs already consumed by an earlier lab in the same tick (in-memory merge). Note `boostCreep` happens before `runReaction` on the same lab.

---

## 8. Link

### LINK-TRANSFER — `transferEnergy(target, [amount])`
API (`reference/engine/src/game/structures.js:488-533`):
1. `amount < 0` → `ERR_INVALID_ARGS`.
2. target invalid (falsy, no id, not in `register.structures`, not `Structure`, `=== this`, not a link) → `assertTargetObject`, `ERR_INVALID_TARGET`.
3. `!target.my` → `ERR_NOT_OWNER` (**target** ownership checked first).
4. `this.my === false && a rampart is on this tile` → `ERR_NOT_OWNER` (source ownership is *only* checked this way; PROC drops foreign-source intents anyway).
5. `cooldown > 0` → `ERR_TIRED`.
6. `!room.controller` → `ERR_RCL_NOT_ENOUGH`; RCL check → `ERR_RCL_NOT_ENOUGH`.
7. `!raw.store || !raw.store.energy` → `ERR_NOT_ENOUGH_ENERGY`.
8. if `!amount`: `amount = min(raw.store.energy, target cap ? cap.energy - target.store.energy : 0)`.
9. `this.energy < amount` → `ERR_NOT_ENOUGH_ENERGY`.
10. `!target.storeCapacityResource || !cap.energy || target.store.energy + amount > cap.energy` → `ERR_FULL`.
11. different room → `ERR_NOT_IN_RANGE` (checked last).
12. `intents.set(id, 'transfer', {id: target.id, amount, resourceType: 'energy'})` → `OK`.
- **GOTCHA**: with `amount` omitted and a full target, step 8 yields 0, step 10 passes (`cap + 0 > cap` is false) → returns `OK`; PROC then no-ops.
- **GOTCHA**: string amounts (`"100"`) make step 10 a string concatenation → `ERR_FULL`.

PROC-LINK-TRANSFER (`reference/engine/src/processor/intents/links/transfer.js:6-54`): stop if `!store || store.energy < amount || amount < 0`, target missing / not a link, `cooldown > 0`, source inactive (RCL), target has no energy cap, or target already full. If `target + amount > cap`, `amount = cap - target` (clamped, no error). Then `target += amount; source -= amount; target -= ceil(amount * LINK_LOSS_RATIO (0.03))`; `cooldown += LINK_COOLDOWN (1) * chebyshev(source, target)`; `actionLog.transferEnergy`; event `EVENT_TRANSFER {targetId, resourceType: energy, amount}` (pre-loss amount). Target ownership not re-checked.

### PROC-LINK-COOLDOWN
`reference/engine/src/processor/intents/links/tick.js:7-31`: after intents, if `cooldown > 0` decrement by 1 (same tick). So a transfer over distance d at tick T shows `cooldown = d-1` at T+1 and the link can fire again at T+d.

---

## 9. Nuker / Observer / PowerSpawn / Rampart / Factory

### NUKER-LAUNCH — `launchNuke(pos)`
API (`reference/engine/src/game/structures.js:1356-1388`):
1. `!my` → `ERR_NOT_OWNER`.
2. `!(pos instanceof RoomPosition)` → `ERR_INVALID_ARGS`.
3. launch room `novice > Date.now()` or `respawnArea > Date.now()` (raw room doc), or target room present in `roomStatusData.novice` / `roomStatusData.respawn` → `ERR_INVALID_TARGET`. The `||` chain short-circuits: the `roomStatusData` lookups run only if the launch-room checks pass (`reference/engine/src/game/structures.js:1363-1369`). **GOTCHA**: when the runner holds the initial `{}` (see RUNTIME-ROOM-STATUS-DATA), `roomStatusData.novice` is `undefined` and `undefined[pos.roomName]` throws a `TypeError` into player code (`register.wrapFn` is identity because `game.init` gets `undefined` as `_sandboxedFunctionWrapper`; nothing catches it — `reference/driver/lib/runtime/runtime.js:216-230`, `reference/engine/src/game/game.js:74`).
4. `cooldown > 0` → `ERR_TIRED`.
5. RCL → `ERR_RCL_NOT_ENOUGH`.
6. `|tx-x| > NUKE_RANGE (10) || |ty-y| > NUKE_RANGE` (no wrap) → `ERR_NOT_IN_RANGE`.
7. `energy < energyCapacity || ghodium < ghodiumCapacity` → `ERR_NOT_ENOUGH_RESOURCES`.
8. `intents.set(id, 'launchNuke', {roomName, x, y})` → `OK`.
- `roomStatusData` is a per-runner-process cache refreshed at most every 60 s of data rebuilt every 21 ticks — full rules in RUNTIME-ROOM-STATUS-DATA. Wall-clock based — **GOTCHA**: chitin must keep novice/respawn expiry as wall-clock timestamps (or define an equivalent) and the staleness is observable.

### RUNTIME-ROOM-STATUS-DATA — `roomStatusData` snapshot
Build (`updateRoomStatusData`, `reference/driver/lib/index.js:498-524`): `now = Date.now()`; query rooms matching `novice > now || respawnArea > now || openTime > now || status == 'out of borders'`; for each, first match wins: `novice > now` → `novice[id] = novice`; else `respawnArea > now` → `respawn[id] = respawnArea`; else `openTime > now` → `closed[id] = openTime`; else `closed[id] = Infinity`. Stored as `env.roomStatusData = JSON.stringify({novice, respawn, closed})`.
- **GOTCHA**: `JSON.stringify(Infinity)` is `null`, so an out-of-borders room with no future `openTime` arrives in the runtime as `closed[id] = null`. `_.isUndefined(null)` is false, so `Game.map.getRoomStatus` returns `{status: 'closed', timestamp: null}` for it (`reference/engine/src/game/map.js:218-219`), never `Infinity`. Chitin must emit `null` there.
- Written only from engine_main's step 5 (`reference/engine/src/main.js:8,79-83`): first when the new `gameTime > 20` after that process starts (later every 21 ticks), not awaited.

Runner cache (`reference/driver/lib/runtime/data.js:18-21,88-97`): per runner process, initial `{expireTimestamp: 0, data: {}}`. `getRoomStatusData()` is called once per user run inside `runtimeData.get` (`:143`, exposed as `runtimeData.roomStatusData`, `:212`). If `Date.now() > expireTimestamp` it **first** sets `expireTimestamp = now + 60 000`, then fetches the env key and `JSON.parse`s it into `data`; otherwise it returns the cached `data` (initially `{}`). Up to `RUNNER_THREADS` (default 2) users run concurrently per process (`reference/driver/lib/index.js:558-568`).

Fresh-world absence. The shipped env (`reference/launcher/init_dist/db.json`, `reference/storage/db.original.json`; `gameTime: 1`, `accessibleRooms` present) has no `roomStatusData` key, and nothing writes it until engine_main's first refresh (new `gameTime` 21 on a fresh world). Until then:
1. `env.get` → storage `dbEnvGet` returns `item.data[name]` = `undefined` (`reference/storage/lib/db.js:539-543`); the RPC response `{id, result: undefined}` loses the key in JSON and the client resolves `undefined` (`reference/common/lib/rpc.js:86-92`, `:118-122`).
2. `JSON.parse(undefined)` throws `SyntaxError`; `runtimeData.get` rejects; `make` rethrows `{error: stack}` (`reference/driver/lib/runtime/make.js:144,279-293`); `runUser` passes it to `saveResult` as `runResult.error` (`reference/engine/src/runner.js:15-27`). **The user whose call triggered the fetch loses that tick**: no code runs, no intents, no memory save, the parse error goes to their console. This repeats for one user per runner process at every 60 s expiry while the key is still absent.
3. Every other user in that process during the 60 s window gets the initial `{}` (`data` was never replaced):
   - `StructureNuker.launchNuke` throws `TypeError` (NUKER-LAUNCH step 3).
   - `Game.map.getRoomStatus(name)`: invalid names still return `undefined` (regex first); `!runtimeData.roomStatusData` is false for `{}`, so the "No runtime status data" `Error` is **not** thrown; but the next line reads `roomStatusData.closed[roomName]` with `closed` undefined → `TypeError` (`reference/engine/src/game/map.js:209-219`). The `accessibleRooms` fallback (`:228-232`) is never reached.
- **Chitin (deliberate divergence)**: compute `roomStatusData` from the room table before the first tick, and recompute it in-process whenever the inputs change (or at the same 21-tick cadence); never expose the absent/`{}` state. Bots therefore never see the fresh-world parse error or the `TypeError`s above. Keep the `Infinity`→`null` serialisation and the 60 s staleness only if bit-exact staleness is wanted (see PERF summary).
- **PERF**: global, read-only per tick; build once per refresh into a shared immutable map (`room → (status, timestamp)`) and share it across all runner threads — the original's per-process 60 s cache exists only to avoid DB round-trips.

PROC-NUKER-LAUNCH (`reference/engine/src/processor/intents/nukers/launch-nuke.js:8-51`): RCL; full G and energy vs `storeCapacityResource`; `cooldownTime > time` → stop; `x,y ∉ [0,49]` → stop; launch room novice/respawn (wall clock) → stop (target room status **not** rechecked); room-name regex; range. Effects: `store = {energy:0, G:0}`, `cooldownTime = time + NUKER_COOLDOWN` (100 on PTR); `bulk.insert({type:'nuke', room: target, x, y, landTime: time + NUKE_LAND_TIME (50 000; 100 on PTR), launchRoomName})` — **G**: a write into another room, visible from the next tick.

### OBSERVER-OBSERVE — `observeRoom(roomName)`
API (`reference/engine/src/game/structures.js:548-570`): `!my` → `ERR_NOT_OWNER`; not a string or not `/^(W|E)\d+(S|N)\d+$/` (case-sensitive) → `ERR_INVALID_ARGS`; RCL → `ERR_RCL_NOT_ENOUGH`; if no active `PWR_OPERATE_OBSERVER` and `|dx| > OBSERVER_RANGE (10) || |dy| > 10` (no wrap) → `ERR_NOT_IN_RANGE`; `intents.set(id, 'observeRoom', {roomName})` → `OK` (repeat calls: last wins).
PROC-OBSERVER: every observer's in-memory `observeRoom` is reset to `null` at room start (`reference/engine/src/processor.js:116-118`), set from the intent (`:284-288`, no validation), and written every tick (`:403-406`). The next tick's runtime adds `observeRoom` to the player's visible rooms for every observer they own (`reference/driver/lib/runtime/data.js:120-122`). Vision lasts exactly one tick (T+1). **PERF**: observer = cross-room read for the next tick; also one DB write per observer per tick in the original.

### PSPAWN-PROCESS — `processPower()`
API (`reference/engine/src/game/structures.js:613-631`): `!my` → `ERR_NOT_OWNER`; RCL → `ERR_RCL_NOT_ENOUGH`; `amount = 1 + (PWR_OPERATE_POWER active ? effect[level-1] (1..5) : 0)`; `power < amount || energy < amount * POWER_SPAWN_ENERGY_RATIO (50)` → `ERR_NOT_ENOUGH_RESOURCES`; `intents.set(id, 'processPower', {})` → `OK`.
PROC-PSPAWN-PROCESS (`reference/engine/src/processor/intents/power-spawns/process-power.js:7-39`): type/store; RCL; `amount = 1`, with effect (`endTime >= time`) `amount = min(store.power, 1 + effect)`; check; `power -= amount; energy -= 50*amount`; stats `powerProcessed`; `users.power += amount` (GPL).
- **GOTCHA**: with the effect active the API demands the full boosted amount, while PROC would happily process `min(power, …)`.

### RAMPART-SETPUBLIC — `setPublic(isPublic)`
API: `!my` → `ERR_NOT_OWNER`; `intents.set(id, 'setPublic', {isPublic: !!isPublic})` → `OK` (no RCL check, any argument) (`reference/engine/src/game/structures.js:651-657`). PROC-RAMPART-SETPUBLIC: `isPublic = !!intent.isPublic` (`reference/engine/src/processor/intents/ramparts/set-public.js:6-11`).

### FACTORY-PRODUCE — `produce(resourceType)`
API (`reference/engine/src/game/structures.js:1434-1470`):
1. `!my` → `ERR_NOT_OWNER`.
2. `cooldown > 0` → `ERR_TIRED`.
3. `!COMMODITIES[resourceType]` → `ERR_INVALID_ARGS`.
4. recipe has `level` and `level != factory.level` → `ERR_INVALID_TARGET`.
5. RCL → `ERR_RCL_NOT_ENOUGH`.
6. recipe has `level`, `factory.level > 0`, and no raw effect with `power == PWR_OPERATE_FACTORY && effect.level == recipe.level && endTime >= time` → `ERR_BUSY`.
7. any component `(store[c]||0) < need` → `ERR_NOT_ENOUGH_RESOURCES`.
8. `!storeCapacity || sum(store) - sum(components) + (recipe.amount||1) > storeCapacity` → `ERR_FULL`.
9. `intents.set(id, 'produce', {resourceType})` → `OK`.
PROC-FACTORY-PRODUCE (`reference/engine/src/processor/intents/factories/produce.js:6-44`): same checks in order store/recipe/level, cooldown, RCL, operate effect, components, capacity; subtract components, add `recipe.amount || 1`, write full store; `actionLog.produce`; `cooldownTime = time + recipe.cooldown` (not power-modified).

---

## 10. Tower

### TOWER-ATTACK — `attack(target)`
API (`reference/engine/src/game/structures.js:766-784`):
1. `!my` → `ERR_NOT_OWNER`.
2. `!target || !target.id || (!register.creeps[id] && !register.powerCreeps[id] && !register.structures[id]) || !(Creep|PowerCreep|StructureSpawn|Structure instance)` → `assertTargetObject`, `ERR_INVALID_TARGET`.
3. `!raw.store || raw.store.energy < TOWER_ENERGY_COST (10)` → `ERR_NOT_ENOUGH_ENERGY`.
4. RCL → `ERR_RCL_NOT_ENOUGH`.
5. `intents.set(id, 'attack', {id: target.id})` → `OK`.
- **GOTCHA**: no same-room check — a target in another visible room (or an unspawned power creep from `Game.powerCreeps`) returns `OK` and PROC no-ops. Own creeps/structures and the tower itself are accepted by the API.

### TOWER-HEAL — `heal(target)`
API (`:786-804`): `!my` → NOT_OWNER; target must be a registered `Creep` or `PowerCreep` → else INVALID_TARGET; energy < 10 → NOT_ENOUGH_ENERGY; RCL → RCL_NOT_ENOUGH; `intents.set(id, 'heal', {id})`.

### TOWER-REPAIR — `repair(target)`
API (`:806-824`): `!my` → NOT_OWNER; target must be in `register.structures` and a `Structure` → else INVALID_TARGET; energy → NOT_ENOUGH_ENERGY; RCL → RCL_NOT_ENOUGH; `intents.set(id, 'repair', {id})`.

### PROC-TOWER-PRIORITY
`reference/engine/src/processor/intents/towers/intents.js:2-10`: `heal` if present, **else** `repair`, **else** `attack`. One action per tower per tick; the others are discarded although each returned OK and cost 0.2 CPU.

### PROC-TOWER-FALLOFF
`reference/engine/src/processor/intents/towers/attack.js:32-46` (identical in heal.js:23-37, repair.js:20-34):
```
range = chebyshev(tower, target)        // after rampart redirect for attack
amount = POWER                           // TOWER_POWER_ATTACK 600 / HEAL 400 / REPAIR 800
if range > TOWER_OPTIMAL_RANGE (5):
    range = min(range, TOWER_FALLOFF_RANGE (20))
    amount -= amount * TOWER_FALLOFF (0.75) * (range - 5) / (20 - 5)
for p in [PWR_OPERATE_TOWER, PWR_DISRUPT_TOWER]:   // in this order
    if effect(p) && endTime > time: amount *= POWER_INFO[p].effect[level-1]
amount = floor(amount); if amount == 0 → no-op (no energy spent)
```
Floating-point order matters for `floor` (compute in f64 exactly in this order).

### PROC-TOWER-ATTACK
`reference/engine/src/processor/intents/towers/attack.js:6-70`: stop if no store, target missing, `target == tower`, spawning creep, `!target.hits` (controller etc.), or energy < 10. If a rampart exists on the target tile the **rampart becomes the target** (also when the target is itself a structure under a rampart, or an own creep under a hostile rampart). Apply falloff, then `_damage(tower, target, amount, EVENT_ATTACK_TYPE_RANGED)` (`reference/engine/src/processor/intents/_damage.js:6-94`: creeps get `_damageToApply`; walls/ramparts with FORTIFY/INVULNERABILITY are immune; structures lose hits and are destroyed at ≤ 0 with ruin + `EVENT_OBJECT_DESTROYED`; power bank hit-back is computed but towers never apply `_damageToApply`). Then `energy -= 10`, `actionLog.attack`, target `actionLog.attacked`, attack notification if `target.notifyWhenAttacked`.

### PROC-TOWER-HEAL
`reference/engine/src/processor/intents/towers/heal.js:6-53`: target must be creep/powerCreep, not spawning; energy ≥ 10; falloff; `target._healToApply += amount` (applied in creep tick); `energy -= 10`; actionLogs; `EVENT_HEAL {targetId, amount, healType: EVENT_HEAL_TYPE_RANGED}`. **GOTCHA**: healing a full-hp creep still spends 10 energy.

### PROC-TOWER-REPAIR
`reference/engine/src/processor/intents/towers/repair.js:6-56`: target must have `CONSTRUCTION_COST[type]` and `hits < hitsMax` (else no energy spent); falloff; `hits = min(hits + amount, hitsMax)`; `energy -= 10`; stats `energyConstruction += 10`; `EVENT_REPAIR {targetId, amount, energySpent: 10}` (amount = pre-clamp value).

---

## 11. Terminal

### TERMINAL-SEND — `send(resourceType, amount, destination, [description])`
API (`reference/engine/src/game/structures.js:714-745`):
1. `!my` → `ERR_NOT_OWNER`.
2. RCL → `ERR_RCL_NOT_ENOUGH`.
3. `!/^(W|E)\d+(N|S)\d+$/.test(destination)` → `ERR_INVALID_ARGS` (no string check; `test` coerces).
4. `resourceType ∉ RESOURCES_ALL` → `ERR_INVALID_ARGS`.
5. `!raw.store || !raw.store[r] || raw.store[r] < amount` → `ERR_NOT_ENOUGH_RESOURCES` (amount is not type-checked; `undefined` passes).
6. `raw.cooldownTime > time` → `ERR_TIRED`.
7. `cost = calcTerminalEnergyCost(amount, calcRoomsDistance(room, destination, true))` (no power discount); `r != energy && energy < cost || r == energy && energy < amount + cost` → `ERR_NOT_ENOUGH_RESOURCES`.
8. `description && (!_.isString(description) || description.length > 100)` → `ERR_INVALID_ARGS`.
9. `intents.set(id, 'send', {resourceType, amount, targetRoomName: destination, description})` → `OK`.
- **GOTCHA**: `TERMINAL_MIN_SEND` is not enforced; non-integer amounts are truncated by the sanitizer (`0 < amount < 1` → silently nothing); a missing/unowned destination terminal returns OK and nothing happens.

### PROC-TERMINAL-SEND-ROOM
`reference/engine/src/processor/intents/terminal/send.js:6-33` (room stage): re-validates regex, resource, `amount` truthy and `store[r] >= amount`, cost with PWR_OPERATE_TERMINAL (`endTime >= time`, `ceil(cost * effect)`) and energy sufficiency; then persists `terminal.send = intent`. No cooldown/RCL/owner check here.

### PROC-TERMINAL-SEND-GLOBAL
`reference/engine/src/processor/global-intents/market.js:71-95` (first thing in the global stage, over all terminals with `send`, in DB order):
1. clear `send`.
2. `cooldownTime > time` → stop.
3. `terminalsByRoom[target]` (if a room has several terminals the last in DB order wins, `:13`) must exist and have a `user` (any player) → else stop.
4. `cooldown = TERMINAL_COOLDOWN (10)`, with PWR_OPERATE_TERMINAL (`endTime > time`) `Math.round(10 * effect)`.
5. `executeTransfer(from, to, r, amount, feePayer = from, {description: description ? description.replace(/</g, '&lt;') : undefined})`; on success `cooldownTime = time + cooldown`.

### PROC-MARKET-EXECUTE-TRANSFER
`reference/engine/src/processor/global-intents/market.js:15-69`:
```
if any terminal missing → false
if from.user && store[r] < amount → false          // NPC terminals (no user) have infinite stock
if to.user: amount = min(amount, max(0, to.storeCapacity - sum(to.store)))   // partial delivery
if !(amount > 0) → undefined (falsy, no cooldown)
cost = ceil(amount * (1 - exp(-range/30)))  with range = wrapped distance(from.room, to.room)
if feePayer has PWR_OPERATE_TERMINAL (endTime > time): cost = ceil(cost * effect)
if feePayer is from: need energy >= cost (+amount if r is energy); if feePayer is to: need to.energy >= cost
to.store[r] += amount (only if to.user); from.store[r] -= amount; feePayer.energy -= cost
insert transaction {time, sender: from.user, recipient: to.user, resourceType, amount, from: room, to: room, ...extra}
→ true
```
- **GOTCHA**: delivery is silently clipped to the recipient's free space (cost recomputed on the clipped amount); a full recipient means no transfer and no cooldown. Inactive recipient terminals have `storeCapacity 0` → nothing.
- **GOTCHA**: `description` stored HTML-escaped (`<` → `&lt;`) and truncated to 100 chars by the sanitizer; empty description becomes `undefined`.
- Sends run before deals in the same stage, and the cooldown they set blocks deals from the same terminal (PROC-MARKET-DEAL-TERMINAL step 2) — **GOTCHA**: `send` + `deal` on one terminal in one tick: send wins, deal silently dropped.

---

## 12. Spawn

### SPAWN-PROPS
(`reference/engine/src/game/structures.js:857-863`) `name`, `energy`, `energyCapacity` (extension formulas), `spawning = raw.spawning ? new StructureSpawn.Spawning(id) : null`, `store`.

### SPAWN-MEMORY
Getter (`:866-877`): `!my` → `undefined`; if `Memory.spawns` is `undefined` **or the string `'undefined'`** set it to `{}`; if not an object → `undefined`; return `Memory.spawns[name] = Memory.spawns[name] || {}` (**GOTCHA**: reading creates/normalises the entry; falsy stored values are replaced by `{}`). Setter (`:879-890`): `!my` → throw `Could not set other player's spawn memory`; same init; non-object → throw `Could not set spawn memory`; assign. Touching it parses `Memory` (lazy JSON parse cost).

### SPAWN-TOSTRING / SPAWN-GLOBAL-ALIAS
`[spawn <name>]` for own spawns, else `[spawn #<id>]` (`:893-895`). Both `StructureSpawn` and `Spawn` globals refer to the same constructor (`:1292-1293`).

### SPAWN-ROOM-ENERGY
`Room.energyAvailable` / `energyCapacityAvailable` start at 0 (`reference/engine/src/game/rooms.js:434-435`) and sum `store.energy` / `storeCapacityResource.energy` over **the player's own** spawns and extensions in that room with `!off` (`reference/engine/src/game/game.js:325-328`). Not updated during the tick.

### SPAWN-COST
`calcCreepCost(body)` = Σ `BODYPART_COST[part]` (`move 50, work 100, attack 80, carry 50, heal 250, ranged_attack 150, tough 10, claim 600`), parts may be strings or `{type}` objects; unknown part throws (`reference/engine/src/utils.js:114-126`, `reference/common/lib/constants.js:96-105`).

### SPAWN-NAME-RULES
In `spawnCreep` (`reference/engine/src/game/structures.js:1065-1075`): `!name` → `ERR_INVALID_ARGS` (empty string, 0, null, undefined); `name = name.toString()`; `length > 100` → `ERR_INVALID_ARGS`; `Game.creeps[name]` truthy **or** name already used by a spawn call this tick (`createdCreepNames`, reset per tick at `:27`) → `ERR_NAME_EXISTS`.
- **GOTCHA**: `Game.creeps` is a plain object, so names equal to `Object.prototype` members (`toString`, `constructor`, `hasOwnProperty`, `valueOf`, `__proto__`, `isPrototypeOf`, `propertyIsEnumerable`, `toLocaleString`, `__defineGetter__`, `__defineSetter__`, `__lookupGetter__`, `__lookupSetter__`) return `ERR_NAME_EXISTS`.
- Uniqueness is per player only, checked only in the runtime; the processor never checks names (`reference/engine/src/processor/intents/spawns/create-creep.js`). If two live creeps share a name, the runtime makes the second one `suicide()` (`reference/engine/src/game/game.js:273-276`).

### SPAWN-SPAWNCREEP — `spawnCreep(body, name, opts = {})`
Validation order (`reference/engine/src/game/structures.js:1063-1218`):
1. `!name || !_.isObject(opts)` → `ERR_INVALID_ARGS` (`opts === null` fails; arrays/functions pass).
2. name rules (SPAWN-NAME-RULES) → `ERR_INVALID_ARGS` / `ERR_NAME_EXISTS`.
3. `energyStructures = opts.energyStructures && _.uniq(_.map(opts.energyStructures, 'id'))` (no validation; non-arrays are mapped lodash-style).
4. directions (SPAWN-OPT-DIRECTIONS) → `ERR_INVALID_ARGS`.
5. `!my` → `ERR_NOT_OWNER`.
6. `raw.spawning` → `ERR_BUSY`.
7. `raw.off` → `ERR_RCL_NOT_ENOUGH`.
8. `!body || !isArray || length == 0 || length > MAX_CREEP_SIZE (50)` or any element `∉ BODYPARTS_ALL` → `ERR_INVALID_ARGS`.
9. `available = energyStructures ? calcEnergyAvailable(energyStructures) : room.energyAvailable`; `< calcCreepCost(body)` → `ERR_NOT_ENOUGH_ENERGY`.
10. `opts.dryRun` truthy → `OK` (no side effects at all).
11. Side effects: push name to `createdCreepNames`; memory (SPAWN-OPT-MEMORY); create the stub (SPAWN-STUB-CREEP); `intents.set(spawnId, 'createCreep', {name, body, energyStructures, directions})` → `OK`.
- **GOTCHA**: argument/direction errors are reported before ownership/busy; body errors after busy/RCL.
- **GOTCHA**: a second `spawnCreep` on the same spawn in the same tick passes again (raw `spawning` unchanged) and overwrites the intent; both names stay in `Game.creeps`/`Memory.creeps`, only the last is spawned.
- **GOTCHA**: energy is not reserved: several spawns in one room can all return OK; PROC charges sequentially and later ones fail silently, leaving their `Memory.creeps[name]` entries behind.

### SPAWN-OPT-DIRECTIONS
`reference/engine/src/game/structures.js:1079-1090`: if `opts.directions !== undefined`: not an array → `ERR_INVALID_ARGS`; `d = _.uniq(map(+x))`; `d.length == 0` or any value not an integer in 1..8 → `ERR_INVALID_ARGS`. The normalised (deduplicated, numeric, order-preserving) array is sent.

### SPAWN-OPT-ENERGYSTRUCTURES
API availability (`:1053-1061`): Σ over ids of `store.energy` for objects that exist in the player's **whole** visible object set, are not `off`, and are of type spawn/extension — owner and room are **not** checked. PROC (PROC-SPAWN-CHARGE) only accepts the spawn owner's non-off spawns/extensions **in the same room** → **GOTCHA**: listing structures from another room or another player can make the API return OK while PROC silently fails. `[]` → available 0 → `ERR_NOT_ENOUGH_ENERGY`.

### SPAWN-OPT-MEMORY
`if Memory.creeps === undefined → {}`; if `Memory.creeps` is an object: `Memory.creeps[name] = opts.memory || Memory.creeps[name] || {}` (`:1124-1130`). **GOTCHA**: falsy memory values (`0`, `''`, `false`, `null`) are replaced; existing memory for that name is reused when `opts.memory` is falsy.

### SPAWN-OPT-DRYRUN
`:1118-1120` — returns `OK` after all checks, before any side effect.

### SPAWN-STUB-CREEP
`:1132-1213` (identical block in createCreep `:966-1047`): `Game.creeps[name] = new Creep()` (no id) with `RoomObject` at the spawn's position (no effects), plus own non-configurable enumerable getters: `name`, `spawning: true`, `my: true`, `body: body.map(type => ({type, hits:100}))` (fresh array each access), `owner: {username}`, `ticksToLive: CREEP_LIFE_TIME` (1500 even with CLAIM), `carryCapacity: 50 × #CARRY`, `carry: {energy:0}`, `store: new Store({store:{energy:0}})` (fresh each access, no capacity → `getCapacity()` null), `fatigue: 0`, `hits = hitsMax = 100 × body.length`, `saying: undefined`.
- **GOTCHA**: the stub has `id === undefined`, is not in `Game.getObjectById`, `room.find`, or `register.creeps` (so `lab.boostCreep(stub)` → `ERR_INVALID_TARGET`), but prototype methods exist (calling e.g. `move` will fail on `data(undefined)`).

### SPAWN-CANCREATECREEP — `canCreateCreep(body, [name])` (deprecated)
`reference/engine/src/game/structures.js:897-931`: `!my` → NOT_OWNER; `raw.spawning` → BUSY; body invalid → INVALID_ARGS; `room.energyAvailable < cost` → NOT_ENOUGH_ENERGY; `raw.off` → RCL_NOT_ENOUGH; if `name` truthy: `toString`, exists (`Game.creeps` or `createdCreepNames`) → NAME_EXISTS, length > 100 → INVALID_ARGS; → OK. **GOTCHA**: energy is checked before `off` (opposite of spawnCreep).

### SPAWN-CREATECREEP — `createCreep(body, [name], [memory])` (deprecated)
`:933-1051`:
1. If `_.isObject(name) && memory === undefined` → `memory = name; name = undefined`.
2. `r = canCreateCreep(body, name)`; non-OK → return `r`.
3. If `!name` → `name = getUniqueName(…)` (SPAWN-UNIQUE-NAME).
4. Push name (raw, not stringified) to `createdCreepNames`; memory: `memory !== undefined ? Memory.creeps[name] = memory : Memory.creeps[name] ||= {}` (null is stored as-is, unlike spawnCreep); stub; `intents.set(id, 'createCreep', {name, body})` (no energyStructures/directions → old energy handling, all directions).
5. Returns the **name** (string or the original non-string value) on success.

### SPAWN-UNIQUE-NAME
`reference/engine/src/game/names.js:4-23`, predicate `:946-949`:
```
c = 0
loop:
  list = Math.random() > 0.5 ? boyNames : girlNames        // RNG draw 1
  name = list[floor(Math.random() * list.length)]          // RNG draw 2
  if c > 3: name += list[floor(Math.random() * list.length)]   // RNG draw 3 (same list)
  c++
  until !(exists(name))
exists(n) = any raw object in runtimeData.roomObjects with {type:'creep', user: spawn.user, name: n} || n in createdCreepNames
```
Name lists are fixed literals at `names.js:1-2` (copy verbatim). **Randomness**: uses the sandbox `Math.random`; chitin must expose the same RNG stream semantics only if replay determinism is required (Open questions). **PERF**: each attempt scans all visible objects.

### SPAWN-NOTIFY
Spawn override (`reference/engine/src/game/structures.js:1220-1235`): `!my` → NOT_OWNER; `!_.isBoolean` → INVALID_ARGS; set intent if different → OK. (No `room`/controller-owner checks.)

### SPAWN-RENEW — `renewCreep(target)`
API (`:1237-1267`):
1. `this.spawning` → `ERR_BUSY` (before ownership).
2. target not a registered `Creep`, `target.spawning`, or has a `CLAIM` part → `ERR_INVALID_TARGET`.
3. `!my || !target.my` → `ERR_NOT_OWNER`.
4. `raw.off` → `ERR_RCL_NOT_ENOUGH`.
5. not adjacent → `ERR_NOT_IN_RANGE`.
6. `room.energyAvailable < ceil(SPAWN_RENEW_RATIO (1.2) * calcCreepCost(body) / CREEP_SPAWN_TIME (3) / body.length)` → `ERR_NOT_ENOUGH_ENERGY`.
7. `ticksToLive + floor(1.2 * CREEP_LIFE_TIME / 3 / body.length) > CREEP_LIFE_TIME` → `ERR_FULL`.
8. if any part boosted: `register.deprecated(<message>)` → one `console.log` per distinct message per tick (`reference/engine/src/game/game.js:113-120`).
9. `intents.set(id, 'renewCreep', {id})` → `OK`.

PROC-SPAWN-RENEW (`reference/engine/src/processor/intents/spawns/renew-creep.js:6-55`): spawn not spawning; target creep of same user, not spawning, adjacent, no CLAIM; `effect = floor(600/len)`; stop if `ageTime + effect > time + CREEP_LIFE_TIME`; `cost = ceil(1.2*cost/3/len)` charged with **old** energy handling (PROC-SPAWN-CHARGE); stats `energyCreeps`; `actionLog.healed`; `ageTime += effect`; if boosted: clear boosts, recalc body, drop resources beyond new capacity (`_drop-resources-without-space`, iterating `RESOURCES_ALL` order), save body/storeCapacity.
- Several spawns may renew the same creep in one tick (each re-checks the in-memory `ageTime`).

### SPAWN-RECYCLE — `recycleCreep(target)`
API (`:1269-1290`): `!my` → NOT_OWNER; target not a registered `Creep` or spawning → INVALID_TARGET; `raw.off` → RCL_NOT_ENOUGH; `!target.my` → NOT_OWNER; not adjacent → NOT_IN_RANGE; `intents.set(id, 'recycleCreep', {id})` → OK. (Works while the spawn is spawning.)
PROC-SPAWN-RECYCLE (`reference/engine/src/processor/intents/spawns/recycle-creep.js:6-23`): target creep of same user, not spawning, adjacent → `_die(target, dropRate = 1.0, violent = false)` (`reference/engine/src/processor/intents/creeps/_die.js:6-102`):
```
lifeTime = has CLAIM ? CREEP_CLAIM_LIFE_TIME (600) : CREEP_LIFE_TIME (1500)
lifeRate = 1.0 * (ageTime - time at room start) / lifeTime
per part: energy += min(CREEP_PART_MAX_ENERGY (125), BODYPART_COST[type] * lifeRate)
          if boosted: res[boost] += LAB_BOOST_MINERAL * lifeRate; energy += LAB_BOOST_ENERGY * lifeRate
each resource: floor; put into a container on the creep's tile (hits > 0) up to free space, rest into the tombstone
creep store: same container-then-tombstone rule
tombstone at creep tile, decayTime = time + body.length * TOMBSTONE_DECAY_PER_PART (5), EVENT_OBJECT_DESTROYED {type:'creep'}
```
No `creepsLost` stat (not violent). Note the drop goes to the creep's tile, not the spawn's.

### PROC-SPAWN-ORDER
`reference/engine/src/processor/intents/spawns/intents.js:1-17`: `createCreep`, `renewCreep`, `recycleCreep`, `setSpawnDirections`, `cancelSpawning` — all may run in one tick.

### PROC-SPAWN-CREATE
`reference/engine/src/processor/intents/spawns/create-creep.js:7-101`:
1. stop if `spawn.spawning`, not a spawn, or inactive by COMMON-RCL-CHECK (not `off`).
2. directions: if defined: non-array → stop; `uniq(map(+))`; if non-empty and any invalid → stop. (An empty array passes here — only reachable bypassing the API.)
3. `body = body.slice(0, MAX_CREEP_SIZE)` (already filtered by the sanitizer); `cost = calcCreepCost(body)`; charge (PROC-SPAWN-CHARGE) or stop.
4. stats `energyCreeps += cost`, `creepsProduced += body.length`.
5. `needTime = CREEP_SPAWN_TIME * body.length`; PWR_OPERATE_SPAWN (`endTime > time`): `needTime = ceil(needTime * effect)` (0.9 … 0.2).
6. `spawn.spawning = {name, needTime, spawnTime: time + needTime, directions}`.
7. insert creep `{name, x, y (spawn tile), body: [{type, hits:100}], store:{energy:0}, storeCapacity: 50·#CARRY, type:'creep', room, user, hits = hitsMax = 100·len, spawning: true, fatigue: 0, notifyWhenAttacked: true, tutorial?}` (no `ageTime` yet).

### PROC-SPAWN-CHARGE
`reference/engine/src/processor/intents/spawns/_charge-energy.js:6-78`:
- **Old handling** (`energyStructures === undefined`; createCreep, renew): spawns and extensions of the spawn's user in the room with `!off`; if total < cost → fail. Drain **all spawns first**, sorted (stable) by Chebyshev distance to the *spawning* spawn (itself first at distance 0), then extensions sorted by distance to the spawning spawn, each `min(remaining, energy)`.
- **New handling** (`energyStructures` array): keep ids that exist in the room, `!off`, same user, type spawn/extension; `uniq`; if total < cost → fail; drain in the given order until cost reached.
- Ties are broken by room object order (Open questions).

### PROC-SPAWN-SET-DIRECTIONS / SPAWNING-SETDIRECTIONS
API `Spawning.setDirections(directions)` (`reference/engine/src/game/structures.js:1312-1326`): `!spawn.my` → NOT_OWNER; if non-empty array and every `+d` (after uniq) is an integer in 1..8 → `intents.set(spawn.id, 'setSpawnDirections', {directions})` → OK; otherwise `ERR_INVALID_ARGS`. PROC (`reference/engine/src/processor/intents/spawns/set-spawn-directions.js:6-21`): spawn must be spawning; same validation; replaces `spawning.directions` (array replaced, not merged).

### PROC-SPAWN-CANCEL / SPAWNING-CANCEL
API `Spawning.cancel()` (`reference/engine/src/game/structures.js:1328-1334`): `!spawn.my` → NOT_OWNER; `intents.set(spawn.id, 'cancelSpawning', {})` → OK. PROC (`reference/engine/src/processor/intents/spawns/cancel-spawning.js:6-13`): if spawning: remove the creep found at the spawn tile with that name, `spawning = null`. Energy is not refunded.
- **GOTCHA**: if the creep isn't found (e.g. a stale `Spawning` object kept in `global` used in the same tick as a new `spawnCreep`, whose creep is only bulk-inserted, not in `roomObjects`) the original throws and loses the room's tick; chitin should no-op.

### SPAWNING-PROPS
`new StructureSpawn.Spawning(spawnId)` (`reference/engine/src/game/structures.js:1301-1310`): own enumerable data properties `name`, `needTime`, `remainingTime = spawning.spawnTime - time`, `directions` (may be `undefined`); non-enumerable `spawn` (the spawn wrapper). Also used for invader cores.

### PROC-SPAWN-TICK
`reference/engine/src/processor/intents/spawns/tick.js:6-49` (after all intents):
- If spawning: with active `PWR_DISRUPT_SPAWN` (`endTime > time`) → `spawnTime += 1`; else if `time >= spawnTime - 1` → `_born-creep`; success → `spawning = null`, failure → `spawnTime = time + 1`.
- `_born-creep` (`reference/engine/src/processor/intents/spawns/_born-creep.js:7-79`): try `spawning.directions || [1..8]` in order; a tile is blocked by any `OBSTACLE_OBJECT_TYPES` object or obstacle construction site, a tile already claimed by movement this tick, or wall terrain without a road. First free → place creep, `spawning:false`. If none free: remember the first direction tile holding a creep of another user; if found and none of the *other* (non-preferred) directions is free (that re-check treats wall terrain as blocked even with a road) → kill that creep (`_die`, violent) and place there ("spawn stomp"); else fail.
- Energy regen: only if the controller exists, `level ≥ 1`, `controller.user == spawn.user`, the spawn is among the N closest of **all** spawns in the room, not tutorial, room `energyAvailable` (all users' non-off spawns+extensions, computed once after intents, `reference/engine/src/processor.js:326`) `< SPAWN_ENERGY_CAPACITY` and spawn energy `< 300` → `+1`. Several spawns can each add 1 in the same tick.

---

## 13. Game.market

`Game.market` is rebuilt every tick (`reference/engine/src/game/game.js:433`). Methods are plain functions on the object and use `this` (`this.credits`, `this.orders`, `this.calcTransactionCost`); see Open questions for detached calls. Property enumeration order: the nine methods (`calcTransactionCost, getAllOrders, getHistory, getOrderById, createOrder, cancelOrder, deal, changeOrderPrice, extendOrder`) then getters `credits, incomingTransactions, outgoingTransactions, orders` (`reference/engine/src/game/market.js:29-250`).

### MARKET-UNITS
Server stores money (`users.money`) and order `price` in **milli-credits** (integers for prices; money can become fractional, see PROC-MARKET-EXPIRY). The API divides by 1000 for display (`credits`, order objects) and the sanitizer converts API prices with `parseInt((1000*p).toFixed(0))` (`reference/common/lib/system.js:8`). Fees are `ceil`ed in milli-credits in PROC. **GOTCHA**: a price that rounds to 0 milli-credits (e.g. 0.0004) passes the API (`price > 0`) and is silently dropped by PROC.

### MARKET-DATA-SOURCES
Runtime data per player tick (`reference/driver/lib/runtime/data.js`):
- `market.orders` = **active** orders of all players and NPCs, grouped `{all: {id: order}, <resourceType>: {id: order}}`, raw fields minus `_id` plus `id` string, prices in milli (`:31-49`). Cached per runner process per `gameTime` — shared by all players on that process.
- `market.myOrders` = all of the player's orders (active and inactive) (`:227,243-247`).
- `market.history` = all `market.stats` docs grouped by resource plus `all`, cached per process for **3 hours** (`:51-75`).
- `transactions.outgoing` / `incoming` = last 100 by `time` desc where `sender` / `recipient` is the player (`:138-139,213-216`).
- `user.money`, `user.resources` (account resources).
- **GOTCHA** (private server only): LokiJS documents also carry `meta` and `$loki` fields which leak into `orders`, `getAllOrders`, transactions (history strips them, `:63-65`). The official MMO (MongoDB) does not have them — chitin should not emit them.

### MARKET-CREDITS
`(user.money || 0) / 1000` (`reference/engine/src/game/market.js:196-201`).

### MARKET-INCOMING-TX / MARKET-OUTGOING-TX
Lazy, cached per tick (`:203-233`): maps the raw transaction docs **in place**: `transactionId = "" + _id`, delete `_id`, `sender = sender ? {username} : undefined`, `recipient = recipient ? {username} : undefined`. Fields: `transactionId, time, sender, recipient, resourceType, amount, from, to`, plus `description` (sends) or `order: {id, type, price}` (deals, price in credits). Order: newest first, max 100 (older ones are also purged hourly, MARKET-TX-PURGE). Same array object returned on every access in a tick.

### MARKET-ORDERS
Lazy, cached per tick (`:235-249`): `_.indexBy` of `myOrders` mapped in place: `id = "" + _id`, delete `_id` and `user`, `price /= 1000`. Includes inactive and fully-filled orders. Fields: `id, created (ticks; absent for intershard), createdTimestamp (ms), active, type, resourceType, roomName, amount, remainingAmount, totalAmount, price`. Orders created this tick appear next tick.

### MARKET-CALC-COST — `calcTransactionCost(amount, roomName1, roomName2)`
`calcTerminalEnergyCost(amount, calcRoomsDistance(r1, r2, true))` (`:31-34`). No validation: non-string room names throw (`substr`), bad names give `NaN`. Ignores PWR_OPERATE_TERMINAL. Doc example: 1000 units W0N0→W10N5 → 284.

### MARKET-GET-ALL-ORDERS — `getAllOrders([filter])`
`:13-39`:
1. `key = (filter && filter.resourceType) || 'all'`.
2. If not cached this tick: if `key != 'all'` and not in `RESOURCES_ALL ∪ INTERSHARD_RESOURCES` → use `{}` (not cached; final result `[]`); else `cache[key] = JSON.parse(JSON.stringify(orders[key]) || '{}')` (valid resource with no orders → `{}`) and `price /= 1000` on each.
3. Return `_.filter(cache[key], filter)` (lodash 3: object → `_.matches` deep partial, function → predicate, string → property truthiness, `undefined` → all values) — always an array, in the cached object's key order.
- Returned objects contain `id, user (owner user id!), active (true), created, createdTimestamp, type, resourceType, roomName, amount, remainingAmount, totalAmount, price` — **GOTCHA**: `user` and `totalAmount`/`active` are present though undocumented; own orders are included (docs say "other players'").
- **GOTCHA**: the same order objects are returned by every call with the same `resourceType` key in a tick → user mutations persist within the tick; the `all` cache and per-resource caches are separate copies.
- **PERF**: deep copy of the whole order book per player per tick when called without `resourceType`.

### MARKET-GET-HISTORY — `getHistory([resourceType])`
`:41-53`: key `resourceType || 'all'`; invalid resource → `{}`; else cached per tick `JSON copy of (history[key] || {})`. Returns an **array** of `{resourceType, date: 'YYYY-MM-DD', transactions, volume, avgPrice, stddevPrice}` when data exists, but `{}` (an object) for a valid resource with no history — **GOTCHA**.

### MARKET-GET-ORDER-BY-ID — `getOrderById(id)`
`:55-66`: if `this.orders[id]` → fresh JSON copy of the own order (any state); else if active order `market.orders.all[id]` → JSON copy with `price /= 1000`; else `null`.

### MARKET-CREATE-ORDER — `createOrder(params)` or `createOrder(type, resourceType, price, totalAmount, roomName)`
`:68-98`:
1. If `_.isObject(type)` destructure `{type, resourceType, price, totalAmount, roomName}` from it.
2. `resourceType ∉ RESOURCES_ALL ∪ INTERSHARD_RESOURCES` → `ERR_INVALID_ARGS`.
3. `type ∉ {ORDER_BUY 'buy', ORDER_SELL 'sell'}` (loose `!=`) → `ERR_INVALID_ARGS`.
4. `price = parseFloat(price)`, `totalAmount = parseInt(totalAmount)`; `!price || price <= 0 || !totalAmount` → `ERR_INVALID_ARGS` (**negative totalAmount passes**).
5. `price * totalAmount * MARKET_FEE (0.05) > credits` → `ERR_NOT_ENOUGH_RESOURCES` (checked against the full balance every call; not decremented by earlier calls this tick).
6. non-intershard and (`!roomName` or no own terminal with `room == roomName` in `userObjects`) → `ERR_NOT_OWNER` (terminal may be inactive).
7. `_.size(this.orders) + ordersCreatedThisTick >= MARKET_MAX_ORDERS (300)` → `ERR_FULL` (filled/inactive orders count until cancelled; docs' "50" is stale).
8. `ordersCreatedThisTick++`; `pushByName('global', 'createOrder', {type, resourceType, price, totalAmount, roomName})` (no limit) → `OK`.

PROC-MARKET-CREATE (`reference/engine/src/processor/global-intents/market.js:109-166`): stop if `!price || !totalAmount`, resource invalid, non-intershard and the room's terminal is missing or not owned by the user, `price <= 0 || totalAmount <= 0`; `fee = ceil(price * totalAmount * 0.05)` (milli); `user.money < fee` → stop; `money -= fee`; insert order `{createdTimestamp: Date.now(), user, active: false, type, amount: 0, remainingAmount: totalAmount, resourceType, price, totalAmount, roomName, created: time (non-intershard only)}`; money log `market.fee`. The order is inactive until the next tick's maintenance (visible to others two ticks after the call). For intershard orders `roomName` is the sanitized string `"undefined"`.

### MARKET-CANCEL-ORDER — `cancelOrder(orderId)`
API (`reference/engine/src/game/market.js:100-106`): `!this.orders[orderId]` → `ERR_INVALID_ARGS`; `pushByName('global','cancelOrder',{orderId}, 50)` — **return value ignored**, so the 51st+ call still returns `OK` but is dropped. PROC-MARKET-CANCEL (`reference/engine/src/processor/global-intents/market.js:256-263`): if the order belongs to the user: `remainingAmount = 0`, `_cancelled = true` in memory (blocks deals this tick); removed at maintenance (`:502-505`). No fee refund.

### MARKET-CHANGE-PRICE — `changeOrderPrice(orderId, newPrice)`
API (`reference/engine/src/game/market.js:155-172`): own order else `ERR_INVALID_ARGS`; `newPrice = parseFloat`; `!newPrice || <= 0` → `ERR_INVALID_ARGS`; if `newPrice > order.price && (newPrice - price) * remainingAmount * 0.05 > credits` → `ERR_NOT_ENOUGH_RESOURCES`; `pushByName(..., 50)` (result ignored) → `OK`. (Docs list ERR_NOT_OWNER; never returned.)
PROC-MARKET-CHANGE-PRICE (`reference/engine/src/processor/global-intents/market.js:168-213`): own order; `newPrice > 0`; if `newPrice != price` → `order._skip = true` (**before** the fee check — a failed increase still blocks all deals on this order this tick); if increasing: `fee = ceil((new - old) * remainingAmount * 0.05)`, insufficient money → stop (price unchanged); `money -= fee`, log; `price += new - old`. Lowering is free (no refund).

### MARKET-EXTEND-ORDER — `extendOrder(orderId, addAmount)`
API (`reference/engine/src/game/market.js:174-191`): own order else INVALID_ARGS; `addAmount = parseInt`; `!addAmount || <= 0` → INVALID_ARGS; `price * addAmount * 0.05 > credits` → NOT_ENOUGH_RESOURCES; `pushByName(..., 50)` (ignored) → OK.
PROC-MARKET-EXTEND (`reference/engine/src/processor/global-intents/market.js:215-253`): own order; `addAmount > 0`; `fee = ceil(price * addAmount * 0.05)`; money check; `money -= fee`; `remainingAmount += add`, `totalAmount += add` (in memory too — affects deals this tick).

### MARKET-DEAL — `deal(orderId, amount, [yourRoomName])`
API (`reference/engine/src/game/market.js:108-153`):
1. `order = market.orders.all[orderId]` (active orders only, may be your own) → missing → `ERR_INVALID_ARGS`.
2. `amount = parseInt(amount)`; `!amount || amount < 0` → `ERR_INVALID_ARGS`.
3. Intershard resource: if order is BUY and `(user.resources[r]||0) < amount` → `ERR_NOT_ENOUGH_RESOURCES`.
   Otherwise: `!yourRoomName` → `ERR_INVALID_ARGS`; `terminal = first own terminal with room == yourRoomName` (userObjects); `cost = this.calcTransactionCost(amount, yourRoomName, order.roomName)` (computed before the terminal check; may throw for non-string names); no terminal → `ERR_NOT_OWNER`; `!terminal.store || store.energy < cost` → `ERR_NOT_ENOUGH_RESOURCES`; `terminal.cooldownTime > time` → `ERR_TIRED`; BUY order: (`r != energy` and `store[r]` missing or `< amount`) or (`r == energy` and `energy < amount + cost`) → `ERR_NOT_ENOUGH_RESOURCES`.
4. SELL order and `(user.money||0) < amount * order.price` (milli, uncapped `amount`) → `ERR_NOT_ENOUGH_RESOURCES`.
5. `pushByName('global','deal',{orderId, targetRoomName: yourRoomName, amount}, 10)` false → `ERR_FULL` (11th deal of the tick) else `OK`.
- No terminal RCL/active check. Cost ignores PWR_OPERATE_TERMINAL.

### PROC-MARKET-STAGE-ORDER
Global stage after all rooms (`reference/engine/src/processor/global-intents/market.js`), single-threaded:
1. Terminal sends (PROC-TERMINAL-SEND-GLOBAL), `:71-95`.
2. For each `users.intents` doc (DB order = runner completion order; **GOTCHA** nondeterministic in the original): `createOrder[]`, `changeOrderPrice[]`, `extendOrder[]`, `cancelOrder[]`, then collect `deal[]` (`:104-287`).
3. Terminal deals, stable-sorted by wrapped distance between `targetRoomName` and the order room (shortest first) (`:289-398`).
4. Intershard deals, `_.shuffle`d (Math.random) (`:400-495`).
5. Order maintenance over the orders loaded at stage start (`:497-589`).
All money/store/order changes are applied in memory as they happen (`bulk.update`/`bulk.inc` mutate), so later steps see earlier ones.

### PROC-MARKET-DEAL-TERMINAL
Collection (`:265-285`): drop if order missing or `_skip`, `amount <= 0`; intershard → direct deals; else the user must own the terminal in `targetRoomName`. Execution per deal (`:291-398`):
1. order terminal (`terminalsByRoom[order.roomName]`) and dealer terminal must exist.
2. dealer terminal `cooldownTime > time` → skip (so only the first successful deal per terminal per tick executes; later ones see the new cooldown).
3. SELL order: buyer = dealer terminal, seller = order terminal; BUY order: reversed.
4. `amount = min(deal.amount, order.remainingAmount)`; if seller has a user: `min(…, seller.store[r]||0)`; if buyer has a user: `min(…, max(0, buyer.storeCapacity - sum(buyer.store)))`; `!(amount > 0)` → skip.
5. `dealCost = amount * price`; if buyer has a user: `dealCost = min(dealCost, buyerUser.money||0)`, `amount = floor(dealCost/price)`, `dealCost = amount*price`, `amount == 0` → skip.
6. `executeTransfer(seller, buyer, r, amount, feePayer = dealer terminal, {order: {id, type, price/1000}})`; on success: seller user `money += dealCost` (log `market.sell`), buyer user `money -= dealCost` (log `market.buy`), order `amount -= n`, `remainingAmount -= n`, dealer terminal `cooldownTime = time + round(10 * operateEffect)`.
- Note `order.amount` may go negative transiently; maintenance recomputes it. The order owner's terminal gets no cooldown. Ownership of the order terminal vs `order.user` is not re-checked here.

### PROC-MARKET-DEAL-DIRECT (intershard resources)
`:400-495`: shuffled; buyer/seller are user docs; `amount = min(deal.amount, order.amount, order.remainingAmount, seller.resources[r]||0)`; `!amount || < 0` → skip; buyer money check is effectively skipped (`buyer.user` is undefined on user docs); moves money and `resources.<r>`; logs to `users.money` and `users.resources`; `order.amount/remainingAmount -= n`. **GOTCHA**: the in-memory buyer money is decremented twice (`bulk.inc` plus explicit `-=`), affecting later checks/log balances in the same tick, while the DB is decremented once. Low priority for a private world; implement faithfully only if account resources are supported.

### PROC-MARKET-EXPIRY
`:507-534`: for orders with a `user` and `Date.now() - createdTimestamp > MARKET_ORDER_LIFE_TIME` (30 days, **wall clock ms**): refund `remainingAmount * price * 0.05` (**not rounded**) to money, log, remove the order. Orders without `createdTimestamp` never expire (NaN comparison).

### PROC-MARKET-MAINTENANCE
`:497-589`, per order loaded at stage start: cancelled → remove; expiry (above); NPC orders (no user) untouched. Then:
- SELL: `avail = intershard ? user.resources[r] : (order-room terminal owned by order.user ? terminal.store[r]||0 : 0)`; `avail = min(avail, remainingAmount)`; active → if `!avail` set `{active:false, amount:0}` else update `amount` if changed; inactive → if `avail > 0` set `{active:true, amount: avail}`.
- BUY: `n = min(floor(money / price), remainingAmount)`; if the room has a terminal with any user: `n = min(n, max(0, storeCapacity - sum(store)))`; `active = (intershard || terminal owned by order.user) && n > 0`; write if `amount` or `active` changed.
- **GOTCHA**: fully filled orders (`remainingAmount 0`) are never deleted automatically; they stay inactive and keep counting toward the 300-order limit.

### MARKET-HISTORY-STATS / MARKET-TX-PURGE / MARKET-NPC-ORDERS (backend cron, private server)
- Hourly (`reference/backend/lib/cronjobs.js:14-26`): `calcMarketStats` (`:472-523`) rebuilds `market.stats` from `users.money` entries of type `market.sell` in the last 14 days, grouped by resource and server-local `YYYY-MM-DD`: `transactions = count`, `volume = Σ amount`, `avgPrice = round3(Σ change / Σ amount)`, `stddevPrice = round3(sqrt(Σ amount·(price - avg)² / Σ amount))`.
- `purgeTransactions` (`:443-470`): keeps each transaction if it is among the newest 100 of its sender or of its recipient.
- `recreateNpcOrders` (`:38-122`, every 5 min, Math.random): for unowned terminals in normal rooms, replaces that room's orders with a random sell order (mineral, 200 000 units, fixed milli-prices), 50 % an energy sell order, 25 % a mineral buy order; next refresh after `5000 * (0.8..1.2)` ticks. Private-server flavour only; make it a mod in chitin.

---

## 14. Intent summary (what each API call emits)

| API | Target key | Intent name | Payload | Limit |
|---|---|---|---|---|
| `Structure.destroy` | `room` (pushByName) | `destroyStructure` | `{roomName, id}` | none |
| `notifyWhenAttacked` | object id | `notifyWhenAttacked` | `{enabled}` | set |
| `Controller.unclaim` | controller | `unclaim` | `{}` | set |
| `Controller.activateSafeMode` | controller | `activateSafeMode` | `{}` | 1 per tick (last wins) |
| `Lab.runReaction` | lab | `runReaction` | `{lab1, lab2}` | set |
| `Lab.reverseReaction` | lab | `reverseReaction` | `{lab1, lab2}` | set |
| `Lab.boostCreep` | lab | `boostCreep` | `{id, bodyPartsCount}` | set |
| `Lab.unboostCreep` | lab | `unboostCreep` | `{id}` | set |
| `Link.transferEnergy` | link | `transfer` | `{id, amount, resourceType:'energy'}` | set |
| `Nuker.launchNuke` | nuker | `launchNuke` | `{roomName, x, y}` | set |
| `Observer.observeRoom` | observer | `observeRoom` | `{roomName}` | set |
| `PowerSpawn.processPower` | powerSpawn | `processPower` | `{}` | set |
| `Rampart.setPublic` | rampart | `setPublic` | `{isPublic}` | set |
| `Factory.produce` | factory | `produce` | `{resourceType}` | set |
| `Terminal.send` | terminal | `send` | `{resourceType, amount, targetRoomName, description}` | set |
| `Tower.attack/heal/repair` | tower | `attack`/`heal`/`repair` | `{id}` | set (priority heal>repair>attack) |
| `Spawn.spawnCreep` | spawn | `createCreep` | `{name, body, energyStructures, directions}` | set |
| `Spawn.createCreep` | spawn | `createCreep` | `{name, body}` | set |
| `Spawn.renewCreep` | spawn | `renewCreep` | `{id}` | set |
| `Spawn.recycleCreep` | spawn | `recycleCreep` | `{id}` | set |
| `Spawning.setDirections` | spawn | `setSpawnDirections` | `{directions}` | set |
| `Spawning.cancel` | spawn | `cancelSpawning` | `{}` | set |
| `market.createOrder` | `global` | `createOrder` | `{type, resourceType, price, totalAmount, roomName}` | ≤300 orders total |
| `market.cancelOrder` | `global` | `cancelOrder` | `{orderId}` | 50/tick (silent) |
| `market.changeOrderPrice` | `global` | `changeOrderPrice` | `{orderId, newPrice}` | 50/tick (silent) |
| `market.extendOrder` | `global` | `extendOrder` | `{orderId, addAmount}` | 50/tick (silent) |
| `market.deal` | `global` | `deal` | `{orderId, targetRoomName, amount}` | 10/tick (`ERR_FULL`) |

Read-only (no intent, no CPU surcharge): all properties, `isActive`, `canCreateCreep`, `spawnCreep({dryRun})`, `calcTransactionCost`, `getAllOrders`, `getHistory`, `getOrderById`.

---

## 15. PERF summary

- **Room-local (parallelisable per room)**: every structure intent except terminal sends/market; `launchNuke` writes a nuke into another room (queue as a cross-room insert applied before the next tick); observer intent only affects the next tick's visibility set.
- **Global serial stage**: terminal send execution, all market intents, order maintenance. With ~1000 players this is one pass over all terminals with pending sends, all orders and all users with market intents per tick; shard by nothing — keep it O(intents + orders) and do the deal distance sort once. The order book read path (`getAllOrders`) should be a shared immutable per-tick snapshot materialised into JS objects lazily per player (copy-on-first-access per resource key) to keep per-player mutation semantics.
- **Hot runtime paths**: `checkStructureAgainstController` (called by nearly every structure method and by player `isActive()` loops) — precompute per `(room, user, structureType)` ranks once per tick (COMMON-RCL-CHECK closed form); `Room.energyAvailable` precomputed; `getUniqueName` existence check should use a per-user creep-name hash set instead of scanning all objects; Store `_sum` cached.
- **Original slow spots to avoid**: full `_.find`/`_.filter` scans of `roomObjects` inside processors (rampart lookup per tower attack, container lookup per drop, spawn obstacle scan per direction, `_.filter(roomObjects,{type:'spawn'})` per spawn tick); one DB write per observer per tick; JSON deep copies of the order book per player.
- **Caches that are observable**: order book per runner process per tick, history per process for 3 h, room status data 60 s / 21 ticks. Chitin may refresh more often only where the difference is not observable by bots (history staleness is observable but harmless; room-status staleness changes `launchNuke` / `Game.map.getRoomStatus` results near expiry). The fresh-world absence of `roomStatusData` is deliberately not reproduced (RUNTIME-ROOM-STATUS-DATA).

---

## 16. Open questions

1. **Tie-break order for COMMON-RCL-CHECK / COMMON-OFF-FLAG / PROC-SPAWN-CHARGE**: the original uses room-object iteration order, which is LokiJS query order in the private server (`find({user})` in the runtime vs `find({room})` in the processor, both via binary indices — `reference/storage/lib/db.js:336-381`) and MongoDB natural order on the MMO. Whether the two orders always agree for the player's own objects was not verified. Proposal: define the canonical order as object creation order (id/insertion), identical in runtime and processor.
2. **Strict mode of engine game code**: `structures.js`/`market.js` have no `'use strict'`; whether the bundled runtime code executes sloppy (so a detached `const {deal} = Game.market; deal(...)` gets `this === globalThis`) or strict (`this === undefined` → TypeError) was not determined from the sources here.
3. **Official-server differences** not visible in these sources: whether the MMO's `getAllOrders` objects also expose `user`; whether MMO terminals/market use the same global-stage ordering; whether official `Math.random`-dependent behaviour (`getUniqueName`, intershard deal shuffle) needs to be reproducible (it is not seeded in the original, so exact replay is impossible anyway — recommend a seeded per-tick RNG).
4. **Crashing edge cases** (reverseReaction with a lab destroyed earlier the same tick; `Spawning.cancel` when the spawning creep is not in `roomObjects`; terminal deal whose order terminal is owned by a user not loaded in `usersById`): the original throws and loses that room's (or the global stage's) tick. This spec recommends treating them as silent no-ops; confirm that this divergence is acceptable.
5. ~~`roomStatusData` initial state~~ — resolved, see RUNTIME-ROOM-STATUS-DATA (the triggering user's tick fails, others get `{}` and `launchNuke` / `getRoomStatus` throw `TypeError`; chitin precomputes the data as a deliberate divergence).

