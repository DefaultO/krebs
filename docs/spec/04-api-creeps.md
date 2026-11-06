# 04 — Player API: Creep, PowerCreep, RoomObject & body helpers

Scope: the in-sandbox (player runtime) half of `Creep` and `PowerCreep`: object construction, every property
getter, every method (validation order, return codes, the exact intent emitted), intent bookkeeping (overwrite,
cancel, CPU), and the client-visible side effects (Memory, RoomVisual). The processor half (what an intent
*does*) belongs to the processor specs; it is summarised here only where it defines API-observable rules
(simultaneous-action priorities, ordering, sanitisation, power-creep global intents).

Citation format: `reference/<pkg>/<path>:<line>`. "raw" = the plain DB object `runtimeData.roomObjects[id]`
(for power creeps: the merged object, see PC-DATA-MERGE). `C.X` = constant from
`reference/common/lib/constants.js`. Return-code numbers: OK 0, ERR_NOT_OWNER -1, ERR_NO_PATH -2,
ERR_NAME_EXISTS -3, ERR_BUSY -4, ERR_NOT_FOUND -5, ERR_NOT_ENOUGH_RESOURCES -6, ERR_INVALID_TARGET -7,
ERR_FULL -8, ERR_NOT_IN_RANGE -9, ERR_INVALID_ARGS -10, ERR_TIRED -11, ERR_NO_BODYPART -12,
ERR_RCL_NOT_ENOUGH -14, ERR_GCL_NOT_ENOUGH -15, ERR_ACCESS_DENIED -16
(`reference/common/lib/constants.js:2-19`).

---

## 0. Inventory

| ID | Item | Kind | Primary source |
|---|---|---|---|
| API-INTENT-SET | `intents.set(id,name,data)` per-object intent slot, last-call-wins | infra | reference/driver/lib/runtime/runtime.js:66 |
| API-INTENT-PUSHBYNAME | `intents.pushByName('global',name,data,50)` list intents | infra | reference/driver/lib/runtime/runtime.js:82 |
| API-INTENT-REMOVE | `intents.remove` (cancelOrder) incl. CPU refund | infra | reference/driver/lib/runtime/runtime.js:92 |
| API-INTENT-CPU | 0.2 CPU per intent, free `say`/`pull`, getUsed vs billing | infra | reference/driver/lib/runtime/runtime.js:59-61,202; reference/driver/lib/runtime/make.js:98-111,193 |
| API-INTENT-SANITIZE | server-side field schema/coercion of every creep/PC intent | infra | reference/common/lib/system.js:4-114 |
| API-INTENT-ROUTING | grouping of intents by room / `global`; dropped ids | infra | reference/engine/src/utils.js:405-436 |
| API-NO-CLIENT-SIM | no local state simulation; complete list of client side effects | infra | reference/engine/src/game/creeps.js (whole) |
| API-GETTER-CACHE | `defineGameObjectProperties` lazy cache (`_name` own props) | infra | reference/engine/src/utils.js:509-533 |
| API-TOJSON | `toJSON` of game objects | infra | reference/engine/src/utils.js:535-544 |
| API-CLASS-LIFETIME | classes built once per global; module vars rebound per tick | infra | reference/engine/src/game/creeps.js:37-48 |
| API-STALE-OBJECT | behaviour of objects kept across ticks | infra | reference/engine/src/game/creeps.js:50-58 |
| API-ASSERT-TARGET | "serialized game object" exception | infra | reference/engine/src/game/game.js:122-126 |
| API-DATA-LOOKUP | `data(id)` throws on missing id | infra | reference/engine/src/game/creeps.js:50-58 |
| API-SAFEMODE-RULE | which methods check hostile safe mode, and with which code | infra | reference/engine/src/game/creeps.js (various) |
| ROOMOBJ-CTOR | `RoomObject(x,y,room,effects)` → `room`,`pos`,`effects` | class | reference/engine/src/game/rooms.js:1648-1661 |
| ROOMOBJ-EFFECTS | effects mapping/filtering, absent when raw has none | prop | reference/engine/src/game/rooms.js:1651-1658 |
| HELP-HAS-ACTIVE-BODYPART | `_hasActiveBodypart` (undefined body ⇒ true) | helper | reference/engine/src/game/creeps.js:20-31 |
| HELP-COUNT-ACTIVE-BODYPARTS | `_getActiveBodyparts` tail scan | helper | reference/engine/src/game/creeps.js:9-18 |
| HELP-FETCH-XY-ARGS | `fetchXYArguments` | helper | reference/engine/src/utils.js:34-63 |
| HELP-GET-DIRECTION | `getDirection(dx,dy)` | helper | reference/engine/src/utils.js:65-99 |
| HELP-POS-PREDICATES | `isNearTo`/`inRangeTo`/`isEqualTo`/`getDirectionTo` as used by checks | helper | reference/engine/src/game/rooms.js:1362-1394,1578-1584 |
| HELP-SERIALIZE-PATH | `serializePath` | helper | reference/engine/src/utils.js:555-574 |
| HELP-DESERIALIZE-PATH | `deserializePath` (throws) | helper | reference/engine/src/utils.js:576-613 |
| HELP-CALC-RESOURCES | `calcResources` | helper | reference/engine/src/utils.js:615-621 |
| HELP-CAPACITY-FOR-RESOURCE | `capacityForResource` | helper | reference/engine/src/utils.js:671-675 |
| HELP-CALC-NEEDED-GCL | `calcNeededGcl` | helper | reference/engine/src/utils.js:661-663 |
| HELP-STRUCT-VS-CONTROLLER | `checkStructureAgainstController` | helper | reference/engine/src/utils.js:456-507 |
| HELP-FREE-POWER-LEVELS | `calcFreePowerLevels` | helper | reference/engine/src/game/power-creeps.js:9-13 |
| GAME-CREEPS | `Game.creeps` construction & order | game | reference/engine/src/game/game.js:260-285 |
| GAME-CREEPS-DUP-NAME | duplicate creep name ⇒ automatic `suicide()` | game | reference/engine/src/game/game.js:273-279 |
| GAME-POWERCREEPS | `Game.powerCreeps` construction (incl. unspawned) | game | reference/engine/src/game/game.js:255-258,286-297 |
| GAME-OBJECT-REGISTRY | `register.creeps/powerCreeps/_objects`, `getObjectById` | game | reference/engine/src/game/game.js:171-173,271-293 |
| GAME-FIND-CREEP-FILTERS | FIND_* creep / power-creep cache predicates | game | reference/engine/src/game/game.js:15-36 |
| CREEP-CTOR | `Creep(id)` constructor, `Creep.prototype` chain | class | reference/engine/src/game/creeps.js:60-69 |
| CREEP-PROP-ID | `id` | prop | reference/engine/src/game/creeps.js:64 |
| CREEP-PROP-NAME | `name` | prop | reference/engine/src/game/creeps.js:72 |
| CREEP-PROP-BODY | `body` (raw array, shared) | prop | reference/engine/src/game/creeps.js:73 |
| CREEP-BODY-SHAPE | body part `{type,hits,boost}` incl. `boost:null` | data | reference/engine/src/processor/intents/spawns/create-creep.js:66-73 |
| CREEP-PROP-MY | `my` | prop | reference/engine/src/game/creeps.js:74 |
| CREEP-PROP-OWNER | `owner` | prop | reference/engine/src/game/creeps.js:75 |
| CREEP-PROP-SPAWNING | `spawning` | prop | reference/engine/src/game/creeps.js:76 |
| CREEP-PROP-TTL | `ticksToLive` | prop | reference/engine/src/game/creeps.js:77 |
| CREEP-PROP-FATIGUE | `fatigue` | prop | reference/engine/src/game/creeps.js:81 |
| CREEP-PROP-HITS | `hits` | prop | reference/engine/src/game/creeps.js:82 |
| CREEP-PROP-HITSMAX | `hitsMax` | prop | reference/engine/src/game/creeps.js:83 |
| CREEP-PROP-SAYING | `saying` | prop | reference/engine/src/game/creeps.js:84-92 |
| CREEP-PROP-STORE | `store` | prop | reference/engine/src/game/creeps.js:80 |
| CREEP-PROP-CARRY | `carry` (separate Store instance) | prop | reference/engine/src/game/creeps.js:79 |
| CREEP-PROP-CARRYCAPACITY | `carryCapacity` | prop | reference/engine/src/game/creeps.js:78 |
| CREEP-PROP-MEMORY | `memory` getter/setter | prop | reference/engine/src/game/creeps.js:95-120 |
| CREEP-PROP-POS-ROOM-EFFECTS | inherited `pos`,`room`,`effects` | prop | reference/engine/src/game/creeps.js:63 |
| CREEP-TOSTRING | `toString()` | method | reference/engine/src/game/creeps.js:122-124 |
| CREEP-MOVE-DIR | `move(direction)` | method | reference/engine/src/game/creeps.js:126-156 |
| CREEP-MOVE-CREEP | `move(creep)` (pull follower) | method | reference/engine/src/game/creeps.js:135-142 |
| CREEP-MOVEBYPATH | `moveByPath(path)` | method | reference/engine/src/game/creeps.js:305-333 |
| CREEP-MOVETO | `moveTo(...)` overall algorithm | method | reference/engine/src/game/creeps.js:158-303 |
| MOVETO-ARGS | argument forms & opts cloning | moveTo | reference/engine/src/game/creeps.js:168-187 |
| MOVETO-OPTS-DEFAULTS | `reusePath`/`serializeMemory`/visual defaults, caller mutation | moveTo | reference/engine/src/game/creeps.js:189-198 |
| MOVETO-EARLY-CHECKS | owner/spawning/fatigue/MOVE ordering | moveTo | reference/engine/src/game/creeps.js:162-178 |
| MOVETO-SAME-POS | already at target ⇒ OK, no side effects | moveTo | reference/engine/src/game/creeps.js:200-202 |
| MOVETO-REUSE | cached-path branch | moveTo | reference/engine/src/game/creeps.js:241-277 |
| MOVETO-MEM-FORMAT | `memory._move` format | moveTo | reference/engine/src/game/creeps.js:285-292 |
| MOVETO-NOPATHFINDING | `noPathFinding` | moveTo | reference/engine/src/game/creeps.js:279-281 |
| MOVETO-FINDPATH | `findPathTo` → `Room.findPath` → `_findPath2` call chain | moveTo | reference/engine/src/game/rooms.js:1396-1427,863-870,223-302 |
| MOVETO-PF-OPTIONS | which find options are honoured / ignored, cost defaults | moveTo | reference/engine/src/game/rooms.js:231-268; reference/driver/lib/path-finder.js:90-96 |
| MOVETO-COSTMATRIX | per-tick cached base cost matrix ("grid2") | moveTo | reference/engine/src/game/rooms.js:157-221 |
| MOVETO-COSTCALLBACK | `costCallback(roomName, matrix)` contract | moveTo | reference/engine/src/game/rooms.js:243-253 |
| MOVETO-PATH-POSTPROCESS | target append (range 0) & truncation to current room | moveTo | reference/engine/src/game/rooms.js:268-301 |
| MOVETO-VISUALIZE | `visualizePathStyle` poly output | moveTo | reference/engine/src/game/creeps.js:196-198,267-270,298-300; reference/engine/src/game/rooms.js:1184-1190 |
| MOVETO-RESULT | final move, full return-code / exception matrix | moveTo | reference/engine/src/game/creeps.js:294-302 |
| MOVETO-LEGACY-PF | behaviour after `PathFinder.use(false)` | moveTo | reference/engine/src/game/path-finder.js:67-75; reference/engine/src/game/rooms.js:873-970,1412-1425 |
| CREEP-PULL | `pull(target)` | method | reference/engine/src/game/creeps.js:1101-1121 |
| CREEP-HARVEST | `harvest(target)` source/mineral/deposit | method | reference/engine/src/game/creeps.js:335-402 |
| CREEP-DROP | `drop(resourceType, amount)` | method | reference/engine/src/game/creeps.js:404-426 |
| CREEP-TRANSFER | `transfer(target, resourceType, amount)` | method | reference/engine/src/game/creeps.js:428-491 |
| CREEP-TRANSFER-CONTROLLER | `transfer(controller, energy)` ⇒ `upgradeController` | method | reference/engine/src/game/creeps.js:455-457 |
| CREEP-WITHDRAW | `withdraw(target, resourceType, amount)` | method | reference/engine/src/game/creeps.js:493-564 |
| CREEP-PICKUP | `pickup(target)` | method | reference/engine/src/game/creeps.js:566-587 |
| CREEP-BUILD | `build(target)` incl. tile-blocking scan | method | reference/engine/src/game/creeps.js:762-811 |
| CREEP-REPAIR | `repair(target)` | method | reference/engine/src/game/creeps.js:734-760 |
| CREEP-DISMANTLE | `dismantle(target)` | method | reference/engine/src/game/creeps.js:1024-1055 |
| CREEP-UPGRADE-CONTROLLER | `upgradeController(target)` | method | reference/engine/src/game/creeps.js:922-959 |
| CREEP-CLAIM-CONTROLLER | `claimController(target)` incl. per-tick claim counter | method | reference/engine/src/game/creeps.js:839-886 |
| CREEP-RESERVE-CONTROLLER | `reserveController(target)` | method | reference/engine/src/game/creeps.js:961-994 |
| CREEP-ATTACK-CONTROLLER | `attackController(target)` | method | reference/engine/src/game/creeps.js:888-920 |
| CREEP-SIGN-CONTROLLER | `signController(target, text)` | method | reference/engine/src/game/creeps.js:1080-1099 |
| CREEP-GENERATE-SAFE-MODE | `generateSafeMode(target)` | method | reference/engine/src/game/creeps.js:1057-1078 |
| CREEP-ATTACK | `attack(target)` | method | reference/engine/src/game/creeps.js:593-624 |
| CREEP-RANGED-ATTACK | `rangedAttack(target)` | method | reference/engine/src/game/creeps.js:626-656 |
| CREEP-RANGED-MASS-ATTACK | `rangedMassAttack()` | method | reference/engine/src/game/creeps.js:658-676 |
| CREEP-HEAL | `heal(target)` | method | reference/engine/src/game/creeps.js:678-704 |
| CREEP-RANGED-HEAL | `rangedHeal(target)` | method | reference/engine/src/game/creeps.js:706-732 |
| CREEP-SAY | `say(message, isPublic)` | method | reference/engine/src/game/creeps.js:826-837 |
| CREEP-SUICIDE | `suicide()` | method | reference/engine/src/game/creeps.js:813-824 |
| CREEP-NOTIFY-WHEN-ATTACKED | `notifyWhenAttacked(enabled)` | method | reference/engine/src/game/creeps.js:996-1014 |
| CREEP-CANCEL-ORDER | `cancelOrder(name)` | method | reference/engine/src/game/creeps.js:1016-1022 |
| CREEP-GET-ACTIVE-BODYPARTS | `getActiveBodyparts(type)` | method | reference/engine/src/game/creeps.js:589-591 |
| PC-CTOR | `PowerCreep(id)` constructor (no pos when unspawned) | class | reference/engine/src/game/power-creeps.js:34-43 |
| PC-DATA-MERGE | `data(id)` = merge(user record, room object) | infra | reference/engine/src/game/power-creeps.js:15-17 |
| PC-PROP-NAME | `name` | prop | reference/engine/src/game/power-creeps.js:46 |
| PC-PROP-MY | `my` | prop | reference/engine/src/game/power-creeps.js:47 |
| PC-PROP-OWNER | `owner` | prop | reference/engine/src/game/power-creeps.js:48 |
| PC-PROP-LEVEL | `level` | prop | reference/engine/src/game/power-creeps.js:49 |
| PC-PROP-CLASSNAME | `className` | prop | reference/engine/src/game/power-creeps.js:50 |
| PC-PROP-HITS | `hits` | prop | reference/engine/src/game/power-creeps.js:52 |
| PC-PROP-HITSMAX | `hitsMax` | prop | reference/engine/src/game/power-creeps.js:51 |
| PC-PROP-SHARD | `shard` | prop | reference/engine/src/game/power-creeps.js:53 |
| PC-PROP-SPAWN-COOLDOWN-TIME | `spawnCooldownTime` (wall clock) | prop | reference/engine/src/game/power-creeps.js:54 |
| PC-PROP-DELETE-TIME | `deleteTime` (wall clock) | prop | reference/engine/src/game/power-creeps.js:55 |
| PC-PROP-POWERS | `powers` `{level, cooldown}` | prop | reference/engine/src/game/power-creeps.js:56-59 |
| PC-PROP-SAYING | `saying` | prop | reference/engine/src/game/power-creeps.js:60-68 |
| PC-PROP-STORE | `store` | prop | reference/engine/src/game/power-creeps.js:70 |
| PC-PROP-CARRY | `carry` | prop | reference/engine/src/game/power-creeps.js:69 |
| PC-PROP-CARRYCAPACITY | `carryCapacity` | prop | reference/engine/src/game/power-creeps.js:71 |
| PC-PROP-TTL | `ticksToLive` (NaN when unspawned) | prop | reference/engine/src/game/power-creeps.js:72 |
| PC-PROP-MEMORY | `memory` (Memory.powerCreeps) | prop | reference/engine/src/game/power-creeps.js:75-100 |
| PC-PROP-POS-ROOM-EFFECTS | `pos`,`room` (spawned only), `effects` never set | prop | reference/engine/src/game/power-creeps.js:36-38 |
| PC-TOSTRING | `toString()` | method | reference/engine/src/game/power-creeps.js:102-104 |
| PC-CREATE | static `PowerCreep.create(name, className)` | method | reference/engine/src/game/power-creeps.js:395-410 |
| PC-SPAWN | `spawn(powerSpawn)` (+ Memory init) | method | reference/engine/src/game/power-creeps.js:162-189 |
| PC-SUICIDE | `suicide()` (global intent) | method | reference/engine/src/game/power-creeps.js:191-202 |
| PC-DELETE | `delete(cancel)` | method | reference/engine/src/game/power-creeps.js:204-215 |
| PC-UPGRADE | `upgrade(power)` | method | reference/engine/src/game/power-creeps.js:217-244 |
| PC-RENAME | `rename(name)` | method | reference/engine/src/game/power-creeps.js:356-373 |
| PC-USE-POWER | `usePower(power, target)` | method | reference/engine/src/game/power-creeps.js:246-293 |
| PC-ENABLE-ROOM | `enableRoom(controller)` | method | reference/engine/src/game/power-creeps.js:295-317 |
| PC-RENEW | `renew(target)` | method | reference/engine/src/game/power-creeps.js:319-340 |
| PC-MOVE | `move` (delegates to Creep) | method | reference/engine/src/game/power-creeps.js:106-111 |
| PC-MOVETO | `moveTo` (delegates; uses Memory.powerCreeps) | method | reference/engine/src/game/power-creeps.js:113-118 |
| PC-MOVEBYPATH | `moveByPath` (delegates) | method | reference/engine/src/game/power-creeps.js:120-125 |
| PC-TRANSFER | `transfer` (delegates; controller+energy throws) | method | reference/engine/src/game/power-creeps.js:127-132 |
| PC-WITHDRAW | `withdraw` (delegates) | method | reference/engine/src/game/power-creeps.js:134-139 |
| PC-DROP | `drop` (delegates) | method | reference/engine/src/game/power-creeps.js:141-146 |
| PC-PICKUP | `pickup` (delegates) | method | reference/engine/src/game/power-creeps.js:148-153 |
| PC-SAY | `say` (delegates) | method | reference/engine/src/game/power-creeps.js:155-160 |
| PC-NOTIFY-WHEN-ATTACKED | `notifyWhenAttacked(enabled)` | method | reference/engine/src/game/power-creeps.js:375-393 |
| PC-CANCEL-ORDER | `cancelOrder(name)` | method | reference/engine/src/game/power-creeps.js:342-354 |
| PC-ABSENT-MEMBERS | members a PowerCreep does NOT have | class | reference/engine/src/game/power-creeps.js:42 |
| PC-POWER-INFO | `POWER_INFO` table semantics | data | reference/common/lib/constants.js:854-1016 |
| PROC-CREEP-PRIORITY | simultaneous-action exclusion table | processor | reference/engine/src/processor/intents/creeps/intents.js:3-23 |
| PROC-CREEP-ORDER | creep intent execution order | processor | reference/engine/src/processor/intents/creeps/intents.js:15-31 |
| PROC-PC-ORDER | power-creep intent execution order | processor | reference/engine/src/processor/intents/power-creeps/intents.js:3-13 |
| PROC-OWNERSHIP-FILTER | intents on objects not owned by issuer are skipped | processor | reference/engine/src/processor.js:248 |
| PROC-NOTIFY | `notifyWhenAttacked` application | processor | reference/engine/src/processor.js:313-315 |
| PROC-ACTIONLOG-RESET | per-tick `actionLog` reset (drives `saying`) | processor | reference/engine/src/processor.js:48-65,151-160 |
| PROC-SAY | say truncation to 10 UTF-16 units | processor | reference/engine/src/processor/intents/creeps/say.js:6-21 |
| PROC-SIGN | sign text / empty-string removal | processor | reference/engine/src/processor/intents/creeps/signController.js:23-28 |
| PROC-MOVE-INTENT | move intent decoding (direction vs id) | processor | reference/engine/src/processor/intents/creeps/move.js:7-38 |
| PROC-CREEP-AGETIME | when `ageTime` (⇒ ticksToLive) appears | processor | reference/engine/src/processor/intents/creeps/tick.js:80-96 |
| PROC-PC-CREATE | createPowerCreep global intent | processor | reference/engine/src/processor/global-intents/power/createPowerCreep.js:7-40 |
| PROC-PC-SPAWN | spawnPowerCreep global intent | processor | reference/engine/src/processor/global-intents/power/spawnPowerCreep.js:7-42 |
| PROC-PC-DIE | suicide / death of a power creep | processor | reference/engine/src/processor/global-intents/power/_diePowerCreep.js:6-58 |
| PROC-PC-DELETE | deletePowerCreep global intent | processor | reference/engine/src/processor/global-intents/power/deletePowerCreep.js:7-28 |
| PROC-PC-UPGRADE | upgradePowerCreep global intent | processor | reference/engine/src/processor/global-intents/power/upgradePowerCreep.js:7-65 |
| PROC-PC-RENAME | renamePowerCreep global intent | processor | reference/engine/src/processor/global-intents/power/renamePowerCreep.js:7-22 |
| PROC-PC-USEPOWER | room-level usePower checks (summary) | processor | reference/engine/src/processor/intents/power-creeps/usePower.js:6-310 |
| PROC-PC-RENEW-ENABLE | renew / enableRoom processing (summary) | processor | reference/engine/src/processor/intents/power-creeps/renew.js:6-23; reference/engine/src/processor/intents/power-creeps/enableRoom.js:6-22 |

Inventory count: 157 items.

---

## 1. Shared infrastructure

### API-INTENT-SET
`intents.set(id, name, data)` (`reference/driver/lib/runtime/runtime.js:66-72`):
- `list[id]` is a null-prototype map; `list[id][name] = data` **replaces** any previous intent of the same name
  on the same object (last call in the tick wins). Different names on the same object coexist; conflicts
  among them are resolved by the processor (PROC-CREEP-PRIORITY), never client-side.
- CPU: `cpu += 0.2` only if `name` is not free and `list[id][name]` was falsy before (re-issuing the same method
  is not charged again).
- **GOTCHA**: because nothing is simulated locally (API-NO-CLIENT-SIM), a second `transfer` from the same creep
  in one tick *replaces* the first; a creep can carry exactly one intent per method name per tick.

### API-INTENT-PUSHBYNAME
`pushByName(id, name, data, maxLen)` (`reference/driver/lib/runtime/runtime.js:82-91`): appends to `list[id][name]` (array). Returns
`false` without pushing when `maxLen` reached; else pushes and charges 0.2 CPU (always, no free list).
All PowerCreep account-level methods use `id = 'global'`, `maxLen = 50` (PC-CREATE, PC-SPAWN, PC-SUICIDE,
PC-DELETE, PC-UPGRADE, PC-RENAME). **GOTCHA**: callers ignore the boolean — the API still returns `OK` when the
51st global intent of that name is silently dropped.

### API-INTENT-REMOVE
`remove(id, name)` (`reference/driver/lib/runtime/runtime.js:92-99`): if `list[id][name]` truthy → `delete`, `cpu -= 0.2`, return true; else
false. **GOTCHA**: the 0.2 refund also applies to `say`/`pull`, which were never charged, so
`cancelOrder('say')` lowers `Game.cpu.getUsed()` by 0.2. Only works for `set`-style slots of that object;
global (`pushByName`) intents cannot be cancelled.

### API-INTENT-CPU
- Free methods: `say`, `pull` (`reference/driver/lib/runtime/runtime.js:61`).
- `Game.cpu.getUsed() = isolateCpu + intents.cpu - startTime` (`reference/driver/lib/runtime/runtime.js:202-204`), so intents count towards
  in-game CPU readings immediately, including intents issued during Game construction (GAME-CREEPS-DUP-NAME).
- Reported `usedTime` = `ceil(cpu + intents.cpu)` (`reference/driver/lib/runtime/runtime.js:271`), but **bucket billing** recomputes
  `0.2 * count(final intents)` excluding `say`/`pull`, arrays counted per element
  (`reference/driver/lib/runtime/make.js:98-111,193-194`). So a cancelled intent is never billed, and a
  refunded-but-free `say` cancel does not reduce billing below 0. Chitin must keep both numbers.

### API-INTENT-SANITIZE
After the player run, every object intent is rebuilt from a fixed schema (`reference/common/lib/system.js:16-82`,
applied by `sanitizeIntent`, `reference/common/lib/system.js:84-93`): output contains exactly the schema fields, in schema order, each passed
through a transform (`reference/common/lib/system.js:4-14`). Unknown names are dropped; unknown fields dropped. Transforms:
`string`: `""+v` (undefined ⇒ `"undefined"`); `number`: `parseInt(v)` (ToString first: `1.5`⇒1, `1e21`⇒1,
`1e-7`⇒1, `-0.5`⇒-0, undefined⇒NaN); `boolean`: `!!v`; `userString`: `(""+(v||"")).substring(0,100)`.
Relevant schemas (field order is the output order):

| intent | fields |
|---|---|
| move | id:string, direction:number |
| harvest, pickup, pull, rangedAttack, rangedHeal, dismantle, claimController, reserveController, attackController, upgradeController, generateSafeMode, enableRoom, renew | id:string |
| attack, heal, build, repair | id:string, x:number, y:number |
| transfer, withdraw | id:string, amount:number, resourceType:string |
| drop | amount:number, resourceType:string |
| say | message:userString, isPublic:boolean |
| signController | id:string, sign:userString |
| suicide, rangedMassAttack | (none) |
| notifyWhenAttacked | enabled:boolean |
| usePower | power:number, id:string |
| createPowerCreep | name:userString, className:string |
| spawnPowerCreep | id:string, name:userString |
| suicidePowerCreep | id:string |
| deletePowerCreep | id:string, cancel:boolean |
| upgradePowerCreep | id:string, power:number |
| renamePowerCreep | id:string, name:userString |

**GOTCHA**: the client validates the *unsanitised* value. E.g. `transfer(t,'energy',0.5)` passes all
client checks (0.5 is truthy) and reaches the processor as `amount: 0`; `move({direction:1.5})` → `1`;
`usePower(PWR_GENERATE_OPS)` produces `id: "undefined"`. Moddability: `customIntentTypes` extends the schema
(`reference/common/lib/system.js:95-111`).

### API-INTENT-ROUTING
`storeIntents` (`reference/engine/src/utils.js:405-436`): key `notify` / `room` / `global` handled specially;
every other key is an object id looked up in `userObjects[i] || roomObjects[i]`; if not found the intent set
is silently dropped; else stored at `intents[object.room][id]`. So an intent "issued" through a foreign
creep wrapper (e.g. CREEP-SIGN-CONTROLLER has no owner check) is stored and later skipped by
PROC-OWNERSHIP-FILTER, but still charged CPU.

### API-NO-CLIENT-SIM
Vanilla never mutates game state locally: after `creep.transfer(...)` returns OK, `creep.store`, `pos`, target
hits etc. are unchanged for the rest of the tick. The complete list of client-observable side effects of the
Creep/PowerCreep API:
1. `Memory` access/writes: `creep.memory` getter creates `Memory.creeps[name] = {}` (CREEP-PROP-MEMORY);
   `moveTo` reads/writes/deletes `memory._move` (MOVETO-REUSE, MOVETO-MEM-FORMAT); `PowerCreep.spawn`
   initialises `Memory.powerCreeps[name]` (PC-SPAWN). Any `Memory` access triggers the lazy `JSON.parse`
   of RawMemory (`reference/engine/src/game/game.js:479-500`).
2. `RoomVisual` output from `moveTo` (MOVETO-VISUALIZE), which can throw at the 500 KB per-room limit.
3. `controllersClaimedInTick` counter (CREEP-CLAIM-CONTROLLER).
4. `intents.cpu` (API-INTENT-CPU).
5. Deprecation `console.log` for `avoid`/`ignore` options, once per message per tick
   (`reference/engine/src/game/game.js:113-120`, `reference/engine/src/game/rooms.js:231-238`).
6. Getter caches on the instance (API-GETTER-CACHE).

### API-GETTER-CACHE
`defineGameObjectProperties(proto, dataFn, props)` (`reference/engine/src/utils.js:509-533`) defines on the **prototype**, for each
prop, an enumerable, non-configurable accessor:
`get(){ if(!this['_'+name]) this['_'+name] = fn(dataFn(this.id), this.id); return this['_'+name]; }`, no
setter (`opts.canSet` unused for creeps).
- Cache is an ordinary own enumerable property `_name` on the instance; falsy results (0, false, undefined,
  NaN, '') are recomputed on every access.
- **GOTCHA**: user code can poison the cache: `creep._hits = 5; creep.hits === 5`. `Object.keys(creep)` grows as
  props are read. Assigning `creep.hits = 1` is ignored in sloppy code, TypeError in strict user code
  (accessor without setter).
- PERF: every getter is O(1) after first read; the original evaluates `new Store(o)` lazily.

### API-TOJSON
`proto.toJSON` (`reference/engine/src/utils.js:535-544`): `for (i in this)` over own + inherited enumerable keys, skipping keys
starting with `_` and `toJSON`/`toString`; copies `this[i]`. Methods are enumerable (assigned to the prototype),
so they are copied but dropped by `JSON.stringify` as functions; `memory` is non-enumerable (defineProperty
default). Creep own key order: `room`, `pos`, [`effects`], `id`; then prototype getters in definition order
(`name, body, my, owner, spawning, ticksToLive, carryCapacity, carry, store, fatigue, hits, hitsMax, saying`).

### API-CLASS-LIFETIME
`make()` stores `runtimeData/intents/register/globals` in **module-level variables every tick**, then returns
early if `globals.Creep` already exists (`reference/engine/src/game/creeps.js:37-48`; same in `reference/engine/src/game/power-creeps.js:23-32`). Hence:
- `Creep`/`PowerCreep` constructors and prototypes are created once per sandbox global and persist across ticks
  (user prototype extensions survive while the global lives).
- All methods read the *current tick's* data through the module variables.
- `controllersClaimedInTick = 0` is reset every tick *before* the early return (`reference/engine/src/game/creeps.js:44`).

### API-STALE-OBJECT
A Creep instance kept in a global variable across ticks: own props (`pos`, `room`, `id`) are frozen at the old
tick; already-read getters return cached old values; never-read getters call `data(this.id)` against the
*current* tick (current value if the creep still exists, else throw "Could not find an object with ID ...").
Methods use the stale `this.pos/this.room` but current raw data. **GOTCHA** for 1:1: reproduce this mixed
staleness (or document divergence).

### API-ASSERT-TARGET
`register.assertTargetObject(obj)` (`reference/engine/src/game/game.js:122-126`): if `obj` is a plain object (`_.isPlainObject`) whose
`id` is a string of length exactly 24, throw `Error("It seems you're trying to use a serialized game object
stored in Memory which is not allowed. Please use \`Game.getObjectById\` to retrieve a live object reference
instead.")`. Called right before most `ERR_INVALID_TARGET` returns. **GOTCHA**: private-server ids are 15 chars
(`reference/storage/lib/db.js:318-328`: 4 hex + 7 hex of `Date.now()` + 4 hex), so the throw never fires
there but does on official (24-char ObjectId) servers; chitin's id format decides this behaviour.

### API-DATA-LOOKUP
`reference/engine/src/game/creeps.js:50-58`: `data(id)` throws `"This creep doesn't exist yet"` if `!id`, `"Could not find an object with
ID "+id` if the id is not in `runtimeData.roomObjects`. It is used for `this.id` *and* for target ids inside
`transfer`/`withdraw`/`build` etc. **GOTCHA**: `creep.transfer(unspawnedPowerCreep, ...)` throws (the PC is in
`register.powerCreeps` but not in `roomObjects`).

### API-SAFEMODE-RULE
"Hostile safe mode" = `this.room.controller && !this.room.controller.my && this.room.controller.safeMode`
(controller of the *actor's* room; `safeMode` getter is remaining ticks or undefined,
`reference/engine/src/game/structures.js:184`). Client behaviour:
- returns **ERR_NO_BODYPART** (undocumented): attack, rangedAttack, rangedMassAttack, heal, rangedHeal,
  claimController, attackController, dismantle.
- returns ERR_NOT_OWNER: withdraw. PowerCreep.usePower: ERR_INVALID_ARGS. enableRoom: ERR_INVALID_TARGET when
  `target.safeMode && !target.my`.
- not checked client-side: move/moveTo/moveByPath, harvest, transfer, drop, pickup, build (but see blocking
  creeps rule), repair, upgradeController, reserveController, generateSafeMode, signController, pull, say.
Position of the check inside each method differs (see each item) and is part of 1:1.

---

## 2. RoomObject & helpers

### ROOMOBJ-CTOR
`RoomObject(x, y, room, effects)` (`reference/engine/src/game/rooms.js:1648-1659`): `this.room = register.rooms[room]` (Room or
undefined), `this.pos = new RoomPosition(x, y, room)`, `this.effects` only if `effects` truthy. Plain own
enumerable props in that order. `RoomObject.prototype` has no members of its own.

### ROOMOBJ-EFFECTS
`effects = raw.effects.map(i => ({power: i.power, effect: i.effect, level: i.level, ticksRemaining: i.endTime -
time})).filter(ticksRemaining > 0)` (lodash chain; works for array or object-valued raw effects).
**GOTCHA**: when the raw object has no `effects` field the property is **absent (`undefined`)**, not `[]`
(most creeps). Natural effects (`EFFECT_INVULNERABILITY` 1001, `EFFECT_COLLAPSE_TIMER` 1002) have
`power`/`level` undefined; power effects have `effect == power`. Undocumented `power` key is present.

### HELP-HAS-ACTIVE-BODYPART
`_hasActiveBodypart(body, type)` (`reference/engine/src/game/creeps.js:20-31`): if `!body` → **true** (this is why power creeps pass
`MOVE` checks); scan from last index down; stop at the first part with `hits <= 0`; true if a part of `type`
seen. Relies on damage being applied front-first (PROC body recalc fills hits from the tail:
`reference/engine/src/processor/intents/creeps/_recalc-body.js:10-18`), so active parts are always a suffix.
Reads `this.body` — the raw, user-mutable array (CREEP-PROP-BODY).

### HELP-COUNT-ACTIVE-BODYPARTS
`_getActiveBodyparts(body, type)` (`reference/engine/src/game/creeps.js:9-18`): same tail scan, returns the count. Boosts ignored. Throws
on undefined body.

### HELP-FETCH-XY-ARGS
`fetchXYArguments(a, b)` (`reference/engine/src/utils.js:34-63`):
- if `b` undefined or not a number: if `a` not an object → `[undefined×3]`; if `a instanceof RoomPosition` → its
  x,y,roomName; then if `a.pos instanceof RoomPosition` → overrides with `a.pos`.
- else (b is a number): `x = a` (no type check), `y = b`, roomName undefined.
- NaN x/y → undefined.

### HELP-GET-DIRECTION
`getDirection(dx, dy)` (`reference/engine/src/utils.js:65-99`): `|dx| > 2|dy|` → RIGHT/LEFT; `|dy| > 2|dx|` → BOTTOM/TOP; else the
diagonal by signs. `(0,0)` → `undefined`. Works for non-adjacent deltas (approximate octant).

### HELP-POS-PREDICATES
As used by the checks (`reference/engine/src/game/rooms.js`):
- `isNearTo(t)` (`reference/engine/src/game/rooms.js:1378-1381`): Chebyshev ≤ 1 and (`!roomName` or same room) — a plain `{x,y}` with no room
  passes the room test.
- `inRangeTo(t, r)` (`reference/engine/src/game/rooms.js:1362-1376`): with 2 args uses `t.pos || t`; requires `roomName == this.roomName`
  (a `{x,y}` without roomName fails).
- `isEqualTo(p)` (`reference/engine/src/game/rooms.js:1578-1584`): `__packedPos` compare if arg has it, else x/y(+room) compare.
- `getDirectionTo(t)` (`reference/engine/src/game/rooms.js:1383-1394`): same room → `getDirection`; other room → world-coordinate delta.

### HELP-SERIALIZE-PATH
`serializePath(path)` (`reference/engine/src/utils.js:555-574`): throws `Error('path is not an array')` if not array; `''` for
empty; throws if `path[0].x<0 || path[0].y<0`; output = 2-digit x of `path[0]` + 2-digit y of `path[0]`
(zero-padded only when ≤ 9) + every element's `direction` concatenated. Coordinates of elements after the first
are not stored. Example: steps (10,5,dir 3),(11,6,dir 4) → `"100534"`.

### HELP-DESERIALIZE-PATH
`deserializePath(str)` (`reference/engine/src/utils.js:576-613`): throws `Error('\`path\` is not a string')` for non-strings; `[]`
for `''`; `x=parseInt(str[0..2])`, `y=parseInt(str[2..4])`, NaN ⇒ throw "`path` is not a valid serialized path
string"; for each char from index 4: `direction=parseInt(ch)`; invalid (0, 9, non-digit) ⇒ same throw;
`dx,dy` from direction; from the 2nd step on `x+=dx, y+=dy`; push `{x, y, dx, dy, direction}` (key order).

### HELP-CALC-RESOURCES
`calcResources(o)` (`reference/engine/src/utils.js:615-621`): `_.sum(o.store)` if `o.store`, else legacy per-resource fields.
Called both with raw objects and with game objects (`target`), where it sums the Store's own enumerable keys.

### HELP-CAPACITY-FOR-RESOURCE
`capacityForResource(o, r)` (`reference/engine/src/utils.js:671-675`): `o.storeCapacityResource && o.storeCapacityResource[r]` if
truthy, else `max(0, (o.storeCapacity||0) - sum(o.storeCapacityResource))`.
Consequences: creep with no CARRY ⇒ 0 for every resource; spawn/extension ⇒ 0 for non-energy; controller
⇒ 0.

### HELP-CALC-NEEDED-GCL
`calcNeededGcl(n) = C.GCL_MULTIPLY * (n-1) ** C.GCL_POW` (1e6, 2.4) (`reference/engine/src/utils.js:661-663`).

### HELP-STRUCT-VS-CONTROLLER
`checkStructureAgainstController(obj, roomObjects, controller)` (`reference/engine/src/utils.js:456-507`): no `obj.user` ⇒ true;
no controller / level < 1 / different owner ⇒ false; `allowed = CONTROLLER_STRUCTURES[type][level]`; 0 ⇒ false;
if `CONTROLLER_STRUCTURES[type][8] === 1` ⇒ true (single-instance types incl. powerSpawn, extractor);
else counts same-type same-owner structures strictly closer (Chebyshev to controller), ties broken by
object-iteration order with identity check `object === compareObj`. PERF: O(objects) per call except
single-instance types.

### HELP-FREE-POWER-LEVELS
`calcFreePowerLevels()` (`reference/engine/src/game/power-creeps.js:9-13`): `floor(((user.power||0)/C.POWER_LEVEL_MULTIPLY) **
(1/C.POWER_LEVEL_POW)) - (count(userPowerCreeps) + sum(userPowerCreeps.level))` (1000, 2). Each power creep
costs 1 level for existing plus its level.

---

## 3. Game construction

### GAME-CREEPS
`makeGameObject` (`reference/engine/src/game/game.js:260-285`), per tick per player: iterate `runtimeData.roomObjects` (key order:
other players' objects of visible rooms in DB order, then the player's own objects — `_.extend` in
`reference/driver/lib/runtime/data.js:209`); skip `temp` objects; for `type == 'creep'`:
`register._objects[id] = new Creep(id)`; add to `register.creeps`, `register.byRoom[room].creeps` and the
spatial index (`index = x*50+y`, `reference/engine/src/game/game.js:38-48`); if `runtimeData.userObjects[id]` (own): add to
`Game.creeps[name]` (see dup rule); add to FIND caches. Spawning creeps are included in `Game.creeps`,
`getObjectById` and `lookAt`, but excluded from FIND_* (GAME-FIND-CREEP-FILTERS).
**GOTCHA**: `Game.creeps` is an ordinary object: names that are canonical array indices ("7", "123") enumerate
first in ascending numeric order, the rest in insertion order. PERF: one wrapper per visible creep per player
per tick (vanilla is eager; chitin may be lazy if identity/ordering is preserved).

### GAME-CREEPS-DUP-NAME
If `Game.creeps[name]` already exists when a second own creep with that name is met, `register.creeps[id]
.suicide()` is called on the later one (iteration order) and it is not put in `Game.creeps` (`reference/engine/src/game/game.js:273-279`).
That intent costs 0.2 CPU; if that creep is spawning, `suicide` returns ERR_BUSY and nothing happens.

### GAME-POWERCREEPS
`reference/engine/src/game/game.js:255-258`: for every `id` in `runtimeData.userPowerCreeps` (the player's `users.power_creeps`
records, spawned or not; `reference/driver/lib/runtime/data.js:220`): `register.powerCreeps[id] = new PowerCreep(id)`,
`Game.powerCreeps[name] = it` (duplicate names: later overwrites, no suicide). Then in the room-object loop
(`reference/engine/src/game/game.js:286-297`) each `powerCreep` room object reuses the own instance or creates a foreign one, is put into
`register._objects`, registers, spatial index and FIND_POWER_CREEPS / MY / HOSTILE caches.

### GAME-OBJECT-REGISTRY
`Game.getObjectById(id) = register._objects[id] || null` (`reference/engine/src/game/game.js:171-173`). Unspawned power creeps are
**not** in `_objects` ⇒ `getObjectById` returns null for them. Same instance identity across `Game.creeps`,
`register.creeps`, `getObjectById`, `room.find`, `lookAt`.

### GAME-FIND-CREEP-FILTERS
`reference/engine/src/game/game.js:15-28`: FIND_CREEPS `!spawning`, FIND_MY_CREEPS `!spawning && my`, FIND_HOSTILE_CREEPS
`!spawning && !my`, FIND_MY_POWER_CREEPS `my`, FIND_HOSTILE_POWER_CREEPS `!my`, FIND_POWER_CREEPS all.
Evaluated once at construction; result arrays are cloned per `find` call (Room spec).

---

## 4. Creep properties

### CREEP-CTOR
`Creep(id)` (`reference/engine/src/game/creeps.js:60-66`): if `id` truthy: `RoomObject.call(this, raw.x, raw.y, raw.room, raw.effects)`
then `this.id = id`; else returns an empty instance (getters then throw "This creep doesn't exist yet").
`Creep.prototype = Object.create(RoomObject.prototype)`, `constructor = Creep` (enumerable). Exposed as
non-writable enumerable `globals.Creep` (`reference/engine/src/game/creeps.js:1123`).

### CREEP-PROP-ID / CREEP-PROP-NAME
`id`: own plain property (string). `name`: `raw.name`.

### CREEP-PROP-BODY
`body = raw.body` — **the raw array itself** (not a copy); shared by all readers in the tick; user mutation
changes subsequent client checks (`_hasActiveBodypart(this.body, ...)`). No boost multipliers applied.

### CREEP-BODY-SHAPE
Each part `{type, hits}` plus optional `boost`:
- spawn-created: `{type, hits: 100}`, no `boost` key (`reference/engine/src/processor/intents/spawns/create-creep.js:66-73`).
- boosted by lab: `boost = mineralType` (`reference/engine/src/processor/intents/labs/boost-creep.js:42`).
- after renew of a boosted creep or unboost: **`boost: null`** on every part (`reference/engine/src/processor/intents/spawns/renew-creep.js:44-47`,
  `reference/engine/src/processor/intents/labs/unboost-creep.js:28`). Docs say `string | undefined`; code yields `null` too.
- NPC stronghold creeps may be created boosted (`reference/engine/src/processor/intents/invader-core/create-creep.js:20-27`).
- `hits` per part 0..100, filled from the tail (`reference/engine/src/processor/intents/creeps/_recalc-body.js:10-18`). Keys starting with `_` (e.g. `_oldHits`)
  never reach the player (`reference/driver/lib/bulk.js:6-23`).

### CREEP-PROP-MY / CREEP-PROP-OWNER
`my = raw.user == runtimeData.user._id` (loose). `owner = {username: runtimeData.users[raw.user].username}` —
a fresh object per instance (cached).

### CREEP-PROP-SPAWNING
`raw.spawning` passthrough (true while spawning, false after birth).

### CREEP-PROP-TTL
`raw.ageTime ? raw.ageTime - time : undefined`. Undefined while spawning (player creeps) and possibly on the
first non-spawning tick (PROC-CREEP-AGETIME). Stronghold creeps carry `ageTime` while still spawning.

### CREEP-PROP-FATIGUE / HITS / HITSMAX
Raw passthrough (`fatigue`, `hits`, `hitsMax` = 100 × parts at creation).

### CREEP-PROP-SAYING
`reference/engine/src/game/creeps.js:84-92`: if `!raw.actionLog || !raw.actionLog.say` ⇒ undefined; owner sees `say.message`; others only if
`say.isPublic`. `actionLog` is from the previous processing tick (PROC-ACTIONLOG-RESET), so `saying` is the
text said in the previous tick, already truncated (PROC-SAY).

### CREEP-PROP-STORE / CREEP-PROP-CARRY
Both `new Store(raw)` (`reference/engine/src/game/creeps.js:33-35,79-80`) — **two distinct Store instances** (`creep.carry !==
creep.store`) over the same raw data. Store semantics: `reference/engine/src/game/store.js:19-97` (own
enumerable keys only for non-zero amounts; any `RESOURCES_ALL` key reads 0 via Proxy).
**GOTCHA**: creep without CARRY: `store.getCapacity()` → `null` (storeCapacity 0 ⇒ `|| null`),
`store.getFreeCapacity()` → `null`, but `carryCapacity` → `0`.

### CREEP-PROP-CARRYCAPACITY
`raw.storeCapacity` (number; sum of CARRY capacity incl. boosts, recomputed by processor).

### CREEP-PROP-MEMORY
Non-enumerable accessor (`reference/engine/src/game/creeps.js:95-120`):
- get: if `this.id && !this.my` ⇒ undefined. If `Memory.creeps` is undefined **or the string `'undefined'`** ⇒
  `Memory.creeps = {}`. If `Memory.creeps` not an object ⇒ undefined. Return
  `Memory.creeps[name] = Memory.creeps[name] || {}` (**GOTCHA**: falsy stored memory — 0, '', false, null — is
  replaced by `{}` and written back).
- set: foreign ⇒ throw `Error("Could not set other player's creep memory")`; same `'undefined'` repair; not
  object ⇒ throw `Error('Could not set creep memory')`; `Memory.creeps[name] = value` (overwrites).
- `Memory` itself null (corrupt RawMemory) ⇒ TypeError from property access.

### CREEP-PROP-POS-ROOM-EFFECTS
From ROOMOBJ-CTOR: `pos` (RoomPosition), `room` (Room, always visible for creeps), `effects` (usually absent).

### CREEP-TOSTRING
`` `[creep ${this.name}]` ``.

---

## 5. Creep methods

Notation: each method lists checks in order; the first failing one returns. "target-valid(X)" means the
listed condition; on failure `assertTargetObject(target)` (API-ASSERT-TARGET) then ERR_INVALID_TARGET.
All successful paths return OK (0) after the intent write.

### CREEP-MOVE-DIR — `move(direction)`
1. `!this.my` → ERR_NOT_OWNER. 2. `this.spawning` → ERR_BUSY.
3. (if `target instanceof Creep` → CREEP-MOVE-CREEP)
4. `raw.fatigue > 0` → ERR_TIRED. 5. `!_hasActiveBodypart(body, MOVE)` → ERR_NO_BODYPART.
6. `direction = +target` (ToNumber); `!direction || direction < 1 || direction > 8` → ERR_INVALID_ARGS.
7. intent `move: {direction}` (the coerced number).
**GOTCHA**: not integer-checked: `move(1.5)`, `move("3")`, `move(true)` (=1), `move([3])` all succeed;
sanitiser `parseInt` floors. `move(powerCreep)` falls into the numeric path ⇒ ERR_INVALID_ARGS (after
fatigue/MOVE checks). `reference/engine/src/game/creeps.js:126-156`.

### CREEP-MOVE-CREEP — `move(creep)`
After checks 1–2: `!target.pos.isNearTo(this.pos)` → ERR_NOT_IN_RANGE; intent `move: {id: target.id}`. Fatigue
and MOVE checks are **skipped** (pulled followers have no MOVE). No ownership/spawning check on the target.
Shares the `move` slot with CREEP-MOVE-DIR (last call wins). Processor accepts it only if the target is a
`creep` at distance exactly 1 at processing time (PROC-MOVE-INTENT). `reference/engine/src/game/creeps.js:135-142`.

### CREEP-MOVEBYPATH — `moveByPath(path)`
`reference/engine/src/game/creeps.js:305-333`. No owner/spawning check of its own (they come from `this.move`), so foreign creeps can
get ERR_NOT_FOUND / ERR_INVALID_ARGS before ERR_NOT_OWNER.
- A. `isArray(path) && path.length > 0 && path[0] instanceof RoomPosition`:
  `idx` = first index with `path[i].isEqualTo(this.pos)`; if `idx == -1 && !path[0].isNearTo(this.pos)` →
  ERR_NOT_FOUND; `idx++`; `idx >= path.length` → ERR_NOT_FOUND; return
  `this.move(this.pos.getDirectionTo(path[idx]))` (cross-room capable, non-adjacent gives approximate
  direction).
- B. `isString(path)` → `path = deserializePath(path)` (**throws** on malformed strings).
- C. `!isArray(path)` → ERR_INVALID_ARGS.
- D. `cur` = first element with `i.x - i.dx == pos.x && i.y - i.dy == pos.y` (loose `==`); none →
  ERR_NOT_FOUND (also for `[]`); return `this.move(cur.direction)`.
**GOTCHA**: step-array form ignores room names (a path from another room matches by coordinates). Only
`path[0]` is type-tested in A; later non-RoomPosition elements throw TypeError on `isEqualTo`. `this.move` is
dynamic (PowerCreep override is used for power creeps).

### CREEP-MOVETO — `moveTo(x, y, [opts])` / `moveTo(target, [opts])`
Full algorithm (`reference/engine/src/game/creeps.js:158-303`), sub-items below give the details:
```
visualized = false
if !my → ERR_NOT_OWNER ; if spawning → ERR_BUSY                       (MOVETO-EARLY-CHECKS)
if isObject(a): opts = lodashClone(b)        ; opts = opts || {}        (MOVETO-ARGS)
if raw.fatigue > 0 && !opts.visualizePathStyle → ERR_TIRED
if !hasActiveBodypart(body, MOVE) → ERR_NO_BODYPART
[x,y,room] = fetchXYArguments(a,b) ; room = room || pos.roomName
if x or y undefined: assertTargetObject(a); → ERR_INVALID_TARGET
targetPos = new RoomPosition(x,y,room)        // may throw
defaults: reusePath=5, serializeMemory=true, _.defaults(visualizePathStyle, DEFAULT)   (MOVETO-OPTS-DEFAULTS)
if x==pos.x && y==pos.y && room==pos.roomName → OK                    (MOVETO-SAME-POS)
cached-path branch (may return)                                        (MOVETO-REUSE)
if opts.noPathFinding → ERR_NOT_FOUND                                  (MOVETO-NOPATHFINDING)
path = this.pos.findPathTo(targetPos, opts)                            (MOVETO-FINDPATH)
if opts.reusePath && memory && isObject(memory): memory._move = {...}  (MOVETO-MEM-FORMAT)
if path.length == 0 → ERR_NO_PATH
if opts.visualizePathStyle && !visualized: room.visual.poly(path, style)
return this.move(path[0].direction)                                    (MOVETO-RESULT)
```

### MOVETO-ARGS
- `_.isObject(a)` (objects, arrays, functions) ⇒ `opts = _.clone(b)` — a **shallow** lodash-3 clone (nested
  objects such as `visualizePathStyle`, `costCallback` shared). `b` undefined ⇒ undefined ⇒ `{}`.
- `a` a number ⇒ `opts` is the caller's third argument **uncloned** (MOVETO-OPTS-DEFAULTS mutates it).
- Target resolution via HELP-FETCH-XY-ARGS: RoomPosition, any object with a RoomPosition `pos` (creep, flag,
  structure), or `(x, y)` numbers in the creep's room. A plain `{x,y,roomName}` (e.g. from Memory) ⇒
  ERR_INVALID_TARGET (or the API-ASSERT-TARGET throw if it has a 24-char `id`).
- **GOTCHA**: `moveTo(60, 10)` (or any coordinate outside 0..49) **throws** `Error('Invalid arguments in
  RoomPosition constructor')` (`reference/engine/src/game/rooms.js:1276-1287`) instead of returning a code. `moveTo(5)` ⇒
  ERR_INVALID_TARGET. Odd forms like `moveTo(pos, 5)` take the numeric branch with `x = pos`.

### MOVETO-OPTS-DEFAULTS
`reference/engine/src/game/creeps.js:189-198`: `reusePath` set to 5 if `=== undefined`; `serializeMemory` set to true if `=== undefined`;
if `visualizePathStyle` truthy, `_.defaults(style, {fill:'transparent', stroke:'#fff', lineStyle:'dashed',
strokeWidth:.15, opacity:.1})` — mutates the caller's style object in place (user keys keep their order,
missing defaults appended in that order). In the `(x,y,opts)` form the caller's opts object also gains
`reusePath`/`serializeMemory`. Engine code is sloppy-mode: if `opts` is a primitive, the assignments are
silently ignored.

### MOVETO-EARLY-CHECKS
Order: NOT_OWNER → BUSY → TIRED (raw fatigue, skipped when `visualizePathStyle` truthy) → NO_BODYPART →
INVALID_TARGET (`reference/engine/src/game/creeps.js:162-185`). **GOTCHA**: with `visualizePathStyle` a tired creep runs the whole
algorithm (memory reuse/repath/visual) and normally ends in ERR_TIRED from `move()`; but it can instead return
OK / ERR_NO_PATH (empty cached path), ERR_NOT_FOUND (`noPathFinding`) or OK (same pos). In the reuse branch the
`moveByPath` ERR_TIRED is "not OK" ⇒ falls through to a **fresh path search every tick** while tired (and
rewrites `_move` with a new `time`). PERF hot spot.

### MOVETO-SAME-POS
`x == pos.x && y == pos.y && roomName == pos.roomName` ⇒ OK; no memory access, no intent, no visual.

### MOVETO-REUSE
Entered iff `opts.reusePath && this.memory && _.isObject(this.memory) && this.memory._move` (`reference/engine/src/game/creeps.js:241`).
With `reusePath: 0` Memory is not touched at all (and an old `_move` is left in place).
```
m = memory._move
if time > m.time + parseInt(opts.reusePath) || m.room != pos.roomName:
    delete memory._move                       // then fall through to path finding
else if m.dest.room == roomName && m.dest.x == x && m.dest.y == y:   // loose ==
    path = isString(m.path) ? deserializePath(m.path) : m.path       // may throw
    idx = _.findIndex(path, {x: pos.x, y: pos.y})                     // first match
    if idx != -1:
        path.splice(0, idx+1)
        m.path = opts.serializeMemory ? serializePath(path) : path    // try/catch: logs '$ERR ...' then rethrows
    if path.length == 0: return pos.isNearTo(targetPos) ? OK : ERR_NO_PATH
    if opts.visualizePathStyle: room.visual.poly(path, style); visualized = true
    r = this.moveByPath(path)            // plain-step branch D
    if r == OK: return OK
    // any other code: fall through to noPathFinding / repath
// destination differs: fall through (old _move overwritten later)
```
Semantics to preserve:
- Expiry: a path stored at tick T is reused at T+1..T+reusePath, recomputed at T+reusePath+1.
  `parseInt` of a non-numeric `reusePath` ⇒ NaN ⇒ never expires by time.
- Leaving the room (`m.room` = room where the path was computed) invalidates the cache.
- Blocked creep (did not move): `idx == -1`, path untouched, `moveByPath` finds the step whose predecessor is
  the current tile and re-issues the same direction every tick until expiry (no stuck detection).
- Pushed off the path ⇒ `moveByPath` ERR_NOT_FOUND ⇒ repath.
- With `serializeMemory:false` the stored array is mutated in place (live Memory object).
- Corrupt `_move` (missing `dest`) ⇒ TypeError; corrupt path string ⇒ deserialize Error. Both escape moveTo.
- **GOTCHA** "empty path is cached": MOVETO-MEM-FORMAT writes `_move` before the `path.length == 0` check, so an
  ERR_NO_PATH result is replayed from cache (no search) for `reusePath` ticks.
- **GOTCHA** "range": with `opts.range > 0` the last stored step is at distance `range`; when the creep reaches
  it the spliced path is empty and the result is `isNearTo(target) ? OK : ERR_NO_PATH` ⇒ **ERR_NO_PATH on
  arrival** for range ≥ 2; repathing from inside the range also yields an empty path ⇒ ERR_NO_PATH.
- Moving targets (creeps) change `dest` every tick ⇒ repath every tick. PERF.

### MOVETO-MEM-FORMAT
`reference/engine/src/game/creeps.js:285-292`, written only if `opts.reusePath && this.memory && _.isObject(this.memory)`:
```
memory._move = { dest: {x, y, room: roomName}, time: Game.time,
                 path: serializeMemory ? serializePath(path) : _.clone(path), room: pos.roomName }
```
Key order (matters for byte-identical RawMemory): `dest{x,y,room}`, `time`, `path`, `room`. `x`/`y` are the
values from argument parsing (numbers in all reachable forms). Array form = shallow clone of the step objects
`{x,y,dx,dy,direction}`. Written on every fresh search, including empty results. The cache branch rewrites only
`path` (after splice).

### MOVETO-NOPATHFINDING
`if(opts.noPathFinding) return ERR_NOT_FOUND` (`reference/engine/src/game/creeps.js:279-281`) — reached when there is no valid cache, the cache
was expired/deleted, destination differs, or the cached step failed (incl. tired+visualizing).

### MOVETO-FINDPATH
`this.pos.findPathTo(targetPos, opts)` (`reference/engine/src/game/rooms.js:1396-1427`): re-derives x,y,roomName from `targetPos`,
`opts = _.clone(opts)` (so the caller's object is not mutated by find), room = `register.rooms[pos.roomName]`;
with the new path finder (default) always `room.findPath(pos, new RoomPosition(x,y,roomName), opts)` →
`_findPath2(creepRoom, from, to, opts)` (`reference/engine/src/game/rooms.js:863-870,223-302`):
1. `from.isEqualTo(to)` ⇒ `[]` (unreachable from moveTo).
2. `opts.avoid` / `opts.ignore` ⇒ deprecation log, set to undefined.
3. if `opts.maxOps === undefined && (opts.maxRooms === undefined || opts.maxRooms > 1) && from.roomName !=
   to.roomName` ⇒ `opts.maxOps = 20000`.
4. `PathFinder.search(from, {range: Math.max(1, opts.range || 0), pos: to}, searchOpts)` with
   `searchOpts = {roomCallback, maxOps: opts.maxOps, maxRooms: opts.maxRooms}` +
   `plainCost: 2, swampCost: 10` unless `ignoreRoads`; then `opts.plainCost` / `opts.swampCost` override if
   truthy.
5. MOVETO-PATH-POSTPROCESS.
PERF: cross-room: up to 16 rooms / 20000 ops searched, only the current-room prefix is used, and the cache is
dropped on every room change ⇒ one full multi-room search per room crossed per creep.

### MOVETO-PF-OPTIONS
Honoured: `ignoreCreeps`, `ignoreDestructibleStructures`, `ignoreRoads` (matrix + cost defaults),
`costCallback`, `maxOps`, `maxRooms`, `range`, `plainCost`, `swampCost` (truthy only).
Ignored: `avoid`, `ignore` (deprecated log), **`heuristicWeight`, `maxCost`, `flee`** (never forwarded ⇒
driver defaults 1.2, 0xffffffff, false). Driver clamps (`reference/driver/lib/path-finder.js:90-96`):
plain/swamp `(v|0)||default` clamped 1..254 (defaults 1/5 apply only with `ignoreRoads`), maxOps
`(v|0)||2000` ≥1, maxRooms `(v|0)||16` clamped 1..64, heuristicWeight clamped 1..9.
**GOTCHA**: docs say plainCost 1/swampCost 5 defaults — findPath/moveTo actually use 2/10 (roads 1) unless
`ignoreRoads`. `opts.serialize: true` makes `findPath` return a string ⇒ moveTo throws
`Error('path is not an array')` in `serializePath` (reusePath on) or returns ERR_INVALID_ARGS from
`move(undefined)` (reusePath 0).

### MOVETO-COSTMATRIX
`getPathfindingGrid2(room, opts)` (`reference/engine/src/game/rooms.js:198-221`): non-visible room ⇒ a fresh empty CostMatrix (not
cached); visible ⇒ cached per tick per room per key `grid2[_ignoreCreeps][_ignoreDestructibleStructures]
[_ignoreRoads]` and returned **by reference** (only cloned when a costCallback exists).
`makePathfindingGrid2` (`reference/engine/src/game/rooms.js:157-196`), for every non-temp object of the room:
```
obstacle = OBSTACLE_OBJECT_TYPES + ['portal']
  minus [constructedWall,spawn,extension,link,storage,observer,tower,powerBank,powerSpawn,lab,terminal]
        if ignoreDestructibleStructures
  minus [creep,powerCreep] if ignoreCreeps || mySafeMode      // mySafeMode = controller.safeMode && controller.my
set 0xFF if type ∈ obstacle
  or (!ignoreCreeps && mySafeMode && type ∈ {creep,powerCreep} && user == me)
  or (!ignoreDestructibleStructures && type=='rampart' && !isPublic && user != me)
  or (!ignoreDestructibleStructures && type=='constructionSite' && user == me && structureType ∈ OBSTACLE_OBJECT_TYPES)
if type=='swamp' && get==0: set(ignoreRoads ? 5 : 10)          // legacy object type, inert in current data
if !ignoreRoads && type=='road' && get < 0xFF: set 1
```
Terrain is not written (0 = terrain default). **GOTCHA**: with `ignoreDestructibleStructures`, nuker, factory,
invaderCore, controller, sources, minerals, deposits, portals stay obstacles. Hostile construction sites never
block; own sites block only for obstacle structure types. In own safe mode only own creeps block. The matrix
reflects tick-start positions (intents do not update it).
PERF: O(objects) per (room, flag-combo, player, tick). Mostly room-local and player-independent except
own-construction-sites, rampart ownership/public, own-creeps-in-safe-mode — a shared per-room base plus a
per-player overlay reproduces it exactly.

### MOVETO-COSTCALLBACK
In `roomCallback(roomName)` (`reference/engine/src/game/rooms.js:243-253`): `m = grid2(roomName)`; if `typeof opts.costCallback ==
'function'`: `m = m.clone(); r = opts.costCallback(roomName, m); if (r instanceof PathFinder.CostMatrix) m = r`;
return `m`. Called lazily by the native search, once per room the search expands into (including non-visible
rooms, where it receives a clone of an empty matrix). In-place mutation of the argument works; returning
`false`/undefined does **not** block the room (unlike raw `PathFinder.search` roomCallback). It is invoked as
a method of the cloned options object (`this` = that clone); exceptions propagate out of moveTo.

### MOVETO-PATH-POSTPROCESS
`reference/engine/src/game/rooms.js:270-301`:
- If `!opts.range` and (`ret.path` non-empty and its last pos `isNearTo(to)` and not equal) or (`ret.path`
  empty and `from.isNearTo(to)`): `ret.path.push(to)` — the target tile is appended **even if it is an
  obstacle**.
- Convert sequentially from `from`: `{x, y, dx, dy, direction: getDirection(dx,dy)}`; stop at the first
  position whose `roomName` differs from the creep's room (path ends on the exit tile).
- Incomplete searches still return the partial path (no `incomplete` flag reaches moveTo).
**GOTCHA**: consequence — `moveTo(source)` while adjacent emits `move` into the source every tick (OK, 0.2 CPU,
fails in processing). Bots rely on `moveTo` returning OK here.

### MOVETO-VISUALIZE
`room.visual.poly(path, style)` (`reference/engine/src/game/rooms.js:1184-1190`) appends
`JSON.stringify({t:'p', points:[[x,y],...], s: style}) + "\n"` to the per-room visual buffer
(`reference/engine/src/game/console.js:57-74`); points are the remaining steps **excluding** the creep's tile.
Throws `Error('RoomVisual size in room <r> has exceeded 500 KB limit')` when the buffer would exceed
500*1024 chars. Drawn at most once per call (cache branch sets `visualized`). Non-object truthy styles are
passed through unmodified.

### MOVETO-RESULT
`return this.move(path[0].direction)` — re-runs all `move()` checks (ERR_TIRED when visualizing while tired).
Complete outcome set: OK, ERR_NOT_OWNER, ERR_BUSY, ERR_TIRED, ERR_NO_BODYPART, ERR_INVALID_TARGET, ERR_NO_PATH,
ERR_NOT_FOUND, ERR_INVALID_ARGS (degenerate paths); thrown: RoomPosition ctor error, assertTargetObject error,
deserializePath error, `_move` shape TypeError, RoomVisual size error, `serialize:true` error, costCallback
exceptions. Intent emitted: at most one `move: {direction}` (via `move`).

### MOVETO-LEGACY-PF
`PathFinder.use(false)` sets `register._useNewPathFinder = false` for the **rest of the current tick** (register
is rebuilt every tick with `true`; `reference/engine/src/game/game.js:67-68`, `reference/engine/src/game/path-finder.js:67-75`). Then `findPathTo`
(`reference/engine/src/game/rooms.js:1412-1425`): same room ⇒ legacy `Room.findPath` (A* from `@screeps/pathfinding` over a grid: wall 0,
plain 2, swamp 10, border tiles 11, road 1, obstacles 0, `avoid`/`ignore` honoured, per-room `pathCache`,
`maxOps` default 2000, heuristicWeight 1; `reference/engine/src/game/rooms.js:16-155,873-970`); other room ⇒ `findExitTo` +
`findClosestByPath(exitDir)` + path to that exit tile. Rarely used; implement after the default path.

### CREEP-PULL — `pull(target)`
1. `!my` → ERR_NOT_OWNER. 2. `spawning` → ERR_BUSY.
3. target-valid: `target && target.id && register.creeps[target.id] && target instanceof Creep &&
   !target.spawning && target.id != this.id`.
4. `!target.pos.isNearTo(this.pos)` → ERR_NOT_IN_RANGE.
5. intent `pull: {id}` — **free** (no CPU). No MOVE-part check (docs claim one). Target may be foreign
(processor: target must be a non-spawning creep within range 1; `reference/engine/src/processor/intents/creeps/pull.js:7-21`).

### CREEP-HARVEST — `harvest(target)`
1. NOT_OWNER. 2. BUSY. 3. `!_hasActiveBodypart(WORK)` → ERR_NO_BODYPART. 4. `!target || !target.id` →
ERR_INVALID_TARGET (no assert).
5. Source (`register.sources[id] && instanceof Source`): `!target.energy` → ERR_NOT_ENOUGH_RESOURCES; not near →
   ERR_NOT_IN_RANGE; `room.controller && (controller.owner && owner.username != me.username ||
   controller.reservation && reservation.username != me.username)` → ERR_NOT_OWNER (username compare; actor's
   room).
6. Mineral (`register.minerals[id] && instanceof Mineral`): `!mineralAmount` → ERR_NOT_ENOUGH_RESOURCES; not near
   → ERR_NOT_IN_RANGE; extractor = first structure at target pos with type extractor (`pos.lookFor('structure')`)
   — none → ERR_NOT_FOUND; `extractor.owner && !extractor.my` → ERR_NOT_OWNER; `!extractor.isActive()` →
   ERR_RCL_NOT_ENOUGH; `extractor.cooldown` → ERR_TIRED.
7. Deposit (`register.deposits[id] && instanceof Deposit`): not near → ERR_NOT_IN_RANGE; `target.cooldown` →
   ERR_TIRED.
8. else assert + ERR_INVALID_TARGET.
9. intent `harvest: {id}`. No CARRY requirement; no hostile-safe-mode check. `reference/engine/src/game/creeps.js:335-402`.

### CREEP-DROP — `drop(resourceType, [amount])`
1. NOT_OWNER. 2. BUSY. 3. `resourceType ∉ RESOURCES_ALL` → ERR_INVALID_ARGS.
4. `!raw.store || !raw.store[rt]` → ERR_NOT_ENOUGH_RESOURCES. 5. `!amount` ⇒ `amount = raw.store[rt]`.
6. `raw.store[rt] < amount` → ERR_NOT_ENOUGH_RESOURCES. 7. intent `drop: {amount, resourceType}`.
**GOTCHA**: negative / fractional / numeric-string amounts pass (e.g. -5) and are left to sanitiser/processor.
`reference/engine/src/game/creeps.js:404-426`.

### CREEP-TRANSFER — `transfer(target, resourceType, [amount])`
`reference/engine/src/game/creeps.js:428-491`:
1. NOT_OWNER. 2. BUSY. 3. `amount < 0` → ERR_INVALID_ARGS. 4. `rt ∉ RESOURCES_ALL` → ERR_INVALID_ARGS.
5. target-valid unless any of: `!target`; `!target.id`; id in none of `register.spawns/powerCreeps/creeps/
   structures/customObjects`; `!data(id).store && register.structures[id].structureType != 'controller'`;
   `target instanceof Creep && target.spawning`; target is none of StructureSpawn/Structure/Creep/PowerCreep
   and not a custom object.
6. `rt == energy` and target is a controller ⇒ `return this.upgradeController(target)` (CREEP-TRANSFER-CONTROLLER).
7. `!capacityForResource(data(target), rt)` → ERR_INVALID_TARGET (e.g. target creep with no CARRY, non-energy into
   spawn/extension/controller).
8. not near → ERR_NOT_IN_RANGE. 9. `!raw.store || !raw.store[rt]` → ERR_NOT_ENOUGH_RESOURCES.
10. `stored = data(target).storeCapacityResource ? (target store[rt]||0) : calcResources(target)`;
    `cap = capacityForResource(data(target), rt)`.
11. `!data(target).store || stored >= cap` → ERR_FULL.
12. `!amount` ⇒ `amount = min(raw.store[rt], cap - stored)`.
13. `raw.store[rt] < amount` → ERR_NOT_ENOUGH_RESOURCES. 14. `amount + stored > cap` → ERR_FULL.
15. intent `transfer: {id, amount, resourceType}`.
**GOTCHA**: `amount` is never coerced client-side: a string amount makes step 14 a string concatenation
compare. Self-transfer is not rejected client-side. Hostile targets allowed.

### CREEP-TRANSFER-CONTROLLER
`transfer(controller, RESOURCE_ENERGY, anyAmount)` returns `this.upgradeController(controller)` — amount
ignored, upgradeController's own checks/codes apply (range 3, not 1). Called through `this`, so for power
creeps it **throws TypeError** (no `upgradeController` method) — see PC-TRANSFER.

### CREEP-WITHDRAW — `withdraw(target, resourceType, [amount])`
`reference/engine/src/game/creeps.js:493-564`:
1. NOT_OWNER. 2. BUSY. 3. `amount < 0` → ERR_INVALID_ARGS. 4. `rt ∉ RESOURCES_ALL` → ERR_INVALID_ARGS.
5. target-valid unless: `!target || !target.id || !data(id).store` (data throws for unknown ids) or target is not
   (`register.structures[id] && instanceof Structure`) and not Tombstone and not Ruin and not a custom object
   (creeps are rejected).
6. `target.structureType == 'terminal'` and an effect with `power == PWR_DISRUPT_TERMINAL` and
   `ticksRemaining > 0` → ERR_INVALID_TARGET.
7. `target.my === false` and any structure at target pos is a rampart with `!my && !isPublic` → ERR_NOT_OWNER.
   **GOTCHA**: neutral targets (`my` undefined: containers, tombstones, ruins) are never protected by ramparts here.
8. hostile safe mode in actor's room → ERR_NOT_OWNER.
9. target is nuker or powerBank → ERR_INVALID_TARGET.
10. `!capacityForResource(data(target), rt) && !data(target).store[rt] && !(target instanceof Tombstone)` →
    ERR_INVALID_TARGET.
11. not near → ERR_NOT_IN_RANGE.
12. `empty = raw.storeCapacity - calcResources(raw)`; `empty <= 0` → ERR_FULL.
13. `!amount` ⇒ `amount = min(empty, target store[rt])` (may be NaN). 14. `amount > empty` → ERR_FULL.
15. `!amount || (target store[rt]||0) < amount` → ERR_NOT_ENOUGH_RESOURCES.
16. intent `withdraw: {id, amount, resourceType}`.

### CREEP-PICKUP — `pickup(target)`
1. NOT_OWNER. 2. BUSY. 3. target-valid: `target.id && register.energy[id] && target instanceof Energy`
(`Energy === Resource`, `reference/engine/src/game/resources.js:45-46`). 4. `calcResources(raw) >=
raw.storeCapacity` → ERR_FULL (checked **before** range; no CARRY ⇒ always FULL). 5. not near →
ERR_NOT_IN_RANGE. 6. intent `pickup: {id}`. `reference/engine/src/game/creeps.js:566-587`.

### CREEP-BUILD — `build(target)`
`reference/engine/src/game/creeps.js:762-811`:
1. NOT_OWNER. 2. BUSY. 3. no active WORK → ERR_NO_BODYPART. 4. `!this.carry.energy` →
ERR_NOT_ENOUGH_RESOURCES (before target validation). 5. target-valid: `register.constructionSites[id] &&
instanceof ConstructionSite`. 6. `!this.pos.inRangeTo(target, 3)` → ERR_NOT_IN_RANGE.
7. Scan **all** objects of the creep's room (`register.objectsByRoom[raw.room]`) on the target tile whose type
   ∈ OBSTACLE_OBJECT_TYPES: creeps → `creepsInTile` (+ `myCreepsInTile` if `user == raw.user`), everything
   else (incl. powerCreep) → `objectsInTile`. If `target.structureType ∈ OBSTACLE_OBJECT_TYPES`: any
   `objectsInTile` → ERR_INVALID_TARGET; blocking creeps = (own controller in safe mode) ? my creeps : all
   creeps; any → ERR_INVALID_TARGET.
8. intent `build: {id, x, y}` (target pos).
Hostile construction sites can be targeted client-side. PERF: O(room objects) per call — index by tile.

### CREEP-REPAIR — `repair(target)`
1. NOT_OWNER. 2. BUSY. 3. no WORK → ERR_NO_BODYPART. 4. `!this.carry.energy` → ERR_NOT_ENOUGH_RESOURCES.
5. target-valid: `register.structures[id] && (instanceof Structure || StructureSpawn)`. 6. `inRangeTo(target,3)`
fails → ERR_NOT_IN_RANGE. 7. intent `repair: {id, x, y}`. No hits/ownership checks. `reference/engine/src/game/creeps.js:734-760`.

### CREEP-DISMANTLE — `dismantle(target)`
1. NOT_OWNER. 2. BUSY. 3. no WORK → ERR_NO_BODYPART. 4. target-valid: structure registered, instanceof
Structure/Spawn, and `C.CONSTRUCTION_COST[structureType]` truthy (excludes controller, powerBank, keeperLair,
portal, invaderCore). 5. not near → ERR_NOT_IN_RANGE. 6. hostile safe mode → ERR_NO_BODYPART. 7. effect with
`power == PWR_FORTIFY || effect == EFFECT_INVULNERABILITY` and `ticksRemaining > 0` → ERR_INVALID_TARGET.
8. intent `dismantle: {id}`. `reference/engine/src/game/creeps.js:1024-1055`.

### CREEP-UPGRADE-CONTROLLER — `upgradeController(target)`
1. NOT_OWNER. 2. BUSY. 3. no WORK → ERR_NO_BODYPART. 4. `user.shardAccess === false` → ERR_ACCESS_DENIED.
5. `!this.carry.energy` → ERR_NOT_ENOUGH_RESOURCES. 6. target-valid: structure registered and instanceof
StructureController. 7. `target.upgradeBlocked > 0` (getter: remaining ticks) → ERR_INVALID_TARGET.
8. `!target.pos.inRangeTo(this.pos, 3)` → ERR_NOT_IN_RANGE. 9. `!target.my` → ERR_NOT_OWNER.
10. `!target.level || !target.owner` → ERR_INVALID_TARGET. 11. intent `upgradeController: {id}`.
`reference/engine/src/game/creeps.js:922-959`.

### CREEP-CLAIM-CONTROLLER — `claimController(target)`
`reference/engine/src/game/creeps.js:839-886`:
1. NOT_OWNER. 2. BUSY.
3. `claimed = user.rooms.length + controllersClaimedInTick`; `claimed && (!user.gcl || user.gcl <
   calcNeededGcl(claimed + 1))` → ERR_GCL_NOT_ENOUGH (before target validation!).
4. `claimed >= C.GCL_NOVICE (3) && runtimeData.rooms[raw.room].novice > Date.now()` → ERR_FULL (wall clock).
5. target-valid: structure registered and instanceof Structure.
6. no active CLAIM → ERR_NO_BODYPART. 7. `shardAccess === false` → ERR_ACCESS_DENIED. 8. not near →
   ERR_NOT_IN_RANGE. 9. not a controller → ERR_INVALID_TARGET. 10. `level > 0` → ERR_INVALID_TARGET.
11. reserved by another username → ERR_INVALID_TARGET. 12. hostile safe mode → ERR_NO_BODYPART.
13. `controllersClaimedInTick++`; intent `claimController: {id}`.
**GOTCHA**: the counter increments on every OK call, even when the same creep calls twice (intent overwritten)
or the claim later fails in processing; it only resets next tick.

### CREEP-RESERVE-CONTROLLER — `reserveController(target)`
1. NOT_OWNER. 2. BUSY. 3. target-valid: structure registered and instanceof Structure. 4. not near →
ERR_NOT_IN_RANGE. 5. not controller → ERR_INVALID_TARGET. 6. `target.owner` → ERR_INVALID_TARGET.
7. reserved by another username → ERR_INVALID_TARGET. 8. no CLAIM → ERR_NO_BODYPART. 9. `shardAccess ===
false` → ERR_ACCESS_DENIED. 10. intent `reserveController: {id}`. No safe-mode check. `reference/engine/src/game/creeps.js:961-994`.

### CREEP-ATTACK-CONTROLLER — `attackController(target)`
1. NOT_OWNER. 2. BUSY. 3. target-valid: structure registered and instanceof StructureController. 4. CLAIM count
0 → ERR_NO_BODYPART. 5. not near → ERR_NOT_IN_RANGE. 6. `!owner && !reservation` → ERR_INVALID_TARGET.
7. `raw(target).upgradeBlocked > time` → ERR_TIRED. 8. hostile safe mode → ERR_NO_BODYPART. 9. any effect
`EFFECT_INVULNERABILITY` with ticksRemaining > 0 → ERR_INVALID_TARGET. 10. intent `attackController: {id}`.
Own controllers are not rejected client-side. `reference/engine/src/game/creeps.js:888-920`.

### CREEP-SIGN-CONTROLLER — `signController(target, text)`
**No ownership check and no CLAIM requirement.** 1. `spawning` → ERR_BUSY. 2. target-valid: structure
registered and instanceof Structure. 3. not near → ERR_NOT_IN_RANGE. 4. not controller → ERR_INVALID_TARGET.
5. intent `signController: {id, sign: ""+text}`. **GOTCHA**: omitted text ⇒ sign `"undefined"`; `""` removes the
sign (PROC-SIGN); called on a foreign creep it returns OK, costs CPU and is discarded (API-INTENT-ROUTING).
`reference/engine/src/game/creeps.js:1080-1099`.

### CREEP-GENERATE-SAFE-MODE — `generateSafeMode(target)`
1. NOT_OWNER. 2. BUSY. 3. `!raw.store || !(raw.store.G >= C.SAFE_MODE_COST (1000))` →
ERR_NOT_ENOUGH_RESOURCES. 4. target-valid: structure registered and instanceof StructureController.
5. not near → ERR_NOT_IN_RANGE. 6. intent `generateSafeMode: {id}`. No controller ownership check client-side.
`reference/engine/src/game/creeps.js:1057-1078`.

### CREEP-ATTACK — `attack(target)`
1. NOT_OWNER. 2. BUSY. 3. no ATTACK → ERR_NO_BODYPART. 4. hostile safe mode → ERR_NO_BODYPART.
5. target-valid: id in creeps/powerCreeps/structures registers and instanceof Creep/PowerCreep/StructureSpawn/
Structure. 6. FORTIFY/INVULNERABILITY effect active → ERR_INVALID_TARGET (before range). 7. not near →
ERR_NOT_IN_RANGE. 8. intent `attack: {id, x, y}`. Own targets allowed. `reference/engine/src/game/creeps.js:593-624`.

### CREEP-RANGED-ATTACK — `rangedAttack(target)`
1. NOT_OWNER. 2. BUSY. 3. no RANGED_ATTACK → ERR_NO_BODYPART. 4. hostile safe mode → ERR_NO_BODYPART.
5. target-valid as attack. 6. `!this.pos.inRangeTo(target, 3)` → ERR_NOT_IN_RANGE (**before** the effect
check — opposite of attack). 7. FORTIFY/INVULNERABILITY → ERR_INVALID_TARGET. 8. intent
`rangedAttack: {id}`. `reference/engine/src/game/creeps.js:626-656`.

### CREEP-RANGED-MASS-ATTACK — `rangedMassAttack()`
1. NOT_OWNER. 2. BUSY. 3. no RANGED_ATTACK → ERR_NO_BODYPART. 4. hostile safe mode → ERR_NO_BODYPART.
5. intent `rangedMassAttack: {}`. `reference/engine/src/game/creeps.js:658-676`.

### CREEP-HEAL — `heal(target)`
1. NOT_OWNER. 2. BUSY. 3. no HEAL → ERR_NO_BODYPART. 4. target-valid: id in creeps/powerCreeps registers and
instanceof Creep/PowerCreep (spawning and hostile targets allowed). 5. not near → ERR_NOT_IN_RANGE.
6. hostile safe mode → ERR_NO_BODYPART (**after** range). 7. intent `heal: {id, x, y}`. `reference/engine/src/game/creeps.js:678-704`.

### CREEP-RANGED-HEAL — `rangedHeal(target)`
1. NOT_OWNER. 2. BUSY. 3. no HEAL → ERR_NO_BODYPART. 4. target-valid as heal. 5. hostile safe mode →
ERR_NO_BODYPART (**before** range). 6. `!inRangeTo(target,3)` → ERR_NOT_IN_RANGE. 7. intent
`rangedHeal: {id}`. `reference/engine/src/game/creeps.js:706-732`.

### CREEP-SAY — `say(message, [isPublic])`
1. NOT_OWNER. 2. BUSY. 3. intent `say: {message: ""+message, isPublic}` — free. `say()` ⇒ `"undefined"`;
Symbols throw on string conversion. Truncation: 100 by sanitiser, 10 UTF-16 code units by processor
(PROC-SAY). Visible next tick via `saying`. `reference/engine/src/game/creeps.js:826-837`.

### CREEP-SUICIDE — `suicide()`
1. NOT_OWNER. 2. BUSY. 3. intent `suicide: {}`. `reference/engine/src/game/creeps.js:813-824`.

### CREEP-NOTIFY-WHEN-ATTACKED — `notifyWhenAttacked(enabled)`
1. NOT_OWNER. 2. BUSY. 3. `!_.isBoolean(enabled)` → ERR_INVALID_ARGS (no default). 4. if `enabled !=
raw.notifyWhenAttacked` → intent `notifyWhenAttacked: {enabled}`. Return OK either way.
**GOTCHA**: `(false)` then `(true)` in one tick on a creep whose stored value is `true`: the second call writes
nothing, so the `false` intent survives — not last-call-wins. Raw value may be undefined (then any call
writes). `reference/engine/src/game/creeps.js:996-1014`.

### CREEP-CANCEL-ORDER — `cancelOrder(methodName)`
No owner/spawning checks: `intents.remove(this.id, name) ? OK : ERR_NOT_FOUND` (API-INTENT-REMOVE). Cancels
only the intent slot; side effects already done (moveTo memory, visuals, claim counter) remain.
`reference/engine/src/game/creeps.js:1016-1022`.

### CREEP-GET-ACTIVE-BODYPARTS — `getActiveBodyparts(type)`
`_getActiveBodyparts(this.body, type)` — any value for `type`, unboosted count. `reference/engine/src/game/creeps.js:589-591`.

---

## 6. PowerCreep

### PC-CTOR
`PowerCreep(id)` (`reference/engine/src/game/power-creeps.js:34-40`): `_data = data(id)`; if `_data.room` ⇒
`RoomObject.call(this, x, y, room)` — **no effects argument**; then `this.id = id`. Unspawned power creeps have
no `pos`/`room` own props. `PowerCreep.prototype = Object.create(RoomObject.prototype)` (not a Creep subclass;
`instanceof Creep` false). Static `PowerCreep.create`. Global `PowerCreep` (`reference/engine/src/game/power-creeps.js:414`).

### PC-DATA-MERGE
`data(id) = Object.assign({}, runtimeData.userPowerCreeps[id], runtimeData.roomObjects[id])` — a fresh shallow
merge per call, room object wins; never throws (foreign PC: room object only; unspawned: user record only).
User record fields: `name, className, user, level, hitsMax, store, storeCapacity, spawnCooldownTime, powers`
(+`deleteTime`, `shard`) (`reference/engine/src/processor/global-intents/power/createPowerCreep.js:27-37`). Inside delegated
`Creep.prototype` methods, `data` is **creeps.js's** `data` (raw room object, throws if absent).

### PC-PROP-* (getters, `reference/engine/src/game/power-creeps.js:45-73`, cached per API-GETTER-CACHE)
- PC-PROP-NAME / PC-PROP-LEVEL / PC-PROP-CLASSNAME / PC-PROP-HITS / PC-PROP-HITSMAX: merged passthrough of
  `name`, `level`, `className`, `hits`, `hitsMax` (unspawned: values from the user record).
- PC-PROP-MY: `my = user == me` (loose). PC-PROP-OWNER: `{username: users[user].username}` (as creep).
- PC-PROP-SHARD: `shard = o.shard || undefined`.
- PC-PROP-SPAWN-COOLDOWN-TIME: `(o.spawnCooldownTime !== null && o.spawnCooldownTime > Date.now()) ? value :
  undefined` — **wall-clock ms**, not ticks; the stored value is null while spawned, 0 for a new creep.
- PC-PROP-DELETE-TIME: `o.deleteTime || undefined` (wall-clock ms).
- PC-PROP-POWERS: `_.mapValues(o.powers, p => ({level: p.level, cooldown: Math.max(0, (p.cooldownTime||0) -
  time)}))` — keys are power ids as strings; **GOTCHA** `cooldown` is a number (0) for unspawned creeps, docs
  say undefined.
- PC-PROP-SAYING: as CREEP-PROP-SAYING.
- PC-PROP-STORE / PC-PROP-CARRY: two Store instances over the merged object. PC-PROP-CARRYCAPACITY:
  `o.storeCapacity` (100 × (level+1)).
- PC-PROP-TTL: `o.ageTime - time` — **GOTCHA** `NaN` for unspawned creeps (docs: undefined).
- PC-PROP-MEMORY (`reference/engine/src/game/power-creeps.js:75-100`): identical to CREEP-PROP-MEMORY but on
  `Memory.powerCreeps`; same error strings ("Could not set other player's creep memory", "Could not set
  creep memory").
- PC-PROP-POS-ROOM-EFFECTS: `pos`/`room` own props only when spawned (PC-CTOR); `effects` never set
  (constructor omits it) — always undefined.
- PC-TOSTRING: `` `[powerCreep ${this.name}]` ``.

### PC-ABSENT-MEMBERS
No `body`, `fatigue`, `spawning`, `getActiveBodyparts`, `harvest`, `attack*`, `heal*`, `build`, `repair`,
`dismantle`, `*Controller`, `generateSafeMode`, `pull`. Delegated Creep code sees `body` undefined
(⇒ `_hasActiveBodypart` true), `spawning` undefined, raw `fatigue` undefined (never tired).

### PC-CREATE — `PowerCreep.create(name, className)` (static)
1. `!name || !_.isString(name) || name.length > 100` → ERR_INVALID_ARGS. 2. `calcFreePowerLevels() <= 0` →
ERR_NOT_ENOUGH_RESOURCES. 3. any own PC with `name` → ERR_NAME_EXISTS. 4. `className ∉ values(C.POWER_CLASS)`
(`'operator'`) → ERR_INVALID_ARGS. 5. global intent `createPowerCreep: {name, className}`.
No local accounting: several creates per tick all pass step 2 (processor re-checks). `reference/engine/src/game/power-creeps.js:395-410`.

### PC-SPAWN — `spawn(powerSpawn)`
1. `this.room` → ERR_BUSY. 2. `!(powerSpawn instanceof StructurePowerSpawn)` → ERR_INVALID_TARGET (no assert).
3. `!this.my || !powerSpawn.my` → ERR_NOT_OWNER. 4. `!checkStructureAgainstController(data(ps),
objectsByRoom[ps.room], data(ps.room.controller))` → ERR_RCL_NOT_ENOUGH. 5. `this.spawnCooldownTime` (wall
clock) → ERR_TIRED. 6. if `Memory.powerCreeps` undefined ⇒ `{}`; if it is an object and
`Memory.powerCreeps[name]` undefined ⇒ `{}` (client side effect). 7. global intent
`spawnPowerCreep: {id: powerSpawn.id, name: this.name}`. `reference/engine/src/game/power-creeps.js:162-189`.

### PC-SUICIDE — `suicide()`
1. `!this.room` → ERR_BUSY. 2. `!my` → ERR_NOT_OWNER. 3. global intent `suicidePowerCreep: {id}`.
`reference/engine/src/game/power-creeps.js:191-202`.

### PC-DELETE — `delete([cancel])`
1. `this.room` → ERR_BUSY. 2. `!my` → ERR_NOT_OWNER. 3. global intent `deletePowerCreep: {id, cancel}`
(cancel → boolean). `reference/engine/src/game/power-creeps.js:204-215`.

### PC-UPGRADE — `upgrade(power)`
1. `!my` → ERR_NOT_OWNER. 2. `calcFreePowerLevels() <= 0` → ERR_NOT_ENOUGH_RESOURCES. 3. `level >=
C.POWER_CREEP_MAX_LEVEL (25)` → ERR_FULL. 4. `info = POWER_INFO[power]`; `!info || info.className !==
className` → ERR_INVALID_ARGS. 5. `pl = data.powers[power]?.level || 0`; `pl == 5` → ERR_FULL. 6. `level <
info.level[pl]` → ERR_FULL. 7. global intent `upgradePowerCreep: {id, power}`. Works spawned or unspawned;
no per-tick accounting. `reference/engine/src/game/power-creeps.js:217-244`.

### PC-RENAME — `rename(name)`
1. `this.room` → ERR_BUSY. 2. `!my` → ERR_NOT_OWNER. 3. invalid name (as create) → ERR_INVALID_ARGS. 4. any own
PC (including itself) with that name → ERR_NAME_EXISTS. 5. global intent `renamePowerCreep: {id, name}`.
`reference/engine/src/game/power-creeps.js:356-373`.

### PC-USE-POWER — `usePower(power, [target])`
`reference/engine/src/game/power-creeps.js:246-293`:
1. `!my` → ERR_NOT_OWNER. 2. `!this.room` → ERR_BUSY.
3. if `room.controller`: `!isPowerEnabled` → ERR_INVALID_ARGS; `!controller.my && controller.safeMode` →
   ERR_INVALID_ARGS.
4. `pd = data.powers[power]`, `info = POWER_INFO[power]`; `!pd || !pd.level || !info` → ERR_NO_BODYPART.
5. `pd.cooldownTime > time` → ERR_TIRED.
6. `ops = info.ops || 0`, array ⇒ `ops[pd.level-1]`; `(data.store.ops || 0) < ops` → ERR_NOT_ENOUGH_RESOURCES.
7. if `info.range`: `!target` → ERR_INVALID_TARGET (no assert, no type check); `!this.pos.inRangeTo(target,
   info.range)` → ERR_NOT_IN_RANGE; `e = first target.effects with power == power`; `e && e.level > pd.level &&
   e.ticksRemaining > 0` → ERR_FULL.
8. intent `usePower: {power, id: target ? target.id : undefined}` (slot `usePower`: one per tick, last wins).
Powers without `range` (GENERATE_OPS, SHIELD) ignore the target. Target type and room are validated only by
the processor (PROC-PC-USEPOWER). `PWR_SHIELD.energy` is not checked anywhere.

### PC-ENABLE-ROOM — `enableRoom(controller)`
1. `!my` → ERR_NOT_OWNER. 2. `!room` → ERR_BUSY. 3. target-valid: structure registered and instanceof
Structure. 4. not near → ERR_NOT_IN_RANGE. 5. `structureType != 'controller' || (target.safeMode && !target.my)`
→ ERR_INVALID_TARGET. 6. intent `enableRoom: {id}`. `reference/engine/src/game/power-creeps.js:295-317`.

### PC-RENEW — `renew(target)`
1. `!my` → ERR_NOT_OWNER. 2. `!room` → ERR_BUSY. 3. target-valid: structure registered and instanceof
StructurePowerBank or StructurePowerSpawn. 4. power spawn failing `checkStructureAgainstController` →
ERR_RCL_NOT_ENOUGH. 5. not near → ERR_NOT_IN_RANGE. 6. intent `renew: {id}`. **No ownership check of the
power spawn** (client or processor). `reference/engine/src/game/power-creeps.js:319-340`.

### PC-MOVE / PC-MOVETO / PC-MOVEBYPATH / PC-TRANSFER / PC-WITHDRAW / PC-DROP / PC-PICKUP / PC-SAY
Each: `if(!this.room) return ERR_BUSY;` then `Creep.prototype.X.call(this, ...)` (`reference/engine/src/game/power-creeps.js:106-160`).
So ERR_BUSY (unspawned) precedes ERR_NOT_OWNER here, the reverse of usePower/enableRoom/renew. Behaviour then
equals the Creep item, with PC-ABSENT-MEMBERS substitutions; `moveTo` uses `Memory.powerCreeps[name]._move`.
**GOTCHA**: `pc.transfer(controller, RESOURCE_ENERGY)` throws `TypeError` (`this.upgradeController` is not a
function). `pc.move(creep)` is accepted (intent `{id}`).

### PC-NOTIFY-WHEN-ATTACKED
1. `!room` → ERR_BUSY. 2. `!my` → ERR_NOT_OWNER. 3. not boolean → ERR_INVALID_ARGS. 4. write only if different
from merged `notifyWhenAttacked`. `reference/engine/src/game/power-creeps.js:375-393`.

### PC-CANCEL-ORDER
1. `!room` → ERR_BUSY. 2. `!my` → ERR_NOT_OWNER. 3. remove ⇒ OK / ERR_NOT_FOUND. `reference/engine/src/game/power-creeps.js:342-354`.

### PC-POWER-INFO
`C.POWER_INFO[pwr]` (`reference/common/lib/constants.js:854-1016`); fields: `className`; `level[5]` = minimum creep level to buy
power level 1..5 (PC-UPGRADE step 6 indexes by *current* power level); `cooldown` ticks (written as
`cooldownTime = time + cooldown`); `duration` (number or per-level array); `range` (truthy ⇒ target required);
`ops` (number or per-level array); `effect` (per-level array); `period` (regen powers); `energy` (SHIELD,
unused).

| PWR | id | level req | cd | dur | range | ops |
|---|---|---|---|---|---|---|
| GENERATE_OPS | 1 | 0,2,7,14,22 | 50 | – | – | – (effect 1,2,4,6,8 ops) |
| OPERATE_SPAWN | 2 | 0,2,7,14,22 | 300 | 1000 | 3 | 100 |
| OPERATE_TOWER | 3 | 0,2,7,14,22 | 10 | 100 | 3 | 10 |
| OPERATE_STORAGE | 4 | 0,2,7,14,22 | 800 | 1000 | 3 | 100 |
| OPERATE_LAB | 5 | 0,2,7,14,22 | 50 | 1000 | 3 | 10 |
| OPERATE_EXTENSION | 6 | 0,2,7,14,22 | 50 | – | 3 | 2 |
| OPERATE_OBSERVER | 7 | 0,2,7,14,22 | 400 | 200..1000 | 3 | 10 |
| OPERATE_TERMINAL | 8 | 0,2,7,14,22 | 500 | 1000 | 3 | 100 |
| DISRUPT_SPAWN | 9 | 0,2,7,14,22 | 5 | 1..5 | 20 | 10 |
| DISRUPT_TOWER | 10 | 0,2,7,14,22 | 0 | 5 | 50 | 10 |
| DISRUPT_SOURCE | 11 | 0,2,7,14,22 | 100 | 100..500 | 3 | 100 |
| SHIELD | 12 | 0,2,7,14,22 | 20 | 50 | – | – |
| REGEN_SOURCE | 13 | 10,11,12,14,22 | 100 | 300 | 3 | – |
| REGEN_MINERAL | 14 | 10,11,12,14,22 | 100 | 100 | 3 | – |
| DISRUPT_TERMINAL | 15 | 20..24 | 8 | 10 | 50 | 50,40,30,20,10 |
| OPERATE_POWER | 16 | 10,11,12,14,22 | 800 | 1000 | 3 | 200 |
| FORTIFY | 17 | 0,2,7,14,22 | 5 | 1..5 | 3 | 5 |
| OPERATE_CONTROLLER | 18 | 20..24 | 800 | 1000 | 3 | 200 |
| OPERATE_FACTORY | 19 | 0,2,7,14,22 | 800 | 1000 | 3 | 100 |

(Exact effect arrays: cite constants; the per-power processor effects belong to the structures/power spec.)

---

## 7. Processor rules that define API-observable semantics

### PROC-CREEP-PRIORITY (simultaneous actions)
`checkPriorities(intents, name) = intents[name] && !any(priorities[name], i => intents[i])`
(`reference/engine/src/processor/intents/creeps/intents.js:3-23`). An intent is **skipped** if any blocker intent is *present*
(regardless of whether the blocker succeeds):

| intent | blocked by |
|---|---|
| rangedHeal | heal |
| attackController | rangedHeal, heal |
| dismantle | attackController, rangedHeal, heal |
| repair | dismantle, attackController, rangedHeal, heal |
| build | repair, dismantle, attackController, rangedHeal, heal |
| attack | build, repair, dismantle, attackController, rangedHeal, heal |
| harvest | attack, build, repair, dismantle, attackController, rangedHeal, heal |
| rangedMassAttack | build, repair, rangedHeal |
| rangedAttack | rangedMassAttack, build, repair, rangedHeal |

Unrestricted: drop, transfer, withdraw, pickup, heal, move, say, suicide, claimController, upgradeController,
reserveController, generateSafeMode, signController, pull, notifyWhenAttacked. **GOTCHA**: the client returns
OK for both of a conflicting pair; e.g. `heal` silently cancels `attack`/`harvest`/`build`, a failing `heal`
still blocks them.

### PROC-CREEP-ORDER
Execution order per creep: drop, transfer, withdraw, pickup, heal, rangedHeal, dismantle, attack, harvest, move,
repair, build, rangedMassAttack, rangedAttack, say, suicide, claimController, upgradeController,
reserveController, attackController, generateSafeMode, signController, pull (`reference/engine/src/processor/intents/creeps/intents.js:15-17`).
Cross-object/user ordering: processor spec.

### PROC-PC-ORDER
Power creep: move, usePower, withdraw, transfer, say, drop, pickup, enableRoom, renew — no exclusions
(`reference/engine/src/processor/intents/power-creeps/intents.js:3-13`). Global PC intents in order spawn, suicide, delete, upgrade, create, rename per
user (`reference/engine/src/processor/global-intents/power.js:8-28`).

### PROC-OWNERSHIP-FILTER
`if (!object || object._skip || object.user && object.user != userId) continue;` (`reference/engine/src/processor.js:248`).

### PROC-NOTIFY
Applied for creeps, power creeps and constructible structures: `notifyWhenAttacked = !!enabled`
(`reference/engine/src/processor.js:313-315`).

### PROC-ACTIONLOG-RESET
Every tick before intents, creep/power-creep `actionLog` is reset with `say: null` etc.
(`reference/engine/src/processor.js:48-65,151-160`) — `saying` therefore reflects only the previous tick.

### PROC-SAY
Creep: skipped if not `creep` type or spawning or message not a string; stores `{message:
message.substring(0,10), isPublic}` (UTF-16 code units — an emoji counts 2) (`reference/engine/src/processor/intents/creeps/say.js:6-21`). Power creep same
without spawning check (`reference/engine/src/processor/intents/power-creeps/say.js`).

### PROC-SIGN
Creep only, not spawning, target controller within range 1; `sign = text ? {user, text, time, datetime:
Date.now()} : null` (`reference/engine/src/processor/intents/creeps/signController.js:6-28`).

### PROC-MOVE-INTENT
`direction` truthy ⇒ offsets; `id` naming a `creep` at Chebyshev distance 1 ⇒ delta to it (overrides
direction); out-of-room-bounds or static obstacle/hostile non-public rampart ⇒ ignored; power creeps cannot
enter inter-shard portals (`reference/engine/src/processor/intents/creeps/move.js:7-44`; PC uses the same module). Conflict resolution: movement spec.

### PROC-CREEP-AGETIME
`ageTime` is set (`time + CREEP_LIFE_TIME` or `CREEP_CLAIM_LIFE_TIME` if any CLAIM part) the first time the
creep is processed while not spawning (`reference/engine/src/processor/intents/creeps/tick.js:87-91`). Birth (`spawning:false`) happens in the spawn's
tick (`reference/engine/src/processor/intents/spawns/tick.js:17-27`, `reference/engine/src/processor/intents/spawns/_born-creep.js:44-49`); whether the creep's own tick in that same pass sees
`spawning:false` depends on object iteration order ⇒ `ticksToLive` may be undefined for one tick after birth.

### PROC-PC-CREATE / PROC-PC-UPGRADE / PROC-PC-RENAME / PROC-PC-DELETE / PROC-PC-SPAWN / PROC-PC-DIE
- create: re-checks free levels including `user._usedPowerLevels` (per-tick counter), class, **truncates name to
  50** then checks duplicates; inserts level 0, hitsMax 1000, storeCapacity 100, `spawnCooldownTime: 0`.
  **GOTCHA**: client allows 100-char names and checks duplicates on the untruncated name.
- upgrade: free-level re-check (does not count same-tick upgrades), level/class/power-level checks; `level+1`,
  `hitsMax+1000`, `storeCapacity+100`, power level +1; updates room object if spawned.
- rename: unspawned only (`spawnCooldownTime !== null`), name truncated to 50, duplicate check.
- delete: unspawned only; `cancel` ⇒ `deleteTime=null`; else immediate removal if
  `user.powerExperimentationTime > Date.now()`, else (if not already set) `deleteTime = Date.now() +
  POWER_CREEP_DELETE_COOLDOWN` (24 h).
- spawn: power spawn must be owned and not used this tick; PC unspawned and `spawnCooldownTime <= Date.now()`;
  no PC already on that tile; user record `{shard, spawnCooldownTime:null, deleteTime:null}`; room object =
  record + `{type, room, x, y, hits: hitsMax, ageTime: time + 5000, actionLog:{spawned:true},
  notifyWhenAttacked: true}`.
- die (suicide / ttl / hits ≤ 0): tombstone, cargo to container first, room object removed, record
  `{shard:null, spawnCooldownTime: Date.now() + 8 h}`.

### PROC-PC-USEPOWER
Re-checks room power-enabled / hostile safe mode, level, cooldown, ops, range (Chebyshev), effect level; per
power validates target type; applies effect `{effect, power, level, endTime}`; sets `powers[p].cooldownTime`,
deducts ops; event log `EVENT_POWER` (`reference/engine/src/processor/intents/power-creeps/usePower.js`).

### PROC-PC-RENEW-ENABLE
renew: target powerBank/powerSpawn within 1, active power spawn ⇒ `ageTime = time + 5000`. enableRoom: target
controller within 1, not hostile safe mode ⇒ `isPowerEnabled = true`.

---

## 8. PERF summary

- **Room-local**: every Creep/PowerCreep check reads only the actor's room data (registers, spatial index,
  controller). Intents are routed per room; the processor applies them room-locally.
- **Per-user global**: `claimController` (user.rooms, gcl, novice wall clock), all PowerCreep account methods
  (global intent queue, ≤ 50 per type per tick per user), `Game.powerCreeps`.
- **Cross-room**: `moveTo`/`findPath` (PathFinder over up to 16 rooms, visible-room cost matrices of other rooms,
  `costCallback` per room).
- Hot paths in typical bots: `moveTo` (cache lookup + Memory JSON for `_move` + periodic PathFinder),
  `Game.creeps` construction, getters (`store` Proxy allocation), `transfer`/`withdraw` capacity math.
- Original slow spots worth avoiding while keeping results identical: eager wrapper creation for every visible
  object per player per tick; per-player cost-matrix rebuild by scanning all room objects with `_.contains`;
  `build()` full-room scan; tired+visualize repath every tick; cross-room full search reused for one room only;
  `Memory` JSON parse/stringify driven by `_move` writes.

---

## 9. xxscreeps divergences (do not copy)

`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts`: no PowerCreep at all; `move` requires
`Number.isInteger` (`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:538`); `say` truncates to 10 client-side (`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:388`); `notifyWhenAttacked(enabled = true)`
default (`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:344`); memory setter uses `??=` (never overwrites, no throw) (`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:125-130`); `moveTo` searches a path
even when tired without `visualizePathStyle`, validates the cached step with `isNearTo` instead of
`moveByPath`, does not mutate options (`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:240-337`); transfer-to-controller check runs first (`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:410`);
`spawning` derived from `#ageTime === 0` and `saying` requires `saying.time === Game.time` (`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:96,103-108`).
Useful as moddability reference only (intent `save/remove` with per-name slots: `reference/xxscreeps/packages/xxscreeps/game/intents.ts:46-76`).

---

## 10. Open questions

1. `PowerCreep.effects` is never populated by the engine (constructor omits it). Confirm that this is also the
   behaviour on official shards (no effect currently targets power creeps, so it is unobservable today).
2. Ids: vanilla's `assertTargetObject` only fires for 24-char ids; chitin must choose an id format (15-char
   private-server style vs 24-char) — affects that exception and id-length-sensitive bots.
3. Whether chitin must reproduce API-STALE-OBJECT mixed staleness exactly, or may declare it unsupported.
4. `deleteTime` expiry (actual removal of a power creep after 24 h) is not in engine/driver sources read here
   (likely backend cron) — owned by the backend spec.
5. Legacy path finder (MOVETO-LEGACY-PF) depends on `@screeps/pathfinding` A* internals
   (`reference/pathfinding`); exact tie-breaking needs its own spec before 1:1 can be claimed.
6. PROC-CREEP-AGETIME depends on processor object iteration order (DB order of `rooms.objects`); the processor
   spec must define the canonical order for chitin.
