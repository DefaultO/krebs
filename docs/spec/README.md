# Specs of the original Screeps server

Written by reading the reference sources (`reference/`, git-ignored). Each spec starts with an inventory of items with stable IDs used to track parity. Every rule cites `reference/<pkg>/<file>:<line>`.

## [01-tick-loop.md](01-tick-loop.md) — 152 items

The spec covers one full tick of the original private server, with 152 inventory items, each cited to source.

- **Main loop:** users run, then rooms are processed, then the global stage runs, then time is incremented. The loop also covers pause, tick rate, the 5 s queue watchdog, the 21-tick refresh of accessible rooms, and error behaviour.
- **Runner phase and CPU bucket:** pre-run skips, user deactivation, the timeout model, and the exact bucket formula, including the 0.2 CPU per-intent cost that sits outside the timeout.
- **Intent flow:** the runtime list format, sanitization sentinels, routing by `object.room`, the `rooms.intents` `$merge`, and per-type caps. There is no global MAX_INTENTS.
- **Active rooms:** every activation source, and the processor's self-activation rule.
- **Room processor:** step order, the bulk-writer semantics (deep merge into live objects, hidden `_` fields, undefined values and arrays), and what a later step sees from an earlier one.
- **Cross-room effects and global stage:** when each cross-room effect applies, and the global stage order (inter-room moves, then power, then market).
- **Other:** post-tick outputs, wall-clock crons, mod hooks, `Math.random` and `Date.now()` sites.
- **Performance:** what runs in parallel and what is serialized. As measured, about 85–100 ms per tick of which game logic is under 1 ms per room and about 4 ms of bot CPU; about 90% is IPC, database, copying and timers.

Top 1:1 pitfalls:
- Inactive rooms are not processed at all, and the stale state is visible to players (negative ticksToRegeneration, stale event log, missed exact-tick triggers such as a nuke landing at landTime-1). Chitin must reproduce the active-room set exactly and must not process every room every tick.
- The active-room set is read and cleared when the room phase starts. Rooms activated during room processing or the global stage are processed next tick; rooms activated by a runner's intents are processed in the same tick.
- Within a room, users '2' (Invader) and '3' (Source Keeper) are always processed first because JS orders integer-like keys first. Other users follow the order their intents were saved, which is non-deterministic, and matters because structure damage and destruction apply immediately.
- Objects within a user are processed in the order of the first intents.set call per object; the 'room' pseudo-object sits wherever the first room intent was issued.
- lodash _.forEach takes a snapshot of the object's keys. Temporary ids of objects inserted this tick ('1', '2', …) are iterated first, so a pile dropped this tick decays ceil(amount/1000) in the same tick. Objects created during the tick loop are not visited by it.
- bulk.update deep-merges into the live object, so later steps see the change at once. Each top-level key is persisted with its merged value as of the last update call.
- Writing undefined through bulk.update does nothing; only null clears a field. Arrays merge index by index, so a shorter new array keeps the old tail.
- Keys starting with '_' (and $loki) are never persisted; they are per-tick scratch fields.

## [02-runtime-sandbox.md](02-runtime-sandbox.md) — 152 items

The spec covers the player runtime from tick start to output handoff, with 152 inventoried items. Each item is tagged by how strong the 1:1 evidence is: engine code the official servers share, private-server driver code, or docs-only (official-only APIs).

Areas covered:
- VM lifecycle and every global-reset trigger.
- The per-tick sequence, the runtime data fields and the visibility rules (observer targets visible next tick; tombstones also grant visibility on the private server).
- The complete list of globals with their property descriptors, and what is rebuilt each tick versus what persists in the heap.
- Module system: flat require namespace, cache quirks, wrapper text with a +36 column offset on line 1, binary modules returned as a Buffer, and legacy mode when main exports no loop.
- Game object construction order and lazy getters keyed by object id.
- CPU rules: getUsed, billing, bucket update, timeouts, halt.
- Memory, RawMemory, segments and foreign segments, plus a failure matrix of what is saved under each error.
- Console formatting, error stack processing, visuals, intents, mod hooks and JS engine requirements.

Most important findings:
- Errors and CPU timeouts in the loop still save Memory and intents. A Memory serialization failure or a segment-validation failure discards the whole tick, unbilled.
- Cached game objects keep working across ticks: unread getters return current data, while pos and memoized values are stale.
- console has only log and logUnsafe; console.error does not exist.
- InterShardMemory and Game.cpu.generatePixel, unlock and shardLimits are official-only and absent from the reference code.

Top 1:1 pitfalls:
- Class constructors and prototypes are created once per VM: prototype patches persist across ticks, and the engine's own methods call patchable public paths (e.g. destroy() calls this.room.find). The game API must keep the original JS call graph.
- Game objects are new each tick, but cached old instances still work: unread getters read the current tick's data by id, memoized _x values and pos/room are stale, methods queue intents into the current tick, and a vanished object throws 'Could not find an object with ID'.
- Lazy getters memoize into own _name properties only for truthy values. The descriptors are enumerable, non-configurable and have no setter. Find-cache filters pre-populate _my, _spawning and _energy during construction.
- Memory: the getter does JSON.parse on first access and the root object has a null prototype. After access it becomes a non-writable but configurable data property, so `Memory = x` fails and the memhack (delete global.Memory; RawMemory._parsed = x) must work.
- Invalid Memory JSON makes Memory null every tick and the stored string is never re-serialized. RawMemory.set after Memory was accessed silently discards writes made to the old Memory object.
- End-of-tick JSON.stringify of Memory and segment validation run outside the try/catch. A cycle, a BigInt, an 11th segment key, or a non-string/oversize segment loses all of that tick's intents, Memory and segments, and the tick is not billed.
- Every key present in RawMemory.segments is re-saved each tick, including preloaded ones. With 10 active segments, writing an 11th id throws.
- Uncaught errors and CPU timeouts in loop still save Memory and the intents queued before the throw, and the tick is billed. Console commands queued for that tick are lost.

## [03-api-room-position.md](03-api-room-position.md) — 182 items

The spec covers the player-facing API for Room, Room.Terrain, RoomPosition, RoomObject, Flag, ConstructionSite, Source/Mineral/Deposit/Nuke/Resource/Tombstone/Ruin, Store, the PathFinder/CostMatrix JS wrapper (with the driver's option handling), Game.map, RoomVisual/MapVisual, defineGameObjectProperties, and the vanilla mod hook (custom object prototypes and intents). It opens with a 182-item inventory with stable IDs, including one row per FIND_* and LOOK_* constant. Every rule cites file:line.

Main findings:
- Every find and look result follows one order: other players' and neutral objects first, then the player's own, each in database order.
- Getters cache a value only when it is truthy, and the cache shows up as visible `_name` keys.
- The find cache is a snapshot taken when the tick starts; FIND_EXIT stores whichever filter was used first and keeps it for the rest of the tick.
- lookAt returns every dropped resource twice.
- lookAtArea switches between two strategies depending on how many objects the room has; this changes result order and output shape, and decides whether out-of-bounds coordinates throw.
- findPath appends the target tile, changes the caller's opts object, and ignores heuristicWeight.
- In findClosestByPath the last tied object wins; in findClosestByRange the first wins.
- createConstructionSite cannot take a spawn name in the pos form.
- RoomPosition packing limits the world to about W127–E127 and maps W127N127 to 'sim'.
- findRoute only searches within 30 rooms of the start and turns a callback cost of 0 into 1.

Top 1:1 pitfalls:
- find/look/lookAtArea order = runtimeData.roomObjects key order: non-owned (neutral+other players) objects in DB order, then the player's own objects in DB order; findClosestByRange first-min wins, findClosestByPath LAST adjacent wins
- defineGameObjectProperties caches only truthy values in visible own `_<name>` props; falsy values re-read raw data every access; no setter (strict-mode assignment throws)
- Room.find cache is a tick-start snapshot; FIND_EXIT caches the concatenation filtered by the first caller's opts.filter, order TOP,BOTTOM,RIGHT,LEFT; unknown types return []
- lookAt/lookAtArea list every dropped resource twice (LOOK_ENERGY and LOOK_RESOURCES share one register); lookAt ignores the target's roomName; spawning creeps appear in look but not FIND_CREEPS
- lookAtArea/lookForAtArea pick per-type by-objects vs spatial mode (objects-of-type < area): changes asArray order (type-major, then object or row-major), presence of `type` key and empty-tile [] entries, and whether out-of-bounds coords throw 'look coords are out of bounds'
- lookForAt with invalid type returns ERR_INVALID_ARGS (number); terrain lookups have no bounds check and alias (x=50 -> next row)
- findPath (PathFinder mode) searches with range>=1 then appends toPos even if it is an obstacle; mutates the caller's opts (avoid/ignore undefined, maxOps=20000 cross-room); ignores heuristicWeight/flee/maxCost; plainCost/swampCost default 2/10 unless ignoreRoads
- findClosestByPath ignores plainCost/swampCost/range/maxRooms; uses range-1 goals in a single room

## [04-api-creeps.md](04-api-creeps.md) — 157 items

The spec covers the player-side Creep and PowerCreep API, plus the RoomObject and body helpers they use. For every property and method it gives the exact check order, the return codes, the intent object written (name and fields), and the server-side field cleanup each intent goes through. It also covers how the intent queue works: only the last call per method name per tick is kept, each new intent costs 0.2 CPU except say and pull, cancelOrder gives CPU back, and 'global' intents are capped at 50. It covers the simultaneous-action priority table, how Game.creeps and Game.powerCreeps are built (including the automatic suicide of a creep with a duplicate name), the property cache that stores values on the instance under '_' names, and the POWER_INFO table.

moveTo is documented branch by branch:
- argument cloning and the defaults it writes into the caller's opts object
- the memory._move format and key order, path reuse and expiry
- noPathFinding and visualizePathStyle (a tired creep that is visualizing recomputes its path every tick)
- the findPathTo → _findPath2 → PathFinder call chain and which options it honours or ignores
- the per-tick cached cost matrix and costCallback, appending the target tile, cutting the path at the room edge
- the old pathfinder used after PathFinder.use(false)

Key findings:
- The original never updates game state locally after an API call.
- Many return codes and edge cases differ from the docs (for example the safe-mode ERR_NO_BODYPART, NaN ticksToLive for unspawned power creeps, boost:null on body parts).
- Several calls throw instead of returning a code.

Top 1:1 pitfalls:
- Nothing is simulated client-side: store/pos/hits are unchanged after an OK call; a second same-name call on the same object replaces the first (intents.set last-wins).
- Sanitizer runs parseInt/String on fields after the client checks have passed: transfer amount 0.5 passes the checks but arrives as 0; move(1.5)/move('3')/move(true) are accepted; usePower without a target sends id 'undefined'; signController(c) with no text sets the sign to 'undefined'.
- In a room where someone else's safe mode is active, attack/rangedAttack/rangedMassAttack/heal/rangedHeal/claim/attackController/dismantle return ERR_NO_BODYPART (undocumented); withdraw returns ERR_NOT_OWNER; usePower returns ERR_INVALID_ARGS. Where this check sits in the order differs per method.
- Check order differs between similar methods: attack checks fortify/invulnerability before range, rangedAttack after; heal checks safe mode after range, rangedHeal before; claimController checks GCL and novice before validating the target.
- moveTo with range>=2 returns ERR_NO_PATH on arrival. An empty path result is cached in memory._move for reusePath ticks. With range 0 the target tile is always appended, so a creep next to a source sends a move into the source every tick and gets OK.
- moveTo(60,10) throws (RoomPosition constructor) instead of returning a code. A corrupt memory._move.path string throws from deserializePath. opts.serialize:true breaks moveTo.
- findPath/moveTo use plainCost 2 / swampCost 10 unless ignoreRoads (docs say 1/5). heuristicWeight, maxCost and flee are silently ignored. maxOps defaults to 20000 for cross-room targets.
- moveTo writes defaults into the caller's opts in the (x,y,opts) form, and always fills defaults into the caller's visualizePathStyle object.

## [05-api-structures-market.md](05-api-structures-market.md) — 148 items

The spec covers the player-facing API of all 21 Structure classes plus StructureSpawn.Spawning, the Store class and Game.market. It also covers the server-side processing of every intent these emit, in both the room processor and the global market/terminal stage, since 1:1 behaviour only shows up in the processor result. It opens with a 148-item inventory, each with a stable ID, a locality tag (room-local, user-wide or global) and a source citation.

Each item gives the exact validation order, return codes, the intent emitted and the sanitizer transforms applied to it, the processor checks and effects, formulas with constants by name, and processing order.

Main findings:
- isActive picks the N closest structures by Chebyshev distance to the controller, ties broken by object iteration order. Spawns and extensions use a separate `off` flag with different rules: it counts enemy structures and lags RCL changes by one tick.
- Many API and processor checks disagree, so the API returns OK and the processor drops the intent silently. Examples: tower one action per tick (heal > repair > attack), link transferEnergy returning OK when the target is full, terminal sends clipped to the receiver's free space, deal and send on the same terminal in one tick.
- Market prices are stored in milli-credits. Order activation lags creation by a tick. getHistory and getAllOrders have shape quirks.
- Some effect timing (`endTime`) comparisons and Math.random uses are inconsistent between API and processor.

Top 1:1 pitfalls:
- isActive: the N structures closest to the controller by Chebyshev distance are active (N from CONTROLLER_STRUCTURES); ties go by room-object iteration order; only the owner's structures count; types capped at 1 at RCL 8 short-circuit to active.
- The spawn/extension `off` flag is a separate algorithm: it counts all owners' spawns/extensions and uses the controller level from the previous processing pass. It drives spawnCreep ERR_RCL_NOT_ENOUGH and Room.energyAvailable, so it can disagree with isActive().
- spawnCreep checks: name and directions errors come before ERR_NOT_OWNER/ERR_BUSY, body errors come after busy/RCL. Names that match Object.prototype members ('toString', '__proto__', …) return ERR_NAME_EXISTS.
- spawnCreep reserves no energy and does not mark the spawn busy within the tick: repeated calls all return OK and the last intent wins. Memory.creeps entries and stub creeps remain even when the processor later fails silently.
- energyStructures availability in the API ignores owner and room; the processor only uses the spawn owner's structures in the same room. opts.memory falsy values are replaced; createCreep stores null as given.
- Tower: only one action per tick (heal > repair > attack). Attack is redirected to a rampart on the target tile. No same-room check (returns OK, does nothing). Heal on a full-HP creep still spends 10 energy.
- Link transferEnergy with amount omitted and a full target returns OK. Target ownership is checked before source ownership. Cooldown is set and decremented in the same tick.
- Lab: input labs need not be yours. boostCreep ignores cooldown and target ownership; non-TOUGH parts are boosted right-to-left. unboost can leave a creep holding more than its capacity. reverseReaction's processor has no range/RCL/owner check, and its variant order follows REACTIONS key order.

## [06-processor-creeps.md](06-processor-creeps.md) — 131 items

Spec of server-side creep and power-creep processing, with 131 inventory IDs. It covers the processRoom phase order, all creep intents (with their fixed order and priority exclusions), the 9 power-creep intents including every usePower case, how damage and healing are queued and then applied (rampart redirect, melee and power-bank hit-back, TOUGH reduction), the full movement algorithm, the creep tick (spawning/birth, spawn-stomp, exits, portals, aging, fatigue recovery), death (tombstone, corpse reclaim formula, death variants), power-creep death in the global stage, and the Keeper and Invader AI (fake-runtime pathing and path cache, raid generation by backend cron). Stronghold defenders are only summarised.

Main findings:
- No randomness is used in creep processing; outcomes depend on processing order.
- Invader ("2") and keeper ("3") intents always run first because their user ids are integer-like keys.
- Player order follows runner finish order and is non-deterministic.
- Movement gives one winner per tile using four ranks; exact ties go to the first move registered (stable sort). A winner that later fails is not replaced by a loser.
- There are real defects in the original: a creep can die twice in one tick (two tombstones, store copied), pending inserts can be lost, and some crafted intents abort the whole room tick.

Top 1:1 pitfalls:
- Per-user intent order is Object.keys order: NPC users "2" then "3" are integer-like keys so they run first; players follow in runner $merge (finish) order, which is non-deterministic.
- Within a user, objects are processed in the order the script first issued an intent for each one; a creep's own intents run in the fixed creepActions order (drop, transfer, withdraw, pickup, heal, ... pull), each changing the store in place.
- Priority exclusion goes by whether the higher intent is present, not whether it succeeded (a failed heal still cancels harvest/attack); upgradeController/claim/transfer/etc. are never excluded.
- bulk.update merges into the live object right away, so later intents see store/hits/controller changes; creep damage, healing and positions only change in the creep's own tick.
- Movement ties are broken by stable sort in move-registration order (no Math.random); xxscreeps uses random tie-breaks and must not be copied.
- Conflict ranks are: how many movers want my current tile (100 for a swap partner), then pulled, then pulling, then moves/weight. Fatigue is not a rank, and a winner that later fails leaves the tile empty.
- Fatigue weight counts dead non-MOVE/CARRY parts and carry parts filled from the end of the body using the post-intent store; road beats swamp; entering an exit tile from inside costs 0 and resets fatigue.
- A pull train's move cost and spare MOVE recovery all go to the train head; a pulled creep must step exactly into its puller's current tile.

## [07-processor-structures-room.md](07-processor-structures-room.md) — 146 items

Spec for the room processor's non-creep side. It covers structure intents (spawn, tower, link, lab, factory, power spawn, terminal, nuker, observer, rampart, controller), room-level intents (flags, construction sites, destroyStructure, genEnergy) and every per-object tick: decay, regeneration, capacity-by-RCL, cooldowns, keeper lairs, portals, nukes, tombstones, ruins and dropped resources. It also covers construction-site completion (initial fields per structure type), structure destruction and ruins, the full invader core and stronghold lifecycle (AI behaviours, deploy, rewards, collapse, plus the backend crons that spawn and expand strongholds, power banks and deposits), the event-log entries with their data shapes, and a runtime-vs-processor validation cross-reference that lists return codes and where the two disagree. The inventory has 146 stable IDs. The most important finding is that exact behaviour depends on driver bulk-write semantics (in-memory merge, lost-insert bug), object iteration order, room activation (dormant rooms don't tick), and a set of reachable crash or lost-update bugs. Every rule cites reference/<pkg>/<path>:<line>.

Top 1:1 pitfalls:
- bulk.update(obj,…) mutates the in-memory object immediately; later handlers in the same tick see new values. bulk.update(idString,…) does not (e.g. spawn/extension `off` stays stale all tick).
- Timers fire at T >= X-1 (visible at X). Exceptions: portals use T > decayTime, and a nuke lands at landTime-1 but is only removed at landTime.
- Rooms are only processed when activated. Dormant rooms' decay/regen timers fire late and reschedule from the processing tick. Stronghold rooms go dormant after two quiet ticks.
- Tower attack retargets to any rampart on the target tile, of any owner. Heal > repair > attack: only one runs per tick even though the runtime returned OK for all.
- Link: target gains amount then loses ceil(amount*0.03). Cooldown += distance and is decremented the same tick, so players see distance-1. The event amount is pre-loss.
- Controller: _upgraded extends downgradeTime by min(+101, T+DOWNGRADE+1). A downgrade adds 90% of the new level's progress requirement. Invader-owned (user 2) controllers skip the whole tick.
- Nuke controller check has an operator-precedence bug, and `null < T` is true: unowned controllers with upgradeBlocked:null still get blocked.
- Mineral refill uses the OLD density. The reroll uses Math.random with cumulative probabilities, retrying until the density differs.

## [08-processor-global.md](08-processor-global.md) — 124 items

The spec covers everything the original server does outside room-local intents. That means the global stage (inter-room teleports, power-creep create/rename/delete/upgrade/spawn/suicide/death, terminal sends, the market) and per-user state (GCL, GPL, credits, the user.rooms list, CPU bucket, the active flag). It also covers notifications, room status (novice, respawn, closed), the active-room set, observer visibility, nuke launch/flight/landing, portals and inter-shard (mostly absent), pixels (absent), the NPC pathing helper fake-runtime.js, and every world-changing cron: power banks, deposits, invaders, strongholds (spawn, deploy, expand, rewards), NPC orders, power-creep deletion, notifications.

It has 124 inventory IDs with exact semantics, validation order, return codes, formulas and source citations.

Key findings:
- Global effects run after all rooms and before the tick counter goes up, so they are visible next tick.
- Storage writes also change the in-memory objects, so later steps in the same stage see earlier ones; much of the market and power-creep behaviour depends on this.
- The order of users' global intents is nondeterministic in the original.
- All crons run on wall-clock time, not ticks.
- Several real bugs must be replicated or consciously fixed: the deposit regex, the world-size off-by-one, the dotted-key $inc on LokiJS, power creeps never dropping into containers, duplicate tombstones and names, and the calcReward index bug.

Top 1:1 pitfalls:
- genDeposits uses the string '^[WE]\d*5[NS]\d*5$' where '\d' becomes 'd', so deposits only spawn in sectors W5N5/W5S5/E5N5/E5S5; early `return`s also abort the remaining sectors.
- calcWorldSize starts maxX/maxY at 0, so an all-W/N world is 1 room wider (default world = 12): terminal cost W0N0<->W10N0 uses wrapped distance 2.
- Storage writes (bulk.update/inc) change the in-memory object only when given the object, not its id; this is why same-tick renames/creates can produce duplicate power-creep names.
- Power-creep death never drops into a container (the power scope has no roomObjects); suicide twice, or suicide plus age death, inserts two tombstones.
- PowerCreep.shard is undefined while spawned on the private server (shardName ''), while Game.shard.name is the machine hostname.
- Power-creep runtime methods return OK even when the 50-intent cap silently dropped the intent; the runtime allows names of 100 chars, the processor truncates to 50.
- terminal executeTransfer returns undefined (not false) when target is full: send is cleared, no cooldown; energy cost is computed on the clamped amount.
- OPERATE_TERMINAL effect check is `endTime >= T` in the room stage but `> T` in the global stage.

## [09-pathfinding-terrain.md](09-pathfinding-terrain.md) — 101 items

The spec covers pathfinding and terrain end to end, with 101 stable inventory IDs. Areas: terrain storage (TERRAIN-*), room-name and coordinate helpers (COORD-*), path serialisation (PATHSER-*), CostMatrix (CM-*), the PathFinder JS layer (PF-API-*), the native PathFinder (PF-CORE-*), the Room/RoomPosition wrappers, the legacy @screeps/pathfinding branch (LEGACY-*), world routing (MAP-*) and processor/NPC pathfinding (NPC-PF-*). Every rule cites reference/<pkg>/<path>:<line>.

Terrain: the DB holds a row-major string per room. The world blob is deflate+base64 and padded with all-wall rooms; out-of-borders rooms are also all-wall in it. The runtime unpacks 2500 B per room; the native PathFinder packs 2 bits per tile, column-major, 625 B per room. Terrain code 3 (wall+swamp) occurs in real data and each consumer reads it differently.

Native PathFinder: the spec gives pseudocode for every routine, including the exact heap tie-breaking, room loading and callback order, both heuristics, the JPS forced-neighbour and border rules, and path reconstruction.

Conformance: the driver's profile.js is a golden test (expected checksum 11843305) that chitin can port directly.

findRoute: Manhattan-heuristic A* over a 60×60 room window, with exits counted from plain tiles only.

Biggest finding: PathFinder.use(false) only takes effect on the first tick of a fresh global, so the legacy pathfinder is effectively dead code.

Top 1:1 pitfalls:
- Two transposed layouts: terrain is row-major (y*50+x), but CostMatrix._bits and the native PathFinder index are column-major (x*50+y).
- Native heap tie-breaking must be ported verbatim: bubble_up uses <= (a new node rises above equal-priority nodes); pop uses >= (the sinking node goes below equal children, right child preferred on ties); update is a linear scan from the end. A standard BinaryHeap gives different paths.
- Heuristic weight must be computed as (u32)(h as f64 * w) with truncation, at both push and pop (g = f - trunc(h*w)). Integer approximations change tie-breaks.
- Neighbour push order sets the tie-breaks. Origin: TOP..TOP_LEFT with border-move restrictions. Popped nodes: straight-x, straight-y, diagonal, then the two forced neighbours. Forced means the side tile's cost differs, not only that it is an obstacle (weighted JPS).
- The origin is never marked open or closed and can be pushed back later. ops counts only main-loop expansions, not the origin expansion, the goal pop or a maxCost-break pop.
- Incomplete results go to the lowest unweighted h among popped nodes, first pop wins ties. A goal is accepted before the maxCost check, so a complete path can cost more than maxCost. If nothing is ever popped, cost is 4294967295.
- Option clamps use |0: maxCost >= 2^31 becomes 1; maxCost 0/Infinity means unlimited; swampCost 0 becomes 5; plainCost/swampCost are capped at 254.
- roomCallback: only boolean false blocks a room, and blocked rooms do not count toward maxRooms. Any other value uses ret._bits, and only if its length is exactly 2500, so a bare Uint8Array return or a deserialized matrix of the wrong length is silently ignored.

## [11-world-data-model.md](11-world-data-model.md) — 201 items

The spec covers the original server's stored world and everything that shapes it. The inventory has 201 IDs, and every ID has its own section with source citations. Topics:
- The LokiJS storage layer: how updates are applied, how IDs are generated, migrations, the work queues and pubsub.
- Every env key and every collection, with its document schema.
- Every user field, and the stored fields of every room-object type (creeps, power creeps, sources, minerals, deposits, controllers, every structure, sites, drops, tombstones, ruins, nukes, flags). These are the same fields the websocket room diff sends.
- Terrain: the 0–3 encoding, how the world terrain blob is compressed, and how each consumer reads the value 3.
- Room naming: the W0/E0 seam, five different name parsers, and the RoomPosition and pathfinder range limits.
- Sectors, highways and center/source-keeper rooms, and room status (normal, out of borders, novice, respawn, openTime).
- map.generateRoom described step by step so it can be reproduced: tables, smoothing, flood check, how sources, lairs, the controller and the mineral are placed, and the order in which it draws random numbers.
- The shipped default world, and the cron jobs that spawn power banks, deposits, invaders and strongholds.
- A categorized plan for porting constants.js, noting computed values, sparse tables, floats, aliases and how constants are exposed to players.

Key findings:
- Writes deep-merge nested objects, so emptied keys stay at 0 or null rather than being removed. A pending-insert bug can silently lose objects.
- Code 3 (wall plus swamp) appears in generated terrain and is returned raw by getRawBuffer.
- getWorldSize returns 12 for the default 11×11 world.
- Room coordinates are effectively capped at W127..E127.
- The deposit cron's regex only matches the four origin sector centers.
- generateRoom never sets bus or depositType.

Top 1:1 pitfalls:
- driver bulk.update deep-merges object fields: nested keys are never deleted, so emptied store entries stay as 0 and lab storeCapacityResource[mineral] becomes null. getDiff never transmits removed sub-keys.
- Fields starting with '_' are transient and stripped before every write; only _id is ever stored.
- Pending-insert bugs: updates to a doc inserted in the same tick are lost unless it carries the temporary id. bulk.remove of a pending insert decrements the counter, so the next insert can overwrite another pending object.
- $inc with a dotted key ('resources.pixel') creates a literal top-level key, not a nested one.
- Terrain code 3 (wall+swamp) exists in generated rooms. Room.Terrain.get returns 1, but getRawBuffer returns 3. A road on such a tile costs and has hits multiplied by 5*150.
- Every dropped resource is stored with type 'energy', and its amount sits under a key named after the resource. LOOK_RESOURCES is 'resource'.
- calcWorldSize starts maxX and maxY at 0, so getWorldSize() is 12 for the default W0..W10/N0..N10 world.
- RoomPosition packing limits the world to W127..E127. W127N127 reads back as roomName 'sim', and the cached getRoomNameFromXY returns 'sim' for (-128,-128).

## [12-xxscreeps-architecture.md](12-xxscreeps-architecture.md) — 120 items

Spec of xxscreeps (@d1e3bad) as prior art, 120 inventory items (XX-*), each cited to source. Covers: mod system (manifest {dependencies, provides}; DFS load order with sorted siblings and cycle errors; 9 provide slots and which service loads each; virtual xxscreeps:mods/* modules; config merge/validation; hook registry with frozen-after-first-use semantics; full hook catalogue for game/runner/processor/driver/backend; schema extension paths Room / Room.objects / Room.eventLog / ResourceType / ActionLog.action; intent registration with before/after/type/internal; tick, room, shard and NPC processors; '#'-prototype protocol, renderers, storage providers, inter-mod registries; built-in mod catalogue). Also the binary blob format (layout, header, overlay getters, writer, SHA-1 versioning and upgrade, id and position packing), storage (local keyval host thread, blobs, double buffer, key layout, saves), the main/processor/runner tick pipeline (per-room readiness queue, phases, intent dispatch, movement, tasks, inter-room finalize phase, sleep/wake), runtime (sandbox, CPU, Memory, flags), pathfinder and backend. Ends with enumerated perf wins and bottlenecks, 17 vanilla compatibility gaps (movement, weight, intent order, stale objects, Memory, CPU, border crossing, event log, dropped intents in vision-only rooms...), concrete copy/avoid lists for chitin, and open questions.

Top 1:1 pitfalls:
- Movement conflicts use power/weight priority with Math.random() tie-breaks (movement.ts:169); vanilla uses rate1..rate4 per target tile (engine movement.js:105-140). Port vanilla.
- Creep weight is ceil(used/50) plus non-MOVE/CARRY parts, ignoring boosted CARRY capacity; vanilla counts the live CARRY parts needed, from the end of the body.
- Per-object intent order comes from before/after constraints through a non-topological selection sort; vanilla uses a fixed creepActions list (pull runs last) plus a blocking table.
- Cross-user intent order inside a room is the order runner payloads arrived (nondeterministic); NPC intents run after players.
- Objects kept from a previous tick throw when read (buffers are detached); vanilla keeps stale objects readable.
- The parsed Memory object is reused across ticks (only undefined values are cleaned). Vanilla runs JSON.parse each tick, gives the root a null prototype and redefines Memory as a data property.
- CPU limit is fixed at 100, the 0.2-per-intent cost is never charged, and getUsed measures wall time. Vanilla charges ceil(clean time + 0.2 per intent, except say/pull).
- Border crossing resets fatigue to 0 and always removes the creep, even when the destination room is missing; vanilla keeps fatigue and skips inaccessible rooms.

## [14-js-engine-requirements.md](14-js-engine-requirements.md) — 137 items

This spec says what chitin's own JS engine must implement for bots to run unchanged. It draws on four sources: the official runtime (driver, engine and lodash 3.10.1); the isolated-vm 6.1.2 source, which the driver pins and which needs Node ≥22, so the engine era is V8 12.4 / ES2024; a TypeScript-AST scan of 355 JS/TS bot repos, with evidence counts per feature; and V8 behaviour probes on a local Node.

Key findings:
- The runtime and player code share one realm.
- Modules run inside a sloppy function wrapper. Line-1 columns shift by +36 and the script name is the bare module name, which makes V8's stack text and frame naming a hard contract: 80 repos parse stacks and 89 use source maps.
- esbuild bundles ship ES2020-2022 syntax natively; TS sources mostly target ES2017/18.
- Promise jobs run only after tick output is captured, and an unhandled rejection discards the whole tick.
- A CPU timeout cannot be caught, yet intents and Memory changes made before it are kept. isolated-vm, not V8, formats the timeout stack.
- lodash's isNative and isFunction depend on exact Function.prototype.toString text and type tags, so an async loop never runs.
- Timers, TextEncoder and console.error are absent, and bots polyfill them.

The spec covers 137 inventory items, a frame-naming algorithm fitted to about 60 probe cases, a P0/P1/P2 list and the performance requirements. It also answers three open questions from spec 02.

Top 1:1 pitfalls:
- `module.exports.loop = async function(){}` is never run: the runtime checks lodash 3 `_.isFunction`, which returns false for `[object AsyncFunction]` and `[object GeneratorFunction]`.
- Line-1 columns are +36 because of the `(function __module(module,exports){ ` prefix, and the script name is the bare module name (`main`). Error mappers use V8's 1-based columns as-is.
- Promise continuations run only after the tick's output has been captured, so their intents and logs are lost. An unhandled rejection rejects the whole run, which discards that tick's intents and Memory.
- A CPU timeout terminates the script in a way user code cannot catch (`finally` blocks do not run), but intents and Memory mutations made before it are still committed.
- The timeout error's stack is rendered by isolated-vm, not V8: no `Type.` prefix and no `[as x]`, it stops at the first anonymous frame, and the header shows the rewritten 'CPU time limit reached' message.
- `console` is a null-prototype fake with only log/logUnsafe (plus internal methods): `console.error`/`warn` throw TypeError, `console.log(NaN)` prints `null`, and `console.log('')` prints `""`.
- setTimeout, queueMicrotask, TextEncoder/TextDecoder, structuredClone, atob, performance, Buffer and process must be ABSENT, because bots detect them and polyfill. lodash `debounce`/`delay` throw as a result.
- `Function.prototype.toString` must return the exact source slice. Overmind's obfuscated module runs `(((.+)+)+)+$` against its own source and hangs on any newline. lodash's reIsNative needs V8's `function f() { [native code] }` text.

