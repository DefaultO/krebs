# 07 — Processor: structure intents, room intents, and non-creep object ticks

Scope: everything the room processor does for objects that are **not** creeps / power creeps:
structure intents, room-level intents (`room` pseudo-object), per-object `tick` handlers, the
shared helpers they use (`_damage`, `_destroy`, `_create-energy`, …), construction-site completion,
ruin creation, NPC invader cores / strongholds, nukes, and the room event-log entries emitted by all
of the above. Creep intents (harvest, build, upgrade, …) and movement are specified elsewhere; they are
only cross-referenced here where they set hidden fields (`_upgraded`, `_cooldown`, road wear) consumed
by the ticks below.

Source baseline: `reference/engine` @8097782 (all `processor/…` paths are relative to
`reference/engine/src/`), constants in `reference/common/lib/constants.js`, bulk writer in
`reference/driver/lib/bulk.js`, lodash 3.10.1 semantics.

Notation: `T` = `gameTime` being processed (the same value player code saw as `Game.time` this tick).
"persist" = written to DB at end of room processing. "in-memory" = visible to the rest of this
room's processing in this tick only.

---

## 0. Inventory

| ID | Item | Phase | Primary source |
|---|---|---|---|
| CONV-NOOP | Processor intents never return codes; invalid = silent no-op | all | processor/intents/** |
| CONV-TIME | "fires at `T >= X-1`" timer convention | tick | e.g. processor/intents/roads/tick.js:10 |
| CONV-BULK | bulk writer semantics (in-memory merge, `_` fields, id-only updates, inserts) | all | reference/driver/lib/bulk.js:33-100 |
| CONV-BULK-INSERTCNT | insert-counter reuse after removing a same-tick insert (lost insert) | all | reference/driver/lib/bulk.js:64-88 |
| CONV-ORDER | roomObjects iteration order; integer-like keys first; snapshot keys | all | processor.js:342 |
| CONV-OWNER-GATE | generic owner gate for object intents | intent | processor.js:248 |
| CONV-CRASH | uncaught exception aborts whole room tick (no writes) | all | processor.js:579 |
| CONV-WALLCLOCK | `Date.now()` dependencies | various | see items |
| CONV-EFFECT-CMP | `>` vs `>=` effect-expiry comparisons per power | various | see §0.2 |
| PROC-PIPELINE | room processing phase order | — | processor.js:16-519 |
| PROC-INIT-ACTIONLOG | per-type actionLog reset at start of tick | init | processor.js:43-171 |
| PROC-INIT-OBSERVER | observer.observeRoom reset | init | processor.js:116-118 |
| PROC-ACTIVATION | conditions that mark the room active for next tick | init/end | processor.js:106-140,499-507 |
| PROC-CALC-SPAWNS | spawn/extension `off` flags | pre-intent | processor/intents/_calc_spawns.js:6-41 |
| PROC-DISPATCH-INTENTS | per-user, per-object structure intent dispatch | intent | processor.js:229-321 |
| PROC-NOTIFY-ATTACKED | generic `notifyWhenAttacked` intent | intent | processor.js:313-315 |
| PROC-ENERGY-AVAILABLE | `scope.energyAvailable` | post-intent | processor.js:326 |
| PROC-DISPATCH-TICK | per-object tick dispatch & order | tick | processor.js:342-411 |
| PROC-COLLAPSE | generic EFFECT_COLLAPSE_TIMER removal | tick | processor.js:412-419 |
| PROC-DECAY-PB-DEPOSIT | generic powerBank/deposit decay removal | tick | processor.js:421-426 |
| PROC-HOOKS | mod hook points (`driver.config.emit`) | all | processor.js:169,246,318,482,489 |
| HELP-CREATE-ENERGY | drop resource (container first, merge pile) | helper | processor/intents/_create-energy.js:6-54 |
| HELP-DAMAGE | damage application to structures (+hit-back, events) | helper | processor/intents/_damage.js:6-94 |
| HELP-DESTROY | structure destruction + ruin creation | helper | processor/intents/structures/_destroy.js:6-54 |
| HELP-CLEAR-NEWBIE-WALLS | remove all owned decaying walls | helper | processor/intents/creeps/_clear-newbie-walls.js |
| HELP-STRUCT-ACTIVE | `checkStructureAgainstController` | helper | utils.js:456-507 |
| HELP-CTRL-AVAIL | `checkControllerAvailability` | helper | utils.js:338-354 |
| HELP-CSITE-CHECK | `checkConstructionSite` | helper | utils.js:128-189 |
| HELP-DIST-SORT | `comparatorDistance` | helper | utils.js:394-403 |
| HELP-CALC-REWARD | `calcReward` (stronghold loot) incl. density bug | helper | utils.js:677-698 |
| HELP-TERMINAL-COST | `calcTerminalEnergyCost`, `calcRoomsDistance` | helper | utils.js:644-659 |
| HELP-REACTIONS | `getReactionVariants`, `calcTotalReactionsTime` | helper | utils.js:665-669,700-710 |
| HELP-ATTACK-NOTIFY | `sendAttackingNotification` | helper | utils.js:438-454 |
| ROOM-INTENTS | room pseudo-object intent order & flag parse/flush | intent | processor/intents/room/intents.js:3-53 |
| ROOM-FLAGS-MULTIUSER | flag re-parse per user: lost updates / crash | intent | processor/intents/room/intents.js:7-10,42-52 |
| ROOM-CREATE-FLAG | createFlag | intent | processor/intents/room/create-flag.js:6-34 |
| ROOM-REMOVE-FLAG | removeFlag | intent | processor/intents/room/remove-flag.js:6-20 |
| ROOM-CREATE-CSITE | createConstructionSite | intent | processor/intents/room/create-construction-site.js:9-75 |
| ROOM-REMOVE-CSITE | removeConstructionSite (+refund) | intent | processor/intents/room/remove-construction-site.js:6-20 |
| ROOM-DESTROY-STRUCTURE | destroyStructure | intent | processor/intents/room/destroy-structure.js:6-26 |
| ROOM-GEN-ENERGY | genEnergy (legacy, unreachable) | intent | processor/intents/room/gen-energy.js:6-28 |
| FLAG-OBJ-DEAD | flags/intents.js is dead code | — | processor/intents/flags/intents.js |
| CSITE-TICK | construction site tick (no-op) | tick | processor/intents/construction-sites/tick.js |
| CSITE-COMPLETE | structure creation on build completion | intent(creep) | processor/intents/creeps/build.js:103-293 |
| CSITE-STOMP | hostile creep stepping on site removes it | tick(creep) | processor/intents/movement.js:223-233 |
| CTRL-RESERVATION-EXPIRE | reservation removal | tick | processor/intents/controllers/tick.js:10-12 |
| CTRL-OWNER-ROOMS | addRoomToUser each tick | tick | processor/intents/controllers/tick.js:18 |
| CTRL-UPGRADE-BLOCKED | persist `_upgradeBlocked` | tick | processor/intents/controllers/tick.js:20-23 |
| CTRL-SAFEMODE | activateSafeMode intent + tick apply | intent/tick | processor/intents/controllers/activateSafeMode.js:6-25; controllers/tick.js:25-31 |
| CTRL-DOWNGRADE-INIT | initialise downgradeTime | tick | processor/intents/controllers/tick.js:33-36 |
| CTRL-UPGRADE-RESTORE | downgradeTime extension after upgrade | tick | processor/intents/controllers/tick.js:38-43 |
| CTRL-DOWNGRADE | level loss loop & notifications | tick | processor/intents/controllers/tick.js:45-82 |
| CTRL-UNCLAIM | unclaim | intent | processor/intents/controllers/unclaim.js:6-28 |
| CTRL-LEVELUP-XREF | level-up/progress (creep upgrade) cross-ref | intent(creep) | processor/intents/creeps/upgradeController.js:59-103 |
| SPAWN-CREATE | createCreep | intent | processor/intents/spawns/create-creep.js:7-101 |
| SPAWN-CHARGE | energy charging (old / energyStructures) | helper | processor/intents/spawns/_charge-energy.js:6-78 |
| SPAWN-RENEW | renewCreep | intent | processor/intents/spawns/renew-creep.js:6-55 |
| SPAWN-RECYCLE | recycleCreep | intent | processor/intents/spawns/recycle-creep.js:6-23 |
| SPAWN-SET-DIRECTIONS | setSpawnDirections | intent | processor/intents/spawns/set-spawn-directions.js:6-21 |
| SPAWN-CANCEL | cancelSpawning | intent | processor/intents/spawns/cancel-spawning.js:6-13 |
| SPAWN-INTENT-ORDER | order of spawn intents in one tick | intent | processor/intents/spawns/intents.js:1-17 |
| SPAWN-TICK-SPAWNING | spawning progress, PWR_DISRUPT_SPAWN | tick | processor/intents/spawns/tick.js:12-30 |
| SPAWN-BORN | creep placement & spawn-stomp | helper | processor/intents/spawns/_born-creep.js:7-79 |
| SPAWN-TICK-REGEN | +1 energy/tick regeneration | tick | processor/intents/spawns/tick.js:32-47 |
| EXT-TICK | extension capacity by RCL | tick | processor/intents/extensions/tick.js:6-20 |
| TOWER-INTENT-ORDER | heal > repair > attack, one per tick | intent | processor/intents/towers/intents.js:2-10 |
| TOWER-FALLOFF | range falloff + power multipliers | intent | processor/intents/towers/attack.js:32-46 |
| TOWER-ATTACK | attack | intent | processor/intents/towers/attack.js:6-70 |
| TOWER-HEAL | heal | intent | processor/intents/towers/heal.js:6-53 |
| TOWER-REPAIR | repair | intent | processor/intents/towers/repair.js:6-56 |
| TOWER-TICK | actionLog persist | tick | processor/intents/towers/tick.js:7-17 |
| LINK-TRANSFER | transferEnergy (loss, cooldown) | intent | processor/intents/links/transfer.js:6-54 |
| LINK-TICK | cooldown decrement | tick | processor/intents/links/tick.js:7-30 |
| LAB-INTENT-ORDER | boost, unboost, run XOR reverse | intent | processor/intents/labs/intents.js:2-14 |
| LAB-STORE-MODEL | lab storeCapacity / storeCapacityResource switching | — | processor/intents/labs/run-reaction.js:53-86 |
| LAB-RUN | runReaction | intent | processor/intents/labs/run-reaction.js:6-89 |
| LAB-REVERSE | reverseReaction | intent | processor/intents/labs/reverse-reaction.js:6-91 |
| LAB-BOOST | boostCreep | intent | processor/intents/labs/boost-creep.js:6-62 |
| LAB-UNBOOST | unboostCreep | intent | processor/intents/labs/unboost-creep.js:6-53 |
| LAB-TICK | actionLog persist | tick | processor/intents/labs/tick.js:7-13 |
| PSPAWN-PROCESS | processPower | intent | processor/intents/power-spawns/process-power.js:7-39 |
| FACTORY-PRODUCE | produce | intent | processor/intents/factories/produce.js:6-44 |
| FACTORY-TICK | capacity by RCL/owner, actionLog | tick | processor/intents/factories/tick.js:6-19 |
| TERMINAL-SEND | send (room phase: validate & stash) | intent | processor/intents/terminal/send.js:6-33 |
| TERMINAL-SEND-EXEC | send execution + cooldown (global phase) | global | processor/global-intents/market.js:15-97 |
| TERMINAL-TICK | capacity by RCL/owner | tick | processor/intents/terminal/tick.js:6-15 |
| STORAGE-TICK | capacity by RCL/owner + PWR_OPERATE_STORAGE | tick | processor/intents/storages/tick.js:6-19 |
| OBSERVER-OBSERVE | observeRoom intent + per-tick write | intent/tick | processor.js:284-288,403-406 |
| NUKER-LAUNCH | launchNuke (cross-room insert) | intent | processor/intents/nukers/launch-nuke.js:8-51 |
| NUKE-PRETICK | cancel createCreep intents on landing tick | pretick | processor/intents/nukes/pretick.js:6-15 |
| NUKE-NOVICE | nuke removed in novice/respawn room | tick | processor/intents/nukes/tick.js:10-14 |
| NUKE-LAND | landing effects | tick | processor/intents/nukes/tick.js:16-78 |
| NUKE-REMOVE | removal one tick after landing | tick | processor/intents/nukes/tick.js:80-83 |
| RAMPART-SET-PUBLIC | setPublic | intent | processor/intents/ramparts/set-public.js:6-11 |
| RAMPART-SHIELD | PWR_SHIELD temporary rampart expiry | tick | processor/intents/ramparts/tick.js:10-17 |
| RAMPART-HITSMAX | hitsMax by RCL/owner | tick | processor/intents/ramparts/tick.js:19-24 |
| RAMPART-DECAY | RAMPART_DECAY_AMOUNT / RAMPART_DECAY_TIME | tick | processor/intents/ramparts/tick.js:26-40 |
| ROAD-DECAY | road decay incl. terrain multipliers | tick | processor/intents/roads/tick.js:6-35 |
| ROAD-WEAR | creep/power-creep wear on roads | tick(creep) | processor/intents/movement.js:211-222 |
| WALL-HITSMAX | constructedWall hitsMax by RCL | tick | processor/intents/constructedWalls/tick.js:10-15 |
| WALL-NEWBIE | decaying (newbie/respawn) walls | tick | processor/intents/constructedWalls/tick.js:17-44 |
| CONTAINER-DECAY | owned/unowned decay, drop store | tick | processor/intents/containers/tick.js:6-35 |
| EXTRACTOR-TICK | cooldown decrement / set from harvest | tick | processor/intents/extractors/tick.js:7-27 |
| SOURCE-REGEN | ENERGY_REGEN_TIME refill | tick | processor/intents/sources/tick.js:10-29 |
| SOURCE-PWR | PWR_DISRUPT_SOURCE / PWR_REGEN_SOURCE | tick | processor/intents/sources/tick.js:17-39 |
| SOURCE-CAPACITY | capacity by ownership/reservation/keeper | tick | processor/intents/sources/tick.js:46-59 |
| MINERAL-REGEN | MINERAL_REGEN_TIME refill | tick | processor/intents/minerals/tick.js:8-37 |
| MINERAL-DENSITY | density reroll (randomness) | tick | processor/intents/minerals/tick.js:19-34 |
| MINERAL-PWR | PWR_REGEN_MINERAL | tick | processor/intents/minerals/tick.js:39-47 |
| DEPOSIT-COOLDOWN | cooldownTime from harvest `_cooldown` | tick | processor/intents/deposits/tick.js:7-11 |
| DEPOSIT-DECAY | decay/removal | tick | processor/intents/deposits/tick.js:13-16; processor.js:421-426 |
| POWERBANK-DECAY | decay/removal | tick | processor.js:421-426 |
| POWERBANK-HITBACK | POWER_BANK_HIT_BACK | helper | processor/intents/_damage.js:35-37,86-91 |
| POWERBANK-RUIN | destroyed power bank → 10-tick ruin with power | helper | processor/intents/structures/_destroy.js:21-50 |
| PORTAL-TICK | unstable → decay, removal + paired wall | tick | processor/intents/portals/tick.js:6-25 |
| KEEPERLAIR-TICK | keeper respawn timer | tick | processor/intents/keeper-lairs/tick.js:6-72 |
| ENERGY-DECAY | dropped resource decay | tick | processor/intents/energy/tick.js:6-24 |
| TOMBSTONE-DECAY | tombstone expiry → drop store | tick | processor/intents/tombstones/tick.js:6-20 |
| RUIN-DECAY | ruin expiry → drop store | tick | processor/intents/ruins/tick.js:6-19 |
| CORE-PRETICK | invader core AI context build | pretick | processor/intents/invader-core/pretick.js:7-60 |
| CORE-BEHAVIORS | behaviour table deploy/default/bunker1-5 | pretick | processor/intents/invader-core/stronghold/stronghold.js:388-491 |
| CORE-HANDLE-CTRL | reserve/attack/upgrade selection | pretick | stronghold.js:135-149 |
| CORE-DEPLOY | stronghold deployment | pretick | stronghold.js:22-133 |
| CORE-REFILL-TOWERS | random tower refill | pretick | stronghold.js:151-167 |
| CORE-REFILL-CREEPS | creep refill | pretick | stronghold.js:169-182 |
| CORE-TOWERS-MAINT | tower heal/repair | pretick | stronghold.js:184-206 |
| CORE-FOCUS-CLOSEST | target closest hostile | pretick | stronghold.js:208-238 |
| CORE-FOCUS-MAX | target max-damage hostile | pretick | stronghold.js:240-299 |
| CORE-POPULATION | population decks & maintainCreep | pretick | stronghold.js:301-332,408-480 |
| CORE-ANTINUKE | rampart hitsTarget vs nukes | pretick | stronghold.js:334-361 |
| CORE-ASSIGN-DEFENDERS | rampart spot distribution | pretick | stronghold.js:363-386; stronghold/defence.js:37-56 |
| CORE-CREEP-AI | coordinated / simple-melee / fortifier + bodies | pretick | stronghold/creeps.js:15-126; stronghold/defence.js:8-35 |
| CORE-TRANSFER | core → tower/creep energy | intent | processor/intents/invader-core/transfer.js:6-38 |
| CORE-CREATE-CREEP | core spawns stronghold creep | intent | processor/intents/invader-core/create-creep.js:7-66 |
| CORE-RESERVE | core reserveController | intent | processor/intents/invader-core/reserveController.js:6-40 |
| CORE-ATTACK-CTRL | core attackController | intent | processor/intents/invader-core/attackController.js:6-42 |
| CORE-UPGRADE-CTRL | core upgradeController | intent | processor/intents/invader-core/upgradeController.js:8-55 |
| CORE-TICK | collapse, spawning, actionLog | tick | processor/intents/invader-core/tick.js:6-47 |
| CORE-DESTROY | rewards & controller reset on destruction | helper | processor/intents/invader-core/destroy.js:7-38 |
| STRONGHOLD-SPAWN | backend placement of a stronghold (cron/CLI) | cron | reference/backend/lib/strongholds.js:14-128 |
| STRONGHOLD-GEN | per-sector generation cron | cron | reference/backend/lib/strongholds.js:130-161,225-236 |
| STRONGHOLD-EXPAND | level-0 core expansion cron | cron | reference/backend/lib/strongholds.js:238-349 |
| CRON-POWERBANK | power bank spawning cron | cron | reference/backend/lib/cronjobs.js:194-243 |
| CRON-DEPOSIT | deposit spawning cron | cron | reference/backend/lib/cronjobs.js:556-643 |
| EVT-TABLE | event-log entries emitted by this area | all | see §14 |
| RT-XREF | runtime pre-validation & return codes, mismatches | runtime | reference/engine/src/game/structures.js |

Inventory count: 146 items.

### 0.1 Constants used (by name; values for convenience)

All in `reference/common/lib/constants.js`: RAMPART_DECAY_AMOUNT 300 / RAMPART_DECAY_TIME 100 /
RAMPART_HITS 1 / RAMPART_HITS_MAX (:131-134); ENERGY_REGEN_TIME 300, ENERGY_DECAY 1000 (:136-137);
SPAWN_HITS 5000, SPAWN_ENERGY_CAPACITY 300, CREEP_SPAWN_TIME 3, SPAWN_RENEW_RATIO 1.2 (:139-143);
SOURCE_ENERGY_CAPACITY 3000 / NEUTRAL 1500 / KEEPER 4000 (:145-147); WALL_HITS 1, WALL_HITS_MAX 3e8
(:149-150); EXTENSION_HITS 1000, EXTENSION_ENERGY_CAPACITY (:152-153); ROAD_HITS 5000, ROAD_WEAROUT 1,
ROAD_WEAROUT_POWER_CREEP 100, ROAD_DECAY_AMOUNT 100, ROAD_DECAY_TIME 1000 (:155-159); LINK_* (:161-165);
STORAGE_* (:167-168); CONSTRUCTION_COST (:192-209); CONSTRUCTION_COST_ROAD_SWAMP_RATIO 5 / _WALL_RATIO 150
(:210-211); CONTROLLER_LEVELS (:213); CONTROLLER_STRUCTURES (:214-231); CONTROLLER_DOWNGRADE (:232);
CONTROLLER_DOWNGRADE_RESTORE 100, _SAFEMODE_THRESHOLD 5000, CONTROLLER_CLAIM_DOWNGRADE 300,
CONTROLLER_RESERVE 1, CONTROLLER_RESERVE_MAX 5000, CONTROLLER_ATTACK_BLOCKED_UPGRADE 1000,
CONTROLLER_NUKE_BLOCKED_UPGRADE 200 (:233-240); SAFE_MODE_DURATION 20000, SAFE_MODE_COOLDOWN 50000
(:242-243); TOWER_* (:246-254); OBSERVER_RANGE 10 (:257); POWER_BANK_* (:259-264); POWER_SPAWN_* (:266-269);
EXTRACTOR_COOLDOWN 5 (:272); LAB_* (:274-282); MINERAL_REGEN_TIME 50000, MINERAL_DENSITY,
MINERAL_DENSITY_PROBABILITY {1:.1,2:.5,3:.9,4:1}, MINERAL_DENSITY_CHANGE .05, DENSITY_* (:298-327);
DEPOSIT_EXHAUST_* , DEPOSIT_DECAY_TIME 50000 (:329-331); TERMINAL_COOLDOWN 10 (:337); CONTAINER_HITS
250000, CONTAINER_CAPACITY 2000, CONTAINER_DECAY 5000, CONTAINER_DECAY_TIME 100, _OWNED 500 (:339-343);
NUKER_* / NUKE_LAND_TIME 50000 / NUKE_RANGE 10 / NUKE_DAMAGE {0:1e7, 2:5e6} (:345-355); FACTORY_* (:356-357);
TOMBSTONE_DECAY_PER_PART 5 (:359); RUIN_DECAY 500, RUIN_DECAY_STRUCTURES {powerBank:10} (:362-365);
PORTAL_DECAY 30000 (:367); POWER_BANK_RESPAWN_TIME (:774); EVENT_* (:785-806); EFFECT_INVULNERABILITY
1001, EFFECT_COLLAPSE_TIMER 1002 (:839-840); INVADER_CORE_* and STRONGHOLD_* (:842-850); POWER_INFO (:854+).

### 0.2 Effect-expiry comparisons (CONV-EFFECT-CMP)

Power effects are `{power, level, endTime}`; the comparison is not uniform — replicate per site:

| Site | Active when | Source |
|---|---|---|
| PWR_DISRUPT_SOURCE / PWR_REGEN_SOURCE | `endTime > T` | processor/intents/sources/tick.js:18,32 |
| PWR_REGEN_MINERAL | `endTime > T` | processor/intents/minerals/tick.js:40 |
| PWR_DISRUPT_SPAWN (tick), PWR_OPERATE_SPAWN (create) | `endTime > T` | spawns/tick.js:14; spawns/create-creep.js:52 |
| PWR_OPERATE_TOWER / PWR_DISRUPT_TOWER | `endTime > T` | towers/attack.js:42 |
| PWR_OPERATE_LAB | `endTime > T` | labs/run-reaction.js:14 |
| PWR_OPERATE_STORAGE | `endTime > T` | storages/tick.js:11 |
| PWR_OPERATE_POWER | `endTime >= T` | power-spawns/process-power.js:18 |
| PWR_OPERATE_TERMINAL (room phase cost check) | `endTime >= T` | terminal/send.js:23 |
| PWR_OPERATE_TERMINAL (global exec cost & cooldown) | `endTime > T` | global-intents/market.js:38,86 |
| PWR_OPERATE_FACTORY | `endTime >= T` | factories/produce.js:21 |
| PWR_FORTIFY / EFFECT_INVULNERABILITY (wall/rampart immunity) | `endTime > T` | _damage.js:27 |
| PWR_SHIELD rampart removal | removed when `endTime <= T` | ramparts/tick.js:12 |
| EFFECT_COLLAPSE_TIMER removal | removed when `endTime <= T` | processor.js:414 |

**GOTCHA**: at `endTime == T`, OPERATE_TERMINAL validates with the discounted cost in the room phase
but executes with the full cost in the global phase; the global energy check may then fail → no
transfer, no cooldown, `send` cleared.

---

## 1. Conventions

### CONV-NOOP
Every processor intent handler validates and returns silently on failure (no return codes, no
events). Return codes exist only in the runtime (player sandbox) pre-checks, see RT-XREF (§15). The
processor re-validates a *subset* of the runtime checks, sometimes with different comparisons; where
it is laxer, crafted/modded intents can do things the API forbids (noted per item).

### CONV-TIME
Absolute-tick timers are checked as `T >= X - 1` (sources, minerals, roads, ramparts, containers,
ruins, tombstones, spawning, keeper lairs, controller downgrade, power banks, deposits via
processor.js:422). Effect: the change is applied while processing tick `X-1` and is first visible to
players at tick `X`, i.e. exactly when `ticksToX = X - Game.time` would reach 0. Exceptions using
strict `T > X` (one tick later than the getter suggests): portals (portals/tick.js:15), deposits'
own check (deposits/tick.js:13, shadowed by processor.js:422), nuke removal `T >= landTime`
(nukes/tick.js:80) while landing happens at `T == landTime-1`.

### CONV-BULK (reference/driver/lib/bulk.js)
- `bulk.update(objRef, data)` (:33-63): deep-clones `data`; for each object-valued key, `_.merge`s it
  into a copy of the current value (so store diffs like `{store:{energy:5}}` keep other keys);
  then `_.merge(objRef, data)` — **the in-memory object is mutated immediately**, so later handlers
  in the same tick see the new values. Arrays merge index-wise; to replace an object/array the code
  uses `update(o,{k:null}); update(o,{k:newValue})` (e.g. spawns/set-spawn-directions.js:17-18,
  invader-core/upgradeController.js:46-50). `undefined` source values are skipped by `_.merge`.
- `bulk.update(idString, data)` does **not** touch the in-memory object (used by _calc_spawns.js:20,38
  and room/intents.js:47) — **GOTCHA** `off` flags written this way stay stale in memory for the rest
  of the tick.
- Keys starting with `_` are stripped before persisting (:6-23): `_off`, `_cooldown`, `_upgraded`,
  `_upgradeBlocked`, `_safeModeActivated`, `_actionLog`, `_damageToApply`, … are tick-local.
- `bulk.update(obj)` where `obj._id` is undefined (objects created this tick and stashed in
  `roomObjects` under synthetic keys) mutates memory but persists nothing (writes to id "undefined").
- `bulk.insert(data)` (:64-77) clones; the inserted object is **not** in `roomObjects` unless the caller
  adds it. Returns a per-bulk counter (1,2,3,…); only `_create-energy` uses it as `_id`.
- `bulk.inc` (:90-100) mutates memory and queues `$inc`.
- Execute order (:121-130): `remove`/`inc`/`addToSet`/`pull` ops in call order, then all `$set` updates,
  then inserts.

### CONV-BULK-INSERTCNT
`remove(id)` of an id present in `inserts` deletes it **and decrements `insertCnt`** (:83-86); the
next `insert` reuses the counter value of the *last* insert and overwrites it. Reachable: a 1-unit
drop created in the intent phase decays to 0 in the same tick (ENERGY-DECAY) and is removed; any
later insert in that tick (tombstone, ruin, another drop) overwrites the most recent pending insert.
**GOTCHA** (lost object). See Open questions.

### CONV-ORDER
`roomObjects` is a plain object keyed by id. Both the intent phase (`for objectId in
userIntents.objects`) and tick phase (`_.forEach(roomObjects)`, processor.js:342) depend on key
order: integer-like keys (`_create-energy` ids "1","2",…) enumerate first in ascending order, then
string keys in insertion order (DB load order, then synthetic keys `_createdConstructionSite<n>`,
`createdStructure<n>`, `deployedStructure<n>` added during the tick). lodash 3 `_.forEach` snapshots
keys at start: objects added during the tick loop are not ticked; deleted objects yield `undefined`
and are skipped (processor.js:344). Objects added during the intent phase **are** ticked (e.g. a
fresh drop decays in its creation tick, a just-built extension gets its capacity tick in memory only).
Tie-breaks in `checkStructureAgainstController` and stable sorts also follow this order.

### CONV-OWNER-GATE (processor.js:248)
Object intents are skipped if `!object || object._skip || (object.user && object.user != userId)`.
**GOTCHA**: objects without `user` (roads, walls, containers, portals, power banks…) accept object
intents from any user (only `notifyWhenAttacked` is reachable for them).

### CONV-CRASH
`processRoom` errors are caught and logged (processor.js:579); the bulk writers are never executed,
so **nothing** from that room tick persists (side effects already performed: notifications sent,
`driver.config.emit` hooks). Several reachable crash paths are flagged below.

### CONV-WALLCLOCK
`Date.now()` is consulted for: `roomInfo.novice` / `respawnArea` (nukes/tick.js:10,
nukers/launch-nuke.js:22, controllers safeModeCooldown), portal `unstableDate` (portals/tick.js:8),
wall `decayTime.timestamp` (constructedWalls/tick.js:39-43). Room activation by cron is also
wall-clock based (PROC-ACTIVATION). chitin must decide whether these are real time or a virtual clock.

---

## 2. Room processing pipeline (structure-relevant parts)

### PROC-PIPELINE (processor.js:16-519)
1. Load objects, intents, terrain, gameTime, roomInfo, flags.
2. **Init loop** over roomObjects (:43-171): reset actionLogs (PROC-INIT-ACTIONLOG), reset
   `observer.observeRoom = null` (:116-118), collect spawns/extensions/nukes/keepers/invaders/core,
   compute activation flags, emit `processObject` hook.
3. **Pretick**: nukes (NUKE-PRETICK, :176-178), keeper creeps, non-stronghold invader creeps, then
   invader core (CORE-PRETICK, :208-219). Pretick intents are merged as `_.assign(generated, existing)`
   — **player/earlier intents of the same name win** over generated ones.
4. `_calc_spawns` if any spawn/extension (:221-223).
5. `movement.init` (:225).
6. **Intent phase** (:229-321): for each user in `intents.users` key order, merge `objectsManual`
   (backend UI intents) into `objects`, then for each objectId: `room` → ROOM-INTENTS; else gate
   (CONV-OWNER-GATE) and dispatch by type (PROC-DISPATCH-INTENTS), then `notifyWhenAttacked`.
7. `movement.check` (:324).
8. `scope.energyAvailable` (:326).
9. **Tick phase** (:342-483): per object, type tick (PROC-DISPATCH-TICK), then PROC-COLLAPSE,
   PROC-DECAY-PB-DEPOSIT, history snapshot, map view, `postProcessObject` hook.
10. `processRoom` hook (:489); save map view, bulk objects/users/flags, event log (:493-497); activate
    room / save history (:499-507); save roomInfo if changed (:509-511).

**PERF**: everything in this document except NUKER-LAUNCH (cross-room insert), TERMINAL-SEND-EXEC
(global phase), user-collection writes (rooms list, power, gcl), notifications, stats and the
backend crons is room-local and can run in parallel per room.

### PROC-INIT-ACTIONLOG (processor.js:43-171)
`_actionLog = actionLog` then reset to all-null keys for: link `{transferEnergy}`, lab
`{runReaction, reverseReaction}`, tower `{attack, heal, repair}`, factory `{produce}`, invaderCore
`{transferEnergy, reserveController, attackController, upgradeController}`. Each type's tick persists
`actionLog` only if `!_.isEqual(_actionLog, actionLog)` (link persists unconditionally while on
cooldown, links/tick.js:18-21). Keys added later (e.g. tower `actionLog.attacked` from `_damage`) are
persisted as part of the object.

### PROC-ACTIVATION
Room is queued for processing next tick (`driver.activateRoom`) if any (processor.js:106-140, 499-507):
invader core with `deployTime`; controller reserved by user "2" with
`endTime - T < CONTROLLER_RESERVE_MAX - INVADER_CORE_CONTROLLER_POWER*CONTROLLER_RESERVE`; controller
owned by a user other than "2"; any object with `user` that is not "3", not `userNotActive`, not a
flag, not stronghold (`strongholdId`), not the controller; powerBank/deposit with `T > decayTime-500`;
any dropped resource, nuke, tombstone, portal; or `roomInfo.active` set during processing (core
attackController, core actionLog change). Rooms not activated are **not processed at all** next tick
(driver/lib/index.js:170-174 pops the set); they are force-activated by the backend cron
`roomsForceUpdate` every ~90-109 ticks (reference/backend/lib/cronjobs.js:178-192, run every 20 s wall
time).
**GOTCHA**: all "per-tick" timers in dormant rooms (roads, containers, sources, minerals, keeper
lairs, stronghold AI) only advance when processed; a timer that expired while dormant fires once
and reschedules from the processing tick (decay is slower in unvisited rooms). Event logs of
dormant rooms are not cleared (driver keeps the last hset).
**PERF**: this is the original's main scaling trick; chitin should keep the active-set model (or an
equivalent wake-time scheduler like xxscreeps `context.wakeAt`,
reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:275-282) to be both fast and 1:1.

### PROC-CALC-SPAWNS (processor/intents/_calc_spawns.js:6-41)
For spawns then extensions (lists collected in init loop, all owners):
```
allowed = CONTROLLER_STRUCTURES[type][roomController.level|0]
if count > allowed: sort by Chebyshev distance to controller (stable), first `allowed` get _off=false, rest true
else all _off=false
persist off=_off by id where _off !== off (in-memory `off` NOT updated)
```
**GOTCHA**: throws if spawns/extensions exist in a room without controller (`roomController.level`).
**GOTCHA**: `off` used by `_charge-energy`, `energyAvailable`, runtime `spawnCreep` is last tick's value.

### PROC-DISPATCH-INTENTS (processor.js:250-311)
link→links/intents (transfer); tower→towers/intents; lab→labs/intents; spawn→spawns/intents;
rampart→`setPublic`; terminal→`send`; nuker→`launchNuke`; observer→`observeRoom` (inline);
powerSpawn→`processPower`; invaderCore→invader-core/intents; factory→`produce`;
controller→`unclaim` then `activateSafeMode`. Then generic notifyWhenAttacked.

### PROC-NOTIFY-ATTACKED (processor.js:313-315)
If intent present and (`CONSTRUCTION_COST[type]` or creep/powerCreep): persist
`notifyWhenAttacked = !!enabled`.

### PROC-ENERGY-AVAILABLE (processor.js:326)
`sum(store.energy)` over all spawns+extensions with falsy `off` (any owner), computed after intents.

### PROC-DISPATCH-TICK (processor.js:348-410)
Fixed if-chain per object (only one matches): invaderCore, energy, source, deposit, mineral, creep,
powerCreep, spawn, rampart, extension, road, constructionSite, keeperLair, portal, constructedWall,
link, extractor, tower, controller, lab, container, terminal, tombstone, ruin, factory, nuke,
observer (inline write), storage. No tick for: powerSpawn, nuker, powerBank (generic decay only),
flags. Order *between* objects is CONV-ORDER — e.g. a nuke landing kills creeps that already
ticked this tick, a creep moving onto a road before the road's own tick advances that road's decay
in the same tick.

### PROC-COLLAPSE (processor.js:412-419)
After the type tick: if `effects` contains EFFECT_COLLAPSE_TIMER with `endTime <= T`, remove the
object (no ruin, no event, no drop) and skip history/map view. Applies to all stronghold
structures, the core (after CORE-TICK) and anything else carrying the effect.

### PROC-DECAY-PB-DEPOSIT (processor.js:421-426)
powerBank / deposit: `T >= decayTime - 1` → remove (no ruin, no event, no drop). Object still goes
to history/map view this tick.

### PROC-HOOKS
`processObject(object, roomObjects, roomTerrain, T, roomInfo, bulk, bulkUsers)` (:169),
`preProcessObjectIntents(object, userId, objectIntents, …)` (:246, called even when object missing),
`processObjectIntents(…)` (:318), `postProcessObject(object, …, eventLog, mapView)` (:482),
`processRoom(roomId, roomInfo, roomObjects, roomTerrain, T, bulk, bulkUsers, eventLog)` (:489),
`strongholdDestroyed(core, scope)` (invader-core/destroy.js:37). Custom intent types are allowed
through `sanitizeUserIntents(…, customIntentTypes)` (reference/common/lib/system.js:95-140). These are
the original's mod surface; chitin's mod API should expose equivalents.

---

## 3. Shared helpers

### HELP-CREATE-ENERGY (processor/intents/_create-energy.js:6-54)
`createEnergy(x, y, room, amount, resourceType='energy')`:
1. `amount = Math.round(amount)`; return if `<= 0`.
2. Container at (x,y) (first found) with `hits > 0`: put `min(amount, storeCapacity - sum(store))` in it.
3. Remainder: if an `energy` object with same x,y **and same `resourceType` field** exists, add to it;
   else insert `{type:'energy', x, y, room, [resourceType]: amount, resourceType}` and add to
   roomObjects under `_id = "" + insertCounter`.
**GOTCHA**: piles lacking a `resourceType` field (legacy, ROOM-GEN-ENERGY) never merge.
**PERF**: two full-room scans per call; use a per-tile index.

### HELP-DAMAGE (processor/intents/_damage.js:6-94) — structure-relevant semantics
`damage(attacker, target, damage, attackType)`:
1. Return if `!target._id || !target.hits` (fresh same-tick objects and hit-less objects immune).
2. creep/powerCreep targets accumulate `_damageToApply` (applied in their tick; melee hit-back
   for creeps is creep-spec). Structures: if attackType ≠ NUKE and target is constructedWall/rampart
   with an active PWR_FORTIFY or EFFECT_INVULNERABILITY effect (`endTime > T`) → **return** (no
   event, no notification). Else `target.hits -= damage` (in memory).
3. powerBank: `attackBackPower = damage * POWER_BANK_HIT_BACK` (POWERBANK-HITBACK).
4. attackBackPower = 0 if room controller is owned by attacker's user and in safe mode.
5. If target is constructedWall with `decayTime` → HELP-CLEAR-NEWBIE-WALLS (hits change **not**
   persisted; unowned decaying walls keep their DB hits). Else if `hits <= 0` and not a creep:
   HELP-DESTROY(target, attackType) then push EVENT_OBJECT_DESTROYED `{type}`. Else persist hits.
6. Attacker creep actionLog attack/rangedAttack; `target.actionLog.attacked = attacker pos` if target
   has an actionLog.
7. If attacker user not "2"/"3": notify target owner if `notifyWhenAttacked`; set
   `roomInfo.lastPvpTime = T` if both have users, differ, and target user not NPC.
8. If attackBackPower: `attacker._damageToApply += it`, `attacker.actionLog.attacked = target pos`,
   push EVENT_ATTACK `{targetId: attacker, damage: attackBackPower, attackType: HIT_BACK}` with
   `objectId = target`.
9. Push EVENT_ATTACK `{targetId, damage, attackType}` with `objectId = attacker`.
Event order for a kill: OBJECT_DESTROYED, [HIT_BACK], ATTACK.
**GOTCHA** (crash): a nuke landing on a power bank sets attackBackPower and then dereferences
`nuke.actionLog` (undefined) → CONV-CRASH; the whole landing tick is lost and the nuke is removed
next tick without effect. A tower attacking a power bank gets `_damageToApply` (never applied) and
`actionLog.attacked`, plus a hit-back event.

### HELP-DESTROY (processor/intents/structures/_destroy.js:6-54)
`destroy(object, attackType?)`:
1. spawn with `spawning`: remove the first object with `{user, name: spawning.name}`.
2. invaderCore: CORE-DESTROY (mutates `object.store` with rewards).
3. Unless attackType is NUKE: insert ruin
   `{type:'ruin', room, x, y, structure:{id:String(_id), type, hits:0, hitsMax, user}, destroyTime:T,
   decayTime: T + (RUIN_DECAY_STRUCTURES[type] || RUIN_DECAY), user?: object.user, store: object.store || {}}`;
   if the object has an EFFECT_COLLAPSE_TIMER effect, `decayTime = max(decayTime, effect.endTime)`.
4. Remove the object (persist + delete from roomObjects).
No event here (callers push it). Ruin is not added to roomObjects.

### HELP-CLEAR-NEWBIE-WALLS (processor/intents/creeps/_clear-newbie-walls.js)
Remove every `constructedWall` with `decayTime && user` in the room (no ruins, no events).

### HELP-STRUCT-ACTIVE — `checkStructureAgainstController(object, roomObjects, controller)` (utils.js:456-507)
```
if !object.user: true
if !controller || controller.level < 1 || controller.user !== object.user: false
allowed = CONTROLLER_STRUCTURES[type][level]; if allowed === 0: false
if CONTROLLER_STRUCTURES[type][8] === 1: true            // single-instance types: ALL copies active
d = cheb(object, controller)
for o in roomObjects (key order) with same type & same user:
   if cheb(o) < d: --allowed == 0 → false
   elif !foundSelf && cheb(o) == d: o === object ? foundSelf=true : (--allowed == 0 → false)
true
```
**GOTCHA**: single-instance types (storage, observer, powerSpawn, extractor, terminal, nuker,
factory) never get deactivated by count. **PERF**: O(N) per call; called per intent — cache per
(type,user) ranks once per tick.

### HELP-CTRL-AVAIL — `checkControllerAvailability(type, roomObjects, controller)` (utils.js:338-354)
`rcl = controller.level` if owned (user/owner) else 0; count objects of `type` plus construction
sites of that structureType (includes same-tick sites); return `count < CONTROLLER_STRUCTURES[type][rcl]`
(undefined → false).

### HELP-CSITE-CHECK — `checkConstructionSite(objectsOrTerrain, type, x, y)` (utils.js:128-189)
Border rule: for types other than road/container at x∈{1,48} or y∈{1,48}, the 3 adjacent edge tiles
(x=0/49 or y=0/49 at offsets −1,0,+1) must all be terrain walls (terrain variant only). Terrain
variant: extractor always OK; non-road on wall terrain fails. Objects variant: fail if same type at
tile, any constructionSite at tile; extractor requires a mineral and no extractor; non-rampart/non-road
fails if any non-rampart/non-road `CONSTRUCTION_COST` structure at tile; fail if on edge (≤0/≥49).

### HELP-DIST-SORT (utils.js:394-403)
Comparator by Chebyshev distance to target, `da - db`. Assumes stable sort (V8 ≥ 7.0 TimSort) — ties
keep CONV-ORDER.

### HELP-CALC-REWARD (utils.js:677-698)
```
resources, densities = entries of resourceDensities (insertion order)
order = _.shuffle(range(n)); if itemsLimit: order = order[0:itemsLimit]
cur = 0
for i in 0..len(order)-2:
   r[i] = max(0, round(random() * (target - cur) / densities[order[i]])); cur += r[i]*densities[order[i]]
r[last] = max(0, round((target - cur) / densities[len(order)-1]))     // BUG: not densities[order[last]]
return {resources[order[i]]: r[i]}   (zero amounts kept as keys)
```
**GOTCHA**: the last item's amount uses the density of the resource at *index* `len(order)-1`, not of
the resource actually assigned. Randomness: lodash `shuffle` = Fisher-Yates via
`baseRandom(i, n-1) = i + floor(random()*(n-i))` (reference/lodash-3.10.1/lodash.js:2501-2503,
6928-6967), then one `Math.random()` per non-last item.

### HELP-TERMINAL-COST (utils.js:644-659)
`range = calcRoomsDistance(a, b, continuous=true)` = Chebyshev room distance with world wrap
(`min(worldSize-d, d)` per axis); `cost = ceil(amount * (1 - exp(-range/30)))`.

### HELP-REACTIONS (utils.js:665-669, 700-710)
`getReactionVariants(c)`: all `[r1,r2]` with `REACTIONS[r1][r2] == c`, in REACTIONS key order.
`calcTotalReactionsTime(m)`: recursive sum of REACTION_TIME over the reaction tree (base minerals 0).

### HELP-ATTACK-NOTIFY (utils.js:438-454)
Notification "Your {creep name|spawn name|type #id} in room R is under attack!" to target.user or
controller.user. Notifications are DB upserts with count (driver/lib/index.js:455-472) — not gameplay.

---

## 4. Room-level intents (`room` pseudo-object)

Runtime routes room intents per target room via `sanitizeUserRoomIntents` (grouped by sanitized
`roomName`, reference/common/lib/system.js:116-140); field types are coerced (`number` → parseInt,
`userString` → first 100 chars). Each intent kind is an array processed in order.

### ROOM-INTENTS (processor/intents/room/intents.js:3-53)
Per user (in user order): re-parse **all** room flag docs: `_parsed = data.split("|").map(s => s.split("~"))`;
then process in fixed order: all `removeFlag`, all `createFlag`, all `createConstructionSite`, all
`removeConstructionSite`, all `destroyStructure`, then `genEnergy`; finally for every flag doc with
`_modified`: `data = _parsed.map(j => j.join("~")).join("|")`, update by `_id` or insert
`{data, user, room}`.

### ROOM-FLAGS-MULTIUSER
**GOTCHA** (two bugs, both reachable when ≥2 users send room intents to the same room in one tick):
(a) the second user's pass re-parses every doc from the unchanged in-memory `data`, resets the first
user's `_parsed` while `_modified` stays true, and re-flushes it → the first user's flag edits are
overwritten (last `$set` wins); (b) if the first user created their *first* flag in this room, the
pushed doc has no `data` → `undefined.split` → CONV-CRASH for the whole room tick. Decide in Open
questions whether to replicate.

### ROOM-CREATE-FLAG (processor/intents/room/create-flag.js:6-34)
1. `name` escaped: `|`→`$VLINE$`, `~`→`$TILDE$`.
2. Reject if this user already has a flag with that (escaped) name **in this room's docs**.
3. Reject unless `color` and `secondaryColor` are truthy members of COLORS_ALL (1-10).
4. Reject unless 0 ≤ x,y ≤ 49.
5. Find/create the user's doc for this room, push `[name, color, secondaryColor, x, y]`, mark modified.
No FLAGS_LIMIT and no global-uniqueness check (runtime only). Flags are a separate collection
(`rooms.flags`), never in roomObjects.

### ROOM-REMOVE-FLAG (processor/intents/room/remove-flag.js:6-20)
Find user's doc in this room; escape name; if present, remove all entries with that name, mark
modified. Flag `setColor`/`setPosition` are implemented in the runtime as removeFlag+createFlag
(reference/engine/src/game/flags.js:57-91) — so they go through these two handlers (remove first).

### ROOM-CREATE-CSITE (processor/intents/room/create-construction-site.js:9-75)
1. Reject if `x <= 0 || x >= 49 || y <= 0 || y >= 49`.
2. Reject if `!CONSTRUCTION_COST[structureType]`.
3. If `roomName` matches `/^(W|E)/` (i.e. not `sim`): reject if controller owned by another user or
   reserved by another user; reject if HELP-CTRL-AVAIL fails (counts same-tick sites).
4. Reject unless HELP-CSITE-CHECK passes for objects and for terrain.
5. `progressTotal = CONSTRUCTION_COST[type]`; road: ×5 on swamp terrain (or legacy `swamp` object),
   ×150 on wall terrain (or legacy `wall` object); `config.ptr` → 1; `sim` tower → 100.
6. Insert `{structureType, x, y, type:'constructionSite', room: roomName, user, progress:0,
   progressTotal, name?(spawn only)}` and add to roomObjects as `_createdConstructionSite<n>`
   (module-global counter, never reset).
**GOTCHA**: no MAX_CONSTRUCTION_SITES check and no spawn-name uniqueness check here (runtime only).

### ROOM-REMOVE-CSITE (processor/intents/room/remove-construction-site.js:6-20)
Object must be a constructionSite; requester must be its owner or the room controller owner.
`bulk.remove`; if `progress > 1` drop `floor(progress/2)` energy (HELP-CREATE-ENERGY).
**GOTCHA**: the site is **not** deleted from roomObjects → two removals in one tick (e.g. two
`remove()` calls, or owner + controller owner) refund twice (merged into one pile); a later `build`
the same tick still sees it.

### ROOM-DESTROY-STRUCTURE (processor/intents/room/destroy-structure.js:6-26)
1. Object exists and `CONSTRUCTION_COST[type]` (controllers, portals, banks, cores excluded).
2. Room controller owned by requester (structure owner **not** checked — any structure in your room).
3. Reject unowned decaying walls (`constructedWall && decayTime && !user`).
4. Reject if any creep/powerCreep in the room has `user != requester` (includes NPCs "2"/"3").
5. HELP-DESTROY (ruin created, store kept in ruin); if owned decaying wall → HELP-CLEAR-NEWBIE-WALLS.
**GOTCHA**: no EVENT_OBJECT_DESTROYED is emitted.

### ROOM-GEN-ENERGY (processor/intents/room/gen-energy.js:6-28)
Only for user "3". Pick `x,y = floor(random()*48)+1` until not an obstacle object and not wall
terrain (2 randoms per attempt); insert `{x,y,type:'energy',energy:300,room}` (no `resourceType`).
Not in `system.js` intentTypes → unreachable from player code; keep for mods/parity only.

### FLAG-OBJ-DEAD
`processor/intents/flags/intents.js` (remove/setColor/setPosition with color *names*) is never
required by processor.js. Do not implement.

---

## 5. Construction sites

### CSITE-TICK (processor/intents/construction-sites/tick.js)
No-op. Sites never decay.

### CSITE-COMPLETE (processor/intents/creeps/build.js:103-293)
When a build intent makes `progress >= progressTotal` (build rules are creep-spec): remove the site,
insert the new structure, add it to roomObjects as `createdStructure<n>` (no `_id`, module counter),
delete the site from roomObjects. Every new structure gets `{type, x, y, room, notifyWhenAttacked:true}` plus:

| type | user | hits / hitsMax | store / capacity | other |
|---|---|---|---|---|
| spawn | site.user | SPAWN_HITS | store{energy:0}, cap{energy:SPAWN_ENERGY_CAPACITY} | `name` from site |
| extension | site.user | EXTENSION_HITS | store{energy:0}, cap{energy:0} | capacity fixed next tick (EXT-TICK) |
| link | site.user | LINK_HITS / LINK_HITS_MAX | store{energy:0}, cap{energy:LINK_CAPACITY} | cooldown 0 |
| storage | site.user | STORAGE_HITS | store{energy:0}, storeCapacity STORAGE_CAPACITY | |
| rampart | site.user | RAMPART_HITS / (builder owns controller ? RAMPART_HITS_MAX[level]\|\|0 : 0) | — | nextDecayTime T+RAMPART_DECAY_TIME |
| road | — | ROAD_HITS ×5 swamp ×150 wall (both=hitsMax) | — | nextDecayTime T+ROAD_DECAY_TIME |
| constructedWall | — | WALL_HITS / WALL_HITS_MAX | — | hitsMax corrected by WALL-HITSMAX |
| tower | site.user | TOWER_HITS | store{energy:0}, cap{energy:TOWER_CAPACITY} | no actionLog until next tick |
| observer | site.user | OBSERVER_HITS | — | |
| extractor | site.user | EXTRACTOR_HITS | — | |
| lab | site.user | LAB_HITS | store{energy:0}, storeCapacity LAB_ENERGY+LAB_MINERAL, cap{energy:LAB_ENERGY_CAPACITY} | mineralAmount 0, cooldown 0 (legacy fields) |
| powerSpawn | site.user | POWER_SPAWN_HITS | store{energy:0}, cap{energy:POWER_SPAWN_ENERGY_CAPACITY, power:POWER_SPAWN_POWER_CAPACITY} | |
| terminal | site.user | TERMINAL_HITS | store{energy:0}, storeCapacity TERMINAL_CAPACITY | |
| container | — | CONTAINER_HITS | store{energy:0}, storeCapacity CONTAINER_CAPACITY | nextDecayTime T+CONTAINER_DECAY_TIME (unowned rate even in owned room) |
| nuker | site.user | NUKER_HITS | store{energy:0}, cap{energy:NUKER_ENERGY_CAPACITY, G:NUKER_GHODIUM_CAPACITY} (ptr: 1/1) | **cooldownTime T+NUKER_COOLDOWN** (ptr 100) |
| factory | site.user | FACTORY_HITS | store{energy:0}, storeCapacity FACTORY_CAPACITY | cooldown 0 (legacy) |

**GOTCHA**: a fresh nuker starts with a full 100 000-tick cooldown. Same-tick ticks of the new
structure only mutate memory (no `_id`), so e.g. a fresh extension persists `energyCapacity 0` for
one tick.

### CSITE-STOMP (processor/intents/movement.js:223-233)
A creep that moves onto a tile containing another user's construction site removes it (refund
`floor(progress/2)` energy if `progress > 1`) unless the room is in safe mode owned by someone else
than the mover. Creep-spec owns details.

---

## 6. Controller

Controller tick (processor/intents/controllers/tick.js:6-85) runs in this order:

### CTRL-RESERVATION-EXPIRE (:10-12)
If `reservation && (T >= reservation.endTime-1 || user)` → persist `reservation: null`.

Then return if `!user || user == "2"` (**invader-owned stronghold controllers never downgrade,
never get restore/safe mode processing**).

### CTRL-OWNER-ROOMS (:18)
`addRoomToUser(room, users[user])`: `$addToSet users.rooms` if missing (driver/lib/index.js:542-546).

### CTRL-UPGRADE-BLOCKED (:20-23)
If `_upgradeBlocked` (set by creep attackController or core attackController this tick) → persist
`upgradeBlocked = _upgradeBlocked`.

### CTRL-SAFEMODE
Intent (processor/intents/controllers/activateSafeMode.js:6-25) — reject unless: `user && level`;
`safeModeAvailable > 0`; NOT `safeModeCooldown >= T`; NOT `upgradeBlocked > T`; NOT
`downgradeTime < T + CONTROLLER_DOWNGRADE[level]/2 - CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD`.
Sets `_safeModeActivated = 1`.
Tick (:25-31): if `_safeModeActivated && !(upgradeBlocked > T)` (re-checked after same-tick attacks)
persist `safeModeAvailable-1`, `safeMode = T + SAFE_MODE_DURATION`,
`safeModeCooldown = roomInfo.novice > Date.now() ? null : T + SAFE_MODE_COOLDOWN`.
Safe mode is active while `safeMode > T`.
**GOTCHA**: processor rejects at `safeModeCooldown == T` while the runtime getter already returns
undefined (runtime OK, processor no-op). The processor does not check "another controller of the user
already in safe mode" nor "this room already in safe mode" (runtime ERR_BUSY / cooldown cover it,
except in novice rooms where cooldown is null).

### CTRL-DOWNGRADE-INIT (:33-36)
If `!downgradeTime || tutorial` → persist `downgradeTime = T + CONTROLLER_DOWNGRADE[level] + 1`; return.
(After a claim sets `downgradeTime: null`, this fires the same tick.)

### CTRL-UPGRADE-RESTORE (:38-43)
If `_upgraded` (truthy; set by creep upgrade) and `!(upgradeBlocked > T)` → persist
`downgradeTime = min(downgradeTime + CONTROLLER_DOWNGRADE_RESTORE + 1, T + CONTROLLER_DOWNGRADE[level] + 1)`; return.
Note `level`/`downgradeTime` may already reflect a same-tick level-up (CTRL-LEVELUP-XREF).

### CTRL-DOWNGRADE (:45-82)
- Notification if `T == downgradeTime - 3000` (exact equality; missed if dormant or shifted).
- `while T >= downgradeTime-1 && level > 0`: `level--`; notification; if `level == 0`:
  `removeRoomFromUser`, `progress=0, user=null, downgradeTime=null, upgradeBlocked=null, safeMode=null,
  safeModeAvailable=0, safeModeCooldown = novice ? null : T+SAFE_MODE_COOLDOWN, isPowerEnabled=false`;
  else `downgradeTime += CONTROLLER_DOWNGRADE[level]/2 + 1`,
  `progress += round(CONTROLLER_LEVELS[level] * 0.9)`, `safeModeAvailable = 0`, safeModeCooldown as
  above. Persist all these fields each iteration (can cascade several levels in one tick).
**GOTCHA**: progress after downgrade is old progress + 90% of the new level's requirement (can exceed
the requirement). Reservation, sign, effects are not cleared on loss.

### CTRL-UNCLAIM (processor/intents/controllers/unclaim.js:6-28)
Requires `type == controller`, `user && level`. `removeRoomFromUser`; persist `user:null, level:0,
progress:0, downgradeTime:null, safeMode:null, safeModeAvailable:0, safeModeCooldown: novice ? null :
T+SAFE_MODE_COOLDOWN, isPowerEnabled:false` (upgradeBlocked, reservation, sign, effects kept). Because
of CONV-BULK merge, all later handlers this tick see the room as unowned (capacities drop immediately).

### CTRL-LEVELUP-XREF (processor/intents/creeps/upgradeController.js:59-103)
Level-up requires `progress + effect >= CONTROLLER_LEVELS[level]` **and**
`downgradeTime + CONTROLLER_DOWNGRADE_RESTORE >= T + CONTROLLER_DOWNGRADE[level]`; on level-up
`downgradeTime = T + CONTROLLER_DOWNGRADE[newLevel]/2`, `safeModeAvailable += 1`, progress carries
over (0 at RCL8). `_upgraded` accumulates energy spent (RCL8 cap CONTROLLER_MAX_UPGRADE_PER_TICK, +
PWR_OPERATE_CONTROLLER with `endTime >= T`). Full rules in the creep spec.

---

## 7. Spawns and extensions

### SPAWN-INTENT-ORDER (processor/intents/spawns/intents.js:1-17)
createCreep, renewCreep, recycleCreep, setSpawnDirections, cancelSpawning — all may run.
**GOTCHA**: createCreep sets `spawning` in memory, so a same-tick renewCreep is rejected (runtime
returned OK).

### SPAWN-CREATE (processor/intents/spawns/create-creep.js:7-101)
Intent fields (sanitized): `name` (≤100 chars), `body` (only valid parts kept), `energyStructures`
(string[] or undefined), `directions` (number[] or undefined).
1. Reject if `spawning`; if type ≠ spawn; if !HELP-STRUCT-ACTIVE.
2. `directions` defined: must be array; `uniq(map(+d))`; if non-empty every d integer 1..8.
3. `body = body.slice(0, MAX_CREEP_SIZE)`; `cost = calcCreepCost(body)`; SPAWN-CHARGE or reject.
4. stats `energyCreeps += cost`, `creepsProduced += body.length`.
5. `needTime = CREEP_SPAWN_TIME * body.length`; PWR_OPERATE_SPAWN active → `ceil(needTime * effect[level-1])`.
6. Persist `spawning = {name, needTime, spawnTime: T + needTime, directions}`.
7. Insert creep `{name, x: spawn.x, y: spawn.y, body: [{type, hits:100}], store:{energy:0},
   storeCapacity: 50 per CARRY, type:'creep', room, user, hits/hitsMax = 100*parts, spawning:true,
   fatigue:0, notifyWhenAttacked:true, tutorial?}` (not added to roomObjects; `ageTime` set on first
   non-spawning tick by the creep tick).
No name-uniqueness check (runtime only). Empty `directions` array passes here (runtime rejects).

### SPAWN-CHARGE (processor/intents/spawns/_charge-energy.js:6-78)
- `energyStructures === undefined` (old): spawns and extensions of `spawn.user` with falsy `off`;
  fail if total < cost; drain spawns sorted by distance to the paying spawn (every spawn gets an update
  even if 0 taken), then extensions by distance until cost covered.
- Given list: keep ids that exist, not `off`, same user, spawn/extension; `uniq`; fail if sum < cost;
  drain in list order until covered.
`off` is last tick's value (PROC-CALC-SPAWNS).

### SPAWN-RENEW (processor/intents/spawns/renew-creep.js:6-55)
Reject if type ≠ spawn, spawn `spawning`, target not own creep / spawning, range > 1, any CLAIM part.
`effect = floor(SPAWN_RENEW_RATIO * CREEP_LIFE_TIME / CREEP_SPAWN_TIME / body.length)`; reject if
`ageTime + effect > T + CREEP_LIFE_TIME`. `cost = ceil(SPAWN_RENEW_RATIO * bodyCost / CREEP_SPAWN_TIME /
body.length)` via old charging. stats; `target.actionLog.healed = spawn pos`; `$inc ageTime effect`.
If any boost: clear all boosts (no refund), recalc body, drop overflow, persist body/storeCapacity.
**GOTCHA**: processor ignores `off` and RCL activity (runtime returns ERR_RCL_NOT_ENOUGH).

### SPAWN-RECYCLE (processor/intents/spawns/recycle-creep.js:6-23)
Target own non-spawning creep in range 1 → `_die(target, dropRate=1.0, violent=false)` (tombstone,
EVENT_OBJECT_DESTROYED `{type:'creep'}`). Spawn may be spawning; `off` not checked here.

### SPAWN-SET-DIRECTIONS (processor/intents/spawns/set-spawn-directions.js:6-21)
Requires `spawning`; non-empty array; `uniq(map(+d))`; all integers 1..8 → replace `spawning` with a
clone carrying the new `directions`.

### SPAWN-CANCEL (processor/intents/spawns/cancel-spawning.js:6-13)
Requires `spawning`; find creep `{type:'creep', name, x, y}` at the spawn, remove it, `spawning:null`.
No energy refund. **GOTCHA**: if the creep is not found → TypeError → CONV-CRASH (not reachable via
runtime).

### SPAWN-TICK-SPAWNING (processor/intents/spawns/tick.js:12-30)
If `spawning`: PWR_DISRUPT_SPAWN active → `spawning.spawnTime += 1` (merge) and nothing else; else if
`T >= spawnTime-1`: SPAWN-BORN with the creep found by name at the spawn; success → `spawning:null`,
failure → `spawning.spawnTime = T+1` (retry every tick).

### SPAWN-BORN (processor/intents/spawns/_born-creep.js:7-79)
`directions = spawning.directions || [1..8]`. For each direction in order: tile is occupied if any
obstacle object (OBSTACLE_OBJECT_TYPES) or any user's construction site of an obstacle type is there,
or `movement.isTileBusy`, or wall terrain without a road. First free → move creep there,
`spawning:false`, return true. Remember the first occupied tile with a creep of another user (NPCs
included, power creeps not). If none free and a hostile was found: if any *other* direction (not in
`directions`) is free (here wall terrain counts as occupied even with a road) → return false;
else `_die(hostile, undefined, violent=true)` and place the creep on its tile. Else false.
If the creep object is missing, the update is a no-op and it returns true (spawning cleared).

### SPAWN-TICK-REGEN (processor/intents/spawns/tick.js:32-47)
Only if controller exists, `level >= 1`, `controller.user == spawn.user`; among **all** spawns in the
room (any owner), if count > allowed for level, only the `allowed` closest to the controller
(HELP-DIST-SORT) regenerate. If `!tutorial && energyAvailable < SPAWN_ENERGY_CAPACITY &&
store.energy < SPAWN_ENERGY_CAPACITY` → `store.energy += 1`.
**GOTCHA**: `energyAvailable` is computed once (PROC-ENERGY-AVAILABLE) — with several spawns each
gains +1 per tick, so the room total can exceed 300. Spawns built this tick are included in the
count (synthetic key, `type:'spawn'`).
**PERF**: `_.filter(roomObjects, {type:'spawn'})` + sort per spawn per tick.

### EXT-TICK (processor/intents/extensions/tick.js:6-20)
If controller exists: `cap = EXTENSION_ENERGY_CAPACITY[level] || 0`; persist
`storeCapacityResource.energy = cap` when different or falsy. Owner not checked; stored energy above
capacity is not clamped.

---

## 8. Towers

### TOWER-INTENT-ORDER (processor/intents/towers/intents.js:2-10)
Exactly one of `heal` else `repair` else `attack`. **GOTCHA**: the runtime returns OK for all three
(intents are keyed by name), only the highest-priority one executes.

### TOWER-FALLOFF (towers/attack.js:32-46; heal.js:23-37; repair.js:20-34)
```
r = cheb(tower, target); amount = BASE
if r > TOWER_OPTIMAL_RANGE: r = min(r, TOWER_FALLOFF_RANGE)
   amount -= amount * TOWER_FALLOFF * (r - TOWER_OPTIMAL_RANGE) / (TOWER_FALLOFF_RANGE - TOWER_OPTIMAL_RANGE)
for p in [PWR_OPERATE_TOWER, PWR_DISRUPT_TOWER]: if active (endTime > T): amount *= POWER_INFO[p].effect[level-1]
amount = floor(amount); if amount == 0: reject
```
BASE = TOWER_POWER_ATTACK / TOWER_POWER_HEAL / TOWER_POWER_REPAIR. No range limit, no
HELP-STRUCT-ACTIVE check in the processor (runtime only). Floating-point order matters: subtract,
then multiply, then floor.

### TOWER-ATTACK (processor/intents/towers/attack.js:6-70)
Reject if not tower / no store; target missing or self; spawning creep; `!target.hits`;
`energy < TOWER_ENERGY_COST`. If any rampart on the target tile, **target = that rampart** (any
owner, public or not, even the tower owner's own). Compute falloff vs the (possibly redirected)
target; HELP-DAMAGE with EVENT_ATTACK_TYPE_RANGED; energy −= TOWER_ENERGY_COST; actionLog.attack;
target actionLog.attacked; extra notifyWhenAttacked notification (duplicate of HELP-DAMAGE's).
**GOTCHA**: energy is spent even when the wall/rampart is immune (fortify/invulnerability) — no event.

### TOWER-HEAL (processor/intents/towers/heal.js:6-53)
Target creep or powerCreep, not spawning, energy ≥ cost (no hits<hitsMax check — heals full-HP
targets). `target._healToApply += amount` (applied in the target's tick); energy −= cost;
actionLogs; EVENT_HEAL `{targetId, amount, healType: EVENT_HEAL_TYPE_RANGED}`.

### TOWER-REPAIR (processor/intents/towers/repair.js:6-56)
Target with `CONSTRUCTION_COST[type]` and `hits < hitsMax` (any owner); energy ≥ cost.
`hits = min(hits + amount, hitsMax)`; energy −= cost; stats `energyConstruction += TOWER_ENERGY_COST`;
EVENT_REPAIR `{targetId, amount, energySpent: TOWER_ENERGY_COST}` (**amount is the unclamped value**).

### TOWER-TICK (processor/intents/towers/tick.js:7-17)
Persist actionLog if changed.

---

## 9. Links

### LINK-TRANSFER (processor/intents/links/transfer.js:6-54)
Validation order: object is link; `store` exists and `store.energy >= amount` and `amount >= 0`;
target exists and is a link; `cooldown > 0` → reject; HELP-STRUCT-ACTIVE; target
`storeCapacityResource.energy` truthy and target not full. Then:
```
amount = min(amount, targetCap - target.energy)
target.energy += amount; source.energy -= amount
target.energy -= ceil(amount * LINK_LOSS_RATIO)
source.cooldown += LINK_COOLDOWN * cheb(source, target)
```
actionLog.transferEnergy; EVENT_TRANSFER `{targetId, resourceType:'energy', amount}` (pre-loss
amount). Target ownership/range not checked (same room implied).
**GOTCHA**: `amount == 0` passes (no energy moves, cooldown still added). A link that received energy
earlier this tick can forward it in the same tick (in-memory). Cooldown uses `+=`.

### LINK-TICK (processor/intents/links/tick.js:7-30)
If `cooldown > 0`: `cooldown -= 1` (floor 0) and persist cooldown+actionLog; else persist actionLog if
changed. Since the transfer intent runs before the tick, a transfer over distance d leaves
`cooldown = d-1` visible next tick (distance 1 → can fire every tick).

---

## 10. Labs

### LAB-INTENT-ORDER (processor/intents/labs/intents.js:2-14)
boostCreep, then unboostCreep, then runReaction **else** reverseReaction.

### LAB-STORE-MODEL
Lab with a mineral: `storeCapacityResource = {energy: LAB_ENERGY_CAPACITY, [mineral]: LAB_MINERAL_CAPACITY}`,
`storeCapacity: null`. Without mineral: `storeCapacityResource = {energy}`, `storeCapacity =
LAB_ENERGY_CAPACITY + LAB_MINERAL_CAPACITY`. Every handler that empties or introduces a mineral
switches the model (run-reaction.js:53-86, reverse-reaction.js:52-88, boost-creep.js:48-56).
"mineralType" everywhere = first key of `store` (key order) ≠ energy with truthy amount.

### LAB-RUN (processor/intents/labs/run-reaction.js:6-89)
1. Reject if `cooldownTime > T`.
2. `amount = LAB_REACTION_AMOUNT (+ PWR_OPERATE_LAB effect)`.
3. lab1: exists, is lab, has mineral with `>= amount`, `cheb <= 2`; lab2 same.
4. Reject if `(self[selfMineral]||0) + amount > LAB_MINERAL_CAPACITY`.
5. `product = REACTIONS[m1][m2]`; reject if none or self holds a different mineral.
6. self `store[product] += amount`, `cooldownTime = T + REACTION_TIME[product]` (+ model switch);
   lab1/lab2 `-= amount` (+ model switch when emptied); actionLog.runReaction `{x1,y1,x2,y2}`.
No ownership check of lab1/lab2 and no HELP-STRUCT-ACTIVE in processor. No event.

### LAB-REVERSE (processor/intents/labs/reverse-reaction.js:6-91)
1. Reject if not lab or `cooldownTime > T`; reject if `lab1 == lab2` (ids).
2. amount as above; self mineral amount `>= amount`.
3. lab1 (null-safe) must be a lab; if it holds a mineral, `held + amount <= storeCapacityResource[held]`;
   lab2 same.
4. `variant` = first of HELP-REACTIONS variants where lab1 empty or holds v[0] and lab2 empty or holds v[1].
5. self `-= amount`, `cooldownTime = T + REACTION_TIME[selfMineral]`; lab1 `+= amount` of v[0],
   lab2 `+= amount` of v[1] (model switches); actionLog.reverseReaction.
**GOTCHA**: **no range check** in the processor (runtime: range ≤ 2).

### LAB-BOOST (processor/intents/labs/boost-creep.js:6-62)
1. Lab has store and a mineral; mineral `>= LAB_BOOST_MINERAL` and energy `>= LAB_BOOST_ENERGY`.
2. Target creep, not spawning, `cheb <= 1` (any owner; no cooldown and no RCL check here).
3. `parts` = unboosted parts with `BOOSTS[part.type][mineral]`; none → reject.
4. If `parts[0].type != TOUGH` reverse (non-TOUGH boosts apply from the body end; TOUGH from the
   front); if `bodyPartsCount` truthy, take the first `bodyPartsCount`.
5. While resources suffice and parts remain: boost next part, mineral −30, energy −20.
6. Persist lab store (model switch if mineral exhausted); `_recalc-body`; persist body/storeCapacity;
   `target.actionLog.healed = lab pos` (**boost shows as "healed"**). No event.

### LAB-UNBOOST (processor/intents/labs/unboost-creep.js:6-53)
Reject if `cooldownTime > T`; target own creep; HELP-STRUCT-ACTIVE; range 1; must have boosted parts.
Clear all boosts, recalc, persist. For each resource in RESOURCES_ALL order with `n` boosted parts:
drop `n*LAB_UNBOOST_ENERGY` energy (0 → nothing) and `n*LAB_UNBOOST_MINERAL` of the compound at the
**creep's** tile (HELP-CREATE-ENERGY, container first); `cooldown += n * calcTotalReactionsTime(r) *
LAB_UNBOOST_MINERAL / LAB_REACTION_AMOUNT`. Persist `cooldownTime = T + cooldown` if > 0.

### LAB-TICK (processor/intents/labs/tick.js:7-13)
Persist actionLog if changed. No capacity/RCL updates for labs.

---

## 11. Other owned structures

### PSPAWN-PROCESS (processor/intents/power-spawns/process-power.js:7-39)
Requires powerSpawn with store; HELP-STRUCT-ACTIVE. `amount = 1`; if PWR_OPERATE_POWER active
(`endTime >= T`): `amount = min(store.power, 1 + effect[level-1])`. Reject if `power < amount` or
`energy < amount * POWER_SPAWN_ENERGY_RATIO`. Subtract; stats `powerProcessed`; `bulkUsers.inc(user,
'power', amount)` (global user doc). No event.
**GOTCHA**: with the effect, partial batches are processed (runtime requires the full amount).

### FACTORY-PRODUCE (processor/intents/factories/produce.js:6-44)
Reject unless: store exists; `COMMODITIES[r]` exists; commodity `level` falsy or equal to
`factory.level`; not `cooldownTime > T`; HELP-STRUCT-ACTIVE; if commodity has a level and
`factory.level > 0`, an active PWR_OPERATE_FACTORY effect (`endTime >= T`) with matching `level`;
every component available; `sum(store) - sum(components) + (amount||1) <= storeCapacity`.
Subtract components (keys kept at 0), add `amount||1` product; actionLog.produce `{x,y,resourceType}`;
`cooldownTime = T + COMMODITIES[r].cooldown`. No event/stats.

### FACTORY-TICK (processor/intents/factories/tick.js:6-19)
If controller exists: `storeCapacity = (level>0 && controller.user==user && CONTROLLER_STRUCTURES.factory[level]) ? FACTORY_CAPACITY : 0`, persist if different. Persist actionLog if changed.

### TERMINAL-SEND (processor/intents/terminal/send.js:6-33)
Reject unless `targetRoomName` matches `/^(W|E)\d+(N|S)\d+$/`, resource in RESOURCES_ALL, `amount`
truthy and `store[r] >= amount`; compute HELP-TERMINAL-COST (OPERATE_TERMINAL `endTime >= T` →
`ceil(cost*effect)`); reject if energy insufficient (`energy < cost`, or for energy `energy <
amount + cost`). Persist `send = intent` (no transfer yet). **No cooldown check** here.

### TERMINAL-SEND-EXEC (processor/global-intents/market.js:71-97, executeTransfer :15-69)
Global phase after all rooms (engine main loop stage `global`, reference/engine/src/main.js:62-66),
same `T`. For each terminal with `send` (DB order): clear `send`; reject if `cooldownTime > T`; reject
unless the target room has a terminal with a user. `cooldown = TERMINAL_COOLDOWN`, OPERATE_TERMINAL
(`endTime > T`) → `round(cooldown*effect)`. executeTransfer: re-check store; clamp amount to target
free space (`storeCapacity - sum(store)`); `!(amount > 0)` → stop (no cooldown); cost on clamped
amount (effect `endTime > T`); energy check; move resources; insert transaction (`description`
with `<` escaped). On success persist `cooldownTime = T + cooldown`.
**PERF**: cross-room; serialise after room phase (as the original does). Target terminals are
indexed by room (`_.indexBy`, last terminal per room wins).

### TERMINAL-TICK (processor/intents/terminal/tick.js:6-15) / STORAGE-TICK (processor/intents/storages/tick.js:6-19)
If controller exists: capacity = TERMINAL_CAPACITY / STORAGE_CAPACITY when `level > 0 &&
controller.user == user && CONTROLLER_STRUCTURES[type][level] (>0)` else 0; storage adds
`POWER_INFO[PWR_OPERATE_STORAGE].effect[level-1]` when active and base > 0. Persist if different.
Contents above capacity are kept.

### OBSERVER-OBSERVE (processor.js:116-118, 284-288, 403-406)
Init: `observeRoom = null`. Intent: `observeRoom = intent.roomName` (string; **no range, RCL or
format validation in processor**). Tick: persist `observeRoom` every tick (**PERF**: unconditional write).
The runner adds `observeRoom` to the owner's visible rooms on the next tick
(reference/driver/lib/runtime/data.js:120-122).

### NUKER-LAUNCH (processor/intents/nukers/launch-nuke.js:8-51)
Reject unless: HELP-STRUCT-ACTIVE; `store.G >= cap.G` and `store.energy >= cap.energy`; NOT
`cooldownTime > T`; `0 <= x,y <= 49`; launching room not novice/respawn (`> Date.now()`);
`roomName` string matching `/^(W|E)\d+(S|N)\d+$/`; per-axis room distance `<= NUKE_RANGE` (no wrap).
Persist `store {energy:0, G:0}`, `cooldownTime = T + NUKER_COOLDOWN` (ptr 100); **insert into the
target room** `{type:'nuke', room, x, y, landTime: T + NUKE_LAND_TIME (ptr 100), launchRoomName}`.
Target-room novice status is not checked here (NUKE-NOVICE removes it later).
**PERF**: cross-room write from a room worker.

### NUKE-PRETICK (processor/intents/nukes/pretick.js:6-15)
If `landTime == T+1` (the landing tick): set `createCreep = null` in every object intent present at
that point (player intents; runs before the keeper/invader/core pretick, so the invader core's own
`createCreep` added later survives; `objectsManual` is merged later and is not touched).

### NUKE-NOVICE (processor/intents/nukes/tick.js:10-14)
Each tick: if the room is novice or respawn area (`> Date.now()`), remove the nuke silently.

### NUKE-LAND (processor/intents/nukes/tick.js:16-78), when `T == landTime-1`
1. For every object in the room (key order): creep → `_die(creep, 0, violent, attackType NUKE)`
   (no tombstone, EVENT_OBJECT_DESTROYED creep, stats creepsLost); powerCreep → persist `hits: 0`;
   constructionSite / energy / tombstone / ruin → removed; spawn with spawning → `spawning:null`.
   Ramparts do not protect creeps.
2. For dx,dy in −2..2 (dx outer, dy inner): `damage = NUKE_DAMAGE[0]` at range 0 else `NUKE_DAMAGE[2]`;
   objects = all roomObjects at the tile; if a rampart exists: HELP-DAMAGE(nuke, rampart, damage,
   NUKE) then `damage -= rampartHitsBefore`, rampart excluded; if `damage > 0` HELP-DAMAGE every
   other object on the tile with the remaining damage (hit-less objects skipped; NUKE ignores
   fortify/invulnerability; destroyed structures leave **no ruin**; power banks → crash, see
   HELP-DAMAGE).
3. Controller: if `safeMode > T` → `safeMode = T, safeModeCooldown = null`. Then
   **GOTCHA** (operator precedence): `(user && !invulnerable && !upgradeBlocked) || upgradeBlocked < T`
   → `upgradeBlocked = T + CONTROLLER_NUKE_BLOCKED_UPGRADE`. JS: `null < T` is true, `undefined < T`
   false — an unowned controller with `upgradeBlocked: null` (after downgrade to 0) is blocked too.
Effects of multiple nukes stack in key order.

### NUKE-REMOVE (processor/intents/nukes/tick.js:80-83)
Remove when `T >= landTime` (nuke visible for one tick after landing).

### RAMPART-SET-PUBLIC (processor/intents/ramparts/set-public.js:6-11)
Persist `isPublic = !!intent.isPublic` (only the generic owner gate).

### RAMPART-SHIELD (processor/intents/ramparts/tick.js:10-17)
If the rampart has any PWR_SHIELD effect: remove it when `endTime <= T`; in all cases return (no
decay, no hitsMax update for shield ramparts).

### RAMPART-HITSMAX (processor/intents/ramparts/tick.js:19-24)
If controller exists and rampart user ≠ "2": `hitsMax = (user == controller.user) ?
RAMPART_HITS_MAX[level] || 0 : 0`; persist if different. Hits are not clamped.

### RAMPART-DECAY (processor/intents/ramparts/tick.js:26-40)
If `!nextDecayTime || T >= nextDecayTime-1`: `hits = (hits||0) - RAMPART_DECAY_AMOUNT`; `<= 0` → remove
(no ruin/event); else persist hits and `nextDecayTime = T + RAMPART_DECAY_TIME`.

### ROAD-DECAY (processor/intents/roads/tick.js:6-35)
If `!nextDecayTime || T >= nextDecayTime-1`: `decay = ROAD_DECAY_AMOUNT`, ×CONSTRUCTION_COST_ROAD_SWAMP_RATIO
on swamp (terrain or legacy `swamp` object), ×CONSTRUCTION_COST_ROAD_WALL_RATIO on wall (terrain or
legacy `wall` object); `hits -= decay`; `<= 0` → remove (no ruin/event); else persist hits and
`nextDecayTime = T + ROAD_DECAY_TIME`.
**PERF**: two full-room scans per decaying road for legacy swamp/wall objects; precompute.

### ROAD-WEAR (processor/intents/movement.js:211-222)
When a creep/power creep completes a move onto a road tile (during its tick): `nextDecayTime -=
ROAD_WEAROUT * body.length` (all parts, dead included) or `ROAD_WEAROUT_POWER_CREEP`. Interacts with
CONV-ORDER (may affect the road's own tick this tick or next).

### WALL-HITSMAX (processor/intents/constructedWalls/tick.js:10-15)
If controller exists and not `newbieWall`: `hitsMax = CONTROLLER_STRUCTURES.constructedWall[level] ?
WALL_HITS_MAX : 0`, persist if different. **GOTCHA**: level 0/1 → hitsMax 0 (walls in unowned
controller rooms cannot be repaired); rooms without controller keep the built value.

### WALL-NEWBIE (processor/intents/constructedWalls/tick.js:17-44)
- `ticksToLive > 0` → persist `decayTime = T + ticksToLive`, `ticksToLive = null` (merged in memory).
- Numeric `decayTime`: remove when `T >= decayTime-1` **or** controller exists and is unowned; notify
  owner at `T == decayTime-5000` if the wall has a user.
- Object `decayTime`: remove when `Date.now() > decayTime.timestamp`.
These walls come from official-server room protection; the reference backend never creates them.

### CONTAINER-DECAY (processor/intents/containers/tick.js:6-35)
If `!nextDecayTime || T >= nextDecayTime-1`: `hits = (hits||0) - CONTAINER_DECAY`; `<= 0` → drop every
positive store amount on the tile (HELP-CREATE-ENERGY skips this container since its in-memory hits
≤ 0) then remove (no ruin, no event); else `nextDecayTime = T + (controller && controller.level > 0 ?
CONTAINER_DECAY_TIME_OWNED : CONTAINER_DECAY_TIME)`, persist. Owner of the controller is irrelevant.

### EXTRACTOR-TICK (processor/intents/extractors/tick.js:7-27)
If `cooldown > 0`: decrement (floor 0), persist. Then if `_cooldown` (set by a mineral harvest this
tick, EXTRACTOR_COOLDOWN) persist `cooldown = _cooldown`. Net: harvest at T blocks harvests at
T+1..T+5 (`cooldown` 5,4,3,2,1 seen by players), allowed again at T+6. Multiple creeps can harvest the
same tick (harvest only reads `cooldown`, creeps-spec harvest.js:84).

---

## 12. Natural / NPC objects

### SOURCE-REGEN + SOURCE-PWR (processor/intents/sources/tick.js:10-42)
Only when `energy < energyCapacity`:
1. If `!nextRegenerationTime` → `nextRegenerationTime = T + ENERGY_REGEN_TIME` (persist).
2. PWR_DISRUPT_SOURCE active → `nextRegenerationTime += 1` (merged in memory too).
3. If `T >= nextRegenerationTime-1` → `energy = energyCapacity`, `nextRegenerationTime = null`.
4. PWR_REGEN_SOURCE active and `(endTime - T - 1) % period == 0` →
   `energy = min(capacity, energy + effect[level-1])` (reads values updated in steps 1-3).
Regeneration happens ENERGY_REGEN_TIME ticks after the first tick energy was below capacity (the
first harvest tick).

### SOURCE-CAPACITY (processor/intents/sources/tick.js:46-59)
After regen: controller exists → unowned and unreserved: capacity SOURCE_ENERGY_NEUTRAL_CAPACITY and
`energy = min(energy, it)` (only written when capacity differs); owned or reserved (any user):
SOURCE_ENERGY_CAPACITY (energy unchanged). No controller: SOURCE_ENERGY_KEEPER_CAPACITY.
**GOTCHA**: when a capacity drop clamps energy to full while a regen timer is pending, the stale
`nextRegenerationTime` survives and is reused the next time energy drops (early refill).

### MINERAL-REGEN + MINERAL-DENSITY (processor/intents/minerals/tick.js:8-37)
When `!mineralAmount`: if `!nextRegenerationTime` → `T + MINERAL_REGEN_TIME`. If
`T >= nextRegenerationTime-1`: `update = {nextRegenerationTime:null, mineralAmount:
MINERAL_DENSITY[density]}` (**old density**); then if `density == DENSITY_LOW || density ==
DENSITY_ULTRA || random() < MINERAL_DENSITY_CHANGE` (random consumed only for moderate/high):
repeat { `r = random()`; pick the first density key ("1".."4" ascending) with
`r <= MINERAL_DENSITY_PROBABILITY[key]` } while equal to old density; `update.density = new`.
**GOTCHA**: the refilled amount uses the old density; the new density applies to the next cycle.

### MINERAL-PWR (processor/intents/minerals/tick.js:39-47)
PWR_REGEN_MINERAL active (`endTime > T`), no pending regen and amount > 0, and
`(endTime - T - 1) % period == 0` → `mineralAmount += effect[level-1]` (no cap).

### DEPOSIT-COOLDOWN / DEPOSIT-DECAY (processor/intents/deposits/tick.js:6-17; processor.js:421-426)
`_cooldown` (set by harvest when `ceil(DEPOSIT_EXHAUST_MULTIPLY * harvested^DEPOSIT_EXHAUST_POW) > 1`,
using `harvested` after this harvest's `$inc`) → persist `cooldownTime = T + _cooldown`. Every harvest
also persists `decayTime = T + DEPOSIT_DECAY_TIME` (creeps-spec harvest.js:114-139). Removal: generic
`T >= decayTime-1` (deposit's own `T > decayTime` check never wins). Multiple creeps harvest in the same
tick (cooldownTime only updated in the tick); the last harvester's `_cooldown` wins. Deposit harvest
emits **no** EVENT_HARVEST.

### POWERBANK-DECAY / POWERBANK-HITBACK / POWERBANK-RUIN
No tick handler. Removed at `T >= decayTime-1` (processor.js:421-426). Attacks (any `_damage` caller)
cause hit-back `damage * POWER_BANK_HIT_BACK` to the attacker (zeroed only if attacker owns the
safe-moded controller — highways have none). Destroyed → ruin with `store {power}` and
`decayTime = T + RUIN_DECAY_STRUCTURES.powerBank` (10).

### PORTAL-TICK (processor/intents/portals/tick.js:6-25)
If `unstableDate && Date.now() > unstableDate` → `decayTime = T + PORTAL_DECAY`, `unstableDate = null`.
If `decayTime && T > decayTime` → remove portal and the constructedWall at **(x+1, y+1)** if any.

### KEEPERLAIR-TICK (processor/intents/keeper-lairs/tick.js:6-72)
Keeper = creep with `user "3"` and `name == 'Keeper' + lair._id`.
1. If `!nextSpawnTime` and (no keeper or `keeper.hits < 5000`) → `nextSpawnTime = T + ENERGY_REGEN_TIME`.
2. If `nextSpawnTime && T >= nextSpawnTime-1`: remove existing keeper (`bulk.remove` only — no
   tombstone, no event, still in roomObjects this tick); insert keeper at the lair tile:
   body `17×TOUGH, 13×MOVE, 10×[ATTACK, RANGED_ATTACK]` (hits 100 each), store{energy:0},
   storeCapacity 0, hits/hitsMax 5000, user "3", spawning false, fatigue 0 (no ageTime: creep tick
   sets `T + CREEP_LIFE_TIME` on its first tick); `nextSpawnTime = null`.
**GOTCHA**: once the timer starts it always replaces the keeper (even if healed back).

### ENERGY-DECAY (processor/intents/energy/tick.js:6-24)
`rt = resourceType || 'energy'`; `obj[rt] -= ceil(obj[rt] / ENERGY_DECAY)`; `<= 0` or falsy/NaN →
remove; else persist. Applies in the creation tick for piles created in the intent phase (CONV-ORDER).

### TOMBSTONE-DECAY / RUIN-DECAY (processor/intents/tombstones/tick.js:6-20; ruins/tick.js:6-19)
If `!decayTime || T >= decayTime-1`: HELP-CREATE-ENERGY for every store entry (container first, then
pile), remove. Tombstone decayTime = `T + body.length * TOMBSTONE_DECAY_PER_PART` or `tombstoneDecay`
(creeps-spec _die.js:15-18).

---

## 13. Invader cores and strongholds

Core object fields: `level` (0 = expansion core, 1-5 = stronghold), `templateName`,
`strongholdBehavior`, `strongholdId`, `depositType`, `deployTime`, `decayTime`, `nextExpandTime`,
`population`, `spawning`, `effects`, `user "2"`.

### CORE-PRETICK (processor/intents/invader-core/pretick.js:7-60)
Runs only if the room has a core with `user` (the last core found in init loop wins, processor.js:73).
behavior = `deployTime ? 'deploy' : strongholdBehavior || 'default'`. Context lists (single pass):
non-spawning creeps/powerCreeps → `defenders` (same user, incl. non-stronghold invaders) /
`damagedDefenders` / `hostiles` (user ≠ "3"); `towers`, `ramparts` with core's user; `damagedRoads`
(roads of this stronghold with hits < hitsMax). Intents produced via `intents.set(id, name, data)`
and merged into user "2" intents with existing intents taking precedence (PROC-PIPELINE step 3).

### CORE-BEHAVIORS (stronghold.js:388-491)
- deploy: HANDLE-CTRL; DEPLOY.
- default (level-0 cores): HANDLE-CTRL; REFILL-TOWERS; FOCUS-CLOSEST.
- bunker1: HANDLE-CTRL; REFILL-TOWERS || REFILL-CREEPS; FOCUS-CLOSEST.
- bunker2: HANDLE-CTRL; REFILL-TOWERS || REFILL-CREEPS; init population `[weakDefender/simple-melee]`;
  POPULATION; FOCUS-CLOSEST.
- bunker3: HANDLE-CTRL; REFILL-TOWERS; population `2×[fullDefender/simple-melee]`; POPULATION; FOCUS-CLOSEST.
- bunker4: HANDLE-CTRL; REFILL-TOWERS || REFILL-CREEPS; population = `shuffle([fortifier, 4×boostedDefender/coordinated, 3×boostedRanger/coordinated]).take(4)`;
  spots = ASSIGN-DEFENDERS; POPULATION; TOWERS-MAINT; FOCUS-MAX.
- bunker5: same as 4 but population `[fortifier, ...shuffle(shuffle([fortifier, 7×fullBoostedMelee, 9×fullBoostedRanger])).take(8)]`
  (double shuffle consumes randoms twice); ANTINUKE before ASSIGN-DEFENDERS.
Population is persisted once (`if(!core.population)`), via bulk merge.

### CORE-HANDLE-CTRL (stronghold.js:135-149)
If controller: owned by core user → if `downgradeTime - T < INVADER_CORE_CONTROLLER_DOWNGRADE - 25`
issue `upgradeController`; else if no reservation or reserved by core user → `reserveController`;
else → `attackController`.

### CORE-DEPLOY (stronghold.js:22-133), when `deployTime <= T+1`
1. `duration = round(STRONGHOLD_DECAY_TICKS * (0.9 + random()*0.2))`, `decayTime = T + duration`.
2. Push collapse effect `{effect: EFFECT_COLLAPSE_TIMER, power: EFFECT_COLLAPSE_TIMER, endTime:
   decayTime, duration}` to core; persist `deployTime:null, decayTime, hits/hitsMax INVADER_CORE_HITS, effects`.
3. Remove all `ramparts` (core-user ramparts = placeholders).
4. For each template structure (common/lib/strongholds.js templates) at `core + (dx,dy)`: for every
   non-stronghold object on that tile: creep/powerCreep → `_die(o, undefined, violent=true)`;
   constructionSite → remove, refund `floor(progress/2)` if `progress > 1`; any `CONSTRUCTION_COST`
   structure → HELP-DESTROY (ruin). Then (except the core entry) insert
   `{...templateEntry minus dx/dy, x, y, room, strongholdId, decayTime, effects:[collapse], ...typeOptions}`,
   towers/ramparts get `user`; add to roomObjects as `deployedStructure<n>`.
   typeOptions: rampart `{hits: STRONGHOLD_RAMPART_HITS[template.rewardLevel], hitsMax:
   RAMPART_HITS_MAX[8], hitsTarget: STRONGHOLD_RAMPART_HITS[core.level], nextDecayTime: decayTime}`;
   tower `{hits/hitsMax TOWER_HITS, store{energy:TOWER_CAPACITY}, cap{energy:TOWER_CAPACITY}, actionLog nulls}`;
   container `{notifyWhenAttacked:false, hits/hitsMax CONTAINER_HITS, nextDecayTime: decayTime,
   storeCapacity:0, store: calcReward(containerRewards, containerAmounts[rewardLevel], 3)}`;
   road `{notifyWhenAttacked:false, hits/hitsMax ROAD_HITS, nextDecayTime: decayTime}`.
**GOTCHA** (crash): `_die` on a power creep dereferences `body.length` → CONV-CRASH every tick
while a power creep stands on the footprint at deploy time.

### CORE-REFILL-TOWERS (stronghold.js:151-167)
`if towerRefillChance[level] < random()` → false (`[0,0.01,0.1,0.3,1,1]`; always consumes one random).
Undercharged = towers with `energy <= 2*TOWER_ENERGY_COST` standing on a core-user rampart; pick
`_.min` by energy (first minimum) → core `transfer {id, amount: cap - energy}`.

### CORE-REFILL-CREEPS (stronghold.js:169-182)
Defenders with `storeCapacity > 0 && 2*energy <= storeCapacity` → `_.min` energy → `transfer`.

### CORE-TOWERS-MAINT (stronghold.js:184-206)
If towers and (damaged defenders or damaged roads): first damaged defender on a rampart → first tower
`heal`, tower removed from list; else if any damaged road on a rampart → first tower `repair`
**`_.first(damagedRoads)`** (not the filtered list — may target an unprotected road).

### CORE-FOCUS-CLOSEST (stronghold.js:208-238)
Target = `_.min(hostiles, cheb to core)`. All towers `attack`; defenders with ATTACK at range 1 →
`attack {id,x,y}`; defenders with RANGED_ATTACK in range 3 → `rangedMassAttack` at range 1 else
`rangedAttack`.

### CORE-FOCUS-MAX (stronghold.js:240-299)
`activeTowers` = energy ≥ TOWER_ENERGY_COST. Target = `_.max(hostiles, est)` where est = Σ towers
`floor(TOWER-FALLOFF(600))` + Σ defenders (ranged power if in range 3 + melee power if range 1).
Then melee/ranged as FOCUS-CLOSEST, then active towers `attack`.

### CORE-POPULATION (stronghold.js:301-332)
For each population index i: name `defender${i}`; if a defender with that name exists and the
behaviour exists → run behaviour; else if core not spawning and not `_spawning` → `createCreep
{name, body, boosts}` and set `core._spawning` (one per tick).

### CORE-ANTINUKE (stronghold.js:334-361)
Only when `T % 10 == 0`. For each core rampart not on a container tile: `hitsTarget =
STRONGHOLD_RAMPART_HITS[level] + Σ nukes (range 0: NUKE_DAMAGE[0]; range ≤2: NUKE_DAMAGE[2])`; persist if changed.

### CORE-ASSIGN-DEFENDERS (stronghold.js:363-386; defence.js:37-56)
meleeSpots = unique ramparts within 1 of any hostile; rangerSpots = unique ramparts within 3 minus
meleeSpots. distribute(positions, agentIds): truncate agents to positions; repeatedly take next agent,
`place = _.max(weights,'weight')` (all start 100, first max wins), assign
`result[50*x+y] = agentId`, remove place, every remaining `w.weight -= max(0, remaining - cheb(w,place))`.
Ranger assignment `Object.assign`ed over melee.

### CORE-CREEP-AI (stronghold/creeps.js:15-126; defence.js:8-35; processor/common/fake-runtime.js)
- safeMatrixCallback (only when hostiles exist): all tiles 255, core-user ramparts 1, non-creep
  obstacle objects 255.
- coordinated: walk every assigned creep to its spot (range 0, safe matrix); if core spawning, the
  first unassigned defender adjacent to the core flees (range 2) with a `move` intent.
- simple-melee: if hostiles: `findClosestByPath` (safe matrix); adjacent → `attack`, else walkTo.
- fortifier: needs energy; first rampart with `hitsTarget && hits < hitsTarget`; within 3 → `repair`;
  else walkTo (range 3) then repair the first such rampart now in range.
- bodies: fortifier 15 WORK(XLH2O)/15 CARRY/15 MOVE; weakDefender 15 ATTACK/15 MOVE; fullDefender
  25/25; boostedDefender 25 ATTACK(UH2O)/25 MOVE; boostedRanger 25 RANGED(KHO2)/25 MOVE;
  fullBoostedMelee 44 ATTACK(XUH2O)/6 MOVE(XZHO2); fullBoostedRanger 44 RANGED(XKHO2)/6 MOVE(XZHO2).
- walkTo/moveTo/findPath/flee semantics (path caching in `memory_move`, reusePath 5, pushing idle
  allies) are in fake-runtime.js:143-263 and shared with the NPC creep spec; PathFinder parity required.

### CORE-TRANSFER (processor/intents/invader-core/transfer.js:6-38)
Target tower or creep (no range, no core energy cost). `total` = creep: sum(store), tower: energy;
`cap = storeCapacity || storeCapacityResource.energy`; reject if full; clamp; `energy += amount`;
actionLog.transferEnergy; EVENT_TRANSFER `{targetId, resourceType:'energy', amount}`.

### CORE-CREATE-CREEP (processor/intents/invader-core/create-creep.js:7-66)
Reject if spawning or `!INVADER_CORE_CREEP_SPAWN_TIME[level]` (levels 2-5). Body sliced to 50, invalid
parts skipped; part i boosted if `boosts[i]` is valid for it. `storeCapacity =
calcBodyEffectiveness(body, CARRY, 'capacity', CARRY_CAPACITY, true)`. Insert creep
`{strongholdId, name, x,y = core, body, store{energy:0}, storeCapacity, room, user, hits/hitsMax
100*parts, spawning:true, fatigue:0, notifyWhenAttacked:false, ageTime: core.decayTime}`; persist
`spawning {name, needTime: SPAWN_TIME[level]*parts, spawnTime: T + needTime}`. Stronghold creeps
never drop resources on death (`_die` skips `strongholdId`).

### CORE-RESERVE (processor/intents/invader-core/reserveController.js:6-40)
Target controller; reject if owned or reserved by another user. If no reservation, create
`{user, endTime: T+1}` in memory. `effect = INVADER_CORE_CONTROLLER_POWER * CONTROLLER_RESERVE`;
reject if `endTime + effect > T + CONTROLLER_RESERVE_MAX`; `endTime += effect`, persist;
actionLog.reserveController; EVENT_RESERVE_CONTROLLER `{amount: effect}`.

### CORE-ATTACK-CTRL (processor/intents/invader-core/attackController.js:6-42)
Reject if core spawning; target controller with user or reservation; precedence-quirk safe-mode
check `(ctrl.user != core.user && safeMode > T) || upgradeBlocked > T`. Reservation:
`endTime -= floor(POWER * CONTROLLER_RESERVE)`; owner: `downgradeTime -= floor(POWER *
CONTROLLER_CLAIM_DOWNGRADE)`, `_upgradeBlocked = T + CONTROLLER_ATTACK_BLOCKED_UPGRADE`.
**GOTCHA**: sets `actionLog.reserveController` (not attackController); no invulnerability check
(creep version has one); sets `roomInfo.active`; EVENT_ATTACK_CONTROLLER with **no `data`**.

### CORE-UPGRADE-CTRL (processor/intents/invader-core/upgradeController.js:8-55)
Target controller with `level != 0` and same user; not `upgradeBlocked > T`. Set/extend
EFFECT_INVULNERABILITY `endTime = T + INVADER_CORE_CONTROLLER_DOWNGRADE` (new effect replaces the
array); `downgradeTime = T + INVADER_CORE_CONTROLLER_DOWNGRADE`; `_upgraded += 1` (NaN when unset —
harmless since CTRL tick skips user "2"); actionLog.upgradeController; EVENT_UPGRADE_CONTROLLER
`{amount:1, energySpent:0}`.

### CORE-TICK (processor/intents/invader-core/tick.js:6-47)
1. Collapse effect with `endTime <= T` → reset controller `{user:null, level:0, progress:0,
   downgradeTime:null, safeMode:null, safeModeAvailable:0, safeModeCooldown:null,
   isPowerEnabled:false, effects:null}` and return (PROC-COLLAPSE then removes the core; no ruin, no
   loot).
2. `spawning && T >= spawnTime-1` → SPAWN-BORN with the core as spawn (all 8 directions).
3. actionLog changed → `roomInfo.active = true`, persist.
**GOTCHA**: room activity of stronghold rooms depends on actionLog toggling (PROC-ACTIVATION) — two
quiet ticks in a row make the room dormant.

### CORE-DESTROY (processor/intents/invader-core/destroy.js:7-38)
On HELP-DESTROY of a core: always reset the room controller as in CORE-TICK step 1 (**GOTCHA**: even
if a player has claimed the controller — no `removeRoomFromUser`). If `depositType`, `templateName`
valid: `rewardLevel = template.rewardLevel`; `rewards = coreRewards[depositType][0..rewardLevel]`
(array entries `_.sample`d); `store = calcReward(zip(rewards, coreDensities[0..rewardLevel]),
coreAmounts[rewardLevel])` merged into the core → becomes the ruin store; ruin lasts until
`max(T+RUIN_DECAY, collapse endTime)`. Emit `strongholdDestroyed`. Nuke kills leave no ruin → loot lost.

### STRONGHOLD-SPAWN / STRONGHOLD-GEN / STRONGHOLD-EXPAND (reference/backend/lib/strongholds.js)
- gen (cron every 5 min wall time): for each sector centre room `[WE]\d*5[NS]\d*5` without a core or
  core ruin in its sector, pick a random eligible room (centre-ish, not very centre, not highway, no
  owner/reservation/claimer) → spawnStronghold with deployTime 5000.
- spawnStronghold: scan x,y in 4..44 for templates whose footprint has no wall terrain and no
  rampart/obstacle object; `_.sample` template then spot; `depositType = _.sample(coreRewards keys)`;
  insert core (`deployTime = T+5000`, invulnerability until deployTime, `nextExpandTime = deployTime +
  INVADER_CORE_EXPAND_TIME[level]`, `strongholdId = room_T`) and placeholder ramparts (hits 1, public,
  `nextDecayTime = deployTime`); set controller user "2", level 8, `downgradeTime = deployTime`,
  invulnerable.
- expand (cron every 15 min): cores with `level > 0`, no deployTime, `nextExpandTime < T`: BFS over
  shuffled exits (skipping highways and novice/respawn rooms) to the first room with an unowned
  controller and no core; place a level-0 core within ±5 of the controller (`findFreePos`), sharing
  `strongholdId` and **effects** (collapse timer) of the parent; reset `nextExpandTime`.
These are wall-clock cron jobs; chitin should run them on a tick schedule (Open questions).

### CRON-POWERBANK / CRON-DEPOSIT (reference/backend/lib/cronjobs.js:194-243, 556-643)
Power banks: per highway room every `round(random*RESPAWN/2 + 0.75*RESPAWN)` ticks, on a **wall** tile
in 5..44 with a non-wall neighbour; `power = floor(random*(MAX-MIN)+MIN)` (+MAX with prob
POWER_BANK_CAPACITY_CRIT); hits POWER_BANK_HITS; `decayTime = T + POWER_BANK_DECAY`. Deposits: per
sector while throughput `Σ 20/max(1, exhaust)` < 2.5, in a free highway room with `depositType`, wall
tile with non-wall neighbour and no object within 2, `decayTime = T + DEPOSIT_DECAY_TIME`.

---

## 14. Event log entries emitted by this area (EVT-TABLE)

Event log = array pushed in processing order, saved per processed room (driver/lib/index.js:583-585),
returned to players next tick. Shapes `{event, objectId, data}`:

| Event | objectId | data | Emitters |
|---|---|---|---|
| EVENT_ATTACK (1) | attacker (tower / nuke) | `{targetId, damage, attackType}` RANGED(2) tower, NUKE(6) | HELP-DAMAGE via TOWER-ATTACK, NUKE-LAND |
| EVENT_ATTACK (1) hit-back | power bank | `{targetId: attacker, damage: dmg*0.5, attackType: HIT_BACK(5)}` | HELP-DAMAGE |
| EVENT_OBJECT_DESTROYED (2) | structure | `{type}` | HELP-DAMAGE (not destroyStructure, decay, collapse, deploy, newbie walls) |
| EVENT_OBJECT_DESTROYED (2) | creep | `{type:'creep'}` | NUKE-LAND, SPAWN-BORN stomp, SPAWN-RECYCLE, CORE-DEPLOY (`_die`) |
| EVENT_ATTACK_CONTROLLER (3) | core | *(no data key)* | CORE-ATTACK-CTRL |
| EVENT_BUILD (4) | creep | `{targetId, amount, structureType, x, y, incomplete}` | CSITE-COMPLETE (creep build) |
| EVENT_HEAL (6) | tower | `{targetId, amount, healType: RANGED(2)}` | TOWER-HEAL |
| EVENT_REPAIR (7) | tower | `{targetId, amount(unclamped), energySpent: 10}` | TOWER-REPAIR |
| EVENT_RESERVE_CONTROLLER (8) | core | `{amount: 2}` | CORE-RESERVE |
| EVENT_UPGRADE_CONTROLLER (9) | core | `{amount: 1, energySpent: 0}` | CORE-UPGRADE-CTRL |
| EVENT_TRANSFER (12) | link / core | `{targetId, resourceType:'energy', amount}` (link: pre-loss) | LINK-TRANSFER, CORE-TRANSFER |

No events for: lab reactions/boosts, factory, power spawn, terminal send, nuke launch, spawning,
renew, decay removals, keeper respawn, source/mineral regen, deposit harvest.

---

## 15. Runtime pre-validation cross-reference (RT-XREF)

Return codes come from the runtime methods (reference/engine/src/game/…); the processor then
silently re-validates. Mismatches (runtime OK but processor no-op, or vice versa) are 1:1 hazards:

| Intent | Runtime (codes in check order) | Processor differences |
|---|---|---|
| destroyStructure | structures.js:72-87: INVALID_TARGET, NOT_OWNER, BUSY(hostile creeps/PCs) | rejects unowned decaying walls silently |
| notifyWhenAttacked | structures.js:89-106 (spawn 1220-1235) | none material |
| unclaim | structures.js:201-209: NOT_OWNER | requires `level` |
| activateSafeMode | structures.js:211-234: NOT_OWNER, NOT_ENOUGH_RESOURCES, TIRED, BUSY; one per tick per player | `safeModeCooldown >= T` (one-tick stricter) |
| runReaction | structures.js:317-358: NOT_OWNER, TIRED, RCL, INVALID_TARGET, NOT_IN_RANGE, FULL, NOT_ENOUGH_RESOURCES, INVALID_ARGS | no RCL/owner check |
| reverseReaction | structures.js:360-410 (+INVALID_ARGS lab1==lab2, FULL) | **no range check** |
| boostCreep | structures.js:412-441: …, NOT_FOUND if bodyPartsCount > eligible | no RCL check; boosting during cooldown allowed both sides |
| unboostCreep | structures.js:443-466 | same checks |
| link transferEnergy | structures.js:488-533: INVALID_ARGS(<0), INVALID_TARGET, NOT_OWNER, TIRED, RCL, NOT_ENOUGH_ENERGY, FULL (exact fit required), NOT_IN_RANGE(other room) | processor clamps instead of FULL |
| observeRoom | structures.js:548-570: NOT_OWNER, INVALID_ARGS, RCL, NOT_IN_RANGE (unless OPERATE_OBSERVER) | **no checks** |
| processPower | structures.js:613-631 (`ticksRemaining > 0`) | partial batch with effect; `endTime >= T` |
| setPublic | structures.js:651-657 | — |
| terminal send | structures.js:714-745: NOT_OWNER, RCL, INVALID_ARGS, NOT_ENOUGH_RESOURCES, TIRED, NOT_ENOUGH_RESOURCES(energy), INVALID_ARGS(description) | no cooldown/RCL in room phase; cooldown checked in global phase |
| tower attack/heal/repair | structures.js:766-824: NOT_OWNER, INVALID_TARGET, NOT_ENOUGH_ENERGY, RCL | no RCL check; repair rejects full/non-structure targets silently |
| spawnCreep | structures.js:1063-1218: INVALID_ARGS, NAME_EXISTS, INVALID_ARGS(directions), NOT_OWNER, BUSY, RCL(`off`), INVALID_ARGS(body), NOT_ENOUGH_ENERGY | uses HELP-STRUCT-ACTIVE instead of `off`; no name check |
| renewCreep | structures.js:1237-1267: BUSY, INVALID_TARGET, NOT_OWNER, RCL(`off`), NOT_IN_RANGE, NOT_ENOUGH_ENERGY, FULL | no `off` check |
| recycleCreep | structures.js:1269-1290 | no `off` check |
| setDirections / cancel | structures.js:1312-1334 | — |
| launchNuke | structures.js:1356-1388: NOT_OWNER, INVALID_ARGS, INVALID_TARGET(novice/respawn either room), TIRED, RCL, NOT_IN_RANGE, NOT_ENOUGH_RESOURCES | target-room status not checked (nuke removed on arrival instead) |
| produce | structures.js:1434-1470 | same checks |
| createConstructionSite | rooms.js:1029-1097: INVALID_ARGS, NOT_OWNER, RCL, INVALID_TARGET, FULL(MAX_CONSTRUCTION_SITES) | no site limit |
| createFlag / setColor / setPosition / remove | rooms.js:978-1027; flags.js:57-91 | name uniqueness only per room |
| construction site remove | construction-sites.js:50-56 | double refund possible |

---

## 16. PERF summary

- Hot per-tick paths in every active room: one full object scan for init, one for ticks; per-object
  linear scans inside ticks (`_.find`/`_.filter` on roomObjects: containers/drops in
  HELP-CREATE-ENERGY, legacy swamp/wall objects in ROAD-DECAY and CSITE creation, rampart lookup in
  TOWER-ATTACK, spawns filter+sort per spawn in SPAWN-TICK-REGEN, keeper lookup per lair, 25 tile
  filters per nuke, HELP-STRUCT-ACTIVE per intent). chitin: per-room tile index (x,y → small vec)
  and per-(type,user) lists maintained incrementally; precompute active-structure ranks once per tick.
- Unconditional writes every tick: observer `observeRoom`, link actionLog while on cooldown,
  controller `addRoomToUser` check, spawn regen. Only emit DB/state deltas when values change.
- Timers are absolute (`nextDecayTime`, `nextRegenerationTime`, `spawnTime`, `landTime`, …) — ideal
  for a per-room min-heap of wake times instead of scanning, but PROC-ACTIVATION semantics (when a
  room is processed at all) must be preserved for 1:1 decay rates.
- Room-local: all ticks/intents here except: NUKER-LAUNCH (insert into another room — queue as a
  cross-room message applied before that room's next processing), TERMINAL-SEND-EXEC (global phase,
  reads all terminals), CTRL user `rooms` list, PSPAWN user `power`, stats, notifications, crons.
- RNG: Math.random only in MINERAL-DENSITY, CORE-DEPLOY duration, HELP-CALC-REWARD (+lodash shuffle),
  CORE-REFILL-TOWERS, bunker4/5 population shuffles, CORE-DESTROY `_.sample`, ROOM-GEN-ENERGY,
  backend crons. A seeded per-room RNG keeps rooms independent and deterministic.
- The original single-threaded processor also deep-clones every bulk update (`_.cloneDeep`) and
  JSON-serialises history each tick — large constant costs chitin can avoid.

---

## 17. Open questions

1. CONV-BULK-INSERTCNT (lost insert after same-tick insert+remove) and ROOM-FLAGS-MULTIUSER (lost
   flag edits / room-tick crash when two users send room intents to one room): replicate, or fix and
   document as a deliberate divergence? Bots cannot rely on them, but they are observable.
2. CONV-CRASH paths (nuke on power bank, power creep on stronghold footprint at deploy, cancelSpawning
   without creep, flags crash): replicate the "whole room tick lost" outcome or fix? The nuke/power
   bank case changes gameplay (nuke fizzles).
3. User processing order within a room (`intents.users` key order = order runner results were merged
   into `rooms.intents`, effectively nondeterministic in the original). chitin must pick a
   deterministic order (e.g. by user id or runner completion); parity can only be distributional.
4. Wall-clock dependencies (novice/respawn timestamps, portal `unstableDate`, wall `decayTime.timestamp`,
   cron intervals in seconds, `roomsForceUpdate` 20 s cadence): map to real time or to ticks?
5. The order of `roomObjects` keys after DB load (storage `find` order) is not specified by the
   engine; tie-breaks (HELP-STRUCT-ACTIVE, stable sorts, event order) depend on it. Assumed: insertion
   (creation) order. Needs confirmation against reference/storage behaviour.
6. `Array.prototype.sort` stability: assumed stable (Node ≥ 11). Confirm the official server's Node
   version if exact tie behaviour on very old shards matters.
7. Newbie/respawn walls (`ticksToLive`, `decayTime.timestamp`, `newbieWall`) have no creator in the
   reference backend; implement only for data import compatibility?
