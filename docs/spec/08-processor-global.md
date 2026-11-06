# 08 — Global processing, per-user state, NPC & world lifecycle

Scope: everything in the original Screeps private server that is **not** a plain room-local intent:
the `global` stage of the main loop (inter-room transfers, power creeps, terminal sends, market),
per-user accumulators (GCL, GPL, credits, rooms list, CPU bucket, active flag), notifications,
room status / activation, observer visibility, nukes in flight, portals / inter-shard, pixels, and
every backend cron job that changes the world (power banks, deposits, invaders, strongholds, NPC
market orders, power-creep deletion). It also covers `engine/src/processor/common/fake-runtime.js`
(the NPC pathing helper).

Reference versions: engine @8097782, driver @cf63d8a, common @2fb779b, storage @3d6c3d0,
backend 3.3.0, launcher 4.2.0, xxscreeps @d1e3bad.

## 0. Conventions

- Citations are `reference/<pkg>/<path>:<line>` (ranges `a-b`).
- **T** = the tick being processed (`gameTime` value read by the stage). "Visible at T+1" means the
  player runtime of the next tick sees the change.
- **Wall-clock** = `Date.now()` milliseconds. Many global mechanics are wall-clock, not tick based;
  each one is flagged.
- **Money units**: `user.money`, `order.price` and every fee are stored in **milli-credits**
  (credits × 1000). The runtime divides by 1000 for display
  (reference/engine/src/game/market.js:196-201). Intent prices are converted with
  `parseInt((1000*value).toFixed(0))` (reference/common/lib/system.js:8).
- **Snapshot** = data loaded from storage at the start of a stage. **In-memory mutation** = see
  BULK-MUTATE; it decides what later code in the same stage sees.
- Kind column: `RT` player-runtime API validation, `RP` room processor, `GP` global processor,
  `CR` backend cron, `API` HTTP/CLI admin, `DRV` driver/storage.
- Partition column (**PERF**): `S` must be serial over the whole world (or needs a deterministic
  global order), `U` partitionable per user, `R` room-local, `X` cross-room (needs a mailbox
  between partitions), `Z` partitionable per sector, `—` not tick-relevant.

## 1. Inventory

| ID | Item | Kind | Trigger / timing | Part. | Primary source |
|---|---|---|---|---|---|
| TICK-PIPELINE | Main-loop stage order; where the global stage runs | DRV | every tick | S | reference/engine/src/main.js:26-93 |
| TICK-GLOBAL-LOAD | What the global stage loads (snapshot contents) | DRV | global stage | S | reference/driver/lib/index.js:429-449 |
| TICK-GLOBAL-ORDER | Order of sub-steps inside the global stage | GP | global stage | S | reference/engine/src/processor/global.js:33-62 |
| TICK-ACCESSIBLE-REFRESH | accessibleRooms / roomStatusData env refresh every >20 ticks | DRV | after gameTime increment | S | reference/engine/src/main.js:79-83 |
| BULK-MUTATE | bulk.update/inc mutate the in-memory object | DRV | everywhere | — | reference/driver/lib/bulk.js:33-100 |
| BULK-EXEC | Op order at execute: ops list, then $set, then inserts | DRV | stage end | — | reference/driver/lib/bulk.js:121-130 |
| BULK-DOTKEY | `$inc` with dotted key creates a flat field (LokiJS) | DRV | market direct deals | — | reference/storage/lib/db.js:264-266 |
| ORDER-STORAGE | Iteration order of query results = storage insertion order | DRV | everywhere | S | reference/storage/lib/db.js:480-513 |
| ORDER-UINTENTS | Order of users.intents docs = runner completion order | DRV | global stage | S | reference/driver/lib/index.js:229-232 |
| GINT-SANITIZE | Global intent field coercion (string/number/price/userString) | DRV | runner save | U | reference/common/lib/system.js:4-114 |
| GINT-STORE | `global` and `notify` intent routing in storeIntents/saveUserIntents | DRV | runner save | U | reference/engine/src/utils.js:405-436 |
| GINT-LIMITS | Per-tick intent caps (50 power/market ops, 10 deals, 20 notify) | RT | runtime | U | reference/driver/lib/runtime/runtime.js:73-91 |
| GINT-API | HTTP `add-global-intent` inserts extra users.intents docs | API | any time | U | reference/backend/lib/game/api/game.js:714-720 |
| GINT-CLEAR | users.intents cleared after global stage | GP | global stage end | S | reference/driver/lib/index.js:326-328 |
| XROOM-EDGE | Creep / power creep on edge tile gets `interRoom` | RP | room tick | R | reference/engine/src/processor/intents/creeps/tick.js:50-78 |
| XROOM-PORTAL | Creep / power creep on portal gets `interRoom = destination` | RP | room tick | R | reference/engine/src/processor/intents/creeps/tick.js:80-85 |
| XROOM-APPLY | Global stage teleports interRoom objects into accessible rooms | GP | global stage | X | reference/engine/src/processor/global.js:33-43 |
| XROOM-STALE | interRoom never cleared when target inaccessible | GP | global stage | X | reference/engine/src/processor/global.js:34-36 |
| PC-RT-VALIDATION | PowerCreep runtime methods and return codes | RT | runtime | U | reference/engine/src/game/power-creeps.js:162-410 |
| PC-CREATE | createPowerCreep global intent | GP | global stage | U | reference/engine/src/processor/global-intents/power/createPowerCreep.js:7-40 |
| PC-RENAME | renamePowerCreep global intent | GP | global stage | U | reference/engine/src/processor/global-intents/power/renamePowerCreep.js:7-22 |
| PC-DELETE | deletePowerCreep (start timer / cancel / instant during experimentation) | GP | global stage | U | reference/engine/src/processor/global-intents/power/deletePowerCreep.js:7-28 |
| PC-DELETE-CRON | deletePowerCreeps cron: remove + GPL penalty | CR | every 600 s wall | U | reference/backend/lib/cronjobs.js:533-554 |
| PC-UPGRADE | upgradePowerCreep global intent | GP | global stage | U | reference/engine/src/processor/global-intents/power/upgradePowerCreep.js:7-66 |
| PC-SPAWN | spawnPowerCreep global intent | GP | global stage | U | reference/engine/src/processor/global-intents/power/spawnPowerCreep.js:7-41 |
| PC-SUICIDE | suicidePowerCreep global intent | GP | global stage | U | reference/engine/src/processor/global-intents/power/suicidePowerCreep.js:7-16 |
| PC-DIE | Power creep death (age / hits ≤ 0 / suicide) → tombstone, cooldown | GP | global stage | U | reference/engine/src/processor/global-intents/power/_diePowerCreep.js:6-59 |
| PC-RENEW | renew at power bank / power spawn (room-level) | RP | room intents | R | reference/engine/src/processor/intents/power-creeps/renew.js:6-23 |
| PC-SHARD-FIELD | `shard` stored as `''` → `PowerCreep.shard` is undefined | GP | spawn | U | reference/driver/lib/index.js:446 |
| PC-WEB-API | HTTP create/delete/cancel-delete/upgrade/rename/experimentation | API | any time | U | reference/backend/lib/game/api/power-creeps.js:35-230 |
| GPL-FORMULA | GPL level/progress formulas (runtime and processor) | RT/GP | — | U | reference/engine/src/game/game.js:133-168 |
| GPL-ACCRUAL | processPower adds to `user.power` | RP | room intents | U | reference/engine/src/processor/intents/power-spawns/process-power.js:16-38 |
| GPL-FREE-LEVELS | Free levels = level − (#creeps + Σ creep levels) | RT/GP | — | U | reference/engine/src/game/power-creeps.js:9-13 |
| GCL-FORMULA | GCL level/progress, calcNeededGcl | RT | — | U | reference/engine/src/game/game.js:130-162 |
| GCL-ACCRUAL | upgradeController adds boostedEffect to `user.gcl` | RP | room intents | U | reference/engine/src/processor/intents/creeps/upgradeController.js:84-86 |
| GCL-CLAIM-CHECK | Claim allowed iff gcl ≥ needed(claimed+1); novice cap | RT/RP | room intents | U | reference/engine/src/processor/intents/creeps/claimController.js:34-55 |
| USER-ROOMS | `user.rooms` add/pull bookkeeping | RP | controller tick / claim / unclaim | U | reference/driver/lib/index.js:542-552 |
| TERM-SEND-RT | StructureTerminal.send runtime validation | RT | runtime | U | reference/engine/src/game/structures.js:714-748 |
| TERM-SEND-RP | Room processor stores `terminal.send` | RP | room intents | R | reference/engine/src/processor/intents/terminal/send.js:6-33 |
| TERM-SEND-GP | Global execution of sends | GP | global stage | S | reference/engine/src/processor/global-intents/market.js:71-95 |
| TERM-TRANSFER | executeTransfer: clamp, cost, effects, writes | GP | global stage | S | reference/engine/src/processor/global-intents/market.js:15-69 |
| TERM-COST | calcTerminalEnergyCost & continuous room distance | — | — | — | reference/engine/src/utils.js:644-659 |
| TERM-WORLDSIZE | worldSize used for wrap-around distance (off-by-one) | DRV | process start | — | reference/common/index.js:183-192 |
| TERM-CAPACITY | Terminal storeCapacity depends on RCL/owner | RP | room tick | R | reference/engine/src/processor/intents/terminal/tick.js:6-15 |
| TERM-TRANSACTIONS | transactions log documents; runtime view (100 newest) | GP/DRV | global stage | U | reference/engine/src/processor/global-intents/market.js:58-66 |
| TERM-PURGE | purgeTransactions cron | CR | every 3600 s wall | S | reference/backend/lib/cronjobs.js:443-470 |
| MKT-RT-VALIDATION | Game.market runtime methods and return codes | RT | runtime | U | reference/engine/src/game/market.js:29-192 |
| MKT-RT-VIEW | getAllOrders cache (active orders only), myOrders, history | DRV | runtime | U | reference/driver/lib/runtime/data.js:31-75 |
| MKT-CREATE | createOrder processing & fee | GP | global stage | U | reference/engine/src/processor/global-intents/market.js:109-166 |
| MKT-CHANGE-PRICE | changeOrderPrice (+fee, `_skip`) | GP | global stage | U | reference/engine/src/processor/global-intents/market.js:168-213 |
| MKT-EXTEND | extendOrder (+fee) | GP | global stage | U | reference/engine/src/processor/global-intents/market.js:215-253 |
| MKT-CANCEL | cancelOrder (no refund) | GP | global stage | U | reference/engine/src/processor/global-intents/market.js:256-263 |
| MKT-DEAL-COLLECT | deal intent filtering into terminal / direct lists | GP | global stage | S | reference/engine/src/processor/global-intents/market.js:265-285 |
| MKT-DEAL-TERMINAL | Terminal deals: distance sort, execution | GP | global stage | S | reference/engine/src/processor/global-intents/market.js:289-398 |
| MKT-DEAL-DIRECT | Inter-shard-resource deals (shuffled) | GP | global stage | S | reference/engine/src/processor/global-intents/market.js:400-495 |
| MKT-ORDER-MAINTAIN | Per-tick active/amount recomputation of every order | GP | global stage | S | reference/engine/src/processor/global-intents/market.js:497-589 |
| MKT-ORDER-EXPIRE | 30-day (wall) expiry with partial fee refund | GP | global stage | U | reference/engine/src/processor/global-intents/market.js:507-534 |
| MKT-MONEY-LOG | users.money / users.resources ledger documents | GP | global stage | U | reference/engine/src/processor/global-intents/market.js:154-164 |
| MKT-NPC-ORDERS | recreateNpcOrders cron for owner-less terminals | CR | every 300 s wall | R | reference/backend/lib/cronjobs.js:38-122 |
| MKT-STATS | calcMarketStats cron → Game.market.getHistory | CR | every 3600 s wall | S | reference/backend/lib/cronjobs.js:472-523 |
| MKT-DEAD-RETURN | market processor's returned closure never runs | GP | — | — | reference/engine/src/processor/global-intents/market.js:591-599 |
| NOTIFY-GAME | Game.notify → users.notifications with groupInterval buckets | RT/DRV | runner save | U | reference/driver/lib/index.js:185-227 |
| NOTIFY-ENGINE | engine sendNotification (attacked, controller, walls) | DRV | room stage | U | reference/driver/lib/index.js:455-471 |
| NOTIFY-ERROR | console errors as `error` notifications | DRV | runner save | U | reference/driver/lib/index.js:371-410 |
| NOTIFY-CRON | sendNotifications cron (delivery hook) | CR | every 60 s wall | U | reference/backend/lib/cronjobs.js:124-176 |
| USER-RUN-SET | Which users run each tick and in what queue order | DRV | tick start | S | reference/driver/lib/index.js:144-151 |
| USER-DEACTIVATE | User with zero objects gets `active: 0` | DRV | runner | U | reference/driver/lib/runtime/data.js:103-109 |
| USER-WORLD-STATUS | empty / normal / lost | API | HTTP | U | reference/backend/lib/utils.js:88-113 |
| USER-RESPAWN | respawnUser: wipe objects, ruins, power creeps | API | HTTP / CLI | U | reference/backend/lib/utils.js:115-168 |
| USER-PLACE-SPAWN | place-spawn: first spawn, controller, safe mode, invaderGoal | API | HTTP | R | reference/backend/lib/game/api/game.js:328-410 |
| USER-BOT-SPAWN | CLI bots.spawn equivalent of place-spawn | API | CLI | R | reference/backend/lib/cli/bots.js:70-124 |
| USER-CPU | Per-user CPU limit / bucket accounting | DRV | runner | U | reference/driver/lib/runtime/make.js:54-96,192-212 |
| USER-SHARDACCESS | `shardAccess === false` blocks upgrade/claim (never set in OSS) | RP/RT | — | U | reference/engine/src/processor/intents/creeps/upgradeController.js:13-15 |
| ROOMSTAT-ACCESSIBLE | status 'normal' + openTime gate | DRV | global stage / env refresh | S | reference/driver/lib/index.js:433-434,490-496 |
| ROOMSTAT-DATA | novice / respawn / closed map for runtime | DRV | every >20 ticks | S | reference/driver/lib/index.js:498-524 |
| ROOMSTAT-RT | Game.map.getRoomStatus / isRoomAvailable | RT | runtime | U | reference/engine/src/game/map.js:201-233 |
| ROOMSTAT-CLI | openRoom / closeRoom / removeRoom | API | CLI | — | reference/backend/lib/cli/map.js:568-609 |
| ROOMSTAT-NOVICE-EFFECTS | Every place novice/respawnArea changes behaviour | RP/RT | — | R | reference/engine/src/processor/intents/nukes/tick.js:10-14 |
| ACTIVE-SET | ACTIVE_ROOMS set: who is processed next tick | DRV/RP | every tick | S | reference/engine/src/processor.js:106-142,499-507 |
| ACTIVE-FORCE-CRON | roomsForceUpdate cron (90–109 tick jitter) | CR | every 20 s wall | S | reference/backend/lib/cronjobs.js:178-192 |
| CRON-ACTIVE-VIEW | ACTIVE_ROOMS as crons see it (`_id: {$nin: activeRooms}`): lifecycle, storage edge cases, chitin eligibility rule | CR/DRV | genInvaders / roomsForceUpdate runs | S | reference/backend/lib/utils.js:315-317 |
| OBS-RT | observeRoom runtime validation | RT | runtime | U | reference/engine/src/game/structures.js:548-569 |
| OBS-RP | observeRoom reset/persist in room processor | RP | room stage | R | reference/engine/src/processor.js:116-118,284-288,403-406 |
| OBS-VISIBILITY | Observed room loaded into runtime at T+1 | DRV | runner | U | reference/driver/lib/runtime/data.js:114-126 |
| NUKE-LAUNCH-RT | launchNuke runtime validation | RT | runtime | U | reference/engine/src/game/structures.js:1356-1387 |
| NUKE-LAUNCH-RP | launch-nuke processing: cross-room insert | RP | room intents | X | reference/engine/src/processor/intents/nukers/launch-nuke.js:8-51 |
| NUKE-FLIGHT | in-flight nuke: activation, novice cancellation | RP | room tick | R | reference/engine/src/processor/intents/nukes/tick.js:10-14 |
| NUKE-PRETICK | cancel createCreep intents the tick before landing | RP | room pretick | R | reference/engine/src/processor/intents/nukes/pretick.js:6-15 |
| NUKE-LAND | landing effects (kill, clear, area damage, controller) | RP | room tick | R | reference/engine/src/processor/intents/nukes/tick.js:16-78 |
| PORTAL-DECAY | unstableDate → decayTime → removal (+wall at x+1,y+1) | RP | room tick | R | reference/engine/src/processor/intents/portals/tick.js:6-25 |
| SHARD-NAME | Game.shard = hostname/normal/ptr false | DRV | sandbox init | — | reference/driver/lib/index.js:35-52 |
| SHARD-INTERSHARD | Inter-shard portals, InterShardMemory, shard limits: absent | — | — | — | reference/driver/lib/runtime/runtime.js:125-128 |
| PIXEL-GENERATE | Game.cpu.generatePixel: absent on private server | — | — | — | reference/engine/src/game/game.js:144-157 |
| CRON-SCHEDULER | Wall-clock cron runner, intervals, overlap | CR | setInterval 1 s | S | reference/backend/lib/cronjobs.js:14-36 |
| PB-GEN | genPowerBanks cron | CR | every 300 s wall | R | reference/backend/lib/cronjobs.js:194-243 |
| PB-DECAY | power bank removal at decayTime−1 | RP | room tick | R | reference/engine/src/processor.js:421-426 |
| PB-DESTROY | destroyed bank → ruin (decay 10) | RP | room | R | reference/engine/src/processor/intents/structures/_destroy.js:21-49 |
| DEP-TYPE | room.depositType assignment (DB upgrade v4) | DRV | DB load | — | reference/storage/lib/db.js:55-63 |
| DEP-GEN | genDeposits cron | CR | every 300 s wall | Z | reference/backend/lib/cronjobs.js:556-643 |
| DEP-DECAY | deposit removal | RP | room tick | R | reference/engine/src/processor/intents/deposits/tick.js:13-16 |
| INV-HARVEST | `invaderHarvested` accumulation on sources | RP | harvest | R | reference/engine/src/processor/intents/creeps/harvest.js:45-48 |
| INV-GEN | genInvaders eligibility | CR | every 300 s wall | Z | reference/backend/lib/cronjobs.js:377-431 |
| INV-RAID | raid composition (count, types, boosts, tiles) | CR | genInvaders | R | reference/backend/lib/cronjobs.js:320-375 |
| INV-CREEP | invader creep document | CR | genInvaders | R | reference/backend/lib/cronjobs.js:262-312 |
| INV-GOAL | invaderGoal re-roll & reset | CR | after raid | R | reference/backend/lib/cronjobs.js:433-439 |
| INV-SUMMON-API | create-invader / remove-invader HTTP | API | HTTP | R | reference/backend/lib/game/api/game.js:631-704 |
| SH-GEN | genStrongholds cron (sector without core) | CR | every 300 s wall | Z | reference/backend/lib/strongholds.js:225-236 |
| SH-SELECT | selectRoom eligible SK rooms | CR | genStrongholds | Z | reference/backend/lib/strongholds.js:130-161 |
| SH-SPAWN | spawnStronghold: template/spot pick, core+ramparts insert | CR/API | genStrongholds / CLI | R | reference/backend/lib/strongholds.js:14-128 |
| SH-DEPLOY | deploy at deployTime−1: collapse timer, structures | RP | room pretick | R | reference/engine/src/processor/intents/invader-core/stronghold/stronghold.js:22-128 |
| SH-COLLAPSE | collapse timer end: controller reset, object removal | RP | room tick | R | reference/engine/src/processor/intents/invader-core/tick.js:11-27 |
| SH-EXPAND | expandStrongholds cron: BFS, level-0 core insert | CR | every 900 s wall | Z | reference/backend/lib/strongholds.js:238-349 |
| SH-DESTROY | core destroyed: controller reset, reward store | RP | room | R | reference/engine/src/processor/intents/invader-core/destroy.js:7-38 |
| SH-REWARD | utils.calcReward algorithm (with index bug) | RP | deploy / destroy | R | reference/engine/src/utils.js:677-698 |
| SH-AI | Per-tick stronghold behaviours (cross-ref; room-local NPC AI) | RP | room pretick | R | reference/engine/src/processor/intents/invader-core/stronghold/stronghold.js:388-491 |
| FAKERT-POS | NPC RoomPosition / packLocal | RP | NPC AI | R | reference/engine/src/processor/common/fake-runtime.js:8-77 |
| FAKERT-MATRIX | NPC CostMatrix & default matrix | RP | NPC AI | R | reference/engine/src/processor/common/fake-runtime.js:79-141 |
| FAKERT-FINDPATH | NPC findPath | RP | NPC AI | R | reference/engine/src/processor/common/fake-runtime.js:143-182 |
| FAKERT-MOVETO | NPC moveTo / path cache in `memory_move` | RP | NPC AI | R | reference/engine/src/processor/common/fake-runtime.js:200-245 |
| FAKERT-WALKTO | NPC walkTo (push idle friendly creep) | RP | NPC AI | R | reference/engine/src/processor/common/fake-runtime.js:247-263 |
| FAKERT-FLEE | NPC flee | RP | NPC AI | R | reference/engine/src/processor/common/fake-runtime.js:185-198 |
| FAKERT-CLOSEST | NPC findClosestByPath (last-match quirk) | RP | NPC AI | R | reference/engine/src/processor/common/fake-runtime.js:265-309 |
| FAKERT-NEXTDIR | nextDirectionByPath | RP | NPC AI | R | reference/engine/src/processor/common/fake-runtime.js:311-332 |
| MOD-HOOKS | Extension points relevant to this area | DRV | — | — | reference/driver/lib/index.js:19-33 |
| RNG-INVENTORY | Every Math.random use in this area | — | — | — | (section 22) |

---

## 2. Tick pipeline and global-stage mechanics

### TICK-PIPELINE
Main loop (one process, reference/engine/src/main.js:26-93):
1. `notifyTickStarted` (aborts the tick if paused).
2. `getAllUsers` → push every runnable user into the users queue; runners execute player code
   (runtime of tick T) and save intents (USER-RUN-SET, GINT-STORE).
3. `getAllRoomsNames` = SMEMBERS + DEL of `ACTIVE_ROOMS` (reference/driver/lib/index.js:170-174);
   room processors process every listed room in parallel (reference/engine/src/processor.js:16-520).
4. `commitDbBulk` (no-op), **global stage** (`require('./processor/global')()`), commit (no-op).
5. `incrementGameTime` → T+1.
6. If `gameTime > lastAccessibleRoomsUpdate + 20` refresh `ACCESSIBLE_ROOMS` and
   `ROOM_STATUS_DATA` (TICK-ACCESSIBLE-REFRESH). `lastAccessibleRoomsUpdate` is process-local and
   starts at 0, so the first tick after start always refreshes, then every 21 ticks.
7. publish `roomsDone`, run `config.mainLoopCustomStage()` (mod hook).

**GOTCHA**: an exception anywhere in steps 2-4 skips `incrementGameTime`; the same T is replayed
next loop and `users.intents` is not cleared (accumulates). This is failure behaviour; chitin should
not replicate it (log + continue is acceptable).

### TICK-GLOBAL-LOAD
`driver.getInterRoom()` (reference/driver/lib/index.js:429-449) loads, all as full scans:
- `gameTime`;
- `creeps`: `rooms.objects` with `type ∈ {creep, powerCreep}` and `interRoom != null`;
- `accessibleRooms`: map of rooms with `status == 'normal'` and (`!openTime || openTime < Date.now()`);
- `roomObjects`: every `terminal`, `powerSpawn`, `powerCreep` in the world;
- `orders` (all `market.orders`), `userPowerCreeps` (all `users.power_creeps`), `userIntents`
  (all `users.intents` docs);
- `users`: only users whose `_id` appears as `.user` in orders ∪ power creeps ∪ intents;
- `shardName: ''`.

**PERF**: the original does O(all terminals + all power creeps + all orders + all PC docs) work per
tick regardless of activity. Chitin should keep these as resident indexed tables and only touch
dirty entries; but observable results must equal a full scan (see MKT-ORDER-MAINTAIN).

### TICK-GLOBAL-ORDER
reference/engine/src/processor/global.js:33-62. Exact order:
1. XROOM-APPLY for every interRoom object (storage order).
2. Power processor (reference/engine/src/processor/global-intents/power.js:16-36): for each
   `users.intents` doc (ORDER-UINTENTS), for intent types in the fixed order
   `spawnPowerCreep, suicidePowerCreep, deletePowerCreep, upgradePowerCreep, createPowerCreep,
   renamePowerCreep` (power.js:8), for each intent in array order → handler. Then sweep every power
   creep room object: `gameTime >= ageTime-1 || hits <= 0` → PC-DIE (power.js:30-36).
3. Market processor: TERM-SEND-GP; then per `users.intents` doc: `createOrder`, `changeOrderPrice`,
   `extendOrder`, `cancelOrder`, `deal` (collection only); then MKT-DEAL-TERMINAL; MKT-DEAL-DIRECT;
   MKT-ORDER-MAINTAIN (incl. expiry).
4. Execute all bulks in parallel (objects, rooms (unused), users, market orders ×2, users.money,
   transactions, users.resources, users.power_creeps) and clear `users.intents`.

All effects are applied before `incrementGameTime` ⇒ visible at T+1. All global effects of tick T
happen **after** every room of tick T was processed, so they see post-room-stage state (stores,
hits, positions).

**PERF**: steps 2 and 3 share state only through terminals (sends/deals) and user money; power
creep intents are fully independent of the market. XROOM-APPLY is independent of both except that
the power creep death sweep uses pre-move positions (see PC-DIE).

### BULK-MUTATE
`bulk.update(obj, data)` (reference/driver/lib/bulk.js:33-63): deep-clones `data`; for each value
that is an object, merges it into `obj[key]` (so `{store:{energy:5}}` keeps other store keys) and
writes the merged full sub-object into the $set; then `_.merge(obj, data)` mutates the caller's
object. Passing an id string instead of an object does **not** mutate anything (and throws if a
value is an object). `bulk.inc(obj, key, n)` sets `obj[key] = (obj[key]||0)+n` in memory
(bulk.js:90-100). Consequence: later code in the same stage sees updated values **only** when the
earlier call passed the object, not its id. This drives many GOTCHAs below (e.g. PC-RENAME).

### BULK-EXEC
At `execute()` (bulk.js:121-130) the storage receives, in order: all `remove`/`inc`/`addToSet`/
`pull` ops in call order, then one merged `$set` per updated id (insertion order of first update),
then inserts. A `$set` after a `remove` of the same id is a no-op. Different bulk objects (e.g. the
two market-order bulks) execute concurrently on the same collection; within the market processor
they never touch the same order with conflicting ops.

### BULK-DOTKEY
The LokiJS storage applies `$inc` keys literally (reference/storage/lib/db.js:264-266), and
`bulk.inc` also writes `obj['resources.X']` literally. `$inc: {'resources.pixel': n}` therefore
creates a flat field named `resources.pixel` instead of changing `resources.pixel`.
**GOTCHA**: on the private server MKT-DEAL-DIRECT moves money but never moves inter-shard
resources in `user.resources` (what `Game.resources` reads, reference/engine/src/game/game.js:170).
The official server (MongoDB) has nested semantics. See Open questions.

### ORDER-STORAGE
Loki `find` without an index scans the collection array in insertion order
(reference/storage/lib/db.js:515-530); removals compact it. Therefore "storage order" =
object creation order. Chitin: give every object a monotonic creation sequence and iterate global
lists (terminals, power creeps, power spawns, orders, interRoom objects) in that order.

### ORDER-UINTENTS
One `users.intents` doc is inserted per user per tick when that user's runner finishes
(reference/driver/lib/index.js:229-232), plus any HTTP-inserted docs (GINT-API). Runners run in
parallel (`RUNNER_THREADS`, default 2, reference/driver/lib/index.js:562-568), so the doc order is
completion order — **nondeterministic**. It matters for: market fees vs balance, `_skip` from price
changes vs deals, distance-tie order of terminal deals, power-creep name collisions.
Chitin must choose a deterministic order (recommendation in Open questions).

---

## 3. Global intent ingestion

### GINT-SANITIZE
All intents pass `sanitizeIntent` (reference/common/lib/system.js:84-93): only declared fields
survive, each coerced: `string` → `""+v` (undefined → `"undefined"`), `number` → `parseInt`
(NaN possible, truncates fractions), `boolean` → `!!v`, `price` → `parseInt((1000*v).toFixed(0))`,
`userString` → first 100 chars, `userText` → first 1000 chars. Global intent schemas
(system.js:23-33): `cancelOrder{orderId}`, `changeOrderPrice{orderId,newPrice:price}`,
`createOrder{type,resourceType,price:price,totalAmount:number,roomName}`,
`createPowerCreep{name:userString,className}`, `deal{orderId,amount:number,targetRoomName}`,
`deletePowerCreep{id,cancel:boolean}`, `extendOrder{orderId,addAmount:number}`,
`renamePowerCreep{id,name:userString}`, `spawnPowerCreep{id,name:userString}`,
`suicidePowerCreep{id}`, `upgradePowerCreep{id,power:number}`, `notify{message:userText,
groupInterval:number}`. Custom intent types from mods (`config.engine.customIntentTypes`) are
appended (system.js:105-111).

### GINT-STORE
`utils.storeIntents` (reference/engine/src/utils.js:405-436) splits the runtime intent list:
`notify` → sanitized array; `room` → per-room `room` intents; `global` → sanitized object of arrays;
any other key is an object id routed to its room. `saveUserIntents`
(reference/driver/lib/index.js:180-243) writes `notify` to `users.notifications` (NOTIFY-GAME),
`global` as one `users.intents` doc `{user, intents}`, rooms into `rooms.intents` and activates
those rooms.

### GINT-LIMITS
Runtime `intents.pushByName(id, name, data, maxLen)` refuses when the array already has `maxLen`
items (reference/driver/lib/runtime/runtime.js:82-91). Each accepted push adds 0.2 CPU. Caps:
`createOrder` none at push level (guarded by MARKET_MAX_ORDERS, see MKT-RT-VALIDATION),
`cancelOrder`/`changeOrderPrice`/`extendOrder` 50, `deal` 10 (returns ERR_FULL), every power-creep
global intent 50 (return value ignored → still returns OK when full), `notify` 20 (ERR_FULL).
**GOTCHA**: power-creep methods return OK even when the 50-cap dropped the intent
(reference/engine/src/game/power-creeps.js:187,200,213,242,371,408).

### GINT-API
`POST /api/game/add-global-intent` inserts `{user, intents: sanitize({[name]: [intent]})}` directly
into `users.intents` (reference/backend/lib/game/api/game.js:714-720). Such docs are processed in
the next global stage exactly like runner docs (extra doc per user → separate iteration entry).

### GINT-CLEAR
`users.intents.clear()` after the stage (reference/driver/lib/index.js:326-328). An HTTP intent
inserted between load and clear is lost.

---

## 4. Inter-room movement

### XROOM-EDGE
Room tick of a non-spawning creep (reference/engine/src/processor/intents/creeps/tick.js:50-78) and
of a power creep (reference/engine/src/processor/intents/power-creeps/tick.js:16-42): after
`movement.execute`, if `x==0||x==49||y==0||y==49` (reference/engine/src/utils.js:547-553) and owner
∉ {'2','3'}: compute target by first match in order `x==0` → west room, x=49; `y==0` → north room,
y=49; `x==49` → east, x=0; `y==49` → south, y=0. `bulk.update(obj, {interRoom:{room,x,y}})` and
push `EVENT_EXIT {room,x,y}` to the room event log.
**GOTCHA**: a creep that ends any tick on an exit tile is moved again (exit ping-pong); corner
tiles resolve by the precedence above.

### XROOM-PORTAL
Creeps: if `ageTime` is set and a portal is on the tile, `interRoom = portal.destination` (overrides
the edge value; tick.js:80-85). Shard destinations are **not** filtered for creeps. Power creeps:
only if `!portal.destination.shard` (power-creeps/tick.js:44-47).

### XROOM-APPLY
reference/engine/src/processor/global.js:33-43: for each interRoom object (storage order): if
`accessibleRooms[interRoom.room]` is falsy → skip. Else activate the target room (once per room)
and `update(obj, {room, x, y, interRoom: null})`.
No collision check, no terrain check, no ownership/novice check: arrivals can stack on one tile.
Arrival is visible at T+1 in the new room.
**GOTCHA**: an inter-shard portal destination `{shard, room}` on a creep becomes `x,y = undefined`
if a local room with that name is accessible. Private servers have no such portals by default.
**PERF**: pure function of the object; partition by destination room (mailbox, applied before
the T+1 runtime snapshot). Order-independent.

### XROOM-STALE
If the target room is inaccessible, `interRoom` stays set forever (it is only nulled on success).
The object is re-fetched every tick; if the target later becomes accessible (openTime passes, admin
opens the room) the object is teleported from wherever it is to the stored x,y.
**GOTCHA**: replicate (store the field; clear only on success).

---

## 5. Power creeps

Data model: `users.power_creeps` doc per creep (account-level, survives death) with `name,
className, user, level, hitsMax, store, storeCapacity, spawnCooldownTime, powers{P:{level}},
deleteTime?, shard?`. While spawned, a `rooms.objects` doc with the **same `_id`** exists
(type `powerCreep`). Runtime merges both (reference/engine/src/game/power-creeps.js:15-17).

### PC-RT-VALIDATION
reference/engine/src/game/power-creeps.js (only global-intent methods; room methods belong to the
power-creep room spec):
- `PowerCreep.create(name, className)` (395-410): name falsy / not string / >100 chars →
  ERR_INVALID_ARGS; `calcFreePowerLevels() <= 0` → ERR_NOT_ENOUGH_RESOURCES; existing name in
  `userPowerCreeps` (start-of-tick snapshot) → ERR_NAME_EXISTS; className ∉ POWER_CLASS values →
  ERR_INVALID_ARGS; push `createPowerCreep` (cap 50); OK.
- `spawn(powerSpawn)` (162-189): `this.room` set → ERR_BUSY; not StructurePowerSpawn →
  ERR_INVALID_TARGET; `!this.my || !powerSpawn.my` → ERR_NOT_OWNER; structure inactive vs RCL →
  ERR_RCL_NOT_ENOUGH; `this.spawnCooldownTime` (getter: non-null and `> Date.now()`) → ERR_TIRED;
  initialises `Memory.powerCreeps[name] = {}` if absent; push `spawnPowerCreep{id: spawnId, name}`.
- `suicide()` (191-202): not in a room → ERR_BUSY; !my → ERR_NOT_OWNER.
- `delete(cancel)` (204-215): in a room → ERR_BUSY; !my → ERR_NOT_OWNER.
- `upgrade(power)` (217-244): !my → ERR_NOT_OWNER; free levels ≤ 0 → ERR_NOT_ENOUGH_RESOURCES;
  `level >= POWER_CREEP_MAX_LEVEL` → ERR_FULL; unknown power / wrong class → ERR_INVALID_ARGS;
  power level 5 → ERR_FULL; `this.level < POWER_INFO[p].level[curPowerLevel]` → ERR_FULL.
- `rename(name)` (356-373): in a room → ERR_BUSY; !my; invalid → ERR_INVALID_ARGS; name exists →
  ERR_NAME_EXISTS.
- `renew(target)` (319-340): !my; not in room → ERR_BUSY; target not powerBank/powerSpawn in
  `register.structures` → ERR_INVALID_TARGET; inactive power spawn → ERR_RCL_NOT_ENOUGH; not adjacent
  → ERR_NOT_IN_RANGE; `intents.set(id,'renew')` (room intent).
Getters: `shard: o.shard || undefined`, `spawnCooldownTime` only while `> Date.now()`,
`deleteTime: o.deleteTime || undefined` (power-creeps.js:53-55).

### PC-CREATE
createPowerCreep.js:7-40. Per intent:
1. `powerLevel = floor(pow((user.power||0)/POWER_LEVEL_MULTIPLY, 1/POWER_LEVEL_POW))`;
   `used = #userPCs + Σ userPCs.level + (user._usedPowerLevels||0)`; `used >= powerLevel` → drop.
2. className ∉ POWER_CLASS values → drop.
3. `name = intent.name.substring(0,50)`; any existing user PC (snapshot) with that name → drop.
4. Insert `{name, className, user, level:0, hitsMax:1000, store:{}, storeCapacity:100,
   spawnCooldownTime:0, powers:{}}`; `user._usedPowerLevels += 1` (in-memory only).
**GOTCHA**: name truncation is 50 here but 100 in runtime validation; two creates (or create +
rename) with the same name in one tick both succeed → duplicate names (snapshot not updated).

### PC-RENAME
renamePowerCreep.js:7-22: find user's PC by id; missing or `spawnCooldownTime === null` (spawned)
→ drop; `name = substring(0,50)`; name already used (snapshot) → drop; `update(id, {name})`.
**GOTCHA**: update is by id → no in-memory mutation → two renames to the same name in one tick
both succeed; the old name stays "taken" for the rest of the tick.

### PC-DELETE
deletePowerCreep.js:7-28: find by user+id; missing or spawned (`spawnCooldownTime === null`) →
drop. `cancel` → `deleteTime: null`. Else (logs `user.powerExperimentationTime` to console):
if `user.powerExperimentationTime > Date.now()` → **remove the doc immediately** (no GPL penalty);
else if `deleteTime` already set → nothing; else `deleteTime = Date.now() + POWER_CREEP_DELETE_COOLDOWN`
(24 h wall-clock).

### PC-DELETE-CRON
reference/backend/lib/cronjobs.js:533-554, every 600 s wall-clock, sequential: for each PC doc with
`deleteTime < now` (and truthy): load user; `L = floor(pow(power/POWER_LEVEL_MULTIPLY,
1/POWER_LEVEL_POW))` (calcPowerLevel, 525-531), `base(k) = k^POWER_LEVEL_POW * POWER_LEVEL_MULTIPLY`;
`change = round(power − base(L−1) − (power − base(L))·(base(L) − base(L−1))/(base(L+1) − base(L)))`;
`$inc power: −change` and remove the doc. Effect: the account drops exactly one GPL level while
keeping its fractional progress within the level.
**GOTCHA**: actual deletion happens up to 10 minutes after `deleteTime` (cron granularity).
Edge: L = 0 gives base(−1) = 1000 (negative change possible).

### PC-UPGRADE
upgradePowerCreep.js:7-66, per intent:
1. Same `used >= powerLevel` check as PC-CREATE (includes `_usedPowerLevels`).
2. Find user PC by id (may be spawned or not); missing → drop.
3. `level >= POWER_CREEP_MAX_LEVEL (25)` → drop; `POWER_INFO[power]` missing or class mismatch → drop.
4. If `powers[p]` missing, create `{level:0}` in memory (even if later rejected); power level 5 →
   drop; `pc.level < POWER_INFO[p].level[powerLevel]` → drop.
5. `level+1`, `storeCapacity+100`, `hitsMax+1000`, `powers[p].level++`; if a room object exists,
   update it (merge keeps `cooldownTime`); update the PC doc **by object** (in-memory level
   mutation ⇒ the next upgrade in the same tick sees the new level and the higher `used`).
Hits are not changed on upgrade (only hitsMax).

### PC-SPAWN
spawnPowerCreep.js:7-41:
1. `powerSpawn = first powerSpawn with _id == intent.id`; missing, other owner, or
   `_justSpawned` (one spawn per power spawn per tick) → drop. **No RCL/active check here**.
2. PC = user's PC doc **by name** (`intent.name`, first match); missing, spawned
   (`spawnCooldownTime === null`), or `spawnCooldownTime > Date.now()` → drop.
3. Any power creep room object (pre-stage snapshot, any owner) on the spawn's tile → drop.
4. Update PC doc `{shard: shardName (''), spawnCooldownTime: null, deleteTime: null}` (by object →
   mutates); insert room object = copy of PC doc + `{type:'powerCreep', room,x,y of spawn,
   hits: hitsMax, ageTime: T + POWER_CREEP_LIFE_TIME (5000), actionLog:{spawned:true},
   notifyWhenAttacked:true}` with the same `_id`; mark `_justSpawned`.
Visible at T+1. Pending deletion is cancelled by spawning.

### PC-SUICIDE
suicidePowerCreep.js:7-16: find spawned PC room object by user+id (pre-stage list) → PC-DIE.
**GOTCHA**: no dedup. Two suicide intents, or suicide + age/hits death in the same tick, call
PC-DIE twice → **two tombstones** are inserted (the remove/update are idempotent).

### PC-DIE
_diePowerCreep.js:6-59 (also reached from the sweep `T >= ageTime-1 || hits <= 0`):
- Tombstone `{type:'tombstone', room,x,y (pre-move snapshot), user, deathTime:T,
  decayTime: T + TOMBSTONE_DECAY_POWER_CREEP (500), store:{}, powerCreepId, powerCreepName,
  powerCreepTicksToLive: ageTime − T, powerCreepClassName, powerCreepLevel,
  powerCreepPowers: {p:{level}}, powerCreepSaying: actionLog.say.message if isPublic}`.
- Store: all positive amounts go into the tombstone.
  **GOTCHA**: the container branch never runs — `roomObjects` is not in the power scope
  (global.js:45-46), so `_.find(undefined, …)` is undefined. Do **not** drop into a container.
- Remove the room object; update PC doc `{shard: null, spawnCooldownTime: Date.now() +
  POWER_CREEP_SPAWN_COOLDOWN}` (8 h wall-clock).
**GOTCHA**: a power creep that crossed a room edge this tick dies (and gets its tombstone) at its
pre-move position (the sweep list is a different snapshot than the interRoom list).
Hits ≤ 0 comes from room-stage damage (reference/engine/src/processor/intents/_damage.js:22-24 +
power-creeps/tick.js:49-67) or nukes (nukes/tick.js:25-27). The PC therefore exists with hits ≤ 0
through the rest of its room stage and dies in the global stage of the same tick.

### PC-RENEW
reference/engine/src/processor/intents/power-creeps/renew.js:6-23 (room stage): target must be
powerBank or powerSpawn, Chebyshev range ≤ 1, power spawn active vs RCL; sets
`ageTime = T + POWER_CREEP_LIFE_TIME`, `actionLog.healed`. No cost.

### PC-SHARD-FIELD
`shardName` is hard-coded `''` (reference/driver/lib/index.js:446) so spawned PCs store
`shard: ''` and `PowerCreep.shard` is `undefined` even while spawned.
**GOTCHA**: official bots that test `pc.shard` to detect "spawned" break on the private server.
Chitin must pick: 1:1 private server ('') or official (shard name). See Open questions.

### PC-WEB-API
reference/backend/lib/game/api/power-creeps.js: `create` (35-64, same rules as PC-CREATE, name
50 chars), `delete` (66-89: rejects spawned or `shard` set; experimentation → immediate remove;
else 24 h timer), `cancel-delete` (91-105, `$unset deleteTime`), `upgrade` (107-201: bulk
level-set with `powersAreValid` replay, writes both docs, `hitsMax = 1000*(L+1)`,
`storeCapacity = 100*(L+1)`), `rename` (203-222), `experimentation` (224-230: consumes one
`powerExperimentations`, sets `powerExperimentationTime = now + 24 h`). These bypass the tick
pipeline (direct DB writes, any time).

### GPL-FORMULA
Runtime (reference/engine/src/game/game.js:133-134,164-168):
`level = floor(pow(power/POWER_LEVEL_MULTIPLY, 1/POWER_LEVEL_POW))`,
`progress = power − level^POW·MULT`, `progressTotal = (level+1)^2·1000 − level^POW·MULT`.
**GOTCHA**: `progressTotal` hard-codes 2 and 1000 instead of the constants.
**GOTCHA**: `Math.pow` results must be bit-identical to V8 (fdlibm-style `ieee754::pow`) for the
`floor` at level boundaries; this is a JS-engine requirement.

### GPL-ACCRUAL
process-power.js:16-38 (room stage): `amount = 1`, with PWR_OPERATE_POWER effect
(`endTime >= T`) `amount = min(store.power, 1 + effect[level-1])`; requires `power >= amount` and
`energy >= amount·POWER_SPAWN_ENERGY_RATIO (50)`; deducts; `stats.powerProcessed += amount`;
`bulkUsers.inc(userId, 'power', amount)` (by id → per-room bulk, commutative).
Visible in `Game.gpl` at T+1.

### GPL-FREE-LEVELS
Runtime `calcFreePowerLevels` (power-creeps.js:9-13) = level − (#PC docs + Σ levels), start-of-tick
snapshot, ignores same-tick intents. The global processor is the real gate (PC-CREATE / PC-UPGRADE).

---

## 6. GCL and per-user room list

### GCL-FORMULA
Runtime (reference/engine/src/game/game.js:130-131,159-163):
`level = floor(pow(gcl/GCL_MULTIPLY, 1/GCL_POW)) + 1`; `base = (level−1)^GCL_POW·GCL_MULTIPLY`;
`progress = gcl − base`; `progressTotal = level^GCL_POW·GCL_MULTIPLY − base`.
`calcNeededGcl(n) = GCL_MULTIPLY·(n−1)^GCL_POW` (reference/engine/src/utils.js:661-663).
Bot-created users get `gcl = GCL_MULTIPLY·(opts.gcl−1)^GCL_POW` (reference/backend/lib/cli/bots.js:75).

### GCL-ACCRUAL
upgradeController.js:84-86: `bulkUsers.inc(controller.user, 'gcl', boostedEffect)` per upgrade
action (per-room bulk, commutative). Visible at T+1.

### GCL-CLAIM-CHECK
Runtime (reference/engine/src/game/creeps.js:848-855): `claimed = user.rooms.length +
controllersClaimedInTick`; if `claimed && (!gcl || gcl < calcNeededGcl(claimed+1))` →
ERR_GCL_NOT_ENOUGH; if `claimed >= GCL_NOVICE (3)` and the creep's room is novice → ERR_FULL.
Processor (claimController.js:34-43): `claimedRooms = user.rooms.length` from the per-room `users`
snapshot loaded when the room was fetched; `user.gcl < calcNeededGcl(claimedRooms+1)` → drop.
**GOTCHA**: two claims by the same user in different rooms in one tick both pass the processor
check (each room sees the same snapshot; or a partially updated one, depending on processing
order). Deterministic chitin choice: start-of-tick user snapshot for every room.

### USER-ROOMS
`addRoomToUser` / `removeRoomFromUser` (reference/driver/lib/index.js:542-552) issue `$addToSet` /
`$pull` on `users.rooms` if the in-memory list lacks/has the room. Callers: claim
(claimController.js:55), every owned-controller tick (controllers/tick.js:18, self-healing),
downgrade to 0 (controllers/tick.js:53), unclaim (controllers/unclaim.js:16).
**PERF**: per-room bulk on the shared users collection; set semantics → mergeable per user.

---

## 7. Terminals and transactions

### TERM-SEND-RT
reference/engine/src/game/structures.js:714-748, order: !my → ERR_NOT_OWNER; inactive vs RCL →
ERR_RCL_NOT_ENOUGH; target not `/^(W|E)\d+(N|S)\d+$/` → ERR_INVALID_ARGS; resource ∉
RESOURCES_ALL → ERR_INVALID_ARGS; `store[res] < amount` (or missing) → ERR_NOT_ENOUGH_RESOURCES;
`cooldownTime > time` → ERR_TIRED; energy cost (continuous distance, no power effect applied)
`energy < cost` (or `< amount+cost` for energy) → ERR_NOT_ENOUGH_RESOURCES; description not string
or > 100 → ERR_INVALID_ARGS. `intents.set(id,'send',{...})` (last call per tick wins).
Note: `amount` is not validated as positive/integer here; sanitizer `parseInt`s it.

### TERM-SEND-RP
terminal/send.js:6-33 (room stage, before object ticks): regex, resource ∈ RESOURCES_ALL,
`amount` truthy and `store[res] >= amount`; cost with PWR_OPERATE_TERMINAL if `effect.endTime >= T`;
energy check; then `bulk.update(terminal, {send: intent})` (persisted on the object).
No cooldown check, no ownership/RCL check.
**GOTCHA**: power-effect comparison is `>=` here but `>` in the global stage.

### TERM-SEND-GP
market.js:71-95. For each terminal with `send` (storage order): clear `send` (always); if
`cooldownTime > T` → stop; target = `terminalsByRoom[targetRoomName]` (last terminal per room);
missing or owner-less → stop; `cooldown = TERMINAL_COOLDOWN (10)`, with sender's
PWR_OPERATE_TERMINAL (`endTime > T`) `round(10·effect[level−1])`; `executeTransfer(sender, target,
res, amount, feeTerminal = sender, {description: description.replace(/</g,'&lt;')})`; on `true` set
`cooldownTime = T + cooldown`. Effective: runtime `cooldown` = 9 at T+1, usable again at T+10.

### TERM-TRANSFER
`executeTransfer(from, to, res, amount, fee, extra)` (market.js:15-69):
```
if !from || !to || !fee: return false
if from.user && store[res] missing or < amount: return false
if to.user: amount = min(amount, max(0, to.storeCapacity − Σ to.store))
if !(amount > 0): return undefined            // GOTCHA: not false; no cooldown, send cleared
range = calcRoomsDistance(from.room, to.room, continuous=true)
cost  = ceil(amount·(1 − exp(−range/30)))
if fee has PWR_OPERATE_TERMINAL with endTime > T: cost = ceil(cost·effect[level−1])
if fee == from and (res != energy and from.energy < cost or res == energy and from.energy < amount+cost)
   or fee == to and to.energy < cost: return false
if to.user: to.store[res] += amount            (update by object → mutates)
from.store[res] −= amount                      (NPC terminal: undefined − n = NaN is written)
fee.store.energy −= cost                       (sees the two mutations above)
insert transaction {time:T, sender?, recipient?, resourceType, amount, from, to, ...extra}
return true
```
Capacity clamp uses the target's **current** `storeCapacity` field (0 if the target's RCL/owner
does not allow a terminal, TERM-CAPACITY) and current in-memory store, i.e. after every room
stage effect of tick T and every earlier send/deal of this global stage. Resources arrive in the
same tick's global stage; visible at T+1.
**GOTCHA**: cost is computed on the clamped amount; the runtime pre-check used the unclamped one.
**PERF**: sends form a dependency chain through shared terminals (A→B then B→C changes B's free
space). Keep sends+deals serial in storage order; cost is O(#sends + #deals) per tick (≤ ~11 per
user), negligible in Rust.

### TERM-COST
`calcRoomsDistance(a, b, continuous)` (reference/engine/src/utils.js:644-655): Chebyshev over room
coordinates; if continuous, each axis `d = min(worldSize − d, d)` (torus wrap).
`calcTerminalEnergyCost(amount, range) = ceil(amount·(1 − exp(−range/30)))` (utils.js:657-659).
`Game.market.calcTransactionCost` uses the same (reference/engine/src/game/market.js:31-34).
**GOTCHA**: `Math.exp` must be bit-identical to V8 for the `ceil`.

### TERM-WORLDSIZE
`calcWorldSize` (reference/common/index.js:183-192) starts with `maxX = maxY = 0` and
`minX = minY = Infinity` and returns `max(maxX−minX+1, maxY−minY+1)` over all rooms in DB, computed
once at process start (reference/driver/lib/index.js:135-137).
**GOTCHA**: for worlds entirely in W/N (negative coordinates) `maxX` stays 0, so the size is one
larger than the real extent. Default world W0..W10 × N0..N10 → x ∈ [−11,−1] → worldSize 12, and
W0N0↔W10N0 has wrapped distance `min(12−10, 10) = 2`. Replicate exactly (including that rooms
added after start do not change it until restart, or document a deliberate deviation).

### TERM-CAPACITY
terminal/tick.js:6-15: if the room has a controller: `storeCapacity = (controller.level > 0 &&
controller.user == terminal.user && CONTROLLER_STRUCTURES.terminal[level]) ? TERMINAL_CAPACITY : 0`.
Room without controller: unchanged.

### TERM-TRANSACTIONS
Transaction docs as above (deals add `order:{id, type, price: price/1000}`). Runtime
`Game.market.outgoingTransactions` / `incomingTransactions` = 100 newest by `time` desc for
sender / recipient (reference/driver/lib/runtime/data.js:138-139), mapped to `{transactionId,
sender:{username}, recipient:{username}, ...}` (reference/engine/src/game/market.js:203-233).

### TERM-PURGE
cronjobs.js:443-470 (hourly wall): sort all transactions by `−parseInt(time)` (stable), keep any
transaction that is within the newest 100 for its sender **or** its recipient (bucket key may be
`undefined` for NPC), delete the rest.

---

## 8. Market

### MKT-RT-VALIDATION
reference/engine/src/game/market.js:
- `createOrder(type|obj, resourceType, price, totalAmount, roomName)` (68-98): resource ∉
  RESOURCES_ALL ∪ INTERSHARD_RESOURCES → ERR_INVALID_ARGS; type ∉ {buy,sell} → ERR_INVALID_ARGS;
  `price=parseFloat`, `totalAmount=parseInt`; `!price || price<=0 || !totalAmount` →
  ERR_INVALID_ARGS (negative totalAmount passes!); `price·total·MARKET_FEE > credits` →
  ERR_NOT_ENOUGH_RESOURCES; non-intershard and no own terminal in `roomName` → ERR_NOT_OWNER;
  `size(myOrders) + createdThisTick >= MARKET_MAX_ORDERS (300)` → ERR_FULL; push; OK.
- `cancelOrder(id)` (100-106): not own order → ERR_INVALID_ARGS; push (cap 50); OK even if dropped.
- `deal(id, amount, room)` (108-153): order must be in the **active** cache → else
  ERR_INVALID_ARGS; `amount=parseInt`, `!amount||amount<0` → ERR_INVALID_ARGS; intershard: buy
  order and `resources[r] < amount` → ERR_NOT_ENOUGH_RESOURCES; else room required
  (ERR_INVALID_ARGS), own terminal (ERR_NOT_OWNER), `energy < cost` (ERR_NOT_ENOUGH_RESOURCES),
  cooldown (ERR_TIRED), buy order: resource/energy+cost check; sell order:
  `money < amount·order.price` (both milli) → ERR_NOT_ENOUGH_RESOURCES; push cap 10 → ERR_FULL.
- `changeOrderPrice` (155-172), `extendOrder` (174-191): own order, positive parse, fee vs credits.
**GOTCHA**: runtime OK does not imply success; the processor silently drops on its own checks
(ceil'd fee, `totalAmount <= 0`, etc.).

### MKT-RT-VIEW
`getAllOrders` / `getOrderById` read a per-runner-process cache of `market.orders` with
`active: true`, refreshed once per gameTime (reference/driver/lib/runtime/data.js:31-49); prices
divided by 1000 on first access per resourceType (market.js:13-27). `Game.market.orders` = all own
orders (active or not) (data.js:227, market.js:235-249). `getHistory` = `market.stats`, cached
3 h wall-clock per process (data.js:51-75).

### MKT-CREATE
market.js:109-166, per intent: `!price || !totalAmount` → drop; resource ∉ RESOURCES_ALL ∪
INTERSHARD_RESOURCES → drop; non-intershard and (no terminal in roomName or terminal.user ≠ user)
→ drop; `price <= 0 || totalAmount <= 0` → drop; `fee = ceil(price·total·MARKET_FEE)`;
`user.money < fee` → drop; `inc(user, money, −fee)` (mutates); insert order =
`extend({createdTimestamp: Date.now() (stage start), user, active:false,
type: (intent.type == 'sell' ? 'sell' : 'buy'), amount:0, remainingAmount: total}, intent)`
(+`created: T` for non-intershard). Ledger `market.fee` entry with `balance` after deduction.
**GOTCHA**: `_.extend(defaults, intent)` overwrites the normalised `type` with the raw string;
`roomName` of an inter-shard order is the string `"undefined"` (GINT-SANITIZE).
**GOTCHA**: new orders are not in this tick's `ordersById`; they cannot be dealt/cancelled/changed
this tick and stay `active:false, amount:0` until the next tick's MKT-ORDER-MAINTAIN, so other
players can first see them at T+2.

### MKT-CHANGE-PRICE
market.js:168-213: order must exist and belong to the user; `!newPrice || newPrice <= 0` → drop;
if `newPrice != price` set `order._skip = true` (**before** the fee check); if increasing:
`fee = ceil((new−old)·remainingAmount·MARKET_FEE)`, `money < fee` → drop (but `_skip` stays);
charge + ledger; `inc(order, 'price', new − old)` (mutates price).
**GOTCHA**: `_skip` blocks all later-collected deals on that order this tick; deals of users whose
intents doc was processed **earlier** were already collected and execute at the **new** price.

### MKT-EXTEND
market.js:215-253: own order, `addAmount > 0`, `fee = ceil(price·addAmount·MARKET_FEE)`,
`money < fee` → drop; charge + ledger; `inc remainingAmount` and `inc totalAmount`.

### MKT-CANCEL
market.js:256-263: own order → in memory `remainingAmount = 0`, `_cancelled = true`. Removed in
MKT-ORDER-MAINTAIN with **no refund**. Cancel always wins over same-tick deals (deals run later and
see remaining 0).

### MKT-DEAL-COLLECT
market.js:265-285: `intent.user = dealer`; order missing or `_skip` → drop; `amount <= 0` → drop
(NaN passes!); inter-shard resource → `directDeals`; else dealer must own the terminal in
`targetRoomName` → `terminalDeals`. Order active-ness is **not** checked here.

### MKT-DEAL-TERMINAL
Sort `terminalDeals` ascending by continuous distance(targetRoomName, order.roomName) (stable;
ties keep collection order) (market.js:289). Then per deal (291-398):
```
orderT = terminalsByRoom[order.roomName]; targetT = terminalsByRoom[deal.targetRoomName]
if !orderT || !targetT: skip
if targetT.cooldownTime > T: skip               // one deal (or send) per dealer terminal per tick
(buyer, seller) = order.type == sell ? (targetT, orderT) : (orderT, targetT)
amount = min(deal.amount, order.remainingAmount)
if seller.user: amount = min(amount, seller.store[res] || 0)
if buyer.user:  amount = min(amount, max(0, buyer.storeCapacity − Σ buyer.store))
if !(amount > 0): skip
cost = amount·order.price
if buyer.user: cost = min(cost, money(buyer.user)||0); amount = floor(cost/price); cost = amount·price; if !amount skip
if executeTransfer(seller, buyer, res, amount, fee = targetT, {order:{id,type,price/1000}}):
   seller.user? money += cost, ledger 'market.sell' {resourceType, roomName, targetRoomName, price, npc: !buyer.user, owner, dealer, amount}
   buyer.user?  money −= cost, ledger 'market.buy'
   order.amount −= amount; order.remainingAmount −= amount   (by object)
   targetT.cooldownTime = T + (TERMINAL_COOLDOWN or round(10·effect) if targetT has OPERATE_TERMINAL)
```
The dealer always pays the energy fee. "Money" is the in-memory user object (already reduced by
fees/deals this stage). NPC side (no `user`): no stock, capacity or money limits.
**GOTCHA**: the docs' "shortest distance takes precedence" is exactly this sort; the per-terminal
cooldown makes only the first successful deal per dealer terminal count.
**GOTCHA**: `order.amount` may go negative here; it is overwritten by MKT-ORDER-MAINTAIN.
**GOTCHA**: self-dealing (same terminal both sides) is allowed and nets to the energy fee.
Edge: if the order terminal's owner is not in the loaded `users` set the stage throws
(TypeError) — failure behaviour, do not replicate.

### MKT-DEAL-DIRECT
market.js:400-495: `directDeals = _.shuffle(directDeals)` (**Math.random**). Per deal: buyer/seller
are user docs (sell order: dealer buys); missing → skip; `amount = min(deal.amount, order.amount,
order.remainingAmount, seller.resources[r]||0)`; `!amount || amount<0` → skip; `cost =
amount·price`; the buyer money check tests `buyer.user`, which user docs do not have → **never
enforced** (money can go negative). Seller `inc money +cost`, `inc 'resources.r' −amount`,
`seller.resources[r] −= amount`; buyer `inc money −cost` **and** `buyer.money −= cost` (double
in-memory decrement — affects later checks/ledger balances only), `inc 'resources.r' +amount`;
order `inc amount −n`, `inc remainingAmount −n`; ledgers incl. `users.resources` entries.
See BULK-DOTKEY for why resources do not actually move on LokiJS.

### MKT-ORDER-MAINTAIN
market.js:497-589, for every order loaded at stage start (storage order):
1. `_cancelled` → remove.
2. `order.user && Date.now() − createdTimestamp > MARKET_ORDER_LIFE_TIME` → MKT-ORDER-EXPIRE.
3. NPC orders (no user) → untouched.
4. Sell: `avail = intershard ? user.resources[r]||0 : (terminal && terminal.user == order.user ?
   terminal.store[r]||0 : 0)`; `avail = min(avail, remainingAmount)`; if active: `!avail || avail<0`
   → `{active:false, amount:0}`, else if `amount != avail` → `{amount: avail}`; if inactive and
   `avail > 0` → `{active:true, amount:avail}`.
5. Buy: `amt = min(floor(money/price), remainingAmount)`; if the room has an owned terminal
   (any owner!) clamp by its free capacity; `active = (intershard || terminal.user == order.user)
   && amt > 0`; write if `amount` or `active` changed.
All inputs are post-stage in-memory values (stores after sends/deals, money after fees/deals).
**GOTCHA**: fully filled orders are deactivated, never deleted (except expiry/cancel); they keep
counting toward MARKET_MAX_ORDERS in the runtime.
**PERF**: original is O(#orders) per tick. Incremental chitin version: recompute an order only if
its terminal (store/capacity/owner), its owner's money/resources, or the order itself changed this
tick — must produce identical writes.

### MKT-ORDER-EXPIRE
market.js:507-534 (wall-clock 30 days, `MARKET_ORDER_LIFE_TIME`): refund `remainingAmount·price·
MARKET_FEE` **without rounding** (money may become fractional milli-credits), ledger, remove.

### MKT-MONEY-LOG
`users.money` docs `{date, tick:T, user, type: market.fee|market.sell|market.buy, balance
(credits, after change), change (credits), market:{...}}`; `users.resources` docs for direct deals.
Read by HTTP money-history and MKT-STATS only (not by the runtime).

### MKT-NPC-ORDERS
cronjobs.js:38-122 (every 300 s wall), sequential over owner-less terminals: skip rooms not
`normal`; skip if `nextNpcMarketOrder > T`; draw sell mineral and buy mineral uniformly from the
17-item lists (H/O weighted ×4, Z/K/L/U ×2, X ×1); orders: sell mineral `200000` @
`{H:3000,O:3000,Z/K/U/L:6000,X:18000}` milli; 50 %: sell energy 200000 @1000; 25 %: buy mineral
100000 @1000; all `active:true`, `created:T`, no `user`. Remove **all** orders with this
`roomName`, insert the new ones, `nextNpcMarketOrder = T + round(5000·(0.8+0.4·rand))`.
Default world has no NPC terminals (launcher/init_dist/db.json) → no-op there.

### MKT-STATS
cronjobs.js:472-523 (hourly wall): wipe `market.stats`; from `users.money` entries of type
`market.sell` in the last 14 days (wall), group by resource and **server-local** date
`YYYY-MM-DD`: `avg = Σchange/Σamount`, `stddev = sqrt(Σ amount·(price−avg)²/Σamount)`, insert
`{resourceType, date, transactions, volume, avgPrice: +avg.toFixed(3), stddevPrice}`.
**GOTCHA**: purchases from NPC sell orders produce no `market.sell` entry → not in history.

### MKT-DEAD-RETURN
The closure returned by the market processor (incl. `driver.clearMarketIntents`, which does not
exist in the driver) is never invoked (global.js:48-49 ignores it). Do not port.

---

## 9. Notifications

### NOTIFY-GAME
Runtime `Game.notify(message, groupInterval)` → push (cap 20) → OK / ERR_FULL
(reference/engine/src/game/game.js:174-181). Saved by the runner (reference/driver/lib/index.js:
185-227): take first 20; `groupInterval` (parseInt, may be NaN) clamped to [0,1440] minutes,
×60000, floored; `date = gi ? ceil(now/gi)·gi : now` (wall-clock bucket end); `message =
(""+msg).substring(0,500)`; upsert `users.notifications {user, message, date, type:'msg'}`
`$inc count`. Identical messages in the same bucket coalesce.

### NOTIFY-ENGINE
`driver.sendNotification(user, message)` (reference/driver/lib/index.js:455-471): upsert matching
`{user, message, date <= now, type:'msg'}` → `$set date: now`, `$inc count`. Sources:
attacked objects with `notifyWhenAttacked` and attacker not NPC
(reference/engine/src/utils.js:438-454, reference/engine/src/processor/intents/_damage.js:77-80),
controller upgraded (upgradeController.js:73), downgrade warning at exactly `T == downgradeTime−3000`
and each downgrade (controllers/tick.js:45-51), newbie wall warning at `T == decayTime−5000`
(constructedWalls/tick.js:34-36).

### NOTIFY-ERROR
`sendConsoleError` (reference/driver/lib/index.js:371-410): NPC users only logged; others: upsert
type `error` bucketed by `notifyPrefs.errorsInterval` (default 30 min) and publish to console.

### NOTIFY-CRON
cronjobs.js:124-176 (every 60 s wall): notifications with `date < now`, per user sequentially: if
`notifyPrefs && (disabled || !email)` → delete them; else if `lastNotifyDate + interval` (default
60 min) is in the future → keep; else emit `config.backend 'sendUserNotifications'(user, [{message,
date, count, type}])` (mod hook; launcher example mailer), delete them, set `lastNotifyDate`.
Not observable by bots beyond Game.notify's return code.

---

## 10. Users lifecycle

### USER-RUN-SET
`getAllUsers` = users with `active != 0` and `cpu > 0`, sorted by `lastUsedDirtyTime` desc
(reference/driver/lib/index.js:144-151). NPC users '2' (Invader) and '3' (Source Keeper) have
`cpu: 0` and never run; their creeps are driven by room-stage pretick AI.

### USER-DEACTIVATE
Runtime data load: if the user owns no `rooms.objects` → `$set active: 0` and skip the run
(reference/driver/lib/runtime/data.js:103-109). Re-activated (`active: 10000`) by code upload
(reference/backend/lib/game/api/user.js:102), place-spawn (game.js:405), bots.spawn
(cli/bots.js:79). `userNotActive` checked in room activation (processor.js:119) is never set by
OSS code.

### USER-WORLD-STATUS
reference/backend/lib/utils.js:88-113: no objects → `empty`; else `normal` if any own spawn sits in
a room whose controller the user owns, otherwise `lost`.

### USER-RESPAWN
`POST /user/respawn` requires status normal or lost (reference/backend/lib/game/api/user.js:182-193).
`respawnUser` (reference/backend/lib/utils.js:115-168): requires system user `Screeps`; delete own
creeps, powerCreeps (room objects), constructionSites; delete **all** `users.power_creeps` of the
user; every own structure with CONSTRUCTION_COST → ruin owned by the system user (`decayTime =
T + 500000`, store copied); reassign own ruins to the system user; delete remaining own objects
except controllers; delete own flags; reset owned controllers (level 0, user null, safe mode
fields cleared, isPowerEnabled false); `rooms = []`. GCL/GPL/money untouched.
**GOTCHA**: the intended room `status: 'normal'` reset loop iterates a Promise (missing `await`)
and does nothing. `lastRespawnDate` (checked by place-spawn) is never written by OSS code.

### USER-PLACE-SPAWN
reference/backend/lib/game/api/game.js:328-410: validates x,y ∈ [0,49], name ≤ 50, not blocked,
`cpu > 0`, 180 s since `lastRespawnDate`; room exists, is W/E, not out of borders/closed;
user has no objects; controller exists, unowned, unreserved, `bindUser` matches; spot rules
(checkConstructionSpot 63-95). Then: remove other users' creeps/construction sites in the room;
convert other users' structures to ruins (decay 100000); claim controller `{level:1, progress:0,
downgradeTime:null, safeMode: T+20000}`; `invaderHarvested = 0` on sources; insert spawn
`{store.energy: SPAWN_ENERGY_START, ...}`; `rooms.invaderGoal = 1000000`; activate room;
`active = 10000`. No novice-area restriction in OSS.

### USER-BOT-SPAWN
reference/backend/lib/cli/bots.js:70-124: creates user (cpu default 100, `gcl` from level, `active:
10000`), code, memory `{}`, a spawn at random non-wall x,y ∈ [3,48], claims controller with
`safeMode: T+20000`, `invaderGoal = 1000000`, activates room. `rooms` list is filled lazily by
USER-ROOMS.

### USER-CPU
reference/driver/lib/runtime/make.js:54-96,192-212 (runner, per user): if `skipTicksPenalty > 0`
→ skip tick, decrement. If `cpuAvailable < 0` → skip the tick and set `cpuAvailable =
(cpuAvailable < −2·cpu) ? −2·cpu : cpuAvailable + cpu` (make.js:72-82). Else tick limit `min(cpuAvailable + cpu, cpuMaxPerTick=500)`. After run:
`used = ceil(usedCleanTime + 0.2·#intents(excluding say/pull))`;
`cpuAvailable = min(cpuAvailable + cpu − used, cpuBucketSize=10000)`. Users without `cpu` get
Infinity. (Cross-ref: runtime/CPU spec owns exact timing semantics.)

### USER-SHARDACCESS
`users[uid].shardAccess === false` blocks upgradeController (upgradeController.js:13-15) and claim
(claimController.js:37-39; runtime ERR_ACCESS_DENIED creeps.js:863-865). Never set by OSS code.

---

## 11. Room status and activation

### ROOMSTAT-ACCESSIBLE
A room is accessible iff `status == 'normal'` and (`!openTime || openTime < Date.now()`)
(wall-clock). Global stage reads it fresh from DB every tick (XROOM-APPLY); runtime uses env
`ACCESSIBLE_ROOMS` refreshed every 21 ticks (main.js:79-83) and cached 60 s wall per runner
process (reference/driver/lib/runtime/data.js:77-86); also feeds the runtime map grid.

### ROOMSTAT-DATA
`updateRoomStatusData` (reference/driver/lib/index.js:498-524), every 21 ticks: rooms with
`novice > now` → `novice[room] = novice`; else `respawnArea > now` → `respawn`; else `openTime >
now` → `closed[room] = openTime`; else (status 'out of borders') → `closed[room] = Infinity`.
Serialized with JSON (`Infinity` → `null`). Cached 60 s wall per runner process (data.js:88-97).

### ROOMSTAT-RT
`Game.map.getRoomStatus(name)` (reference/engine/src/game/map.js:209-233): non-matching name →
undefined; missing data → throws; closed → `{status:'closed', timestamp}` (null for out of
borders); novice; respawn; accessible → `{status:'normal', timestamp:null}`; otherwise
`{status:'closed', timestamp:null}`. Precedence closed > novice > respawn here, but ROOMSTAT-DATA
already chose one bucket with novice > respawn > closed.

### ROOMSTAT-CLI
`map.openRoom(room, [timestamp])` sets `status:'normal'` (+`openTime`); `closeRoom` sets
`status:'out of borders'` only if currently normal; `removeRoom` deletes room, objects, flags,
terrain, intents (reference/backend/lib/cli/map.js:568-609). `novice` / `respawnArea` timestamps
are only set by direct DB edits in OSS.

### ROOMSTAT-NOVICE-EFFECTS
Complete list in OSS: nuke launch from a novice/respawn room (NUKE-LAUNCH-RP) and nukes flying into
one (NUKE-FLIGHT, NUKE-LAUNCH-RT); claim cap GCL_NOVICE (GCL-CLAIM-CHECK); `safeModeCooldown`
null instead of `T+SAFE_MODE_COOLDOWN` on activation/downgrade/unclaim in novice rooms
(controllers/tick.js:29,61,68; controllers/unclaim.js:25); stronghold expansion skips them
(SH-EXPAND). **GOTCHA**: there is no movement barrier between novice/respawn and normal areas in
the engine (official uses wall objects with timestamp `decayTime`, handled by
constructedWalls/tick.js:39-44).

### ACTIVE-SET
Only rooms in `ACTIVE_ROOMS` are processed. A processed room re-adds itself
(processor.js:504-507) if any of (processor.js:106-142,499-502): controller owned by a non-'2' user;
invader reservation with `endTime − T < CONTROLLER_RESERVE_MAX − INVADER_CORE_CONTROLLER_POWER·
CONTROLLER_RESERVE`; any object with a `user` ∉ {'3'} that is not a flag, not a stronghold object,
not a controller (and not `userNotActive`); powerBank/deposit within 500 ticks of decay; dropped
energy; nuke; tombstone; portal; invader core with `deployTime`; `roomInfo.active` set by intents.
Other activators: rooms receiving player intents (driver saveUserIntents), XROOM-APPLY targets,
cron inserts (power bank, deposit, invaders, strongholds), roomsForceUpdate.
**GOTCHA**: activity is semantically visible (e.g. `T == downgradeTime−3000` notifications, source
regen timers start only when processed, room history/event log only for processed rooms).
Chitin must reproduce the active set exactly, or prove that processing an idle room is a no-op for
every object type.
**PERF**: this is the original's main scaling device; chitin should keep it (most of a 1000-player
world is idle on any tick) and implement it as a per-tick bitset.

### ACTIVE-FORCE-CRON
cronjobs.js:178-192 (every 20 s wall): `T` is read once, then the room list is one query
`{status: {$ne: 'out of borders'}, _id: {$nin: activeRooms}}` (180-181, see CRON-ACTIVE-VIEW for
what `activeRooms` contains); for each listed room (sequential, awaited) with
`!nextForceUpdateTime || T >= nextForceUpdateTime` → activate, `nextForceUpdateTime = T + 90 +
floor(rand·20)`. Owned / occupied rooms pass the filter whenever the cron catches them inside their
processing window, so their (unobservable) `nextForceUpdateTime` is also rewritten.
**Chitin**: same eligibility rule as genInvaders (CRON-ACTIVE-VIEW): every room not 'out of borders'
whose deadline is unset or `<= T`; activating a room already scheduled for T+1 is a no-op.

### CRON-ACTIVE-VIEW
What `activeRooms = await utils.getActiveRooms()` (= SMEMBERS `ACTIVE_ROOMS`,
reference/backend/lib/utils.js:315-317) returns depends on where the main loop is when the cron
fires, because crons are wall-clock and run concurrently with the tick (CRON-SCHEDULER). It is read
once per run, before the sequential room loop, by roomsForceUpdate (cronjobs.js:180-181) and
genInvaders (cronjobs.js:377-378) as `_id: {$nin: activeRooms}`.

Set lifecycle within tick T (stage order reference/engine/src/main.js:28-55):
1. Users phase: each runner's `saveUserIntents` SADDs every room that received player intents
   (reference/driver/lib/index.js:234-240). Rooms that re-added themselves at the end of T−1 are
   already present → the set is "rooms to process at T".
2. Rooms phase start: `getAllRoomsNames` = SMEMBERS, then DEL (reference/driver/lib/index.js:170-174).
   The key is now **missing**, not empty.
3. Each room re-adds itself only when **its own** processing finishes, if it stays active
   (reference/engine/src/processor.js:499-505; conditions in ACTIVE-SET; the SADD is not awaited).
   Cron `activateRoom` calls also SADD (reference/backend/lib/utils.js:311-313).
4. Global stage, gameTime increment, next users phase: only intents (step 1) and crons add.

Consequences:
- Idle rooms are never in the set. Rooms that stay active every tick (owned controller, player
  creeps — including remote/reserved rooms with creeps, see ACTIVE-SET) are absent exactly from the
  DEL until the end of their own processing. A cron run sees such a room as eligible with probability
  ≈ (wall time from DEL to that room's end) / (wall time per tick); the share grows when the users
  phase gets short relative to the rooms phase (fast tick loops).
- So on the private server owned and remote rooms **do** get raids: an owned room whose goal is
  reached is raided on the first 5-min run that happens to fall into its window (geometric delay).
  This is intended: genInvaders passes `controller && controller.user && controller.level`
  (cronjobs.js:430-431) so owned RCL ≥ 4 rooms get 'big' raids (INV-RAID), and the official docs
  describe light invaders in "neutral, reserved, and claimed rooms up to level 3" and heavy ones in
  claimed rooms RCL ≥ 4 (reference/docs/source/invaders.md:4,20-21).
- **GOTCHA (LokiJS, stock storage)**: between the DEL and the first SADD of the rooms phase,
  `dbEnvSmembers` returns `undefined` for the missing key (reference/storage/lib/db.js:750-758; DEL
  removes the key even when it holds `[]`, db.js:630-640). The query goes over JSON RPC
  (reference/common/lib/storage.js:95, reference/common/lib/rpc.js:49), where `{$nin: undefined}`
  serialises as `{}`: storage receives `_id: {}` with no operator and the LokiJS `find` (lokijs
  ^1.5.6, reference/storage/package.json:16; library source not in reference) throws. That cron run
  rejects and does nothing: no raids, no forced updates.
- Redis storage (screepsmod-mongo, not in reference): SMEMBERS of a missing key is `[]`, so in the
  same interval nothing is excluded and every room passes the filter.
- Other races of the original (do not replicate): SMEMBERS and DEL are two separate RPCs, so a cron
  SADD landing between them is lost (the room is neither processed at T nor scheduled). A raid
  inserted into a room that is mid-processing survives (bulk writes are by `_id`), but the
  `invaderHarvested: 0` reset (cronjobs.js:438) is overwritten if that tick's harvest
  `bulk.update(source, {invaderHarvested})` executes later
  (reference/engine/src/processor/intents/creeps/harvest.js:45-48). The goal then stays met and
  the room can be raided again on a later run.

**Chitin rule** (shared by INV-GEN and ACTIVE-FORCE-CRON): world jobs run at the post-global barrier
(CRON-SCHEDULER). The original's exclusion is read as "rooms whose processing for the current tick
is still pending". At the barrier every room of tick T has finished, so that set is empty and the
active-set filter excludes **nothing**:
- genInvaders: `status == 'normal'` plus INV-GEN steps 1-4. Owned, reserved and remote rooms are
  raided on the first job run after their goal is reached (the fast-tick limit of the original).
- roomsForceUpdate: as in ACTIVE-FORCE-CRON above.
- Do **not** test against the next-tick active set (rooms scheduled for T+1). Owned rooms and
  rooms with player creeps are always in it, so they would never be raided. That diverges from the
  private server and the official game and leaves bots' invader-defence code untested.
- The view can never be "missing", so a job run is never aborted for that reason.
**PERF**: no per-room set lookup. Keep a per-room "goal reached" flag (`goal == 1 || Σ
invaderHarvested >= goal`) maintained by INV-HARVEST / INV-GOAL and scan only flagged rooms;
roomsForceUpdate is a timer wheel keyed by `nextForceUpdateTime`.

---

## 12. Observer visibility

### OBS-RT
reference/engine/src/game/structures.js:548-569: !my → ERR_NOT_OWNER; name regex →
ERR_INVALID_ARGS; inactive → ERR_RCL_NOT_ENOUGH; without an active PWR_OPERATE_OBSERVER
(`ticksRemaining > 0`), `|dx| > OBSERVER_RANGE || |dy| > OBSERVER_RANGE` (non-wrapping) →
ERR_NOT_IN_RANGE; `intents.set` (last call wins).

### OBS-RP
At room load every observer gets `observeRoom = null` in memory (processor.js:116-118); an
`observeRoom` intent sets it to `intent.roomName` without re-validation (284-288); in the object
loop it is always persisted (403-406).

### OBS-VISIBILITY
Runtime room set = rooms of own objects (except flags / construction sites) ∪ each own observer's
`observeRoom` (reference/driver/lib/runtime/data.js:114-126). So an observation issued at T is
visible exactly at T+1 (full room objects of other users included; the room must exist in DB to
create `Game.rooms[name]`). Observer rooms are owned → always active → the field is reset every tick.

---

## 13. Nukes

### NUKE-LAUNCH-RT
reference/engine/src/game/structures.js:1356-1387: !my; `pos` not RoomPosition → ERR_INVALID_ARGS;
source room novice/respawn (`runtimeData.rooms`) or target in `roomStatusData.novice/respawn`
(stale ≤ 21 ticks + 60 s) → ERR_INVALID_TARGET; cooldown → ERR_TIRED; inactive →
ERR_RCL_NOT_ENOUGH; non-wrapping `|dx|,|dy| > NUKE_RANGE (10)` → ERR_NOT_IN_RANGE; not full
energy+G → ERR_NOT_ENOUGH_RESOURCES.

### NUKE-LAUNCH-RP
launch-nuke.js:8-51: active vs RCL; full G and energy; cooldown; x,y ∈ [0,49]; **source** room
novice/respawn → drop; name regex; range. Then nuker `store {energy:0, G:0}`, `cooldownTime = T +
NUKER_COOLDOWN (100000; 100 on PTR)`; insert into the **target room** via the source room's bulk
`{type:'nuke', room: target, x, y, landTime: T + NUKE_LAND_TIME (50000; 100 on PTR),
launchRoomName}`. The target room is not activated and need not exist.
**PERF/X**: the only room-stage write into another room in this area → chitin needs a cross-room
insert mailbox applied before T+1.

### NUKE-FLIGHT
nukes/tick.js:10-14: whenever the target room is processed and its `novice` or `respawnArea` is in
the future (wall-clock), the nuke is removed. Any processed room with a nuke stays active
(ACTIVE-SET), so after its first processing (ACTIVE-FORCE-CRON at the latest) the target room is
processed every tick for up to 50000 ticks. A nuke aimed at a non-existent room is never processed.
**PERF**: chitin can keep the room asleep and schedule a wake-up at `landTime−1` only if it proves
idle processing is a no-op (see ACTIVE-SET); 1:1 default is to keep it active.

### NUKE-PRETICK
nukes/pretick.js:6-15 (before intents): if `landTime == T+1`, null every `createCreep` intent of
every user in the room (spawning is prevented the tick before impact).

### NUKE-LAND
nukes/tick.js:16-78 at `T == landTime − 1`, in room-object iteration order:
1. Every creep in the room dies (`_die(…, EVENT_ATTACK_TYPE_NUKE)`), power creeps get `hits: 0`
   (die in the global stage), construction sites / dropped resources / tombstones / ruins are
   removed, spawns stop spawning.
2. For dx,dy ∈ [−2,2] (dx outer): `damage = range==0 ? NUKE_DAMAGE[0] (10M) : NUKE_DAMAGE[2] (5M)`;
   a rampart on the tile takes the damage first (`_damage` with NUKE type ignores
   fortify/invulnerability) and `damage −= rampartHitsBefore`; if `damage > 0` every other object
   on the tile is damaged. Structures destroyed by nukes leave **no ruin** (_destroy.js:21).
3. Controller: active safe mode → `safeMode: T, safeModeCooldown: null`; then (JS precedence)
   `(user && !invulnerable && !upgradeBlocked) || upgradeBlocked < T` → `upgradeBlocked = T +
   CONTROLLER_NUKE_BLOCKED_UPGRADE (200)`.
   **GOTCHA**: the `||` branch also applies to unowned controllers with an old `upgradeBlocked`.
4. `T >= landTime` → remove the nuke (so the object is still present for one tick after impact).
Multiple nukes landing the same tick apply sequentially in object order.

---

## 14. Portals, inter-shard, pixels

### PORTAL-DECAY
portals/tick.js:6-25: `unstableDate` (wall-clock ms) passed → `decayTime = T + PORTAL_DECAY
(30000)`, clear `unstableDate`; `T > decayTime` → remove portal and any `constructedWall` at
(x+1, y+1). OSS has no portal generator (only manual inserts); travel is XROOM-PORTAL/XROOM-APPLY.

### SHARD-NAME
`Game.shard = {name: os.hostname(), type: 'normal', ptr: false}` defined on every sandbox
(reference/driver/lib/index.js:35-52). **GOTCHA**: hostname-dependent; differs from PC `shard`
(''). Chitin: configurable shard name (see Open questions).

### SHARD-INTERSHARD
Not implemented in OSS: `RawMemory.interShardSegment` is a writable property that is never saved
(runtime.js:125-128); no `InterShardMemory`, no `Game.cpu.setShardLimits`, no inter-shard
portals. INTERSHARD_RESOURCES market path exists (MKT-DEAL-DIRECT).

### PIXEL-GENERATE
`Game.cpu.generatePixel` does not exist in OSS (`cpu` object, game.js:144-157; constants only
`PIXEL`, `PIXEL_CPU_COST: 10000`). Official semantics per docs
(reference/docs/api/source/Game.md:300-316): cost 10000 bucket → 1 pixel, ERR_NOT_ENOUGH_RESOURCES.
See Open questions.

---

## 15. Cron scheduler

### CRON-SCHEDULER
reference/backend/lib/cronjobs.js:14-36; `setInterval(run, 1000)` in the backend process
(reference/backend/lib/index.js:20). Each job runs when `Date.now() − lastRun > interval·1000`;
`lastRun` starts undefined ⇒ every job runs within 1 s of backend start. Jobs are fire-and-forget
(not awaited, may overlap themselves). Intervals (seconds): sendNotifications 60,
roomsForceUpdate 20, genPowerBanks 300, genInvaders 300, purgeTransactions 3600,
recreateNpcOrders 300, calcMarketStats 3600, deletePowerCreeps 600, genDeposits 300,
genStrongholds 300, expandStrongholds 900. `config.cronjobs` is a mutable map → mods add jobs.
**GOTCHA**: crons run concurrently with the tick pipeline and write straight to storage; an
insert may land between runtime and room stage of a tick (processor sees it, runtime did not).
Crons that read the active set see a different set depending on the moment (CRON-ACTIVE-VIEW).
**GOTCHA**: tick-based deadlines (powerBankTime, nextNpcMarketOrder, nextExpandTime, ...) are only
checked at wall-clock cadence; at higher TPS the effective tick granularity grows (5 min at 50 tps
= 15000 ticks). Chitin should run world jobs at a fixed point of the tick (after the global stage)
on a configurable tick cadence; default cadence is an Open question.
**PERF**: all world-gen jobs are per-room or per-sector independent (Z) and can run on worker
threads; the original does sequential awaited DB round-trips per room (slow, O(rooms)).

---

## 16. Power banks and deposits

### PB-GEN
cronjobs.js:194-243, for every room with `bus: true, status: 'normal'` (sequential):
`respawnTime = round(rand·POWER_BANK_RESPAWN_TIME/2 + POWER_BANK_RESPAWN_TIME·0.75)` ∈
[37500, 62500] (drawn every visit). No `powerBankTime` → set `T + respawnTime` (whole room doc
`$set`) and continue. `T < powerBankTime` → continue. Else: `powerBankTime = T + respawnTime`;
loop (unbounded): `x,y = floor(rand·40+5)` ∈ [5,44], accept if terrain wall and any non-wall in the
3×3 around (centre is a wall, so one of 8 neighbours); `power = floor(rand·(MAX−MIN)+MIN)` ∈
[500, 4999], with probability `POWER_BANK_CAPACITY_CRIT (0.3)` add `POWER_BANK_CAPACITY_MAX
(5000)`; insert `{type:'powerBank', x,y, room, store:{power}, hits=hitsMax=POWER_BANK_HITS (2M),
decayTime: T + POWER_BANK_DECAY (5000)}`; save `powerBankTime`; activate.
**GOTCHA**: no check for an existing bank or any object on the chosen tile; the position loop never
terminates if no qualifying wall exists (hangs the cron). Rooms generated by `map.generateRoom`
have no `bus` flag → never get banks.

### PB-DECAY
processor.js:421-426: after object ticks, `T >= decayTime − 1` → remove (also deposits). Runtime
`ticksToDecay = decayTime − time` (structures.js power bank getters).

### PB-DESTROY
_destroy.js:21-49: ruin `{structure:{id,type,hits:0,hitsMax,user}, store: bank.store,
decayTime: T + RUIN_DECAY_STRUCTURES.powerBank (10)}` (unless nuked → nothing).

### DEP-TYPE
storage upgrade v4 (reference/storage/lib/db.js:55-63): for rooms matching `^[WE]\d*0` or `0$`:
`depositType = [silicon, metal, biomass, mist][Number(lonStr + latStr) % 4]` (string
concatenation, e.g. W10N10 → "1010" % 4 = 2 → biomass). Inner single-digit rooms get `bus:false`.

### DEP-GEN
cronjobs.js:556-643. Central rooms = rooms matching the **string** `'^[WE]\d*5[NS]\d*5$'`, not
'out of borders'.
**GOTCHA**: inside a JS string `'\d'` is just `d`, so the regex is `^[WE]d*5[NS]d*5$` and only
matches W5N5, W5S5, E5N5, E5S5 — deposits spawn only in the four sectors around the origin (the
default 11×11 world is exactly sector W5N5, so it works there). For huge worlds this means "no
deposits elsewhere" in 1:1 mode (see Open questions).
Per central room (sequential):
1. `sectorEdges` = the 44 unique highway rooms on the sector border (`10·xx+i` for i ∈ 0..10 along
   the four sides; `xx`/`yy` are the leading digits, '' → 0).
2. Sector centre in world tiles `cxx = 24 + 50·cx`, `cyy = 24 + 50·cy`; sector deposits = deposits in
   edge rooms with `|50·rx + d.x − cxx| < 250` and `|50·ry + d.y − cyy| < 250`.
3. `throughput = Σ 20 / max(1, DEPOSIT_EXHAUST_MULTIPLY·harvested^DEPOSIT_EXHAUST_POW)`; `>= 2.5`
   → next sector.
4. Candidate rooms: edge rooms with `bus:true, status:'normal', depositType` set; none → the
   function rejects (and `sectorRegex` is a ReferenceError) → **all remaining sectors skipped**.
5. Free rooms = candidates without a sector deposit; none → `return` (**remaining sectors
   skipped**).
6. `room = _.sample(free)`; no terrain → `return` (skips the rest).
7. Up to 1000 tries: `x,y ∈ [5,44]`, wall with a non-wall neighbour, no object of any type within
   Chebyshev 2. `inSector` is computed but **not** required. Failure → log, next sector.
8. Insert `{type:'deposit', depositType, x,y, room, harvested:0, decayTime: T +
   DEPOSIT_DECAY_TIME (50000)}`, activate. At most one deposit per sector per run.
**GOTCHA**: a deposit placed outside the 250-tile window is not counted next time → over-spawn.

### DEP-DECAY
deposits/tick.js:13-16 removes at `T > decayTime`, but PB-DECAY's `T >= decayTime−1` in
processor.js runs for deposits too and wins. Harvest refreshes `decayTime` (room spec).

---

## 17. Invaders (NPC raids)

### INV-HARVEST
harvest.js:45-48: every successful source harvest (any harvester) adds the harvested amount to the
source's `invaderHarvested` (in-memory merge, so multiple harvesters accumulate). Minerals and
deposits do not count.

### INV-GEN
cronjobs.js:377-439, sequential over the rooms of one query `{status:'normal', _id: {$nin:
activeRooms}}` (377-378), i.e. not in the env active set **at the wall-clock moment of that read**
(lifecycle and storage edge cases: CRON-ACTIVE-VIEW):
1. Load sources, controller, creeps. Any creep of user '2' → skip.
2. `goal = room.invaderGoal || INVADERS_ENERGY_GOAL (100000)`; `goal != 1 && Σ invaderHarvested <
   goal` → skip (`goal == 1` forces a raid).
3. Sector regex `^<Xprefix>\d<Yprefix>\d$` from the room name (all-but-last digit); require ≥ 1
   `invaderCore` with `level > 0` in that "sector" → else skip.
   **GOTCHA**: random invaders only appear in sectors with a deployed/undeployed stronghold core.
4. Exit sides from terrain: indices 0..48 of row 0, row 49, column 0, column 49 that are not
   walls; a side is kept unless the neighbouring room's controller is owned or reserved.
   No sides → skip.
5. `exit = random side`; `createRaid(controllerLevel = owned ? controller.level : falsy, room,
   exitSquares[side])`; then INV-GOAL; activate.
**GOTCHA**: owned rooms and rooms with player creeps are missing from the set from the rooms-phase
DEL until the end of their own processing, so on the private server they **are** raided, whenever
a 5-min run falls into that window (probability = that window's share of the tick's wall time).
Idle harvested rooms are always eligible. On stock LokiJS a run that falls between the DEL and the
first re-add throws and creates no raids at all (CRON-ACTIVE-VIEW).
**Chitin**: apply the CRON-ACTIVE-VIEW rule. At the post-global barrier no room is pending, so the
active-set filter excludes nothing and eligibility is `status == 'normal'` plus steps 1-4. Never
filter against the rooms scheduled for T+1: that would stop all raids on owned and remote rooms.

### INV-RAID
cronjobs.js:320-375 (Math.random call order matters only for seeded reproducibility):
```
type = (level >= 4) ? 'big' : 'small';  max=1; count=1; boost=0.5
center = isCenter(x,y)                       // 3x3 SK/centre block of a sector, backend utils.js:301-304
if rand() > 0.9 || center:
   max=2; boost = (type=='big') ? 0 : 0.5
   if rand() > 0.8 || center:
      max=5
      if big: level<5→2; <6→2; <7→3; <8→(boost .4, 3); else (boost .4, 5)
   count = min(floor(rand()·(max−1)) + 2, exitTiles.length)
for i in 0..count-1:
   sub = (i==0 && !center) ? Melee
       : (i==0 || ((i==1 || (i==2 && count==5)) && rand() > 0.5)) ? Ranged : Healer
   tile = (i==0) ? exitTiles[floor(rand()·n)] : first of exitTiles sorted (stable) by Chebyshev to previous tile
   createCreep(type+sub, room, tile, rand() < boost); remove tile from exitTiles
```
Invaders spawn on the room's own exit tiles.

### INV-CREEP
cronjobs.js:262-312: bodies (bigHealer 25 MOVE+25 HEAL…, see source), every part `{type, hits:100}`,
`hits = hitsMax = 100·parts`, `ticksToLive: 1500` (ignored; `ageTime` is set on first room tick
to `T + CREEP_LIFE_TIME`, creeps/tick.js:88-91), `fatigue 0`, `store {}`, `storeCapacity 0`,
`name = invader_<room>_<floor(rand·1000)>` (may collide), `user '2'`. Boosts if boosted: centre
rooms T3 (XLHO2/XKHO2/XZH2O/XUH2O/XGHO2), else T1 (LO/KO/ZH/UH/GO) per part type.

### INV-GOAL
cronjobs.js:433-439: `goal = floor(INVADERS_ENERGY_GOAL·(rand·0.6 + 0.7))`; with probability 0.1:
`goal *= floor(rand > 0.5 ? 2 : 0.5)` → ×2 or **×0**; 0 falls back to the default 100000 via `||`.
Save `invaderGoal`; reset every source's `invaderHarvested` to 0. place-spawn / bots.spawn set
`invaderGoal = 1000000` (USER-PLACE-SPAWN).

### INV-SUMMON-API
game.js:631-704: owner/reserver may summon up to 5 invaders if no foreign creeps; T1 boosts (no
tough boost); `userSummoned` field; such invaders die when a creep of a user other than '2' and the
controller owner is in the room (creeps/tick.js:114-116). `remove-invader` deletes own summoned.

---

## 18. Strongholds (invader cores)

### SH-GEN
backend/lib/strongholds.js:225-236: central rooms `^[WE]\d*5[NS]\d*5$` (correctly escaped here),
not out of borders. A sector is occupied if any `invaderCore` or ruin of an invaderCore maps to it
(room name with last digit of each coordinate replaced by 5). For each unoccupied sector
(sequential, awaited) → SH-SELECT. A rejection (e.g. "No spots") aborts the remaining sectors.
Ruins keep a sector occupied until they decay (`max(T+500, collapse endTime)`, _destroy.js:42-47).

### SH-SELECT
strongholds.js:130-161: sector rooms by regex; eligible: not `bus`, `isCenter`, not
`isVeryCenter`, name not matching `0$` / `0[NS]`; and either no controller, or a controller with
no owner, no reservation and no creep with a CLAIM part in the room. Pick `_.sample`; spawnStronghold
with `deployTime: 5000`. In practice: the 8 SK rooms of a sector.

### SH-SPAWN
strongholds.js:14-128: `user` default '2'; obstacles = ramparts ∪ OBSTACLE_OBJECT_TYPES in the room;
controller owned / reserved by someone else → reject. If templateName+x+y given → returns `opts`
without spawning (CLI quirk). Else for x,y ∈ [4,44] and each template, the spot is valid if every
template tile is non-wall and has no obstacle object (keeper lairs, roads, containers allowed).
Template = `_.sample` over templates with ≥ 1 spot (uniform, not weighted), spot = `_.sample`,
`depositType = _.sample(keys(coreRewards))`, `deployTime = T + opts.deployTime (default 1)`,
`strongholdId = "<room>_<T>"`. Inserts: the core `{level, strongholdBehavior, templateName,
hits=hitsMax=INVADER_CORE_HITS, nextExpandTime: deployTime + INVADER_CORE_EXPAND_TIME[level],
depositType, deployTime, strongholdId, effects:[INVULNERABILITY until deployTime, duration 5000]}`
and each template rampart `{hits:1, hitsMax:1, isPublic:true, strongholdId, nextDecayTime:
deployTime}`; an existing controller is set to `{user, level 8, progress 0, downgradeTime:
deployTime, effects:[INVULNERABILITY]}`. Activate. Templates/rewards:
reference/common/lib/strongholds.js:3-272 (bunker1..5, coreRewards, coreAmounts, coreDensities,
containerRewards, containerAmounts).
**GOTCHA**: `q.reject('invalid template')` is not returned (execution continues).

### SH-DEPLOY
stronghold.js:22-128 (core pretick behaviour `deploy` while `deployTime` set, invader-core/pretick.js:18): when `deployTime <= T+1`: `duration = round(STRONGHOLD_DECAY_TICKS·(0.9 + rand·0.2))`,
`decayTime = T + duration`; push COLLAPSE_TIMER effect; core `{deployTime:null, decayTime, hits,
effects}`; remove the placeholder ramparts; for each template tile: kill non-stronghold creeps/power
creeps there, remove construction sites (drop `floor(progress/2)` energy if progress > 1), destroy
structures with CONSTRUCTION_COST; insert every non-core template structure with
`strongholdId, decayTime, effects:[COLLAPSE_TIMER]` + per-type options: rampart `hits =
STRONGHOLD_RAMPART_HITS[rewardLevel]`, `hitsMax = RAMPART_HITS_MAX[8]`, `hitsTarget =
STRONGHOLD_RAMPART_HITS[core.level]`; tower full energy; container `store = calcReward(
containerRewards, containerAmounts[rewardLevel], 3)`; road; towers/ramparts get the core's user.

### SH-COLLAPSE
invader-core/tick.js:11-27: when the collapse effect `endTime <= T`, reset the room controller
(user null, level 0, …, effects null). processor.js:412-419 removes **any** object whose
COLLAPSE_TIMER ended (after its tick).

### SH-EXPAND
strongholds.js:238-349 (every 900 s wall). Candidates: cores with `deployTime: null`,
`level > 0`, `nextExpandTime < T`. Map grid = accessible rooms with exit counts per side
(buildMapGrid 163-223). Per core: abort if its room is novice/respawn; set `nextExpandTime = T +
INVADER_CORE_EXPAND_TIME[level]` (even if expansion fails); BFS from the core room, exits
shuffled per room; skip neighbours not in the grid, names matching `/(W|E)\d*0/` or `/(N|S)\d*0/`,
and novice/respawn rooms; the first neighbour with a controller that has no owner and no core is the
target (reservations are ignored); rooms without controller or with a core are enqueued. Position:
`findFreePos(room, 1, rect)` with `rect = [max(1,cx−5), min(48,cx+5))` per axis (upper bound
exclusive ⇒ asymmetric), up to 500 tries, 3×3 free of walls and obstacle/rampart/portal objects.
Insert `{type:'invaderCore', level:0, user:'2', hits=hitsMax=INVADER_CORE_HITS, strongholdId,
effects: parent.effects}`; activate.
**GOTCHA**: the unanchored highway regexes match any room whose X or Y number **contains** a `0`
digit (`W101N5`, `W3N105`, `W105N3` are all treated as highways) — equivalent to "x%10==0" only for
coordinates < 100.
**GOTCHA**: child cores copy the parent's effects, so they collapse at the parent's collapse time
(even if the parent dies earlier); they have no `templateName`/`depositType` → no reward.

### SH-DESTROY
invader-core/destroy.js:7-38 (via structures/_destroy.js:17-19): reset controller; if the core has
a valid `depositType` and `templateName`: `rewards = coreRewards[depositType].slice(0,
1+rewardLevel)`, densities = `coreDensities.slice(0, 1+rewardLevel)`, `store = calcReward(
zip(rewards, densities), coreAmounts[rewardLevel])` written to the core so the ruin inherits it
(not when nuked: no ruin); emit `strongholdDestroyed` (mod hook).

### SH-REWARD
utils.calcReward (reference/engine/src/utils.js:677-698): `order = shuffle(range(n))`, truncated to
`itemsLimit`; for k < last: `amount_k = max(0, round(rand·(target − acc)/density[order[k]]))`,
`acc += amount_k·density[order[k]]`; last: `max(0, round((target − acc)/density[order.length−1]))`.
**GOTCHA**: the last item divides by `density[order.length−1]` (index bug) instead of
`density[order[last]]`. Replicate.

### SH-AI
Room-local NPC behaviour per tick (cross-ref to the NPC/room spec): behaviours `deploy`, `default`
(level-0 cores), `bunker1..5` (stronghold.js:388-491) composed of handleController (upgrade own
controller when `downgradeTime − T < INVADER_CORE_CONTROLLER_DOWNGRADE − 25`, else reserve, else
attack), refillTowers (chance `[0,.01,.1,.3,1,1][level]`), refillCreeps, population maintenance
(bunker4/5 shuffle decks), towers maintenance, focusClosest/focusMax, antinuke (every 10 ticks),
defender spot assignment. Uses FAKERT-*.

---

## 19. NPC pathing helper (processor/common/fake-runtime.js)

Used by keepers, invaders and stronghold creeps (room-local, R). The native PathFinder is bound to
this RoomPosition via `driver.pathFinder.make({RoomPosition})` each room (processor.js:174).

### FAKERT-POS
fake-runtime.js:8-77: `RoomPosition(x,y,room)` (`+x`, `+y`, throws on NaN / non-string room);
`getRangeTo` Chebyshev or Infinity across rooms; `getDirectionTo` via world coordinates
(`roomX·50 + x`); `lookFor(LOOK_TERRAIN)` returns `[name]` from the module-global `terrains` map, or
the bare string `'wall'` for unknown rooms; `packLocal(x,y) = String.fromCharCode(32 + (x<<6 | y))`,
`sUnpackLocal` inverse (throws if code < 32).

### FAKERT-MATRIX
79-141: `CostMatrix` is `Uint8Array(2500)` indexed `x·50 + y`, `set` clamps to [0,255] (Infinity →
255). Default matrix (only for the creep's own room; other rooms → `false`): obstacles =
OBSTACLE_OBJECT_TYPES + portal (minus destructible structures if `ignoreDestructibleStructures`,
minus creep if `ignoreCreeps`), non-public foreign ramparts, own obstacle construction sites → 255;
roads → 1 unless `ignoreRoads` (and not already 255). The `swamp` branch is dead code (terrain is
not an object).

### FAKERT-FINDPATH
143-182: sets `terrains = {[source.room]: roomTerrain}`; room callback = default matrix, then
`costCallback(roomName, clone)` may replace it if it returns a CostMatrix; search opts = opts +
`maxRooms: 1` + (unless `ignoreRoads`) `plainCost 2, swampCost 10`. If the target is a
RoomPosition, `!opts.range`, and the path ends at range 1 (or empty path from range 1) → append the
target.

### FAKERT-MOVETO
200-245: defaults `reusePath 5`, `range 0`; in range → 0. Recompute when no `memory_move`, dest
packed differs, or `T > time + reusePath`: store `memory_move {dest, path: packed, time: T}` on the
creep object (persisted). Next direction via FAKERT-NEXTDIR; non-zero → `memory_move.lastMove = T`.

### FAKERT-WALKTO
247-263: moveTo; if an own creep (same user) is on the next tile and has no `memory_move` or
`lastMove + 1 < T`, give it a swap move toward the walker and reset its `memory_move`
(`lastMove = T`); then `move` intent for the walker.

### FAKERT-FLEE
185-198: goals = hostiles with `range`, `flee: true`; first path step → direction (0 if none).

### FAKERT-CLOSEST
265-309: object on the same tile wins; else one multi-goal search (each goal range 1); result =
the **last** object in input order within Chebyshev 1 of the path end (or of `fromPos` for an empty
path). **GOTCHA**: last match, not closest.

### FAKERT-NEXTDIR
311-332: index of current packed position in the path; at the end → 0; not found → step to
`path[0]` only if adjacent; else next element.

---

## 20. Moddability hooks touching this area

- `config.engine` events: `mainLoopStage(stage)` incl. `'global'` (main.js:63-64),
  `processObject`, `preProcessObjectIntents`, `processObjectIntents`, `postProcessObject`,
  `processRoom` (processor.js:169,246,318,482,489), `playerSandbox` (driver/lib/index.js:35),
  `strongholdDestroyed` (invader-core/destroy.js:37); `mainLoopCustomStage()` (main.js:92);
  `customIntentTypes` (driver/lib/index.js:28; system.js:105-111).
- Backend: `config.cronjobs` map (cronjobs.js:14), `config.backend 'sendUserNotifications'`.
- There is **no** hook inside the global processor itself (power/market are fixed modules).
- xxscreeps design for reference: cross-room intents via `context.sendRoomIntent` processed in a
  finalize phase (reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:57-84,184-202),
  terminal send as a cross-room intent with an ingress counter
  (reference/xxscreeps/packages/xxscreeps/mods/market/processor.ts:14-50, **not** 1:1: different
  capacity arbitration), and tick-based world jobs `registerShardTickProcessor` / `everyNTicks`
  (reference/xxscreeps/packages/xxscreeps/engine/processor/shard.ts:5-28). Chitin: expose
  equivalent "global stage" and "world job" registration points.

---

## 21. PERF summary — serial vs partitionable

| Work | Original | Chitin recommendation |
|---|---|---|
| XROOM-APPLY | single process, full scan of interRoom objects | per-destination mailbox, parallel; order-free |
| NUKE-LAUNCH-RP insert | room bulk writes another room | cross-room insert mailbox |
| GCL/GPL/money `$inc` from rooms | per-room bulk on shared users | per-thread delta maps merged at barrier (commutative) |
| USER-ROOMS add/pull | per-room `$addToSet/$pull` | set deltas per user merged at barrier |
| Power-creep intents & deaths | serial over users.intents | per-user parallel (power spawns are single-owner; tile check reads pre-stage snapshot) — keep deterministic user order for name dedup only within a user |
| Terminal sends + terminal deals | serial | keep serial in storage order (state chains through terminals); tiny |
| Direct (inter-shard) deals | serial, shuffled | serial after terminal deals; seeded shuffle |
| Order maintenance | O(all orders) per tick | dirty-tracked recompute; identical outputs |
| Market fees/create/extend/change/cancel | serial over users.intents | per-user parallel except `_skip`/deal interplay → do intents per user in deterministic order, then deals |
| Runtime market cache | each runner process reloads all active orders per tick | one immutable per-tick snapshot shared by all sandboxes |
| Crons (banks, deposits, invaders, strongholds) | sequential awaited DB queries, wall-clock | per-sector jobs on worker threads at a fixed tick point |
| ACTIVE-SET | env set, SMEMBERS/DEL per tick | per-tick bitset; essential for 1000-player scale |
| Notifications | DB upserts per message | per-user append queue, delivered off-tick |

Global serialization points per tick: (1) the deterministic order of users' global intents,
(2) the terminal/deal chain, (3) the barrier merging per-room user deltas. Everything else is
room-, user- or sector-local.

---

## 22. RNG-INVENTORY (all Math.random uses in this area)

| Where | Use |
|---|---|
| market.js:400 | `_.shuffle(directDeals)` |
| cronjobs.js:71-72,88,102,119 | NPC order minerals, 50 % energy sell, 25 % buy, next time ±20 % |
| cronjobs.js:187 | roomsForceUpdate jitter `floor(rand·20)` |
| cronjobs.js:199,214-215,227-228 | power bank respawn time, position, power amount, crit |
| cronjobs.js:288,328,331,354,362,365,369,429,433-435 | invader names, raid size, subtypes, tiles, boosts, exit side, next goal |
| cronjobs.js:605,617-618 | deposit room sample, position |
| backend/lib/strongholds.js:61,63,65,157,261 | template, spot, depositType, room sample, BFS exit shuffle |
| backend/lib/utils.js:334-335 | findFreePos |
| stronghold.js:27,153,451,468,477 | collapse duration, tower refill chance, population decks |
| engine utils.js:685,692 | calcReward shuffle and amounts |
| game.js:671 / bots.js:97-101 | summoned invader name; bot spawn position |

No RNG in the global processor except the direct-deal shuffle. Exact RNG streams cannot be matched
with V8; chitin must match distributions and may use a seeded PRNG for reproducibility (keep the
same draw order to make seeded runs explainable).

---

## 23. Open questions

1. **users.intents order** (ORDER-UINTENTS) is nondeterministic in the original. Proposal: process
   in `getAllUsers` order (lastUsedDirtyTime desc, then `_id`) or plain `_id` order; HTTP-inserted
   docs after runner docs. Needs a project decision.
2. **Inter-shard resources on LokiJS** (BULK-DOTKEY): replicate the private-server flat-key bug
   (resources never move) or implement MongoDB/official nested semantics? Recommend official.
3. **Shard identity**: `Game.shard.name` = hostname and `PowerCreep.shard` = '' on the private
   server; official bots expect a real shard name in both. Recommend a configurable name used in
   both places (deviation from private-server 1:1, matches official).
4. **Pixels**: implement official `Game.cpu.generatePixel` (bucket −10000, +1 pixel in
   `user.resources`, timing of bucket deduction unknown from source) or keep it absent? Official
   semantics are not in the reference source.
5. **Cron cadence**: original crons are wall-clock (5/15/10/60 min). At high TPS chitin needs a
   tick-based default; which ratio (e.g. official ~3 s ticks ⇒ 300 s ≈ 100 ticks)?
6. **Deposit regex bug** (DEP-GEN): faithful 1:1 means deposits only in the four sectors adjacent
   to the origin; for "worlds far larger than official" this is likely undesirable. Flag as
   config option?
7. **Highway regex in SH-EXPAND** for coordinates ≥ 100: replicate (blocks expansion into many
   rooms in huge worlds) or fix?
8. **worldSize off-by-one / wrap** (TERM-WORLDSIZE): terminal costs on a non-wrapping world still
   use torus distance. Keep 1:1?
9. Whether the official server shares the private-server quirks listed as GOTCHA in the market
   (unrounded expiry refund, `_skip` set before the fee check, unenforced buyer money in direct
   deals) cannot be determined from the reference source.
10. Power-creep duplicate tombstones / duplicate names: faithful replication is cheap; confirm the
    project wants bug-for-bug here.
11. CRON-ACTIVE-VIEW: the claim that LokiJS `find` throws on `_id: {}` follows from its operator
    lookup, but the lokijs source is not in reference; confirm against lokijs 1.5.x. It does not
    affect chitin, whose rule never has a missing set.
