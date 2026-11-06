# 01 — Tick pipeline (main loop, runners, room processors, global stage)

Scope: everything that happens between two increments of `Game.time` on the original private server, end to end: which users run, how their intents travel to the room processors, which rooms get processed, how a room processor reads and writes objects, the global (cross-room) stage, the game-time increment, and the post-tick outputs (history, map view, event log, notifications, stats). It also covers CPU-bucket bookkeeping, user deactivation, tick-rate and pause settings, mod hooks, ordering and randomness, and wall-clock dependencies. Per-object game rules (harvest, attack, movement conflict resolution, and so on) belong to other specs. This spec only defines **where** in the pipeline they run and what state they see.

Three cross-cutting decisions are **normative here** and the processor and API specs (05–08, 11) reference them instead of deciding locally:
- ORD-POLICY (§16): the one deterministic order rule for players, global intent documents and randomness (RNG-SEED).
- CLOCK-POLICY (§16): which wall-clock reads become a virtual clock `vclock(T)` and which stay real time.
- COMPAT-TABLE (§19A): replicate / fix / configurable for every known original defect, per target profile (`private` or `mmo`). Parity tests take their expected results from it.

Sources (read-only, cited as `reference/<pkg>/<path>:<line>`): engine @8097782, driver @cf63d8a, common @2fb779b, storage @3d6c3d0, backend 3.3.0, launcher 4.2.0, lodash 3.10.1, xxscreeps @d1e3bad (comparison only). Measurements come from the user's own profiling mods and harness: `E:\screeps\server\engine-*.js`, `E:\screeps\server\logs\*`, `E:\screeps\harness\{benchmark*.cjs,mini-bench.cjs,results,mini-results}`. Those are cited by absolute path.

Conventions
- Locality tags (as in spec 05): **R** = room-local, **U** = per-player (needs the player's whole view), **G** = global/cross-room or server-wide.
- `T` = the game time during a tick (`Game.time` seen by players == `gameTime` seen by the processors of the same tick). The tick ends by setting the stored time to `T+1`.
- "env" = the storage key/value area (`reference/common/lib/storage.js:33-52`). "pubsub" = storage pub/sub.
- **GOTCHA** = a subtle point that must be copied exactly for 1:1 behaviour (or that a naive port gets wrong). **PERF** = notes on structure and performance: what is room-local versus global, and where time goes.
- "Loki" = the default storage process (`@screeps/storage`). "Mongo/Redis" = `screepsmod-mongo`, which is what the user's harness runs and what the official server resembles. Where the two differ, both are stated.
- `vclock(T)` = chitin's virtual clock in epoch milliseconds (CLOCK-VIRTUAL). "profile" = the chitin compatibility profile, `private` or `mmo` (COMPAT-PROFILES).

---

## 1. Inventory

| ID | Item | Kind | Loc | Primary source |
|---|---|---|---|---|
| TOPO-PROCS | Process set: storage, backend, engine_main, N runners, M processors; launcher defaults | config | G | reference/launcher/lib/start.js:18-27,176-224 |
| TOPO-RPC | All state goes over JSON-framed RPC to one storage process (Loki) | infra | G | reference/common/lib/rpc.js:5-54; reference/common/lib/storage.js:62-164 |
| TOPO-RESTART | Runners and processors are killed and restarted every `restart_interval` (3600 s) | config | G | reference/launcher/lib/start.js:26,139-149,214,223 |
| TOPO-DRIVER | Engine loads the driver from `DRIVER_MODULE` | infra | G | reference/engine/src/utils.js:15-21 |
| CFG-ENGINE-DEFAULTS | `config.engine` defaults (mainLoopMinDuration 1000, reset 5000, cpuMaxPerTick 500, bucket 10000, historyChunkSize 20, …) | config | G | reference/driver/lib/index.js:19-33 |
| CFG-ENV-KEYS | env keys used by the loop (gameTime, activeRooms, tickRate, mainLoopPaused, …) | config | G | reference/common/lib/storage.js:33-60 |
| CFG-TICKRATE | Tick duration from env `tickRate` and pubsub `setTickRate`; CLI `setTickDuration` | config | G | reference/driver/lib/index.js:118-133; reference/backend/lib/cli/system.js:54-72 |
| CFG-PAUSE | Pause and resume via env `mainLoopPaused` | config | G | reference/driver/lib/index.js:339-347; reference/backend/lib/cli/system.js:29-41 |
| CFG-PTR | `PTR` env changes the nuker cooldown and land time to 100 | config | G | reference/engine/src/config.js:4-11; reference/engine/src/processor/intents/nukers/launch-nuke.js:37-48 |
| TICK-LOOP | Main loop stage order | phase | G | reference/engine/src/main.js:11-127 |
| TICK-STAGE-EVENTS | `mainLoopStage` events per stage | hook | G | reference/engine/src/main.js:16,29,34,…,115 |
| TICK-PAUSE-CHECK | Start-of-tick pause check plus pubsub `tickStarted` | phase | G | reference/driver/lib/index.js:339-347 |
| TICK-GET-USERS | User selection: `active != 0 && cpu > 0`, sorted by lastUsedDirtyTime desc | phase | G | reference/driver/lib/index.js:144-151 |
| TICK-RUN-USERS | Enqueue users, wait until all are done | phase | G | reference/engine/src/main.js:32-41 |
| TICK-GET-ROOMS | Take the active-room set (`smembers` then `del`) | phase | G | reference/driver/lib/index.js:170-174 |
| TICK-RUN-ROOMS | Enqueue rooms, wait until all are done | phase | G | reference/engine/src/main.js:47-56 |
| TICK-EMPTY-ROOMS | Loki: an empty active set returns `undefined`, so `addMulti` throws and the tick aborts | gotcha | G | reference/storage/lib/db.js:750-764; reference/driver/lib/queue.js:43-48 |
| TICK-COMMIT | `commitDbBulk` ×2 (no-ops) | phase | G | reference/engine/src/main.js:57-71; reference/driver/lib/index.js:534-536 |
| TICK-GLOBAL | Global stage | phase | G | reference/engine/src/main.js:62-66; reference/engine/src/processor/global.js:9-64 |
| TICK-INC-TIME | `gameTime = gameTime + 1` (read, then set) | phase | G | reference/driver/lib/index.js:416-419 |
| TICK-ACCESSIBLE | Every 21 ticks: rebuild `accessibleRooms` and `roomStatusData` (not awaited) | phase | G | reference/engine/src/main.js:79-83; reference/driver/lib/index.js:490-524 |
| TICK-ROOMS-DONE | pubsub `roomsDone` with the new time | phase | G | reference/driver/lib/index.js:350-352 |
| TICK-CUSTOM | `mainLoopCustomStage()` hook | hook | G | reference/engine/src/main.js:89-93 |
| TICK-DELAY | Next-tick delay = `max(minDuration - used, 0)` or `getNextTickDelay(used)` | phase | G | reference/engine/src/main.js:100-122 |
| TICK-WATCHDOG | Every 5 s while a tick is running: reset all queues | gotcha | G | reference/engine/src/main.js:19-24; reference/driver/lib/queue.js:63-65 |
| TICK-ERROR | An error in any stage skips the rest of the tick (no time increment); the next loop is still scheduled | gotcha | G | reference/engine/src/main.js:94-99 |
| QUEUE-SEMANTICS | pending/processing lists, fetch, markDone, whenAllDone, reset | infra | G | reference/storage/lib/queue.js:24-105 |
| QUEUE-ORDER | Loki pops LIFO; Redis is FIFO | gotcha | G | reference/storage/lib/queue.js:32,67; E:\screeps\server\node_modules\screepsmod-mongo\lib\common\queue.js:68,113 |
| QUEUE-NAMES | `users` maps to `usersIvm`; `rooms` | infra | G | reference/driver/lib/queue.js:18-22 |
| QUEUE-SIGTERM | SIGTERM stops fetching (returns a never-resolving promise) | infra | G | reference/driver/lib/queue.js:10-16,28-31 |
| RUN-LOOP | Runner loop with a generic-pool concurrency of `RUNNER_THREADS` (default 2) | phase | U | reference/driver/lib/index.js:558-581; reference/engine/src/runner.js:53-78 |
| RUN-USER | `runUser`: makeRuntime, then `saveResult` on success **and** failure | phase | U | reference/engine/src/runner.js:9-51 |
| RUN-SKIP | Pre-run skips: skipTicksPenalty, negative bucket | rule | U | reference/driver/lib/runtime/make.js:54-96 |
| RUN-DATA | Runtime data assembly (visible rooms, objects, users, market, segments) | rule | U | reference/driver/lib/runtime/data.js:99-264 |
| RUN-DEACTIVATE | User with no `rooms.objects` gets `active: 0` and does not run | rule | U | reference/driver/lib/runtime/data.js:103-109 |
| RUN-REACTIVATE | Code upload and respawn set `active: 10000` | rule | U | reference/backend/lib/game/api/user.js:102; reference/backend/lib/game/api/game.js:405 |
| RUN-VM | Isolate cache: reuse, recreate on a newer code timestamp, dispose after 3 min idle | rule | U | reference/driver/lib/runtime/user-vm.js:13-101,160-166 |
| RUN-TIMEOUT | Per-`evalCode` timeout `tickLimit+5` ms (minimum 30); 5 s "ungraceful" timeout | rule | U | reference/driver/lib/runtime/runtime-driver.js:40-45; reference/driver/lib/runtime/make.js:297-318 |
| RUN-RESULT | Output message: usedTime, intentsList, memory, segments, console, visuals | rule | U | reference/driver/lib/runtime/runtime.js:233-297 |
| RUN-SAVE | Save order and failure semantics (memory, segments, intents) | rule | U | reference/engine/src/runner.js:18-50 |
| RUN-PARTIAL-ERROR | Exception in player code: intents and Memory **are still saved** | gotcha | U | reference/driver/lib/runtime/runtime.js:237-248 |
| RUN-TOTAL-ERROR | Throw after the loop (>10 segments, circular Memory): the whole tick output is discarded | gotcha | U | reference/driver/lib/runtime/runtime.js:246-269; reference/driver/lib/runtime/make.js:279-294 |
| RUN-MEMORY-LIMIT | Memory >2 MiB: memory save rejected, intents still saved, error not shown to the player | gotcha | U | reference/driver/lib/index.js:153-160; reference/engine/src/runner.js:42-45 |
| RUN-CONSOLE | Console log/results/errors published; NPC ids 2 and 3 special-cased | rule | U | reference/driver/lib/index.js:354-410 |
| RUN-CONSOLE-CMDS | Console commands run after `loop()` with no timeout; consumed every tick | rule | U | reference/engine/src/game/game.js:535-547; reference/driver/lib/runtime/data.js:149 |
| RUN-VISUAL | Room visuals stored with a TTL of `mainLoopResetInterval/1000` s | rule | U | reference/driver/lib/runtime/make.js:253-260 |
| RUN-SEGMENTS | active/public/foreign segment bookkeeping written to the user document | rule | U | reference/driver/lib/runtime/make.js:200-251 |
| RUN-INTERSHARD | InterShardMemory is never persisted | gotcha | U | reference/engine/src/runner.js:39-41; reference/driver/lib/runtime/runtime.js:125-128 |
| RUN-SANDBOX-HOOK | `playerSandbox` event before run (sets `Game.shard`) | hook | U | reference/driver/lib/index.js:35-52; reference/driver/lib/runtime/make.js:166-185 |
| CPU-LIMIT | `tickLimit = min(max(0,cpuAvailable) + cpu, cpuMaxPerTick)` | formula | U | reference/driver/lib/runtime/data.js:151-165 |
| CPU-INTENT-COST | 0.2 CPU per intent; `say`/`pull` free; `set` charged once per (id,name) | formula | U | reference/driver/lib/runtime/runtime.js:60-100 |
| CPU-USED | usedTime / usedCleanTime / usedDirtyTime definitions | formula | U | reference/driver/lib/runtime/runtime.js:202-204,235,271-273 |
| CPU-BUCKET-UPDATE | `cpuAvailable += cpu - ceil(clean + 0.2*count)`, capped at the bucket size, no floor | formula | U | reference/driver/lib/runtime/make.js:98-111,192-212 |
| CPU-NEGATIVE-SKIP | `cpuAvailable < 0`: skip the tick and refill (clamped at `-2*cpu`) | formula | U | reference/driver/lib/runtime/make.js:72-83 |
| CPU-PENALTY | `skipTicksPenalty` skips | rule | U | reference/driver/lib/runtime/make.js:61-71 |
| CPU-GAME-FIELDS | `Game.cpu.{tickLimit,limit,bucket,getUsed}`, `Game.cpuLimit` | rule | U | reference/engine/src/game/game.js:145-157 |
| CPU-STATS | lastUsedCpu, lastUsedDirtyTime, pubsub `user:<id>/cpu` | rule | U | reference/driver/lib/runtime/make.js:196-214,262-273 |
| INT-LIST | Runtime intent list shape; `set`/`push`/`pushByName`/`remove` | format | U | reference/driver/lib/runtime/runtime.js:63-100 |
| INT-OVERWRITE | Same (object, intent name) in one tick: last call wins | rule | U | reference/driver/lib/runtime/runtime.js:66-72 |
| INT-LIMITS | Per-type caps (notify 20, deal 10, global power 50, …); **no global MAX_INTENTS** | rule | U | reference/engine/src/game/game.js:175; reference/engine/src/game/market.js:104,149,168,187 |
| INT-STORE | Route intents by `object.room`; unknown ids dropped | rule | U | reference/engine/src/utils.js:405-436 |
| INT-SANITIZE | Field whitelist plus transforms (parseInt, price, userString…); missing fields become sentinels | rule | U | reference/common/lib/system.js:4-114 |
| INT-ROOM-GROUP | `room` intents grouped by the sanitized `roomName` | rule | U | reference/common/lib/system.js:116-140 |
| INT-SAVE | `rooms.intents` `$merge` upsert per room, `users.intents` insert, notify, activate rooms | rule | U | reference/driver/lib/index.js:180-243 |
| INT-NOTIFY | Game.notify persistence: ≤20, groupInterval clamp, 500 chars, wall-clock buckets | rule | U | reference/driver/lib/index.js:185-227 |
| INT-MANUAL | API `objectsManual` replaces script intents per object | rule | R | reference/backend/lib/game/api/game.js:509-527; reference/engine/src/processor.js:231-234 |
| INT-GLOBAL-API | API `add-global-intent` inserts into `users.intents` | rule | G | reference/backend/lib/game/api/game.js:714-720 |
| INT-CLEAR | Room intents cleared after each room; global intents cleared after the global stage | rule | R/G | reference/engine/src/processor.js:580-583; reference/driver/lib/index.js:322-328 |
| INT-ORDER-USERS | Users within a room: ids `2`,`3` first (integer keys), then `$merge` arrival order (runner completion order); chitin: ORD-POLICY | gotcha | R | reference/engine/src/processor.js:229; reference/driver/lib/index.js:236 |
| INT-ORDER-OBJECTS | Objects within a user: order of the first `intents.set` per object; position of the `room` pseudo-object | gotcha | R | reference/driver/lib/runtime/runtime.js:66-72; reference/engine/src/utils.js:409-433 |
| INT-OWNERSHIP | Processor skips a missing object, `_skip`, or an object owned by another user; unowned objects accept intents | rule | R | reference/engine/src/processor.js:248 |
| INT-DISPATCH | Per-type intent dispatch table | rule | R | reference/engine/src/processor.js:250-311 |
| INT-NOTIFY-ATTACKED | Generic `notifyWhenAttacked` handling | rule | R | reference/engine/src/processor.js:313-315 |
| INT-CUSTOM | `customIntentTypes` (mods) | hook | U | reference/driver/lib/index.js:27; reference/common/lib/system.js:105-111 |
| ACT-SET | `activeRooms` env set: `sadd` anywhere; consumed and deleted at room-phase start | rule | G | reference/driver/lib/index.js:170-178 |
| ACT-SOURCES | Every activation source (runner intents, processor, global, API, cron, strongholds) | rule | G | see §8 |
| ACT-RULE | Processor self-activation conditions (object pre-scan plus `roomInfo.active`) | rule | R | reference/engine/src/processor.js:43-171,499-507 |
| ACT-STALENESS | Inactive rooms are not processed; stale state is observable | gotcha | R | reference/engine/src/processor.js:504-507 |
| ACT-FORCE-UPDATE | Cron wakes each inactive room every ~90–109 ticks (wall-clock gated) | rule | G | reference/backend/lib/cronjobs.js:178-192 |
| PROC-LOOP | Processor loop: fetch, 6 parallel reads, process (not awaited), clear intents, markDone | phase | R | reference/engine/src/processor.js:539-594 |
| PROC-INPUT | Inputs: objects (mapById), owners, terrain, gameTime, roomInfo, flags | rule | R | reference/driver/lib/index.js:245-280,412-423 |
| PROC-PRESCAN | Per-object pre-pass (actionLog reset, collections, activation flags, hook) | phase | R | reference/engine/src/processor.js:43-171 |
| PROC-NUKE-PRETICK | Nukes landing next tick: null every `createCreep` intent | phase | R | reference/engine/src/processor/intents/nukes/pretick.js:6-15 |
| PROC-NPC-PRETICK | Keeper/invader/invader-core AI injects intents (merge rule) | phase | R | reference/engine/src/processor.js:180-219 |
| PROC-CALC-SPAWNS | `off` flags for excess spawns and extensions | phase | R | reference/engine/src/processor.js:221-223; reference/engine/src/processor/intents/_calc_spawns.js:6-41 |
| PROC-MOVE-INIT | `movement.init` (module-global state) | phase | R | reference/engine/src/processor.js:225; reference/engine/src/processor/intents/movement.js:61-67 |
| PROC-INTENTS | Per-user, per-object intent processing | phase | R | reference/engine/src/processor.js:227-322 |
| PROC-ROOM-INTENTS | `room` pseudo-object: flags, construction sites, destroyStructure, genEnergy | phase | R | reference/engine/src/processor/intents/room/intents.js:3-54 |
| PROC-MOVE-CHECK | `movement.check(safeModeOwner)` once per room after all intents | phase | R | reference/engine/src/processor.js:324 |
| PROC-ENERGY-AVAILABLE | `scope.energyAvailable` computed after intents | phase | R | reference/engine/src/processor.js:326 |
| PROC-TICK-LOOP | Per-object tick handlers in object-key order; collapse timer; powerBank/deposit expiry | phase | R | reference/engine/src/processor.js:342-483 |
| PROC-SAVE | mapView, bulk executes, eventLog, roomInfo, activation, history | phase | R | reference/engine/src/processor.js:489-518 |
| PROC-ERROR | Exception in processRoom: no writes for that room, room not re-activated | gotcha | R | reference/engine/src/processor.js:570-583 |
| PROC-PATHFINDER | Per-process terrain load into the native pathfinder; `pathFinder.make` per room | infra | R | reference/driver/lib/index.js:113-116; reference/engine/src/processor.js:174 |
| PROC-ITER | lodash `_.forEach(obj)` snapshots `Object.keys`; JS key order rules | gotcha | R | reference/lodash-3.10.1/lodash.js:3064-3079,9671-9678 |
| BULK-UPDATE | Deep-merge into the live object; `$set` of the merged top-level values | rule | R | reference/driver/lib/bulk.js:33-63 |
| BULK-INSERT | Clone at insert, counter id, optional explicit `_id` | rule | R | reference/driver/lib/bulk.js:64-77 |
| BULK-REMOVE | Remove (including a pending insert; counter-decrement bug, reachable: BUG-INSERT-COUNTER) | rule | R | reference/driver/lib/bulk.js:78-89 |
| BULK-INC-SET-PULL | `inc` (mutates the live object), `addToSet`, `pull` | rule | R | reference/driver/lib/bulk.js:90-120 |
| BULK-EXECUTE | Op order: queued ops (remove/inc/addToSet/pull), then `$set`s, then inserts | rule | R | reference/driver/lib/bulk.js:121-130; reference/storage/lib/db.js:480-513 |
| BULK-HIDDEN | Keys starting with `_` (recursively) and `$loki` are never persisted | rule | R | reference/driver/lib/bulk.js:6-23 |
| BULK-UNDEFINED | `undefined` values are skipped by the merge and dropped by JSON; only `null` clears | gotcha | R | reference/lodash-3.10.1/lodash.js:2351-2381 |
| BULK-ARRAY-MERGE | Arrays merge index-wise into the existing array | gotcha | R | reference/lodash-3.10.1/lodash.js:2400-2440 |
| BULK-STORAGE-APPLY | Storage applies `$set,$merge,$inc,$unset,$addToSet,$pull` in that order | rule | G | reference/storage/lib/db.js:257-293 |
| BULK-COLLECTION-WIDE | A bulk writer can touch any document of the collection (other rooms) | rule | G | reference/driver/lib/index.js:282-320 |
| VIS-INMEM | Later steps see earlier in-memory mutations in the same room tick | rule | R | reference/driver/lib/bulk.js:50-53 |
| VIS-DEFERRED | Deferred fields (`_damageToApply`, `_healToApply`, `_pulled`, …) | rule | R | reference/engine/src/processor/intents/_damage.js:16-33 |
| VIS-NEW-OBJECTS | Inserted objects added to `roomObjects` are ticked in the same tick; others are not | gotcha | R | reference/engine/src/processor/intents/_create-energy.js:42-52 |
| VIS-DELETED | Deleted objects are skipped by later loops; immediate structure destruction | gotcha | R | reference/engine/src/processor/intents/structures/_destroy.js:52-53 |
| VIS-USERS-STALE | `bulkUsers.inc(idString, …)` does not update in-memory user docs | gotcha | R | reference/engine/src/processor/intents/creeps/upgradeController.js:84-86 |
| VIS-RUNTIME-SNAPSHOT | Runners read state as of the end of the previous tick | rule | U | reference/engine/src/main.js:32-56 |
| XROOM-EDGE | Creep or power creep on an edge sets `interRoom` in the room phase; moved in the global stage | rule | G | reference/engine/src/processor/intents/creeps/tick.js:52-78; reference/engine/src/processor/global.js:33-43 |
| XROOM-PORTAL | Portal sets `interRoom = destination` | rule | G | reference/engine/src/processor/intents/creeps/tick.js:80-85; reference/engine/src/processor/intents/power-creeps/tick.js:44-47 |
| XROOM-BOUNCE | An idle creep on an exit tile alternates rooms every tick | gotcha | G | (consequence of XROOM-EDGE) |
| XROOM-INACCESSIBLE | Target not accessible: move skipped, stale `interRoom` kept | gotcha | G | reference/engine/src/processor/global.js:34-36 |
| XROOM-TERMINAL | `send` validated and stored in the room phase, executed in the global stage | rule | G | reference/engine/src/processor/intents/terminal/send.js:6-33; reference/engine/src/processor/global-intents/market.js:71-95 |
| XROOM-NUKE | Launch inserts a nuke into the target room (not activated); lands only at `gameTime == landTime-1` | gotcha | G | reference/engine/src/processor/intents/nukers/launch-nuke.js:42-49; reference/engine/src/processor/intents/nukes/tick.js:16 |
| XROOM-OBSERVER | `observeRoom` persisted for one tick; next-tick visibility; target not activated | rule | G | reference/engine/src/processor.js:116-118,284-288,403-406; reference/driver/lib/runtime/data.js:120-122 |
| XROOM-WORLD-WRAP | Terminal and market distances wrap around `worldSize` (torus) | gotcha | G | reference/engine/src/utils.js:644-655; reference/common/index.js:183-193 |
| GLOB-INPUT | `getInterRoom` queries, read after all room writes | rule | G | reference/driver/lib/index.js:429-449 |
| GLOB-ORDER | Inter-room moves, then power, then market, then execute all bulks and clear `users.intents` | phase | G | reference/engine/src/processor/global.js:31-62 |
| GLOB-INTERROOM | Move creeps; activate the destination room | rule | G | reference/engine/src/processor/global.js:33-43 |
| GLOB-POWER-INTENTS | spawn, suicide, delete, upgrade, create, rename (per `users.intents` doc) | rule | G | reference/engine/src/processor/global-intents/power.js:8-28 |
| GLOB-POWER-DEATH | Power creep dies if `gameTime >= ageTime-1` or `hits <= 0` | rule | G | reference/engine/src/processor/global-intents/power.js:30-36 |
| GLOB-MARKET-SENDS | Terminal sends executed | rule | G | reference/engine/src/processor/global-intents/market.js:71-95 |
| GLOB-MARKET-INTENTS | createOrder, changeOrderPrice, extendOrder, cancelOrder, deal collection | rule | G | reference/engine/src/processor/global-intents/market.js:104-287 |
| GLOB-MARKET-TDEALS | Terminal deals sorted by distance, then executed | rule | G | reference/engine/src/processor/global-intents/market.js:289-398 |
| GLOB-MARKET-DDEALS | Direct (intershard-resource) deals `_.shuffle`d | rng | G | reference/engine/src/processor/global-intents/market.js:400-495 |
| GLOB-MARKET-MAINT | Order expiry (wall clock), active/amount recompute | rule | G | reference/engine/src/processor/global-intents/market.js:497-589 |
| GLOB-EXECUTE | All global bulks executed in parallel; `users.intents` cleared | rule | G | reference/engine/src/processor/global.js:51-62 |
| POST-GAMETIME | Time semantics T and T+1 | rule | G | reference/driver/lib/index.js:412-419 |
| POST-HISTORY | Per-tick room snapshot if active; chunk upload with diffs every 20 ticks | rule | R | reference/engine/src/processor.js:428-440,522-536; reference/driver/lib/history.js:7-43 |
| POST-MAPVIEW | `mapView:<room>` written every processed tick | rule | R | reference/engine/src/processor.js:328-337,442-481,493 |
| POST-EVENTLOG | `roomEventLog` hash field written every processed tick | rule | R | reference/engine/src/processor.js:497; reference/driver/lib/index.js:583-585 |
| POST-ROOMINFO | `rooms` doc `$set` if changed | rule | R | reference/engine/src/processor.js:509-511 |
| POST-NOTIFICATIONS | `sendNotification` / `users.notifications`; email cron | rule | G | reference/driver/lib/index.js:455-471; reference/backend/lib/cronjobs.js:124-176 |
| POST-STATS | Room stats accumulated in memory, never persisted | rule | R | reference/driver/lib/index.js:473-487 |
| POST-TICK-TIMING | Backend `/api/game/tick` = min of the last 30 `roomsDone` gaps | rule | G | reference/backend/lib/game/api/game.js:722-735 |
| CRON-SCHED | Backend crons on wall-clock periods, checked every 1 s | rule | G | reference/backend/lib/index.js:20; reference/backend/lib/cronjobs.js:14-36 |
| CRON-WORLD-GEN | genPowerBanks/genInvaders/genDeposits/strongholds insert objects and activate rooms asynchronously | rule | G | reference/backend/lib/cronjobs.js:194-441,556-643 |
| CRON-NPC-ORDERS | recreateNpcOrders | rule | G | reference/backend/lib/cronjobs.js:38-122 |
| CRON-OTHER | purgeTransactions, calcMarketStats, deletePowerCreeps, sendNotifications | rule | G | reference/backend/lib/cronjobs.js:124-176,443-554 |
| HOOK-EVENTS | `config.engine` events: init, mainLoopStage, runnerLoopStage, processorLoopStage, playerSandbox, processObject, preProcessObjectIntents, processObjectIntents, postProcessObject, processRoom, saveRoomHistory | hook | G | reference/driver/lib/index.js:139; reference/engine/src/processor.js:169,246,318,482,489 |
| HOOK-CONFIG | mainLoopCustomStage, getNextTickDelay, customIntentTypes, registerCustomObjectPrototype | hook | G | reference/driver/lib/index.js:19-73; reference/engine/src/main.js:117-119 |
| ORD-ROOMS | Room processing order across processors is irrelevant except for shared side effects | rule | G | reference/engine/src/processor.js:555-589 |
| ORD-KEYS | Object-key order: integer-like keys first, then insertion order | gotcha | R | reference/lodash-3.10.1/lodash.js:9671-9678 |
| RNG-SITES | Every `Math.random` / lodash `_.shuffle` / `_.sample` reachable from the pipeline (ids, minerals, calcReward, strongholds, market shuffle, crons) | rng | G | reference/storage/lib/db.js:317-328 (and §16) |
| CLOCK-WALL | Every `Date.now()` / `new Date()` read in the pipeline, backend crons and API | gotcha | G | §16 |
| ORD-POLICY | One deterministic order rule for every order-sensitive point (players, global docs, deals, RNG) | rule | G | §16 |
| ORD-USER-KEY | Per-tick player rank `orderKey(T, uid)` (seeded hash, ties by id) | formula | G | §16 |
| ORD-POINTS | The order-sensitive points: room intents loop, power global intents, market intents, deal ties, direct-deal shuffle, object-order loops | rule | G | reference/engine/src/processor.js:229; reference/engine/src/processor/global-intents/power.js:17; reference/engine/src/processor/global-intents/market.js:71,105,289,400 |
| ORD-REACHABLE | Any fixed player order equals the order of some reachable original tick | rule | G | reference/storage/lib/queue.js:32; reference/driver/lib/index.js:144-151,180-243,558-581; reference/engine/src/runner.js:63-76 |
| RNG-SEED | Seeded PRNG streams (room × tick, global × tick, cron × unit) and lodash draw order | rng | G | reference/lodash-3.10.1/lodash.js:2501-2503,6928-6967 |
| CLOCK-VIRTUAL | `vclock(T) = baseMs + (T − baseTick) × nominalTickMs`, default nominalTickMs 1000 | formula | G | reference/driver/lib/index.js:21 |
| CLOCK-POLICY | Per-site decision: virtual clock, real time, or removed | rule | G | §16 |
| CLK-CRON-PERIOD | Cron periods on `vclock`, checked once per tick (sendNotifications excepted) | rule | G | reference/backend/lib/cronjobs.js:14-36 |
| CLK-FORCE-UPDATE | roomsForceUpdate: 20 s gate on `vclock` plus the tick gate `nextForceUpdateTime` | rule | G | reference/backend/lib/cronjobs.js:16,178-192 |
| CLK-ROOM-STATUS | novice / respawnArea / openTime comparisons on `vclock` | rule | G | reference/engine/src/processor/intents/nukes/tick.js:10; reference/engine/src/processor/intents/controllers/tick.js:29,61,68; reference/driver/lib/index.js:434,490-524 |
| CLK-MARKET-EXPIRY | Order `createdTimestamp` and 30-day expiry on `vclock` | rule | G | reference/engine/src/processor/global-intents/market.js:102,136,507 |
| CLK-MARKET-HISTORY | `users.money.date`, 14-day window and day buckets of market stats on `vclock`, UTC | rule | G | reference/backend/lib/cronjobs.js:472-523 |
| CLK-PC-TIMERS | Power-creep spawnCooldownTime, deleteTime, powerExperimentationTime on `vclock` | rule | G | reference/engine/src/processor/global-intents/power/_diePowerCreep.js:57; reference/backend/lib/cronjobs.js:533-554 |
| CLK-NOTIFY | Notification buckets, error buckets, email cron: real time | rule | G | reference/driver/lib/index.js:196-206,397-405,455-471; reference/backend/lib/cronjobs.js:124-176 |
| CLK-PORTAL-WALL | Portal `unstableDate`, wall `decayTime.timestamp` on `vclock` | rule | R | reference/engine/src/processor/intents/portals/tick.js:8; reference/engine/src/processor/intents/constructedWalls/tick.js:39-44 |
| CLK-SIGN | Controller sign `datetime` on `vclock` | rule | R | reference/engine/src/processor/intents/creeps/signController.js:27 |
| CLK-RUNNER-CACHE | 60 s accessibleRooms/roomStatusData and 3 h market-history runner caches removed | rule | U | reference/driver/lib/runtime/data.js:51-97 |
| CLK-VISUAL-TTL | Room visual retention in ticks instead of 5 s | rule | U | reference/driver/lib/runtime/make.js:253-260 |
| CLK-SANDBOX-DATE | Player and runtime `Date` read `vclock` | rule | U | reference/engine/src/game/creeps.js:853; reference/engine/src/game/structures.js:1364-1365 |
| CLK-API | Backend API time checks: openTime virtual, respawn rate limit real | rule | G | reference/backend/lib/game/api/game.js:28,363 |
| CLK-INFRA | Tick pacing, watchdogs, VM idle disposal, process restarts, tick timing: real time or not replicated | rule | G | reference/engine/src/main.js:19-24,100-122; reference/driver/lib/runtime/user-vm.js:160-166 |
| CLK-IDS | Object ids read no clock (the original's `genId` uses `Date.now()`) | rule | G | reference/storage/lib/db.js:317-328 |
| CRON-STAGE | chitin runs crons between ticks (after TICK-INC-TIME), sequentially in `config.cronjobs` key order | phase | G | reference/backend/lib/cronjobs.js:14-36 |
| COMPAT-PROFILES | Target profiles `private` (default) and `mmo`; per-row override | config | G | §19A |
| COMPAT-TABLE | Replicate / fix / configurable decision per defect | rule | G | §19A |
| COMPAT-CRASH-ROOM | Room-scope abort semantics when a crash is replicated | rule | R | reference/engine/src/processor.js:570-583 |
| COMPAT-CRASH-GLOBAL | No global-scope throw is replicated; TICK-ERROR's frozen time with retained `users.intents` is never reproduced | rule | G | reference/engine/src/main.js:94-99; reference/engine/src/processor/global.js:51-62 |
| BUG-DIE-DOUBLE | Creep or power creep dies twice in one tick: two tombstones, store duplicated | compat | R/G | reference/engine/src/processor/intents/creeps/tick.js:93-135; reference/engine/src/processor/global-intents/power/suicidePowerCreep.js:11-16 |
| BUG-INSERT-COUNTER | Pending insert overwritten after removing an earlier pending insert | compat | R | reference/driver/lib/bulk.js:64-89 |
| BUG-DOTKEY-INC | Loki `$inc` with a dotted key writes a flat field: intershard resources never move | compat | G | reference/storage/lib/db.js:264-266; reference/engine/src/processor/global-intents/market.js:435,465 |
| BUG-CRASH-WITHDRAW | withdraw from a target without `store` throws | compat | R | reference/engine/src/processor/intents/creeps/withdraw.js:43 |
| BUG-CRASH-OPEXT | PWR_OPERATE_EXTENSION in a room without controller throws | compat | R | reference/engine/src/processor/intents/power-creeps/usePower.js:108,115 |
| BUG-CRASH-NUKE-PB | Nuke damaging a power bank throws on `nuke.actionLog` | compat | R | reference/engine/src/processor/intents/_damage.js:35-36,86-88 |
| BUG-CRASH-SH-PC | Stronghold deploy over a power creep throws in `_die` | compat | R | reference/engine/src/processor/intents/invader-core/stronghold/stronghold.js:85-86; reference/engine/src/processor/intents/creeps/_die.js:15 |
| BUG-CRASH-CANCEL-SPAWN | cancelSpawning when the spawning creep is missing throws | compat | R | reference/engine/src/processor/intents/spawns/cancel-spawning.js:9-10 |
| BUG-CRASH-REVERSE-LAB | reverseReaction with a lab destroyed earlier in the tick throws | compat | R | reference/engine/src/processor/intents/labs/reverse-reaction.js:28-29,36-37 |
| BUG-CRASH-CALC-SPAWNS | Spawns or extensions in a room without controller throw | compat | R | reference/engine/src/processor/intents/_calc_spawns.js:9,27 |
| BUG-CRASH-FLAG-FIRST | Second user's room intents re-parse a flag doc that has no `data` yet | compat | R | reference/engine/src/processor/intents/room/intents.js:7-10; reference/engine/src/processor/intents/room/create-flag.js:26-30 |
| BUG-CRASH-UNDEF-ROOM | Room intent with roomName `"undefined"` (or a room without terrain) throws | compat | R | reference/engine/src/processor.js:33-34; reference/common/lib/system.js:116-140 |
| BUG-CRASH-MKT-USER | Terminal deal whose order terminal's owner is not loaded throws in the global stage | compat | G | reference/engine/src/processor/global-intents/market.js:331,352; reference/driver/lib/index.js:439-441 |
| BUG-FLAGS-REPARSE | Multi-user flag re-parse loses the earlier user's flag edits | compat | R | reference/engine/src/processor/intents/room/intents.js:7-10,42-52 |
| BUG-DEPOSIT-REGEX | genDeposits regex string escape: only W5N5, W5S5, E5N5, E5S5 sectors | compat | G | reference/backend/lib/cronjobs.js:574 |
| BUG-WORLDSIZE | calcWorldSize starts maxX/maxY at 0 (phantom column/row for one-quadrant worlds) | compat | G | reference/common/index.js:183-193 |
| BUG-SH-HIGHWAY-REGEX | expandStrongholds highway test unanchored (coordinates ≥ 100) | compat | G | reference/backend/lib/strongholds.js:280 |
| BUG-CALC-REWARD | calcReward last item uses the wrong density | compat | R | reference/engine/src/utils.js:677-698 |
| BUG-PC-DEATH-CONTAINER | Power-creep death never fills a container (no `roomObjects` in the global stage) | compat | G | reference/engine/src/processor/global-intents/power/_diePowerCreep.js:26; reference/engine/src/processor/global.js:45-46 |
| BUG-DISMANTLE-NAN | dismantle gives `NaN` energy to creeps whose store has no `energy` key | compat | R | reference/engine/src/processor/intents/creeps/dismantle.js:44 |
| BUG-STATUSDATA-COLD | Fresh world: `roomStatusData` absent; cold runner caches | compat | U | reference/driver/lib/runtime/data.js:77-97; reference/driver/lib/runtime/runtime.js:206-208 |
| ID-SHARD-NAME | `Game.shard.name = os.hostname()` | compat | U | reference/driver/lib/index.js:35-51 |
| ID-PC-SHARD | Power creep `shard` stored `''`, exposed as `undefined` | compat | G/U | reference/driver/lib/index.js:446; reference/engine/src/processor/global-intents/power/spawnPowerCreep.js:25; reference/engine/src/game/power-creeps.js:53 |
| ID-LENGTH | 15-char Loki ids vs the length-24 check of `assertTargetObject` | compat | U | reference/storage/lib/db.js:317-328; reference/engine/src/game/game.js:122-126 |
| ID-FOREIGN-SEG0 | Foreign segment id 0 treated as "no id" | compat | U | reference/driver/lib/runtime/data.js:231; reference/driver/lib/runtime/make.js:236 |
| PERF-PARALLEL | What runs in parallel today | perf | G | §17 |
| PERF-SERIAL | What is serialized today | perf | G | §17 |
| PERF-HOT | Hot paths and per-tick full copies | perf | R/U | §17 |

---

## 2. Process topology and configuration

### TOPO-PROCS
`screeps start` spawns, in this order: `storage` (unless `storage_disabled`), `backend`, `engine_main`, then `runners_cnt` × `engine_runner<i>` and `processors_cnt` × `engine_processor<i>` (reference/launcher/lib/start.js:176-224). Defaults (reference/launcher/lib/start.js:18-27): `processors_cnt 2`, `restart_interval 3600`, `storage_timeout 5000`. `runners_cnt` defaults to 1 (reference/launcher/lib/start.js:206).
- **GOTCHA**: DEFAULTS contains `runners_threads: 2`, but the code reads `opts.runner_threads` (reference/launcher/lib/start.js:21,213). Unless `.screepsrc` sets `runner_threads`, `RUNNER_THREADS` is undefined. Node drops undefined env values, so the driver falls back to `|| 2` (reference/driver/lib/index.js:566). Net default: 2 concurrent isolates per runner.
- All engine processes resolve the driver through `DRIVER_MODULE` (reference/engine/src/utils.js:15-21; reference/launcher/lib/start.js:203,212,221).

### TOPO-RPC
Every DB, env, queue and pubsub call is a length-prefixed JSON frame over one TCP or unix socket per client process to the single storage process (reference/common/lib/rpc.js:5-54,125-146; reference/common/lib/storage.js:91-144). The storage process is single-threaded LokiJS (reference/storage/lib/db.js:12-30). **PERF**: every read and write is a JSON serialize, socket transfer and JSON parse, applied serially. **GOTCHA** (data model): values pass through JSON, so `undefined` keys vanish and `NaN` becomes `null`. Under Mongo, BSON keeps `NaN`.

### TOPO-RESTART
Runner and processor processes get `restartInterval = restart_interval` (reference/launcher/lib/start.js:214,223). After that many seconds the launcher kills them and restarts them (reference/launcher/lib/start.js:139-149). **GOTCHA**: every player's global scope (isolate) is reset at least once per hour on the default private server, regardless of activity. A processor killed mid-room leaves an id in the queue's `processing` list. `whenAllDone` then stalls until TICK-WATCHDOG resets the queues.

### CFG-ENGINE-DEFAULTS
reference/driver/lib/index.js:19-33:

| key | default | used by |
|---|---|---|
| mainLoopMinDuration | 1000 ms | TICK-DELAY |
| mainLoopResetInterval | 5000 ms | TICK-WATCHDOG, RUN-TIMEOUT (ungraceful = `max(5000, this)`), RUN-VISUAL TTL |
| mainLoopCustomStage | `() => q.when()` | TICK-CUSTOM |
| cpuMaxPerTick | 500 | CPU-LIMIT |
| cpuBucketSize | 10000 | CPU-BUCKET-UPDATE |
| customIntentTypes | `{}` | INT-SANITIZE |
| historyChunkSize | 20 | POST-HISTORY |
| useSigintTimeout, reportMemoryUsageInterval, enableInspector | false/0/false | diagnostics |

### CFG-ENV-KEYS
reference/common/lib/storage.js:33-60. Keys used by the loop: `gameTime`, `activeRooms` (set), `tickRate` (min duration), `mainLoopPaused`, `accessibleRooms` (JSON array string), `roomStatusData` (JSON), `memory:<uid>`, `memorySegments:<uid>` (hash), `publicMemorySegments:<uid>`, `roomHistory:<room>` (hash tick→json), `roomEventLog:` (hash room→json), `mapView:<room>`, `roomVisual:<uid>,<room>,<tick>` (with TTL), `terrainData` (zlib+base64 JSON). Pubsub keys: `tickStarted`, `roomsDone`, `queueDone:<name>`, `runtimeRestart`, `setTickRate`, `user:<uid>/console`, `user:<uid>/cpu`.

### CFG-TICKRATE
On `driver.connect('main')` (reference/driver/lib/index.js:118-133), main reads env `tickRate`. If `value && parseInt(value)` is truthy, it sets `mainLoopMinDuration = value` (the raw value, possibly a string). Otherwise it writes the default (1000) back to env. It also subscribes to pubsub `setTickRate` with the same function. CLI `system.setTickDuration(n)`: `parseInt(n)`, rejects NaN or 0, sets env, then publishes `setTickRate` (reference/backend/lib/cli/system.js:61-72). **GOTCHA**: the publish is evaluated eagerly as the argument of `.then(...)` (reference/backend/lib/cli/system.js:69), so it fires before the env write resolves. This is harmless.

### CFG-PAUSE
`system.pauseSimulation()` sets env `mainLoopPaused='1'`; resume sets `'0'` (reference/backend/lib/cli/system.js:29-41). See TICK-PAUSE-CHECK.

---

## 3. Main loop (engine_main)

### TICK-LOOP — exact stage order
reference/engine/src/main.js:11-127. One iteration = one tick. Pseudocode:
```
loop():
  t0 = hrtime; stage = 'start'; emit mainLoopStage('start')
  watchdog = setInterval(queue.resetAll, mainLoopResetInterval)      # TICK-WATCHDOG (Node only)
  try:
    notifyTickStarted()                     # TICK-PAUSE-CHECK; throws 'Simulation paused'
    users = getAllUsers()                   # stage getUsers          (TICK-GET-USERS)
    usersQueue.addMulti(users.map(_id))     # stage addUsersToQueue
    usersQueue.whenAllDone()                # stage waitForUsers      -> RUN-* happens here
    rooms = getAllRoomsNames()              # stage getRooms          (TICK-GET-ROOMS)
    roomsQueue.addMulti(rooms)              # stage addRoomsToQueue
    roomsQueue.whenAllDone()                # stage waitForRooms      -> PROC-* happens here
    commitDbBulk()                          # stage commit1 (no-op)
    processor/global()                      # stage global            (GLOB-*)
    commitDbBulk()                          # stage commit2 (no-op)
    newTime = incrementGameTime()           # stage incrementGameTime
    if newTime > lastAccessibleRoomsUpdate + 20:   # TICK-ACCESSIBLE (fire-and-forget)
        updateAccessibleRoomsList(); updateRoomStatusData(); lastAccessibleRoomsUpdate = newTime
    notifyRoomsDone(newTime)                # stage notifyRoomsDone
    config.mainLoopCustomStage()            # stage custom
  catch e: if e != 'Simulation paused': log("Error while main loop (stage …)")
  finally:
    clearInterval(watchdog); used = ms since t0; emit mainLoopStage('finish')
    delay = getNextTickDelay ? getNextTickDelay(used) : max(mainLoopMinDuration - used, 0)
    setTimeout(loop, delay)
```
- The runner phase and the processor phase never overlap. All users finish (or are reset) before the active-room set is even read (**PERF-SERIAL**).
- Rooms activated *during* the room phase (processor self-activation, global stage) are processed in the **next** tick, because the set was already consumed (ACT-SET).
- Backend crons are not part of this loop in the original (CRON-SCHED). Chitin adds one stage, CRON-STAGE (§16), after `incrementGameTime` and the TICK-ACCESSIBLE refresh and before `notifyRoomsDone`.

### TICK-PAUSE-CHECK
reference/driver/lib/index.js:339-347: `if (+env.mainLoopPaused) reject('Simulation paused')`, otherwise publish `tickStarted` "1". A paused loop runs no stage and re-polls after `max(minDuration - used, 0)`.

### TICK-GET-USERS
reference/driver/lib/index.js:144-151: `users.find({$and:[{active:{$ne:0}},{cpu:{$gt:0}}]})`, then sort by `(b.lastUsedDirtyTime||0) - (a.lastUsedDirtyTime||0)` (descending, most expensive first). NPC users `2` (Invader) and `3` (Source Keeper) have `cpu: 0` in the default DB (reference/storage/db.original.json), so they never run as runners. Their AI runs inside the room processor (PROC-NPC-PRETICK). `active` may be a number or a boolean. `$ne: 0` treats `true` as active. **GOTCHA**: the sort key is the previous tick's measured CPU time, so this order is not reproducible between runs. Chitin uses this query only to select users, never as an order (ORD-POLICY, §16).

### TICK-GET-ROOMS
reference/driver/lib/index.js:170-174: `result = smembers('activeRooms'); del('activeRooms')`. These are two RPCs and not atomic. An `sadd` from another process (backend API or cron) that lands between them is lost. That is a race in the original, not something to copy.

### TICK-EMPTY-ROOMS (GOTCHA, Loki only)
Loki `dbEnvSmembers` returns `undefined` when the key is absent (reference/storage/lib/db.js:750-764), and `del` removes the key (reference/storage/lib/db.js:630-646). The driver's `addMulti(array)` evaluates `array.length` (reference/driver/lib/queue.js:43-48). With no active room, that throws a TypeError. TICK-ERROR then applies: global stage skipped, **game time not incremented**, while users already ran and saved Memory. With Redis, `smembers` returns `[]` and nothing breaks. The Loki case only arises when no room at all was activated, which in practice means no player objects and no cron activations. Chitin: treat a missing set as empty (Redis behaviour).

### TICK-INC-TIME
reference/driver/lib/index.js:416-419: `getGametime()` (`parseInt(env.gameTime)`, reference/common/index.js:65-67), then `env.set(gameTime, t+1)`. Only engine_main writes it, so the read-modify-write is safe. The resolved value (T+1) is logged and passed to `notifyRoomsDone`.

### TICK-ACCESSIBLE
`lastAccessibleRoomsUpdate` starts at 0 (reference/engine/src/main.js:8), so the first tick after main starts always refreshes. The condition is strict `>` (reference/engine/src/main.js:79): the refresh period is **21** ticks, not 20.
- `accessibleRooms` = JSON array of `_id` of `rooms` with `status:'normal'` and (`!openTime || openTime < Date.now()`) (reference/driver/lib/index.js:490-496).
- `roomStatusData` = `{novice:{room:ts}, respawn:{…}, closed:{…}}`. Built from rooms with `novice > now`, `respawnArea > now`, `openTime > now`, or `status:'out of borders'`. Out-of-borders rooms map to `Infinity`, which `JSON.stringify` turns into `null` (reference/driver/lib/index.js:498-524).
- **GOTCHA**: the runtime caches both for 60 s of wall time per runner process (reference/driver/lib/runtime/data.js:77-97). The `WorldMapGrid` is built from `accessibleRooms` **once per isolate lifetime** (reference/driver/lib/runtime/runtime.js:206-208). The runtime's view of room status therefore lags by up to 21 ticks plus 60 s, and the route grid by up to the VM lifetime.
- **GOTCHA** (cold cache): the cache timestamp is set **before** the fetch resolves (reference/driver/lib/runtime/data.js:78-79,89-90). A second user of the same runner process whose run starts while the first fetch is in flight gets the initial value (`undefined` for accessibleRooms, `{}` for roomStatusData). A fresh isolate then throws at `JSON.parse(undefined)` (reference/driver/lib/runtime/runtime.js:207), which is a RUN-TOTAL-ERROR for that user's tick. See BUG-STATUSDATA-COLD.
- Chitin: keep the 21-tick refresh (tick-based, deterministic) and evaluate `openTime`, `novice` and `respawnArea` against `vclock(T+1)` (CLK-ROOM-STATUS). The 60 s runner cache is removed (CLK-RUNNER-CACHE): every user of tick T reads the snapshot last written by this refresh. `WorldMapGrid` is rebuilt when that snapshot changes.

### TICK-WATCHDOG (GOTCHA)
reference/engine/src/main.js:19-24: while a tick is in progress, every `mainLoopResetInterval` (5 s) `queue.resetAll()` empties `pending` and `processing` of every queue and emits `done` (reference/storage/lib/queue.js:93-104). `whenAllDone` resolves and the tick **continues** with users or rooms skipped. Workers still running finish later:
- a late runner `saveUserIntents` writes `rooms.intents` and activates rooms, so those intents are processed in a later tick. If the room was already processed this tick, its doc was cleared and the late intents survive to the next tick. There the user's next-tick intents are `$merge`d (deep-merged) on top of them (INT-SAVE).
- a late processor still executes its bulk writes after the global stage has read the state.

Chitin: ideally avoid the condition (no 5 s stalls). If a hard deadline is ever needed, define deterministic drop semantics. Do not replicate the race.

### TICK-ERROR (GOTCHA)
Any rejection in the chain (reference/engine/src/main.js:94-99) skips the remaining stages, including global and `incrementGameTime`. The `finally` block still schedules the next loop. Users run again at the same `Game.time`.
- A throw **inside** the global stage happens before its final `q.all` (reference/engine/src/processor/global.js:45-62). So no global bulk is executed **and `users.intents` is not cleared** (`clearGlobalIntents` is part of that `q.all`, reference/driver/lib/index.js:326-328). The retained documents are read again by the next global stage together with the new ones, so the same global intents execute again. If the throwing condition is still true, the stage throws again: `Game.time` stays frozen while every room phase keeps writing. This is reachable through BUG-CRASH-MKT-USER.
- Room-phase writes of the aborted tick are already persisted, because each room's `markDone` waits for its bulk writes (PROC-LOOP).
- Chitin never reproduces this (COMPAT-CRASH-GLOBAL, §19A).

### TICK-DELAY
`usedTime` covers the whole tick, including all waiting (reference/engine/src/main.js:106-113). The default mainLoopMinDuration of 1000 ms caps the server at 1 tick/s unless changed. The `getNextTickDelay(usedTime)` hook overrides the formula (reference/engine/src/main.js:117-119).

---

## 4. Work queues

### QUEUE-SEMANTICS
reference/storage/lib/queue.js:24-105 (Loki), reference/driver/lib/queue.js:18-61 (client wrapper):
- `addMulti(name, ids)`: `pending = pending.concat(ids)`, emit `add`. The client returns early for `[]`.
- `fetch(name)`: wait until `pending` is non-empty, `item = pending.pop()`, push it to `processing`, return it.
- `markDone(name,id)`: `_.pull(processing, id)`, emit `done`.
- `whenAllDone(name)`: resolves when `pending` and `processing` are both empty, and publishes `queueDone:<name>`.
- `reset(name)`: clears both lists and emits `done`.
- Queue `users` is renamed to `usersIvm` (reference/driver/lib/queue.js:20-22).

### QUEUE-ORDER (GOTCHA)
Loki `pop()` takes the **last** element (reference/storage/lib/queue.js:32), so users are fetched cheapest-first (the reverse of the descending sort), and rooms run in reverse activation order. Redis (screepsmod-mongo) does `lpush` then `rpoplpush`, which is **FIFO**: most-expensive user first (E:\screeps\server\node_modules\screepsmod-mongo\lib\common\queue.js:68,113). Upstream, that queue polls every 10 ms; the harness copy is patched to 1 ms (same file, lines 53-62).
- Fetch order is not save order. With `RUNNER_THREADS` > 1 (default 2, RUN-LOOP) several users run at once, and their saves reach storage in **completion** order. Completion order depends on each player's measured run time.
- Execution order is unobservable to game logic except through the save order, which becomes the key order of `rooms.intents.users` (INT-ORDER-USERS) and the find order of `users.intents` (GLOB-ORDER).
- **GOTCHA**: the TICK-GET-USERS sort key `lastUsedDirtyTime` is a measured CPU time (CPU-STATS), so even "TICK-GET-USERS order" changes from run to run. It is **not** a deterministic order and chitin must not use it as one. Chitin's rule is ORD-POLICY (§16).

---

## 5. Runner phase (per user)

### RUN-LOOP
`driver.startLoop('runner', fn)` (reference/driver/lib/index.js:558-581) uses a generic-pool with `max = RUNNER_THREADS || 2`. Each acquired slot schedules the next `loop` via `setTimeout(0)` and runs `fn`: `usersQueue.fetch()`, then `runUser(id)`, catch-and-log, then `markDone(id)` (reference/engine/src/runner.js:63-76). So up to `RUNNER_THREADS` users per runner process are in flight. Isolates execute on isolated-vm threads.

### RUN-USER / RUN-SAVE
reference/engine/src/runner.js:9-51. `makeRuntime(userId).then(saveResult, saveResult)`: the rejection value is also passed to `saveResult`. In order:
1. `if r.console`: `sendConsoleMessages` (not awaited).
2. `if r.error`: `sendConsoleError` (not awaited).
3. `promises = []`. `if r.memory`: `saveUserMemory` (rejects if `memory.data.length > 2*1024*1024`, reference/driver/lib/index.js:153-160). `if r.memorySegments`: `hmset`. `if r.interShardSegment`: never true (RUN-INTERSHARD). `if r.intents`: `saveUserIntents`.
4. `q.all(promises)`. A rejection propagates to the runner loop, which only logs it (reference/engine/src/runner.js:73). `markDone` is still called.
- A failure value is `{error}` or `false` (deactivation). `false.console` is `undefined`, so nothing is saved.

### RUN-SKIP (pre-run)
reference/driver/lib/runtime/make.js:54-96 (`getUserData`, before any runtime data is fetched):
```
if user.cpu:                                   # 0/undefined => cpu = Infinity (unreachable: runner only gets cpu>0)
  if user.skipTicksPenalty > 0:  $set {lastUsedCpu:0,lastUsedDirtyTime:0}, $inc {skipTicksPenalty:-1}
                                 reject {type:'error', error:'Your script is temporary blocked …'}
  if user.cpuAvailable < 0:     $set {lastUsedCpu:0,lastUsedDirtyTime:0,
                                      cpuAvailable: cpuAvailable < -2*cpu ? -2*cpu : cpuAvailable + cpu}
                                 reject {type:'error', error:'Script execution has been terminated: CPU bucket is empty'}
```
A skipped user sends a console error and saves no intents or Memory. The user's rooms still run because the processor activates them (ACT-RULE).

### RUN-DATA
reference/driver/lib/runtime/data.js:99-264. Fetch order (four RPC waves):
1. `rooms.objects.find({user})` → `userObjects`. **If empty: `users.update {active:0}` (not awaited) and `reject(false)`** (RUN-DEACTIVATE, lines 103-109).
2. Visible rooms = rooms of every own object whose type is not `flag`/`constructionSite`, plus each own observer's `observeRoom` (lines 113-127). In parallel (lines 129-144): user doc, `users.code {activeWorld:true}`, `memory:<uid>`, `users.console`, gameTime, `rooms` docs of the visible rooms, all **foreign** objects in the visible rooms, `accessibleRooms` (60 s cache), the last 100 outgoing and 100 incoming transactions, own flags, `roomEventLog` for the visible rooms, own power creeps, `roomStatusData` (60 s cache).
3. Console commands are removed from DB as soon as they are read (line 149). The runtime CPU limit is computed here (CPU-LIMIT). Module names are decoded (`$DOT$`, `$SLASH$`, `$BACKSLASH$`, lines 167-176). Users referenced by objects, reservations, signs and transactions are collected (lines 178-198).
4. In parallel (lines 223-239): referenced users, active market orders (cached per gameTime, lines 31-49), market history (cached 3 h, lines 51-75), own orders, active segments, foreign segment.
- `runtimeData.time = gameTime` (line 207). That is `Game.time = T`.
- RUN-REACTIVATE: once `active: 0`, the user is excluded by TICK-GET-USERS until a code upload (reference/backend/lib/game/api/user.js:102) or placing a spawn (reference/backend/lib/game/api/game.js:405) sets `active: 10000`. While deactivated, Memory, console commands and segments are untouched.
- **PERF**: this is a full re-read and a full `ExternalCopy` into the isolate of every visible object, every tick (reference/driver/lib/runtime/make.js:144-145). No incremental state.

### RUN-VM
reference/driver/lib/runtime/user-vm.js:13-101. One isolate per user per runner process (`memoryLimit = 256 + terrainBytes/1MiB` MB, line 30). It is recreated only when `codeTimestamp > cached.codeTimestamp` (strictly greater, lines 23-25), or after it was disposed (heap exhaustion, halt, ungraceful timeout, `runtimeRestart` pubsub, reference/driver/lib/index.js:102-108). Isolates idle for >3 min are disposed once a minute (lines 160-166). The terrain of every room sits in one shared buffer per runner process (reference/driver/lib/runtime/make.js:20-51). **GOTCHA**: with ≥2 runner processes, a user's global may differ between ticks depending on which process picks the user up. That is a documented hazard: `.screepsrc` says multiple runners mean multiple global environments.

### RUN-TIMEOUT
- `game.init(…, timeout = data.cpu = tickLimit, …)` (reference/driver/lib/runtime/runtime.js:216-231; reference/engine/src/game/game.js:448,463). Every `evalCode` (module first-evaluation via `require`, and the `module.exports.loop()` call) runs with `timeout = tickLimit + 5`, minimum 30 ms (reference/driver/lib/runtime/runtime-driver.js:40-45). The limit is applied **per evalCode call**, not per tick. Console commands run with no timeout (reference/engine/src/game/game.js:537-542).
- The outer `run.apply(..., {timeout: data.timeout})` gets `undefined` because `data.timeout` is never set (reference/driver/lib/runtime/make.js:187).
- Ungraceful: `setTimeout(max(5000, mainLoopResetInterval))` around the whole `make()`. It rejects with "Script execution timed out ungracefully, restarting virtual machine", clears the VM and publishes cpu `error` (reference/driver/lib/runtime/make.js:297-318).
- **GOTCHA**: intent CPU (0.2 each) is *not* part of the timeout budget. It only affects `Game.cpu.getUsed()` and the bucket.

### RUN-RESULT / RUN-PARTIAL-ERROR / RUN-TOTAL-ERROR
reference/driver/lib/runtime/runtime.js:233-297:
```
startTime = isolate.cpuTime                      # Game construction (game.init) happens BEFORE this
try game.run(uid)  (require('main'), main.loop(), then console commands)
catch e -> out.type='error', out.error = e.stack || e.toString()    # RUN-PARTIAL-ERROR
if rawMemory._parsed: data.userMemory.data = JSON.stringify(rawMemory._parsed)   # may throw -> TOTAL
segments: >10 keys -> throw; key not int 0..99 -> throw; value not string -> throw; >100*1024 chars -> throw  # TOTAL
out.usedTime = ceil(cpuNow - startTime + intents.cpu); out.usedCleanTime = cpuNow - startTime
out.usedDirtyTime = ceil(cpuNow - startDirtyTime); out.intentsList = intents.list; out.memory = data.userMemory
out.console = {log, results}; out.visual?; activeSegments/activeForeignSegment/defaultPublicSegment/publicSegments
```
- **RUN-PARTIAL-ERROR (GOTCHA)**: a player exception, including the "CPU time limit reached" timeout raised inside `evalCode`, is caught. All intents issued before the throw **and** the Memory object are saved, and the error goes to the console.
- **RUN-TOTAL-ERROR (GOTCHA)**: a throw from Memory stringification (circular structure) or from the segment checks escapes the Reference. `make()` then throws `{error}` (reference/driver/lib/runtime/make.js:279-294) and **the whole tick's intents, Memory and segments are discarded**.
- **RUN-MEMORY-LIMIT (GOTCHA)**: if the serialized Memory is >2 MiB, `saveUserMemory` rejects. The intents promise was already started in the same `q.all` and is still applied. The error reaches only the runner log, not the player console. `RawMemory.set` of >2 MiB throws earlier, inside player code (reference/driver/lib/runtime/runtime.js:113-115).

### RUN-INTERSHARD
`rawMemory.interShardSegment` is a writable property (reference/driver/lib/runtime/runtime.js:125-128) but is never copied into the output. `driver.saveUserMemoryInterShardSegment` does not exist. Nothing is persisted.

### RUN-CONSOLE
reference/driver/lib/index.js:354-410. Users `3` and `2` are only logged to stdout. For everyone else, log lines are HTML-escaped when `item.escape` is set and published on `user:<id>/console`. Errors are published and upserted into `users.notifications` with `type:'error'`, bucketed by `errorsInterval` (default 30 min, wall clock). Chitin keeps real time here (CLK-NOTIFY).

### RUN-SANDBOX-HOOK
The driver's own `playerSandbox` listener defines `Game.shard = {name: os.hostname(), type:'normal', ptr:false}` in every run (reference/driver/lib/index.js:35-52; emitted at reference/driver/lib/runtime/make.js:166-185). Chitin: ID-SHARD-NAME (§19A).

### RUN-VISUAL
For each room key in the run's `visual` output, `env.setex('roomVisual:<uid>,<room>,<T>', mainLoopResetInterval/1000 = 5 s, data)` (reference/driver/lib/runtime/make.js:253-260). The room key is `''` for global visuals and `'map'` for map visuals. The backend reads only tick `gameTime−1` when it pushes after `roomsDone` (reference/backend/lib/game/socket/rooms.js:82-83; reference/backend/lib/game/socket/user.js:102,174). Chitin: CLK-VISUAL-TTL.

---

## 6. CPU and bucket bookkeeping

### CPU-LIMIT
reference/driver/lib/runtime/data.js:151-165:
```
if user.cpu:  bucket = max(0, user.cpuAvailable || 0);  tickLimit = min(bucket + user.cpu, cpuMaxPerTick=500)
else:         tickLimit = bucket = Infinity
```
`Game.cpu = {limit: user.cpu, tickLimit, bucket, getUsed, getHeapStatistics, halt}`, `Game.cpuLimit = tickLimit` (reference/engine/src/game/game.js:145-157). `Game.cpu.bucket` is never negative because of the `max(0, …)`.

### CPU-INTENT-COST
reference/driver/lib/runtime/runtime.js:60-100. `intentCpu = 0.2`, `freeMethods = {say, pull}`.
- `set(id,name,data)`: charge 0.2 only if `!free[name] && !list[id][name]`, then overwrite. Re-issuing the same intent costs nothing extra.
- `push(name,data,maxLen)` / `pushByName(id,name,data,maxLen)`: if `maxLen` is given and the list length is ≥ maxLen, return false (no charge). Otherwise append and charge 0.2.
- `remove(id,name)`: if present, delete it and **refund 0.2 even for free methods** (GOTCHA: `cancelOrder('say')` refunds CPU that was never charged).
- `Game.cpu.getUsed() = cpuTime - startTime + intents.cpu` (lines 202-204).

### CPU-BUCKET-UPDATE
reference/driver/lib/runtime/make.js:98-111,192-212:
```
count   = Σ over list[entity][intentName] (entity includes 'room','global','notify'):
            skip if intentName in {say,pull}; += isArray(v) ? v.length : 1
cpuUsed = ceil(usedCleanTime + 0.2*count)
if user cpu finite: cpuAvailable = min(user.cpuAvailable + user.cpu - cpuUsed, cpuBucketSize=10000)   # no lower bound
$set {lastUsedCpu: usedTime, lastUsedDirtyTime: usedDirtyTime, cpuAvailable, activeSegments?, defaultPublicSegment?}
```
- **GOTCHA**: `count` is recomputed from the final list, not from `intents.cpu`. A `remove` refund therefore has no effect on the bucket. For `notify` (top-level array), the iteration is over array indices, so each entry counts 1.
- `user.cpuAvailable` is the value read at tick start (getUserData). The update is a `$set`, not an `$inc`.
- The bucket can go negative because the timeout allows `tickLimit+5` ms (minimum 30 ms) per evalCode, intents are billed outside the timeout, and `ceil` rounds up.

### CPU-NEGATIVE-SKIP
See RUN-SKIP. From a deficit `d < 0`: if `d < -2*cpu` it is set to `-2*cpu` (no refill that tick); otherwise `d + cpu`. So after clamping, at most 3 ticks are skipped. The reported stats are `lastUsedCpu = 0`, `lastUsedDirtyTime = 0`. Note that `lastUsedDirtyTime` is the sort key of TICK-GET-USERS.

### CPU-STATS
Each run publishes `user:<id>/cpu` `{cpu: usedTime or 'error', memory: memory.data.length}` (reference/driver/lib/runtime/make.js:262-273). Ungraceful timeouts publish `{cpu:'error'}` (line 306).

---

## 7. Intents: format, limits, routing, storage, order

### INT-LIST (runtime format)
`intents.list` (null-prototype objects, reference/driver/lib/runtime/runtime.js:63-100):
```
{ <objectId>: { <intentName>: <args object> },               # via set()
  room:   { createFlag:[…], removeFlag:[…], createConstructionSite:[…], removeConstructionSite:[…], destroyStructure:[…] },  # pushByName('room',…)
  global: { createOrder:[…], cancelOrder:[…], deal:[…], changeOrderPrice:[…], extendOrder:[…],
            spawnPowerCreep:[…], suicidePowerCreep:[…], deletePowerCreep:[…], upgradePowerCreep:[…],
            createPowerCreep:[…], renamePowerCreep:[…] },
  notify: [ {message, groupInterval}, … ] }                    # push('notify', …, 20)
```
Call sites: reference/engine/src/game/{creeps,structures,power-creeps,market,rooms,flags,construction-sites}.js (see the grep list in spec 05 COMMON-INTENT-*).

### INT-OVERWRITE
`set` assigns `list[id][name] = data`. The last call in the tick wins, and earlier args are lost (reference/driver/lib/runtime/runtime.js:71). Different intent names on the same object coexist. Which of them execute is decided by the processor (for example the creep priority table, reference/engine/src/processor/intents/creeps/intents.js:3-31).

### INT-LIMITS
There is **no** global per-tick intent cap anywhere in the reference sources (no `MAX_INTENTS`; grep is empty). The caps are per type:

| intent | cap per tick | source |
|---|---|---|
| notify | 20 (runtime push) and 20 again (driver `_.take`) | reference/engine/src/game/game.js:175; reference/driver/lib/index.js:189-191 |
| cancelOrder, changeOrderPrice, extendOrder | 50 each | reference/engine/src/game/market.js:104,168,187 |
| deal | 10 (`ERR_FULL` beyond) | reference/engine/src/game/market.js:149 |
| createOrder | `MARKET_MAX_ORDERS` (300) minus existing orders | reference/engine/src/game/market.js:90-96 |
| spawn/suicide/delete/upgrade/create/renamePowerCreep | 50 each | reference/engine/src/game/power-creeps.js:187-408 |
| createConstructionSite | `MAX_CONSTRUCTION_SITES` (100) minus own sites | reference/engine/src/game/rooms.js:1071 |
| flags | `FLAGS_LIMIT` (10000) | reference/engine/src/game/rooms.js:984 |

### INT-STORE (routing)
`utils.storeIntents(userId, list, runtimeData, customIntentTypes)` (reference/engine/src/utils.js:405-436), iterating `for i in list` in insertion order:
- `notify` → `intents.notify = sanitizeUserIntents({notify}).notify`.
- `room` → `sanitizeUserRoomIntents(list.room, intents)` (INT-ROOM-GROUP).
- `global` → `intents.global = sanitizeUserIntents(list.global)`.
- otherwise: `object = userObjects[i] || roomObjects[i]`. If missing, the entry is **dropped**. Else `intents[object.room][i] = sanitizeUserIntents(list[i])`. `object.room` is the room at tick start.

### INT-SANITIZE (GOTCHA-dense)
reference/common/lib/system.js:4-114.
- Only names in `intentTypes` (lines 16-82) or `customIntentTypes` survive, in the **declaration order of `intentTypes`**, then custom ones. A name is kept only if `input[name]` is truthy.
- For each kept intent, **every declared field is emitted** through its transform, present or not:
  - `string`: `""+v`. A missing field becomes the string `"undefined"`. Example: `creep.move(direction)` yields `{id:"undefined", direction:n}`, and the processor does `roomObjects["undefined"]`, which is harmlessly undefined (reference/engine/src/processor/intents/creeps/move.js:19-24).
  - `number`: `parseInt(v)`. Missing becomes `NaN`. Through the Loki JSON transport `NaN` becomes `null`; under Mongo it stays NaN. Floats truncate via string conversion: `parseInt(5e-7) === 5`, `parseInt(1e21) === 1`.
  - `boolean`: `!!v`. `price`: `parseInt((1000*v).toFixed(0))`, so prices are stored ×1000. `string[]`/`number[]`: map if an array, else `undefined`. `bodypart[]`: `_.filter` against `BODYPARTS_ALL`. `userString`: `(""+(v||"")).substring(0,100)`. `userText`: `…substring(0,1000)`.
- Array-valued intents (room, global, notify) are sanitized element-wise. Built-in names inside arrays are sanitized without `customIntentTypes` (line 100), which is irrelevant for built-ins.

### INT-ROOM-GROUP
reference/common/lib/system.js:116-140: for each `room` intent array element, `sanitized = sanitizeIntent(name, el)` and `result[sanitized.roomName].room[name].push(sanitized)`. A missing or invalid `roomName` groups under `"undefined"` (or the bad name). That becomes a `rooms.intents` doc and an active-room entry for a non-existent room. The processor then throws on the missing terrain (reference/engine/src/processor.js:33-34, PROC-ERROR). That is harmless because the bogus room has no state. Chitin drops such intents at INT-STORE but still charges their CPU, since CPU-BUCKET-UPDATE counts the runtime list (BUG-CRASH-UNDEF-ROOM, §19A).

### INT-SAVE
reference/driver/lib/index.js:180-243, for each key `room` of the stored intents:
- `notify` → INT-NOTIFY.
- `global` → `users.intents.insert({user, intents: intents.global})`. One document per user per tick, plus any `add-global-intent` API documents.
- otherwise → `rooms.intents.update({room}, {$merge: {users: {[uid]: {objects: intents[room]}}}}, {upsert:true})`, and the room is added to `activeRooms`. One `sadd` covers all of the user's rooms.
- `$merge` is a lodash deep `_.merge` applied at storage (reference/storage/lib/db.js:261-263). It matters only if a user's entry already exists (INT-MANUAL, TICK-WATCHDOG).

### INT-NOTIFY
reference/driver/lib/index.js:185-227: take the first 20. `groupInterval` is clamped to [0,1440] minutes, multiplied by 60000 and floored. `date = groupInterval ? ceil(now/gi)*gi : now`, all **wall clock** (reference/driver/lib/index.js:196-206). `message = (""+m).substring(0,500)`. Then upsert `users.notifications {user,message,date,type:'msg'}` with `$inc count`. `checkNotificationOnline` is a stub that always resolves (reference/driver/lib/index.js:77-79). Chitin keeps real time here (CLK-NOTIFY).

### INT-MANUAL
API `POST /api/game/add-object-intent` sanitizes one intent and `$merge`s it into `rooms.intents.users[uid].objectsManual[objectId]`, then activates the room (reference/backend/lib/game/api/game.js:509-527). The processor does `_.extend(userIntents.objects, userIntents.objectsManual)` (reference/engine/src/processor.js:231-234). Replacement is **per object id**: all script intents of that object for the tick are replaced.

### INT-ORDER-USERS (GOTCHA)
`_.forEach(intents.users, …)` (reference/engine/src/processor.js:229) enumerates `Object.keys` order: **integer-like keys ascending first**, so `"2"` (Invader) and then `"3"` (Source Keeper) always run first, even when injected by PROC-NPC-PRETICK. Then string keys in insertion order. Insertion order is the order in which `$merge`s on that room's doc reached storage (reference/driver/lib/index.js:236). That is runner **completion** order: LIFO fetch (reference/storage/lib/queue.js:32) of a list sorted by a measured CPU time (TICK-GET-USERS), with `RUNNER_THREADS` users in flight per runner process (RUN-LOOP). An `add-object-intent` API call that arrived earlier puts that user's key earlier (INT-MANUAL). The order is non-deterministic in the original. Player ids are 15-hex `genId` strings (Loki) or 24-hex ObjectIds (Mongo). Both are longer than the 10 digits of the largest array index (2^32−2), so even an all-decimal id is never integer-like.
- It matters because structure damage and destruction apply immediately (VIS-DELETED). Example: user A dismantles user B's tower to 0 hits; if A is processed first, B's tower intents are skipped (`roomObjects[towerId]` is gone).
- Chitin: ORD-POLICY point (1) (§16). Integer-like ids first, in ascending numeric order (`"2"`, then `"3"`), then players by ORD-USER-KEY. Do **not** use TICK-GET-USERS order, which is measured CPU time and not deterministic (QUEUE-ORDER).

### INT-ORDER-OBJECTS (GOTCHA)
Within a user, `for (objectId in userIntents.objects)` follows the insertion order created by INT-STORE. That is the order of the **first** `intents.set` per object in the player's script (the null-prototype list keeps insertion order, and later `set`s do not move a key). The `room` pseudo-object is inserted into each room's dict at the position where the player's first `pushByName('room',…)` appears relative to other first-intents. Intent names within one object are dispatched in a fixed per-type order, not call order (INT-DISPATCH).

### INT-OWNERSHIP
reference/engine/src/processor.js:241-248:
```
if objectId == 'room': room/intents(userId, objectIntents, scope); continue
emit preProcessObjectIntents(object, …)          # object may be undefined here
if !object || object._skip || (object.user && object.user != userId): continue
```
Objects **without** a `user` (for example an unowned controller, wall or road) accept intents from any user that names them. The runtime never emits such intents for objects it does not own, but `add-object-intent` can. `_skip` is never set by engine code; it is a mod hook.

### INT-DISPATCH
reference/engine/src/processor.js:250-311. Dispatch is by `object.type`: creep → creeps/intents (fixed action order plus priority table), powerCreep → power-creeps/intents (`move, usePower, withdraw, transfer, say, drop, pickup, enableRoom, renew`), link, tower, lab, spawn → their `intents.js`. Single intents: rampart `setPublic`, terminal `send`, nuker `launchNuke`, observer `observeRoom` (sets `object.observeRoom = roomName` in memory), powerSpawn, invaderCore, factory `produce`, controller `unclaim` then `activateSafeMode`. Then INT-NOTIFY-ATTACKED: if `objectIntents.notifyWhenAttacked && (CONSTRUCTION_COST[type] || creep || powerCreep)`, `bulk.update(object, {notifyWhenAttacked: !!enabled})` (lines 313-315). Then `emit processObjectIntents`.

### INT-CLEAR
After `processRoom` settles (success or error), `rooms.intents.removeWhere({room})`, then `markDone` (reference/engine/src/processor.js:579-583). Global: `users.intents.clear()` is part of the global stage's final `q.all` (reference/engine/src/processor/global.js:61).

---

## 8. Active rooms

### ACT-SET
env set `activeRooms`. `activateRoom(room|rooms)` = `sadd` (reference/driver/lib/index.js:176-178). Loki stores it as an array with no duplicates, in insertion order (reference/storage/lib/db.js:721-748). It is consumed and deleted once per tick at TICK-GET-ROOMS. **Exactly the rooms in the set at that moment are processed this tick.** Nothing else is.

### ACT-SOURCES (complete list in the reference code)
| source | when | cite |
|---|---|---|
| runner `saveUserIntents` | tick T, user phase: every room the user has object or room intents for → processed in **T** | reference/driver/lib/index.js:234-240 |
| processor self-activation | end of room processing in T → processed in **T+1** | reference/engine/src/processor.js:504-506 |
| global inter-room move | global stage of T → destination processed in **T+1** | reference/engine/src/processor/global.js:37-40 |
| backend API: place spawn, create construction site, add-object-intent | any time → next room phase | reference/backend/lib/game/api/game.js:403,525,578 |
| CLI bots spawn | any time | reference/backend/lib/cli/bots.js:123 |
| cron roomsForceUpdate, genPowerBanks, genInvaders, genDeposits, strongholds | wall-clock (CRON-SCHED) | reference/backend/lib/cronjobs.js:184,241,439,640; reference/backend/lib/strongholds.js:127,328 |

### ACT-RULE (processor self-activation)
During PROC-PRESCAN (on the object set **before** intents), `activateRoom = true` if any object matches (reference/engine/src/processor.js:72-140):
1. `invaderCore` with `deployTime` (line 81-83).
2. `controller` reserved by user `'2'` with `reservation.endTime - gameTime < CONTROLLER_RESERVE_MAX - INVADER_CORE_CONTROLLER_POWER*CONTROLLER_RESERVE` (= 5000 − 2·1 = 4998) (lines 108-111).
3. `controller` with `user && user !== '2'` (lines 112-114).
4. any object with truthy `user`, `user != '3'`, `!userNotActive`, `type != 'flag'`, `!strongholdId`, `type !== 'controller'` (lines 119-121). This includes player creeps, structures, construction sites, owned ruins/tombstones, and non-stronghold invader creeps. `userNotActive` is never set by the reference code.
5. `powerBank` or `deposit` with `gameTime > decayTime - 500` (lines 122-127).
6. any `energy` (dropped resource), `nuke`, `tombstone` or `portal` (lines 128-140).
7. after processing: `roomInfo.active` set by an intent or tick handler (invader core tick / attackController), deleted before the roomInfo save (lines 499-502; reference/engine/src/processor/intents/invader-core/tick.js:44; …/attackController.js:39).

If true: `driver.activateRoom(roomId)` (not awaited) and `saveRoomHistory` (POST-HISTORY) (lines 504-507). Flags (separate collection) never activate a room.

### ACT-STALENESS (GOTCHA)
A room not in the set is simply not processed. All time-based logic in it freezes and catches up only partially when it is next processed. The player-observable consequences must be reproduced exactly, which means chitin **must not** "process every room every tick". Examples from the code:
- Depleted source regeneration timer starts only when processed. In an inactive room `ticksToRegeneration` can be negative (reference/engine/src/processor/intents/sources/tick.js:10-29).
- `Room.getEventLog()` of an observed but inactive room returns the stale log of the last processed tick (reference/engine/src/game/rooms.js:573-582; written only by processed rooms, reference/engine/src/processor.js:497).
- Exact-equality time triggers are skipped if the room is not processed on that tick: nuke landing `gameTime == landTime-1` (reference/engine/src/processor/intents/nukes/tick.js:16) and the controller "downgrade in 3000 ticks" notification (reference/engine/src/processor/intents/controllers/tick.js:45).
- Keeper lairs, mineral regeneration, structure decay and similar run only when processed.

### ACT-FORCE-UPDATE
`roomsForceUpdate` cron (period 20 s wall clock): for every room with `status != 'out of borders'` that is not currently in the active set, if `!nextForceUpdateTime || gameTime >= nextForceUpdateTime`, activate it and set `nextForceUpdateTime = gameTime + 90 + floor(random*20)` (reference/backend/lib/cronjobs.js:178-192). The rooms are handled one at a time with awaited writes (`for … await`, lines 182-190). An idle room is woken at the first cron run that finds `gameTime >= nextForceUpdateTime`, so its effective wake period is the first multiple of the cron period that reaches 90..109 ticks. **GOTCHA**: at high tick rates the wall-clock gate dominates (2000 ticks at 100 t/s).
- Chitin: CLK-FORCE-UPDATE (§16). The 20 s gate runs on `vclock`, so at nominalTickMs 1000 the cron runs every 21 ticks (CLK-CRON-PERIOD) whatever the real tick rate. An idle room woken at game time G gets `next = G + 90 + k`. The next runs that can wake it are at G+105 (for k ≤ 15, probability 16/20) and G+126 (for k ≥ 16, probability 4/20). So the room is processed every **105 or 126 ticks**, the same as the reference running at exactly 1 tick/s. **PERF**: the original scans every non-out-of-borders room per run (O(rooms)). Chitin keeps rooms in a min-heap by `nextForceUpdateTime` and touches only due rooms. The RNG draw per woken room comes from that room's cron stream (RNG-SEED).

---

## 9. Room processing (engine_processor)

### PROC-LOOP
reference/engine/src/processor.js:539-594:
```
loop():
  emit processorLoopStage('start')
  roomId = roomsQueue.fetch()
  emit('getRoomData', roomId)
  [intents, {objects, users}, terrain, gameTime, roomInfo, flags] = q.all(6 reads)   # PROC-INPUT
  emit('processRoom', roomId)
  processRoom(roomId, …)                       # promise NOT returned -> not awaited
      .catch(log 'Error processing room')
      .then(clearRoomIntents(roomId))
      .then(roomsQueue.markDone(roomId))
  emit('finish'); setTimeout(loop, 0)
```
- **PERF**: within one processor process, room *computation* is serialized (one JS thread; `processRoom`'s body is synchronous from the pre-scan to `bulk.execute()`). The next fetch overlaps with the previous room's writes. `markDone` waits for that room's bulk writes, mapView, eventLog and roomInfo writes, but **not** for history writes or `activateRoom`.
- `movement.js` keeps module-global state (`matrix`, `objects`, `roomObjects`, `roomTerrain`; reference/engine/src/processor/intents/movement.js:1-67). It is safe only because the body runs synchronously. A parallel port must make movement state per room.

### PROC-INPUT
- `getRoomIntents`: `rooms.intents.findOne({room})`, i.e. `{room, users:{uid:{objects, objectsManual?}}}` or null (reference/driver/lib/index.js:245-247).
- `getRoomObjects`: `rooms.objects.find({room})` → `mapById` (insertion order of the find result), plus `users` docs for every distinct `obj.user` (reference/driver/lib/index.js:249-271).
- `getRoomTerrain`: `rooms.terrain.find({room})` → mapById. The processor takes the first doc's `.terrain` string (reference/engine/src/processor.js:33-36). It is re-read every tick despite being immutable (**PERF**).
- `getGameTime` (T), `getRoomInfo` = `rooms.findOne({_id})`, `getRoomFlags` = `rooms.flags.find({room})` (reference/driver/lib/index.js:273-275,412-423).

### PROC-PRESCAN
reference/engine/src/processor.js:43-171, in `_.forEach(roomObjects)` order:
- creeps: `_actionLog = actionLog`; `_ticksToLive = ageTime - gameTime`; reset `actionLog` to all-null keys {attacked, healed, attack, rangedAttack, rangedMassAttack, rangedHeal, harvest, heal, repair, build, say, upgradeController, reserveController}; collect `keepers` (user `'3'`) and `invaders` (user `'2'` and `!strongholdId`) (lines 48-71; ids at lines 13-14).
- invaderCore: save and reset actionLog {transferEnergy, reserveController, attackController, upgradeController}; remember it as `invaderCore`. Only the last one is kept.
- link {transferEnergy}, lab {runReaction, reverseReaction}, tower {attack, heal, repair}, powerCreep {spawned, attack, attacked, healed, power, say}, factory {produce}: same save-and-reset pattern.
- controller: `scope.roomController = object` (the last controller wins).
- observer: `object.observeRoom = null` (in memory only).
- collect `roomSpawns`, `roomExtensions`, `roomNukes`. Set `hasNewbieWalls` (computed but unused).
- activation conditions (ACT-RULE).
- `emit processObject(object, roomObjects, roomTerrain, gameTime, roomInfo, bulk, bulkUsers)`.

### PROC-NUKE-PRETICK
For each nuke: if `landTime == gameTime + 1`, set every `createCreep` intent of every user in the room to `null` (reference/engine/src/processor/intents/nukes/pretick.js:6-15). This runs before the NPC intents are injected.

### PROC-NPC-PRETICK
reference/engine/src/processor.js:180-219. For each keeper, then each invader, then the invader core (if it has a `user`), the AI returns `{objId: intents}`. Merge rule: `objects[objId] = _.assign(npcIntents, existingStored || {})`. **Stored intents for the same object win key by key** over NPC AI intents. The keeper and invader AIs use the processor's pathfinder (`driver.pathFinder.make({RoomPosition: fakeRuntime.RoomPosition})`, line 174). **PERF**: NPC pathfinding runs inside the processor.

### PROC-CALC-SPAWNS
Only if the room has spawns or extensions (line 221). For spawns, if their count exceeds `CONTROLLER_STRUCTURES.spawn[level|0]`, sort by Chebyshev distance to the controller and the first N stay on. Set `_off` and `bulk.update(id, {off})` when it changed. Same for extensions (reference/engine/src/processor/intents/_calc_spawns.js:6-41). **GOTCHA**: it reads `roomController.level` without a null check (lines 9 and 27, evaluated even when the count is small). Spawns or extensions in a room without a controller throw (PROC-ERROR) every tick. Normal play cannot reach this, because building needs a controller. Chitin: BUG-CRASH-CALC-SPAWNS (§19A).

### PROC-INTENTS
reference/engine/src/processor.js:227-322: per user (INT-ORDER-USERS), per object (INT-ORDER-OBJECTS), INT-OWNERSHIP, INT-DISPATCH, INT-NOTIFY-ATTACKED. During this phase the handlers mutate live objects, push deferred damage and heal, register moves with `movement.add`, and insert or remove objects (VIS-*).

### PROC-ROOM-INTENTS
reference/engine/src/processor/intents/room/intents.js:3-54: parse all room flags (`data.split('|')` then `split('~')`). Then, for this user, in order: `removeFlag[]`, `createFlag[]`, `createConstructionSite[]`, `removeConstructionSite[]`, `destroyStructure[]`, `genEnergy`. Finally re-serialize every flag marked `_modified` (update if it has `_id`, otherwise insert).

### PROC-MOVE-CHECK
`movement.check(roomController && roomController.safeMode > gameTime ? roomController.user : false)` runs once, after all users' intents (reference/engine/src/processor.js:324). Conflict resolution is in the movement spec. The moves are executed per creep inside the tick loop (`movement.execute`, reference/engine/src/processor/intents/creeps/tick.js:50).

### PROC-ENERGY-AVAILABLE
`scope.energyAvailable = Σ store.energy` over spawns and extensions with `!off` (reference/engine/src/processor.js:326). Used by spawn auto-regeneration in the tick loop (reference/engine/src/processor/intents/spawns/tick.js:44).

### PROC-TICK-LOOP
reference/engine/src/processor.js:342-483, `_.forEach(roomObjects)` (PROC-ITER), skipping falsy entries and `_skip`:
1. Type handler. Exactly one matches per type: invaderCore, energy, source, deposit, mineral, creep, powerCreep, spawn, rampart, extension, road, constructionSite, keeperLair, portal, constructedWall, link, extractor, tower, controller, lab, container, terminal, tombstone, ruin, factory, nuke, observer (`bulk.update(object,{observeRoom})`), storage.
2. `EFFECT_COLLAPSE_TIMER` (1002) with `endTime <= gameTime`: remove the object and `return`. No history or mapView entry (lines 412-419).
3. powerBank or deposit with `gameTime >= decayTime-1`: remove. It still falls through to the history and mapView code (lines 421-426).
4. History: every non-flag object goes into `objectsToHistory` (by reference). Creeps and power creeps are deep-copied via `JSON.parse(JSON.stringify())`, with `_actionLog`/`_ticksToLive` removed and a non-public `say` dropped (lines 428-440).
5. mapView: owned objects (except construction sites, newbie walls, ruins, tombstones and public ramparts) go under `mapView[user]`. Otherwise `w` constructedWall, `r` road, `pb` powerBank, `p` portal, `s` source, `m` mineral and deposit, `c` controller, `k` keeperLair, `pb` dropped power (lines 442-481).
6. `emit postProcessObject(object, …, eventLog, mapView)`.

### PROC-SAVE
reference/engine/src/processor.js:489-518: `emit processRoom`. Then `resultPromises = [mapViewSave, bulk.execute, bulkUsers.execute, bulkFlags.execute, saveRoomEventLog]`. **`bulkUsersPowerCreeps` is created but never executed in the room phase** (line 23). Then ACT-RULE item 7, then activation plus history, then `saveRoomInfo` if `!_.isEqual(roomInfo, oldRoomInfo)` (`_.clone` is shallow, so nested mutations of roomInfo are not detected). `roomsStatsSave` is a no-op every 30 s.

### PROC-ERROR (GOTCHA)
A throw anywhere in the synchronous body skips every `execute()`. The room's state changes for the tick are dropped entirely, but fire-and-forget side effects already issued (notifications, `driver.activateRoom` from other paths) remain. Because the activation at line 505 is not reached, **the room drops out of the active set** unless something else re-activates it. Room intents are still cleared.
- Every known reachable throw is listed in §19A (BUG-CRASH-*). Chitin either replicates this outcome (COMPAT-CRASH-ROOM) or applies the row's fixed semantics, depending on the profile.

### PROC-ITER (GOTCHA)
lodash 3 `_.forEach(object)` snapshots `Object.keys(object)` before iterating and reads `object[key]` live (reference/lodash-3.10.1/lodash.js:3064-3079,9671-9678). Consequences:
- keys added **during** a loop are not visited by that loop. Keys added during an earlier phase (the intents phase) **are** visited by the later tick loop.
- keys deleted during a loop yield `undefined` and are skipped by the `if(!object)` guards.
- `Object.keys` order is integer-like keys (canonical array indices up to 2^32−2) ascending first, then string keys in insertion order. Temporary ids of objects inserted this tick are `"1"`, `"2"`, … (BULK-INSERT), so **dropped resources created this tick are iterated first** in the tick loop. `createdStructure<n>` and `_createdConstructionSite<n>` keys are iterated last.
- Base order = order of the `rooms.objects` find result (Open question 1).

---

## 10. Bulk writer and same-tick visibility

One bulk writer per collection per room-processing: `rooms.objects` (`bulk`), `users`, `rooms.flags`, `users.power_creeps` (reference/engine/src/processor.js:20-23). The global stage has its own writers (§12).

### BULK-UPDATE
reference/driver/lib/bulk.js:33-63:
```
update(idOrObj, data):
  if !idOrObj: return
  data = cloneDeep(data)
  for key,value in data where isObject(value):            # arrays and plain objects
      if idOrObj is not an object: throw "can not update an object diff property … without object reference"
      orig = idOrObj[key] || {};  _.merge(orig, value);  data[key] = orig    # merge INTO the live sub-object
  if idOrObj is object: _.merge(idOrObj, data); id = idOrObj._id          # live object updated now (VIS-INMEM)
  removeHidden(data)
  if inserts[id]: _.extend(inserts[id], data) else: _.extend(updates[id] ||= {}, data)
```
- What gets persisted for a top-level object key is the **whole merged value as of the last `update` call** that touched that key. Later in-memory mutations of that key without another `update` are not persisted. An earlier unrecorded in-memory mutation of a sub-key **is** persisted if any later `update` touches the same top-level key, because the merge base is the live value.
- Scalars are set as given. If the live value is a truthy non-object (for example the number 5) and the update value is an object, `_.merge(5, obj)` returns 5 and the object is lost. This is a latent edge.

### BULK-UNDEFINED / BULK-ARRAY-MERGE (GOTCHA)
lodash 3 merge (reference/lodash-3.10.1/lodash.js:2351-2381,2397-2440):
- source values that are `undefined` are skipped (destination kept). JSON transport then drops `undefined` keys from `$set`. **Writing `undefined` is a no-op both in memory and in the DB. Only `null` clears a field.**
- arrays merge **index-wise into the existing array**. A shorter new array leaves the old tail elements in place, unless the code first assigned the new array to the live object (then the merge base is the new array itself).
- a plain object merged over a non-plain value (for example an array) replaces it with `{}` plus the keys.

### BULK-INSERT
reference/driver/lib/bulk.js:64-77: `data = cloneDeep(data); removeHidden(data); if id: data._id = id`. Then `inserts[++insertCnt] = data` and the counter is returned. Mutating the original object later does not change the insert unless `update(sameObjectWith _id == counter)` is called. That is the pattern `_create-energy.js` uses: `obj._id = ""+bulk.insert(obj); roomObjects[obj._id] = obj` (reference/engine/src/processor/intents/_create-energy.js:42-52). The real `_id` is assigned by storage `genId` (BULK-EXECUTE).

### BULK-REMOVE
reference/driver/lib/bulk.js:78-89: if `inserts[id]` exists, delete it **and `insertCnt--`**. Otherwise queue `{op:'remove', id}`. **GOTCHA** (original bug, reachable): after removing a pending insert that is not the newest one, the next insert reuses the counter of a still-pending insert and overwrites it. That object is lost from the DB. If both are dropped resources, it is also replaced in `roomObjects` because `_create-energy` keys them by the counter. Reachable path: a 1-unit drop created in the intent phase decays to 0 in its first tick-loop visit (`amount -= ceil(amount/ENERGY_DECAY)`, reference/engine/src/processor/intents/energy/tick.js:12-19). If another insert (spawned creep, construction site, drop) was queued after it, the drop's `bulk.remove("<n>")` decrements the counter, and the next insert of the tick (for example a tombstone) overwrites the newest pending insert. Drops created this tick are visited first (PROC-ITER), so a later insert is likely. The earlier "no code path known" claim was wrong; specs 07 (CONV-BULK-INSERTCNT) and 11 found this path. Chitin: never reuse ids (BUG-INSERT-COUNTER, §19A).

### BULK-INC-SET-PULL
`inc(obj, key, n)`: if given an object, `obj[key] = (obj[key]||0) + n` (live), then queue `$inc`. `addToSet` and `pull` queue ops only, with no live mutation (reference/driver/lib/bulk.js:90-120). With a string id (for example `bulkUsers.inc(target.user,'gcl',…)`), nothing in memory changes (VIS-USERS-STALE).

### BULK-EXECUTE
reference/driver/lib/bulk.js:121-130: if no ops, resolve. Otherwise `ops = [queued remove/inc/addToSet/pull in call order] ++ [$set per id in first-touch order] ++ [inserts in counter order]`, sent as one `dbBulk`. Storage applies them sequentially; an `update` on a missing doc is a silent no-op (reference/storage/lib/db.js:480-513). **GOTCHA** (harness): screepsmod-mongo builds an **unordered** bulk (`initializeUnorderedBulkOp`, E:\screeps\server\node_modules\screepsmod-mongo\lib\common\_connect.js:185-209), so MongoDB does not guarantee this op order. Chitin follows the Loki order. Consequences: `remove` + `update` of the same id → removed. `inc` followed by `update` of the same key → `$inc` then `$set` of the live value, which already includes the inc. Per-document update application order: `$set, $merge, $inc, $unset, $addToSet, $pull` (reference/storage/lib/db.js:257-293). New `_id` = 4 random hex + `Date.now().toString(16).slice(4)` + 4 random hex (reference/storage/lib/db.js:317-328; **RNG + CLOCK**).

### BULK-HIDDEN
`removeHidden` deletes every key whose first char is `_`, recursively through objects and arrays, plus `$loki` (reference/driver/lib/bulk.js:6-23). Scratch fields (`_actionLog`, `_ticksToLive`, `_damageToApply`, `_healToApply`, `_pull`, `_pulled`, `_oldFatigue`, `_off`, `_upgraded`, `_upgradeBlocked`, `_safeModeActivated`, `_parsed`, `_modified`, …) never reach the DB. Chitin's object schema must keep transient per-tick fields separate from persisted ones.

### BULK-COLLECTION-WIDE
A writer targets a collection, not a room. Room processing can write objects in other rooms. Example: the nuke insert into the target room (reference/engine/src/processor/intents/nukers/launch-nuke.js:42-49). **PERF**: this is the only place the room phase writes another room's objects; everything else cross-room goes through the global stage.

### VIS-INMEM / VIS-DEFERRED
- Handlers mutate live objects in `roomObjects`, directly or through `bulk.update`/`inc`. Every later step of the same room tick (later users and objects in the intents phase, `movement.check`, the tick loop, history) sees the new values.
- Creep and power-creep damage and heal are deferred: `_damageToApply` and `_healToApply` accumulate during intents and are applied in the creep tick (reference/engine/src/processor/intents/_damage.js:16-24; reference/engine/src/processor/intents/creeps/tick.js:118-150). That makes mutual creep combat simultaneous.
- Structure damage is **immediate**: `target.hits -= damage`. At ≤0, `_destroy` removes and deletes it from `roomObjects` at once (reference/engine/src/processor/intents/_damage.js:25-33,46-58; …/structures/_destroy.js:52-53).

### VIS-NEW-OBJECTS (GOTCHA)
| created during intents and added to `roomObjects` → ticked in the same tick | inserted only (invisible until next tick) |
|---|---|
| dropped resources (`_create-energy`; temp id `"<n>"`; **decays `ceil(amount/1000)` in the same tick**, reference/engine/src/processor/intents/energy/tick.js:12-23) | creeps from `createCreep` (reference/engine/src/processor/intents/spawns/create-creep.js:100) |
| construction sites from `createConstructionSite` (`_createdConstructionSite<n>`, reference/engine/src/processor/intents/room/create-construction-site.js:71-74) | tombstones (reference/engine/src/processor/intents/creeps/_die.js:94), ruins (reference/engine/src/processor/intents/structures/_destroy.js:49) |
| structures completed by `build` (`createdStructure<n>`, reference/engine/src/processor/intents/creeps/build.js:287-290) | nukes, keeper-lair creeps, invader-core creeps, power-creep `usePower` inserts |
| stronghold deployed structures (`deployedStructure<n>`) | |
Objects created *inside* the tick loop are never visited by that loop (PROC-ITER).

### VIS-RUNTIME-SNAPSHOT
Runners of tick T read DB state after the room and global writes of T−1 (barriers in TICK-LOOP). Within one tick, a player never sees other players' intents.

---

## 11. Cross-room effects (when they apply)

| effect | room phase (tick T) | global stage (tick T) | visible to players |
|---|---|---|---|
| creep or power creep leaves through an edge | `interRoom = {room, x, y}` set in the creep tick after its move, EVENT_EXIT logged | moved to the new room, `interRoom = null`, destination activated | T+1, in the new room at the mirrored edge |
| portal | `interRoom = portal.destination` (creeps only if `ageTime` set; power creeps only for same-shard portals) | as above | T+1 |
| terminal `send` | validated, `send` field stored on the terminal | transfer, fee, cooldown | T+1 |
| `launchNuke` | nuker drained, nuke inserted into the target room | — | T+1 (nuke object) |
| `observeRoom` | `observeRoom` persisted on the observer | — | T+1 (room visible to the owner) |
| market intents, power-creep global intents | — | executed | T+1 |

### XROOM-EDGE
reference/engine/src/processor/intents/creeps/tick.js:52-78 (power creeps: reference/engine/src/processor/intents/power-creeps/tick.js:16-42). After `movement.execute`, if `x==0`, `y==0`, `x==49` or `y==49` (checked in that order, so corners resolve to the first match) and the user is not `2`/`3`: target = the neighbour room with the coordinate mirrored (0↔49). Then `bulk.update(object, {interRoom})` and `eventLog.push({event: EVENT_EXIT, objectId, data:{room,x,y}})`. The creep stays in this room's `roomObjects` for the rest of the room tick.
- Global: `if !accessibleRooms[target]: return`, otherwise activate the target once and `bulkObjects.update(creep, {room, x, y, interRoom: null})` (reference/engine/src/processor/global.js:33-43). `accessibleRooms` = `rooms {status:'normal'}` with `openTime` passed (`Date.now()`), read fresh each tick (reference/driver/lib/index.js:433-434).
- **XROOM-BOUNCE (GOTCHA)**: in tick T+1 the creep is on the opposite room's edge tile. Unless it moves off in T+1, it is sent back again. Idle creeps on exit tiles alternate rooms every tick.
- **XROOM-INACCESSIBLE (GOTCHA)**: if the target is not accessible, `interRoom` is **not cleared**. If the creep later walks away from the edge, the stale `interRoom` remains, and it teleports if the room ever becomes accessible. This is original behaviour; it is impossible on worlds without status changes.

### XROOM-TERMINAL
Room phase (reference/engine/src/processor/intents/terminal/send.js:6-33): `targetRoomName` must match `^(W|E)\d+(N|S)\d+$`; `resourceType ∈ RESOURCES_ALL`; `amount` truthy and `store[rt] >= amount`. `range = calcRoomsDistance(room, target, true)` and `cost = ceil(amount*(1-exp(-range/30)))`, scaled by `PWR_OPERATE_TERMINAL` if `endTime >= gameTime`. The terminal needs enough energy (cost, plus amount when sending energy). Then `bulk.update(object, {send: intent})`. Cooldown is **not** checked here. Execution is in GLOB-MARKET-SENDS.

### XROOM-NUKE (GOTCHA)
reference/engine/src/processor/intents/nukers/launch-nuke.js:10-49 inserts `{type:'nuke', room: target, x, y, landTime: gameTime + NUKE_LAND_TIME (50000; 100 on PTR), launchRoomName}` into the target room **without activating it**. The target room notices the nuke the next time it is processed (any activation source; ACT-FORCE-UPDATE guarantees one within ~110 ticks or 20 s in the original, and within 126 ticks under CLK-FORCE-UPDATE). From then on the nuke keeps the room active (ACT-RULE item 6). Landing requires processing at exactly `gameTime == landTime-1` (reference/engine/src/processor/intents/nukes/tick.js:16). The nuke is removed at `gameTime >= landTime` (lines 80-83), and immediately if the target room is novice or respawn-area by `Date.now()` (lines 10-14; chitin compares against `vclock(T)`, CLK-ROOM-STATUS). Chitin: activate the target at launch (no observable difference) or replicate exactly.

### XROOM-OBSERVER
Intent phase: `object.observeRoom = intent.roomName` (reference/engine/src/processor.js:284-288). Tick loop: `bulk.update(object, {observeRoom})`, which persists `null` when no intent was issued this tick (PROC-PRESCAN reset) (reference/engine/src/processor.js:403-406). In T+1 the runtime adds `observeRoom` to the visible rooms (reference/driver/lib/runtime/data.js:120-122). The observed room is **not** activated (ACT-STALENESS).

### XROOM-WORLD-WRAP (GOTCHA)
`calcRoomsDistance(a, b, continuous=true)` wraps dx and dy modulo `worldSize` (reference/engine/src/utils.js:644-655). `worldSize` is computed once per process at `connect` from all room names (reference/driver/lib/index.js:135-137). `calcWorldSize` initialises `maxX = maxY = 0`, not −∞ (reference/common/index.js:183-193). A world whose rooms all have x < 0 (only W) or all have y < 0 (only N) therefore gets one phantom column or row. `worldSize` is `max(width, height)`, a square torus even for non-square worlds. Terminal costs, send ranges, market deal sorting, `Game.map.getWorldSize()` (reference/engine/src/game/map.js:269-270, through the isolate's `_worldSize`, reference/driver/lib/runtime/user-vm.js:63) and the runtime map grid (reference/driver/lib/runtime/mapgrid.js:45) all use this value. Chitin: replicate the formula (BUG-WORLDSIZE, §19A). Recompute it when the room table changes, and apply the new value from the next tick in every consumer at once.

---

## 12. Global stage (engine_main, once per tick, after all rooms)

### GLOB-INPUT
`getInterRoom()` (reference/driver/lib/index.js:429-449) runs after every room's writes have completed (each `markDone` follows its room's bulk writes):
`[gameTime, creeps+powerCreeps with interRoom != null, accessibleRooms (rooms status 'normal', openTime < now), all objects of type terminal/powerSpawn/powerCreep (whole world), {orders: all market.orders, userPowerCreeps: all users.power_creeps, userIntents: all users.intents, users: docs of every user referenced by those three, shardName: ''}]`.
- `users` holds only users referenced by orders, power-creep docs or `users.intents` docs (line 440). The owner of an order's **terminal** is not loaded unless it is referenced for one of those reasons, which BUG-CRASH-MKT-USER depends on. `shardName: ''` becomes the spawned power creep's `shard` (ID-PC-SHARD).
- `openTime < Date.now()` here is fresh every tick (line 434), unlike the runtime's 21-tick snapshot (TICK-ACCESSIBLE). Chitin uses `vclock(T)` (CLK-ROOM-STATUS).
**PERF**: whole-world scans every tick, independent of activity.

### GLOB-ORDER
reference/engine/src/processor/global.js:31-62:
1. GLOB-INTERROOM, iterating `creeps` in find order.
2. power (reference/engine/src/processor/global-intents/power.js): for each `users.intents` doc in find order (Loki: insertion order, i.e. runner completion order plus API docs at their arrival; chitin: ORD-POLICY point (2)), for `intentType` in `['spawnPowerCreep','suicidePowerCreep','deletePowerCreep','upgradePowerCreep','createPowerCreep','renamePowerCreep']`, each intent in array order. Then GLOB-POWER-DEATH for every power creep with `gameTime >= ageTime-1 || hits <= 0`. `hits` was already written by the room phase.
3. market (reference/engine/src/processor/global-intents/market.js): GLOB-MARKET-SENDS (terminals with `send`, in find order). Then per `users.intents` doc: createOrder[], changeOrderPrice[], extendOrder[], cancelOrder[], deal[] (collected). Then the terminal deals are sorted ascending by wrapped distance between `targetRoomName` and the order's room. `Array.prototype.sort` is stable in modern V8, so ties keep collection order, which is the `users.intents` doc order. Then the deals execute. Then `directDeals = _.shuffle(directDeals)` (**Math.random**) and execute. Then order maintenance for every order. Chitin: ORD-POLICY points (3) and (4).
- **GOTCHA** (cross-user order effects in the market loop): `changeOrderPrice` sets `order._skip` and mutates `order.price` live (market.js:179-181,211), and `cancelOrder` sets `_cancelled` and `remainingAmount = 0` (market.js:258-261). Another user's `deal` on that order is dropped if that user's doc comes **later** (`_skip` check, market.js:269). If it comes earlier, the deal is collected and later executes at the new price, or with amount 0 after a cancel. Which outcome happens therefore depends on user order.
4. `q.all([bulkObjects, bulkRooms, bulkUsers, bulkMarketOrders, bulkMarketIntershardOrders, bulkUsersMoney, bulkTransactions, bulkUsersResources, bulkUsersPowerCreeps].execute(), clearGlobalIntents())`. The executes run in parallel. `bulkMarketOrders` and `bulkMarketIntershardOrders` are two writers on the **same** collection `market.orders` (reference/driver/lib/index.js:302-308). The market processor also returns a closure that calls a non-existent `driver.clearMarketIntents`. That closure is never invoked (market.js:591-599).
- In-memory visibility inside the global stage: `executeTransfer` updates the destination `store` live and decrements the source through `bulk.update` (live merge), so later sends and deals in the same stage see the new amounts. `user.money` is only decremented live for the buyer of a direct deal. Terminal deals and fees use `bulkUsers.inc(userDoc, …)`, which mutates the live doc because an object is passed. `_justSpawned` on the power spawn allows one power-creep spawn per power spawn per tick (reference/engine/src/processor/global-intents/power/spawnPowerCreep.js:11,41).
- **CLOCK** (chitin: `vclock(T)`, CLK-PC-TIMERS and CLK-MARKET-EXPIRY): power creep spawn and delete cooldowns use `Date.now()` (spawnPowerCreep.js:16; deletePowerCreep.js:20,27; _diePowerCreep.js:57). Order lifetime uses `nowTimestamp - createdTimestamp > MARKET_ORDER_LIFE_TIME` (30 days in ms) (market.js:507). Spawned power creeps get `ageTime = gameTime + POWER_CREEP_LIFE_TIME` (spawnPowerCreep.js:36). A spawned power creep is inserted into `rooms.objects` with its own `_id`, and its room is not activated (the power spawn's owner keeps it active anyway).
- Details of each market and power rule belong to spec 05 (market) and the power-creep spec. Only the timing and order are normative here.

---

## 13. Post-tick outputs

### POST-GAMETIME
Players and processors in tick T both use T. Stored time becomes T+1 only after the global stage. Clients are told `roomsDone(T+1)`. Lifetimes use this: for example a creep dies in its room tick when `gameTime >= ageTime-1` (reference/engine/src/processor/intents/creeps/tick.js:93).

### POST-HISTORY
reference/engine/src/processor.js:504-507,522-536; reference/driver/lib/history.js:7-43:
- only when ACT-RULE fired: `saveRoomHistory(room, objectsToHistory, gameTime)`. It is chained on a **process-global** promise (`currentHistoryPromise`), serializing history I/O across all rooms of that processor, and it is not awaited by the tick.
- `if gameTime % historyChunkSize == 0`: first `upload(room, base = floor((gameTime-1)/20)*20)`, i.e. the previous chunk. Read hash `roomHistory:<room>`. If `data[base]` is missing, return **without deleting** (stale ticks accumulate until the next successful upload). Otherwise produce `{timestamp: Date.now(), room, base, ticks:{base: full, base+k: getDiff(prev, cur)}}` until the first missing tick, `emit saveRoomHistory(room, base, result)`, then `del` the hash.
- then `hmset roomHistory:<room> {gameTime: JSON.stringify(objects)}`. Non-creep objects are stringified at that moment by reference (PROC-TICK-LOOP item 4).
- The diff format is `common.getDiff` (reference/common/index.js:69-136): a removed key becomes `null`, nested objects diff one level deep, a removed object becomes `null`, a new object is stored in full. No gameplay effect; it is for replay and client only.

### POST-MAPVIEW / POST-EVENTLOG / POST-ROOMINFO
Every processed room writes `mapView:<room>` (JSON, PROC-TICK-LOOP item 5) and `roomEventLog` hash field `<room>` = JSON of the tick's `eventLog` array. That field is read by runners in T+1 through `Room.getEventLog`. `rooms` doc `$set roomInfo` happens only if it changed (shallow compare). Unprocessed rooms keep their previous mapView and event log (ACT-STALENESS).

### POST-NOTIFICATIONS
`driver.sendNotification(user, msg)` upserts `users.notifications {user, message, date ≤ now, type:'msg'}` with `$set date = now` and `$inc count` (reference/driver/lib/index.js:455-471). Called from the processor (attack notifications, controller up/downgrade, newbie-wall warnings) and fire-and-forget. Emails are produced by the `sendNotifications` cron through `config.backend.emit('sendUserNotifications', …)`, a no-op without a mod (reference/backend/lib/cronjobs.js:124-176).

### POST-TICK-TIMING
The backend's `roomsDone` subscriber records wall-clock gaps between ticks (last 30). `GET /api/game/tick` returns their minimum (reference/backend/lib/game/api/game.js:722-735). The client socket pushes for rooms, map and user are throttled on `roomsDone` (reference/backend/lib/game/socket/rooms.js:22; …/map.js:15; …/user.js:51).

### POST-STATS
`stats.inc(name, userId, amount)` accumulates into a process-global `roomStatsUpdates[room][user][name]` (reference/driver/lib/index.js:473-482). `roomsStatsSave` is `// TODO` (reference/driver/lib/index.js:484-487), so room stats are **never persisted** on the private server (the counters keep growing in memory). Call sites: energyHarvested, energyConstruction, energyControl, energyCreeps, creepsProduced, creepsLost, powerProcessed (grep `stats.inc` in reference/engine/src/processor/intents). The only per-tick stats that are actually persisted are the user CPU fields (CPU-STATS).

---

## 14. Backend crons feeding the loop

### CRON-SCHED
`setInterval(cronjobs.run, 1000)` (reference/backend/lib/index.js:20). A job runs when `Date.now() - last > period*1000` (reference/backend/lib/cronjobs.js:28-36). Periods in seconds: sendNotifications 60, roomsForceUpdate 20, genPowerBanks 300, genInvaders 300, purgeTransactions 3600, recreateNpcOrders 300, calcMarketStats 3600, deletePowerCreeps 600, genDeposits 300, genStrongholds 300, expandStrongholds 900 (reference/backend/lib/cronjobs.js:14-26). They run concurrently with ticks and write the DB directly (not through bulk writers). They take effect whenever the next tick reads the data.
- `last` (`i[2]`) lives in backend memory and starts at 0 (line 30), so every job runs at the first poll after a backend start. The check is a strict `>` and polling is every 1 s, so the effective period is about P+1 s.
- Jobs started in the same poll are not awaited (`i[1]({utils})`, line 33). Their DB operations interleave.
- Chitin: CLK-CRON-PERIOD and CRON-STAGE (§16).

### CRON-WORLD-GEN
- genPowerBanks: for `rooms {bus:true, status:'normal'}`. The first pass only schedules `powerBankTime = gameTime + round(rand*POWER_BANK_RESPAWN_TIME/2 + 0.75*POWER_BANK_RESPAWN_TIME)`. When due, it rejection-samples a wall tile in [5,44]² with a non-wall neighbour, `power = floor(rand*(MAX-MIN)+MIN)` (+MAX with probability CRIT), inserts the bank and activates the room (reference/backend/lib/cronjobs.js:194-243).
- genInvaders: for normal **inactive** rooms with no invader creep where `Σ source.invaderHarvested >= (room.invaderGoal || INVADERS_ENERGY_GOAL)`, and the sector has an invader core of level > 0. Creates a raid on a random open exit, resets `invaderGoal` (random) and `invaderHarvested`, and activates the room (reference/backend/lib/cronjobs.js:245-441).
- genDeposits, genStrongholds and expandStrongholds similarly insert objects and activate rooms (reference/backend/lib/cronjobs.js:556-643; reference/backend/lib/strongholds.js).
- All of these use `Math.random` (RNG-SITES). The NPC spawn rules themselves belong to the NPC spec. For the pipeline they matter as **asynchronous, wall-clock-driven writers and activators**.
- Tick-based gates inside wall-clock jobs: `powerBankTime` (cronjobs.js:201-210), `nextNpcMarketOrder` (67,119), `nextForceUpdateTime` (183-187), `nextExpandTime` (reference/backend/lib/strongholds.js:255,342). These deadlines are only checked when the job runs, so at high real tick rates they overshoot by up to one job period. Under CLK-CRON-PERIOD the overshoot is at most `periodSec×1000/nominalTickMs + 1` ticks, independent of the real rate.
- Known defects: genDeposits only matches four sector centres (BUG-DEPOSIT-REGEX) and expandStrongholds misclassifies rooms with coordinates ≥ 100 (BUG-SH-HIGHWAY-REGEX).

### CRON-NPC-ORDERS / CRON-OTHER
recreateNpcOrders replaces NPC terminal orders using `Math.random` (reference/backend/lib/cronjobs.js:38-122). purgeTransactions keeps the last 100 per sender and recipient (443-470). deletePowerCreeps applies wall-clock `deleteTime` (533-554). calcMarketStats covers 14 days of `market.sell` (472-523), bucketed by the **server-local** calendar date of `users.money.date` (`getFullYear/getMonth/getDate`, line 480). NPC orders get `createdTimestamp` (line 70) but no `user`, so they never expire (market.js:507). Chitin: CLK-MARKET-HISTORY, CLK-PC-TIMERS.

---

## 15. Mod hooks (moddability baseline)

Original hook points (all synchronous `EventEmitter` emits on `config.engine` unless noted):

| hook | where | args |
|---|---|---|
| `init` | after `driver.connect(type)` | processType (reference/driver/lib/index.js:139) |
| `mainLoopStage` | every main stage | stage[, users or rooms] (reference/engine/src/main.js) |
| `runnerLoopStage` | start, runUser, saveResultStart, saveResultFinish, finish | (reference/engine/src/runner.js:11,20,47,66,75) |
| `processorLoopStage` | start, getRoomData, processRoom, saveRoom, finish | (reference/engine/src/processor.js:491,553-587) |
| `playerSandbox` | before each player run | sandbox {run,set,get,getIsolate,getContext,getGlobal}, userId (reference/driver/lib/runtime/make.js:166-185) |
| `processObject` | pre-scan per object | object, roomObjects, terrain, gameTime, roomInfo, bulk, bulkUsers (reference/engine/src/processor.js:169) |
| `preProcessObjectIntents` | before the ownership filter | object (may be undefined), userId, objectIntents, … (line 246) |
| `processObjectIntents` | after dispatch | (line 318) |
| `postProcessObject` | tick loop per object | …, eventLog, mapView (line 482) |
| `processRoom` | end of room processing | roomId, roomInfo, roomObjects, terrain, gameTime, bulk, bulkUsers, eventLog (line 489) |
| `saveRoomHistory` | chunk upload | roomId, baseTime, chunk (reference/driver/lib/history.js:39) |
| config functions | `mainLoopCustomStage()`, `getNextTickDelay(usedMs)`, `customIntentTypes{name:{field:type}}`, `registerCustomObjectPrototype(type, name, opts)` (stringified functions evaluated in the isolate, reference/driver/lib/runtime/runtime.js:34-44) | reference/driver/lib/index.js:19-73 |
| backend | `config.cronjobs[name] = [periodSec, fn]` (replaceable) | reference/backend/lib/cronjobs.js:14 |

xxscreeps (comparison, for chitin's mod API): per-object `PreTick`/`Tick` symbols, `registerRoomTickProcessor`, `registerIntentProcessor` with priority and a conflict mask computed from declared constraints, `registerShardTickProcessor` (global stage), `registerShardInitializer`, and a context with `setActive()`/`wakeAt(time)`/`sendRoomIntent(room, …)`/`task()` (reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:34-65,94-189; …/processor/symbols.ts:7-31; …/processor/shard.ts:5-28).

---

## 16. Ordering, randomness, wall clock

### ORD-ROOMS
Rooms are independent within a tick, with these exceptions, so their processing order and placement across processes do not affect results:
- (a) the nuke insert into another room (XROOM-NUKE; inserted, never read by the target in the same tick);
- (b) `bulkUsers` `$inc`/`$addToSet`/`$pull`, which commute;
- (c) process-global accumulators (stats, history chain), which do not affect gameplay;
- (d) notifications.
Room order is therefore not observable. Chitin can process active rooms in parallel in any order, as long as each room's internal order follows §9.

### ORD-KEYS
See PROC-ITER and INT-ORDER-*. Chitin needs an ordered map per room whose iteration is "temp-numeric ids ascending, then insertion order", or an equivalent that reproduces the same visit order for all three loops (pre-scan, intents, tick).

### ORD-POLICY (normative): one deterministic order rule
At several points the original's order is not fixed: it depends on runner timing, measured CPU time, or `Math.random`. Chitin applies **one** rule at all of them. Specs 05 (seeded RNG), 06 (PROC-USER-ORDER), 07 (CONV-ORDER, Open question 3) and 08 (ORDER-UINTENTS) refer to this section and do not choose an order themselves.

#### ORD-USER-KEY
Each player `uid` gets a rank key for tick T (the tick's `Game.time`):
```
orderKey(T, uid) = seed("user-order", T, fnv1a64(utf8(uid)))      # RNG-SEED; a key, not a stream
player order     = ascending (orderKey, uid as UTF-8 bytes)
```
- One function serves every point, so the relative order of two players is the same everywhere in the tick and no global list has to be built. **PERF**: each room sorts only the k users that have intents in it (O(k log k), room-local). The global stage sorts the `users.intents` docs once.
- The key changes every tick, so no player is permanently first. In the original the order was also effectively arbitrary from tick to tick (QUEUE-ORDER). A fixed order, for example by `_id`, would give one player a permanent edge in every same-tick conflict.
- NPC ids are not ranked (point 1 below).
- Test hook: a differential harness may replace `orderKey` with an explicit per-tick list (ORD-REACHABLE).

#### ORD-POINTS — where order matters, and the rule used
| # | point | original order | chitin order | source |
|---|---|---|---|---|
| 1 | Room intents loop `_.forEach(intents.users)` | integer-like keys ascending (`"2"` Invader, `"3"` Source Keeper, including the NPC intents injected by PROC-NPC-PRETICK), then `$merge` arrival order | **integer-like user ids first, ascending by numeric value** (canonical array-index strings ≤ 2^32−2; in the reference only `"2"`, then `"3"`), then players by ORD-USER-KEY. Implement this explicitly; never rely on map key order. A user present only through `objectsManual` (INT-MANUAL) is ranked the same way. | reference/engine/src/processor.js:180-219,229; reference/driver/lib/index.js:236 |
| 2 | Power-creep global intents, `userIntents.forEach` | `users.intents` find order = insertion order: each runner doc when that user's save completes, API docs (INT-GLOBAL-API) when they arrive | docs ordered by owner rank (ORD-USER-KEY). Within one owner: API docs in arrival order, then the runner doc. **No NPC-first rule**: NPCs have `cpu: 0`, never run (TICK-GET-USERS), and have no `users.intents` docs. | reference/engine/src/processor/global-intents/power.js:17; reference/driver/lib/index.js:230,439 |
| 3 | Market intents, `userIntents.forEach` (createOrder, changeOrderPrice, extendOrder, cancelOrder, deal collection), and **ties** of the terminal-deal distance sort | same doc order as (2). The stable sort keeps collection order on equal distance. | same doc order as (2), then a **stable** sort by wrapped distance. Equivalent sort key: `(distance, ownerRank, docIndex, dealIndex)`. No NPC-first rule. Cross-user effects through `_skip` and cancel: GLOB-ORDER. | reference/engine/src/processor/global-intents/market.js:105,265-284,289 |
| 4 | Direct deals `_.shuffle(directDeals)` | `Math.random` | shuffle the list collected in (3) order with the global stream `seed("global", T)` (RNG-SEED), using the lodash draw pattern | reference/engine/src/processor/global-intents/market.js:400 |
| 5 | Every other randomness site (RNG-SITES) | `Math.random` | the room stream of (room, T), the global stream of T, or the cron stream of (job, G, unit) (RNG-SEED) | §16 RNG-SITES |
| 6 | Object-order loops (not player order): room object base order (PROC-ITER), terminal sends (`_.filter(terminals, …)`), power-creep death sweep, inter-room moves, order maintenance | storage find order | creation order of the object or order, the same assumption as Open question 1 | reference/engine/src/processor/global-intents/market.js:71,497; reference/engine/src/processor/global-intents/power.js:30-36; reference/engine/src/processor/global.js:33 |

API intents that arrive while tick T is running are applied in tick T+1. In the original that corresponds to an arrival during the TICK-DELAY sleep before T+1.

#### ORD-REACHABLE (normative statement): any fixed player order is indistinguishable from some original run
Reasoning from the code:
1. Use one runner process with `runner_threads = 1`. This is a `.screepsrc` setting; the user's harness runs it (E:\screeps\server\.screepsrc). The generic pool then has one slot (reference/driver/lib/index.js:558-581). The next `fetch` waits until the previous user's `runUser` has resolved, including `saveResult` → `saveUserIntents` (reference/engine/src/runner.js:9-51,63-76). Users therefore save strictly one after another. The key order of every room's `rooms.intents.users` and the insertion order of `users.intents` both equal the fetch order.
2. The fetch order is the TICK-GET-USERS sort reversed (Loki LIFO, reference/storage/lib/queue.js:32), or as sorted (Redis FIFO). The sort key `lastUsedDirtyTime` is the previous tick's measured CPU time (reference/driver/lib/runtime/make.js:196-214), which each player controls by how much CPU it burns. So **every strict order of the players is the fetch order of some reachable tick**.
3. Integer-like ids come first in every run because of JS key order (ORD-KEYS). The NPC-first part of point (1) is therefore not a choice. It is the only order fact every original run shares.
4. With more runner threads or processes the original order is scrambled further. In one tick it can even differ between rooms, and between rooms and `users.intents`. A single consistent order is one of those outcomes.

So a chitin tick processed in order P has the outcome of the original tick in which the players' previous CPU times sorted to P.
- Consequence for parity tests: user order is reachable per tick but not identical between runs. A differential test either replays the reference's order or compares only order-insensitive outcomes. To replay: run the reference with one runner thread, record each tick's fetch order from the `runnerLoopStage('runUser', userId)` events (reference/engine/src/runner.js:11), and inject it through the ORD-USER-KEY test hook.
- Randomness: V8's `Math.random` is unseeded, so a seeded stream gives one possible sequence. Parity at RNG sites is distributional.

### RNG-SITES (complete list of randomness reachable from the pipeline)
Draw counts follow lodash 3.10.1: `baseRandom(min,max) = min + floor(random()*(max-min+1))` (reference/lodash-3.10.1/lodash.js:2501-2503). `_.shuffle(a)` makes exactly `a.length` draws (`sample(a, ∞)`, a Fisher–Yates pass that also draws for the last index). `_.sample(a)` makes 1 draw, or 0 when `a` is empty (lodash.js:6928-6967). A chain `_(a).shuffle().take(k)` still shuffles all of `a`.

Room phase (room stream of (room, T), consumed in program order):
- mineral regeneration: the draw for `Math.random() < MINERAL_DENSITY_CHANGE` happens only when the density is neither LOW nor ULTRA (`||` short-circuit). If a change happens, there is one draw per re-roll until the density differs (reference/engine/src/processor/intents/minerals/tick.js:19-31).
- `room/gen-energy` position, 2 draws per attempt in a rejection loop. Only user `'3'` passes the check, which makes it legacy (reference/engine/src/processor/intents/room/gen-energy.js:8-19).
- invader core destroy: `_.sample` per array reward slot, then `calcReward` (reference/engine/src/processor/intents/invader-core/destroy.js:30-33).
- `utils.calcReward`: `_.shuffle(range(n))` makes n draws, where n is the number of candidate resources. `order` is then cut to `itemsLimit`, and there are `len(order) − 1` more draws (reference/engine/src/utils.js:677-698). It is called by core destroy and by the stronghold container fill.
- stronghold: decay duration at deploy (stronghold.js:27), container rewards (stronghold.js:125), `towerRefillChance[level] < random()` with one draw each call (stronghold.js:153), bunker4 population `_(deck8).shuffle().take(4)` (stronghold.js:451), and bunker5 population: two shuffles of a 17-card deck (stronghold.js:468,477). All paths are under reference/engine/src/processor/intents/invader-core/stronghold/.

Global stage (global stream of T): `_.shuffle(directDeals)` (reference/engine/src/processor/global-intents/market.js:400).

Backend crons (cron stream of (job, G, unit), §14). The unit is the subject of the job's outer loop:
- recreateNpcOrders: unit = terminal room (cronjobs.js:71-72,88,102,119).
- roomsForceUpdate: unit = room (187).
- genPowerBanks: unit = room (199,214-215,227-228).
- genInvaders: unit = room (288 creep-name suffix, 328-369 raid composition, 429-435 exit and next goal).
- genDeposits: unit = sector centre (605,617-618).
- genStrongholds: unit = sector centre (reference/backend/lib/strongholds.js:61,63,65,157).
- expandStrongholds: unit = invader core `_id` (strongholds.js:261, and `findFreePos` via strongholds.js:305 → reference/backend/lib/utils.js:334-335).

Not part of the tick:
- storage `genId`, 2 draws per insert (reference/storage/lib/db.js:317-328). Chitin ids use no RNG and no clock (ID-LENGTH).
- CLI world-building and bot spawning (reference/backend/lib/cli/map.js, reference/backend/lib/cli/bots.js), which are admin commands; seed them per command.
- player-side `Math.random` (spec 02).

### RNG-SEED (normative)
All values are u64 with wrapping arithmetic.
```
fnv1a64(bytes): h = 0xcbf29ce484222325; for b in bytes { h = (h ^ b) * 0x100000001b3 }
mix64(z):       z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9; z = (z ^ (z >> 27)) * 0x94d049bb133111eb; z ^ (z >> 31)
seed(tag, parts…):  h = mix64(worldSeed ^ fnv1a64(utf8(tag))); for p in parts { h = mix64(h ^ p) }
                    # p is a tick as u64, or fnv1a64(utf8(string)) for names
stream(h):      xoshiro256** whose 4 state words are successive SplitMix64 outputs from x = h
                (x += 0x9e3779b97f4a7c15; out = mix64(x))
random():       (next() >> 11) as f64 * 2^-53          # [0,1), replaces Math.random()
```
| stream | seed | consumers |
|---|---|---|
| room | `seed("room", T, fnv1a64(roomName))` | every room-phase RNG site, in program order |
| global | `seed("global", T)` | ORD-POINTS (4) |
| cron | `seed("cron", G, fnv1a64(jobName), fnv1a64(unitKey))` | the job's draws for that unit, in program order. G is the game time the job sees (CRON-STAGE). |
| user order | `seed("user-order", T, fnv1a64(uid))` | ORD-USER-KEY |
- `worldSeed` is a u64 stored in world state. It is chosen at world creation, and tests pin it.
- **PERF**: streams are independent, so rooms and cron units can run in parallel with deterministic results. There is no shared RNG lock and no draw-order coupling between rooms.
- **GOTCHA**: draw counts must match lodash exactly (RNG-SITES), otherwise later draws in the same stream shift.

### CLOCK-WALL (GOTCHA): every wall-clock read
Every `Date.now()` / `new Date()` read in the reference pipeline, grouped by package. Each site maps to a CLOCK-POLICY row.
- engine processor:
  - novice and respawnArea: reference/engine/src/processor/intents/nukers/launch-nuke.js:22; …/nukes/tick.js:10; …/controllers/tick.js:29,61,68; …/controllers/unclaim.js:25.
  - portal `unstableDate`: …/portals/tick.js:8.
  - wall `decayTime.timestamp`: …/constructedWalls/tick.js:40.
  - controller sign `datetime`: …/creeps/signController.js:27.
  - market `nowTimestamp` and `users.money.date`: reference/engine/src/processor/global-intents/market.js:102,155,193,234,348,369,439,452,469,485,513.
  - power-creep timers: …/global-intents/power/spawnPowerCreep.js:16; …/_diePowerCreep.js:57; …/deletePowerCreep.js:20,27.
  - `roomsStatsSave` gate: reference/engine/src/processor.js:513-515.
- engine runtime (runs inside the isolate):
  - novice check of claimController: reference/engine/src/game/creeps.js:853.
  - novice and respawnArea checks of launchNuke: …/game/structures.js:1364-1365.
  - `spawnCooldownTime` exposure: …/game/power-creeps.js:54.
  - console module name: …/game/game.js:540.
- engine main: tick timing, reference/engine/src/main.js:13,112.
- driver:
  - notification buckets: reference/driver/lib/index.js:196-206,397-405,455-471.
  - openTime: index.js:434,493.
  - roomStatusData: index.js:498-524.
  - history chunk timestamp: …/history.js:22.
  - runner caches: …/runtime/data.js:51-97.
  - VM idle disposal: …/runtime/user-vm.js:100,162.
  - visual TTL: …/runtime/make.js:253-260, enforced by the storage TTL sweeper (reference/storage/lib/db.js:295-315).
- storage: object ids, reference/storage/lib/db.js:323.
- backend:
  - cron scheduler: reference/backend/lib/cronjobs.js:30-32.
  - NPC order `createdTimestamp`: cronjobs.js:70.
  - email notifications: cronjobs.js:127-171.
  - market stats window: cronjobs.js:474.
  - power-creep deletion: cronjobs.js:534.
  - stronghold novice checks: reference/backend/lib/strongholds.js:250,284.
  - API: reference/backend/lib/game/api/game.js:28,363,728-734; …/api/power-creeps.js:79,85,229.
  - auth, online status, messages, badges: reference/backend/lib/authlib.js, …/game/socket/server.js, …/game/api/user-messages.js, …/game/api/user.js.

Nothing in the reference ever **writes** `novice`, `respawnArea`, `unstableDate`, `decayTime.timestamp` or `lastRespawnDate`; they are only read (grep over reference/backend, engine, driver). `openTime` is written only by the CLI `map.openRoom(room, timestamp)` (reference/backend/lib/cli/map.js:568-580). The private server's room-status timestamps therefore all come from admin tools or imported data.

### CLOCK-VIRTUAL (normative)
```
vclock(T) = clock.baseMs + (T − clock.baseTick) × clock.nominalTickMs        # epoch ms, i64
```
- `nominalTickMs` defaults to **1000**, the reference's default tick duration `mainLoopMinDuration` (reference/driver/lib/index.js:21). With the default, chitin's game-time behaviour equals the reference running at exactly 1 tick/s, however fast chitin runs. For an MMO-like profile, set it to the target shard's tick duration (no value is in the reference sources).
- `baseMs` and `baseTick` are persisted world state. At world creation they are set to (real now, current T). If `nominalTickMs` is changed at tick T, set `baseMs = vclock(T)` with the old rate and `baseTick = T`, then switch the rate. The clock stays continuous and monotonic.
- Which value each phase uses:
  - processors and the global stage of tick T: `vclock(T)`.
  - TICK-ACCESSIBLE and CRON-STAGE (after the increment): `vclock(T+1)`.
  - backend API handlers: `vclock(G)`, where G is the stored game time when the request arrives.
  - player runs: CLK-SANDBOX-DATE.
- Admin tools that take an absolute real date store `vclock(G) + (date − realNow)`. Tools that take durations store `vclock(G) + duration`.
- Consequence at chitin's target rates: at 300 ticks/s real, one virtual day passes in 288 s of real time, and a 30-day market order lives 2.4 h of real time. The reference at its nominal 1 tick/s would show the same per-tick dynamics.

### CLOCK-POLICY (normative): per-site decision table
"virtual" = compare against or store `vclock` (CLOCK-VIRTUAL). "real" = host wall clock. Tick counts are given at the default nominalTickMs = 1000.

| ID | site | source | chitin | ticks at default | notes |
|---|---|---|---|---|---|
| CLK-CRON-PERIOD | Cron scheduler `now − last > period·1000`, all jobs except sendNotifications | reference/backend/lib/cronjobs.js:14-36 | **virtual**, evaluated once per tick in CRON-STAGE with `now = vclock(T+1)`. `last[job]` is kept in memory, starts at 0, and is not persisted, so every job runs at the first CRON-STAGE after start, as at a backend start. | a job with period P s runs every P+1 ticks: roomsForceUpdate 21; genPowerBanks, genInvaders, recreateNpcOrders, genDeposits, genStrongholds 301; deletePowerCreeps 601; expandStrongholds 901; purgeTransactions, calcMarketStats 3601 | NPC density per tick equals the reference at 1 tick/s, whatever chitin's real rate. Real time would make NPC spawning, power banks, deposits and strongholds hundreds of times rarer per tick at 300 ticks/s. |
| CLK-FORCE-UPDATE | roomsForceUpdate's 20 s gate on top of the tick gate `nextForceUpdateTime = G + 90 + floor(r·20)` | reference/backend/lib/cronjobs.js:16,178-192 | **virtual** period (CLK-CRON-PERIOD); tick gate unchanged | an idle room is processed every 105 ticks (p = 16/20) or 126 ticks (p = 4/20) | ACT-FORCE-UPDATE |
| CLK-ROOM-STATUS | novice, respawnArea and openTime comparisons | reference/engine/src/processor/intents/nukes/tick.js:10; …/nukers/launch-nuke.js:22; …/controllers/tick.js:29,61,68; …/controllers/unclaim.js:25; reference/backend/lib/strongholds.js:250,284; reference/driver/lib/index.js:434,490-496,498-524; reference/engine/src/game/creeps.js:853; reference/engine/src/game/structures.js:1364-1365; reference/backend/lib/game/api/game.js:28 | **virtual** | a timestamp D ms ahead expires after D/1000 ticks | Processor, global stage, runtime and API must use the same clock, or a room can be novice in the runtime but not in the processor. The runtime gets it through CLK-SANDBOX-DATE. |
| CLK-MARKET-EXPIRY | `createdTimestamp = nowTimestamp` at createOrder and for NPC orders; expiry `now − createdTimestamp > MARKET_ORDER_LIFE_TIME` (30 d, reference/common/lib/constants.js:375) | reference/engine/src/processor/global-intents/market.js:102,136,507; reference/backend/lib/cronjobs.js:70 | **virtual** (`vclock(T)` in the global stage, `vclock(G)` in crons) | 2,592,000 | NPC orders never expire (no `user`). |
| CLK-MARKET-HISTORY | `users.money.date`; calcMarketStats' 14-day window and calendar-day buckets | reference/engine/src/processor/global-intents/market.js:155 (and the other `date: new Date()` lines in CLOCK-WALL); reference/backend/lib/cronjobs.js:472-523 | **virtual**; day buckets use the **UTC** date of `vclock` (the original uses the server's local time zone, line 480) | 1 day = 86,400 | `Game.market.getHistory` dates advance with game time |
| CLK-PC-TIMERS | `spawnCooldownTime` (set at death, checked at spawn, exposed to players), `deleteTime` (set by intent and API, executed by cron), `powerExperimentationTime` | reference/engine/src/processor/global-intents/power/_diePowerCreep.js:57; …/spawnPowerCreep.js:16; …/deletePowerCreep.js:20,27; reference/engine/src/game/power-creeps.js:54; reference/backend/lib/cronjobs.js:533-554; reference/backend/lib/game/api/power-creeps.js:79,85,229 | **virtual** | POWER_CREEP_SPAWN_COOLDOWN 8 h = 28,800; POWER_CREEP_DELETE_COOLDOWN 24 h = 86,400, plus up to 601 ticks of deletePowerCreeps lag; experimentation 24 h = 86,400 (reference/common/lib/constants.js:810-811) | `PowerCreep.spawnCooldownTime` and `deleteTime` are ms timestamps that bots compare with `Date.now()`. The official docs' own example does exactly that (`!(…spawnCooldownTime > Date.now())`, reference/docs/api/source/PowerCreep.md:139-140), and it stays consistent under CLK-SANDBOX-DATE. |
| CLK-NOTIFY | `Game.notify` groupInterval buckets, error buckets (`errorsInterval`), `sendNotification` date, sendNotifications job (60 s) and its `lastNotifyDate` email interval | reference/driver/lib/index.js:196-206,397-405,455-471; reference/backend/lib/cronjobs.js:15,124-176 | **real** (sendNotifications runs on its own real-time 60 s timer, not in CRON-STAGE) | n/a | These are human-facing and have no simulation effect. Virtual time at high rates would collapse the grouping and flood email. Excluded from parity tests. |
| CLK-PORTAL-WALL | portal `unstableDate` → `decayTime = T + PORTAL_DECAY`; wall `decayTime.timestamp` removal | reference/engine/src/processor/intents/portals/tick.js:8-13; reference/engine/src/processor/intents/constructedWalls/tick.js:39-44 | **virtual** (`vclock(T)`) | D ms ahead = D/1000 ticks | Nothing in the reference writes these fields (imported or official data only) |
| CLK-SIGN | controller `sign.datetime` | reference/engine/src/processor/intents/creeps/signController.js:27 | **virtual** (`vclock(T)`) | — | Player-visible. Keeps object state reproducible. |
| CLK-RUNNER-CACHE | accessibleRooms 60 s and roomStatusData 60 s per runner process; market history 3 h; `WorldMapGrid` once per isolate | reference/driver/lib/runtime/data.js:51-97; reference/driver/lib/runtime/runtime.js:206-208 | **removed**. The runtime of tick T reads the latest snapshot: the TICK-ACCESSIBLE output (its 21-tick cadence is kept), the latest calcMarketStats result, and a map grid rebuilt whenever the accessibleRooms snapshot changes. The market-orders cache is keyed by `gameTime` (data.js:31-49) and is already per tick, so it stays one snapshot per tick. | 0 extra staleness | Divergence: the original's extra staleness (up to 60 s, 3 h, or the isolate lifetime) depends on runner-process topology and timing, and is not reproduced. This also removes the BUG-STATUSDATA-COLD errors. |
| CLK-VISUAL-TTL | room visual TTL `mainLoopResetInterval/1000` s | reference/driver/lib/runtime/make.js:253-260 | **tick-based retention**: visuals of tick T are kept until tick `T + max(1, ceil(mainLoopResetInterval / nominalTickMs))` | 5 | The backend reads only tick `gameTime−1` (RUN-VISUAL), so at least 1 tick is required. **PERF**: a 5 s real TTL at 300 ticks/s would hold about 1500 ticks of visuals per user and room. |
| CLK-SANDBOX-DATE | player `Date`, and engine runtime code that runs inside the isolate (novice checks, `spawnCooldownTime` exposure, console module name) | reference/engine/src/game/creeps.js:853; reference/engine/src/game/structures.js:1364-1365; reference/engine/src/game/power-creeps.js:54; reference/engine/src/game/game.js:540 | **virtual**: during the run of tick T, `Date.now() = vclock(T) + min(realMsSinceRunStart, nominalTickMs − 1)`; time zone UTC | `Date` advances nominalTickMs per tick | Every server timestamp a bot can read is virtual (CLK-ROOM-STATUS, CLK-MARKET-EXPIRY, CLK-PC-TIMERS, CLK-SIGN), so the sandbox clock must be too, or comparisons such as `pc.spawnCooldownTime > Date.now()` (the docs' own example, reference/docs/api/source/PowerCreep.md:139-140) drift with the real tick rate. Within one run, deltas are real ms (useful for profiling). The clamp keeps `Date` monotonic across ticks. Spec 02 JS-DATE must follow this row. |
| CLK-API | backend API checks | reference/backend/lib/game/api/game.js:28 (openTime: **virtual**), :363 (180 s respawn rate limit: **real**, anti-abuse and human-facing); power-creep API (CLK-PC-TIMERS); auth tokens, online status, messages, badges (**real**) | as listed | — | API handlers use `vclock(G)` (CLOCK-VIRTUAL) |
| CLK-INFRA | tick pacing (`mainLoopMinDuration` / `tickRate`), queue watchdog, ungraceful 5 s timeout, isolate CPU timeouts, VM idle disposal (3 min), TOPO-RESTART (3600 s), env TTL sweeper, `roomsStatsSave` 30 s gate, POST-TICK-TIMING, history chunk `timestamp` | reference/engine/src/main.js:19-24,100-122; reference/driver/lib/runtime/make.js:297-318; reference/driver/lib/runtime/user-vm.js:160-166; reference/launcher/lib/start.js:139-149; reference/storage/lib/db.js:295-315; reference/engine/src/processor.js:513-515; reference/backend/lib/game/api/game.js:722-735; reference/driver/lib/history.js:22 | **real** | — | Not game state. TOPO-RESTART's hourly reset of every player's global is **not** replicated, because it is an artifact of process management and bots already tolerate fewer resets. |
| CLK-IDS | storage `genId` mixes `Date.now()` into ids | reference/storage/lib/db.js:317-328 | **none**: chitin ids read no clock and no RNG (ID-LENGTH) | — | — |

### CRON-STAGE (normative)
Chitin runs the backend cron jobs inside the main loop once per tick: after TICK-INC-TIME and the TICK-ACCESSIBLE refresh, before `notifyRoomsDone`. At that point the stored game time is G = T+1 and the clock is `vclock(G)`.
```
for job in config.cronjobs key order (sendNotifications excluded, CLK-NOTIFY):
    # reference order (cronjobs.js:14-26): roomsForceUpdate, genPowerBanks, genInvaders, purgeTransactions,
    # recreateNpcOrders, calcMarketStats, deletePowerCreeps, genDeposits, genStrongholds, expandStrongholds;
    # mod-added jobs follow in insertion order
    if vclock(G) − last[job] > periodSec[job] × 1000:
        last[job] = vclock(G); run job to completion (RNG: cron streams, RNG-SEED)
```
- Writes and activations land before tick G's runner phase. Runners of G see inserted objects, and the room phase of G processes the activated rooms. The active set a job reads (`$nin: activeRooms`, cronjobs.js:181,376-377) contains the rooms activated for G by processor self-activation and the global stage.
- Reachability: in the original a job fires at an arbitrary point relative to the tick (separate process, CRON-SCHED). "Between ticks" is one of those points; at the default 1 tick/s most of each second is the TICK-DELAY sleep. Jobs that fire in the same poll interleave their DB operations in the original. Running them one after another is reachable whenever their polls differ, and the jobs keep independent `last` stamps. The overlap that matters: roomsForceUpdate runs before genInvaders, so genInvaders skips rooms that roomsForceUpdate has just activated.
- **PERF**: the original does sequential awaited DB round trips per room (O(rooms) per run). Per-unit work is independent for roomsForceUpdate, genPowerBanks, genInvaders, recreateNpcOrders (per room), and genDeposits and genStrongholds (per sector; genDeposits reads one deposit snapshot at job start, cronjobs.js:575). That work may run in parallel inside the stage with deterministic results (one cron stream per unit). expandStrongholds stays sequential in creation order of the cores, because one expansion's inserts are visible to the next core's search.

---

## 17. PERF — structure of the original

### PERF-PARALLEL (what is parallel today)
- **Rooms**: across `processors_cnt` processes (default 2) pulling from one queue. Within a process, the CPU part of one room at a time, with I/O pipelined (PROC-LOOP). Room processing is room-local except BULK-COLLECTION-WIDE (nuke insert) and the commutative `users` `$inc`s. It is embarrassingly parallel once movement state is per room.
- **Users**: `runners_cnt` processes × `RUNNER_THREADS` (default 1 × 2) concurrent isolates. Per-user work is independent except for shared caches.

### PERF-SERIAL (what is serialized)
- The tick itself: users phase → barrier → rooms phase → barrier → global → time++ (TICK-LOOP). Runner and processor work never overlap, and the slowest user or room bounds each phase.
- The global stage: one process, whole-world queries every tick (GLOB-INPUT), single-threaded market and power logic.
- Storage: one single-threaded process (Loki). Every op costs JSON serialization plus a socket round trip (TOPO-RPC). The Mongo/Redis variant has network round trips per op instead.
- History I/O per processor process (one promise chain).
- Scheduling granularity: `setTimeout(loop, 0)` between rooms (processor) and between users (runner pool), plus queue polling (Redis mod: 10 ms upstream, 1 ms patched).

### PERF-HOT (where time goes, from the code)
- Per room per tick: 6 reads (terrain re-read although immutable) and up to 7 writes (mapView, up to 3 bulks, eventLog, roomInfo, activeRooms `sadd`), plus `removeWhere` intents, markDone and fetch. History adds `JSON.stringify(all objects)` every tick for active rooms, `JSON.parse(JSON.stringify(creep))` per creep, and a diff pass every 20 ticks.
- Room compute: O(N) scans of `roomObjects` everywhere (`_.find`/`_.filter` by x,y in movement obstacle checks, `_create-energy`, spawns, nukes). `_.isEqual(actionLog,_actionLog)` per creep, `_.forEach` over all objects three times, `cloneDeep` on every `bulk.update`/`insert`.
- Per user per tick: 1 + 1 + 14 + 6 DB/env reads (RUN-DATA), a full `ExternalCopy` of every visible object into the isolate, Game-object construction from scratch, `JSON.parse` of Memory on first access, `JSON.stringify` of Memory on exit, then a memory write (whole string, every tick), one `$merge` upsert per room with intents, a user `$set`, and two pubsub messages.
- Global: whole-world terminal/powerSpawn/powerCreep scan, all market orders, all `rooms {status:'normal'}` (about 650 rows on the harness world per E:\screeps\server\engine-fast-interroom.js:8), every tick even when idle.

---

## 19. Chitin implementation notes (normative where marked MUST)

1. MUST keep the four barriers' observable semantics. Players of tick T see state after T−1's global stage. Room processing of T sees intents of T and state after T−1. The global stage sees all room writes of T. Time increments last. Overlapping the phases (as xxscreeps does: a room is processed as soon as all users with presence there have published, reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:106-130) is allowed **only** with double-buffered state, and only if the room waits for every user that can target it. In the original that includes users without presence, through `room` intents such as `createFlag` on any room name and `add-object-intent`.
2. MUST reproduce ACT-SET exactly. That covers the activation sources, "consumed at room-phase start", self-activation on pre-intent objects, and activations during the room or global phase taking effect next tick. Inactive-room staleness is player-visible.
3. MUST reproduce per-room iteration order (PROC-ITER, INT-ORDER-*), in-memory visibility (VIS-*), and the persisted-state rules (BULK-HIDDEN, BULK-UNDEFINED, BULK-ARRAY-MERGE, last-update-wins per top-level key). A simple model: one live object map per room in which every mutation is immediately visible; at room end, persist exactly the top-level keys that went through `update`/`inc`/`insert`/`remove`, with the values captured at their last `update`.
4. MUST implement intent sanitization sentinels (`"undefined"`, NaN or null, parseInt-of-number truncation) because processors branch on them.
5. MUST apply ORD-POLICY (§16) at every order-sensitive point: player order in the room intents loop, `users.intents` doc order for power and market intents, terminal-deal ties, and the direct-deal shuffle. MUST replace every `Math.random` site with the seeded streams of RNG-SEED. MUST read time only through CLOCK-POLICY. MUST take bug-compatibility behaviour from COMPAT-TABLE (§19A). No per-spec deviations are allowed.
6. SHOULD treat the global stage as single-threaded but O(work), not O(world).
7. SHOULD expose every hook in §15, plus xxscreeps-style registration APIs for room tick processors, intent processors with priority and mask, and shard (global) tick processors.

---

## 19A. Bug-compatibility decisions and target profiles (normative)

Specs 05, 06, 07, 08 and 11 each list the same defects and ask whether to replicate or fix them. This section records the decisions. Parity tests take their expected results from it.

### COMPAT-PROFILES
- `compat.profile = "private" | "mmo"`, default `"private"`.
  - **private** reproduces the reference stack: engine @8097782, driver, Loki storage, backend 3.3.0. It is the only executable reference, so differential parity tests run this profile.
  - **mmo** is the same as private, except in the rows whose mmo column differs. Those rows are one of three kinds: private-server artifacts of storage, driver or host (ids, shard name, Loki operators); behaviour that the official API docs (reference/docs) contradict; or failures that one player can use to freeze a room. Use this profile to host worlds for bots written for the official server. The official server's code is not in the reference sources, so mmo values are **chosen**. The basis column says what each choice rests on.
- Decision codes:
  - **R**: replicate in every profile.
  - **F**: fix in every profile. There is no override, because chitin has no machinery to reproduce the defect.
  - **C**: configurable. The default comes from the profile column; `compat.override.<ID> = "replicate" | "fix"` overrides one row.
- Expected result for parity tests:
  - R rows: the reference result.
  - F rows: the fixed result. Differential tests must not compare ticks that trigger an F row; targeted unit tests check the fixed result instead.
  - C rows: the column of the profile under test, which is `private` for differential tests.

### COMPAT-CRASH-ROOM (a room-scope throw replicated under `private`)
Reproduce PROC-ERROR at the throw point:
1. Run the room exactly as specified up to the throw point. Fire-and-forget effects already issued (`sendNotification`, stats) stay.
2. Discard every buffered write of this room tick: objects, users, flags, mapView, event log, roomInfo, and the history entry.
3. Do not self-activate the room, because ACT-RULE item 7 and the activation at reference/engine/src/processor.js:499-507 are never reached. The room runs next only if another source activates it.
4. Clear the room's intents (INT-CLEAR).
- **GOTCHA**: a throwing condition that persists freezes the room for every player on every tick in which it is processed. Examples: a power creep standing on a stronghold footprint at deploy time (BUG-CRASH-SH-PC), or an operate-extension intent re-issued each tick (BUG-CRASH-OPEXT).

### COMPAT-CRASH-GLOBAL
No global-scope throw is replicated. The only known one is BUG-CRASH-MKT-USER, which is F. TICK-ERROR's frozen `Game.time` with re-executed `users.intents` is therefore never reproduced. If chitin hits an internal error in the global stage, that is a chitin bug, not a compatibility mode.

### COMPAT-TABLE
| ID | defect (scope) | source | decision | private | mmo | basis |
|---|---|---|---|---|---|---|
| BUG-DIE-DOUBLE | A creep reaches `_die` twice in its tick: age (step at line 93), then `hits ≤ 0` (lines 110 or 134), or `userSummoned` (line 114). Result: a second tombstone with a full copy of the carried store, a second destroy event, and an extra `creepsLost` (06 CREEP-DIE-DOUBLE). A power creep with suicide plus expiry or hits death in one global stage also gets two tombstones (08 PC-SUICIDE). (R/G) | reference/engine/src/processor/intents/creeps/tick.js:93-135; reference/engine/src/processor/global-intents/power/suicidePowerCreep.js:11-16; reference/engine/src/processor/global-intents/power.js:30-36 | **R** | duplicate tombstones | same | A deterministic engine rule with no lost state (deterministic under ORD-POLICY). The reference is the only specification of the engine. The duplication is bounded to one store per death. |
| BUG-INSERT-COUNTER | Lost pending insert after counter reuse (BULK-REMOVE) (R) | reference/driver/lib/bulk.js:64-89; reference/engine/src/processor/intents/energy/tick.js:12-19 | **F** | every insert persists; temp ids are never reused; drops created this tick are still visited first, in creation order (PROC-ITER) | same | A driver bulk-writer artifact that loses objects. Chitin has no shared insert counter. |
| BUG-DOTKEY-INC | Loki applies `$inc {'resources.<type>': n}` as a flat field, so intershard resources never move in direct deals (money does). The live doc also gets a flat key. (G) | reference/storage/lib/db.js:264-266; reference/driver/lib/bulk.js:94-95; reference/engine/src/processor/global-intents/market.js:435,465 | **F** | nested semantics (`user.resources[type] += n`, in memory and persisted) | same | A Loki storage artifact. The dotted key is a MongoDB path, and screepsmod-mongo passes updates to MongoDB unchanged (E:\screeps\server\node_modules\screepsmod-mongo\lib\common\_connect.js:185-209), so Mongo-backed servers move the resources. Spec 08 Open question 2 recommends the same. Differential tests on Loki must exclude intershard direct deals. |
| BUG-CRASH-WITHDRAW | `target.store[…]` read when the target has no `store`, before any mutation. The runtime rejects such targets (reference/engine/src/game/creeps.js:493-516), so only crafted intents reach it (INT-MANUAL). (R) | reference/engine/src/processor/intents/creeps/withdraw.js:43 | **C** | COMPAT-CRASH-ROOM | no-op when `!target.store` | Room DoS through the API. |
| BUG-CRASH-OPEXT | `roomController.user` is read with no controller: any owned target (line 108) or any target at all (line 115). The runtime allows `usePower` in rooms without a controller (reference/engine/src/game/power-creeps.js:246-262), so a power creep next to a container in a highway or SK room reaches it in normal play. (R) | reference/engine/src/processor/intents/power-creeps/usePower.js:104-131 | **C** | COMPAT-CRASH-ROOM | no controller → no-op (no ops spent, no cooldown, no event), the same as the `energySent === 0` return | Any player can freeze a controller-less room, for example a power bank room, every tick. |
| BUG-CRASH-NUKE-PB | A nuke damaging a power bank sets the hit-back amount, then writes `nuke.actionLog.attacked` (`undefined.attacked`) → throw. The landing tick is lost and the nuke is removed without effect when the room is next processed at `gameTime ≥ landTime` (07 HELP-DAMAGE). (R) | reference/engine/src/processor/intents/_damage.js:35-36,86-88; reference/engine/src/processor/intents/nukes/tick.js:16,46-57 | **C** | COMPAT-CRASH-ROOM (the nuke fizzles) | skip the hit-back block when the attacker has no `actionLog`: the nuke deals its damage, with no `_damageToApply` on the nuke and no HIT_BACK event | Normal-play reachable. It changes gameplay: a nuke on a power bank room otherwise does nothing. |
| BUG-CRASH-SH-PC | Stronghold deploy calls creep `_die` on a power creep in the footprint, which reads `body.length` → throw. Deploy is retried every tick, so the room freezes while the power creep stays. (R) | reference/engine/src/processor/intents/invader-core/stronghold/stronghold.js:78-87; reference/engine/src/processor/intents/creeps/_die.js:12-15 | **C** | COMPAT-CRASH-ROOM | power creep → `bulk.update(pc, {hits: 0})`, as a nuke does (reference/engine/src/processor/intents/nukes/tick.js:25-26). It then dies in the global stage (GLOB-POWER-DEATH). | Normal-play reachable room freeze. |
| BUG-CRASH-CANCEL-SPAWN | The spawning creep is not found at the spawn → `undefined._id`. The runtime cannot reach it (07 SPAWN-CANCEL). (R) | reference/engine/src/processor/intents/spawns/cancel-spawning.js:9-10 | **C** | COMPAT-CRASH-ROOM | creep missing → only `bulk.update(spawn, {spawning: null})` | Consistent crash policy |
| BUG-CRASH-REVERSE-LAB | `lab1.store` / `lab2.store` are read before the null check, when the lab was destroyed earlier in the same tick by an earlier user's or object's action (ORD-POLICY decides who is earlier). (R) | reference/engine/src/processor/intents/labs/reverse-reaction.js:28-30,36-38 | **C** | COMPAT-CRASH-ROOM | missing lab → no-op (the null checks at lines 30 and 38 applied first) | Normal-play reachable |
| BUG-CRASH-CALC-SPAWNS | Spawns or extensions in a room without a controller → `roomController.level` read on every tick. Not reachable in normal play (PROC-CALC-SPAWNS). (R) | reference/engine/src/processor/intents/_calc_spawns.js:9,27 | **C** | COMPAT-CRASH-ROOM | treat the controller level as 0: `CONTROLLER_STRUCTURES[type][0]` allows none, so all of them get `off: true`; skip the distance sort | Consistent crash policy |
| BUG-CRASH-FLAG-FIRST | User A creates A's first flag doc in the room (a doc pushed with no `data`). A later user's room intents re-parse every doc → `undefined.split` (07 ROOM-FLAGS-MULTIUSER (b)). (R) | reference/engine/src/processor/intents/room/create-flag.js:26-30; reference/engine/src/processor/intents/room/intents.js:7-10 | **C** | COMPAT-CRASH-ROOM | fixed together with BUG-FLAGS-REPARSE | Normal-play reachable |
| BUG-CRASH-UNDEF-ROOM | A `room` intent whose `roomName` is missing or invalid groups under `"undefined"` (or the bad name). The processor then throws on missing terrain (INT-ROOM-GROUP). (R) | reference/common/lib/system.js:116-140; reference/engine/src/processor.js:33-34 | **F** | drop such intents at INT-STORE; their CPU is still charged | same | Unobservable: the bogus room has no state |
| BUG-CRASH-MKT-USER | Terminal deal: `usersById[orderTerminal.user]` is not loaded → throw (line 331 for buy orders, 352 for sell orders). `usersById` holds only users referenced by orders, power-creep docs and `users.intents`. Reachable when the terminal in `order.roomName` now belongs to another user who is in none of those: through `add-global-intent`, or in the tick the terminal changes hands before maintenance deactivates the order. The global stage then rejects (TICK-ERROR). (G) | reference/engine/src/processor/global-intents/market.js:291-337,345-352; reference/driver/lib/index.js:436-446 | **F** | also load the owner of every order's terminal into `usersById`; the deal proceeds normally | same | Replicating freezes `Game.time` for the whole world and re-executes the retained global intents on every attempt (TICK-ERROR). Spec 08 MKT-DEAL-TERMINAL already says "do not replicate". |
| BUG-FLAGS-REPARSE | When two or more users send room intents to one room in one tick, each pass re-parses every flag doc from its unchanged `data`. The earlier user's `_parsed` is reset while `_modified` stays true, so the doc is re-flushed: last `$set` wins and the earlier user's flag edits are lost (07 ROOM-FLAGS-MULTIUSER (a)). (R) | reference/engine/src/processor/intents/room/intents.js:7-10,42-52 | **C** | replicate (lost edits; ORD-POLICY decides who loses) | parse each doc once per room tick; edits of all users accumulate; every modified doc is persisted once with its final data (inserted if created this tick) | Lost player state in contested rooms. The fixed behaviour is what the code evidently intends. |
| BUG-DEPOSIT-REGEX | `$regex: '^[WE]\d*5[NS]\d*5$'` written in a JS string literal: `'\d'` is the string `'d'`, so the pattern is `/^[WE]d*5[NS]d*5$/`. It matches only W5N5, W5S5, E5N5 and E5S5, so deposits spawn only in the four sectors around the origin. (G) | reference/backend/lib/cronjobs.js:574 | **C** | replicate | every sector centre `^[WE]\d*5[NS]\d*5$` | genStrongholds writes the same pattern with correctly escaped `\\d` (reference/backend/lib/strongholds.js:227). Chitin's large worlds would otherwise have deposits only around the origin. |
| BUG-WORLDSIZE | `calcWorldSize` starts `maxX = maxY = 0`, which adds a phantom column or row when all rooms have x < 0 or all have y < 0 (XROOM-WORLD-WRAP). (G) | reference/common/index.js:183-193 | **R** | formula as written | same | Player-visible (`Game.map.getWorldSize`, terminal costs). Exact for any world that spans both W/E and N/S. |
| BUG-SH-HIGHWAY-REGEX | The unanchored `/(W\|E)\d*0/` and `/(N\|S)\d*0/` treat any room whose x or y number **contains** a `0` digit as a highway, so expansion skips W101N5, W5N105 and so on. Exact only for coordinates < 100 (08 SH-EXPAND). (G) | reference/backend/lib/strongholds.js:280 | **C** | replicate | highway iff `x % 10 == 0 \|\| y % 10 == 0` on the parsed coordinates | No difference below 100. Chitin's large worlds go beyond 100. |
| BUG-CALC-REWARD | The last reward item uses `densities[order.length−1]` instead of `densities[order[last]]` (07 HELP-CALC-REWARD). (R) | reference/engine/src/utils.js:677-698 | **R** | as written | same | Deterministic engine rule |
| BUG-PC-DEATH-CONTAINER | `_diePowerCreep` looks for a container in `scope.roomObjects`, which the global stage never passes, so a dying power creep's store always goes to its tombstone (06 PC-DEATH). (G) | reference/engine/src/processor/global-intents/power/_diePowerCreep.js:26-47; reference/engine/src/processor/global.js:45-46 | **R** | as written | same | Deterministic engine rule |
| BUG-DISMANTLE-NAN | `object.store.energy += gain` on a creep whose store has no `energy` key (cron and summoned invaders, `store: {}`) gives `NaN` (06 CREEP-DISMANTLE). (R) | reference/engine/src/processor/intents/creeps/dismantle.js:44-45 | **R** | `NaN` in memory; persisted per Open question 6 | same | Deterministic engine rule |
| BUG-STATUSDATA-COLD | Fresh world: the `roomStatusData` env key is absent until engine_main's first refresh, so `JSON.parse(undefined)` throws for one user per runner per 60 s, and other users see `{}`, which makes `getRoomStatus` and `launchNuke` throw. The cold accessibleRooms cache can also make a fresh isolate throw (TICK-ACCESSIBLE; 05 RUNTIME-ROOM-STATUS-DATA). (U) | reference/driver/lib/runtime/data.js:77-97; reference/driver/lib/runtime/runtime.js:206-208 | **F** | both values computed before the first tick and refreshed at the TICK-ACCESSIBLE cadence; no runner cache (CLK-RUNNER-CACHE) | same | A start-up race of the runner process, not a game rule. Spec 05 already made the same decision. |
| ID-SHARD-NAME | `Game.shard.name = os.hostname()`, `type 'normal'`, `ptr false` (U) | reference/driver/lib/index.js:35-51 | **C** | the chitin host's hostname; differential tests set `compat.shardName` to the reference host's name | `compat.shardName`, default `"shard0"` (the name used in the API docs' examples, reference/docs/api/source/Game.md:258; reference/docs/api/source/InterShardMemory.md:35) | Host artifact. `compat.shardName`, if set, overrides both profiles. |
| ID-PC-SHARD | The spawned power creep's `shard` is stored as `''` (getInterRoom `shardName: ''`) and exposed by `o.shard \|\| undefined`, so a spawned power creep reads `shard === undefined`, like an unspawned one. (G/U) | reference/driver/lib/index.js:446; reference/engine/src/processor/global-intents/power/spawnPowerCreep.js:24-28; reference/engine/src/game/power-creeps.js:53 | **C** | replicate | the ID-SHARD-NAME value | The docs say `shard` is "the shard where the power creep is spawned, or undefined" (reference/docs/api/source/PowerCreep.md:135-136) |
| ID-LENGTH | Loki ids are 15 hex chars (4 random + 7 from `Date.now().toString(16).slice(4)` + 4 random). `assertTargetObject` only rejects plain objects whose `id.length == 24`, so it never fires on the private server. (U) | reference/storage/lib/db.js:317-328; reference/engine/src/game/game.js:122-126 | **C** | 15-char ids | 24 lowercase hex chars, so the serialized-object check fires | screepsmod-mongo treats 24-char ids as ObjectIds (E:\screeps\server\node_modules\screepsmod-mongo\lib\common\_connect.js:189-195), and the engine's own check expects length 24. In both profiles chitin ids are deterministic (no clock, no RNG). The format belongs to spec 11. |
| ID-FOREIGN-SEG0 | Foreign segment id `0` is falsy: never fetched (data.js:231), and replaced by the target's `defaultPublicSegment` when requested (make.js:236). A default public segment of 0 is likewise never applied. (U) | reference/driver/lib/runtime/data.js:231-237; reference/driver/lib/runtime/make.js:225-241 | **C** | replicate | 0 is a valid id, both when requested and as a default | The docs say "from 0 to 99" (reference/docs/api/source/RawMemory.md:142) |

- Clock-related differences are not bugs. They are decided by CLOCK-POLICY (§16) in every profile.
- Player order and randomness are decided by ORD-POLICY and RNG-SEED in every profile.

---

## 20. Open questions

1. **Base order of `roomObjects`.** It equals the storage `find({room})` result order. Under Loki that is the binary-index order on `room`, where ties keep their order through stable sort and adaptive index updates; whether that stays equal to insertion order after many updates was not verified against lokijs 1.5.9. Under Mongo it is natural order. This affects every order-sensitive tick-loop interaction (for example which of two dropped piles is found first by `_.find`). Proposed chitin choice: creation order. That needs confirmation against a captured official tick if one is available.
2. ~~**Official-server user order inside a room**~~ Resolved by ORD-POLICY. Whatever ordering the official server has, any fixed order is reachable in the original (ORD-REACHABLE), so the remaining unknown cannot change the decision.
3. **isolated-vm `timeout`**: is it wall time or CPU time for nested `runSync`? It decides when "CPU time limit reached" fires relative to `Game.cpu.getUsed()`. Answered in spec 02 (CPU-CLOCK).
4. ~~**Cron cadence and the other wall-clock rules**~~ Resolved by CLOCK-POLICY: virtual clock `vclock(T)` with nominalTickMs = 1000, real time only for human-facing notifications and infrastructure. Still unknown: the official server's own cron cadence, which is not in the reference sources. It only matters for choosing nominalTickMs and the cron periods of an MMO-like world. Both are configuration.
5. **`userNotActive`** is honoured by ACT-RULE but never set in the open-source code. What sets it on the official server, and should chitin set it for deactivated users? Not setting it is 1:1 with the private server.
6. **NaN versus null** in sanitized number fields depends on the storage backend (Loki JSON versus Mongo BSON). Which one should chitin mirror? Every processor checked so far treats both as falsy, but this was not checked exhaustively.
7. **Nuke target activation**: not activated at launch in the reference code. Does the official server do the same? Unobservable unless a target room stays unprocessed for ≥ 50000 ticks.
8. **InterShardMemory** is not persisted by the private server (RUN-INTERSHARD). For bots, should chitin persist it (official multi-shard behaviour) or drop it (private-server behaviour)?
9. **Official behaviour of the engine defects in COMPAT-TABLE.** The official server's code is not in the reference sources. Whether official shards still show BUG-DIE-DOUBLE, the BUG-CRASH-* throws, BUG-FLAGS-REPARSE or BUG-DEPOSIT-REGEX cannot be determined, so the `mmo` column is policy, not observation. Revisit a row if a captured official tick shows otherwise. One case is open in particular: should `mmo` also fix BUG-DIE-DOUBLE, a store duplication players can trigger on purpose?
10. **Cross-spec alignment.** Spec 02 JS-DATE describes the sandbox `Date` as the real wall clock. That conflicts with CLK-SANDBOX-DATE, and this spec is normative for it. Spec 05 RUNTIME-ROOM-STATUS-DATA asks for wall-clock novice and respawn expiry "or an equivalent"; `vclock` is that equivalent. Specs 06, 07, 08 and 11 should replace their local order, RNG and clock open questions with references to ORD-POLICY, RNG-SEED, CLOCK-POLICY and COMPAT-TABLE.
