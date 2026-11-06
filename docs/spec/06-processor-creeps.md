# 06 — Processor: Creep & Power-Creep Intents, Creep Tick, Movement, Death, NPC AI

Scope: everything the **server** does with a creep or power creep after the player runners finished: the room-processor ordering, every creep / power-creep intent (re-validation, effects, numbers), damage/heal queuing and application, the complete movement algorithm (destinations, conflicts, swaps, chains, pull, fatigue, exits, portals, inter-room transfer), creep ticking (spawning, birth, aging, fatigue recovery, hits), death (tombstones, corpse resources), and the built-in NPC AI (Invaders, Source Keepers; stronghold defenders summarised). Player-side (API) validation is covered elsewhere; it is mentioned here only where the server relies on it.

Sources (read-only, cited `reference/<pkg>/<path>:<line>`): engine @8097782, driver @cf63d8a, common @2fb779b, storage @3d6c3d0, backend 3.3.0, docs, lodash 3.10.1, xxscreeps @d1e3bad (comparison only).

Conventions
- `T` = `gameTime` of the tick being processed (same value as `Game.time` the runners saw this tick) — `reference/engine/src/processor.js:563`.
- `dist(a,b)` = Chebyshev `max(|dx|,|dy|)` — `reference/engine/src/utils.js:638-642`. "range ≤ r" in this spec always means `|dx| ≤ r && |dy| ≤ r`.
- "alive part" = body part with `hits > 0` (the code also accepts a transient `_oldHits`, which is provably never present at read time — see BODY-OLDHITS).
- Numbers are JS doubles. `undefined + n = NaN`; every comparison with NaN is false. Several checks below rely on that; chitin must reproduce IEEE/JS semantics for stored amounts (store values, hits, fatigue), not saturating integer math.
- Loc tags: **R** room-local; **U** writes a per-user document (cross-room; GCL, rooms list, stats, notifications); **G** global stage / cross-room.
- "No-op" = the intent handler `return`s without writing anything, emitting no event and not touching `actionLog`.

---

## 0. Tick pipeline (only what this area depends on)

1. All player runners run; each `Creep`/`PowerCreep` method that passes API checks calls `intents.set(id, name, data)` (one entry per (object, name); later calls overwrite) — `reference/driver/lib/runtime/runtime.js:63-72`.
2. Intents are sanitized and split per room (`reference/engine/src/utils.js:405-436`, `reference/common/lib/system.js:84-114`) and `$merge`d into `rooms.intents[room].users[userId].objects` — `reference/driver/lib/index.js:180-243`.
3. Every active room is processed independently (`processRoom`) — `reference/engine/src/main.js:42-56`, `reference/engine/src/processor.js:16-520`.
4. DB commit, then the global stage: creeps/power creeps with `interRoom` are moved to their new room; power-creep global intents and power-creep death — `reference/engine/src/main.js:57-66`, `reference/engine/src/processor/global.js:9-64`.
5. `gameTime++` — `reference/engine/src/main.js:72-76`.

**PERF**: all intent handlers, movement, damage and creep ticks are room-local. The only cross-room effects in this area are: inter-room/portal transfer (step 4), `claimController` (user rooms list), `upgradeController` (GCL `$inc`), stats counters, attack notifications, and power-creep death/spawn (step 4). Rooms can be processed fully in parallel if those are emitted as deltas and applied in a serial global stage.

---

## 1. Inventory

| ID | Item | Loc | Primary source |
|---|---|---|---|
| PROC-PIPELINE | Phase order of `processRoom` | R | reference/engine/src/processor.js:16-520 |
| PROC-PREPASS | Per-object pre-pass: actionLog reset, `_ticksToLive`, NPC lists | R | reference/engine/src/processor.js:43-171 |
| PROC-NPC-PRETICK | Nuke pretick, keeper AI, invader AI, invader-core AI, intent merge | R | reference/engine/src/processor.js:176-219 |
| PROC-USER-ORDER | Order of users in the intents loop (NPC "2","3" first) | R | reference/engine/src/processor.js:229; reference/driver/lib/index.js:236 |
| PROC-OBJECT-ORDER | Order of objects within one user's intents | R | reference/driver/lib/runtime/runtime.js:66-72; reference/engine/src/utils.js:409-433 |
| PROC-OWNER-FILTER | Intents only applied to own (or ownerless) objects | R | reference/engine/src/processor.js:248 |
| PROC-NOTIFY-WHEN-ATTACKED | `notifyWhenAttacked` intent for creeps/PCs | R | reference/engine/src/processor.js:313-315 |
| PROC-TICK-ORDER | Tick loop order (DB order, key snapshot, temp ids first) | R | reference/engine/src/processor.js:342-483 |
| PROC-STATE-VISIBILITY | Which effects are immediate vs deferred within a tick | R | (cross-cutting, see section) |
| PROC-BULK | `bulk.update` in-memory merge, `_` stripping, insert-id reuse | R | reference/driver/lib/bulk.js:6-131 |
| PROC-SANITIZE | Server-side intent field whitelist & type coercion | R | reference/common/lib/system.js:4-114 |
| PROC-MOD-HOOKS | `processObject` / `preProcessObjectIntents` / `processObjectIntents` / `postProcessObject` / `processRoom` events | R | reference/engine/src/processor.js:169,246,318,482,489 |
| PROC-ACTIVATION | Which objects keep a room processed (keepers do not) | R | reference/engine/src/processor.js:119-140,499-507 |
| PROC-HISTORY-SAY | Private `say` stripped from room history | R | reference/engine/src/processor.js:431-439 |
| CREEP-INTENT-ORDER | Fixed per-creep intent execution order | R | reference/engine/src/processor/intents/creeps/intents.js:15-17,25-31 |
| CREEP-INTENT-PRIORITY | Mutually exclusive action pipelines | R | reference/engine/src/processor/intents/creeps/intents.js:3-13,21-23 |
| CREEP-GUARDS | Common `type`/`spawning` guards per intent | R | (each handler) |
| CREEP-MOVE | `move` intent → tentative destination | R | reference/engine/src/processor/intents/creeps/move.js:7-45 |
| CREEP-PULL | `pull` intent → pull link | R | reference/engine/src/processor/intents/creeps/pull.js:7-22 |
| CREEP-HARVEST-SOURCE | Harvest from source | R | reference/engine/src/processor/intents/creeps/harvest.js:25-65 |
| CREEP-HARVEST-MINERAL | Harvest from mineral (extractor) | R | reference/engine/src/processor/intents/creeps/harvest.js:67-112 |
| CREEP-HARVEST-DEPOSIT | Harvest from deposit | R | reference/engine/src/processor/intents/creeps/harvest.js:114-139 |
| CREEP-ATTACK | Melee attack | R | reference/engine/src/processor/intents/creeps/attack.js:6-45 |
| CREEP-RANGED-ATTACK | Ranged attack | R | reference/engine/src/processor/intents/creeps/rangedAttack.js:6-43 |
| CREEP-RANGED-MASS-ATTACK | Ranged mass attack | R | reference/engine/src/processor/intents/creeps/rangedMassAttack.js:6-59 |
| CREEP-HEAL | Melee heal | R | reference/engine/src/processor/intents/creeps/heal.js:6-34 |
| CREEP-RANGED-HEAL | Ranged heal | R | reference/engine/src/processor/intents/creeps/rangedHeal.js:6-34 |
| CREEP-DISMANTLE | Dismantle | R | reference/engine/src/processor/intents/creeps/dismantle.js:6-54 |
| CREEP-BUILD | Build progress | R | reference/engine/src/processor/intents/creeps/build.js:9-107 |
| CREEP-BUILD-COMPLETE | Site → structure creation | R | reference/engine/src/processor/intents/creeps/build.js:108-293 |
| CREEP-REPAIR | Repair | R | reference/engine/src/processor/intents/creeps/repair.js:6-62 |
| CREEP-UPGRADE-CONTROLLER | Upgrade controller (+GCL) | R,U | reference/engine/src/processor/intents/creeps/upgradeController.js:7-109 |
| CREEP-CLAIM-CONTROLLER | Claim controller | R,U | reference/engine/src/processor/intents/creeps/claimController.js:6-56 |
| CREEP-RESERVE-CONTROLLER | Reserve controller | R | reference/engine/src/processor/intents/creeps/reserveController.js:6-53 |
| CREEP-ATTACK-CONTROLLER | Attack controller | R | reference/engine/src/processor/intents/creeps/attackController.js:6-53 |
| CREEP-SIGN-CONTROLLER | Sign controller | R | reference/engine/src/processor/intents/creeps/signController.js:6-28 |
| CREEP-GENERATE-SAFE-MODE | Generate safe mode | R | reference/engine/src/processor/intents/creeps/generateSafeMode.js:6-25 |
| CREEP-TRANSFER | Transfer | R | reference/engine/src/processor/intents/creeps/transfer.js:6-60 |
| CREEP-WITHDRAW | Withdraw | R | reference/engine/src/processor/intents/creeps/withdraw.js:6-61 |
| CREEP-PICKUP | Pickup | R | reference/engine/src/processor/intents/creeps/pickup.js:6-39 |
| CREEP-DROP | Drop | R | reference/engine/src/processor/intents/creeps/drop.js:6-23 |
| CREEP-SAY | Say | R | reference/engine/src/processor/intents/creeps/say.js:6-21 |
| CREEP-SUICIDE | Suicide | R | reference/engine/src/processor/intents/creeps/suicide.js:6-15 |
| BODY-EFFECTIVENESS | `calcBodyEffectiveness` (alive parts × boost) | R | reference/engine/src/utils.js:623-636 |
| BODY-WORK-BOOST-BONUS | Boost bonus via sorted/sliced part list (counts dead parts) | R | reference/engine/src/processor/intents/creeps/build.js:67-80 |
| BODY-OLDHITS | `_oldHits` is transient and never observable | R | reference/engine/src/processor/intents/creeps/_recalc-body.js:11; reference/driver/lib/bulk.js:6-23 |
| BODY-RECALC | Distribute hits over parts, recompute `storeCapacity` | R | reference/engine/src/processor/intents/creeps/_recalc-body.js:6-23 |
| BODY-DROP-OVERFLOW | Drop resources exceeding new capacity | R | reference/engine/src/processor/intents/creeps/_drop-resources-without-space.js:6-17 |
| RES-CREATE-DROP | `_create-energy`: container first, merge into existing pile, else insert | R | reference/engine/src/processor/intents/_create-energy.js:6-54 |
| DMG-QUEUE | `_damage`: creeps/PCs queued, structures immediate | R | reference/engine/src/processor/intents/_damage.js:6-94 |
| DMG-RAMPART-REDIRECT | Attacks on anything under a rampart hit the rampart | R | reference/engine/src/processor/intents/creeps/attack.js:33-36 |
| DMG-HITBACK-MELEE | Melee counter-damage from target's ATTACK parts | R | reference/engine/src/processor/intents/_damage.js:16-19,86-91 |
| DMG-HITBACK-POWERBANK | Power bank reflects 50% of any damage | R | reference/engine/src/processor/intents/_damage.js:35-37 |
| DMG-SAFEMODE | Safe-mode blocks attacks/heals by non-owners; cancels hit-back for owner | R | reference/engine/src/processor/intents/creeps/attack.js:30-32; _damage.js:39-41 |
| DMG-FORTIFY | Fortify / invulnerability immunity of walls & ramparts | R | reference/engine/src/processor/intents/_damage.js:26-31 |
| DMG-NOTIFY-PVP | Attack notifications and `lastPvpTime` | R,U | reference/engine/src/processor/intents/_damage.js:77-84 |
| DMG-EVENTS | `EVENT_ATTACK` / hit-back events / actionLog | R | reference/engine/src/processor/intents/_damage.js:59-93 |
| DMG-TOUGH | TOUGH boost damage reduction at application | R | reference/engine/src/processor/intents/creeps/tick.js:7-29 |
| HEAL-QUEUE | Heals queued in `_healToApply` | R | reference/engine/src/processor/intents/creeps/heal.js:28 |
| CREEP-HITS-APPLY | Damage then heal netting, clamp, death, recalc | R | reference/engine/src/processor/intents/creeps/tick.js:118-150 |
| MOVE-INIT | Per-room movement state reset | R | reference/engine/src/processor/intents/movement.js:61-67 |
| MOVE-PREFILTER | Static obstacle filter at intent time | R | reference/engine/src/processor/intents/creeps/move.js:32-44 |
| MOVE-ADD | Register a tentative move | R | reference/engine/src/processor/intents/movement.js:84-98 |
| MOVE-PULL-LINK | `addPulling` with cycle check | R | reference/engine/src/processor/intents/movement.js:69-75 |
| MOVE-CONFLICT | One winner per destination tile (rates) | R | reference/engine/src/processor/intents/movement.js:107-150 |
| MOVE-TIEBREAK | Stable sort → first-registered wins ties | R | reference/engine/src/processor/intents/movement.js:139 |
| MOVE-CANMOVE | Fatigue / MOVE-part / pulled gate | R | reference/engine/src/processor/intents/movement.js:11-14 |
| MOVE-OBSTACLE | Dynamic obstacle test at the destination | R | reference/engine/src/processor/intents/movement.js:16-39 |
| MOVE-SAFEMODE-PASS | Safe-mode owner walks through stationary foreign creeps | R | reference/engine/src/processor/intents/movement.js:22 |
| MOVE-PULL-VALIDATE | Pull link validated against destinations | R | reference/engine/src/processor/intents/movement.js:176-181 |
| MOVE-CHAIN-CANCEL | Blocked mover cancels everyone moving into its tile, recursively | R | reference/engine/src/processor/intents/movement.js:154-165,183-185 |
| MOVE-SWAP-CYCLE | Swaps, rotations and follow-chains are legal | R | reference/engine/src/processor/intents/movement.js:22 |
| MOVE-NO-REDISPATCH | A failed winner is not replaced by a loser | R | reference/engine/src/processor/intents/movement.js:141-150 |
| MOVE-EXECUTE | Apply move in the creep's own tick | R | reference/engine/src/processor/intents/movement.js:189-252 |
| MOVE-FATIGUE-COST | Terrain rate × weight (incl. dead parts, carried load) | R | reference/engine/src/processor/intents/movement.js:41-59,204-240 |
| MOVE-FATIGUE-CHAIN | Fatigue/recovery routed to the pull-train head | R | reference/engine/src/processor/intents/creeps/_add-fatigue.js:6-35 |
| MOVE-ROAD-WEAR | Road `nextDecayTime` reduction | R | reference/engine/src/processor/intents/movement.js:211-222 |
| MOVE-SITE-STOMP | Foreign construction sites destroyed by stepping on them | R | reference/engine/src/processor/intents/movement.js:224-233 |
| MOVE-EDGE-ZERO-FATIGUE | Entering an exit tile from inside costs 0 and resets fatigue | R | reference/engine/src/processor/intents/movement.js:242-251 |
| MOVE-TILE-BUSY | `isTileBusy` used by spawn placement | R | reference/engine/src/processor/intents/movement.js:100-102 |
| CREEP-TICK-ORDER | Order of steps inside one creep's tick | R | reference/engine/src/processor/intents/creeps/tick.js:31-151 |
| CREEP-TICK-SPAWNING | Spawning creep handling in its own tick | R | reference/engine/src/processor/intents/creeps/tick.js:37-48 |
| CREEP-BIRTH | `_born-creep` placement (directions, obstacles) | R | reference/engine/src/processor/intents/spawns/_born-creep.js:7-79 |
| CREEP-SPAWN-STOMP | Spawn kills a hostile creep to place the newborn | R | reference/engine/src/processor/intents/spawns/_born-creep.js:52-76 |
| CREEP-EXIT | Standing on an exit tile → `interRoom` | R | reference/engine/src/processor/intents/creeps/tick.js:52-78 |
| CREEP-PORTAL | Standing on a portal → `interRoom = destination` | R | reference/engine/src/processor/intents/creeps/tick.js:80-85 |
| MOVE-INTERROOM | Global stage moves creeps/PCs to `interRoom` | G | reference/engine/src/processor/global.js:33-43 |
| CREEP-AGE | `ageTime` initialisation and age death | R | reference/engine/src/processor/intents/creeps/tick.js:87-96 |
| CREEP-FATIGUE-RECOVER | −2 × MOVE power per tick | R | reference/engine/src/processor/intents/creeps/tick.js:105-108 |
| CREEP-ACTIONLOG-SAVE | actionLog persisted only when changed | R | reference/engine/src/processor/intents/creeps/tick.js:98-100 |
| CREEP-USER-SUMMONED | Summoned invaders die when foreign creeps present | R | reference/engine/src/processor/intents/creeps/tick.js:114-116 |
| CREEP-DIE | `_die`: removal, tombstone, events, stats | R,U | reference/engine/src/processor/intents/creeps/_die.js:6-102 |
| CREEP-CORPSE-RESOURCES | Body-part reclaim formula + carried store | R | reference/engine/src/processor/intents/creeps/_die.js:39-92 |
| CREEP-DIE-VARIANTS | dropRate / violent / tombstone per death cause | R | (see section) |
| CREEP-DIE-DOUBLE | A creep can `_die` twice in one tick (2 tombstones) | R | reference/engine/src/processor/intents/creeps/tick.js:93-135 |
| PC-INTENT-ORDER | Power-creep intent execution order (no priorities) | R | reference/engine/src/processor/intents/power-creeps/intents.js:3-13 |
| PC-MOVE | PC move (delegates to creep move) | R | reference/engine/src/processor/intents/power-creeps/move.js:7-10 |
| PC-USE-POWER | `usePower` common validation & bookkeeping | R | reference/engine/src/processor/intents/power-creeps/usePower.js:6-310 |
| PC-POWER-GENERATE-OPS | PWR_GENERATE_OPS | R | reference/engine/src/processor/intents/power-creeps/usePower.js:57-70 |
| PC-POWER-OPERATE-EXTENSION | PWR_OPERATE_EXTENSION (fills extensions) | R | reference/engine/src/processor/intents/power-creeps/usePower.js:104-132 |
| PC-POWER-SHIELD | PWR_SHIELD (inserts rampart) | R | reference/engine/src/processor/intents/power-creeps/usePower.js:228-257 |
| PC-POWER-REGEN-MINERAL | PWR_REGEN_MINERAL extra preconditions | R | reference/engine/src/processor/intents/power-creeps/usePower.js:182-194 |
| PC-POWER-OPERATE-FACTORY | PWR_OPERATE_FACTORY level binding | R | reference/engine/src/processor/intents/power-creeps/usePower.js:259-274 |
| PC-POWER-EFFECT | Effect insertion/replacement on target | R | reference/engine/src/processor/intents/power-creeps/usePower.js:277-289 |
| PC-POWER-TARGET-TYPES | Target type table for effect powers | R | reference/engine/src/processor/intents/power-creeps/usePower.js:72-226 |
| PC-WITHDRAW | PC withdraw (delegates to CREEP-WITHDRAW) | R | reference/engine/src/processor/intents/power-creeps/withdraw.js:6-8 |
| PC-TRANSFER | PC transfer (delegates to CREEP-TRANSFER) | R | reference/engine/src/processor/intents/power-creeps/transfer.js:6-9 |
| PC-DROP | PC drop (delegates to CREEP-DROP) | R | reference/engine/src/processor/intents/power-creeps/drop.js:6-9 |
| PC-PICKUP | PC pickup (delegates to CREEP-PICKUP) | R | reference/engine/src/processor/intents/power-creeps/pickup.js:6-9 |
| PC-SAY | PC say (no spawning check) | R | reference/engine/src/processor/intents/power-creeps/say.js:6-15 |
| PC-ENABLE-ROOM | `enableRoom` | R | reference/engine/src/processor/intents/power-creeps/enableRoom.js:6-21 |
| PC-RENEW | `renew` at power bank / power spawn | R | reference/engine/src/processor/intents/power-creeps/renew.js:6-23 |
| PC-TICK | PC tick: move, exit, portal, hits | R | reference/engine/src/processor/intents/power-creeps/tick.js:7-73 |
| PC-DEATH | PC death (age/hits) in global stage, tombstone | G | reference/engine/src/processor/global-intents/power.js:30-36; power/_diePowerCreep.js:6-59 |
| PC-SPAWN | PC appears on power spawn (global) | G | reference/engine/src/processor/global-intents/power/spawnPowerCreep.js:7-41 |
| PC-SUICIDE | PC suicide (global) | G | reference/engine/src/processor/global-intents/power/suicidePowerCreep.js:6-16 |
| NPC-KEEPER-SPAWN | Keeper lair (re)spawn | R | reference/engine/src/processor/intents/keeper-lairs/tick.js:6-67 |
| NPC-KEEPER-AI | Source Keeper AI | R | reference/engine/src/processor/intents/creeps/keepers/pretick.js:7-71 |
| NPC-INVADER-SPAWN | Invader raid generation (backend cron) | G | reference/backend/lib/cronjobs.js:245-441 |
| NPC-INVADER-SUMMON | User-summoned invaders (backend API) | G | reference/backend/lib/game/api/game.js:631-704 |
| NPC-INVADER-DISPATCH | Invader pretick lists & role dispatch | R | reference/engine/src/processor/intents/creeps/invaders/pretick.js:7-52 |
| NPC-INVADER-HEALER | Healer role | R | reference/engine/src/processor/intents/creeps/invaders/healer.js:8-61 |
| NPC-INVADER-ATTACKER | Attacker role (`findAttack`) | R | reference/engine/src/processor/intents/creeps/invaders/findAttack.js:17-120 |
| NPC-INVADER-FLEE | Flee helper | R | reference/engine/src/processor/intents/creeps/invaders/flee.js:7-20 |
| NPC-INVADER-SHOOT | `shootAtWill` | R | reference/engine/src/processor/intents/creeps/invaders/shootAtWill.js:7-22 |
| NPC-FAKE-PATH | `fakeRuntime.findPath` + default cost matrix | R | reference/engine/src/processor/common/fake-runtime.js:104-182 |
| NPC-FAKE-MOVETO | `moveTo` with path cache in `memory_move` | R | reference/engine/src/processor/common/fake-runtime.js:200-245,311-332 |
| NPC-FAKE-FINDCLOSEST | `findClosestByPath` (last match wins) | R | reference/engine/src/processor/common/fake-runtime.js:265-309 |
| NPC-FAKE-FLEE | `fakeRuntime.flee` | R | reference/engine/src/processor/common/fake-runtime.js:185-198 |
| NPC-FAKE-WALKTO | `walkTo` (pushes own creep ahead) | R | reference/engine/src/processor/common/fake-runtime.js:247-263 |
| NPC-STRONGHOLD-CREEPS | Stronghold defender behaviours (summary; orchestration in invader-core spec) | R | reference/engine/src/processor/intents/invader-core/stronghold/creeps.js:15-128 |

---

## 2. Room processing order (PROC-*)

### PROC-PIPELINE
`processRoom` executes strictly in this order (`reference/engine/src/processor.js`):
1. **Pre-pass** over `roomObjects` (lines 43-171) — PROC-PREPASS.
2. Nuke pretick for each nuke (176-178).
3. Keeper AI for each keeper in pre-pass order (180-192), then invader AI for each invader (194-206), then invader-core AI if present and owned (208-219) — PROC-NPC-PRETICK.
4. `_calc_spawns` (221-223) (spawn spec).
5. `movement.init(roomObjects, terrain)` (225).
6. **Intents loop**: users → objects → per-type handler (227-322).
7. `movement.check(safeModeUser | false)` (324) where `safeModeUser = roomController.user` iff `roomController.safeMode > T`.
8. `scope.energyAvailable` (326).
9. **Tick loop**: for every object in `roomObjects` order, its type's `tick` (342-483). Creeps and power creeps execute their move here (MOVE-EXECUTE).
10. Persist (mapView, bulk writes, event log), room activation, history (493-518).

### PROC-PREPASS
For each object (DB order; `reference/engine/src/processor.js:43-171`):
- `creep`: `_actionLog = actionLog`; `_ticksToLive = ageTime − T` (**NaN** when `ageTime` is absent, i.e. spawning creeps, fresh NPCs); `actionLog` reset to `{attacked, healed, attack, rangedAttack, rangedMassAttack, rangedHeal, harvest, heal, repair, build, say, upgradeController, reserveController}` all `null` (48-65). `user == "3"` → keeper list; `user == "2" && !strongholdId` → invader list (66-70).
- `powerCreep`: `_actionLog`, `actionLog = {spawned, attack, attacked, healed, power, say}` all null (151-161).
- `controller` → `scope.roomController` (106-107).
- emits `processObject` (169).

### PROC-NPC-PRETICK
Each AI returns `{objectId: {intentName: data}}`. They are merged into `intents.users[npcUser].objects[objId] = _.assign(aiIntents, existingIntents || {})` — existing DB intents for that object win per intent name (183-191, 197-205, 211-218). The AIs run **before** any player intent is applied and see start-of-tick state, except their own immediate writes (`memory_sourceId`, `memory_move` via `bulk.update`, which merge into the live object — PROC-BULK). Keeper AI runs for all keepers first, then invaders, then the core.
**PERF**: each AI call re-scans all room objects (O(N) per NPC) and runs up to several PathFinder searches per NPC per tick (NPC-*). In the original this is the most expensive per-object code in the processor.

### PROC-USER-ORDER — **GOTCHA** (the big ordering rule)
`_.forEach(intents.users, ...)` iterates `Object.keys` order: **integer-like keys first in ascending numeric order, then string keys in insertion order** (`reference/engine/src/processor.js:229`; lodash uses native keys for plain objects).
- NPC user ids `"2"` (Invader) and `"3"` (Source Keeper) are integer-like → invader intents are processed first, then keeper intents, then all players.
- Player ids are 15–16 hex chars from `genId` (`reference/storage/lib/db.js:316-328`), never canonical array indices → insertion order = the order in which each player's runner first `$merge`d intents for this room (`reference/driver/lib/index.js:236`, `reference/storage/lib/db.js:261-263`). That is runner-completion order → **non-deterministic** in the original.
- Consequences: who gets a contested resource (two users withdrawing from the same container, two claimers on one controller, two creeps building the last progress point, picking up the same pile), and movement tie-breaks (MOVE-TIEBREAK) depend on this order.
- chitin: NPC-first must be kept (bots observe NPCs winning ties); choose a deterministic order for players (document it; e.g. stable per-user index). Any fixed order is indistinguishable from some original run.

### PROC-OBJECT-ORDER — **GOTCHA**
Within a user, `for (objectId in userIntents.objects)` (processor.js:236) follows the key insertion order of the runner's `intents.list`, i.e. **the order in which the player's script first successfully called an intent-producing method on each object this tick** (`reference/driver/lib/runtime/runtime.js:66-72`, `reference/engine/src/utils.js:409-433`). Re-calling a method later in the tick does not move the object. Objects with no intents are not visited. `objectsManual` (backend-originated) intents are `_.extend`ed onto `objects` (231-234): they override same-id entries in place and append new ids at the end.

### PROC-OWNER-FILTER
Skip if `!object || object._skip || (object.user && object.user != userId)` (248). Ownerless objects accept any user's intents (only matters for structure handlers). A creep deleted earlier this tick (suicide, recycle) is skipped.

### PROC-NOTIFY-WHEN-ATTACKED
After the type handler: if `objectIntents.notifyWhenAttacked` and object is a creep/powerCreep (or buildable structure) → `notifyWhenAttacked = !!enabled` (313-315).

### PROC-TICK-ORDER — **GOTCHA**
`_.forEach(roomObjects, ...)` computes the key list once at loop start, then reads `roomObjects[key]`; objects deleted mid-loop yield `undefined` and are skipped (344-346); objects added mid-loop are not visited. Key order = DB find order (LokiJS index order ≈ insertion order; `reference/driver/lib/index.js:249-258,331-337`), **except** objects inserted earlier this tick by `_create-energy` whose temporary keys are `"1"`, `"2"`, … (integer-like → enumerated first) — so drops created during the intent phase tick (decay) before every DB object. Structures completed by `build` use keys `createdStructureN` (end of order).
Creep position updates, deaths and births happen at the creep's slot in this order, so anything that inspects positions during the tick loop (spawn placement, nukes, tombstone position) is order-dependent.

### PROC-STATE-VISIBILITY
Immediate (visible to every later intent/tick in the same room-tick): store changes, structure hits (attack/dismantle/repair), construction progress & completion (new structure enters `roomObjects`), source `energy`, mineral `mineralAmount`, deposit `harvested`, drops created/removed, controller fields written via `bulk.update` (claim, reserve, attackController reservation/downgradeTime, upgrade level/progress), creep removal by suicide/recycle.
Deferred: creep & power-creep damage/heal (applied in their tick), all positions (applied in each mover's tick), fatigue, extractor cooldown (`_cooldown` → extractor tick), deposit `cooldownTime` (`_cooldown` → deposit tick), controller `upgradeBlocked` (`_upgradeBlocked` → controller tick), `_upgraded` downgrade restore.
Never visible this tick: tombstones (`bulk.insert` only), creeps created by spawns/lairs/cores (`bulk.insert` only).

### PROC-BULK — **GOTCHA**
`reference/driver/lib/bulk.js`:
- `update(obj, data)` (33-63): deep-clones `data`; for object-valued keys merges the clone **into the live sub-object** (`id[key] || {}`) and uses the live sub-object as the value; then `_.merge(obj, data)` → the live object reflects the write immediately (this is why later intents see effects). `undefined` source values are skipped by `_.merge`; `null` is assigned. Arrays merge index-wise; merging an array into a live `null` produces a plain object with keys `"0","1",…` (this is how `effects` ends up as an object — hence `Object.values(target.effects || [])` in usePower).
- `removeHidden` (6-23) deletes every key starting with `_` from the written data recursively; because nested values are the live sub-objects, it also deletes `_`-keys from the live object (BODY-OLDHITS).
- `insert` returns a counter `1,2,3…` (64-77). `remove(id)` of a still-pending insert deletes it and **decrements the counter** (82-86); the next insert reuses that number and overwrites any other pending insert with the same number (original data-loss bug). Realistic trigger: a 1-unit drop created in the intent phase decays to 0 in the same tick's energy tick (`reference/engine/src/processor/intents/energy/tick.js:12-19`) while ≥2 inserts are pending. chitin should use unique ids; see Open questions.
- `inc(obj, key, n)` (90-100) updates the live object and queues `$inc`.

### PROC-SANITIZE
Before storage, each object intent is reduced to whitelisted fields with coercions (`reference/common/lib/system.js:4-82,84-114`): `number` → `parseInt` (decimals truncated, NaN possible), `string` → `"" + v` (missing → the string `"undefined"`), `boolean` → `!!v`, `userString` → `("" + (v || "")).substring(0,100)`. Relevant shapes: `move {id, direction}`, `pull {id}`, `attack/heal/build/repair {id,x,y}`, `rangedAttack/rangedHeal/dismantle/harvest/pickup/claimController/reserveController/attackController/upgradeController/generateSafeMode {id}`, `rangedMassAttack {}`, `suicide {}`, `say {message: userString, isPublic: boolean}`, `signController {id, sign: userString}`, `transfer/withdraw {id, amount, resourceType}`, `drop {amount, resourceType}`, `usePower {power, id}`, `enableRoom/renew {id}`. Mods add `customIntentTypes`.
**GOTCHA**: a `move` by direction arrives with `id: "undefined"`; a `move` by creep arrives with `direction: NaN`; `usePower` without target arrives with `id: "undefined"` (it is copied into the event log). NPC AI intents are **not** sanitized (e.g. keeper `attack {id: undefined}`).

### PROC-MOD-HOOKS
Emitted via `driver.config.emit`: `processObject(object, …)` per object in pre-pass (169); `preProcessObjectIntents(object, userId, objectIntents, …)` before the owner filter (246); `processObjectIntents(...)` after built-in handlers (318); `postProcessObject(object, …, eventLog, mapView)` per object at end of its tick (482); `processRoom(roomId, …)` once (489). chitin's mod API should expose equivalent hook points at the same positions.

### PROC-ACTIVATION
A room is processed next tick only if activated (`reference/engine/src/main.js:42-51`). In this area: any object with `user` other than `"3"` (and not `userNotActive`, not flag/controller/stronghold) activates the room (processor.js:119-121); tombstones (135-137), drops (128-130), portals (138-140) too. **GOTCHA**: Source Keepers (`user "3"`) and lairs do not; an SK room with nothing else goes dormant, so keepers' `ageTime` keeps running in game time while their room is not processed; when the room wakes, `T ≥ ageTime − 1` kills them immediately (CREEP-AGE). Rooms are also activated by `saveUserIntents` and by the global stage for `interRoom` arrivals.

### PROC-HISTORY-SAY
History snapshot of creeps/PCs drops `actionLog.say` unless `isPublic` (processor.js:431-439).

---

## 3. Creep intent dispatch

### CREEP-INTENT-ORDER — **GOTCHA**
`reference/engine/src/processor/intents/creeps/intents.js:15-17,25-31` executes, for one creep, in this fixed order (independent of call order in player code):
`drop, transfer, withdraw, pickup, heal, rangedHeal, dismantle, attack, harvest, move, repair, build, rangedMassAttack, rangedAttack, say, suicide, claimController, upgradeController, reserveController, attackController, generateSafeMode, signController, pull`.
- Each handler mutates the live store, so later handlers see earlier effects: `drop` X then `transfer` X can fail the `store ≥ amount` re-check; `harvest` energy is available to `repair/build/upgradeController` in the same tick; `withdraw` before `pickup` shares free capacity. The docs' claim that each CARRY action sees start-of-tick energy (`reference/docs/source/simultaneous-actions.md:48`) is API-side only.
- `move` only registers a tentative destination (MOVE-ADD); positions do not change until the tick loop, so every range check in every intent uses start-of-tick positions.
- `suicide` deletes the creep, but handlers after it in this list (`claimController … pull`) still run on the detached object; no tick runs for it afterwards.
- Only the creep's **own** intents are ordered here; across creeps, PROC-USER-ORDER/PROC-OBJECT-ORDER apply (all intents of creep A run before any of creep B).

### CREEP-INTENT-PRIORITY
`checkPriorities(intents, name) = intents[name] && !(any i in priorities[name] with intents[i] truthy)` (intents.js:21-23). Blocking tables (intents.js:3-13):

| intent | skipped if any of these intents is present |
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

**GOTCHA**: blocking is by *presence* of the higher-priority intent, not its success (a `heal` on an out-of-range target still cancels `harvest`). Note `rangedHeal` blocks `attackController` etc. even though it is itself blocked by `heal`. Not in any chain (always run if present): `drop, transfer, withdraw, pickup, heal, move, say, suicide, claimController, upgradeController, reserveController, generateSafeMode, signController, pull`. So e.g. `build` + `upgradeController` both execute (energy consumed sequentially). The docs' "conflict only when not enough energy" (`simultaneous-actions.md:29-32`) is not implemented.

### CREEP-GUARDS
Every creep handler returns early on `object.spawning`. Handlers that also require `object.type == 'creep'` (so a PC cannot use them): attack, rangedAttack, rangedMassAttack, heal, rangedHeal, dismantle, harvest, build, repair, upgradeController, claimController, reserveController, attackController, signController, say, suicide, pull. No type check: move, transfer, withdraw, pickup, drop (shared with PCs) and generateSafeMode (the PC dispatcher never calls it).
**GOTCHA (general)**: the server does not re-check body parts for most actions (no `ERR_NO_BODYPART` server-side); a part-less action usually computes power 0 and either no-ops or writes a zero-effect result (noted per intent).

---

## 4. Shared helpers

### BODY-EFFECTIVENESS
`calcBodyEffectiveness(body, type, method, base, withoutOldHits)` = Σ over parts with `type` and (`hits > 0` or (`!withoutOldHits` and `_oldHits`)) of `base × (BOOSTS[type][part.boost][method] if defined else 1)` (`reference/engine/src/utils.js:623-636`). Boost table: `reference/common/lib/constants.js:617-731`.

### BODY-OLDHITS
`_recalc-body` sets `part._oldHits` (`_recalc-body.js:11`) but every caller immediately persists the body with `bulk.update(target, {body})`, whose `removeHidden` deletes `_oldHits` from the live parts (PROC-BULK; callers: `creeps/tick.js:139-149`, `labs/boost-creep.js:58-60`, `labs/unboost-creep.js:29-30`, `spawns/renew-creep.js:49-52`). No read of `_oldHits` can happen in between. **chitin: treat `_oldHits` as nonexistent; "alive" = `hits > 0`.**

### BODY-WORK-BOOST-BONUS — **GOTCHA**
Used by build, repair, upgradeController:
```
boosted = [ for p in body (ALL parts, alive or dead):
              (p.type==WORK && p.boost && BOOSTS.work[p.boost][method] > 0) ? bonus(p) : 0 ]
sort boosted descending; keep first N (N = the energy-limited effect, see each intent)
effect = floor(baseEffect + Σ kept)
```
`bonus = (mult−1)×BUILD_POWER` (build), `(mult−1)×REPAIR_POWER` (repair), `mult−1` (upgrade). Dead boosted WORK parts contribute. N is the *effect amount*, not a part count, so for build/repair N ≥ number of parts in practice and every boosted WORK part (dead included) adds its bonus; with very little energy (N small) only the top N bonuses count (e.g. build with 1 energy and one XLH2O part → progress `floor(1 + 5) = 6`).

### BODY-RECALC
`_recalc-body(creep)` (`_recalc-body.js:6-23`): iterate parts from **last to first**, assign `min(100, remaining)` and subtract 100 (floor at 0) → damage is taken by the **first** parts in the body array. Then `storeCapacity = calcBodyEffectiveness(body, CARRY, 'capacity', CARRY_CAPACITY, true)` unless `noCapacityRecalc`.

### BODY-DROP-OVERFLOW
`_drop-resources-without-space` (`_drop-resources-without-space.js:6-17`): for each `rt` in `RESOURCES_ALL` order (energy, power, H, O, U, K, L, Z, X, G, … — `reference/common/lib/constants.js:1028`): recompute total; stop when `total ≤ storeCapacity`; if `store[rt]` → CREEP-DROP of `min(store[rt], total − capacity)`. Energy is shed first.

### RES-CREATE-DROP
`_create-energy(x, y, room, amount, rt)` (`reference/engine/src/processor/intents/_create-energy.js`): `amount = Math.round(amount)`; ≤0 → nothing (14-18). If a `container` with `hits > 0` is on the tile: move `min(amount, storeCapacity − total)` into it (20-31). Remainder: if a pile `{type:'energy', resourceType: rt}` exists on the tile, add to it (35-40); else insert a new pile `{type:'energy', x, y, room, [rt]: amount, resourceType: rt}` with temporary id `"" + insertCounter` and add it to `roomObjects` (41-51). **GOTCHA**: dropping (or overflow-dropping) on a container tile fills the container first.

---

## 5. Creep intents

Unless stated: "range" uses start-of-tick positions; no event/actionLog on any early return.

### CREEP-MOVE
`reference/engine/src/processor/intents/creeps/move.js:7-45`
1. `spawning` → no-op. Set `object._oldFatigue = object.fatigue` (13) (used by MOVE-CANMOVE).
2. `d = offsets[direction]` if `direction` truthy (`reference/engine/src/utils.js:3,101-112`; directions 1..8 = TOP, TOP_RIGHT, RIGHT, BOTTOM_RIGHT, BOTTOM, BOTTOM_LEFT, LEFT, TOP_LEFT → `[0,-1],[1,-1],[1,0],[1,1],[0,1],[-1,1],[-1,0],[-1,-1]`). Invalid numbers → `undefined` (logs) → no-op.
3. If `id` truthy and `roomObjects[id]` is a `creep` at `dist == 1` → `d = [target.x−x, target.y−y]` (overrides direction) (19-24). Power creeps cannot be followed. This is the "move toward puller" form; the API lets it bypass fatigue/MOVE checks (`reference/engine/src/game/creeps.js:135-142`).
4. No `d` → no-op. Destination out of 0..49 → no-op (32-34).
5. MOVE-PREFILTER, then MOVE-ADD.

### MOVE-PREFILTER
At intent time, the move is dropped (creep stays, and becomes a stationary obstacle for others) if any object on the destination tile is (36-41): a non-creep/non-PC type in `OBSTACLE_OBJECT_TYPES` (`spawn, source, mineral, deposit, controller, constructedWall, extension, link, storage, tower, observer, powerSpawn, powerBank, lab, terminal, nuker, factory, invaderCore` — `reference/common/lib/constants.js:85`); a rampart with `!isPublic && user != mover.user`; for a power creep, a portal with `destination.shard`. Terrain and construction sites are **not** checked here (MOVE-OBSTACLE does).

### CREEP-PULL
`reference/engine/src/processor/intents/creeps/pull.js:7-22`: requires `type=='creep'`, not spawning; target exists, is a `creep`, not spawning, range ≤ 1; then MOVE-PULL-LINK. No MOVE-part or self check server-side (self is rejected by the cycle check).

### MOVE-PULL-LINK
`addPulling(puller, target)` (`movement.js:69-75`): rejected if `target` is the puller or following `target._pull` links reaches the puller (cycle). Else `puller._pull = target._id; target._pulled = puller._id`. One `pull` intent per creep per tick (API overwrite), but several pullers may target the same creep: the last processed wins `target._pulled`.

### CREEP-HARVEST-SOURCE
`harvest.js:6-65`. Order: type/spawning; target exists; range ≤ 1; `target.type=='source'`:
1. `!target.energy` → no-op.
2. Controller present and (`controller.user && controller.user != me` or `controller.reservation && reservation.user != me`) → no-op. (Safe mode does not block harvesting.)
3. `p = calcBodyEffectiveness(body, WORK, 'harvest', HARVEST_POWER=2)` (UO×3, UHO2×5, XUHO2×7). `p == 0` → no-op.
4. `amount = min(source.energy, p)`; `source.energy −= amount`; `store.energy += amount`; `source.invaderHarvested += amount` (counts **all** users; drives NPC-INVADER-SPAWN) (39-48).
5. If total store > `storeCapacity` → CREEP-DROP energy `min(store.energy, total − cap)` (50-57).
6. `actionLog.harvest = {x,y}`; stats `energyHarvested += amount` (U); `EVENT_HARVEST {targetId, amount}` (59-63).
Several creeps on one source in one tick share the decreasing `energy` in processing order.

### CREEP-HARVEST-MINERAL
`harvest.js:67-112`. After range: `!mineralAmount` → no-op; an `extractor` on the mineral tile must exist; `extractor.user` must be empty or me; extractor must be active (`checkStructureAgainstController`, `reference/engine/src/utils.js:456-507`); `extractor.cooldown` (DB value) must be 0; `p = WORK × HARVEST_MINERAL_POWER(1) × boost`; `p == 0` → no-op. `amount = min(mineralAmount, p)`; mineral and store updated; overflow drop of the mineral type; `actionLog.harvest`; `extractor._cooldown = EXTRACTOR_COOLDOWN (5)`; `EVENT_HARVEST {targetId, amount: p}` (**GOTCHA**: event reports unclamped `p`). No controller-ownership check, no stats. **PERF**: `checkStructureAgainstController` scans all room objects per call; cache "active" flags per structure per tick.
**GOTCHA**: the extractor cooldown is only written in the extractor's tick (`reference/engine/src/processor/intents/extractors/tick.js:21-25`), so **every** adjacent creep harvests the mineral in the same tick while the stored cooldown is 0.

### CREEP-HARVEST-DEPOSIT
`harvest.js:114-139`. After range: `cooldownTime > T` → no-op. `amount = WORK × HARVEST_DEPOSIT_POWER(1) × boost` (may be 0 — still proceeds); `store[depositType] += amount`; overflow drop; `actionLog.harvest`; `harvested += amount` (live `bulk.inc`); `cooldown = ceil(DEPOSIT_EXHAUST_MULTIPLY(0.001) × harvested^DEPOSIT_EXHAUST_POW(1.2))`; if `> 1` → `_cooldown = cooldown` (deposit tick writes `cooldownTime = T + _cooldown`, `deposits/tick.js:7-11`); `decayTime = T + DEPOSIT_DECAY_TIME (50000)`. No event, no stats. All adjacent creeps harvest in the same tick; the last one's cumulative `harvested` sets the cooldown.

### CREEP-ATTACK
`attack.js:6-45`. Order: type; spawning; target exists and `!= self`; range ≤ 1; target is a spawning creep → no-op; `!target.hits` → no-op; room in someone else's safe mode (`controller.user != me && safeMode > T`) → no-op; DMG-RAMPART-REDIRECT; `power = calcBodyEffectiveness(body, ATTACK, 'attack', ATTACK_POWER=30)`; DMG-QUEUE with type `EVENT_ATTACK_TYPE_MELEE`. Any object with `hits` is a valid target (own structures/creeps included). Power 0 still calls `_damage` with 0 (event emitted, hit-back still possible).

### CREEP-RANGED-ATTACK
`rangedAttack.js:6-43`: as CREEP-ATTACK with range ≤ 3, `RANGED_ATTACK_POWER=10`, method `rangedAttack`, type `EVENT_ATTACK_TYPE_RANGED`.

### CREEP-RANGED-MASS-ATTACK
`rangedMassAttack.js:6-59`:
1. type; spawning; `p = calcBodyEffectiveness(body, RANGED_ATTACK, 'rangedMassAttack', 10)`; `p == 0` → no-op; other user's safe mode → no-op.
2. Targets (room-object order): objects with a defined `user` (or `type=='powerBank'`) and `user != me` within range 3 (26-30). Includes foreign structures, ramparts, creeps, PCs, NPCs, construction sites; excludes neutral objects except power banks.
3. For each target: skip if a rampart (**any owner**) is on its tile and the target is not itself a rampart (38-40); skip `!hits`; skip spawning creeps; skip if it has an effect with `endTime ≥ T` and (`power == PWR_FORTIFY` or `effect == EFFECT_INVULNERABILITY`) (47-49 — note `≥`, unlike `_damage`'s `>`).
4. `dmg = Math.round(p × {0:1, 1:1, 2:0.4, 3:0.1}[dist])` → DMG-QUEUE type `EVENT_ATTACK_TYPE_RANGED_MASS` (51-55).
5. `actionLog.rangedMassAttack = {}` (58) even with zero targets.
**GOTCHA**: a hostile creep standing on *your* (or an ally's / public) rampart is immune to your RMA. Power banks reflect RMA damage (DMG-HITBACK-POWERBANK).
**PERF**: the original filters all room objects, then scans again per target for a rampart (O(N²) worst case). Iterate the 7×7 window of a tile index instead — but targets must still be processed in room-object order (observable through event-log order and the order in which structures reach 0 hits and are destroyed).

### CREEP-HEAL / CREEP-RANGED-HEAL
`heal.js:6-34`, `rangedHeal.js:6-34`. Order: type; spawning; target exists, is `creep` or `powerCreep`, not spawning; range ≤ 1 (heal) / ≤ 3 (rangedHeal); other user's safe mode → no-op. `amount = calcBodyEffectiveness(body, HEAL, 'heal'|'rangedHeal', HEAL_POWER=12 | RANGED_HEAL_POWER=4)`; `target._healToApply += amount`; `actionLog.heal|rangedHeal = target pos`; `target.actionLog.healed = my pos`; `EVENT_HEAL {targetId, amount, healType: MELEE(1)|RANGED(2)}`. No check of target damage, hostility, or `amount > 0`; self-heal allowed; rangedHeal at range 1 allowed.

### CREEP-DISMANTLE
`dismantle.js:6-54`. Order: type; spawning; target exists and `CONSTRUCTION_COST[target.type]` truthy (buildable structure types only); range ≤ 1; other user's safe mode → no-op; DMG-RAMPART-REDIRECT. `power = calcBodyEffectiveness(body, WORK, 'dismantle', DISMANTLE_POWER=50)` (ZH×2, ZH2O×3, XZH2O×4); `amount = min(power, target.hits)`; `gain = floor(amount × DISMANTLE_COST(0.005))`; `gain = 0` if target has an effect with `endTime ≥ T` and power `PWR_SHIELD`/`PWR_FORTIFY` or `EFFECT_INVULNERABILITY` (37-40). If `amount` truthy: `store.energy += gain` (**GOTCHA**: `NaN` if `store.energy` is absent — cron/summoned invaders have `store: {}`), overflow drop of `used − capacity` energy, then DMG-QUEUE type `EVENT_ATTACK_TYPE_DISMANTLE` (sets `actionLog.attack`). Own structures can be dismantled. Fortified walls/ramparts take no damage (`_damage` returns early) but the attempt still happened.

### CREEP-BUILD
`build.js:9-107`. Order:
1. type; spawning; `!store || store.energy <= 0` → no-op.
2. target is a `constructionSite` with `CONSTRUCTION_COST[structureType]`; range ≤ 3.
3. Scan the site tile (29-44): any object with `type == structureType` → no-op.
4. If `structureType ∈ OBSTACLE_OBJECT_TYPES`: any non-creep object of an obstacle type on the tile (includes power creeps) → no-op; creeps on the tile → no-op, except under my own active safe mode only **my** creeps block (55-58).
5. Terrain wall and type not `extractor`/`road` → no-op.
6. `power = (#WORK parts alive) × BUILD_POWER(5)`; `remaining = progressTotal − progress`; `e = min(power, remaining, store.energy)`; BODY-WORK-BOOST-BONUS with N = `e`, bonus `(build−1)×5` (LH 1.5, LH2O 1.8, XLH2O 2); `boosted = min(floor(e + Σ), remaining)`.
7. `progress += boosted`; `store.energy −= e`; stats `energyConstruction += e`; `actionLog.build`; `EVENT_BUILD {targetId, amount: boosted, structureType, x, y, incomplete}`.
8. `progress < progressTotal` → persist progress; else CREEP-BUILD-COMPLETE.
No owner check (any user's site), no safe-mode check. With 0 WORK the intent still emits a 0-amount event/actionLog.

### CREEP-BUILD-COMPLETE
`build.js:108-293`: remove the site; insert the new structure (`notifyWhenAttacked: true`, `user = site.user` for owned types) with per-type initial fields (spawn: name, `SPAWN_HITS`, energy cap; extension: capacity 0 (set later by RCL logic); rampart: `hits = RAMPART_HITS`, `hitsMax = RAMPART_HITS_MAX[rcl]` if the **builder** owns the controller else 0, `nextDecayTime = T + RAMPART_DECAY_TIME`; road: `ROAD_HITS × 5` on swamp, `× 150` on wall terrain, `nextDecayTime = T + ROAD_DECAY_TIME`; container: `nextDecayTime = T + CONTAINER_DECAY_TIME`; nuker `cooldownTime = T + NUKER_COOLDOWN`; etc. — full table lines 119-285). The new object is added to `roomObjects` as `createdStructureN` (global counter, never reset) **without `_id`**, and the site is deleted from `roomObjects`. Consequences: later intents targeting the site id no-op (energy not spent); the new structure immediately blocks movement this tick (MOVE-OBSTACLE); writes made to it during this tick's structure tick target id `undefined` and are lost (structure spec).

### CREEP-REPAIR
`repair.js:6-62`. Order: type; spawning; `store.energy <= 0`; target exists with truthy `hitsMax`, `CONSTRUCTION_COST[type]`, `hits < hitsMax`; range ≤ 3.
`power = (#WORK alive) × REPAIR_POWER(100)`; `e = min(power, store.energy / REPAIR_COST(0.01), hitsMax − hits)`; `cost = min(store.energy, ceil(e × 0.01))`; BODY-WORK-BOOST-BONUS with N = `e`, bonus `(repair−1)×100`; `boosted = min(floor(e + Σ), hitsMax − hits)`; `boosted == 0` → no-op. `hits += boosted` (clamp), `store.energy −= cost`, stats `energyConstruction += cost`, `actionLog.repair`, `EVENT_REPAIR {targetId, amount: boosted, energySpent: cost}`. No owner/safe-mode check.

### CREEP-UPGRADE-CONTROLLER
`upgradeController.js:7-109`. Order: type/spawning/`store.energy <= 0`; `users[me].shardAccess === false` → no-op; target is a `controller`; range ≤ 3; `level == 0 || user != me` → no-op; `upgradeBlocked > T` → no-op.
- `target._upgraded` (per-tick accumulator, starts 0).
- `power = (#WORK alive) × UPGRADE_CONTROLLER_POWER(1)`; `e = min(power, store.energy)`.
- At level 8: `limit = CONTROLLER_MAX_UPGRADE_PER_TICK(15) + (PWR_OPERATE_CONTROLLER effect with endTime ≥ T ? effect[level−1] : 0)`; `_upgraded ≥ limit` → no-op; `e = min(e, limit − _upgraded)`.
- BODY-WORK-BOOST-BONUS with N = `e`, bonus `upgradeController − 1` (GH 0.5, GH2O 0.8, XGH2O 1); `boosted = floor(e + Σ)`.
- Level < 8: `next = CONTROLLER_LEVELS[level]` (1000 on PTR; 4 for tutorial level 1). If `progress + boosted ≥ next` **and** `downgradeTime + CONTROLLER_DOWNGRADE_RESTORE(100) ≥ T + CONTROLLER_DOWNGRADE[level]`: `progress += boosted − next`, `level++`, `downgradeTime = T + CONTROLLER_DOWNGRADE[newLevel]/2`, notification (U), `progress = 0` if new level is 8, `safeModeAvailable += 1`. Otherwise `progress += boosted` (may exceed `next`; at most one level per intent). Level 8: progress untouched.
- GCL `$inc` by `boosted` (U); `_upgraded += e`; stats `energyControl += boosted`; `store.energy −= e`; `actionLog.upgradeController`; persist level/progress/safeModeAvailable/downgradeTime; `EVENT_UPGRADE_CONTROLLER {amount: boosted, energySpent: e}`.
Downgrade-timer restore from `_upgraded` happens in the controller tick (`reference/engine/src/processor/intents/controllers/tick.js:38-42`).

### CREEP-CLAIM-CONTROLLER
`claimController.js:6-56`. Order: type; spawning; target `controller`; range ≤ 1; `bindUser && bindUser != me` → no-op; `level > 0` → no-op; no alive CLAIM part → no-op; `reservation && reservation.user != me` → no-op; `users[me].shardAccess === false` → no-op; `user.gcl < GCL_MULTIPLY × (ownedRooms)^GCL_POW` (`calcNeededGcl(rooms+1)`, `reference/engine/src/utils.js:661-663`; 1e6 × n^2.4) → no-op. Effect: controller `{user: me, level: 1, progress: 0, downgradeTime: null, reservation: null}`; add room to user (U). No event, no actionLog. Because the live controller now has `level 1`, a second claimer later in the same tick fails; the same creep's later `upgradeController` this tick succeeds.

### CREEP-RESERVE-CONTROLLER
`reserveController.js:6-53`. Order: type; spawning; shardAccess false → no-op; target `controller`; range ≤ 1; `target.user` or reservation by another user → no-op; `effect = (#CLAIM alive) × CONTROLLER_RESERVE(1)`; 0 → no-op. If no reservation: `reservation = {user: me, endTime: T + 1}` (live object). If `endTime + effect > T + CONTROLLER_RESERVE_MAX(5000)` → no-op (no partial). Else `endTime += effect`, persist, `actionLog.reserveController`, `EVENT_RESERVE_CONTROLLER {amount: effect}`. Multiple reservers in one tick stack.

### CREEP-ATTACK-CONTROLLER
`attackController.js:6-53`. Order: type; spawning; target `controller`; range ≤ 1; neither `user` nor `reservation` → no-op; (other user's safe mode) **or** `roomController.upgradeBlocked > T` → no-op (25-28); active `EFFECT_INVULNERABILITY` (endTime > T) → no-op.
- Reserved: `effect = floor(#CLAIM alive × CONTROLLER_RESERVE)`; 0 → no-op; `reservation.endTime −= effect`.
- Owned: `effect = floor(#CLAIM alive × CONTROLLER_CLAIM_DOWNGRADE(300))`; 0 → no-op; `downgradeTime −= effect`; `_upgradeBlocked = T + CONTROLLER_ATTACK_BLOCKED_UPGRADE(1000)` (written by controller tick).
- `actionLog.attack = target pos` (not a dedicated field); `EVENT_ATTACK_CONTROLLER` (no data).
**GOTCHA**: `upgradeBlocked` is only written in the controller tick, so any number of creeps can attackController in the same tick and all reductions stack; the 1000-tick lockout then blocks further attacks and upgrades.

### CREEP-SIGN-CONTROLLER
`signController.js:6-28`: type; spawning; target `controller`; range ≤ 1; `sign = text ? {user, text, time: T, datetime: Date.now()} : null` (text ≤ 100 chars via sanitizer; empty string unsigns). Wall-clock `datetime` is not deterministic.

### CREEP-GENERATE-SAFE-MODE
`generateSafeMode.js:6-25`: spawning; target `controller`; range ≤ 1; `store.G >= SAFE_MODE_COST(1000)`; → `safeModeAvailable += 1`, `store.G −= 1000`. No owner check (any controller), no type check, no event.

### CREEP-TRANSFER
`transfer.js:6-60`. Order:
1. `resourceType ∈ RESOURCES_ALL`.
2. not spawning; has `store`; `store[rt] >= amount` (NaN fails); `amount >= 0`.
3. target exists; not a spawning creep; range ≤ 1.
4. `cap = capacityForResource(target, rt)` = `storeCapacityResource[rt]` if truthy else `max(0, (storeCapacity||0) − Σ storeCapacityResource)` (`reference/engine/src/utils.js:671-675`); 0 → no-op (also covers targets without a store).
5. `stored = target.storeCapacityResource ? target.store[rt] : total(target.store)`; `stored >= cap` → no-op; clip `amount` to `cap − stored`; `amount == 0` → no-op.
6. `target.store[rt] += amount`, `self.store[rt] −= amount`. Lab + non-energy + lab not yet holding that mineral → `storeCapacityResource[rt] = LAB_MINERAL_CAPACITY`, `storeCapacity = null` (51-56).
7. `EVENT_TRANSFER {objectId: self, targetId, resourceType, amount}`.
No owner/safe-mode/rampart checks; any object with a store (structures, creeps, PCs) accepts.

### CREEP-WITHDRAW
`withdraw.js:6-61`. Order:
1. `rt ∈ RESOURCES_ALL`.
2. `free = storeCapacity − total(store)`; `amount = min(intent.amount, free)` (computed first).
3. spawning, `!storeCapacity`, or `amount < 0` → no-op (amount 0 passes).
4. other user's safe mode → no-op.
5. target exists.
6. `target.user != me` and a rampart on the target tile with `user != me && !isPublic` → no-op (25-27).
7. range ≤ 1.
8. target is `nuker` or `powerBank` → no-op.
9. terminal with `PWR_DISRUPT_TERMINAL` effect `endTime > T` → no-op.
10. clip `amount` to `target.store[rt]` (if that is undefined the clip does not happen and the result is NaN — API prevents it).
11. `self.store[rt] += amount`, `target.store[rt] −= amount`; lab with that mineral now 0 → reset lab capacities (52-57).
12. `EVENT_TRANSFER {objectId: target, targetId: self, resourceType, amount}` (note reversed ids), emitted even for amount 0.
No ownership check (hostile unprotected structures can be emptied — matches the API rule). **GOTCHA**: a target without `store` throws a TypeError, aborting the whole room's tick in the original (API prevents; chitin must not crash — treat as no-op).

### CREEP-PICKUP
`pickup.js:6-39`: `store ||= {}`; spawning or `total >= storeCapacity` → no-op; target `type=='energy'`; range ≤ 1; `rt = target.resourceType || 'energy'`; `amount = min(capacity − total, target[rt])`; subtract from pile; pile 0 → removed from DB and `roomObjects` (later pickups of it no-op); add to store. No event, no safe-mode check.

### CREEP-DROP
`drop.js:6-23`: `rt ∈ RESOURCES_ALL`; spawning, no store, or `store[rt] < amount` → no-op; if `amount > 0`: `store[rt] −= amount`, RES-CREATE-DROP at own tile; store written regardless. No event. Used internally for overflow (harvest, dismantle, GENERATE_OPS, BODY-DROP-OVERFLOW).

### CREEP-SAY
`say.js:6-21`: type; spawning; message must be a string → `actionLog.say = {message: message.substring(0,10), isPublic}`. Public says also become `tombstone.creepSaying` if the creep dies this tick, and are kept in history.

### CREEP-SUICIDE
`suicide.js:6-15`: type; spawning; CREEP-DIE with `dropRate = 0` for user `"2"` (invaders), default otherwise; `violent = false`.

---

## 6. Damage and healing

### DMG-RAMPART-REDIRECT — **GOTCHA**
attack, rangedAttack, dismantle (and towers): after validating the original target, if **any** rampart (any owner, public or not) stands on the target's tile, the rampart becomes the target (`attack.js:33-36`, `rangedAttack.js:33-36`, `dismantle.js:27-30`). Attacking an enemy standing on your own rampart damages your rampart. The `!target.hits` check is done on the original target. **PERF**: one full-room scan per attack in the original; keep a per-tile rampart lookup.

### DMG-QUEUE
`_damage(attacker, target, dmg, type)` (`reference/engine/src/processor/intents/_damage.js`):
1. `!target._id || !target.hits` → return (6-12).
2. `target.type=='creep'`: if `type == MELEE` and **no rampart on the attacker's tile** → `hitBack = calcBodyEffectiveness(target.body, ATTACK, 'attack', 30)`; `target._damageToApply += dmg` (16-21).
3. `powerCreep`: `_damageToApply += dmg` (22-24).
4. Structures: unless `type == NUKE`, a `constructedWall`/`rampart` with an effect (`power == PWR_FORTIFY` or `effect == EFFECT_INVULNERABILITY`) and `endTime > T` → **return** (nothing else: no event, no actionLog) (26-31); else `hits −= dmg` immediately (32).
5. `powerBank` → `hitBack = dmg × POWER_BANK_HIT_BACK (0.5)` (any attack type) (35-37).
6. Attacker owns the controller and room is in safe mode → `hitBack = 0` (39-41).
7. Newbie wall (`constructedWall` with `decayTime`) → remove all newbie walls (43-45); else structure `hits ≤ 0` → `_destroy` + `EVENT_OBJECT_DESTROYED`; else persist hits (46-58).
8. Attacker is a creep: MELEE/DISMANTLE → `actionLog.attack = target pos`; RANGED → `actionLog.rangedAttack` (RANGED_MASS sets nothing here) (59-72). `target.actionLog.attacked = attacker pos` if target has an actionLog.
9. DMG-NOTIFY-PVP (77-84).
10. `hitBack` → `attacker._damageToApply += hitBack`; `attacker.actionLog.attacked = target pos`; `EVENT_ATTACK {objectId: target, targetId: attacker, damage: hitBack, attackType: HIT_BACK(5)}` (86-91).
11. `EVENT_ATTACK {objectId: attacker, targetId: target, damage: dmg, attackType: type}` (93).
**GOTCHA**: creep targets keep start-of-tick `hits` during the whole intent phase → overkill is normal; every attacker's damage (and hit-back) is applied even if the target is already "dead".

### DMG-HITBACK-MELEE
Defender's ATTACK power (alive parts, boosted) at start of tick; only when the defender is a creep (not redirected to a rampart, not a PC) and the attacker is not standing on a rampart; zero if the attacker is the safe-mode owner. Hit-back is queued like normal damage on the attacker.

### DMG-NOTIFY-PVP
For non-NPC attackers: `target.notifyWhenAttacked` → `sendAttackingNotification` to `target.user` or (ownerless target) the controller owner (`reference/engine/src/utils.js:438-454`) (U). If attacker and target users differ and neither is NPC → `roomInfo.lastPvpTime = T`.

### DMG-TOUGH
Applied once per tick to the summed `_damageToApply` in the creep's tick (`reference/engine/src/processor/intents/creeps/tick.js:7-29`):
```
if any part boosted:
  reduce = 0; eff = damage
  for part in body (index 0 → end):
    if eff <= 0: break
    r = BOOSTS[part.type][part.boost].damage if defined else 1   // GO .7, GHO2 .5, XGHO2 .3
    h = part.hits / r
    reduce += min(h, eff) × (1 − r);  eff −= min(h, eff)
  damage −= Math.round(reduce)       // JS round: .5 → up
hits −= damage
```
Uses current part hits (start of tick, front-to-back = the order parts lose hits). Unboosted or non-TOUGH parts consume damage at ratio 1.

### HEAL-QUEUE
Creep heal/rangedHeal and tower heal add to `target._healToApply` (`heal.js:28`, `rangedHeal.js:28`, `reference/engine/src/processor/intents/towers/heal.js:43`).

### CREEP-HITS-APPLY
In creep tick (`tick.js:118-150`): `old = hits`; apply DMG-TOUGH; `hits += _healToApply`; clamp to `hitsMax`; `hits ≤ 0` → CREEP-DIE (violent); else if changed → BODY-RECALC, and if `hits < old` → BODY-DROP-OVERFLOW; persist `hits, body, storeCapacity`. Damage and heal of the same tick **net** (a creep taking 150 with 100 hits survives if healed ≥ 51); heal never pre-empts TOUGH reduction (reduction computed on gross damage).

---

## 7. Movement

### MOVE-INIT
`movement.init` clears `matrix` (tile → movers), `objects` (moverId → resolved destination or null), `affectedCnt` (tile → number of movers targeting it) (`movement.js:61-67`). Called once per room after NPC AI, before intents.

### MOVE-ADD
`add(obj, dx, dy)` (`movement.js:84-98`): destination clamped to 0..49 (move.js already rejects out-of-range); appended to `matrix["x,y"]` (insertion order of tiles and of movers preserved); `affectedCnt["x,y"]++`.

### MOVE-CANMOVE
`canMove(o) = o.type=='powerCreep' || o._pulled || (!o._oldFatigue && any alive MOVE part)` (`movement.js:11-14`). `_oldFatigue` = DB fatigue at intent time. A pulled creep ignores fatigue and MOVE parts.

### MOVE-CONFLICT
`check(safeModeUser)` first loop (`movement.js:107-150`), for each destination tile `K=(x,y)` in first-registration order:
- One mover → winner.
- Several → for each candidate `o` compute
  - `moves` = PC ? 0 : `calcBodyEffectiveness(body, MOVE, 'fatigue', 1)` (alive MOVE × ZO 2 / ZHO2 3 / XZHO2 4)
  - `weight` = PC ? 0 : #parts with type ∉ {MOVE, CARRY} (**dead parts included**) + `carryWeight(o)` (MOVE-FATIGUE-COST); `weight ||= 1`
  - `rate1` = `affectedCnt[o's current tile]` (how many movers want o's tile); `= 100` if any mover registered for o's current tile is currently standing on `K` (a swap partner)
  - `rate2` = `o._pulled ? 1 : 0`; `rate3` = `o._pull ? 1 : 0`; `rate4 = moves / weight`
  - sort descending by `(rate1, rate2, rate3, rate4)` lexicographically; winner = first.
- `objects[winner] = {x,y}`; losers get no entry (`undefined`) and stay put.
Rates use post-intent stores (carry weight after this tick's transfers/withdraws/harvests) and pre-validation pull links. Fatigue is **not** a rate: a fatigued or MOVE-less creep (only possible via move-by-creep intents) can win and then fail (MOVE-NO-REDISPATCH).

### MOVE-TIEBREAK — **GOTCHA**
Exact ties keep `matrix[K]` order (V8 `Array.prototype.sort` is stable; arrays ≤ 8 entries are stable even on old V8). So the mover whose `move` intent was processed first wins: NPCs before players (PROC-USER-ORDER), then player order, then object order (PROC-OBJECT-ORDER). **No `Math.random`** anywhere in creep processing or movement. (xxscreeps differs: power-sorted with `Math.random()` ties — `reference/xxscreeps/packages/xxscreeps/engine/processor/movement.ts:169`; do not copy.)

### MOVE-OBSTACLE
`checkObstacleAtXY(x, y, mover, safeModeUser)` (`movement.js:16-39`) — the destination is blocked if any object on it is:
- a creep/PC with no resolved move (`!objects[id]`: no intent, pre-filtered, conflict loser, or cancelled), **unless** MOVE-SAFEMODE-PASS applies;
- a non-creep object of an `OBSTACLE_OBJECT_TYPES` type;
- a rampart `!isPublic && user != mover.user`;
- the mover's **own** construction site of an obstacle structure type (foreign sites are walkable and get stomped);
- or terrain wall without a road on the tile (roads make wall tiles walkable — tunnels).
**PERF**: the original scans every room object per destination tile (O(tiles × N)); a per-tile object index makes this O(objects on tile).

### MOVE-SAFEMODE-PASS — **GOTCHA**
While the room is in safe mode, a stationary creep/PC is *not* an obstacle for a mover owned by the safe-mode owner if the stationary one belongs to another user (`movement.js:22`). Safe-mode owners can walk onto (and stack with) hostile or NPC creeps.

### MOVE-PULL-VALIDATE
Second loop (`movement.js:167-186`), for each surviving tile entry in order: if winner `o` has `_pulled` and the puller still exists, and (`puller._pull != o._id` or `K` != puller's current tile) → delete `puller._pull` and `o._pulled`. A pulled creep must step exactly into its puller's current tile; the puller must itself move away (else its tile is occupied → blocked).

### MOVE-CHAIN-CANCEL
Still in the second loop: if `!canMove(o)` or `checkObstacleAtXY(K, o)` → `removeFromMatrix(K)`: `objects[o] = null`, delete `matrix[K]`, then if some winner targets `o`'s current tile, remove it recursively (`movement.js:154-165,183-185`). Result: every creep queued behind a blocked creep stays too, regardless of iteration order (entries already visited are removed by the recursion; entries removed before being visited are skipped by `for…in`).

### MOVE-SWAP-CYCLE
Because a creep whose own move resolved is not an obstacle, swaps (A↔B), rotations (A→B→C→A) and follow-chains (A into B's tile while B moves on) succeed in one tick if every member's move survives.

### MOVE-NO-REDISPATCH — **GOTCHA**
If a tile's winner is later removed (obstacle, `canMove` false, chain), the tile stays empty this tick; the losers are not reconsidered.

### MOVE-EXECUTE
`execute(o)` is called at the start of `o`'s own tick (`movement.js:189-252`):
1. `m = objects[o]` falsy → return; `!canMove(o)` → return.
2. `cell` = objects on `m` (full scan).
3. Fatigue rate: 2; 10 if terrain swamp (or a legacy `swamp` object); 1 if a road is on the tile (road wins over swamp; also on wall tunnels).
4. Road on tile: `road.nextDecayTime −= ROAD_WEAROUT_POWER_CREEP (100)` for PCs, `−= ROAD_WEAROUT (1) × body.length` (all parts) for creeps.
5. MOVE-SITE-STOMP.
6. Creep fatigue `F = (#parts ∉ {MOVE,CARRY}, dead included + carryWeight(o)) × rate` (PCs: none).
7. If `m` is an edge tile and `o` is not on an edge → MOVE-EDGE-ZERO-FATIGUE: write `{x, y, fatigue: 0}`, `o._fatigue = 0`; else write `{x, y}` and (creeps) MOVE-FATIGUE-CHAIN `+F`.
Position writes are live (later ticks in the loop see the new position).
**PERF**: step 2 is a full-room scan per moving creep in the original; with a tile index it is O(1). Execution must stay at the mover's slot in the tick loop (order-observable), but the cost terms (`weight`, `carryWeight`) can be precomputed after the intent phase.

### MOVE-FATIGUE-COST
`carryWeight(o)` (`movement.js:41-59`): `left = Σ store`; for parts from **last to first**: stop when `left == 0`; skip non-CARRY and dead parts; `left −= min(left, CARRY_CAPACITY × capacityBoost)` (KH 2, KH2O 3, XKH2O 4); `weight++`. Empty CARRY parts weigh nothing; full ones (counted from the body's end) weigh 1 each; dead CARRY parts never weigh. **GOTCHA**: dead non-MOVE/non-CARRY parts still weigh 1 each; dead MOVE parts give no recovery.

### MOVE-FATIGUE-CHAIN
`_add-fatigue(o, d)` (`creeps/_add-fatigue.js:6-35`), `_fatigue` lazily initialised from `fatigue`:
- If `o._fatigue > 0 && d < 0` (recovery): first reduce own `_fatigue` by `min(_fatigue, −d)`, write `max(0,_fatigue)`; if nothing left of `d`, stop.
- Walk `o = roomObjects[o._pulled]` while it exists (to the train head).
- `head._fatigue += d`; write `max(0, head._fatigue)` if it differs from the live `fatigue`.
So a pulled creep's move cost is charged to the train head, and a pulled creep's spare recovery (after clearing its own fatigue) reduces the head's fatigue. `_fatigue` can go negative internally (not persisted). **GOTCHA**: when the head enters an exit tile its `_fatigue` is reset to 0 (MOVE-EDGE-ZERO-FATIGUE); cost added by followers whose tick ran earlier is erased, cost from followers ticking later is kept (tick-order dependent).

### MOVE-SITE-STOMP
Unless the room is in safe mode owned by someone other than the mover (`!controller || controller.user === mover.user || !(safeMode > T)`), the first construction site on `m` with `user != mover.user` is removed from the DB (not from `roomObjects`) and, if `progress > 1`, `floor(progress/2)` energy is created there (RES-CREATE-DROP; a container absorbs it) (`movement.js:224-233`). Applies to NPCs and PCs too.

### MOVE-EDGE-ZERO-FATIGUE
Moving from a non-edge tile onto x∈{0,49} or y∈{0,49} costs no fatigue and sets fatigue to 0 (even a pulled creep's own fatigue). Moving along the edge (edge→edge) costs normally.

### MOVE-TILE-BUSY
`isTileBusy(x,y)` = the final `matrix` has an entry for the tile (`movement.js:100-102`) — surviving destinations, whether or not that mover's tick already ran. Used by CREEP-BIRTH.

---

## 8. Creep tick

### CREEP-TICK-ORDER
`reference/engine/src/processor/intents/creeps/tick.js:31-151`, per creep at its slot in PROC-TICK-ORDER:
1. Spawning → CREEP-TICK-SPAWNING. Else:
   a. MOVE-EXECUTE (50).
   b. CREEP-EXIT (52-78).
   c. CREEP-PORTAL (80-85).
   d. CREEP-AGE (87-96).
   e. CREEP-ACTIONLOG-SAVE (98-100).
2. CREEP-FATIGUE-RECOVER (105-108) — runs for spawning creeps too.
3. `hits` NaN or ≤ 0 (before this tick's damage) → CREEP-DIE violent (110-112).
4. CREEP-USER-SUMMONED (114-116).
5. CREEP-HITS-APPLY (118-150).
Steps after a death keep running on the detached object (CREEP-DIE-DOUBLE).

### CREEP-TICK-SPAWNING
(`tick.js:37-48`) Find a `spawn` or `invaderCore` on the creep's tile. None → delete the creep (no tombstone, no event). Found and (`!spawn.spawning` or `spawn.spawning.name != creep.name`) → CREEP-BIRTH from the creep's side (recovery path). Normal births are driven by the spawn tick: `T ≥ spawning.spawnTime − 1` → CREEP-BIRTH; success clears `spawn.spawning`, failure sets `spawnTime = T + 1` (`reference/engine/src/processor/intents/spawns/tick.js:17-26`); same for invader cores (`invader-core/tick.js:29-41`). `PWR_DISRUPT_SPAWN` pushes `spawnTime` by 1 per tick instead (spawns/tick.js:13-15).
**GOTCHA (lifetime)**: if the spawn ticks before the creep, the creep is unspawned in memory and its own tick this same `T` sets `ageTime = T + 1500`; if the creep ticks first, `ageTime` is set next tick (`T + 1 + 1500`). One tick of lifetime depends on DB order.

### CREEP-BIRTH
`_born-creep(spawn, creep)` (`reference/engine/src/processor/intents/spawns/_born-creep.js:7-79`):
- `directions = spawn.spawning.directions || [1..8]`; `other = [1..8] − directions`.
- For each direction in order: tile `(sx+dx, sy+dy)` is occupied if any object there has an `OBSTACLE_OBJECT_TYPES` type (creeps and PCs included, at their **live** positions) or is a construction site of an obstacle type, or `isTileBusy`, or terrain wall without road. First free tile → `{x, y, spawning: false}`, return true.
- Remember the first hostile **creep** (`user != spawn.user`, NPCs included) found on an occupied tile while scanning `directions` → CREEP-SPAWN-STOMP.
- Otherwise return false.
**GOTCHA**: a neighbour that moves out this tick frees its tile only if its tick already ran.
**PERF**: up to 8 (+8 for stomp) full-room scans per birth in the original.

### CREEP-SPAWN-STOMP
If all allowed directions are occupied and a hostile creep was found: if any of the `other` directions is free (obstacle/`isTileBusy`/terrain wall — **roads not considered here**) → return false (wait). Else CREEP-DIE the hostile (violent, default dropRate → tombstone) and place the newborn on its tile, return true (`_born-creep.js:52-76`). Power creeps are never stomped.

### CREEP-EXIT
(`tick.js:52-78`) If the creep (after moving) is on an edge tile and `user ∉ {"2","3"}`: compute the neighbour room and mirrored coordinate, checking `x==0` (west, x→49), then `y==0` (north, y→49), then `x==49` (east, x→0), then `y==49` (south, y→0); write `interRoom = {room, x, y}`; `EVENT_EXIT {room, x, y}`. Happens **every tick** the creep ends on an exit tile, whether or not it moved — a creep idling on an exit bounces between rooms each tick. NPCs never leave their room.

### CREEP-PORTAL
(`tick.js:80-85`) If `ageTime` is set (fresh NPCs skipped) and a portal is on the creep's tile → `interRoom = portal.destination` (merged over any exit `interRoom`). No user filter (NPCs with `ageTime` use portals). **PERF**: full-room scan per creep per tick in the original; use the tile index.

### MOVE-INTERROOM
Global stage (`reference/engine/src/processor/global.js:33-43`): for every creep/PC with `interRoom` (`reference/driver/lib/index.js:432`): if the destination room is accessible (`status 'normal'` and open — 434) → activate it and write `{room, x, y, interRoom: null}`; else nothing (the creep stays on its edge tile and re-requests next tick). No obstacle or occupancy check at the arrival tile (creeps can stack after crossing); fatigue unchanged (already 0 if it just stepped onto the exit).

### CREEP-AGE
(`tick.js:87-96`) Non-tutorial creeps: if `!ageTime` → `ageTime = T + (any CLAIM part, dead or alive ? CREEP_CLAIM_LIFE_TIME 600 : CREEP_LIFE_TIME 1500)`. Then `T ≥ ageTime − 1` → CREEP-DIE (non-violent). The API shows `ticksToLive = ageTime − Game.time` (`reference/engine/src/game/creeps.js:77`): the creep acts in the tick where it shows 1 and is removed at the end of it. NPCs (lair keepers, cron invaders with only `ticksToLive` field) get `ageTime` on their first processed tick; stronghold creeps carry `ageTime = core.decayTime`.

### CREEP-FATIGUE-RECOVER
`moves = calcBodyEffectiveness(body, MOVE, 'fatigue', 1)`; if `> 0` → MOVE-FATIGUE-CHAIN with `−2 × moves` (`tick.js:105-108`).

### CREEP-ACTIONLOG-SAVE
Persist `actionLog` only if not deep-equal to the previous tick's (`tick.js:98-100`). Spawning creeps never persist it.

### CREEP-USER-SUMMONED
A creep with `userSummoned` dies (non-violent) if any creep exists in the room whose user is neither `"2"` nor the controller owner (`tick.js:114-116`; throws if the room has no controller).

---

## 9. Death

### CREEP-DIE
`_die(creep, dropRate = CREEP_CORPSE_RATE (0.2), violent, scope, attackType)` (`reference/engine/src/processor/intents/creeps/_die.js`):
1. Remove from DB and `roomObjects` (12-13).
2. `decay = body.length × TOMBSTONE_DECAY_PER_PART (5)`, or `creep.tombstoneDecay` (15-18).
3. Unless `attackType == EVENT_ATTACK_TYPE_NUKE`: build tombstone `{type, room, x, y, user, deathTime: T, decayTime: T + decay, creepId, creepName, creepTicksToLive: ageTime − T, creepBody: [types], creepSaying: public say of this tick or undefined, store: {}}` (20-35); container = first `container` on the tile.
4. CREEP-CORPSE-RESOURCES if `dropRate > 0 && !userSummoned && !strongholdId`.
5. Insert tombstone (bulk only; not added to `roomObjects` — two deaths on one tile make two tombstones).
6. `EVENT_OBJECT_DESTROYED {objectId, data: {type: 'creep'}}` (97).
7. `violent` and user not NPC → stats `creepsLost += body.length` (U) (99-101).

### CREEP-CORPSE-RESOURCES
(`_die.js:39-92`)
```
lifeTime = any CLAIM part ? 600 : 1500
lifeRate = dropRate × _ticksToLive / lifeTime      // _ticksToLive from PROC-PREPASS (start of tick)
res = {energy: 0}
for part in body (all parts, dead included, body order):
  if part.boost: res[boost] += LAB_BOOST_MINERAL(30) × lifeRate; res.energy += LAB_BOOST_ENERGY(20) × lifeRate
  res.energy += min(CREEP_PART_MAX_ENERGY(125), BODYPART_COST[type] × lifeRate)
for (rt, amt) in res (energy first, then boosts by first appearance):
  amt = floor(amt); if amt > 0: container (hits > 0) takes min(amt, free); rest → tombstone
for (rt, amt) in creep.store (key order): if amt > 0: container first; rest → tombstone   // not floored
```
Sum in this exact order in f64 before flooring. Examples: recycle (`dropRate 1`) of [WORK,CARRY,MOVE] at ttl 1500 → 200 energy; ttl 750 → 100. **GOTCHA**: `_ticksToLive` is NaN for creeps without `ageTime` (died on their first processed tick, keepers before first tick) → no body resources at all. **GOTCHA**: with `dropRate == 0` (nuke, invader suicide), or for summoned/stronghold creeps, the carried store is destroyed, not dropped.

### CREEP-DIE-VARIANTS
| cause | dropRate | violent | tombstone | source |
|---|---|---|---|---|
| age | 0.2 | no | yes | creeps/tick.js:93-95 |
| hits ≤ 0 / NaN | 0.2 | yes | yes | creeps/tick.js:110-112,134-135 |
| suicide (player) | 0.2 | no | yes | creeps/suicide.js:15 |
| suicide (invader "2") | 0 | no | yes (empty) | creeps/suicide.js:15 |
| recycleCreep | 1.0 | no | yes | spawns/recycle-creep.js:22 |
| spawn stomp | 0.2 | yes | yes | spawns/_born-creep.js:69 |
| nuke landing | 0 | yes | **no** | nukes/tick.js:22-24 |
| stronghold deploy: creep on a template tile | 0.2 | yes | yes (normal resources) | invader-core/stronghold/stronghold.js:78-87 (a PC there reaches `_die` too and throws on `body.length`) |
| userSummoned & foreign creeps | 0.2 | no | yes (no resources) | creeps/tick.js:114-116 |
| spawning creep w/o spawn | — | — | no (silent delete) | creeps/tick.js:39-42 |
| lair replacing keeper | — | — | no (silent delete) | keeper-lairs/tick.js:20-23 |

### CREEP-DIE-DOUBLE — **GOTCHA**
`tick.js` does not return after a death. A creep that dies of age (step d) and then reaches `hits ≤ 0` from this tick's damage (CREEP-HITS-APPLY), or is `userSummoned` with foreigners present, calls `_die` again: a second tombstone with a **second copy of the carried store** (the store is never cleared), a second destroy event, and for the violent call a `creepsLost` stat. Bug-compatibility is a parity decision (Open questions).

---

## 10. Power creeps

### PC-INTENT-ORDER
`reference/engine/src/processor/intents/power-creeps/intents.js:3-13`: `move, usePower, withdraw, transfer, say, drop, pickup, enableRoom, renew`; no priority exclusions. **GOTCHA**: withdraw runs before transfer (creeps: transfer first).

### PC-MOVE / PC-WITHDRAW / PC-TRANSFER / PC-DROP / PC-PICKUP
Delegate to the creep handlers (`power-creeps/move.js:9`, `withdraw.js:7`, `transfer.js:8`, `drop.js:8`, `pickup.js:8`). In movement a PC: always `canMove`, has `rate4 = 0` (loses exact ties on rates 1–3 against any creep with MOVE power), no fatigue, wears roads by 100, cannot enter a tile holding an inter-shard portal (MOVE-PREFILTER).

### PC-SAY
`power-creeps/say.js:6-15`: string message → `actionLog.say = {message[0..10], isPublic}`.

### PC-USE-POWER
`power-creeps/usePower.js:6-310`. Order:
1. Controller present: `!isPowerEnabled` → no-op; `controller.user != me && safeMode > T` → no-op (9-16). No controller → allowed.
2. `info = POWER_INFO[power]` missing → no-op; `cp = powers[power]` missing, level 0, or `cooldownTime > T` → no-op (18-27).
3. `ops = info.ops` (array → `[level−1]`, missing → 0); `store.ops < ops` → no-op (29-37).
4. If `info.range`: target `roomObjects[id]` must exist and be within `range`; an existing effect of the same power with higher level and `endTime > T` → no-op (39-51).
5. Power-specific handling (PC-POWER-*) — any failing type check → no-op.
6. If the power applies an effect → PC-POWER-EFFECT.
7. `powers[power].cooldownTime = T + info.cooldown`; `store.ops −= ops` (291-298); `EVENT_POWER {power, targetId: intent.id}` (may be the string `"undefined"`); `actionLog.power = {id: power, x, y}` (target pos, or own pos without target).
`POWER_INFO` (cooldowns, ranges, ops, durations, effects): `reference/common/lib/constants.js:854-1016`. **GOTCHA**: `PWR_SHIELD.energy = 100` is never charged.

### PC-POWER-TARGET-TYPES
Required `target.type` (else no-op): OPERATE_SPAWN/DISRUPT_SPAWN `spawn`; OPERATE_TOWER/DISRUPT_TOWER `tower`; OPERATE_STORAGE `storage`; OPERATE_LAB `lab`; OPERATE_OBSERVER `observer`; OPERATE_TERMINAL/DISRUPT_TERMINAL `terminal`; DISRUPT_SOURCE/REGEN_SOURCE `source`; REGEN_MINERAL `mineral`; OPERATE_CONTROLLER `controller`; OPERATE_POWER `powerSpawn`; FORTIFY `rampart|constructedWall`; OPERATE_FACTORY `factory` (`usePower.js:72-226,259-274`). No ownership checks on targets. The effects' consequences live in the respective structure specs.

### PC-POWER-GENERATE-OPS
`store.ops += effect[level−1]` (1/2/4/6/8); overflow over `storeCapacity` → drop ops `min(store.ops, total − cap)` (57-70).

### PC-POWER-OPERATE-EXTENSION
Target must have a store and be `storage|terminal|factory|container`; `target.user` set and `!= controller.user` → no-op; target terminal-disrupted (`endTime > T`) → no-op (105-114). Extensions = `type extension && user == controller.user && !off`, sorted by distance to the target (stable → DB order on ties). `limit = min(target.store.energy, effect[level−1] × Σ ext.storeCapacityResource.energy)` (0.2..1.0). Fill each in order with `min(limit − sent, cap − stored)` until `sent ≥ limit`; `sent == 0` → no-op; else `target.store.energy −= sent` (115-131). **GOTCHA**: in a room without a controller this dereferences `roomController.user` and throws, aborting the room tick in the original (chitin: no-op).

### PC-POWER-SHIELD
(228-257) First deletes **any** construction site (any owner) on the PC's tile — even if the next check fails. Then `checkConstructionSite` for a rampart on objects and terrain (`reference/engine/src/utils.js:128-189`: no rampart/site already there, not x/y 0 or 49, on x/y 1 or 48 the three adjacent exit tiles must be walls, not terrain wall) → else no-op. Insert rampart `{user, hits: effect[level−1] (5000..25000), hitsMax: 0, nextDecayTime: T + 50, effects: [{power: PWR_SHIELD, level, endTime: T + 50}]}`.

### PC-POWER-REGEN-MINERAL
Also requires `mineralAmount != 0` and no `nextRegenerationTime` (182-194).

### PC-POWER-OPERATE-FACTORY
Factory without `level` → set `level = power level`; with a level ≠ power level → no-op (259-274).

### PC-POWER-EFFECT
`effects = Object.values(target.effects || [])`, remove entries with this `power`, push `{effect: power, power, level, endTime: T + duration(level)}`; written as `effects: null` then `effects` (277-289). Stored shape may become `{"0":…}` (PROC-BULK); chitin may keep a list with the same order.

### PC-ENABLE-ROOM
`power-creeps/enableRoom.js:6-21`: target `controller`; `controller.user != me && safeMode > T` → no-op; range ≤ 1 → `isPowerEnabled = true`; `actionLog.attack = target pos`. Any controller (owned, reserved, neutral).

### PC-RENEW
`power-creeps/renew.js:6-23`: target `powerBank` or `powerSpawn` within range 1; a power spawn must be active (`checkStructureAgainstController`; no ownership check) → `ageTime = T + POWER_CREEP_LIFE_TIME (5000)`; `actionLog.healed = own pos`.

### PC-TICK
`power-creeps/tick.js:7-73`: MOVE-EXECUTE; CREEP-EXIT rule (same as creeps, NPC filter irrelevant); portal on tile and `!destination.shard` → `interRoom = destination` (no `ageTime` condition); `hits = hits − _damageToApply + _healToApply` (no TOUGH, no body), clamp to `hitsMax`, persist if changed (may go ≤ 0 — no death here); actionLog persisted if changed. No fatigue, no aging here.

### PC-DEATH
Global stage, after power global intents (`reference/engine/src/processor/global-intents/power.js:30-36`): every PC with `T ≥ ageTime − 1 || hits ≤ 0` → `_diePowerCreep` (`power/_diePowerCreep.js:6-59`): tombstone `{…, decayTime: T + TOMBSTONE_DECAY_POWER_CREEP (500), powerCreepId, powerCreepName, powerCreepTicksToLive, powerCreepClassName, powerCreepLevel, powerCreepPowers: {p: {level}}, powerCreepSaying, store}` with the whole store; remove PC; user power-creep doc `{shard: null, spawnCooldownTime: Date.now() + POWER_CREEP_SPAWN_COOLDOWN (8h)}`. **GOTCHA**: the container branch reads `scope.roomObjects`, which the global stage never provides → resources never go into a container. **GOTCHA**: a PC both suicided (global intent) and expired in the same stage dies twice (two tombstones). Wall-clock cooldown.

### PC-SPAWN / PC-SUICIDE
Global intents (`spawnPowerCreep.js:7-41`, `suicidePowerCreep.js:6-16`): spawn inserts the PC on the power spawn tile with `hits = hitsMax`, `ageTime = T + 5000`, `actionLog {spawned: true}`; one PC per power spawn per tick; refused if a PC already stands there. Detailed in the power-creep global spec.

---

## 11. NPC AI

### NPC-KEEPER-SPAWN
`reference/engine/src/processor/intents/keeper-lairs/tick.js:6-67` (lair tick): if `!nextSpawnTime` and the keeper named `"Keeper" + lairId` (user `"3"`) is missing or has `hits < 5000` → `nextSpawnTime = T + ENERGY_REGEN_TIME (300)`. When `T ≥ nextSpawnTime − 1`: delete the existing keeper (no tombstone) and insert a fresh one on the lair tile: body 17 TOUGH, 13 MOVE, then 10 × (ATTACK, RANGED_ATTACK) alternating (50 parts, all 100 hits), `hits = hitsMax = 5000`, `store {energy: 0}`, `storeCapacity 0`, `spawning: false`, `fatigue 0`; `nextSpawnTime = null`. **GOTCHA**: any damaged keeper is silently replaced by a full-health one 300 ticks after the lair first notices; keepers die of age 1500 ticks after their first processed tick (with a corpse tombstone), see also PROC-ACTIVATION.

### NPC-KEEPER-AI
`reference/engine/src/processor/intents/creeps/keepers/pretick.js:7-71`, per keeper:
1. Scan objects: `resources` = sources and minerals (DB order); hostiles = creeps/PCs with user ∉ {"2","3"} (spawning ones included) at `dist ≤ 1` (melee list) and `≤ 3` (ranged list) (23-32).
2. Source: `memory_sourceId` if that object still exists; else first resource with `dist ≤ 5`, saved to `memory_sourceId` (34-43).
3. If a source and `dist > 1`: `dir = moveTo(source, {range: 1, reusePath: 50})`; `dir > 0` → `move {direction}`; else `memory_move = null` (45-52).
4. `meleeTarget = _.min(meleeList, 'hits')` → `attack {id, x, y}`. **GOTCHA**: lodash `_.min([])` returns `Infinity` (truthy) → with no melee target the keeper still gets `attack {id: undefined}` (a no-op that nevertheless blocks `harvest` — irrelevant for keepers) (54-57). Ties: first in DB order.
5. If any ranged-range hostile: `mass = Σ [10,10,4,1][dist]`; `mass > 13` → `rangedMassAttack {}`; else `rangedAttack {id}` on min-hits hostile (59-68).
Keepers never step onto exit tiles' `interRoom` (user filter) and are blocked by player ramparts.

### NPC-INVADER-SPAWN
Backend cron every 300 s wall-clock (`reference/backend/lib/cronjobs.js:18,28-35`), `genInvaders` (245-441): for each `status 'normal'` room **not currently in the active-rooms set** (377-378): skip if invaders present; skip unless `Σ source.invaderHarvested ≥ room.invaderGoal || INVADERS_ENERGY_GOAL (100000)` (goal 1 = always); skip if no level>0 invader core in the sector (393-398); collect exit tiles per side (i in 0..48), drop sides whose neighbour room is owned or reserved (401-428); choose a random side; `createRaid(controller.level if owned)`: type `big` if level ≥ 4 else `small`; count/boost chances via `Math.random` (320-375); roles Melee/Ranged/Healer; bodies (266-273); boosts LO/KO/ZH/UH/GO (center rooms: X-tier) (291-309); creeps get `user "2"`, `ticksToLive: 1500` field (no `ageTime`), `store {}`, `storeCapacity 0`. Then new `invaderGoal = floor(100000 × (0.7 + 0.6·rand))`; with 10% chance it is multiplied by `floor(rand > 0.5 ? 2 : 0.5)` = 2 or 0 (0 then falls back to `INVADERS_ENERGY_GOAL` via `||`) (433-437); sources' `invaderHarvested = 0` (438). This is wall-clock driven, not tick driven; chitin should make it tick-driven (Open questions).

### NPC-INVADER-SUMMON
`POST /create-invader` (`reference/backend/lib/game/api/game.js:631-694`): ≤ 5 invaders, no other foreign creeps, room owned/reserved by requester; same bodies, optional boosts, `userSummoned: requester`. Such creeps drop nothing on death (CREEP-CORPSE-RESOURCES) and die when foreign creeps appear (CREEP-USER-SUMMONED).

### NPC-INVADER-DISPATCH
`reference/engine/src/processor/intents/creeps/invaders/pretick.js:7-52`, per invader (user `"2"`, no `strongholdId`):
- Lists (DB order): `creeps` = non-spawning creeps **or** any PC (precedence quirk); `invaders` = those with the invader's user (incl. itself and stronghold creeps); `healers` = invaders with an alive HEAL; `hostiles` = others with user != `"3"`; `fortifications` = all ramparts and constructed walls (19-39).
- Role: any HEAL part (dead or alive) → NPC-INVADER-HEALER, else NPC-INVADER-ATTACKER; then NPC-INVADER-SHOOT (43-49). Later `intents.set` of the same name overwrites earlier ones.

### NPC-INVADER-FLEE
`flee(creep, range)` (`invaders/flee.js:7-20`): hostiles with `dist < range`; if any → `fakeRuntime.flee` → direction → `move`, return true.

### NPC-INVADER-HEALER
`invaders/healer.js:8-61`:
1. `healTargets` = invaders within 3 (incl. self); sorted by `hitsMax − hits` descending (stable); first → `heal` if `dist ≤ 1` else `rangedHeal` (always issued, even at full health) (11-19).
2. `hits < hitsMax/2`: flee(4); if not fled → `findClosestByPath` to other invaders with alive HEAL → `moveTo(range 1)`; return (21-38).
3. `target = findClosestByPath(damaged invaders)` (self included → returns self at distance 0 → no move since `getDirection(0,0)` is undefined) (40).
4. No target: flee(4) → return if fled; else `findClosestByPath(other invaders without alive HEAL)` (41-46).
5. Still none → `suicide` (48-51).
6. `dist ≤ 1` → `move` toward the target's own tile (only succeeds if it vacates); else `moveTo(range 1)` (52-60).

### NPC-INVADER-ATTACKER
`invaders/findAttack.js:17-120`:
1. `haveAttack` = alive ATTACK. No ATTACK but alive RANGED_ATTACK and flee(3) → return (25-28).
2. `hits < hitsMax/2 && healers exist && !haveAttack` → `findClosestByPath(healers, {ignoreRoads})` → `moveTo(... {maxRooms 1, ignoreRoads})`; no direction → `target = null` (31-43).
3. `haveAttack` → first hostile with `dist ≤ 1` → `attack` (45-50).
4. If no target, try in order until one is found: (a) `findClosestByPath(hostiles, {ignoreRoads, ignoreCreeps})`; (b) `{maxRooms 1, ignoreRoads, costCallback}` where the callback sets every rampart/wall tile (any owner) to cost 0 (= terrain cost, i.e. walkable) — creeps are obstacles again; (c) `{ignoreDestructibleStructures, maxRooms 1, ignoreRoads}`. For each found target: if `haveAttack || dist > 3` → `moveTo` with the same options (plus `maxRooms 1`) (52-80).
5. Still none: spawns (any owner) for which `checkPath` (default-option path ends adjacent) fails; if none and the room has an owned controller → `suicide`; else `moveTo(first such spawn, {ignoreDestructibleStructures, ignoreRoads})`; return (82-99).
6. `attack {id: target}` (**overwrites** step 3 — the attack goes to the path target; with adjacent hostiles `findClosestByPath` returns the **last** adjacent hostile in DB order) (101).
7. If `haveAttack` or alive WORK, and `memory_move.path` non-empty: `pos = unpack(path[0])` (the first step of the cached path, not necessarily adjacent); controller-structure types except spawn on that tile (roads and containers included) → `rangedAttack` it if alive RANGED_ATTACK, and `dismantle` it if alive WORK else `attack` it (overwrites step 6) (102-119). Out-of-range targets just no-op in the processor.

### NPC-INVADER-SHOOT
`invaders/shootAtWill.js:7-22`: alive RANGED_ATTACK and hostiles within 3 → `rangedAttack` the min-hits one (first on ties), overwriting any earlier `rangedAttack`.

### NPC-FAKE-PATH
`fakeRuntime.findPath(source, goal, opts)` (`reference/engine/src/processor/common/fake-runtime.js:143-182`) calls the native PathFinder (`reference/driver/lib/path-finder.js:86-142`, `reference/driver/native/src/pf.cc:506-510` — origin already satisfying a goal → empty path) with `maxRooms 1`, default `maxOps 2000`, `heuristicWeight 1.2`, `plainCost 2 / swampCost 10` unless `ignoreRoads` (then defaults 1/5), and a room callback returning `false` for any other room and otherwise the default matrix (104-141): cost 255 on `OBSTACLE_OBJECT_TYPES` + `portal` tiles (with `ignoreDestructibleStructures` minus `constructedWall, spawn, extension, link, storage, observer, tower, powerBank, powerSpawn, lab, terminal` — **GOTCHA**: `nuker`, `factory`, `invaderCore` stay impassable; with `ignoreCreeps` minus `creep` only — power creeps stay obstacles), on foreign non-public ramparts and own obstacle sites (unless ignoreDestructibleStructures); legacy `swamp` objects 5/10; roads 1 unless `ignoreRoads`. `costCallback` gets a clone. CostMatrix index is `x*50+y` (column-major) (`fake-runtime.js:86`). A RoomPosition goal with no range gets the goal appended if the path ends adjacent (175-179). Exact paths depend on chitin's PathFinder being 1:1 (pathfinder spec).
**PERF**: the default matrix is rebuilt by a full-room scan for every search (several per invader per tick). Cache one matrix per (room, NPC user, option-set) per tick (the rampart/site rules depend on the NPC's user); nothing invalidates it during the keeper/invader pretick phase, which precedes the invader-core pretick and all intents (keeper/invader AIs only write `memory_*` fields; the invader-core pretick can kill creeps/remove sites when deploying, so do not reuse the cache past it).

### NPC-FAKE-MOVETO
`moveTo(creep, target, opts)` (200-245): defaults `reusePath 5`, `range 0`; `dist ≤ range` → 0. Re-path when `memory_move` is missing, has no `dest`/`time`, `dest` ≠ packed target, or `T > time + reusePath`: `memory_move = {dest, path, time: T}` with positions packed as `String.fromCharCode(32 + (x<<6 | y))` (54-60). Next direction (311-332): index of the current tile in `path`; last index → 0; not on path → step to `path[0]` if adjacent, else 0; otherwise step to `path[i+1]`. A non-zero direction stamps `memory_move.lastMove = T`. **GOTCHA**: a blocked NPC keeps retrying the same cached step until the cache expires (keepers: 50 ticks) or (keepers) `moveTo` returns 0.

### NPC-FAKE-FINDCLOSEST
`findClosestByPath(from, objects, opts)` (265-309): empty → null; an object on the same tile → it; else one multi-goal search with every object as `{range: 1}` goal; result = the **last** object in list order within distance 1 of the path's end (or of `from` if the path is empty), else null. Mutates `opts.range = 0` if unset.

### NPC-FAKE-FLEE
`flee(creep, hostiles, range)` (185-198): flee search from all hostile positions with that range (plain 2 / swamp 10); direction to `path[0]`, or 0.

### NPC-FAKE-WALKTO
`walkTo` (247-263, stronghold only): `moveTo`; if an own creep stands on the next tile and it has no `memory_move` or `lastMove + 1 < T`, give it a `move` toward the walker's tile (swap) and reset its `memory_move`.

### NPC-STRONGHOLD-CREEPS
Stronghold defenders (`strongholdId` set) are excluded from invader AI and driven by the invader core's pretick (`reference/engine/src/processor/intents/invader-core/pretick.js:7-60`, `stronghold/stronghold.js:208-332,363-386`): behaviours `coordinated` (walk to an assigned rampart spot via a "safe" cost matrix — only own ramparts walkable; step away from the core while it spawns), `simple-melee` (attack adjacent, else walk to closest hostile), `fortifier` (repair ramparts below `hitsTarget`), bodies in `stronghold/creeps.js:96-126`; focus-fire targeting in `stronghold.js:208-299`. Full specification belongs to the invader-core/stronghold spec.

---

## 12. PERF summary

- **Room-local**: everything in sections 3–9 and NPC AI. Cross-room only: MOVE-INTERROOM, PC death/spawn (global), GCL/rooms/stats/notifications (U).
- **Original hot spots (O(N²) per room)**: rampart/container/pile lookups by full scan in attack/RMA/dismantle/die/drop; `checkObstacleAtXY` scans all objects for every contested/destination tile; `execute` scans all objects per mover; portal lookup per creep per tick; `_born-creep` scans per direction; `checkStructureAgainstController` scans per call; NPC AI rescans the room per NPC and runs 1–5 PathFinder searches per invader per tick (plus one `checkPath` per spawn in the fallback branch).
- **chitin**: per-room tile index (2500-cell arrays) for objects, ramparts, roads, containers, piles, sites; movement matrix as `[SmallVec<mover>; 2500]` + insertion-ordered tile list (order matters for MOVE-TIEBREAK and second-loop order); precomputed per-creep `moves`, `weight`, `carryWeight` once per tick; damage/heal accumulators as fields; deterministic object order (creation sequence) to stand in for DB order.
- Parallelism: rooms are independent within the processing phase; collect U/G effects as ordered deltas and apply serially. NPC AI per room can run in the room worker.

---

## 13. Open questions

1. **User and object order** (PROC-USER-ORDER, PROC-OBJECT-ORDER, PROC-TICK-ORDER): the original is runner-timing and DB-order dependent. Proposed: NPC "2","3" first, players by a stable per-user index, objects by first-intent order (runtime) and creation sequence (tick loop). Needs a project decision.
2. **Bug compatibility**: CREEP-DIE-DOUBLE (duplicate tombstone + store copy), PROC-BULK insert-id reuse (lost inserts), PC-DEATH container skip, PC-POWER-OPERATE-EXTENSION / CREEP-WITHDRAW crashes that abort a room tick, dismantle NaN energy for `store: {}` NPCs. Which to replicate (behind a flag) vs fix?
3. Inter-shard portal destinations (`destination.shard`) for creeps: `interRoom` merges `{shard, room}` over `{room, x, y}`; the global stage only checks `accessibleRooms[room]`. Private-server code has no shard transfer; behaviour for chitin multi-world setups is undefined.
4. NPC-INVADER-SPAWN is wall-clock (300 s) and only considers rooms absent from the active-room set at that instant; a tick-based schedule is needed for deterministic simulation — the exact mapping is a design choice.
5. LokiJS find order for `rooms.objects` by room index was taken as insertion order; not verified against the storage engine's binary-index tie ordering (`reference/storage/lib/db.js`).
