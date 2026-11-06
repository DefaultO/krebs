# 02 — Player runtime / sandbox

Status: research spec for chitin, a from-scratch Rust reimplementation. Covers everything between "a tick starts for player P" and "P's intents, Memory, segments, console output are handed back". The game-object API itself (Creep/Room/... methods) is in other specs. This spec covers how those objects are **constructed, exposed, cached and invalidated**.

## 0. Sources, confidence tags, conventions

Citations use `reference/<pkg>/<path>:<line>`. Packages: `engine` (@8097782), `driver` (@cf63d8a), `common` (@2fb779b), `storage` (@3d6c3d0), `backend` (3.3.0), `launcher` (4.2.0), `docs`, `lodash-3.10.1`, `xxscreeps` (@d1e3bad, comparison only).

isolated-vm is not vendored under `reference/`. The driver pins it to git commit `cb93efcc` (`reference/driver/package.json:11`). The local install `E:\screeps\server\node_modules\isolated-vm` is that commit: its lockfile entry resolves to `#cb93efcc…` with version 6.1.2 (`E:\screeps\server\package-lock.json:2930-2932`). It is cited as `ivm/<path>:<line>`, relative to that directory.

Each inventory item has a **confidence tag**:

* **E**: the code is in `@screeps/engine` (`engine/src/game/*`). The official servers run this package too, behind their own closed-source driver, so these items are the strongest 1:1 evidence.
* **D**: the code is in the open-source private-server driver, backend or launcher (`driver/lib/*`, `backend/*`, `launcher/*`). The official runtime is closed-source and implements the same API, but details such as messages, limits and timing may differ. Each D item is marked as either "replicate", meaning the private-server behaviour is the best evidence, or "PS-QUIRK", meaning it is plausibly private-server-only and gets a recommendation.
* **Doc**: documented in `docs/` with no reference implementation, such as official-only features. Semantics come from the docs, and unknowns go to §16.

Markers: **GOTCHA** = subtle 1:1 pitfall. **PERF** = performance-relevant structure.

Terminology: *VM* = one player's persistent JS heap/context ("global"). *Global reset* = the VM is destroyed and recreated, losing all heap state and the require cache. *Runtime data* = per-player per-tick input snapshot (`driver/lib/runtime/data.js`).

---

## 1. Inventory

| ID | Item | Tag | Primary source |
|---|---|---|---|
| **VM — per-player VM lifecycle** ||||
| VM-ISOLATE-PER-USER | One persistent isolate+context per user per runner process, reused across ticks | D | driver/lib/runtime/user-vm.js:13-101 |
| VM-BOOT | Boot from runtime snapshot/bundle, `_init()`, bootstrap-global cleanup | D | driver/lib/runtime/user-vm.js:30-96 |
| VM-HEAP-LIMIT | Heap limit 256 MB + shared terrain size | D | driver/lib/runtime/user-vm.js:30 |
| VM-CODE-VERSION-RESET | New VM when code timestamp is strictly newer than the VM's | D | driver/lib/runtime/user-vm.js:23-25 |
| VM-DISPOSE-ERRORS | Heap exhaustion / disposed isolate / ArrayBuffer alloc failure → VM cleared | D | driver/lib/runtime/make.js:279-294, user-vm.js:16-20 |
| VM-HARD-TIMEOUT | 5 s watchdog → VM cleared | D | driver/lib/runtime/make.js:297-318 |
| VM-IDLE-EVICT | VM unused for 3 min is disposed (checked every 60 s) | D | driver/lib/runtime/user-vm.js:160-166 |
| VM-RESTART-SIGNAL | `RUNTIME_RESTART` pubsub clears all VMs; runner process restart | D | driver/lib/index.js:102-108, launcher/lib/start.js:26,139-141 |
| VM-MULTI-RUNNER | VMs are per runner process, so a user can hop between heaps | D | launcher/lib/start.js:206-215, driver/lib/runtime/user-vm.js:11 |
| VM-RESET-SUMMARY | Consolidated list of global-reset triggers | D/E | §2.10 |
| **TICK — per-tick lifecycle** ||||
| TICK-MAIN-LOOP | Tick stages: users, then rooms, then global, then gametime++ | D | engine/src/main.js:11-127 |
| TICK-USER-ELIGIBILITY | Users run iff `active≠0 && cpu>0`, ordered by lastUsedDirtyTime desc | D | driver/lib/index.js:144-151 |
| TICK-USER-INACTIVE | User with zero `rooms.objects` → `active=0`, never run again until upload | D | driver/lib/runtime/data.js:103-108, backend/lib/game/api/user.js:102 |
| TICK-RUNNER-CONCURRENCY | Per runner process pool, default 2 concurrent users | D | driver/lib/index.js:558-581 |
| TICK-RUN-SEQUENCE | make(): penalty/bucket check → data → VM → start → sandbox hook → run → post | D | driver/lib/runtime/make.js:113-295 |
| TICK-DATA | Runtime data assembly (all fields) | D | driver/lib/runtime/data.js:99-264 |
| TICK-CACHES | Process-level caches: terrain, accessibleRooms, roomStatus, market | D | driver/lib/runtime/data.js:15-97, make.js:18-51 |
| TICK-REBUILT-VS-PERSISTENT | What is rebuilt each tick vs persists in the VM | E/D | §3.8 |
| **VIS — visibility** ||||
| VIS-ROOMS | Visible rooms = rooms of own objects except flags/construction sites, plus observer targets | D | driver/lib/runtime/data.js:113-127 |
| VIS-OBSERVER | Observer target visible on the tick after the intent | E | engine/src/processor.js:116-118,284-288,403-406 |
| VIS-TOMBSTONE | Own creeps' tombstones carry `user` and so grant visibility | D (PS-QUIRK?) | engine/src/processor/intents/creeps/_die.js:22-26 |
| VIS-OWN-OUTSIDE | Own construction sites / flags in invisible rooms still exposed | E | engine/src/game/game.js:366-382,397-429 |
| VIS-USERS | `runtimeData.users`: owners referenced by visible objects | D | driver/lib/runtime/data.js:178-198,224 |
| **GLB — global scope** ||||
| GLB-LIST | Complete list of globals visible to player code | E/D | §4.1 |
| GLB-GLOBAL-ALIAS | `global` = the global object | D | driver/lib/runtime/user-vm.js:59 |
| GLB-CLASSES-ONCE | Game classes created once per VM; prototype patches persist | E | engine/src/game/rooms.js:401, creeps.js:46, custom-prototypes.js:16 |
| GLB-CLASS-DESCRIPTORS | Class globals are enumerable, non-writable, non-configurable | E | engine/src/game/creeps.js:1123 (etc.) |
| GLB-CONSTANTS | All constants re-injected as a fresh deep copy each tick | E | engine/src/game/game.js:435 |
| GLB-LODASH | `_` = lodash 3.10.1 `runInContext()`, created once per VM | E | engine/src/game/game.js:475-477 |
| GLB-PERTICK | Game, RawMemory, console, Memory accessor reassigned each tick | E | engine/src/game/game.js:470-502 |
| GLB-LEAKS | `__module` and `module` leak to global scope | D | driver/lib/runtime/runtime-driver.js:38,97 |
| GLB-SYSFUNC | Runtime captures frozen builtins before player code runs | D | driver/lib/runtime/system-functions.js:1-30 |
| GLB-ABSENT | Not present: timers, Buffer, process, console.error/warn, InterShardMemory (PS) | D | §4.6 |
| **MOD — modules & code** ||||
| MOD-STORAGE | Code stored per branch; activeWorld; 5 MB upload limit; timestamp | D | backend/lib/game/api/user.js:82-120 |
| MOD-NAMES | Module-name escaping `.`/`/`/`\`; `$`-names dropped; `main` forced | D | backend/lib/utils.js:62-86, driver/lib/runtime/data.js:167-176 |
| MOD-REQUIRE | `require(name)` resolution: strip leading `./`, flat namespace | E | engine/src/game/game.js:551-594 |
| MOD-REQUIRE-CACHE | `require.cache` plain object, `in` lookup, user-mutable | E | engine/src/game/game.js:509-511,555 |
| MOD-REQUIRE-LODASH | `require('lodash')` returns the global `_` | E | engine/src/game/game.js:510 |
| MOD-REQUIRE-ERRORS | Unknown module, circular reference, eval failure un-caches | E | engine/src/game/game.js:557-559,575-581,590-591 |
| MOD-WRAPPER | Module wrapper text, params, `this`, line/column offsets | D | driver/lib/runtime/runtime-driver.js:67 |
| MOD-MODULE-OBJECT | `module` object fields visible to code | E/D | engine/src/game/game.js:567-573, runtime-driver.js:27-31 |
| MOD-SCRIPT-CACHE | Compiled scripts cached per VM by (name, timestamp) | D | driver/lib/runtime/runtime-driver.js:64-94 |
| MOD-BINARY | Binary modules → `Buffer` (Uint8Array subclass) | E/D | engine/src/game/game.js:561-562, runtime-driver.js:13-15 |
| MOD-WASM | WebAssembly must be available | Doc | docs/source/modules.md:28-101 |
| MOD-MAIN-LOOP | `require('main')` each tick, then `module.exports.loop()` | E/D | engine/src/game/game.js:517-533 |
| MOD-NO-LOOP | No `loop` export → legacy mode: whole module graph re-evaluated every tick | E | engine/src/game/game.js:504-512 |
| MOD-REQUIRE-RESET | When a fresh `require` (empty cache) is created | E | engine/src/game/game.js:504-512 |
| MOD-INITGLOBALS | `__initGlobals` / `require.initGlobals`: dead feature, still has descriptors | E/D | engine/src/game/game.js:522-524,584-587, runtime-driver.js:27-36 |
| MOD-TIMEOUT | Each evaluation gets its own timeout `max(30, tickLimit+5)` ms | D | driver/lib/runtime/runtime-driver.js:40-45 |
| **GAME — Game object & construction** ||||
| GAME-SHAPE | Game property list, descriptors, key order | E | engine/src/game/game.js:136-182 |
| GAME-TIME | `Game.time` | E/D | engine/src/game/game.js:144, common/index.js:64-66 |
| GAME-SHARD | `Game.shard` {name,type,ptr}, null-proto, set by sandbox hook | D | driver/lib/index.js:35-52 |
| GAME-GCL | `Game.gcl` formula | E | engine/src/game/game.js:130-131,159-163 |
| GAME-GPL | `Game.gpl` formula | E | engine/src/game/game.js:133-134,164-168 |
| GAME-RESOURCES | `Game.resources` deep copy of account resources | E | engine/src/game/game.js:170 |
| GAME-CPULIMIT-LEGACY | `Game.cpuLimit` (deprecated) = tickLimit | E | engine/src/game/game.js:145 |
| GAME-GETOBJECTBYID | Lookup in per-tick registry; quirks | E | engine/src/game/game.js:171-173 |
| GAME-NOTIFY | `Game.notify(message, groupInterval)` | E/D | engine/src/game/game.js:174-181, driver/lib/index.js:185-227 |
| GAME-CREEPS | `Game.creeps` by name (incl. spawning) | E | engine/src/game/game.js:270-285 |
| GAME-POWERCREEPS | `Game.powerCreeps` by name (incl. unspawned) | E | engine/src/game/game.js:255-258,286-297 |
| GAME-SPAWNS | `Game.spawns` by name | E | engine/src/game/game.js:313-322 |
| GAME-STRUCTURES | `Game.structures` by id (own OwnedStructures) | E | engine/src/game/game.js:298-311 |
| GAME-CONSTRUCTIONSITES | `Game.constructionSites` by id (own, any room) | E | engine/src/game/game.js:366-382 |
| GAME-FLAGS | `Game.flags` by name (all rooms) | E | engine/src/game/game.js:397-429 |
| GAME-ROOMS | `Game.rooms` shallow clone of visible rooms | E | engine/src/game/game.js:218-227 |
| GAME-MAP | `Game.map` (API in map spec); mapGrid built once per VM | E/D | engine/src/game/map.js:11-380, driver/lib/runtime/runtime.js:206-208 |
| GAME-MARKET | `Game.market` (API in market spec) | E | engine/src/game/game.js:433 |
| GAME-CONSTRUCT | Object construction algorithm and order | E | engine/src/game/game.js:50-438 |
| GAME-REGISTER | Internal register: `_objects`, byRoom, spatial, findCache | E | engine/src/game/game.js:30-48,67-111,15-28 |
| GAME-LAZY-PROPS | `defineGameObjectProperties`: lazy getters cached on `_name` | E | engine/src/utils.js:509-545 |
| GAME-IDENTITY | Objects are new each tick; stale objects keep working through ids | E | §6.6 |
| GAME-DUP-NAME | Duplicate own creep name → the later one gets `suicide()` | E | engine/src/game/game.js:273-279 |
| GAME-ENERGY-AVAILABLE | room.energyAvailable/CapacityAvailable computed in construction | E | engine/src/game/game.js:325-328 |
| GAME-DEPRECATED-LOG | Deprecation messages console.log'ed once per tick | E | engine/src/game/game.js:113-120 |
| GAME-ASSERT-SERIALIZED | Error when a 24-char-id plain object is used as a target | E | engine/src/game/game.js:122-126 |
| **CPU** ||||
| CPU-LIMIT | `Game.cpu.limit` = account CPU | E/D | engine/src/game/game.js:151 |
| CPU-TICKLIMIT | `Game.cpu.tickLimit = min(cpuMaxPerTick=500, bucket+limit)` | D | driver/lib/runtime/data.js:151-165 |
| CPU-BUCKET-VALUE | `Game.cpu.bucket` = bucket at tick start, clamped ≥0 | D | driver/lib/runtime/data.js:153-156 |
| CPU-GETUSED | `Game.cpu.getUsed()` = `isolate.cpuTime` delta since run start (wall-rate, CPU-CLOCK) + 0.2/intent | D | driver/lib/runtime/runtime.js:202-204,235 |
| CPU-MEASURE | What is and is not counted | D | §8.4 |
| CPU-INTENT-COST | 0.2 CPU per non-free intent; `say`/`pull` free | D | driver/lib/runtime/runtime.js:60-99, make.js:98-111 |
| CPU-BILLING | Billed `ceil(cleanTime + 0.2*finalIntentCount)` | D | driver/lib/runtime/make.js:193-194 |
| CPU-BUCKET-UPDATE | `bucket += limit − used`, capped at 10000, may go negative | D | driver/lib/runtime/make.js:206-212 |
| CPU-BUCKET-EMPTY | Negative bucket → tick skipped with error, partial refill | D | driver/lib/runtime/make.js:72-83 |
| CPU-PENALTY-SKIP | `skipTicksPenalty` → tick skipped (dormant in PS) | D | driver/lib/runtime/make.js:61-71 |
| CPU-TIMEOUT | CPU limit enforcement and message | D | driver/lib/runtime/runtime-driver.js:40-45,105-107 |
| CPU-CLOCK | Timeout and getUsed()/billing read the same steady wall clock, on Linux too | D | driver/lib/runtime/runtime.js:25-27; ivm/src/isolate/executor.cc:67-73; ivm/src/lib/timer.cc:176-180 |
| CPU-TIMEOUT-BILL | A timed-out tick bills ≥ `max(30, tickLimit+5)`, so with `bucket+limit ≤ 500` the next tick is skipped | D | driver/lib/runtime/make.js:72-83,193-212 |
| CPU-HEAPSTATS | `Game.cpu.getHeapStatistics()` shape | D/Doc | driver/lib/runtime/runtime.js:228-230, docs/api/source/Game.md:183-213 |
| CPU-HALT | `Game.cpu.halt()` → dispose VM, tick discarded, "CPU halted" | D/Doc | driver/lib/runtime/runtime.js:19-23, user-vm.js:68-71, make.js:281-284 |
| CPU-REPORT | lastUsedCpu/lastUsedDirtyTime/pubsub cpu+memory size | D | driver/lib/runtime/make.js:196-198,262-273 |
| CPU-SHARDLIMITS | `Game.cpu.shardLimits` (official) | Doc | docs/api/source/Game.md:30-32 |
| CPU-SETSHARDLIMITS | `Game.cpu.setShardLimits(limits)` (official) | Doc | docs/api/source/Game.md:255-277 |
| CPU-UNLOCKED | `Game.cpu.unlocked`, `Game.cpu.unlockedTime` (official) | Doc | docs/api/source/Game.md:34-38 |
| CPU-UNLOCK | `Game.cpu.unlock()` (official) | Doc | docs/api/source/Game.md:280-298 |
| CPU-GENERATEPIXEL | `Game.cpu.generatePixel()` costs PIXEL_CPU_COST=10000 bucket (official) | Doc | docs/api/source/Game.md:300-316, common/lib/constants.js:384 |
| **MEM — Memory / RawMemory** ||||
| MEM-GETTER | Lazy `JSON.parse` on first `Memory` access per tick | E | engine/src/game/game.js:479-500 |
| MEM-DESCRIPTORS | Accessor (getter only) → non-writable data property after access | E | engine/src/game/game.js:479-496 |
| MEM-ROOT-NULLPROTO | Root Memory object has `__proto__ = null` | E | engine/src/game/game.js:486 |
| MEM-PARSE-FAIL | Invalid JSON → `Memory === null`, nothing re-serialized | E/D | engine/src/game/game.js:488-490, runtime.js:246 |
| MEM-NONOBJECT | JSON scalar/array roots behave per §9.5 | E | engine/src/game/game.js:485-486 |
| MEM-SERIALIZE | End-of-tick `JSON.stringify(RawMemory._parsed)` (if truthy) | D | driver/lib/runtime/runtime.js:246-248 |
| MEM-SERIALIZE-THROW | Stringify throwing (cycle/BigInt/toJSON) → whole tick output lost | D | driver/lib/runtime/runtime.js:246-248, make.js:279-294 |
| MEM-RAW-OBJECT | RawMemory object shape, descriptors, null prototype | D | driver/lib/runtime/runtime.js:102-189 |
| MEM-RAW-GET | `RawMemory.get()` | D | driver/lib/runtime/runtime.js:103-107 |
| MEM-RAW-SET | `RawMemory.set(value)` validation and `_parsed` reset | D | driver/lib/runtime/runtime.js:108-121 |
| MEM-PARSED-HACK | `RawMemory._parsed` / `delete global.Memory` ("memhack") contract | E/D | engine/src/game/game.js:479-500, runtime.js:246 |
| MEM-SIZE-LIMIT | 2 MB (2·1024·1024 UTF-16 units); oversize at save silently dropped (PS) | D | driver/lib/index.js:153-160 |
| MEM-OBJ-ACCESSORS | creep/powerCreep/spawn/room/flag `.memory` contract | E | engine/src/game/creeps.js:95-120 (etc.) |
| MEM-BACKEND-EDIT | UI Memory edits = hidden console commands | D | backend/lib/game/api/user.js:285-316 |
| **SEG — segments** ||||
| SEG-OBJECT | `RawMemory.segments`: null-proto, non-writable property, preloaded | D | driver/lib/runtime/runtime.js:122-124,191-195 |
| SEG-SET-ACTIVE | `setActiveSegments(ids)` validation; effective next tick | D | driver/lib/runtime/runtime.js:129-147, make.js:200-202, data.js:228-253 |
| SEG-SAVE | End-of-tick save of **every** key present | D | driver/lib/runtime/runtime.js:250-269, index.js:162-168 |
| SEG-SAVE-ERRORS | >10 keys / bad id / non-string / >100 KB → whole tick output lost | D | driver/lib/runtime/runtime.js:252-266 |
| SEG-PUBLIC | `setPublicSegments(ids)` | D | driver/lib/runtime/runtime.js:148-162, make.js:249-251 |
| SEG-DEFAULT-PUBLIC | `setDefaultPublicSegment(id)` | D | driver/lib/runtime/runtime.js:163-173, make.js:203-205 |
| SEG-FOREIGN-SET | `setActiveForeignSegment(username, id)` | D | driver/lib/runtime/runtime.js:174-188 |
| SEG-FOREIGN-RESOLVE | Server-side resolution of username/default id | D | driver/lib/runtime/make.js:216-247 |
| SEG-FOREIGN-LOAD | `RawMemory.foreignSegment` availability rules | D | driver/lib/runtime/data.js:231-238,254-260, runtime.js:197-200 |
| SEG-INTERSHARD-LEGACY | `RawMemory.interShardSegment` (deprecated; inert in PS) | D/Doc | driver/lib/runtime/runtime.js:125-128, docs/api/source/RawMemory.md:49-72 |
| SEG-BACKEND-API | HTTP segment read/write | D | backend/lib/game/api/user.js:318-339 |
| **ISM — InterShardMemory** ||||
| ISM-API | `InterShardMemory.getLocal/setLocal/getRemote` (official only) | Doc | docs/api/source/InterShardMemory.md:1-45 |
| ISM-LIMITS | 100 KB per shard, own shard writable only | Doc | docs/api/source/InterShardMemory.md:7-8 |
| **CON — console, errors, visuals** ||||
| CON-OBJECT | `console` is a null-proto object rebuilt every tick | E | engine/src/game/console.js:6-97 |
| CON-LOG-FORMAT | Argument stringification rules | E | engine/src/game/console.js:20-27 |
| CON-LOG-UNSAFE | `console.logUnsafe` (no HTML escaping) | E/D | engine/src/game/console.js:30-48, driver/lib/index.js:367 |
| CON-LOG-SELF-HOOK | Global `self` with `navigator.userAgent` triggers browser-console forwarding | E | engine/src/game/console.js:16-18 |
| CON-LOG-TRANSPORT | he.encode escaping, pubsub channel; NPC users to server log | D | driver/lib/index.js:354-369 |
| CON-LIMITS | No per-tick message count/size limit in reference | D | §11.5 |
| CON-COMMANDS | Console expressions executed after loop via sloppy direct eval | E/D | engine/src/game/game.js:535-547, runtime-driver.js:58-61 |
| CON-COMMAND-LOSS | Commands dropped if loop or main throws | E/D | engine/src/game/game.js:519-547, data.js:149 |
| CON-ERR-PIPELINE | Uncaught error → `outMessage.error` → console + notification | D | driver/lib/runtime/runtime.js:240-244, index.js:371-410 |
| CON-ERR-FORMAT | Stack-trace post-processing rules | D | driver/lib/runtime/runtime-driver.js:101-121 |
| CON-ERR-NONERROR | `throw "str"`/`throw {}` → no error shown | D | driver/lib/runtime/runtime-driver.js:117-120, runtime.js:243 |
| CON-ERR-SYSTEM | Full list of system termination messages | D | §11.10 |
| CON-VISUAL | RoomVisual/MapVisual buffers, 500 KB/1000 KB limits | E | engine/src/game/console.js:57-95 |
| CON-VISUAL-SAVE | Visual persistence key/TTL | D | driver/lib/runtime/make.js:253-260 |
| **INT — intent store & output** ||||
| INT-STORE | Runtime intent store object (`set/push/pushByName/remove`) | D | driver/lib/runtime/runtime.js:63-100 |
| INT-SANITIZE | Intent field whitelisting/coercion by type | D | common/lib/system.js:4-140 |
| INT-ROUTING | Per-room routing of object intents; unknown ids dropped | E | engine/src/utils.js:405-436 |
| INT-NOTIFY-SAVE | Notification clamping, 20/tick, 500 chars, grouping | D | driver/lib/index.js:185-227 |
| **OUT — end of tick** ||||
| OUT-SUCCESS | Output message fields and what is persisted | D | driver/lib/runtime/runtime.js:271-296, engine/src/runner.js:18-50 |
| OUT-FAILURE-MATRIX | What is saved under each failure mode | D | §13.2 |
| **HOOK — moddability** ||||
| HOOK-PLAYER-SANDBOX | `playerSandbox` event: run/set/get code in the player context each tick | D | driver/lib/runtime/make.js:166-185 |
| HOOK-CUSTOM-OBJECT | `registerCustomObjectPrototype` | D/E | driver/lib/index.js:56-73, engine/src/game/custom-prototypes.js:8-58 |
| HOOK-CUSTOM-INTENT | `customIntentTypes` sanitization extension | D | driver/lib/index.js:28, common/lib/system.js:84-140 |
| HOOK-CONFIG-ENGINE | Tunables: cpuMaxPerTick, cpuBucketSize, mainLoopResetInterval, … | D | driver/lib/index.js:19-33 |
| HOOK-LOOP-STAGES | `runnerLoopStage` / `mainLoopStage` events | D | engine/src/runner.js:11-75, main.js:16-115 |
| **JS — language environment requirements** ||||
| JS-ENGINE-LEVEL | Modern ECMAScript + WebAssembly as in V8 | D/Doc | driver/package.json (isolated-vm) |
| JS-STACK-FORMAT | V8 `Error.stack` text format (source-map tooling depends on it) | D | §15.2 |
| JS-MATH | `Math.random` (non-deterministic), fdlibm-exact `Math.pow` etc. | E | engine/src/game/game.js:130-134 |
| JS-DATE | `Date` is wall clock | D | engine/src/game/game.js:540 |
| JS-ASYNC | Promise microtasks run after tick output capture (see §16) | D | driver/lib/runtime/runtime.js:233-297 |
| JS-STRICTNESS | Wrapper/runtime sloppy-mode consequences | D/E | §15.6 |

---

## 2. VM lifecycle (VM-*)

### 2.1 VM-ISOLATE-PER-USER
* The `vms` map is module-level in each runner process: `driver/lib/runtime/user-vm.js:11`. The key is `""+userId`.
* `create({userId, staticTerrainData, staticTerrainDataSize, codeTimestamp})` (`user-vm.js:13-101`):
  1. If a VM exists and `isolate.isDisposed`, clear it and throw `'Script execution has been terminated: your isolate disposed unexpectedly, restarting virtual machine'` (`:17-20`).
  2. If a VM exists but is not ready, return its pending creation promise (`:21-22`).
  3. If `codeTimestamp > vm.codeTimestamp`, clear it (see VM-CODE-VERSION-RESET).
  4. If there is no VM, create the isolate and context, boot, and store `{ready, context, start, evalFn, nativeModInstance, codeTimestamp}` (`:88-95`).
  5. Set `lastUsed = Date.now()` (`:100`).
* **PERF**: the original copies the whole runtime data into the isolate every tick with `new ivm.ExternalCopy(data)` (`make.js:145`, `:160`). That data includes **all user code modules**, market orders, transactions and event logs. chitin should keep code resident in the VM and hand in a shared read-only snapshot.

### 2.2 VM-BOOT
* The isolate is created from `build/runtime.snapshot.bin`, a V8 startup snapshot of the webpack bundle (`driver/make-runtime-snapshot.js:1-9`, `driver/webpack.config.js:5-22`). If the snapshot is missing, the bundle is compiled and run instead (`user-vm.js:34-38`, `:151-158`).
* Bootstrap globals are injected with `setIgnored`: `global`, `_ivm`, `_isolate`, `_context`, `_worldSize`, `_nativeMod`, `_constants`, `_customObjectPrototypes`, `_customIntentTypes`, `_halt` (`user-vm.js:59-71`). Then `_init()` runs (`:72`). It captures these into closures (`runtime.js:1-53`) and defines `_setStaticTerrainData`, `_evalFn` and `_start`.
* A cleanup script deletes every bootstrap global, `_evalFn`/`_start`/`_setStaticTerrainData` included, after references to them are taken (`user-vm.js:42-56`, `:74-86`). **Only `global` survives** from bootstrap.
* Terrain is a single `SharedArrayBuffer` per runner process (or an `ArrayBuffer` if SAB is unavailable), with 2500 bytes per room. Each VM receives `Uint8Array` views keyed by room (`make.js:20-51`, `runtime.js:45-49`).
* Custom object prototype `properties` and `prototypeExtender` are shipped as strings and `eval`'d inside the VM at `_init` (`runtime.js:34-44`, `driver/lib/index.js:56-73`).

### 2.3 VM-HEAP-LIMIT
* `memoryLimit: 256 + staticTerrainDataSize/1024/1024` (MB) (`user-vm.js:30`). The terrain buffer counts against the heap as external memory, so the limit is raised by its size.
* On exhaustion, isolated-vm disposes the isolate. `make.js` matches `/Isolate is disposed/` or `/Isolate has exhausted v8 heap space/`, clears the VM, and forwards the raw error text as the tick error (`make.js:285-293`).
* `/Array buffer allocation failed/` with no run result → VM cleared, and the error is `'Script execution has been terminated: unable to allocate memory, restarting virtual machine'` (`make.js:289-292`).
* **PERF**: 1000 players × 256 MB is far more than a 16-core consumer PC has. chitin needs a configurable limit with the same `heap_size_limit` reporting, and must keep terrain outside the per-player heap (as the original does).

### 2.4 VM-CODE-VERSION-RESET
* `users.code.timestamp` is set to `Date.now()` on upload (`backend/lib/game/api/user.js:106`). The runtime reads `userCodeTimestamp = code.timestamp || 0` (`data.js:204`).
* A new VM is created **only if** `codeTimestamp > vm.codeTimestamp` (strict) (`user-vm.js:23-25`).
* **GOTCHA (PS-QUIRK)**: switching the active branch to an *older* branch (`/set-active-branch`, `backend/lib/game/api/user.js:195-207`) does not satisfy `>`, so the isolate is kept. However, `game.init` sees `userCodeTimestamp != require.timestamp` and builds a fresh `require` (`game.js:504-512`). Result: new code is evaluated **inside the old heap**, so old globals and prototype patches survive. Recommendation for chitin: any change of active code version triggers a full global reset, and the version is a monotonically increasing counter, never a `Date`.

### 2.5 VM-DISPOSE-ERRORS — see 2.3 and the failure matrix in §13.2.

### 2.6 VM-HARD-TIMEOUT
* Every `make(userId)` is raced against `setTimeout(..., Math.max(5000, config.engine.mainLoopResetInterval))`, where the default `mainLoopResetInterval` is 5000 (`make.js:297-318`, `driver/lib/index.js:22`). On fire: `scope.abort = true`, reject `{error: 'Script execution timed out ungracefully, restarting virtual machine'}`, `runtimeUserVm.clear(userId)`, publish `{cpu:'error'}`.
* This is the only bound on console commands, which run without a timeout (CON-COMMANDS), and on infinite loops that the per-script timeout fails to interrupt.

### 2.7 VM-IDLE-EVICT
* Every 60 s, any VM with `lastUsed < now − 3 min` is disposed (`user-vm.js:160-166`). A player who is not run for more than 3 minutes (e.g. a paused server, or skipped ticks) gets a global reset.

### 2.8 VM-RESTART-SIGNAL
* The `RUNTIME_RESTART` pubsub calls `clearAll()` (`driver/lib/index.js:104-107`).
* The launcher kills each runner process after `restart_interval` = 3600 s, **once**, because the restart passes no interval (`launcher/lib/start.js:26`, `:139-148`). That resets every VM in the process.

### 2.9 VM-MULTI-RUNNER
* There are `runners_cnt` runner processes (default 1) (`launcher/lib/start.js:206-215`), and each has its own `vms` map. The users queue is shared, so with more than one runner a user's tick N and tick N+1 may run in **different heaps**.
* **GOTCHA**: bots that assume heap continuity (e.g. `global.lastTick === Game.time-1`) behave differently. chitin should pin each user to one VM (one heap) and make "migration" an explicit global reset.

### 2.10 VM-RESET-SUMMARY (global reset triggers in the reference)
1. First run in this runner process (no VM).
2. Code upload (strictly newer timestamp).
3. `Game.cpu.halt()`.
4. Heap exhaustion, disposed isolate, or ArrayBuffer allocation failure.
5. Hard 5 s timeout.
6. Idle for more than 3 minutes.
7. `RUNTIME_RESTART` signal or runner process restart (hourly once by default).
8. Running on a different runner process (multi-runner).

Partial resets that keep the heap but re-evaluate modules: see MOD-REQUIRE-RESET.

---

## 3. Per-tick lifecycle (TICK-*)

### 3.1 TICK-MAIN-LOOP (`engine/src/main.js:11-127`)
`notifyTickStarted` (rejects if paused) → `getAllUsers` → users queue `addMulti` → **wait for all users** → `getAllRoomsNames` → rooms queue → wait → commit → global processor → commit → `incrementGameTime` → every 20 ticks `updateAccessibleRoomsList` and `updateRoomStatusData` (`:79-83`) → `notifyRoomsDone` → custom stage → sleep `max(mainLoopMinDuration − used, 0)` (default 1000 ms, `driver/lib/index.js:21`).
* **PERF**: all players run before any room is processed. Players are independent of each other within the user stage, so this stage is embarrassingly parallel.

### 3.2 TICK-USER-ELIGIBILITY
* `users` with `active ≠ 0` **and** `cpu > 0` (`driver/lib/index.js:144-151`).
* They are sorted by `lastUsedDirtyTime` descending, so the heaviest players start first. That is good LPT scheduling, and chitin should keep it.

### 3.3 TICK-USER-INACTIVE
* `data.get` queries `rooms.objects` for `{user}`. If the result is empty, it sets `users.active = 0` and rejects with `false` (`data.js:103-108`). The tick produces no output and no console error (`make.js:279-294` → `{error:false}` → `sendConsoleError` returns early on falsy, `driver/lib/index.js:373-375`).
* Uploading code sets `active: 10000` (`backend/lib/game/api/user.js:102`).
* **GOTCHA**: a player whose objects are all gone stops executing, and console commands are not processed either. Objects here include tombstones (see VIS-TOMBSTONE).

### 3.4 TICK-RUNNER-CONCURRENCY
* `startLoop` uses a generic-pool with `max = RUNNER_THREADS || 2` (`driver/lib/index.js:558-581`). The launcher passes `opts.runner_threads`, but the default key is spelled `runners_threads`, so the value is normally undefined and the pool size is 2 (`launcher/lib/start.js:20`, `:213`).

### 3.5 TICK-RUN-SEQUENCE (pseudocode; `engine/src/runner.js:9-51`, `driver/lib/runtime/make.js`)
```
runUser(u):
  r = await makeRuntime(u) catching rejection as r            // runner.js:15-16
  if r.console: sendConsoleMessages(u, r.console)
  if r.error:   sendConsoleError(u, r.error)
  parallel: r.memory → saveUserMemory; r.memorySegments → saveUserMemorySegments;
            r.interShardSegment (never set in PS); r.intents → saveUserIntents

makeRuntime(u) = race(make(u), watchdog 5s)                   // make.js:297-318
make(u):
  terrain once per process                                    // make.js:119
  user = getUserData(u)        // may reject: penalty / bucket empty   make.js:54-96
  data = runtimeData.get(u)    // DB reads; deletes console commands   data.js:99-264
  vm = userVm.create(u, data.userCodeTimestamp)
  run = vm.start(copy(data))   // _start: build RawMemory, intents, console, Game  runtime.js:55-232
  emit 'playerSandbox'(sandbox, u)   // sets Game.shard; mods          make.js:166-185
  out = run()                  // loop, console cmds, Memory/segments serialization runtime.js:233-297
  out.intents = storeIntents(...)                              // make.js:192
  billing + users.$set + foreign segment + public segments + visuals + pubsub cpu
```

### 3.6 TICK-DATA (runtime data fields; `data.js:200-260`)
| Field | Source | Notes |
|---|---|---|
| `userObjects` | all `rooms.objects` with `user=u` | includes construction sites, objects in every room |
| `user` | `users` doc | cpu, cpuAvailable, gcl, power, resources, money, activeSegments, activeForeignSegment, … |
| `userCode` | active-world branch modules | keys un-escaped (`:167-176`) |
| `userCodeTimestamp` | code `timestamp || 0` | |
| `userMemory` | `{data: env MEMORY+u || "", userId}` | raw string |
| `consoleCommands` | `users.console` docs | **deleted from DB immediately** (`:149`) |
| `time` | `parseInt(env gameTime)` | `common/index.js:64-66` |
| `rooms` | `rooms` docs for visible room ids | |
| `roomObjects` | other users' objects in visible rooms, then `_.extend` with userObjects | key order: foreign objects first, own objects last (`:209`) |
| `flags` | `rooms.flags` docs of user (all rooms) | packed strings |
| `accessibleRooms` | JSON string, 60 s process cache | `:77-86` |
| `roomStatusData` | novice/respawn/closed maps, 60 s cache | `:88-97` |
| `transactions` | last 100 outgoing / 100 incoming | `:138-139` |
| `cpu`, `cpuBucket` | see CPU-TICKLIMIT | |
| `roomEventLog` | raw JSON strings per visible room | `:141` |
| `userPowerCreeps` | `users.power_creeps` by id | |
| `users` | users referenced (see VIS-USERS) | |
| `market` | `{orders (all active, per-tick cache), history (3 h cache), myOrders}` | `:31-75`, `:243-247` |
| `memorySegments` | active segments' strings (`""` if missing) | `:248-253` |
| `foreignMemorySegment` | `{username, id, data}` if public | `:254-260` |
| added in `_start` | `mapGrid`, `staticTerrainData`, `customObjectPrototypes` | `runtime.js:210-212` |

**PERF**: per player per tick, the original issues about 14 DB queries, then up to 6 more, and deserializes everything. Visible-room object sets are **room-local data shared by every observer**, so chitin should build one immutable snapshot per room per tick and let each player VM index into it. Market orders and history are **global** (one copy per tick). Transactions, flags, power creeps and Memory are **per player**.

### 3.7 TICK-CACHES
* Terrain: loaded once per runner process and never refreshed (`make.js:20-51`). Rooms added later are invisible to `Room.Terrain` until the runner restarts (PS-QUIRK).
* accessibleRooms: 60 s cache (`data.js:77-86`), refreshed by the main loop every 20 ticks (`main.js:79-83`).
* roomStatusData: 60 s (`data.js:88-97`).
* Market orders: refreshed when `gameTime` changes (`data.js:31-49`). History: 3 h (`:51-75`).
* `mapGrid` (describeExits/findRoute topology): built **once per VM** from the first tick's accessibleRooms (`runtime.js:206-208`). It is stale until a global reset (PS-QUIRK).

### 3.8 TICK-REBUILT-VS-PERSISTENT
| Thing | Lifetime | Source |
|---|---|---|
| `Game` and everything reachable from it (rooms, creeps, structures, ...) | new every tick | game.js:502 |
| Game object instances, their `pos`, `room`, lazy `_x` caches | new every tick | game.js:260-395 |
| `RawMemory` object (incl. `segments`, `_parsed`) | new every tick | runtime.js:102 |
| `console` object and its buffers | new every tick | runtime.js:222, console.js:6-9 |
| `Memory` global property | redefined every tick as accessor | game.js:479 |
| Constant globals (`OK`, `FIND_*`, `BODYPART_COST`, …) | fresh deep copy every tick | game.js:435 |
| Intent store | new every tick | runtime.js:63 |
| Class constructors and their prototypes (Creep, Room, RoomPosition, …) | **persist for VM lifetime** | rooms.js:401 etc. |
| `_` (lodash) | persists | game.js:475-477 |
| `require`, `require.cache`, module exports and closures | persist (unless MOD-REQUIRE-RESET) | game.js:504-512 |
| Any user-created global / `global.x` | persists | — |
| `__module`, `module` leaked globals | overwritten on every evaluation | runtime-driver.js:38,97 |
| `mapGrid`, compiled-script cache, `utils.getRoomNameFromXY` memo | persist | runtime.js:206, runtime-driver.js:8, rooms.js:1260-1274 |
| Per-room `privateStore` (path caches etc.) | new every tick | rooms.js:386, 439 |

---

## 4. Global scope (GLB-*)

### 4.1 GLB-LIST — names defined on the player global object
Built-ins: everything V8 provides in a fresh context, including `WebAssembly`, `globalThis`, typed arrays, `Intl` (if ICU), `eval`, `Function`, `Promise`, `Proxy`, `Reflect`, `Atomics`/`SharedArrayBuffer` (V8-dependent). The V8 default `console` is overwritten.

| Name | Type | Descriptor | Lifetime | Source |
|---|---|---|---|---|
| `global` | the global object | set via `setIgnored` (writable) | VM | user-vm.js:59 |
| `Game` | plain object | writable, enumerable, configurable | per tick | game.js:502 |
| `Memory` | accessor → value | see §9.2 | per tick | game.js:479-500 |
| `RawMemory` | null-proto object | writable data prop | per tick | game.js:470-473 |
| `console` | null-proto object | writable data prop | per tick | game.js:470-473 |
| `_` | lodash 3.10.1 instance | writable | VM (created only if `!globals._`) | game.js:475-477 |
| `require` | bound function + `.cache`, `.timestamp`, `.initGlobals` | writable | VM unless reset | game.js:509-511, runtime-driver.js:32-36 |
| `__module`, `module` | leaked module objects | writable | see GLB-LEAKS | runtime-driver.js:38,97 |
| all `C.*` constants (~500 names: `OK`, `ERR_*`, `FIND_*`, `LOOK_*`, `RESOURCE_*`, `BODYPART_COST`, `REACTIONS`, `POWER_INFO`, …) | numbers/strings/objects | writable, enumerable (plain assignment) | fresh copy per tick | game.js:435, common/lib/constants.js |
| `RoomObject`, `RoomPosition`, `Room`, `RoomVisual` | constructors | enumerable, **non-writable, non-configurable** | VM | rooms.js:1661,1638,1138,1220 |
| `Creep`, `PowerCreep` | constructors | same | VM | creeps.js:1123, power-creeps.js:414 |
| `Structure`, `OwnedStructure`, `StructureContainer`, `StructureController`, `StructureExtension`, `StructureExtractor`, `StructureKeeperLair`, `StructureLab`, `StructureLink`, `StructureObserver`, `StructurePowerBank`, `StructurePowerSpawn`, `StructureRampart`, `StructureRoad`, `StructureStorage`, `StructureTerminal`, `StructureTower`, `StructureWall`, `StructureSpawn`, `Spawn` (alias), `StructureNuker`, `StructurePortal`, `StructureFactory`, `StructureInvaderCore` | constructors | same | VM | structures.js:121-1490 |
| `Source`, `Mineral`, `Deposit`, `Resource`, `Energy` (alias of Resource), `Nuke`, `Tombstone`, `Ruin`, `ConstructionSite`, `Flag`, `Store` | constructors | same | VM | sources.js:45, minerals.js:46, deposits.js:44, resources.js:45-46, nukes.js:44, tombstones.js:240, ruins.js:72, construction-sites.js:59, flags.js:93-96, store.js:99 |
| `PathFinder` | object `{CostMatrix, search, use}`, props enumerable non-writable non-configurable | same | VM | engine/src/game/path-finder.js:50-78 |
| custom object prototypes (mods) | constructors | **plain assignment** (writable) | VM | custom-prototypes.js:56 |

Nested statics: `Room.Terrain` (rooms.js:1223), `Room.serializePath/deserializePath` (rooms.js:528-534), `StructureSpawn.Spawning` (structures.js:1301), `PathFinder.CostMatrix` (+`.deserialize`) (path-finder.js:44-55). These are plain assignments on the constructors, so they are writable.

**Official-only globals (Doc)**: `InterShardMemory` (ISM-API). Not present in the reference private server.

### 4.2 GLB-CLASSES-ONCE
* Every `make()` assigns its module-level `runtimeData/intents/register/globals` **every tick**, then `return`s early if the global class already exists (e.g. `rooms.js:376-403`, `creeps.js:37-48`, `custom-prototypes.js:16-18`).
* Consequences:
  1. Prototype objects keep the same identity for the whole VM lifetime. `Creep.prototype.foo = …` done at module load applies to every future tick's creeps.
  2. Engine methods resolve per-tick state through module-level variables (`runtimeData`, `intents`, `register`). Any instance, old or new, therefore reads the **current** tick's data. See GAME-IDENTITY.
* **GOTCHA (architecture)**: the engine's own methods call through public, patchable paths, e.g. `Structure.prototype.destroy` calls `this.room.find(...)` (`structures.js:80-81`) and memory accessors use `globals.Memory` (`creeps.js:100-106`). Bots that override `Room.prototype.find`, `Creep.prototype.moveTo`, getters etc. change engine-internal behaviour. chitin must reproduce the same internal call graph: implement the player-facing API in JS (ported from `engine/src/game/*`), or ensure native code performs the identical observable property lookups.

### 4.3 GLB-CLASS-DESCRIPTORS
* `Object.defineProperty(globals, 'Creep', {enumerable: true, value: Creep})` gives a non-writable, non-configurable global. `Creep = X` silently fails in sloppy mode and throws `TypeError` in strict mode.
* `PathFinder.search/use/CostMatrix` are non-writable, so profilers that try to wrap `PathFinder.search` silently fail or throw (`path-finder.js:50-76`).
* Lazy game-object getters on prototypes are **non-configurable** and have no setter (GAME-LAZY-PROPS). `creep.hits = 1` is ignored (or throws in strict mode), and `Object.defineProperty(Creep.prototype,'hits',…)` throws.

### 4.4 GLB-CONSTANTS
* `_.extend(globals, JSON.parse(JSON.stringify(C)))` runs at the end of every `makeGameObject` (`game.js:435`). The engine itself uses its private `C` (`runtime.js:6`, `driver.constants = _constants`).
* **GOTCHA**: a bot that mutates `BODYPART_COST.work` or reassigns `OK` affects only itself, for one tick. Module-level captures (`const X = REACTIONS`) keep an old copy forever, and identities differ from tick to tick. The constants contain no `Infinity`/functions (verified: `common/lib/constants.js`), so the JSON round-trip is lossless.
* **PERF**: this is a full deep copy of the constants (tens of KB) per player per tick. chitin can install copy-on-write or lazily materialized constants, provided mutation still never leaks across ticks and each tick sees fresh identities. Identity freshness is observable but rarely relied on, so it is acceptable to give the same identity while unmodified. Mark this as a deliberate deviation if taken.

### 4.5 GLB-LODASH / GLB-LEAKS / GLB-SYSFUNC
* `_`: `if (!globals._) globals._ = _.runInContext()` (`game.js:475-477`), using the bundled lodash 3.10.1 (`reference/lodash-3.10.1/lodash.js`). If the user deletes or overwrites `_`, a new instance is created next tick only when `_` is falsy. chitin must ship lodash 3.10.1 verbatim and evaluate it in its engine. Bots depend heavily on v3 APIs (`_.sum`, `_.pluck`, `_.indexBy`, `_.contains`).
* GLB-LEAKS: `evalCode` sets `globals.__module = module` before running (`runtime-driver.js:38`), then sets `globals.module = oldModule` after a **successful** run (`:97`). This is a bug: it was meant to restore `__module`. `oldModule` is the previous `__module` or `{}`. After any evaluation, `global.module` and `global.__module` are therefore defined. **GOTCHA** for the bot-detection idiom `typeof module !== 'undefined'` at global scope (e.g. in console commands).
* GLB-SYSFUNC: the runtime snapshots `Object.create/keys/setPrototypeOf`, `JSON.stringify/parse`, `Math.ceil`, and `Array.prototype.push/join` (call-bound), then deep-freezes them (`system-functions.js:1-30`). Memory serialization at end of tick uses the **original** `JSON.stringify` (`runtime.js:247`). **GOTCHA**: the `Memory` getter uses the **current** global `JSON.parse` (`game.js:485`), so a bot that overrides `JSON.parse` affects Memory parsing but not serialization.

### 4.6 GLB-ABSENT (private server)
* There is no `setTimeout`/`setInterval`/`setImmediate`/`queueMicrotask`/`process`/`Buffer`/`TextEncoder`/`TextDecoder`/`performance`/`structuredClone` (isolated-vm contexts expose none). `Buffer` exists only internally to make binary modules.
* `console` has **only** `log`, `logUnsafe`, plus internal non-enumerable `commandResult/addVisual/getVisualSize/clearVisual/getVisual` (`console.js:10-96`). **GOTCHA**: `console.error(...)`, `console.warn`, `console.info` throw `TypeError: console.error is not a function`. This is engine code (E), so it is likely the same on official.
* `InterShardMemory` and `Game.cpu.{shardLimits,unlocked,unlockedTime,setShardLimits,unlock,generatePixel}` do not exist in the private server. They do exist on official (Doc), and many bots call `Game.cpu.generatePixel()` unguarded, so chitin should implement them (§8.10-8.13, §10).

---

## 5. Modules & code (MOD-*)

### 5.1 MOD-STORAGE
* Collection `users.code` holds `{user, branch, modules, timestamp, activeWorld, activeSim}` (`backend/lib/game/api/user.js:56-120`).
* Upload `POST /api/user/code {branch?, modules}` rejects if `JSON.stringify(modules).length > 5 MB` (`:84-86`). Branch `$activeWorld` (the default) targets the active branch. On success it sets `timestamp = Date.now()` and `users.active = 10000`, and publishes `user:<id>/code` (`:102-118`).
* A module value is either a JS source string or `{binary: <base64>}` (`engine/src/game/game.js:561`).
* There is a maximum of 30 branches (`user.js:227-228`). Branch names are at most 30 characters (`:211-213`).

### 5.2 MOD-NAMES
* Stored keys escape `.` → `$DOT$`, `/` → `$SLASH$`, `\` → `$BACKSLASH$`. Escaped names beginning with `$` are dropped, and `main` is forced to exist (`''` if missing) (`backend/lib/utils.js:62-86`). The runtime un-escapes them (`data.js:167-176`).
* So names like `role.harvester`, `lib/foo` and `main.js.map` are legal module names.

### 5.3 MOD-REQUIRE (`game.js:551-594`)
```
require(name):
  name = name.replace(/^\.\//, '')        // non-string → TypeError from .replace
  if !(name in require.cache):             // `in` → prototype chain of a plain {} object
     if codeModules[name] === undefined: throw Error(`Unknown module '${name}'`)
     if isObject(codeModules[name]) && codeModules[name].binary !== undefined:
         require.cache[name] = Buffer.from(base64)                       // MOD-BINARY
     else:
         require.cache[name] = -1                                         // in-progress sentinel
         mod = {exports:{}, user, timestamp, name, code}
         try evalCode(mod, globals, false, timeout) catch e: delete require.cache[name]; throw e
         require.cache[name] = mod.exports                                // value AFTER evaluation
  else if require.cache[name] === -1: throw Error(`Circular reference to module '${name}'`)
  return require.cache[name]
```
* The namespace is flat and independent of the requiring module. Only a leading `./` is stripped. `../x`, `x.js` and absolute paths are **not** normalized: `require('x.js')` loads a module literally named `x.js`.
* **GOTCHA**: circular requires throw (Node would return partial exports). `module.exports = -1` makes every later `require` of that module throw "Circular reference".
* **GOTCHA (quirk)**: `require('toString')`, `require('constructor')` and `require('hasOwnProperty')` return `Object.prototype` members, because `in` sees the prototype chain of `require.cache`. `require('__proto__')` returns `Object.prototype`.
* `require.cache` is a user-visible, mutable plain object. `delete require.cache.foo` forces re-evaluation, which some bots use for hot-reload.
* `require` is `Function.call.bind(Function.bind)(requireFn, runCodeCache[userId])`, a bound function (`game.js:10`, `:509`). Its `this` is the per-user state record, whose fields are updated every tick (`game.js:455-468`). A later-tick `require` therefore sees the current `codeModules` and `timeout`.

### 5.4 MOD-REQUIRE-LODASH
* `require.cache = {lodash: globals._}` whenever `require` is (re)created (`game.js:510`). A user module named `lodash` is therefore never loaded.

### 5.5 MOD-REQUIRE-ERRORS
* Unknown module: `Error("Unknown module 'x'")` (`:557-559`). Circular: `Error("Circular reference to module 'x'")` (`:590-591`). Both are real `Error` objects.
* Evaluation error: the cache entry is deleted and the evaluation error is rethrown. **GOTCHA**: that error is an `EvalCodeError` instance (`runtime-driver.js:4-6`). It has **no `message` and no `stack`**, only an own `toString()` returning the processed text. A bot that does `try { require('x') } catch(e) { console.log(e.message) }` prints `undefined`, and `e instanceof Error` is false.

### 5.6 MOD-WRAPPER (`runtime-driver.js:67`)
* Source text: `'(function __module(module,exports){ ' + code + '\n})(__module, __module.exports)'`, compiled as a classic script with `filename = module.name`.
* Consequences:
  * Parameters are `module` and `exports`. `require` is **the global** (no per-module `require`), and there is no `__filename`/`__dirname`.
  * Top-level `var`/function declarations are function-local. `this` at top level is `undefined` in strict mode or the global object in sloppy mode. `arguments` has length 2.
  * `'use strict'` at the top of a module becomes the wrapper function's directive, so it works per module.
  * **Line numbers match the source. Columns on line 1 are shifted by +36** (the prefix length). This matters for minified single-line bundles with source maps.
  * A trailing `//` comment on the last line is safe because the wrapper adds `\n` before `})`.
  * Stack frames for module top-level code are named `__module` and get rewritten by CON-ERR-FORMAT.

### 5.7 MOD-MODULE-OBJECT
* `module` = `{exports: {}, user: <userId>, timestamp, name, code: <full source>}` (`game.js:567-573`) plus `__initGlobals` (non-writable, non-configurable, value `undefined`) (`runtime-driver.js:27-31`). A module can read its own `module.name`.

### 5.8 MOD-SCRIPT-CACHE
* Compiled scripts are cached in `runtimeCache[user].modules[name] = {timestamp, script}` and reused while `timestamp` matches (`runtime-driver.js:64-94`). The cache is per VM (it lives in the bundle).
* **PERF**: re-evaluation after MOD-REQUIRE-RESET does not recompile. Console commands are never cached.

### 5.9 MOD-BINARY
* `require` returns `Buffer.from(base64,'base64')`. That is the webpack-bundled `buffer` polyfill (a `Uint8Array` with `Buffer.prototype`), cached like any module (`game.js:561-562`, `runtime-driver.js:13-15`).
* The docs comment calls it an ArrayBuffer (`docs/source/modules.md:82-83`), but the code returns a Uint8Array subclass, and `new WebAssembly.Module(buf)` accepts it.
* chitin: return a `Uint8Array` whose prototype chain includes a `Buffer.prototype`-like object. At minimum `instanceof Uint8Array`, `.buffer`/`.byteOffset`/`.length` must be correct.

### 5.10 MOD-WASM
* `WebAssembly.Module`, `Instance`, `Memory` and `Table` are required (`docs/source/modules.md:79-101`). Rust bots (screeps-game-api / wasm-bindgen) and C bots depend on them. **GOTCHA**: this is a hard requirement for chitin's own JS engine. See §16 for the proposal-level feature set.

### 5.11 MOD-MAIN-LOOP (`game.js:517-533`)
```
run(u):
  main = require('main')
  if isObject(main) && isFunction(main.loop):
      for f in require.initGlobals (always empty): f()
      evalCode({exports: main, name:'__mainLoop', code:'module.exports.loop();', timestamp}, globals, false, timeout)
  for each console command (see CON-COMMANDS)
```
* `loop` is looked up **every tick** from `main`'s exports (`module.exports.loop()` with `module = {exports: main}`). A bot can therefore swap `module.exports.loop` at runtime, as wasm loaders do.
* **GOTCHA**: `this` inside `loop` is main's `module.exports` object (method call).
* `require('main')` is a cache hit after the first tick.
* On the tick of a global reset, main's top-level code runs inside `require('main')` **within** the measured run (counted in getUsed, §8.4) and under its own timeout (MOD-TIMEOUT).

### 5.12 MOD-NO-LOOP / MOD-REQUIRE-RESET
A fresh `require` with an empty cache (except lodash) is created in `game.init` when **any** of these hold (`game.js:504-512`):
1. `globals.require` is missing (new VM).
2. `runtimeData.userCodeTimestamp != require.timestamp` (loose inequality; see VM-CODE-VERSION-RESET).
3. `require.cache.main` is not an object, e.g. main threw during evaluation and was un-cached, or exports a primitive.
4. `require.cache.main.loop` is not a function.

**GOTCHA**: case 4 is "legacy mode". A `main` without `loop` gets its whole module graph re-evaluated **every tick**, with top-level code acting as the loop. Case 3 means that if main throws at top level, **all** modules, including ones that loaded fine, are re-evaluated next tick in the same heap.

### 5.13 MOD-INITGLOBALS
* `evalCode` defines `module.__initGlobals` and `require.initGlobals` as non-writable, non-configurable `undefined` (`runtime-driver.js:27-36`). The legacy hook in `game.js:522-524,584-587` can therefore never fire. chitin needs only the descriptors.

### 5.14 MOD-TIMEOUT
* `evalCode(…, timeout)`: if `timeout` is defined, non-null, non-zero and not `Infinity`, then `options.timeout = max(30, timeout + 5)` ms (`runtime-driver.js:40-45`). `timeout` = `runtimeData.cpu` = tickLimit (`runtime.js:224`, `game.js:463`).
* Each `require` evaluation and the `__mainLoop` evaluation gets **its own** timer. Timers nest: an inner require started inside loop has its own timer while the loop's timer keeps running. Console commands get **no** timeout.
* The outer `run.apply(..., {timeout: data.timeout})` has `data.timeout === undefined` (`make.js:187`), so it is unbounded except by VM-HARD-TIMEOUT.
* Clock: each timer is a `steady_clock` deadline (wall time) armed at the start of that `runSync`, not at `startTime`. See CPU-CLOCK (§8.13).

---

## 6. Game object construction (GAME-*)

### 6.1 GAME-SHAPE (`game.js:136-182`)
`Game` is a plain object whose keys, in insertion order, are: `creeps, powerCreeps, spawns, structures, flags, constructionSites, rooms, time, cpuLimit, cpu, map, gcl, gpl, market, resources, getObjectById, notify`, and then `shard` (added by the sandbox hook). All are ordinary writable properties, and every one of them is rebuilt next tick.
`Game.cpu` = `{getUsed, tickLimit, limit, bucket, getHeapStatistics, halt}`, a plain writable object (`:146-157`). Bots commonly overwrite `Game.cpu.getUsed` for profiling, and that change lasts one tick.

### 6.2 GAME-TIME / GAME-SHARD / GAME-CPULIMIT-LEGACY / GAME-RESOURCES
* `Game.time` = `parseInt(env.gameTime)` read at data fetch. It is the same value the processor uses for this tick, and it is incremented after processing (`main.js:72-76`).
* `Game.shard` = `Object.create(null, {name: {value: os.hostname(), writable, enumerable}, type: {value:'normal', writable, enumerable}, ptr: {value:false, enumerable}})`, set on every tick by the `playerSandbox` listener (`driver/lib/index.js:35-52`). It is null-prototype, and `ptr` is non-writable.
  * **PS-QUIRK**: `name` = the runner's hostname. Official uses `shard0..shard3`. chitin should make it configurable (default `'shard0'`).
* `Game.cpuLimit` = tickLimit (`game.js:145`).
* `Game.resources` = `JSON.parse(JSON.stringify(user.resources || {}))` (`game.js:170`). The keys are resource constants such as `pixel`, `cpuUnlock`, `accessKey` and `token` (`common/lib/constants.js:379-382`).

### 6.3 GAME-GCL / GAME-GPL (`game.js:130-134,159-168`)
```
gcl = user.gcl || 0
gclLevel        = floor( (gcl / GCL_MULTIPLY) ** (1/GCL_POW) ) + 1        // GCL_MULTIPLY=1e6, GCL_POW=2.4
gclBase         = (gclLevel-1) ** GCL_POW * GCL_MULTIPLY
Game.gcl        = {level: gclLevel, progress: gcl - gclBase, progressTotal: gclLevel ** GCL_POW * GCL_MULTIPLY - gclBase}

power = user.power || 0
gplLevel        = floor( (power / POWER_LEVEL_MULTIPLY) ** (1/POWER_LEVEL_POW) )   // 1000, 2
gplBase         = gplLevel ** POWER_LEVEL_POW * POWER_LEVEL_MULTIPLY
Game.gpl        = {level: gplLevel, progress: power - gplBase, progressTotal: (gplLevel+1)**2 * 1000 - gplBase}
```
Constants: `common/lib/constants.js:284-285,808-809`. Note that gpl `progressTotal` hardcodes `2` and `1000`.
**GOTCHA**: this uses `Math.pow` on f64, and the floor at level boundaries depends on bit-exact `pow`. chitin must use a V8-identical pow (fdlibm `e_pow.c` port, as V8's `ieee754::pow`), not the platform libm.

### 6.4 GAME-CONSTRUCT — algorithm (`game.js:50-438`), in order
1. `customObjectsInfo` from mod prototypes (`:52-65`).
2. `register = {_useNewPathFinder:true, _objects:{}, byRoom:{}, findCache:{}, rooms:{}, roomEventLogCache:{}, wrapFn:identity}` + `populateRegister` (type maps: `creeps, structures, ownedStructures, spawns, sources, energy, flags, constructionSites, minerals, deposits, tombstones, nukes, powerCreeps, ruins, customObjects` + mod lookConstants) (`:67-111`).
3. gcl/gpl. Then the `game` literal.
4. `register.objectsByRoom[room][id] = raw` and `objectsByRoomKeys` for every non-`temp` raw object (`:184-194`).
5. For each visible room: `register.byRoom[room]` with **spatial arrays**, one `new Array(2500)` per type key (`:104-110`, `:196-199`).
6. Module `make`s in fixed order: rooms, makePos, creeps, structures, sources, minerals, deposits, nukes, resources, flags, tombstones, construction-sites, path-finder, power-creeps, ruins, store (`:201-216`). Each resets its per-tick module state (e.g. `createdFlagNames`, `privateStore`, `controllersClaimedInTick`), and defines classes only on the first tick of a VM.
7. `register.rooms[name] = new Room(name)` for each visible room, in `runtimeData.rooms` key order (`:218-220`). The Room constructor creates `room.visual = new RoomVisual(name)` and a per-room `privateStore` (`rooms.js:419-526`).
8. Custom prototypes `make` (`:222-224`).
9. `game.rooms = _.clone(register.rooms)`, a shallow copy taken *before* objects are constructed. The Room instances are shared (`:227`).
10. Power creeps: `register.powerCreeps[id] = new PowerCreep(id)`; `game.powerCreeps[name] = …` for **all** user power creeps (`:255-258`).
11. For each raw object in `roomObjects` insertion order (foreign objects in visible rooms, then own objects), skipping `temp` (`:260-395`):
    * `creep` → `new Creep(id)`, registered. If own: `Game.creeps[name]`, or `suicide()` if the name is already taken (GAME-DUP-NAME). Find caches `FIND_CREEPS`, `FIND_MY_CREEPS`, `FIND_HOSTILE_CREEPS`.
    * `powerCreep` → reuse the `register.powerCreeps[id]` instance if own, else `new PowerCreep(id)`; find caches.
    * Structure types (`rampart, road, extension, constructedWall, keeperLair, controller, link, storage, tower, observer, powerBank, powerSpawn, lab, extractor, terminal, container, spawn, nuker, portal, factory, invaderCore`, `:229-251`) → `new StructureX(id)`. If `instanceof OwnedStructure` → `ownedStructures`, and `Game.structures[id]` if own. Spawns go to `Game.spawns[name]`.
    * `!off && (extension|spawn) && user==me` → add `store.energy` / `storeCapacityResource.energy` to the room's `energyAvailable` / `energyCapacityAvailable` (`:325-328`).
    * `source, mineral, deposit, energy(→Resource), nuke, tombstone, ruin, constructionSite`, plus mod types. Own construction sites go to `Game.constructionSites[id]` even in invisible rooms, and a `byRoom` entry is created for that room. They are added to find caches only if the room is visible (`:366-382`).
    * Unknown types are silently skipped.
12. Flags: each `rooms.flags` doc holds `data = "name~color~secondaryColor~x~y|…"`, with `$VLINE$`/`$TILDE$` un-escaped in names. `id = 'flag_'+name`. They are added to `_objects`, `register.flags`, `byRoom` spatial (only if room known), `Game.flags[name]`, and the `FIND_FLAGS` cache (`:397-429`).
13. `Game.map = makeMap(...)`, `Game.market = market.make(...)` (`:431-433`).
14. Constants copied onto globals (`:435`).

Each raw → instance constructor runs `RoomObject.call(this, x, y, room, effects)`, which sets `this.room = register.rooms[room]` (undefined if invisible), `this.pos = new RoomPosition(...)`, and `this.effects = [...]` (mapped, `ticksRemaining = endTime − time`, filtered >0) when the raw object has effects (`rooms.js:1648-1659`). It then sets `this.id`. Structures also set `room.controller/storage/terminal` (`structures.js:45-55`).

**GOTCHA**: find-cache filters call instance getters during construction (`i.spawning`, `i.my`, `i.energy`, `i.owner`) (`game.js:15-28`, `:30-36`). Those values are therefore already memoized as own `_spawning`, `_my` and similar properties before user code runs, which is visible via `Object.keys(creep)`/`for…in`. Filter table:
| FIND const | filter |
|---|---|
| FIND_CREEPS | `!spawning` |
| FIND_MY_CREEPS | `!spawning && my` |
| FIND_HOSTILE_CREEPS | `!spawning && !my` |
| FIND_MY_POWER_CREEPS / FIND_HOSTILE_POWER_CREEPS | `my` / `!my` |
| FIND_MY_SPAWNS / FIND_HOSTILE_SPAWNS | `my === true` / `my === false` |
| FIND_SOURCES_ACTIVE | `energy > 0` |
| FIND_MY_STRUCTURES / FIND_HOSTILE_STRUCTURES | `my === true` / `my === false && owner` |
| FIND_MY_CONSTRUCTION_SITES / FIND_HOSTILE_CONSTRUCTION_SITES | `my` / `my === false` |
| all others (FIND_STRUCTURES, FIND_SOURCES, FIND_FLAGS, …) | no filter |

**PERF** (where the original is slow):
* Eager construction of every object in every visible room, per player, per tick.
* About 15 × `Array(2500)` spatial arrays per visible room per tick.
* Find caches.
* Room objects visible to N players are constructed N times.

Construction CPU is **not billed** in the reference: it happens in `_start`, before `startTime` (CPU-MEASURE).

chitin may materialize lazily, as long as the observable results are identical:
* the same instance identity within a tick;
* the same key order of `Game.*` hashes (insertion order as above);
* the `_x` memo side-effects of the find filters;
* `room.controller/storage/terminal` set once any structure in the room is constructed. Because construction is eager in the original, these are always set by the time user code runs.

### 6.5 GAME-LAZY-PROPS (`engine/src/utils.js:509-545`)
* For each property: an accessor on the prototype. `get()`: `if (!this['_'+name]) this['_'+name] = fn(data(this.id), this.id); return this['_'+name]`. `set` exists only with `opts.canSet`. Defaults: enumerable **true**, configurable **false**.
* **GOTCHA**: falsy results (`0`, `''`, `false`, `null`, `undefined`) are **recomputed on every access**, never memoized. `hits === 0` and `my === false` are always recomputed from current data.
* **GOTCHA**: the engine reserves own properties `_<name>`. A bot that writes `creep._hits = 5` changes what `creep.hits` returns, and `creep.store` returns a memoized `Store` instance.
* `prototype.toJSON` = `for (i in this)`, skipping keys that start with `_` and `toJSON`/`toString`. Room's version also skips `controller/storage/terminal` (`rooms.js:540-549`). This affects `JSON.stringify(creep)` and objects mistakenly stored in Memory.

### 6.6 GAME-IDENTITY
* Every tick produces brand-new instances: `Game.creeps.X` at tick T+1 `!==` at tick T. `Game.getObjectById(id)` returns the current tick's instance.
* **GOTCHA (very common bot pattern)**: instances cached in the heap across ticks keep working:
  * Getters not yet memoized resolve `data(this.id)` against the **current** `runtimeData` (the module-level variable is reassigned each tick, `creeps.js:39`), so they return fresh values.
  * Memoized `_x` values are **stale**.
  * `pos`, `room` and `effects` are own properties fixed at construction, so they are **stale**.
  * Methods (`oldCreep.move()`) use `this.id` and the module-level `intents`, so they queue intents into the **current** tick.
  * If the object no longer exists, getters throw `Error("Could not find an object with ID <id>")` (`creeps.js:54-56`).
  * chitin must reproduce this "id-indirect lazy getter + per-instance memo" model exactly.
* Prototypes persist for the whole VM lifetime, so `instanceof` works across ticks.
* IDs: the private server generates 15-hex-char ids (`storage/lib/db.js:317-328`), while official uses 24-hex Mongo ObjectIds (`docs/source/architecture.md:16`). `GAME-ASSERT-SERIALIZED` keys off `id.length == 24`, so chitin should generate 24-hex ids (cross-ref object-model spec).

### 6.7 GAME-GETOBJECTBYID (`game.js:171-173`)
* `register._objects[id] || null`. `_objects` is a plain `{}`.
* **GOTCHA**: `Game.getObjectById('toString')` returns `Object.prototype.toString`. `Game.getObjectById('flag_Flag1')` returns that flag, because flags are stored under `'flag_'+name`.
* Own construction sites in invisible rooms are found. Unspawned power creeps are **not** found, because they are only in `register.powerCreeps`.

### 6.8 GAME-DUP-NAME / GAME-DEPRECATED-LOG / GAME-ASSERT-SERIALIZED
* Two own creeps share a name → the one iterated later gets `register.creeps[id].suicide()`, which queues a suicide intent. `Game.creeps[name]` keeps the first (`game.js:273-279`).
* `register.deprecated(msg)` logs through `globals.console.log` once per message per tick (`game.js:113-120`). It is used by e.g. `Game.map.isRoomAvailable`, `getTerrainAt` and `PathFinder.use(false)` (`map.js:202,236`, `path-finder.js:71`).
* `register.assertTargetObject(obj)` throws `"It seems you're trying to use a serialized game object stored in Memory which is not allowed. Please use \`Game.getObjectById\` to retrieve a live object reference instead."` for plain objects with a 24-char string `id` (`game.js:122-126`).

### 6.9 GAME-CREEPS/SPAWNS/STRUCTURES/CONSTRUCTIONSITES/FLAGS/POWERCREEPS/ROOMS keys
| Hash | Key | Membership |
|---|---|---|
| Game.creeps | name | own creeps, **including spawning** ones |
| Game.powerCreeps | name | all own power creeps, spawned or not |
| Game.spawns | name | own spawns |
| Game.structures | id | own `OwnedStructure` instances (incl. controller) |
| Game.constructionSites | id | own sites, any room |
| Game.flags | name | own flags, any room (`flag.room` undefined if invisible) |
| Game.rooms | name | visible rooms (VIS-ROOMS) |

---

## 7. Visibility (VIS-*)

### 7.1 VIS-ROOMS (`data.js:113-127`)
`visible = { o.room | o ∈ userObjects, o.type ∉ {flag, constructionSite} } ∪ { o.observeRoom | o.type == 'observer' }`, intersected with rooms that exist in the `rooms` collection (`data.js:135`).
* "userObjects" = every `rooms.objects` doc with `user == me`: creeps, power creeps, owned structures (incl. the owned controller), etc. A **reserved** controller has no `user` (only `reservation.user`), so reservation alone gives **no** visibility.
* The docs summary is "a creep or an owned structure" (`docs/api/source/Game.md:137`).

### 7.2 VIS-OBSERVER
* The intent `observeRoom` at tick T is stored on the observer by the processor at T (`processor.js:284-288`, `:403-406`). Runtime data at T+1 includes that room. The processor resets `observeRoom = null` at the start of each room tick (`:116-118`).
* Net effect: visible **for exactly the next tick** per call (`docs/api/source/StructureObserver.md:42`).

### 7.3 VIS-TOMBSTONE (PS-QUIRK?)
* Creep and power-creep tombstones are created with `user: object.user` (`engine/src/processor/intents/creeps/_die.js:22-26`, `processor/global-intents/power/_diePowerCreep.js:9-13`). Because of the `user` field, they count as user objects, so a room stays visible while your tombstone exists, and the player stays `active`. Ruins keep the owner only in `structure.user` and do not.
* Whether official behaves the same is in §16.

### 7.4 VIS-OWN-OUTSIDE
* Own construction sites and flags in invisible rooms are exposed (`Game.constructionSites`, `Game.flags`) with `.room === undefined` (`docs/api/source/RoomObject.md:31`).

### 7.5 VIS-USERS
* `runtimeData.users` = the user themself, owners of objects in visible rooms, controller `reservation.user` and `sign.user`, and transaction counterparts (`data.js:101,123-125,180-198,224`). `owner.username` getters index this map, so a missing entry would throw.

**PERF**: visibility is per player and per tick. chitin should compute it from an index `user → rooms with own objects` plus observer targets, maintained incrementally by the processor, instead of a full scan.

---

## 8. CPU (CPU-*)

### 8.1 CPU-LIMIT / CPU-TICKLIMIT / CPU-BUCKET-VALUE (`data.js:151-165`)
```
if user.cpu:                                   // falsy → unlimited
    bucket   = max(0, user.cpuAvailable || 0)
    tickLimit= min(bucket + user.cpu, cpuMaxPerTick /*500*/)
else: bucket = tickLimit = Infinity
Game.cpu.limit = user.cpu ; Game.cpu.tickLimit = tickLimit ; Game.cpu.bucket = bucket ; Game.cpuLimit = tickLimit
```
* `Game.cpu.bucket` is the value **before** this tick's refill and spending. New users start with `cpuAvailable: 0` and `cpu: 100` (PS defaults, `backend/lib/game/api/auth.js:45-53`, `cli/bots.js:74-76`).
* Docs: "full bucket ⇒ tickLimit 500, never less than limit" (`docs/source/cpu-limit.md:20-24`).

### 8.2 CPU-GETUSED (`runtime.js:25-27,202-204,235`)
`getUsed() = isolate.cpuTime_ms − startTime + intents.cpu`
* `startTime` is taken when the run reference is invoked, i.e. **after** Game construction and the `playerSandbox` hook. Calling it before then yields `NaN`.
* It returns an unrounded float. `intents.cpu` accumulates by repeated `+= 0.2` (f64), so it shows drift like `0.6000000000000001`.
* Despite the name, `isolate.cpuTime` advances at **wall-clock rate** during a run on every OS, including Linux (CPU-CLOCK, §8.13).

### 8.3 CPU-INTENT-COST (`runtime.js:60-99`)
* `freeMethods = {say, pull}` (null-proto).
* `set(id, name, data)` adds 0.2 **only if** the name is not free **and** there is no existing `list[id][name]` (replacing is free). `push`/`pushByName` add 0.2 per successful push and do not check free methods. A `remove` that succeeds subtracts 0.2.

### 8.4 CPU-MEASURE — what is counted (reference)
Counted (the `isolate.cpuTime` delta between `startTime` and the end-of-tick measurement; this is wall time, see CPU-CLOCK):
* `require('main')` and module evaluation on reset ticks;
* the loop;
* lazy Memory parse;
* console commands;
* GC that happens during the run;
* **end-of-tick `JSON.stringify(Memory)` and segment validation** (these happen before `usedTime` is measured, `runtime.js:246-273`), which are **not visible to `getUsed()`** inside the loop.

Not counted: data fetch, VM creation and boot, Game object construction, the `playerSandbox` hook, intent sanitization, DB writes.
* `usedDirtyTime = ceil(now − startDirtyTime)` includes construction (`runtime.js:59,273`). It is only used for scheduling order.
* **chitin note**: CPU time comes from chitin's own engine and cannot match V8 numerically. Provide a calibration knob (scale factor) and optionally a deterministic "op-count" CPU mode for parity testing. The *rules* above (what counts, rounding, intent costs) are the 1:1 contract.

### 8.5 CPU-BILLING / CPU-BUCKET-UPDATE (`make.js:193-212`)
```
finalCount = Σ over intentsList entities, Σ over names ∉ {say,pull}: (isArray(v) ? v.length : 1)   // make.js:98-111
cpuUsed    = ceil(usedCleanTime + 0.2*finalCount)          // NOTE 0.2*n, not the running sum
lastUsedCpu = usedTime = ceil(usedCleanTime + intents.cpu) // reported value
if user.cpu finite: cpuAvailable = min(cpuBucketSize /*10000*/, cpuAvailable + user.cpu − cpuUsed)
```
* `notify` is an array entity, so each notification counts 1 via index iteration. Room/global entities count array lengths.
* The bucket **can go negative** when `cpuUsed > bucket + limit`. That is possible because the timeout has slack (`+5`, minimum 30, termination latency; CPU-TIMEOUT), Memory stringify runs after the loop, and intent costs are not time-enforced. A timeout tick always does this when `bucket + limit ≤ 500` (CPU-TIMEOUT-BILL, §8.13).
* Billing is **skipped entirely** when `run` rejects (MEM-SERIALIZE-THROW, SEG-SAVE-ERRORS, halt, dispose, hard timeout). Those ticks are free (§13.2).

### 8.6 CPU-BUCKET-EMPTY (`make.js:72-83`)
* At tick start, if `cpuAvailable < 0`, the code does not run.
* `cpuAvailable := (cpuAvailable < −2·cpu) ? −2·cpu : cpuAvailable + cpu`; `lastUsedCpu = lastUsedDirtyTime = 0`.
* The console error is `'Script execution has been terminated: CPU bucket is empty'`.
* Console commands are **not** consumed (data fetch never happens).

### 8.7 CPU-PENALTY-SKIP (`make.js:61-71`)
* `skipTicksPenalty > 0` → decrement, skip, and error `'Your script is temporary blocked due to a hard reset inflicted to the runtime process.\nPlease try to change your code in order to prevent causing hard timeout resets.'`. Nothing in the reference sets it (dormant; official sets it after hard resets). chitin: set it N ticks after VM-HARD-TIMEOUT (N configurable, default 0 → matches PS).

### 8.8 CPU-TIMEOUT
* Enforced only via per-evaluation timeouts `max(30, tickLimit+5)` ms (MOD-TIMEOUT). isolated-vm timeouts are wall-clock timers, and so is `getUsed()` (CPU-CLOCK, §8.13). A player whose tickLimit is 20 therefore gets 30 ms per evaluation.
* On a reset tick, main's top-level and `loop` are separately bounded, so the total can reach about 2× the limit. Console commands are unbounded (up to the 5 s hard timeout).
* Message: isolated-vm's `'Script execution timed out.'` is rewritten to `'Script execution timed out: CPU time limit reached'` (`runtime-driver.js:105-107`) and then goes through CON-ERR-FORMAT.
* **GOTCHA**: in the reference, a timeout behaves like any other exception. **Memory changes made before the timeout are saved, intents queued before it are submitted, and the tick is billed** (§13.2).
* `make.js:262` tests `/CPU limit reached/`, which does not match the actual text "CPU time limit reached". That is dead code, affecting only the UI CPU graph.

### 8.9 CPU-HEAPSTATS / CPU-HALT / CPU-REPORT
* `getHeapStatistics()` returns `isolate.getHeapStatisticsSync()` with keys `total_heap_size, total_heap_size_executable, total_physical_size, total_available_size, used_heap_size, heap_size_limit, malloced_memory, peak_malloced_memory, does_zap_garbage, externally_allocated_size` (`docs/api/source/Game.md:198-213`). chitin should report its own numbers in the same shape, with `heap_size_limit` consistent with VM-HEAP-LIMIT.
* `halt()`:
  1. Calls the host reference: `vm.didHaltByUserRequest = true; isolate.dispose()` (`user-vm.js:68-71`).
  2. Then throws `Error("No one should ever see this message.")` (`runtime.js:20-23`).
  3. `make` sees `didHaltByUserRequest`, clears the VM, and returns error `"CPU halted"` (`make.js:281-284`).
  4. **GOTCHA**: the whole tick's Memory, intents and segments are discarded, and the next tick is a global reset.
* Reporting: `users.lastUsedCpu = usedTime` and `lastUsedDirtyTime` are set, and pubsub `user:<id>/cpu` publishes `{cpu: usedTime, memory: memoryString.length}` (`make.js:196-198,262-273`).

### 8.10 CPU-SHARDLIMITS / CPU-SETSHARDLIMITS (Doc; official only)
* `Game.cpu.shardLimits: {[shard]: number}`. `setShardLimits(limits)` returns `OK`, `ERR_BUSY` (12 h cooldown not over) or `ERR_INVALID_ARGS` (not a valid object, or total ≠ current total) (`Game.md:255-277`).
* chitin single-shard default: `shardLimits = {[Game.shard.name]: Game.cpu.limit}`. The validation details are in §16.

### 8.11 CPU-UNLOCKED / CPU-UNLOCK (Doc)
* `unlocked: boolean`. `unlockedTime`: ms epoch, **undefined** when not unlocked or unlocked by subscription.
* `unlock()` consumes 1 `cpuUnlock` from `Game.resources` and returns `OK`, `ERR_FULL` (subscription) or `ERR_NOT_ENOUGH_RESOURCES` (`Game.md:280-298`). It takes effect asynchronously (up to 5 minutes).

### 8.12 CPU-GENERATEPIXEL (Doc)
* `generatePixel()` returns `ERR_NOT_ENOUGH_RESOURCES` if the bucket is below `PIXEL_CPU_COST` (10000) (`common/lib/constants.js:384`). Otherwise it returns `OK` and is "scheduled": 10000 is removed from the bucket and +1 `pixel` is added to `Game.resources` (`Game.md:300-316`). Exact timing and repeat-call behaviour are in §16.
* **GOTCHA**: very common unguarded call in bots (`if (Game.cpu.bucket == 10000) Game.cpu.generatePixel()`). It must exist.

### 8.13 CPU-CLOCK / CPU-TIMEOUT-BILL: which clock the timeout and getUsed() use
This answers 01 §20 Q3 and the measurement side of 14 OQ-8. All `ivm/` citations are isolated-vm at the pinned commit (§0).

**Timeout clock (MOD-TIMEOUT).**
* `RunWithTimeout` arms a `timer_t` with deadline `steady_clock::now() + timeout_ms` before it calls `fn()` (`ivm/src/isolate/run_with_timeout.h:66-77,139`, `ivm/src/lib/timer.cc:176-180`). A timer thread does `sleep_until(deadline)` (`timer.cc:97`). This is **wall time**.
* The deadline is pushed back only while the isolate's `CpuTimer` is paused: `CpuTimer::Pause/Resume` call `timer_t::pause/resume(env.timer_holder)`, and `adjust()` adds the paused duration (`ivm/src/isolate/executor.cc:75-92`, `timer.cc:29-50,206-221`).
* When the timer fires, it queues a `TimeoutRunner` interrupt, then calls `TerminateExecution()` in a loop until `fn` returns (`run_with_timeout.h:78-134`). The caller then gets `RuntimeGenericError("Script execution timed out.")` (`:166`).
* The driver sets no `onCatastrophicError`, so isolated-vm's own 5 s "Script failed to terminate" abort is disabled (`run_with_timeout.h:108-113`; no such option in `driver/lib/runtime/user-vm.js:30`). VM-HARD-TIMEOUT is the only backstop.
* Termination unwinds the player's frames as V8's "uncatchable exception", so JS `catch`/`finally` blocks do not run (node headers `v8-isolate.h:1216-1245`). isolated-vm then cancels termination at the `runSync` boundary (`run_with_timeout.h:163-166`). Player code therefore cannot read `getUsed()` after the loop's own timer fires.

**getUsed()/billing clock.**
* `nowCpuTime() = Number(isolate.cpuTime)/1e6` (`runtime.js:25-27`). Every CPU number reads it:
  * `startTime` (`:235`);
  * `getUsed()` (`:202-204`);
  * `usedTime` and `usedCleanTime`, the billed value (`:271-272`);
  * `usedDirtyTime` (`:59,273`).
* `isolate.cpuTime` = `executor.cpu_time` + `cpu_timer->Delta()` (`ivm/src/module/isolate_handle.cc:387-394`, `ivm/src/isolate/environment.cc:529-536`):
  * `executor.cpu_time` is the sum over **finished** `CpuTimer` segments, each `Now() − time` (`executor.cc:59-65,75-81`).
  * `cpu_timer->Delta()` covers the **running** segment.
* `CpuTimer::Now()` is `clock_gettime(CLOCK_THREAD_CPUTIME_ID)` on Linux, because `binding.gyp:68` defines `USE_CLOCK_THREAD_CPUTIME_ID` for `OS == "linux"`. Elsewhere it is `steady_clock` (`executor.cc:94-106`).
* **But** `CpuTimer::Delta()` on Linux returns `steady_clock::now() − steady_time`, which is wall time since the segment started or last resumed (`executor.cc:67-73`, `steady_time` set at `:47-52,83-89`). Elsewhere it is also steady time.
* A segment is one `Executor::Lock`, whose constructor creates the `CpuTimer` (`executor.cc:184-191`). `run.apply` (`make.js:187`) takes one Lock for the whole run.
* Nested `compileScriptSync`/`runSync` on the **same** isolate (`runtime-driver.js:60,69,94`) take the same-isolate shortcut with no new Lock (`ivm/src/isolate/three_phase_task.cc:214-220`).
* Only a synchronous call into **another** isolate pauses the segment: `Executor::Unlock` (`three_phase_task.cc:348`, `executor.cc:196`) or a nested Lock's `WallTimer` (`executor.cc:111-125`). In the runtime the only such call is `halt.applySync()` (`runtime.js:21`), which disposes the VM anyway.

**Result.** `startTime`, every `getUsed()` and `usedCleanTime` are all read inside one running segment. Their differences are therefore **steady-clock wall time on every OS, Linux included**. This is the same clock as the timeout. Thread-CPU time enters `isolate.cpuTime` only at segment boundaries, and those cancel out of `end − startTime`.

`usedDirtyTime` is the exception. It spans the `_start` segment (`make.js:160`) and the `run` segment, so on Linux it mixes `_start`'s thread CPU with wall time. It is used only for scheduling order.

* **GOTCHA** (corrects an earlier hypothesis). It is **false**, for the pinned build, that on Linux `getUsed()` is thread CPU, so that the timeout can fire while `getUsed() < tickLimit` on an oversubscribed host. Under oversubscription (`RUNNER_THREADS` isolates per runner plus processors on the same cores), time the thread spends descheduled inflates the timer, `getUsed()` and billing **equally**. CPU numbers depend on host load, and the bucket drains faster on a busy server without any code change.
  * The 5 ms (`+5`) of slack, or `30 − tickLimit` when `tickLimit < 25`, is the only gap between "getUsed() says over" and "killed".
  * The timer starts at that evaluation's `runSync` entry, which is **after** `startTime` (`runtime.js:235`, then `game.run`). On a reset tick it is also after main's top-level code, which has its own timer. So at the moment of firing, `getUsed() − intents.cpu ≥ timeout_ms` always holds.
* **CPU-TIMEOUT-BILL / GOTCHA**: a tick that ends in the loop timeout is billed `cpuUsed = ceil(usedCleanTime + 0.2·n)` with `usedCleanTime > max(30, tickLimit+5)`. The billed time also includes termination latency and the post-loop `JSON.stringify(Memory)` (`runtime.js:246-273`, `make.js:193-194`).
  * Case `bucket + limit ≤ cpuMaxPerTick` (500): then `tickLimit = bucket + limit`, so the new `cpuAvailable = bucket + limit − cpuUsed ≤ −5`. The next tick is skipped with `'Script execution has been terminated: CPU bucket is empty'` and a partial refill (CPU-BUCKET-EMPTY, `make.js:72-83,206-212`).
  * Example: a new user with `cpu = 20` and bucket 0 who times out ends at `cpuAvailable ≤ −10`.
* **chitin decision (for 14 OQ-8)**:
  * The 1:1 contract is a **single clock**. Per-evaluation enforcement at `max(30, tickLimit+5)`, `getUsed()` and `usedCleanTime` must all read the same meter, armed at the same points as above: the timer at each evaluation's entry, `getUsed` from `startTime`.
  * Which meter it is (wall, thread CPU, or a calibrated op count, §8.4 chitin note) is a server setting.
  * Recommended default: a load-independent meter (thread CPU or op count). At chitin's scale, wall time would bill players for scheduler contention that grows with player count.
  * A `wall` mode reproduces the PS exactly, including the load sensitivity.
  * Do **not** mix clocks, for example timing out on wall time while billing on CPU. That creates exactly the mismatch the PS does not have.

---

## 9. Memory (MEM-*)

### 9.1 MEM-GETTER (`game.js:479-500`)
Each tick in `game.init`:
```
defineProperty(global, 'Memory', {configurable:true, enumerable:true, get(){
    try { p = JSON.parse(RawMemory.get() || "{}"); p.__proto__ = null }   // current global JSON.parse
    catch { p = null }
    RawMemory._parsed = p
    defineProperty(global, 'Memory', {configurable:true, enumerable:true, value:p})   // writable:false
    return p }})
```
* The parse happens on the first access in the tick. That access may be indirect, through `creep.memory`, `room.memory`, `spawn.createCreep(...)` and so on, which all read `globals.Memory`.

### 9.2 MEM-DESCRIPTORS
* Before access, `Memory` is an accessor with no setter. After access, it is a data property with `writable:false, configurable:true`. **GOTCHA**: `Memory = {...}` is silently ignored in sloppy mode and throws `TypeError` in strict mode. The working pattern is `delete global.Memory; global.Memory = x`, which creates a writable property; next tick's `defineProperty` replaces it with the accessor again.

### 9.3 MEM-ROOT-NULLPROTO
* The root object has a `null` prototype (`p.__proto__ = null`). **GOTCHA**: `Memory.hasOwnProperty` is `undefined`, `Memory.toString` is undefined, and `'' + Memory` throws. Nested objects are normal.

### 9.4 MEM-PARSE-FAIL
* `JSON.parse` throws → `Memory === null`, `_parsed = null`. At end of tick `if (rawMemory._parsed)` is false, so the stored string is kept unchanged.
* **GOTCHA**: corrupted Memory stays corrupted and `Memory` is `null` on every tick. Bots crash on `Memory.creeps` until something calls `RawMemory.set(valid)`.

### 9.5 MEM-NONOBJECT (from `game.js:485-486`)
| Stored string | `Memory` | Re-serialized at end? |
|---|---|---|
| `""` | `{}` (null-proto) | yes |
| `"null"` | `null` (`null.__proto__=` throws → catch) | no |
| `"5"`, `"true"`, `"\"s\""` | the primitive (`__proto__` set on a primitive is a no-op) | only if truthy |
| `"0"`, `"false"` | the primitive | no (falsy) |
| `"[...]"` | Array with **null prototype** (no array methods) | yes |

### 9.6 MEM-SERIALIZE / MEM-SERIALIZE-THROW (`runtime.js:246-248`)
* After the loop and console commands, regardless of whether they threw: `if (rawMemory._parsed) userMemory.data = JSON.stringify(rawMemory._parsed)`, using the frozen original stringify.
* This is **outside** the try/catch. **GOTCHA**: a cyclic Memory, a `BigInt`, or a throwing `toJSON`/getter rejects the whole run. The console shows that error's stack, and nothing is saved: no Memory, no segments, no intents, no activeSegments changes. CPU is not billed (§13.2).
* JSON semantics apply: `undefined`/functions dropped, `NaN`/`Infinity` → `null`, `toJSON` honored (game objects stored in Memory serialize via GAME-LAZY-PROPS `toJSON`).

### 9.7 MEM-RAW-OBJECT (`runtime.js:102-189`)
`RawMemory = Object.create(null, {get, set, segments, interShardSegment, setActiveSegments, setPublicSegments, setDefaultPublicSegment, setActiveForeignSegment})`.
* The methods and `segments` are **non-writable, non-enumerable, non-configurable**. `interShardSegment` is writable but non-enumerable. `foreignSegment` (when present) and `_parsed` are plain assignable properties.
* `Object.keys(RawMemory)` is empty unless `_parsed`/`foreignSegment` are set.
* **GOTCHA**: `RawMemory.segments = {}` fails (silently or with TypeError), so segments must be mutated in place.

### 9.8 MEM-RAW-GET / MEM-RAW-SET
* `get()` returns `data.userMemory.data`: the string loaded at tick start, or the last value passed to `set`. **GOTCHA**: it does **not** reflect in-tick changes to `Memory`.
* `set(value)` validates in this order:
  1. `!_.isString(value)` → `Error('Raw memory value is not a string')`.
  2. `value.length > 2*1024*1024` → `Error('Raw memory length exceeded 2 MB limit')`.

  It then `delete this._parsed` and sets the string (`runtime.js:108-121`).
* **GOTCHA**: if `Memory` was already accessed, the global `Memory` stays bound to the old object. Writes to it (including via `creep.memory`) are **lost**, because `_parsed` is gone and the set string wins. If `Memory` is first accessed **after** `set`, it parses the new string, and that object is what gets saved.

### 9.9 MEM-PARSED-HACK ("memhack") — must work exactly
```js
let cached;  // module scope
module.exports.loop = function () {
  if (cached) { delete global.Memory; global.Memory = cached; RawMemory._parsed = cached; }
  else { cached = Memory; }   // first access parses, sets RawMemory._parsed
  ...
}
```
* This relies on: `Memory` being configurable (MEM-DESCRIPTORS); `RawMemory` being extensible with a writable `_parsed`; end-of-tick serialization reading `RawMemory._parsed` (whatever object it is); engine accessors resolving through `globals.Memory`; and `RawMemory` being a new object each tick (so `_parsed` starts `undefined`).
* xxscreeps documents and emulates the same contract (`xxscreeps/packages/xxscreeps/mods/memory/memory.ts:210-252`).

### 9.10 MEM-SIZE-LIMIT
* `RawMemory.set` enforces 2 MB **in UTF-16 code units** (`.length`).
* At save: `memory.data.length > 2*1024*1024` → `saveUserMemory` rejects with `'Script execution has been terminated: memory allocation limit reached'` (`driver/lib/index.js:153-160`). In the reference that rejection only reaches the **server log** (`runner.js:45`, `:73`).
* **PS-QUIRK / GOTCHA**: the oversize Memory is silently not saved. The previous Memory stays, intents are still saved, and CPU is still billed. Official behaviour is in §16. Recommendation: replicate the save-skip and also emit the message as a console error.

### 9.11 MEM-OBJ-ACCESSORS (`creeps.js:95-120`, `power-creeps.js:75-99`, `structures.js:865-891`, `rooms.js:551-571`, `flags.js:31-51`)
Getter for collection `K` ∈ {creeps, powerCreeps, spawns, rooms, flags} and key = name (spawns use `data(id).name`):
1. creeps/powerCreeps/spawns only: `if (this.id && !this.my) return undefined`.
2. `if (Memory.K === undefined || Memory.K === 'undefined') Memory.K = {}`. **GOTCHA**: the literal string `'undefined'` is treated as missing.
3. `if (!_.isObject(Memory.K)) return undefined`.
4. `return Memory.K[key] = Memory.K[key] || {}`. Every read writes. A falsy stored value (`0`, `''`, `false`, `null`) is replaced by `{}`.

Setter:
* Ownership check throws `"Could not set other player's creep memory"` or `"Could not set other player's spawn memory"`.
* Then step 2, then `!isObject` throws `'Could not set creep memory'` (or spawn/room/flag).
* Then `Memory.K[key] = value`.

The accessors are defined with `Object.defineProperty(proto, 'memory', {get, set})`, so they are non-enumerable and non-configurable.

### 9.12 MEM-BACKEND-EDIT
* The UI Memory editor inserts **hidden console commands** (`Memory['a']['b'] = <json>`, `delete …`, or replace-all `for(var i in Memory) delete Memory[i]; _.extend(Memory, …)`), with a 1 MB value cap (`backend/lib/game/api/user.js:285-316`). They are subject to CON-COMMAND-LOSS.

---

## 10. Segments & inter-shard (SEG-*, ISM-*)

### 10.1 SEG-OBJECT / SEG-LOAD
* `RawMemory.segments` is a fresh null-proto object every tick, pre-filled with each active segment id from the **previous** `setActiveSegments` (stored in `users.activeSegments`). Values are strings (`""` if never written) (`runtime.js:191-195`, `data.js:228-230,248-253`).
* Integer-like keys enumerate in ascending numeric order.

### 10.2 SEG-SET-ACTIVE (`runtime.js:129-147`)
Validation order:
1. `!_.isArray(ids)` → ``Error(`"${ids}" is not an array`)``.
2. `ids.length > 10` → `Error('Only 10 memory segments can be active at the same time')`.
3. For each `id = parseInt(ids[i])`, if NaN, >99 or <0 → ``Error(`"${ids[i]}" is not a valid segment ID`)`` (the original value is quoted).

Notes:
* The last call in a tick wins. Duplicates are allowed. `parseInt` means `"3abc"` → 3 and `3.9` → 3.
* The list is persisted after the run (`make.js:200-202`) and takes effect **next tick**. If it is never called, the previous list persists indefinitely. `setActiveSegments([])` clears it.

### 10.3 SEG-SAVE / SEG-SAVE-ERRORS (`runtime.js:250-269`)
After Memory serialization, with `keys = Object.keys(RawMemory.segments)`:
1. `keys.length > 10` → **throw string** `'Cannot save more than 10 memory segments on the same tick'`.
2. For each key in order:
   * `parseInt(key)` NaN or out of 0..99 → throw ``"`"${key}" is not a valid memory segment ID`"``.
   * value not a string → throw ``"Memory segment #${key} is not a string"``.
   * `length > 100*1024` → throw ``"Memory segment #${key} has exceeded 100 KB length limit"``.
   * Otherwise `out[parseInt(key)] = "" + value`.
* **GOTCHA**: **every present key is saved every tick**, including unmodified preloaded ones. With 10 active segments, writing an 11th id throws unless the bot deletes a preloaded key. A key like `"5abc"` saves as segment 5. Deleting a key means "don't save", not "erase".
* **GOTCHA**: these throws happen outside the try/catch, so the entire tick output is lost (Memory, intents, segments, activeSegments, CPU billing). The thrown string becomes the console error.
* Persistence: `env.hmset(memorySegments:<user>, segments)` (`driver/lib/index.js:162-168`). **PERF**: the reference rewrites up to 10×100 KB per user per tick. chitin may dirty-track, since that is not observable.

### 10.4 SEG-PUBLIC / SEG-DEFAULT-PUBLIC
* `setPublicSegments(ids)`: array check, then a per-id parseInt check as in 10.2. **No count limit**. The list is stored as the comma-joined string `publicMemorySegments:<user>` (`runtime.js:148-162`, `make.js:249-251`). The last call wins, and the list persists.
* `setDefaultPublicSegment(id)`: `null` is allowed and clears it. Otherwise parseInt validation, with the error ``"${parsedId}" is not a valid segment ID`` (it prints `"NaN"` for garbage). The value is persisted to `users.defaultPublicSegment` (`runtime.js:163-173`, `make.js:203-205`).

### 10.5 SEG-FOREIGN-SET / RESOLVE / LOAD
* `setActiveForeignSegment(username, id?)`: `username === null` → clear (`$unset`). If `id !== undefined`, validate it with parseInt (as in 10.4). It records `{username, id}` (`runtime.js:174-188`).
* Server resolve (`make.js:216-247`):
  * If the stored request has the same `username` **and** the new `id` is truthy → merge the id.
  * Otherwise look the user up by username, then set `user_id` and `id ||= target.defaultPublicSegment`.
  * Unknown username → stored without `user_id` → never loads.
* Load next tick (`data.js:231-238,254-260`): this requires `user_id && id` to be truthy, **and** the target's public list (split on `,`) must contain `""+id`. Then `RawMemory.foreignSegment = Object.assign(Object.create(null), {username, id, data})`, where `data` may be `null` if the target never wrote it. Otherwise `RawMemory.foreignSegment` is **absent** (`undefined`).
* **GOTCHA (PS)**: segment id **0** is treated as "no id" (falsy) and can never be loaded as a foreign segment unless it is the target's default. The default id is resolved once at request time and is not re-resolved if the owner later changes it. Data is re-read every tick while the request stands.

### 10.6 SEG-INTERSHARD-LEGACY
* `RawMemory.interShardSegment` = `''`, writable. The reference never persists it (not in `outMessage`, and the runner's `saveUserMemoryInterShardSegment` does not exist) (`runtime.js:125-128`, `runner.js:39-41`).
* Docs: deprecated, 100 KB, shared across shards (`RawMemory.md:49-72`). chitin: persist it per user (100 KB) for official parity, or leave it inert. See §16.

### 10.7 SEG-BACKEND-API
* `GET/POST /api/user/memory-segment` validates the id 0..99 and caps writes at 100 KB (`backend/lib/game/api/user.js:318-339`). A write is visible next time the segment is loaded.

### 10.8 ISM-API / ISM-LIMITS (Doc)
* `InterShardMemory.getLocal()` returns the current shard's string. `setLocal(value)` replaces it. `getRemote(shard)` returns another shard's string (read-only). Each is 100 KB, string only (`docs/api/source/InterShardMemory.md:1-45`).
* chitin single-shard: `getRemote(own or unknown)` behaviour, error texts, `null` vs `''` for empty, and the size-limit error are in §16. The data is separate from Memory.

---

## 11. Console, errors, visuals (CON-*)

### 11.1 CON-OBJECT
* `console = Object.create(null, {...})`, made fresh each tick in `_start` (`runtime.js:222`), with the buffers reset (`console.js:6-9`).
* `log`/`logUnsafe` are writable and configurable. `commandResult/addVisual/getVisualSize/clearVisual/getVisual` are non-writable and non-configurable. **All are non-enumerable.**
* **GOTCHA**: patches to `console.log` made at module load disappear after the first tick. The global `console` is writable, but replacing it with an object lacking `addVisual` breaks RoomVisual/MapVisual, which call `globals.console.addVisual` (`rooms.js:1156`, `map.js:286`). Old console objects still write into the current tick's buffer, because the buffer is looked up by user id per call.

### 11.2 CON-LOG-FORMAT (`console.js:20-27`)
For each argument `i`, joined with `' '`:
1. `if (i && i.toString) → i.toString()`: objects give `"[object Object]"` (not JSON), arrays give `"1,2"`, functions give their source, creeps give `"[creep Bob]"`. A throwing or non-function `toString` throws out of `console.log`.
2. Else `typeof i === 'undefined'` → `'undefined'`.
3. Else `JSON.stringify(i)`: `null` → `null`, `0` → `0`, `false` → `false`, **`''` → `""`** (two quote characters), **`NaN` → `null`**, a null-proto object without toString → JSON, `0n` → throws TypeError.

Each message is stored as `{message, escape: true}` (or `false` for `logUnsafe`).

### 11.3 CON-LOG-SELF-HOOK (`console.js:16-18`)
* `if (typeof self != 'undefined' && self.navigator.userAgent) self.console.log.apply(console, arguments)`. **GOTCHA**: if a bot defines a global `self` (wasm-bindgen glue sometimes does) without `navigator`, **every `console.log` throws** `TypeError` reading `userAgent`. chitin must evaluate this same check against the player global.

### 11.4 CON-LOG-TRANSPORT (`driver/lib/index.js:354-369`)
* Users `'2'` (Invader) and `'3'` (Source Keeper) go to the server log only. Otherwise messages with `escape` are run through `he.encode` (HTML-escapes `&<>"'\`` and non-ASCII) and published on `user:<id>/console` as `{messages: {log: [...], results: [...]}, userId}`. Server-side only, so not bot-observable.

### 11.5 CON-LIMITS
* The reference has no per-tick cap on the number or size of log messages (none in engine, driver or backend socket). Official limits are in §16. **PERF**: chitin should cap them (configurable) to protect the server, defaulting high.

### 11.6 CON-COMMANDS (`game.js:535-547`, `runtime-driver.js:58-61`)
* Commands come from `users.console` (fetched and **deleted** at data fetch, `data.js:149`). The UI limits an expression to 1 KB (`backend/lib/game/api/user.js:341-351`).
* They run after the loop, in DB order, each through `evalCode` with `returnValue=true`:
  `(function(code,module,exports) { return "" + eval(code); })(<JSON-quoted expr>, __module, __module.exports)`. The filename is `'_console' + Date.now() + '_' + i`. They are not cached and have **no timeout**.
* Semantics, because this is a sloppy-mode direct eval inside a function:
  * `var x = 1` is local to that invocation and **does not create a global**. Use `global.x = 1` instead.
  * The names `code`, `module` and `exports` are in scope. `this` is the global object.
  * The result is `"" + value`: objects give `[object Object]`, and a Symbol throws.
* If not `hidden`, `console.commandResult(String(result))` adds it to `results`.

### 11.7 CON-COMMAND-LOSS
* If `require('main')` or `loop` throws, `game.run` exits before the command loop. **All of that tick's commands are lost**, since they were already deleted from the DB.
* If a command throws, the remaining commands are skipped and lost, and the error is reported as the tick error.

### 11.8 CON-ERR-PIPELINE
* `_start`'s run function catches any throw from `game.run` and sets `outMessage.error = (isObject(e) && e.stack) || e.toString()` (`runtime.js:240-244`). Execution then **continues** to serialization (§13).
* The runner calls `sendConsoleError(u, error)` (`driver/lib/index.js:371-410`), which:
  * returns early on a falsy error;
  * sends NPC users to the server log;
  * otherwise does `error.toString()`;
  * upserts a `users.notifications` row of type `'error'`, grouped by `ceil(now/interval)*interval`, where `interval = notifyPrefs.errorsInterval min` (default 30). A `0` setting is falsy, so it also becomes 30;
  * publishes `{userId, error}` on `user:<id>/console`.
* Only one error per tick is reported, because the first uncaught throw ends the run.

### 11.9 CON-ERR-FORMAT (`runtime-driver.js:101-121`)
For an error caught by `evalCode` (module evaluation, loop, console command):
1. `e instanceof EvalCodeError` → rethrow unchanged (nested evaluations are processed once).
2. `e.message === 'Script execution timed out.'` → set `e.message = 'Script execution timed out: CPU time limit reached'`.
3. If `e.stack` is truthy, start from `msg = e.stack` and apply in order:
   * `replace(/</g, '&lt;')`;
   * `replace(/ *at.*?$/, '')`: no flags, so `$` is end of input. This removes the **first `at…` run on the last line**, which normally strips the final stack-frame line and leaves a trailing `\n`. On a single-line stack it truncates after the first `at` substring (e.g. `"Error: cannot attack"` → `"Error: cannot"`);
   * `replace(/_console\d+:\d+/, 'console')`: this never matches the current `_console<ms>_<i>` names (an older naming scheme);
   * `replace(/at __module \((.*)\)/g, 'at $1')`: module top-level frames become `at main:12:5`.

   Else `msg = e.message`.
4. `throw new EvalCodeError(msg)`, an object with only `toString() → msg`.

Typical loop error text, assuming V8 frames and the default `Error.stackTraceLimit=10`:
```
TypeError: x is undefined
    at Object.loop (main:12:15)        // user frames; file = module name
    at __mainLoop:1:52                 // was "at __module (__mainLoop:1:52)"
    at __mainLoop:2:3                  // IIFE call line
    ...runtime-internal frames, minus the last line...
```
The presence and naming of runtime-internal frames is in §16.

### 11.10 CON-ERR-NONERROR
* `throw "x"` or `throw {}`: no `stack`, and `e.message` is undefined, so `new EvalCodeError(undefined)` is thrown. Its `toString()` returns `undefined`. In `runtime.js:243`, `isObject(e) && e.stack` is falsy, so `outMessage.error = e.toString()` = `undefined`. **No console error is shown**, although the loop still aborted.
* `throw null` / `throw undefined`: `e.stack` itself throws a TypeError inside the handler. That TypeError (with runtime frames) becomes the reported error.
* `throw {stack: 's'}` → `'s'` is shown.

### 11.11 CON-ERR-SYSTEM — complete list of system error texts (reference)
| Text | Trigger | Source |
|---|---|---|
| `Script execution timed out: CPU time limit reached` (+ frames) | per-evaluation timeout | runtime-driver.js:105-107 |
| `Script execution timed out ungracefully, restarting virtual machine` | 5 s watchdog | make.js:303 |
| `Script execution has been terminated: CPU bucket is empty` | negative bucket | make.js:81 |
| `Your script is temporary blocked due to a hard reset inflicted to the runtime process.\nPlease try to change your code in order to prevent causing hard timeout resets.` | penalty | make.js:69 |
| `Script execution has been terminated: your isolate disposed unexpectedly, restarting virtual machine` | disposed isolate found at create | user-vm.js:19 |
| `Script execution has been terminated: unable to allocate memory, restarting virtual machine` | ArrayBuffer alloc fail | make.js:291 |
| `CPU halted` | `Game.cpu.halt()` | make.js:283 |
| isolated-vm text (e.g. heap exhausted / isolate disposed) | heap limit | make.js:285-293 |
| `Cannot save more than 10 memory segments on the same tick` and the 3 segment messages | SEG-SAVE-ERRORS | runtime.js:253-266 |
| `Script execution has been terminated: memory allocation limit reached` | Memory > 2 MB (server log only in PS) | driver/lib/index.js:156 |

### 11.12 CON-VISUAL / CON-VISUAL-SAVE (`console.js:57-95`, `make.js:253-260`)
* `addVisual(roomName, data)`:
  * `roomName ||= ""`; return if `!data`.
  * `limit = roomName=="map" ? 1000 : 500` KB (×1024 UTF-16 units).
  * `piece = isString(data) ? data : JSON.stringify(data) + "\n"`.
  * If `current.length + piece.length > limit*1024`, throw ``Error(`MapVisual size has exceeded 1000 KB limit`)`` or ``Error(`RoomVisual size in room ${room} has exceeded 500 KB limit`)``.
  * Otherwise append.
* `getVisualSize`, `clearVisual` and `getVisual` (export) operate per room key.
* Saved per non-empty room key as `env.setex('roomVisual:'+user+','+room+','+time, mainLoopResetInterval/1000 /*5 s*/, data)`. The room key `""` comes from `new RoomVisual()` with no name.
* `Game.map.visual` is a getter-created object whose methods are non-writable (`map.js:274-377`). The RoomVisual/MapVisual API itself belongs in the visuals spec.

---

## 12. Intents in the runtime (INT-*)

### 12.1 INT-STORE (`runtime.js:63-100`)
`intents = {list: Object.create(null), cpu: 0, set, push, pushByName, remove}`. All are plain properties of a per-tick object that is reachable only through engine closures.
* `set(id, name, data)`: `list[id] ||= null-proto {}`; cost per CPU-INTENT-COST; `list[id][name] = data` (the last call wins).
* `push(name, data, maxLen)`: `list[name] ||= []`; if `maxLen && len >= maxLen`, return false; push; `cpu += 0.2`; return true. Used by `notify` with maxLen 20.
* `pushByName(id, name, data, maxLen)`: the same for `list[id][name]` arrays. Used by `'room'` (createFlag/removeFlag/createConstructionSite/removeConstructionSite/destroyStructure, without maxLen) and `'global'` (market and power-creep ops). On `'global'`, `deal` has maxLen 10, `createOrder` has none (it is capped separately by `MARKET_MAX_ORDERS`), and the others have 50 (`market.js:90-187`, `power-creeps.js:187-408`).
* `remove(id, name)`: delete if present, `cpu -= 0.2`, return true (used by `cancelOrder`, `creeps.js:1016-1018`, `structures.js:228`).
* Intent data objects are stored **by reference** and sanitized only after the loop. **GOTCHA**: mutating an object after passing it in (e.g. `Game.notify(obj)`) changes what gets submitted.

### 12.2 INT-SANITIZE (`common/lib/system.js:4-140`)
* Only whitelisted fields per intent name survive.
* Transforms:
  * `string` → `""+v`;
  * `number` → `parseInt(v)` (**fractional amounts truncate**, and `undefined` gives `NaN`);
  * `boolean` → `!!v`;
  * `price` → `parseInt((1000*v).toFixed(0))` (milli-credits);
  * `string[]`/`number[]` → mapped array or `undefined`;
  * `bodypart[]` → filtered to `BODYPARTS_ALL`;
  * `userString` → `(""+(v||"")).substring(0,100)`;
  * `userText` → the same with 1000.
* Unknown intent names are dropped. Mods extend this via `customIntentTypes`.

### 12.3 INT-ROUTING (`engine/src/utils.js:405-436`)
* `notify` → `intents.notify` (sanitized).
* `room` → regrouped by each intent's sanitized `roomName` into `intents[roomName].room[name][]`.
* `global` → `intents.global`.
* Any other key is an object id. It is looked up in `userObjects`, then `roomObjects`, and `intents[obj.room][id] = sanitize(...)`. **Ids not found are silently dropped**, e.g. objects in rooms not visible this tick.

### 12.4 INT-NOTIFY-SAVE (`driver/lib/index.js:185-227`, `game.js:174-181`)
* `Game.notify(message, groupInterval)` → `push('notify', {message, groupInterval}, 20)`. It returns `OK`, or `ERR_FULL` after 20 in a tick. Each call costs 0.2 CPU.
* Save: take the first 20; `groupInterval` is clamped to [0,1440] (NaN passes through unclamped), `*60000`, then `floor`. `date = groupInterval ? ceil(now/gi)*gi : now`. The message is `(""+message).substring(0,500)` after sanitize's 1000-cap. The row in `users.notifications` is upserted on `{user, message, date, type:'msg'}`, with `$inc count`.
* **PS-QUIRK**: there is an effective **500-char** cap, but the docs say 1000 (`Game.md:361`).

---

## 13. End of tick (OUT-*)

### 13.1 OUT-SUCCESS (`runtime.js:271-296`, `make.js:192-277`, `runner.js:23-44`)
The output message contains:
* `type`, `error`;
* `memory` (always `{data, userId}`, so **Memory is re-saved every tick**, **PERF**);
* `memorySegments`;
* `usedTime`, `usedCleanTime`, `usedDirtyTime`;
* `intentsList`, `intentsCpu`;
* `console {log, results}`;
* `visual`;
* `activeSegments`, `activeForeignSegment`, `defaultPublicSegment`, `publicSegments`.

Persisted:
* the Memory string (env);
* segments (hmset);
* intents (`rooms.intents` per room, `users.intents` for global, notifications), with each room that received intents activated (`driver/lib/index.js:180-243`);
* `users` fields: `lastUsedCpu`, `lastUsedDirtyTime`, `cpuAvailable`, `activeSegments`, `defaultPublicSegment`, `activeForeignSegment`;
* public segment list;
* visuals.

### 13.2 OUT-FAILURE-MATRIX
| Failure | Console error | Memory | Segments | Intents | CPU billed | Seg settings | VM reset | Console cmds |
|---|---|---|---|---|---|---|---|---|
| Exception in main/loop/require | processed stack | **saved** (if `_parsed`) | saved | **saved** (queued before throw) | yes | saved | no | lost |
| Per-evaluation CPU timeout | timeout text | **saved** | saved | **saved** | yes | saved | no | lost |
| Exception in a console command | processed stack | saved | saved | saved | yes | saved | no | rest lost |
| `throw` of a non-Error value | **none** | saved | saved | saved | yes | saved | no | lost |
| Memory stringify throws | TypeError stack | no | no | no | **no** | no | no | — |
| Segment save validation | message string | no | no | no | **no** | no | no | — |
| `Game.cpu.halt()` | `CPU halted` | no | no | no | no | no | **yes** | lost |
| Heap exhausted / isolate disposed | isolated-vm text | no | no | no | no | no | **yes** | lost |
| 5 s hard timeout | ungraceful text | no | no | no | no | no | **yes** | lost |
| Memory > 2 MB at save | none (server log) | **no** (old kept) | yes | yes | yes | yes | no | ok |
| Bucket < 0 at start | bucket-empty text | n/a | n/a | n/a | special refill | n/a | no | kept |
| No objects | none | n/a | n/a | n/a | n/a | n/a | no | kept (not fetched) |

---

## 14. Moddability hooks (HOOK-*)
* **HOOK-PLAYER-SANDBOX** (`make.js:166-185`): fired every tick after `vm.start` (Game built) and before the run, with `sandbox = {run(code), set(name, value), get(name), getIsolate(), getContext(), getGlobal()}` and `userId`. `Game.shard` is implemented this way (`driver/lib/index.js:35-52`). Code run here is **not** CPU-counted. chitin should offer an equivalent "pre-run hook with access to the player global" as a first-class mod API.
* **HOOK-CUSTOM-OBJECT** (`driver/lib/index.js:56-73`, `engine/src/game/custom-prototypes.js:8-58`): `registerCustomObjectPrototype(objectType, name, {parent, properties: {k: fn(raw,id)}, prototypeExtender(proto, scope, {utils}), userOwned, findConstant, lookConstant})`. The functions are serialized as strings and eval'd in the VM. The class is created once per VM (the global name is a plain assignment), and instances are constructed per tick in the object loop (`game.js:384-393`).
* **HOOK-CUSTOM-INTENT**: `config.engine.customIntentTypes[name] = {field: transformType}` (`driver/lib/index.js:28`).
* **HOOK-CONFIG-ENGINE** (`driver/lib/index.js:19-33`): `mainLoopMinDuration` 1000, `mainLoopResetInterval` 5000, `cpuMaxPerTick` 500, `cpuBucketSize` 10000, `historyChunkSize`, `useSigintTimeout`, `reportMemoryUsageInterval`, `enableInspector`, `mainLoopCustomStage`, `getNextTickDelay` (`main.js:117-119`).
* **HOOK-LOOP-STAGES**: `runnerLoopStage` ('start', 'runUser', 'saveResultStart', 'saveResultFinish', 'finish') and `mainLoopStage` events.
* xxscreeps model, for design comparison only: `registerGlobal`, plus `runtimeConnector` hooks `initialize/receive/send` per tick (`xxscreeps/packages/xxscreeps/game/runtime.ts:1-28`, `mods/memory/game.ts:39-69`). That is a cleaner shape for chitin's internal mod API; Memory/segments themselves can be implemented as such a mod.

---

## 15. JS environment requirements (JS-*)

### 15.1 JS-ENGINE-LEVEL
The reference runs V8 via isolated-vm (`driver/package.json`), so bots assume a modern V8: ES2015–ES2020+ syntax and built-ins, typed arrays, Map/Set/WeakMap, Proxy/Reflect, generators, `JSON`, `Intl` (maybe), and **WebAssembly**. Lodash 3 uses `Function`/`eval`-style constructs (e.g. `_.template`), so `Function`/`eval` must exist. chitin's engine target is listed in §16.

### 15.2 JS-STACK-FORMAT
* Bots and tooling parse `Error.stack`. For example, the TypeScript-starter "ErrorMapper" regex-parses `    at fn (file:line:col)` and maps through a `main.js.map` module.
* chitin must emit V8-format stacks:
  * a header line from `Error.prototype.toString`;
  * frames as `    at <name> (<module>:<line>:<col>)` or `    at <module>:<line>:<col>`;
  * method names like `Object.loop`, constructors as `new X`;
  * columns 1-based, honoring the +36 offset on line 1 (MOD-WRAPPER).
* It should support `Error.stackTraceLimit` (default 10), `Error.captureStackTrace` and ideally `Error.prepareStackTrace`.

### 15.3 JS-MATH
* `Math.random` is V8's unseeded xorshift128+. It is non-deterministic in the original, so only the distribution [0,1) matters.
* `Math.pow/sin/cos/exp/log/atan2/…` should be fdlibm-exact ports (V8's `base::ieee754`), because engine formulas (GAME-GCL) floor results and bots may compare floats.

### 15.4 JS-DATE
* `Date` is the real wall clock. `Date.now()` is used by bots for timing and by runtime naming (`game.js:540`). The timezone is unspecified; use UTC.

### 15.5 JS-ASYNC
* The reference runs the whole tick synchronously inside one host→isolate call. Promise jobs can only run when that call unwinds, i.e. **after** `outMessage` has been copied out (`runtime.js:296`).
* **GOTCHA (derived; see §16)**: Memory writes, intents and logs made after an `await` are lost. chitin should drain microtasks only after capturing output, or before it if official differs. Default: match the reference.

### 15.6 JS-STRICTNESS
* The module wrapper and console-command function are sloppy unless the user code opts in.
* Engine code is mixed: `game.js` and `path-finder.js` are strict, while `runtime.js` and `console.js` are sloppy.
* Strictness affects whether failed assignments (non-writable globals, getter-only props) throw or silently fail **in the user's own code**, so chitin must implement both modes faithfully.

---

## 15b. PERF summary (scale target: about 1000 players on 16 cores)
* **Scope of data**:
  * *Room-local*: object snapshots of a room, event log, terrain. Share one immutable copy per tick across all player VMs.
  * *Per-player*: visibility set, Game hashes, my/hostile filters, Memory, segments, intents, console, flags, power creeps, transactions.
  * *Global*: market orders and history, room status/accessible rooms, world topology (mapGrid), constants. One copy per tick or per world.
* **Where the original is slow, and what chitin should do instead**:
  * About 20 DB queries per player per tick, and an `ExternalCopy` of the entire runtime data, **including all code modules**, into the isolate every tick (`make.js:144-160`). chitin keeps code resident and hands in references.
  * Eager construction of every visible object per player, plus 15×`Array(2500)` spatial arrays per visible room and find caches. Duplicated N times for N observers of a room.
  * A deep copy of all constants per tick (GLB-CONSTANTS).
  * Full Memory `JSON.parse` on first access and full `JSON.stringify` + store **every tick**, even if unchanged.
  * Every loaded segment rewritten every tick.
  * Console commands and notifications go through DB round-trips.
* **Parallelism**: the user stage has no cross-player data dependencies (reads tick-T state, writes only intents and own state), so run players on a work-stealing pool across all cores. Order them heaviest-first (`lastUsedDirtyTime` desc, TICK-USER-ELIGIBILITY). The processor stage is room-local except for cross-room/global intents (other specs).
* **Memory pressure**: persistent per-player heaps are required for parity (global caching is ubiquitous in bots). Bound them with VM-HEAP-LIMIT and VM-IDLE-EVICT, keep terrain and room snapshots outside player heaps, and consider compacting idle heaps.
* **Hot paths inside a VM**: lazy getters (GAME-LAZY-PROPS), `Room.find` via the find caches, `RoomPosition` construction (packed int, `rooms.js:1276-1293`), lodash 3 iteration helpers, and JSON. chitin's engine should make property-access ICs and JSON fast.

---

## 16. Open questions
1. **Official vs PS messages and limits**:
   * the exact official text for the CPU limit exceeded case;
   * whether official saves Memory and intents on timeout (PS: yes);
   * Memory over 2 MB (PS: silent skip);
   * console log count/size limits on official (PS: none);
   * whether `console.error/warn` exist on official (the engine's console.js says no).
2. **Stack trace tail**: does the official error text include runtime-internal frames (`Object.exports.evalCode (<runtime>:…)`), and exactly how many? The reference regex removes only the last line. Not determinable without running V8+isolated-vm.
3. **Timeout message vs `e.stack`**: V8 formats `stack` lazily, so whether the renamed message `'…CPU time limit reached'` appears in the stack header depends on when `stack` was first materialized by isolated-vm. Verify empirically.
4. **Microtask timing** (JS-ASYNC): confirm with isolated-vm that promise jobs run after the run reference returns (after output capture), not at the end of each nested `runSync`.
5. **VIS-TOMBSTONE**: does the official server grant room visibility via tombstones? This is driver-dependent.
6. **Construction CPU on official**: is Game construction billed or visible in `getUsed()` at the start of `loop`? The PS bills neither.
7. **Official-only APIs**:
   * `InterShardMemory` details: the empty value (`''` vs `null`); the error for an unknown shard; the error/limit when `setLocal` exceeds 100 KB; whether `setLocal` is visible to `getLocal` in the same tick.
   * `setShardLimits` validation details and the cooldown source of truth.
   * `generatePixel`: whether the bucket and `Game.cpu.bucket` change in-tick, and whether multiple calls per tick are allowed.
   * `unlock`/`unlocked`/`unlockedTime` state machine.
   * `interShardSegment` persistence on a single-shard chitin world.
8. **JS feature level** chitin must support: which ES version, which Intl, and which WebAssembly proposals (bulk-memory, sign-ext, mutable-globals, multi-value, reference-types, SIMD?) current Rust/Emscripten screeps bots require. Also whether `TextEncoder/TextDecoder` exist on official (absent in PS).
9. **Hard-timeout penalty** (`skipTicksPenalty`): the official values. The PS never sets it.
10. **Foreign segment id 0** unreachability and stale default resolution: is it PS-only? This decides whether to replicate.
11. **Game.shard.name** for a chitin world: the configured name versus `shard0`. Also which shard names `Game.cpu.shardLimits` should list.
12. **Id format**: switch chitin to 24-hex ids to match official, which affects `GAME-ASSERT-SERIALIZED` and bot id-length heuristics (cross-spec decision).
13. **Official CPU clock**: CPU-CLOCK settles the PS: one steady wall clock serves the timeout, `getUsed()` and billing. That answers 01 §20 Q3. Two things remain unknown because the official runtime is closed-source:
    * whether official meters wall time or thread CPU time;
    * whether official arms its timeout per evaluation.

    The docs only say the limit is "a duration of time in milliseconds" (`docs/source/cpu-limit.md:12`). chitin's choice of clock is a server setting (§8.13); its default is decided in 14 OQ-8.
