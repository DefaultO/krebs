# 12 — xxscreeps architecture (prior art: mod system, binary schema, tick pipeline, performance)

Scope: `reference/xxscreeps` (laverdet/xxscreeps @d1e3bad), TypeScript. This spec describes how xxscreeps is
built so chitin can copy its good ideas and avoid its traps. **xxscreeps is NOT the parity target.** The vanilla
engine/driver (`reference/engine`, `reference/driver`) are authoritative for gameplay; every place where xxscreeps
differs from vanilla is listed under `XX-COMPAT-*`. Where a rule below says "vanilla does X", the vanilla citation
is given next to it.

Conventions (line numbers refer to the reference checkouts listed in the task; `.../` inside a bullet abbreviates
the same directory as the previous full citation in that bullet)

- Every rule cites `reference/<pkg>/<path>:<line>`. All xxscreeps sources live under
  `reference/xxscreeps/packages/xxscreeps/` (written out in full), plus `reference/xxscreeps/packages/pathfinder`,
  `reference/xxscreeps/packages/redis`, `reference/xxscreeps/packages/lodash3`.
- `T` = the shard time stored in the `time` key when a tick starts. Runners execute tick `T`; processors read
  room state `T` and write room state `T+1`.
- "system user" = any user id of length <= 2 (`'1'`, `'2'` Invader, `'3'` Source Keeper, ...)
  (`reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:236`).
- **GOTCHA** = subtle behaviour a 1:1 port (or a chitin mod author porting xxscreeps code) will get wrong.
  **PERF** = performance-relevant structure (room-local vs global, hot paths, known slow spots).
- `'#name'` keys are xxscreeps engine-private fields (see XX-SCHEMA-PRIVATE).

---

## 1. Inventory

| ID | Area | Item |
|---|---|---|
| XX-ARCH-SERVICES | Architecture | Service topology: launcher, main loop, processor service + workers, runner service + sandboxes, backend |
| XX-ARCH-CHANNELS | Architecture | Pub/sub channel names and messages between services |
| XX-ARCH-TICK-OVERVIEW | Architecture | End-to-end tick pipeline (one diagram, then details in XX-SVC/XX-PROC/XX-RUN) |
| XX-MOD-MANIFEST | Mods | Mod declaration: `index.ts` exporting `manifest {dependencies, provides}` |
| XX-MOD-RESOLVE | Mods | Mod resolution, dependency DFS, deterministic order, cycle detection |
| XX-MOD-PROVIDES | Mods | The 9 provide slots and which service loads which slot |
| XX-MOD-VIRTUAL | Mods | Virtual modules `xxscreeps:mods/<provide>` (node loader hooks / webpack) |
| XX-MOD-CONFIG | Mods | `config` provide: defaults, initializationDefaults, JSON schema merge + validation |
| XX-MOD-HOOKREG | Mods | `makeHookRegistration` semantics (register / took / iterated / mapped / reduced) |
| XX-MOD-HOOK-GAME | Mods | Game hooks: `environment`, `gameInitializer`, `roomInitializer`, `runtimeConnector` |
| XX-MOD-HOOK-RUNNER | Mods | Runner hook `runnerConnector` (initialize / refresh / save) |
| XX-MOD-HOOK-PROCESSOR | Mods | Processor hooks `flushContext`, `refreshRoom` |
| XX-MOD-HOOK-DRIVER | Mods | Driver hooks `isolateInspector`, `sandboxCreated` |
| XX-MOD-HOOK-BACKEND | Mods | Backend hooks `backendReady`, `middleware`, `roomSocket`, `sendUserInfo`, `route`, `subscription` |
| XX-MOD-GLOBALS | Mods | `registerGlobal` / `defineGlobal`, constants exported to sandbox globals |
| XX-MOD-FINDLOOK | Mods | `registerFindHandlers`, `registerLook` |
| XX-MOD-SCHEMA-EXT | Mods | `registerStruct` / `registerVariant` / `registerEnumerated` on named schema paths |
| XX-MOD-INTENT-REG | Mods | `registerIntentProcessor(receiver, name, constraints, fn)` |
| XX-MOD-TICK-REG | Mods | `registerObjectPreTickProcessor`, `registerObjectTickProcessor`, `registerRoomTickProcessor`, `registerShardTickProcessor`, `everyNTicks`, `registerShardInitializer` |
| XX-MOD-OBSTACLE | Mods | `registerObstacleChecker` (movement/pathing obstacle composition) |
| XX-MOD-STORAGE | Mods | `registerStorageProvider` (URI scheme -> keyval/pubsub provider) |
| XX-MOD-RENDER | Mods | `bindRenderer`, `bindMapRenderer`, `bindTerrainRenderer` (backend object rendering chain) |
| XX-MOD-PROTO | Mods | `'#'`-prefixed virtual methods on `RoomObject` as cross-mod hook surface; prototype wrapping |
| XX-MOD-EXTEND | Mods | `extend(Class, descriptors)` to add API methods/properties to other mods' classes |
| XX-MOD-INTERMOD | Mods | Mod-defined registries: buildable structures, harvestables, NPCs, attack notifications, notification transports |
| XX-MOD-TYPES | Mods | TypeScript declaration merging (`interface Intent/Schema/Find/Look/Global/...`) |
| XX-MOD-CATALOG | Mods | Built-in mod list: dependencies, provides, contributions |
| XX-MOD-TEST | Mods | `test` provide and the `simulate()` harness |
| XX-SCHEMA-WHY | Schema | Why room blobs (stated goals) |
| XX-SCHEMA-FORMATS | Schema | Format combinators (primitives, array, compose, constant, declare, enumerated, optional, struct, union, variant, vector) |
| XX-SCHEMA-LAYOUT | Schema | Binary layout algorithm (sizes, alignment, member sorting/packing, pointers, lists) |
| XX-SCHEMA-STRING | Schema | String encoding (latin1 or UTF-16 with negative length) |
| XX-SCHEMA-HEADER | Schema | 16-byte blob header (magic, version, length) |
| XX-SCHEMA-OVERLAY | Schema | Lazy prototype getters over the buffer, memoization rules, setters |
| XX-SCHEMA-WRITE | Schema | Writer: full re-serialization through a 16 MiB scratch buffer into a SharedArrayBuffer |
| XX-SCHEMA-VERSION | Schema | Version = SHA-1 of layout; schema archive; transparent upgrade; Kaitai |
| XX-SCHEMA-ID | Schema | Object id packing (24 hex chars in 16 bytes), `generateId` |
| XX-SCHEMA-POS | Schema | `RoomPosition` packed int32; world limited to 256x256 rooms |
| XX-SCHEMA-ROOM | Schema | Room blob shape (`#objects`, `#users`, `#eventLog`, mod-added members) |
| XX-SCHEMA-OBJECT | Schema | RoomObject base struct, variants, sizes |
| XX-SCHEMA-TERRAIN | Schema | World terrain blob, 2 bits/tile, shared read-only |
| XX-SCHEMA-PRIVATE | Schema | `'#field'` -> private symbols (babel transform), unsafe fallback |
| XX-SCHEMA-DETACH | Schema | Buffer detach at end of runtime tick; stale-object detection |
| XX-DB-PROVIDER | Storage | KeyVal / PubSub provider interface, `KeyvalScript` |
| XX-DB-INSTANCES | Storage | `Database` (global) vs `Shard` (data / pubsub / scratch) |
| XX-DB-LOCAL | Storage | Local provider: one host thread owns an in-memory Map, others RPC over MessagePort/socket |
| XX-DB-BLOB | Storage | Blob storage (binary values, file per key) |
| XX-DB-SAVE | Storage | Persistence cadence and crash semantics |
| XX-DB-KEYS | Storage | Key layout (data and scratch) |
| XX-DB-DOUBLEBUF | Storage | Room blob double buffer `room0/` / `room1/` |
| XX-DB-CHANNEL-MUTEX | Storage | `Channel` JSON pub/sub wrapper, `Mutex` |
| XX-DB-SORTEDSET | Storage | Local `SortedSet` implementation complexity |
| XX-DB-REDIS | Storage | `@xxscreeps/redis` storage mod |
| XX-SVC-LAUNCH | Tick | Launcher, worker threads vs single-thread mode |
| XX-SVC-BOOT | Tick | Boot: scratch flush, room initialization, first queue |
| XX-SVC-TICK | Tick | Main loop tick steps |
| XX-SVC-ABANDON | Tick | `intentAbandonTimeout` handling |
| XX-SVC-TICKSPEED | Tick | `tickSpeed` and hot reload |
| XX-PROC-QUEUE | Processor | Per-tick room process queue (sorted set, score = runners still pending) |
| XX-PROC-WORKERS | Processor | Processor worker count and room affinity |
| XX-PROC-ROOMCACHE | Processor | In-memory Room reuse across ticks on the same worker |
| XX-PROC-PHASES | Processor | `RoomProcessor.process()` phase order inside one room |
| XX-PROC-GAMETIME | Processor | Processor `Game.time` = T+1, runner `Game.time` = T |
| XX-PROC-INTENT-ORDER | Processor | Global intent priority + conflict masks from before/after/type constraints |
| XX-PROC-INTENT-DISPATCH | Processor | Per-user / per-object intent dispatch rules |
| XX-PROC-INTENT-RUNTIME | Processor | Runtime `IntentManager` (save/remove/pushLocal, last-wins) |
| XX-PROC-DUAL-VALIDATION | Processor | Shared `check*` functions used by runtime API and processor |
| XX-PROC-MOVEMENT | Processor | Movement announce/dispatch, conflict resolution, pull chains, fatigue |
| XX-PROC-CONTEXT | Processor | `ProcessorContext` API (didUpdate, setActive, wakeAt, sendRoomIntent, task) |
| XX-PROC-TASKS | Processor | Async tasks and finalizers (the only way to touch global state from a room) |
| XX-PROC-INTERROOM | Processor | Inter-room intents and the finalize phase |
| XX-PROC-FINALIZE | Processor | `RoomProcessor.finalize()` steps (save/copy/sleep) |
| XX-PROC-SLEEP | Processor | Room sleep / wake scheduling |
| XX-PROC-USERS | Processor | `#users` recomputation and user<->room relationship sets |
| XX-PROC-SHARD | Processor | Shard-level (global) tick processors / initializers in the main loop |
| XX-PROC-NPC | Processor | NPC loops run inside the processor |
| XX-PROC-INTERNAL | Processor | Internal intents (backend/system only) |
| XX-PROC-OBJECTS | Processor | Room object insert/remove/move/flush semantics and indices |
| XX-PROC-EVENTLOG | Processor | Event log storage and reset |
| XX-PROC-ACTIONLOG | Processor | Action log retention (10 ticks) and wakeups |
| XX-RUN-SCHED | Runner | Runner scheduling: user queue, affinity, migration timeout, concurrency |
| XX-RUN-INSTANCE | Runner | `PlayerInstance` lifecycle (code reload, stale reset, left-behind) |
| XX-RUN-PAYLOAD | Runner | Initialization / Tick payloads and tick result |
| XX-RUN-SANDBOX | Runner | Sandbox modes (isolated / unsafe / experimental) and runtime bundle |
| XX-RUN-TICK | Runner | Runtime `tick()` flow inside the sandbox |
| XX-RUN-GAME | Runner | `GameState` / `GameBase` / `Game`, `runWithState` / `runAsUser` / `runForUser` / `runForPlayer` / `runOneShot` |
| XX-RUN-CPU | Runner | CPU limit, bucket, tick limit accounting |
| XX-RUN-MODULES | Runner | Player module loader (CommonJS, ESM, WASM/WASI, JSON) |
| XX-RUN-MEMORY | Runner | `Memory` / `RawMemory` implementation |
| XX-RUN-SEGMENTS | Runner | Memory segments, foreign/public segments |
| XX-RUN-FLAGS | Runner | Flags stored per user, processed inside the runner sandbox |
| XX-RUN-VISUAL | Runner | Room/map visuals transport |
| XX-RUN-CONSOLE | Runner | Console output and console eval |
| XX-RUN-HARDEN | Runner | Sandbox hardening (frozen intrinsics, private symbols) |
| XX-RUN-FIND-CACHE | Runner | `room.find` cache and per-user flush |
| XX-PF-NATIVE | Pathfinder | Native JPS path finder, process-wide terrain, world position packing |
| XX-PF-ROOMSEARCH | Pathfinder | `roomSearch` / `findPath` cost matrix construction and caching |
| XX-BE-SERVER | Backend | Koa server bootstrap, mod routes |
| XX-BE-ROOMSOCKET | Backend | Room websocket: shared subscription, throttle, render, JSON diff |
| XX-BE-ENDPOINTS | Backend | Backend-issued intents: place-spawn, respawn, add-object-intent |
| XX-PERF-FAST | Performance | Everything xxscreeps does to be fast (enumerated) |
| XX-PERF-SLOW | Performance | Where xxscreeps is still slow / will not scale (enumerated) |
| XX-COMPAT-MOVE | Compat gap | Movement conflict resolution differs from vanilla |
| XX-COMPAT-WEIGHT | Compat gap | Creep weight/fatigue formula differs (boosted CARRY) |
| XX-COMPAT-INTENT-ORDER | Compat gap | Intent execution order/priorities derived differently from vanilla |
| XX-COMPAT-USER-ORDER | Compat gap | Cross-user intent order = runner completion order |
| XX-COMPAT-STALE-OBJECTS | Compat gap | Objects from previous ticks throw on access |
| XX-COMPAT-MEMORY | Compat gap | `Memory` object reused across ticks; root prototype; accessor shape |
| XX-COMPAT-CPU | Compat gap | Fixed CPU limit 100, intent CPU never charged, wall-time `getUsed` |
| XX-COMPAT-BORDER | Compat gap | Fatigue reset and unconditional removal on room border crossing |
| XX-COMPAT-EVENTLOG | Compat gap | Stale event log in rooms that did not update (as read) |
| XX-COMPAT-LATE-INTENTS | Compat gap | Abandoned-tick intents applied one tick late |
| XX-COMPAT-CHECK-ORDER | Compat gap | Validation precedence still being corrected (changesets) |
| XX-COMPAT-WORLD-SIZE | Compat gap | W127..E127 / N127..S127 hard limit |
| XX-COMPAT-MISSING | Compat gap | Not implemented / stubbed features |
| XX-COMPAT-FLAGS | Compat gap | Flags applied in the runner, not the processor |
| XX-COMPAT-ID | Compat gap | Object ids must be hex, <= 24 chars |
| XX-COMPAT-VISION-INTENTS | Compat gap | Intents targeting rooms where the user has vision but no intent-capable object are dropped |
| XX-REC-COPY | Recommendation | What chitin should copy (numbered list) |
| XX-REC-AVOID | Recommendation | What chitin should avoid (numbered list) |

---

## 2. Architecture

### XX-ARCH-SERVICES

- Four long-running services, each with its own JS heap: main loop, processor, runner, backend
  (`reference/xxscreeps/docs/architecture.md:52-70`). The launcher starts `main.js` by dynamic import, then
  backend / processor service / runner service either as `worker_threads` (default) or imported into the same
  thread when `launcher.singleThreaded` is set (`reference/xxscreeps/packages/xxscreeps/engine/service/launcher.ts:50-86`).
  CLI flags `--no-backend --no-processor --no-runner` skip services (`.../engine/service/launcher.ts:11-14`).
- The processor service itself spawns N processor workers (worker threads, or in-thread when single-threaded)
  via `negotiateResponderClient('xxscreeps/engine/processor/worker.js')`
  (`reference/xxscreeps/packages/xxscreeps/engine/service/processor.ts:63-80`,
  `reference/xxscreeps/packages/xxscreeps/utility/responder.ts:27-58`).
- The runner service is a single Node thread that owns one sandbox (V8 isolate) per player and runs up to
  `runner.concurrency` sandboxes concurrently (`reference/xxscreeps/packages/xxscreeps/engine/service/runner.ts:42,88-114`).
- All services talk only through the storage providers: a `data` keyval, a `scratch` keyval and a `pubsub`
  per shard, plus a global `data`/`pubsub` database (`reference/xxscreeps/packages/xxscreeps/engine/db/shard.ts:57-79`,
  `reference/xxscreeps/packages/xxscreeps/engine/db/database.ts:20-34`). "JS object instances never cross service
  boundaries"; the exchange format is the binary room blob (`reference/xxscreeps/docs/architecture.md:56-59`).
- Only the main loop writes `time`; only the processor mutates room state (`reference/xxscreeps/docs/architecture.md:54-55,115-119`).
- **PERF**: every cross-service interaction (room pop, intent push, blob get/set, counter decrement) is an
  RPC to the keyval host (XX-DB-LOCAL) or Redis. There is no shared memory between services except blobs that are
  `SharedArrayBuffer`-backed when passed through `MessagePort`.

### XX-ARCH-CHANNELS

| Channel | Messages | Publisher -> subscriber | Source |
|---|---|---|---|
| `channel/service` | `shutdown`, `mainConnected`, `mainDisconnected`, `processorConnected`, `processorInitialized`, `runnerConnected`, `tickFinished{time}` | all <-> main | `reference/xxscreeps/packages/xxscreeps/engine/service/index.ts:9-19` |
| `channel/processor` | `process{time, roomNames?}`, `finalize{time}`, `shutdown` | main/runner/processor -> processor service | `reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:12-18` |
| `channel/runner` | `run{time}`, `shutdown` | main -> runner | `reference/xxscreeps/packages/xxscreeps/engine/runner/model.ts:18-23` |
| `channel/game` | `tick{time}` | main -> everyone (Shard keeps `shard.time` in sync) | `reference/xxscreeps/packages/xxscreeps/engine/db/shard.ts:41-45,70-73` |
| `processor/room/<room>` | `didUpdate{time}`, `willSpawn` | processor/backend -> backend room sockets | `reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:20-25` |
| `runner/<user>` | `eval{ack?,echo,expr}`, `intent{receiver,intent,params}` | backend -> runner | `reference/xxscreeps/packages/xxscreeps/engine/runner/model.ts:27-33` |
| `user/<user>/console`, `user/<user>/ack`, `runner/<user>/usage` | console JSON, eval ack, usage | runner -> backend | `reference/xxscreeps/packages/xxscreeps/engine/runner/model.ts:6-37` |
| `user/<user>/flags`, `user/<user>/visual`, `user/<user>/publicSegments` | mod-specific | mods | `reference/xxscreeps/packages/xxscreeps/mods/flag/model.ts:17-22`, `reference/xxscreeps/packages/xxscreeps/mods/visual/model.ts:9-12`, `reference/xxscreeps/packages/xxscreeps/mods/memory/model.ts:25-27` |

### XX-ARCH-TICK-OVERVIEW

```
main (time = T)
 ├─ copy scratch 'activeUsers' -> 'tick{T%2}/runnerUsers'; publish process{T}; publish run{T}
 │
 ├─ runner: for each user (affinity first): load visible room blobs @T -> sandbox tick -> intents JSON
 │          -> rPush rooms/<r>/intents; zAdd tick{T}/processRooms r -1; when score==0 publish process{roomNames}
 │
 ├─ processor workers: pop rooms with score 0 -> RoomProcessor.process():
 │          PreTick -> RoomTick -> user intents -> movement -> Tick -> async tasks
 │          -> push inter-room intents (rooms/<dst>/finalIntents) -> roomDidProcess (pending--)
 │     when pending==0: publish finalize{T}
 │          every worker: finalize own rooms (apply inter-room intents, save blob @T+1 or copy, sleep)
 │                        + any extra rooms that only received inter-room intents
 │     when all finalized: publish tickFinished{T}
 │
 └─ main: shard tick processors(T); queue for T+1; set time=T+1; publish tick{T+1}; sleep to tickSpeed
```

Runner work and processor work for the same tick overlap: a room is processed as soon as every player who can
issue intents in it has finished (XX-PROC-QUEUE) (`reference/xxscreeps/README.md:40-44`).

---

## 3. Mod system

### XX-MOD-MANIFEST

- A mod is a package directory whose entry (`index.ts`) exports `manifest: Manifest` with
  `dependencies?: string[]` and `provides: Provide | Provide[] | null`
  (`reference/xxscreeps/packages/xxscreeps/config/mods.ts:15-18`).
- Example: the creep mod depends on memory, resource, structure and provides
  `backend, constants, game, processor, test` (`reference/xxscreeps/packages/xxscreeps/mods/creep/index.ts:3-10`).
- A meta-mod has `provides: null` and only dependencies; `classic` pulls in the whole classic game
  (`reference/xxscreeps/packages/xxscreeps/mods/classic/index.ts:3-25`).
- The entry may also export functions for other mods (an "API surface"), e.g. construction exports
  `registerBuildableStructure` (`reference/xxscreeps/packages/xxscreeps/mods/construction/index.ts:5-7`).
- Third-party mods are ordinary npm packages: `@xxscreeps/redis` has `provides: 'storage'`
  (`reference/xxscreeps/packages/redis/index.ts:3-5`).

### XX-MOD-RESOLVE

Algorithm (`reference/xxscreeps/packages/xxscreeps/config/mods.ts:42-110`):

```
from = cwd if cwd/package.json exists else the xxscreeps install dir            (mods.ts:28-38)
load(specifiers):
    imports = for spec in sort(specifiers) (parallel):                           (mods.ts:49-68)
        url = resolve(spec) else resolve(spec + "/index.js")   (ESM resolution from `from`)
        manifest = (await import(url)).manifest
    for (manifest, spec, url) in imports (in sorted order):                      (mods.ts:69-106)
        if spec in resolved: continue
        if spec in stack: throw "Detected cyclic dependency: a -> b -> spec"
        stack.push(spec); load(manifest.dependencies ?? []); stack.pop()
        for p in provides (array | single | null->[]):
            providers[p] = resolve("./p/index.js", url) else resolve("./p.js", url)
        mods.push({ provides: providers, url }); resolved.add(spec)
load(config.mods ?? initializationDefaults.mods)                                 (mods.ts:108)
```

- Result is a DFS post-order: dependencies before dependents, siblings in lexicographic specifier order.
- Default mod list: `xxscreeps/mods/classic`, `xxscreeps/mods/backend/cookie`, `.../password`, `.../steam`
  (`reference/xxscreeps/packages/xxscreeps/config/config.ts:302-308`).
- **GOTCHA**: this order is load-bearing everywhere: it is the registration order of hooks, intent processors
  (input to the constraint sort, XX-PROC-INTENT-ORDER), room tick processors, obstacle checkers, and — most
  importantly — the order of `registerVariant('Room.objects', ...)` calls, i.e. the variant index stored in every
  room blob (XX-SCHEMA-LAYOUT). Adding/removing a mod changes the blob layout (handled by XX-SCHEMA-VERSION).
- Dependencies are a DAG; "a mod should only import from its declared dependencies"
  (`reference/xxscreeps/docs/architecture.md:335-337`). Not enforced by the loader.

### XX-MOD-PROVIDES

Fixed list `backend | config | constants | driver | game | main | processor | storage | test`
(`reference/xxscreeps/packages/xxscreeps/config/loader.ts:3`); unknown provide -> `Cannot find package`
(`reference/xxscreeps/packages/xxscreeps/config/nodejs.ts:49-53`).

| Provide | Loaded by (import site) | Purpose |
|---|---|---|
| `constants` | `game/constants/index.ts:1` (re-exported, so everywhere game code runs) and the sandbox bundle (`driver/sandbox/isolated/index.ts:46-48`) | `C.*` constants; all constants also become sandbox globals (XX-MOD-GLOBALS) |
| `game` | processor worker (`engine/processor/worker.ts:15`), backend (`backend/server.ts:17`), sandbox bundle (`driver/sandbox/isolated/index.ts:48`), room schema module (`engine/db/room.ts:8`), test harness | Game classes, player API, schema registrations, find/look, globals |
| `processor` | processor worker (`engine/processor/worker.ts:16`), backend (`backend/server.ts:18`), simulate (`test/simulate.ts:30`) | Intent/tick processors. The runtime bundle refuses it (`driver/sandbox/isolated/index.ts:38`) |
| `driver` | runner service (`engine/service/runner.ts:14`), processor worker (`engine/processor/worker.ts:14`), tests | Runner connectors (Node side of the sandbox), processor `refreshRoom` hooks |
| `main` | main loop (`engine/service/main.ts:13`), simulate | Shard tick processors / initializers |
| `backend` | backend server (`backend/server.ts:16`) | Routes, sockets, renderers |
| `storage` | lazily on first `connectToProvider` (`engine/db/storage/register.ts:32-45`) | Storage providers |
| `config` | `config/index.ts:4` | Config defaults + JSON schema fragment (`config.schema.json` next to the provider file) |
| `test` | `test/run.ts:9` | Mod test suites |

(All paths above are relative to `reference/xxscreeps/packages/xxscreeps/`.)

- **PERF**: because `game` is loaded in the sandbox, the whole game library (all mods' `game` slots, schema
  readers, lodash) is compiled into every player isolate (XX-RUN-SANDBOX).

### XX-MOD-VIRTUAL

- `xxscreeps:mods/<provide>` specifiers are virtual modules generated from the resolved mod list
  (`reference/xxscreeps/packages/xxscreeps/config/loader.ts:39-49`):
  - `constants`: `export * from "<url>";` per mod (`.../config/loader.ts:21`).
  - `config`: imports each `config` module + sibling `config.schema.json`, exports `defaults`,
    `initializationDefaults`, `schemas` arrays (`.../config/loader.ts:23-33`).
  - every other provide: `import "<url>";` per mod, i.e. side-effect registration in mod order (`.../config/loader.ts:22`).
- Node: a module loader hook resolves `xxscreeps:*` and serves the generated source
  (`reference/xxscreeps/packages/xxscreeps/config/nodejs.ts:31-70`; registered at `.../config/mods.ts:121-135`).
- Sandbox: the runtime is a webpack bundle where `/xxscreeps:mods/constants`, `/xxscreeps:mods/game` and
  `/xxscreeps:packages` are `VirtualModulesPlugin` entries (`reference/xxscreeps/packages/xxscreeps/driver/sandbox/isolated/index.ts:23-53`).

### XX-MOD-CONFIG

- Merge order (later wins): base `initializationDefaults` -> each mod's `initializationDefaults` -> base
  `defaults` -> each mod's `defaults` -> `.screepsrc.yaml` (`reference/xxscreeps/packages/xxscreeps/config/index.ts:14-31`).
- Schema: `allOf: [base, ...mod schemas]`, validated with ajv; invalid config throws at startup
  (`reference/xxscreeps/packages/xxscreeps/config/index.ts:34-43`).
- `--sandbox <experimental|unsafe|isolated>` CLI override (`reference/xxscreeps/packages/xxscreeps/config/raw.ts:26-35`).
- Relevant defaults: `processor.concurrency = cpus+1`, `processor.intentAbandonTimeout = 5000`,
  `runner.concurrency = cpus+1`, `runner.cpu = {bucket 10000, memoryLimit 256 (MB), tickLimit 500}`,
  `runner.migrationTimeout = 50`, `runner.sandbox = 'isolated'`, `database.saveInterval = 2` (minutes; the doc
  comment says 120), `game.tickSpeed = 250` (`reference/xxscreeps/packages/xxscreeps/config/config.ts:259-315`).

### XX-MOD-HOOKREG

`makeHookRegistration<Keys>()` (`reference/xxscreeps/packages/xxscreeps/utility/hook.ts:7-96`):

- `register(key, handler)` appends to the list for `key`; throws `Already took <key>` if an invoker for that key
  has already been materialized (`.../utility/hook.ts:82-92`).
- `makeIterated(key)` returns a function; on its FIRST call it snapshots the handler list, marks the key taken,
  and thereafter calls all handlers in registration order (`.../utility/hook.ts:53-59`; lazy via `lateCallback`,
  `reference/xxscreeps/packages/xxscreeps/utility/memoize.ts:1-13`).
- `makeMapped(key)` returns `(...args) => Iterable<result>` — **GOTCHA**: the iterable is lazy (`Fn.map`), so
  handlers run only when the caller iterates it (`.../utility/hook.ts:61-67`).
- `makeReduced(key)` threads a value through handlers (`.../utility/hook.ts:69-75`); `map(key)` iterates handlers
  without marking taken (`.../utility/hook.ts:78-80`).
- `acquireHookEffects(disposable, hooks)`: hook results of shape `[effect, value]`; effects are deferred into a
  `DisposableStack` (cleanup on disconnect), values collected (`.../utility/hook.ts:98-114`).

### XX-MOD-HOOK-GAME

Registry: `reference/xxscreeps/packages/xxscreeps/game/symbols.ts:8-41`.

| Hook | Signature | Invoked | Example |
|---|---|---|---|
| `environment` | `() => void` | once per process after all mod imports, by `initializeGameEnvironment()` (`game/index.ts:21-26`): processor worker (`engine/processor/worker.ts:40`), backend (`backend/server.ts:20`), runtime init (`driver/runtime/index.ts:87`) | resource mod installs default `0` for every resource key on `Store.prototype` (`mods/resource/store.ts:25-29`) |
| `gameInitializer` | `(game: Game, data?: TickPayload) => void` | every `new Game(...)` (`game/game.ts:101`); `data` only in player runtime | `Game.creeps = {}` (`mods/creep/game.ts:17`), `Game.spawns` (`mods/spawn/game.ts:14`), `Game.flags` + flag insertion (`mods/flag/game.ts:74-91`), `Game.cpu` (`driver/sandbox/isolated/runtime.ts:53-55`) |
| `roomInitializer` | `(room, state: GameState) => void` | for every room in every `GameState` constructor (`game/game.ts:31-33`) — runtime, processor, backend; may run more than once on a reused room | spawn mod computes `room.energyAvailable/energyCapacityAvailable` (`mods/spawn/game.ts:24-36`) |
| `runtimeConnector` | `{initialize?(InitializationPayload), receive?(TickPayload), send?(TickResult)}` | composed in the sandbox runtime: initialize once, receive before the player loop, send after (`driver/runtime/index.ts:60-72,92,116,202`) | memory (`mods/memory/game.ts:39-69`), flags (`mods/flag/game.ts:42-64`), every `registerGlobal` (`game/symbols.ts:47-57`) |

(paths relative to `reference/xxscreeps/packages/xxscreeps/`)

### XX-MOD-HOOK-RUNNER

- `runnerConnector: (player: PlayerInstance) => AsyncEffectAndResult<{initialize?, refresh?, save?}>`
  (`reference/xxscreeps/packages/xxscreeps/engine/runner/symbols.ts:6-14`). Called once per `PlayerInstance`
  creation; `initialize` fills the `InitializationPayload` before sandbox creation, `refresh` fills the
  `TickPayload` before each tick, `save` consumes the `TickResult` after each tick. `save` handlers run in
  REVERSE registration order (`reference/xxscreeps/packages/xxscreeps/engine/runner/instance.ts:22-36`).
- It is the Node-side twin of `runtimeConnector`: a mod moves per-user data into and out of the sandbox by
  extending `InitializationPayload`/`TickPayload`/`TickResult` (declaration merging) and registering both halves.
  Examples: memory (`reference/xxscreeps/packages/xxscreeps/mods/memory/driver.ts:28-137`), flags
  (`reference/xxscreeps/packages/xxscreeps/mods/flag/driver.ts:18-45`), visuals
  (`reference/xxscreeps/packages/xxscreeps/mods/visual/driver.ts:14-22`), controller GCL / controlled-room count
  (`reference/xxscreeps/packages/xxscreeps/mods/controller/driver.ts:21-36`), power
  (`reference/xxscreeps/packages/xxscreeps/mods/powerspawn/driver.ts:10-17`), notifications
  (`reference/xxscreeps/packages/xxscreeps/mods/notifications/driver.ts:23-30`).

### XX-MOD-HOOK-PROCESSOR

- `flushContext()` — after every `process()` and `finalize()` of a room (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:188,221`).
  Used to clear module-level per-room scratch maps, e.g. creep pull maps
  (`reference/xxscreeps/packages/xxscreeps/mods/creep/processor.ts:27-32`).
- `refreshRoom(shard, room)` — "run on every room when processor continuity has been broken", i.e. at processor
  boot for every room (`reference/xxscreeps/packages/xxscreeps/engine/processor/symbols.ts:21-31`,
  `reference/xxscreeps/packages/xxscreeps/engine/processor/worker.ts:64-71`). Controller rebuilds the
  `controlledRooms/reservedRooms` scratch sets (`reference/xxscreeps/packages/xxscreeps/mods/controller/driver.ts:13-19`).

### XX-MOD-HOOK-DRIVER

- `isolateInspector: boolean` (any true enables the V8 inspector) and `sandboxCreated(sandbox, userId)`
  (`reference/xxscreeps/packages/xxscreeps/driver/symbols.ts:4-7`; used at
  `reference/xxscreeps/packages/xxscreeps/driver/sandbox/isolated/index.ts:19`,
  `reference/xxscreeps/packages/xxscreeps/driver/sandbox/index.ts:7,59`).

### XX-MOD-HOOK-BACKEND

`reference/xxscreeps/packages/xxscreeps/backend/symbols.ts:13-21`:

- `backendReady(db, shard)` once after connect (`reference/xxscreeps/packages/xxscreeps/backend/server.ts:24`).
- `middleware(koa, router)` before routes (`.../backend/server.ts:53`).
- `route: {path, method?, execute}` — HTTP endpoints; helpers `makeValidatedPayloadRoute/QueryRoute` validate
  with ajv and return `{error:'invalid'}` (`reference/xxscreeps/packages/xxscreeps/backend/index.ts:93-119`).
- `subscription: {pattern: RegExp, subscribe(params) -> effect}` — websocket channels (e.g. `mapVisual:`)
  (`reference/xxscreeps/packages/xxscreeps/mods/visual/backend.ts:36-72`).
- `roomSocket(shard, userId, roomName) -> [effect, (time) => object]` — extra fields merged into each room socket
  frame (e.g. `visual`) (`reference/xxscreeps/packages/xxscreeps/mods/visual/backend.ts:7-34`,
  `reference/xxscreeps/packages/xxscreeps/backend/sockets/room.ts:46,139,180`).
- `sendUserInfo(db, userId, info, privateSelf)` — augment user info responses.

### XX-MOD-GLOBALS

- `registerGlobal(fn)` / `registerGlobal(name, value)` -> `defineGlobal(name, {configurable, enumerable, writable, value})`,
  which registers a `runtimeConnector.initialize` that `Object.defineProperty(globalThis, ...)` in the sandbox
  (`reference/xxscreeps/packages/xxscreeps/game/symbols.ts:47-70`). Example: `registerGlobal(Creep)`
  (`reference/xxscreeps/packages/xxscreeps/mods/creep/game.ts:20-21`), `registerGlobal('Spawn', StructureSpawn)`
  (`reference/xxscreeps/packages/xxscreeps/mods/spawn/game.ts:41`), lodash `_`
  (`reference/xxscreeps/packages/xxscreeps/game/runtime.ts:11`).
- Every export of the merged constants module becomes a global at sandbox init
  (`reference/xxscreeps/packages/xxscreeps/game/runtime.ts:16-23`).

### XX-MOD-FINDLOOK

- `registerFindHandlers({[FIND_X]: (room) => readonly any[]})` stores handlers in a global Map keyed by number
  (`reference/xxscreeps/packages/xxscreeps/game/room/symbols.ts:3-13`); `room.find(type)` caches the handler result
  per type and returns a copy or `filter` result (`reference/xxscreeps/packages/xxscreeps/game/room/room.ts:47-53`).
  Unknown FIND type -> `[]`.
- `registerLook<T>()(LOOK_X)` adds a look constant; each object declares `'#lookType'` (and optional
  `'#secondaryLookType'`, e.g. Resource is also `LOOK_ENERGY`) and is indexed into per-look lists on insert
  (`reference/xxscreeps/packages/xxscreeps/game/room/room.ts:234-250`,
  `reference/xxscreeps/packages/xxscreeps/mods/resource/resource.ts:30-31`).
- Example creep mod: FIND_CREEPS / MY / HOSTILE (hostile excludes spawning) / TOMBSTONES built from the look index
  (`reference/xxscreeps/packages/xxscreeps/mods/creep/game.ts:31-43`).

### XX-MOD-SCHEMA-EXT

- `registerStruct(path, members)`, `registerVariant(path, structFormat)`, `registerEnumerated(path, ...values)` all
  append to `schemaByPath[path]` (`reference/xxscreeps/packages/xxscreeps/engine/schema/index.ts:17-36`).
  Consumers splice them in when the schema is built: `structForPath()(path, base)` copies member entries into the
  struct (later mod wins on key collision), `variantForPath()(path)` / `enumeratedForPath()(path)` return arrays
  (`.../engine/schema/index.ts:40-63`).
- Extension paths in use:
  - `'Room'` (extra Room members): controller (`#level`, `#user`, safe mode...), npc (`#npcData`), invader, source,
    powerbank (`reference/xxscreeps/packages/xxscreeps/mods/npc/game.ts:7-21`; consumer
    `reference/xxscreeps/packages/xxscreeps/game/room/schema.ts:10-26`).
  - `'Room.objects'` (object types): every object class (`reference/xxscreeps/packages/xxscreeps/mods/creep/game.ts:51-53`).
  - `'Room.eventLog'` (event variants keyed by `EVENT_*`) (`reference/xxscreeps/packages/xxscreeps/mods/creep/game.ts:56-72`).
  - `'ResourceType'` (extra resource enum values; base enum is `undefined, energy, power`)
    (`reference/xxscreeps/packages/xxscreeps/mods/resource/resource.ts:13-17`).
  - `'ActionLog.action'` (action log names) (`reference/xxscreeps/packages/xxscreeps/game/object.ts:177-182`).
- **GOTCHA**: schema extension must happen at import time of the `game` slot, before `engine/db/room.ts` builds the
  room reader/writer (`reference/xxscreeps/packages/xxscreeps/engine/db/room.ts:1-19`). Because enum and variant
  indices are positional, registration order == binary format.

### XX-MOD-INTENT-REG

`registerIntentProcessor(receiver, intent, {before?, after?, type?, internal?}, process)`
(`reference/xxscreeps/packages/xxscreeps/engine/processor/index.ts:44-71`):

- `receiver`: a class (`Creep`, `StructureSpawn`, `Room`, ...). Lookup by `instanceof`, so a processor on
  `Structure` applies to every structure subclass (e.g. `destroyStructure`,
  `reference/xxscreeps/packages/xxscreeps/mods/structure/processor.ts:12`).
- `before`/`after`: intent names this one must run before/after (only meaningful for the same/related receiver).
- `type`: conflict-group names; intents sharing any group on the same object are mutually exclusive in a tick.
- `internal: true`: only executed from payloads flagged `internal` (backend/system) — XX-PROC-INTERNAL.
- `process(receiver, context, ...args)`: args are the JSON-decoded intent arguments; allowed arg types are
  `string | number | boolean | null | array` because `undefined` becomes `null` through JSON
  (`.../engine/processor/index.ts:36-41`).
- The return value is a phantom type carrier for declaration merging (XX-MOD-TYPES), always `undefined` at runtime.

### XX-MOD-TICK-REG

- `registerObjectPreTickProcessor(Class, fn)` / `registerObjectTickProcessor(Class, fn)` assign
  `Class.prototype[PreTick|Tick] = fn` (`reference/xxscreeps/packages/xxscreeps/engine/processor/index.ts:90-100`).
  **GOTCHA**: one slot per prototype; registering twice overwrites; subclasses inherit the parent's slot unless
  they register their own (prototype chain lookup at `object[Tick]?.(...)`).
- `registerRoomTickProcessor((room, context) => void)` — runs once per processed room, between PreTick and
  intents, in registration order (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:16-18,115-117`).
- `registerShardTickProcessor((shard, time) => Promise|void)`, `everyNTicks(n, fn)` (runs when `time % n === 0`),
  `registerShardInitializer(shard => Promise)` (`reference/xxscreeps/packages/xxscreeps/engine/processor/shard.ts:5-28`) —
  XX-PROC-SHARD.

### XX-MOD-OBSTACLE

- `registerObstacleChecker(params => ((object) => boolean) | null)`; `makeObstacleChecker(params)` ORs all non-null
  checkers; `params = {room, user, checkTerrain?, ignoreCreeps?, ignoreDestructibleStructures?}`
  (`reference/xxscreeps/packages/xxscreeps/game/pathfinder/obstacle.ts:6-35`). `makePositionChecker` adds terrain
  (`.../game/pathfinder/obstacle.ts:41-50`).
- Used by movement (per mover user), spawn placement and pathfinding cost matrices. The creep checker implements
  the safe-mode rule: in a safe-moded room owned by `user`, only the user's own creeps block; otherwise every creep
  blocks (`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:478-492`).

### XX-MOD-STORAGE

- `registerStorageProvider(schemes, 'keyval'|'pubsub', (url, disposition) => Promise<provider>)`; duplicates throw;
  `connectToProvider(uri, disposition)` resolves the URI relative to the config file and dispatches on
  `protocol + disposition` (`reference/xxscreeps/packages/xxscreeps/engine/db/storage/register.ts:18-45`).
  Built-ins: `file:`/`local:` keyval (`reference/xxscreeps/packages/xxscreeps/engine/db/storage/local/keyval.ts:16-19`),
  local pubsub; Redis via mod (XX-DB-REDIS).

### XX-MOD-RENDER

- `bindRenderer(Class, (object, next, previousTime?) => rendered)` installs `prototype[Render]` that calls the
  parent class renderer through `next()` so mods extend the JSON sent to clients
  (`reference/xxscreeps/packages/xxscreeps/backend/index.ts:74-83`). `bindMapRenderer` / `bindTerrainRenderer` set
  single functions (`.../backend/index.ts:85-91`). Example creep renderer adds store, actionLog, `say`, body...
  (`reference/xxscreeps/packages/xxscreeps/mods/creep/backend.ts:10-37`).

### XX-MOD-PROTO

The base `RoomObject` defines overridable engine-private members that act as the cross-mod protocol
("communicate through hooks on shared base types", `reference/xxscreeps/docs/architecture.md:339-361`), defined at
`reference/xxscreeps/packages/xxscreeps/game/object.ts:76-115`:

| Member | Default | Meaning / users |
|---|---|---|
| `'#extraUsers'` | `[]` | users needed for rendering (feeds `#users.extra`) |
| `'#hasIntent'` | `false` | object lets its owner issue intents in this room (creeps, owned structures) |
| `'#invulnerable'` | `false` | |
| `'#layer'` | `0.5` | damage layering: creep 0, structure 0, rampart 1 (`mods/defense/rampart.ts:28`); `captureDamage` walks layers top-down (`game/processor.ts:9-78`) |
| `'#pathCost'` | `undefined` | cost for pathfinding cost matrix (roads) |
| `'#providesVision'` | `false` | owner gets the room blob next tick |
| `'#lookType'` / `'#secondaryLookType'` | abstract / `null` | look indices |
| `'#user'` | `null` | owner id; setter throws on unownable objects |
| `'#addToMyGame'(game)` | no-op | e.g. creep adds itself to `Game.creeps` |
| `'#beforeInsert'(room)` / `'#afterRemove'()` | set/unset `room` | |
| `'#applyDamage'(power, type, source)` | subtract hits, destroy at <= 0 | creep overrides to accumulate `tickRawDamage` |
| `'#captureDamage'(power, type, source)` | returns power | rampart absorbs |
| `'#destroy'(type)` | queue removal | structure adds ruin + event |

Mods add more via declaration merging + prototype assignment, e.g. `'#applyNukeImpact'` default no-op on
`RoomObject.prototype` (`reference/xxscreeps/packages/xxscreeps/mods/nuker/processor.ts:16-24`),
`'#checkHarvest'` (`reference/xxscreeps/packages/xxscreeps/mods/harvestable/game.ts:11-22`),
`'#roomStatusDidChange'` broadcast by the controller (`reference/xxscreeps/packages/xxscreeps/mods/controller/processor.ts:62-71`).
Prototype wrapping is also used: combat wraps `Creep.prototype['#applyDamage']` to add melee hit-back
(`reference/xxscreeps/packages/xxscreeps/mods/combat/creep.ts:96-120`).

### XX-MOD-EXTEND

`extend(Class, props | (parentProto) => props)` defines methods (non-enumerable) and accessors on another mod's
class prototype (`reference/xxscreeps/packages/xxscreeps/utility/utility.ts:67-79`). Examples: `Room.prototype.memory`
from the memory mod (`reference/xxscreeps/packages/xxscreeps/mods/memory/game.ts:28-37`), `Room.getEventLog`
(`reference/xxscreeps/packages/xxscreeps/game/room/event-log.ts:44-51`).

### XX-MOD-INTERMOD

Registries owned by one mod and filled by others:

- `registerBuildableStructure(type, {obstacle, stackable?, checkName?, checkPlacement, create})`
  (`reference/xxscreeps/packages/xxscreeps/mods/construction/index.ts:5-7`, traits at
  `reference/xxscreeps/packages/xxscreeps/mods/construction/symbols.ts:6-15`); used by lab, rampart, tower, wall, factory...
- `registerHarvestable(Class, check)` sets `prototype['#checkHarvest']`; `registerHarvestProcessor(Class, fn)` sets
  the harvest implementation symbol; the single `harvest` intent dispatches to it
  (`reference/xxscreeps/packages/xxscreeps/mods/harvestable/game.ts:17-29`,
  `reference/xxscreeps/packages/xxscreeps/mods/harvestable/processor.ts:21-49`).
- `registerNPC(userId, loop)` + `activateNPC(room, userId)` (`reference/xxscreeps/packages/xxscreeps/mods/npc/processor.ts:8-17`).
- `registerAttackNotification(handler)` (`reference/xxscreeps/packages/xxscreeps/mods/structure/structure.ts:276`),
  `registerSendUserNotifications(fn)` (`reference/xxscreeps/packages/xxscreeps/mods/notifications/transports.ts:13`).

### XX-MOD-TYPES

Typing only (no runtime effect): mods augment `interface Intent`, `Schema`, `Find`, `Look`, `Global`, `Game`,
`Room`, `TickPayload`, `TickResult`, `InitializationPayload` via `declare module` so registrations are type-checked
(e.g. `reference/xxscreeps/packages/xxscreeps/mods/creep/processor.ts:102-104`,
`reference/xxscreeps/packages/xxscreeps/mods/memory/driver.ts:9-26`). Rust equivalent: typed registries; nothing to port.

### XX-MOD-CATALOG

From each `mods/<name>/index.ts` (all under `reference/xxscreeps/packages/xxscreeps/mods/`):

| Mod | Dependencies | Provides | Main contributions |
|---|---|---|---|
| structure | — | backend constants game processor test | `Structure`, `OwnedStructure` (`#active`, `isActive` by controller distance), `Ruin`, destroy/notify intents |
| memory | — | backend driver game test | `Memory`, `RawMemory`, segments, `Room.memory` |
| resource | structure | backend constants game processor test | `Store` (Open/Restricted/Single), `Resource`, `StructureContainer`, `drop()` helper, decay |
| creep | memory resource structure | backend constants game processor test | `Creep`, `Tombstone`, move/pull/transfer/withdraw/pickup/drop/say/suicide, movement, border crossing |
| combat | creep | constants game processor test | attack, rangedAttack, rangedMassAttack, heal, rangedHeal, hit-back |
| harvestable | creep | constants game processor | generic `harvest` intent + registries |
| construction | creep | backend constants game processor test | `ConstructionSite`, build/repair/dismantle, createConstructionSite |
| notifications | creep structure | driver game main processor test | `Game.notify` queue, shard drain every 10 ticks |
| controller | creep notifications structure | backend constants driver game processor test | `StructureController`, claim/reserve/upgrade/attack/sign/safe mode, GCL |
| spawn | construction controller memory resource structure | backend constants game processor test | `StructureSpawn`, `StructureExtension`, place-spawn/respawn routes |
| source | creep harvestable resource structure | backend constants game processor test | `Source`, `StructureKeeperLair` |
| mineral | harvestable resource structure | backend constants game processor test | `Mineral`, `StructureExtractor` |
| chemistry | mineral resource structure | backend constants game processor test | `StructureLab`, reactions, boosts |
| defense | combat resource structure | backend constants game processor test | rampart, tower, wall |
| logistics | resource structure | backend constants game processor test | link, storage |
| market | resource structure | backend constants game processor | terminal `send`; `Game.market` stub |
| factory | construction resource structure | backend constants game processor test | `StructureFactory`, produce |
| deposit | factory harvestable resource | backend game main processor test | `Deposit`, sector placement schedule |
| npc | (none key) | game processor | NPC loop runner |
| invader | combat controller creep defense logistics npc | backend constants game processor test | invader creeps/AI, invader core |
| nuker | construction controller creep defense notifications resource structure | backend constants game processor test | nuker, nuke |
| observer | structure | constants game processor test | observer, ObserverSpy |
| portal | creep structure | backend game processor test | portal |
| powerbank | combat creep resource structure | backend game main processor test | power bank + placement schedule |
| powerspawn | construction controller creep resource structure | backend driver game processor test | power spawn |
| road | structure | backend constants game processor test | road |
| flag | memory | backend constants driver game test | flags (runner-side) |
| visual | — | backend driver game test | room/map visuals |
| intershardResource | — | constants game | `Game.resources` zeros |
| classic | chemistry defense deposit factory flag intershardResource invader logistics market notifications nuker observer portal powerbank powerspawn road source spawn visual | null | meta-mod |
| backend/cookie, backend/steam | — | backend | auth |
| backend/password | — | backend test | auth |

### XX-MOD-TEST

- `test` slot holds per-mod test suites (`mods/*/test.ts`), run by `test/run.ts` which also imports `driver`
  (`reference/xxscreeps/packages/xxscreeps/test/run.ts:3,9`).
- `simulate({roomName: room => mutate})` creates a fresh test shard from `test/data/shard.json`, applies room
  mutations, and exposes `player(userId, Game => ...)`, `poke(room, user, (Game, room) => ...)` (direct mutation,
  saved), `sandbox(userId, main)` (real sandbox), `tick(n, players)` and `peekRoom`
  (`reference/xxscreeps/packages/xxscreeps/test/simulate.ts:43-120`). Example: creep "dies of old age"
  (`reference/xxscreeps/packages/xxscreeps/mods/creep/test.ts:14-31`).

---

## 4. Binary schema and room blobs

### XX-SCHEMA-WHY

Stated goals (`reference/xxscreeps/README.md:12-38`): a room is one blob (250 B – 60 KB typical; a wall is 44 bytes,
a 50-part creep ~274 bytes); blobs are read as `ArrayBuffer` and handed to the player runtime without
deserialization; JS objects are overlaid with lazy getters so untouched fields never enter the V8 heap (less GC);
blobs shard trivially ("like a small JPEG").

### XX-SCHEMA-FORMATS

`reference/xxscreeps/packages/xxscreeps/schema/format.ts` (all combinators return plain descriptor objects):

| Combinator | Meaning | Source line |
|---|---|---|
| primitives | `int8 int16 int32 uint8 uint16 uint32 double bool buffer string` | `format.ts:70-71` |
| `array(n, T)` | fixed-length array, element must have fixed stride | `format.ts:117-124` |
| `compose(T, interceptor)` | value transform: `{compose, decompose}`, raw `{composeFromBuffer, decomposeIntoBuffer}`, or a class (overlay) | `format.ts:127-158` |
| `constant(v)` | stored in 0 bytes | `format.ts:161-164` |
| `declare(name, T)` | named format (reuse, archive, version name) | `format.ts:167-170` |
| `enumerated(...values)` | 1 byte index; > 256 values throws | `format.ts:173-181` |
| `optional(T, uninit?)` | value or `undefined`/`null` | `format.ts:184-194` |
| `struct([base,] members)` | record; `base` = inherited struct prefix | `format.ts:213-229` |
| `union({otherKey: T})` | alias another member's bytes with a different type (e.g. `#posId` over `pos`) | `format.ts:231-237`, `reference/xxscreeps/packages/xxscreeps/game/object.ts:21-25` |
| `variant(name)` / `variant(...structs)` | tag a struct / tagged union | `format.ts:240-257` |
| `vector(T)` | variable-length list | `format.ts:260-265` |

### XX-SCHEMA-LAYOUT

`reference/xxscreeps/packages/xxscreeps/schema/layout.ts` (`kPointerSize = 4`, `kHeaderSize = 16`, line 8-10):

- Sizes/alignment: `bool/int8/uint8` 1, `int16/uint16` 2, `int32/uint32` 4, `double` 8 (align = size)
  (`layout.ts:106-127`). `string`/`buffer`: 8 bytes (int32 offset into heap, int32 length), align 4 (`layout.ts:129-136`).
- `array`: `size = align(elem.size, elem.align)*(n-1) + elem.size`; requires elem stride (`layout.ts:138-156`).
- `enum`: 1 byte; `constant`: 0 bytes (`layout.ts:169-178`).
- `optional`: if inner size <= 8: inline, presence byte at `offset + inner.size`, size `inner.size+1`
  (`layout.ts:191-205`); else a 4-byte pointer (0 = absent) to a heap copy (`layout.ts:206-218`).
- `struct`: members (excluding unions) sorted by size desc, align desc, then name asc (`layout.ts:233-240`); then
  greedily placed: starting at `base.size` (inherited prefix), repeatedly pick the first member needing zero padding,
  else the one with least padding (`layout.ts:256-282`). Struct `size` = last member end; `stride` defined only if
  base and all members have strides (`layout.ts:285-289`). Union members get the referenced member's offset and must
  not have stricter alignment (`layout.ts:292-308`).
- `variant`: 5 bytes = int32 pointer to the payload struct in the heap + uint8 variant index, align 4
  (`layout.ts:323-339`; reader `reference/xxscreeps/packages/xxscreeps/schema/read.ts:216-223`, writer
  `reference/xxscreeps/packages/xxscreeps/schema/write.ts:254-268`). **GOTCHA**: the index is the position in the
  variant list = mod registration order, max 256 variants.
- `vector` of fixed-stride elements: 8 bytes (int32 offset, int32 length), elements contiguous at `stride`
  (`layout.ts:355-367`). `vector` of variable elements ("list"): 4-byte head pointer; each element is preceded by a
  4-byte "next" pointer, 0-terminated (`layout.ts:343-354`; `read.ts:151-163`; `write.ts:196-212`).
- Heap: dynamic data is appended after the root struct; writers return the new heap end (`write.ts:14-15`).

### XX-SCHEMA-STRING

Writer tries latin1 (each UTF-16 unit < 0x100 -> 1 byte, positive length); on the first wide char it restarts as
UTF-16 at a 2-aligned heap offset and stores the NEGATIVE length (`reference/xxscreeps/packages/xxscreeps/schema/write.ts:132-156`);
reader mirrors this (`reference/xxscreeps/packages/xxscreeps/schema/read.ts:84-95`).

### XX-SCHEMA-HEADER

Blob = header `uint32[4] = {0xfff35a00 magic, version, total byte length, 0}` + root struct at offset 16 + heap
(`reference/xxscreeps/packages/xxscreeps/schema/write.ts:312-315`). Readers reject wrong magic / nonzero 4th word
(`Corrupted blob`) and length mismatch (`Truncated blob`), and a version mismatch throws unless upgraded
(`reference/xxscreeps/packages/xxscreeps/schema/read.ts:250-268`).

### XX-SCHEMA-OVERLAY

- `withOverlay(Base, shape)` is a TYPE-ONLY helper returning `Base` (`reference/xxscreeps/packages/xxscreeps/schema/overlay.ts:105-111`).
  The binding happens in `compose(shape, Class)`: the reader instantiates `new Class(view, offset)` and, once per
  prototype, injects getters for every member (`reference/xxscreeps/packages/xxscreeps/schema/read.ts:128-144`,
  `.../schema/overlay.ts:17-86`).
- Getter rule (`.../schema/overlay.ts:43-61`): integer and bool members are read from the buffer on EVERY access
  (never memoized); every other member (strings, ids, positions, vectors, nested structs) is read once and then
  defined as an own writable data property on the instance (memoized).
- Setter rule: assignment defines an own data property shadowing the getter (`.../schema/overlay.ts:66-76`). The
  buffer is never mutated in place; mutations live in the JS object until the next write (XX-SCHEMA-WRITE).
- Enumerable iff key is a string not starting with `#` (`.../schema/overlay.ts:65`).
- `Builder({materialize: true})` reads all members eagerly instead (used for flags, archived upgrades)
  (`.../schema/read.ts:131-138`).
- **GOTCHA** (TypeScript-specific): a class field declared without `declare` emits an own property that shadows
  the getter (`reference/xxscreeps/docs/architecture.md:295-311`). In Rust/our JS engine, the equivalent hazard is
  any per-instance cache that is not invalidated when the underlying snapshot changes.
- **PERF**: reading `creep.hits` is a typed-array load; reading `creep.body` allocates once per tick per creep.

### XX-SCHEMA-WRITE

- `makeWriter(info)(value)` walks the whole value graph through property access (own property or getter) and
  writes into a process-wide 16 MiB scratch buffer, then copies the exact length into a new `SharedArrayBuffer`
  (`reference/xxscreeps/packages/xxscreeps/schema/write.ts:296-330`). With `scanFirst` it pre-computes the length
  (scanner) and writes directly (`write.ts:302-310`). Member writers are unrolled 4-at-a-time (`write.ts:46-84`).
- **PERF**: every room save is a full re-serialization of the room (all objects, all fields), even if one field
  changed. Reads of unmodified fields go back through the old buffer's getters.
- **GOTCHA** (as read, not executed): the scratch buffer is reused and not zeroed; bytes that are not written
  (struct padding, the inner bytes of an absent inline optional, union members) keep stale content, so two equal
  states can serialize to different bytes. Do not hash blobs for determinism checks without canonicalizing.

### XX-SCHEMA-VERSION

- Version = first 4 bytes (LE) of SHA-1 over the archived textual layout (`reference/xxscreeps/packages/xxscreeps/schema/build.ts:28-33`).
- On first build, the layout archive (a JS module) and a Kaitai `.ksy` descriptor are written to
  `schemaArchive/<name>-<version>.js|.ksy` (`reference/xxscreeps/packages/xxscreeps/engine/schema/build/index.ts:18-43`).
- `makeUpgrader`: blob version == expected -> pass through; else load the archived layout, read the old blob with a
  materializing reader, `nullify()` the old view so missing fields read as 0, re-write with the current writer
  (`.../engine/schema/build/index.ts:49-72`). Room blobs are upgraded on load (`reference/xxscreeps/packages/xxscreeps/engine/db/shard.ts:115-118`).
  Inside the sandbox upgrading is unavailable (`reference/xxscreeps/packages/xxscreeps/engine/schema/build/runtime.ts:14`).
- "Zero is the default state": new fields read as zero; design flags so 0 is the common case
  (`reference/xxscreeps/docs/architecture.md:238-240,270-273`).

### XX-SCHEMA-ID

- `Id.optionalFormat`: 16 bytes = uint8 length at byte 0, 3 unused bytes, then 3 x uint32 holding up to 24 hex
  characters (right-aligned in 8-char chunks); length 0 = `null`
  (`reference/xxscreeps/packages/xxscreeps/engine/schema/id.ts:4-53`).
- `generateId(24)`: `Math.random()`-based 32-bit hex chunks (`.../engine/schema/id.ts:58-68`). **GOTCHA**:
  non-deterministic randomness in the processor (object ids of new creeps, resources, ruins...).

### XX-SCHEMA-POS

- `RoomPosition` is an int32 `(y << 24) | (x << 16) | (ry << 8) | rx`, with `rx = 128 + signedX`
  (`W0` -> 127, `E0` -> 128) (`reference/xxscreeps/packages/xxscreeps/game/room/name.ts:3-63`,
  `reference/xxscreeps/packages/xxscreeps/game/position.ts:81-96`; stored as `int32` via `format()` at
  `.../game/position.ts:40-56`). Out-of-range constructor args throw
  `TypeError('Invalid arguments in RoomPosition constructor')`.
- **GOTCHA / scale**: world limited to 256 x 256 rooms (W127..E127, N127..S127). See XX-COMPAT-WORLD-SIZE.

### XX-SCHEMA-ROOM

Room struct (`reference/xxscreeps/packages/xxscreeps/game/room/schema.ts:9-26`): `name: string`,
`'#objects': vector(variant(<all 'Room.objects'>))`, `'#users': {intents, presence, vision, extra: vector(Id)}`,
`'#eventLog': vector(variant(<all 'Room.eventLog'>))`, plus all `registerStruct('Room', ...)` members.
Readers/writers for the room and for a single object (`readRoomObject`/`writeRoomObject`, used to move a creep
between rooms) are built once per process (`reference/xxscreeps/packages/xxscreeps/engine/db/room.ts:11-19`).

### XX-SCHEMA-OBJECT

Base `RoomObject` struct: `id: Id`, `pos: RoomPosition`, `'#posId': union({pos: 'int32'})`
(`reference/xxscreeps/packages/xxscreeps/game/object.ts:20-25`). Subclasses extend with `struct(base, {...variant('creep'), ...})`,
e.g. Creep: `body: vector({boost: optional resource enum, hits: int8, type: enum})`, `fatigue int32`, `hits int32`,
`name string`, `store`, `'#actionLog'`, `'#ageTime' int32`, `'#noAttackNotify' bool`, `'#saying' optional{...}`,
`'#user' Id` (`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:65-86`).

### XX-SCHEMA-TERRAIN

- World blob: `vector({name: string, info: {exits: uint8, terrain}})`, sorted by name, composed into a
  `Map<name, info>` (`reference/xxscreeps/packages/xxscreeps/game/map.ts:17-30`).
- Terrain: 625 bytes per room, 2 bits per tile, `index = y*50 + x`,
  `value = (byte[index >> 2] >> ((index & 3) << 1)) & 3` (`reference/xxscreeps/packages/xxscreeps/game/terrain.ts:52-58`).
- Loaded once per process (runner: `shard.loadWorld()`, processor: `world` request) and shared with every isolate and
  the native pathfinder (`reference/xxscreeps/packages/xxscreeps/engine/service/runner.ts:46-47`,
  `reference/xxscreeps/packages/xxscreeps/engine/processor/worker.ts:58-61`).

### XX-SCHEMA-PRIVATE

- Engine-private fields use string keys starting with `#` (e.g. `this['#ageTime']`). A babel plugin rewrites these
  member accesses into private-symbol accesses when files are loaded with `--private-transform`
  (`reference/xxscreeps/packages/xxscreeps/driver/private/plugin.ts:16-33`,
  `reference/xxscreeps/packages/xxscreeps/config/mods.ts:113-134`).
- In isolated-vm the symbols are V8 private symbols (`ivm.lib.privateSymbol`), undiscoverable by player code;
  in `sandbox: unsafe` they are ordinary `Symbol`s (hidden from enumeration only)
  (`reference/xxscreeps/packages/xxscreeps/driver/private/symbol/isolated-vm.ts`,
  `reference/xxscreeps/packages/xxscreeps/driver/private/symbol/unsafe.ts`,
  `reference/xxscreeps/docs/architecture.md:365-393`).
- Schema code iterates own keys including private symbols (`reference/xxscreeps/packages/xxscreeps/driver/private/runtime.ts:35-54`).

### XX-SCHEMA-DETACH

At the end of every runtime tick each room's `BufferView` is detached: its typed-array properties are replaced by
getters throwing `Accessed a released object from a previous tick[T]`
(`reference/xxscreeps/packages/xxscreeps/driver/runtime/index.ts:206-208`,
`reference/xxscreeps/packages/xxscreeps/schema/buffer-view.ts:37-46`). `intents.save` refuses objects whose buffer is
detached (`Could not find an object with ID ...`) (`reference/xxscreeps/packages/xxscreeps/game/intents.ts:46-53`).
Compatibility impact: XX-COMPAT-STALE-OBJECTS.

---

## 5. Storage and persistence

### XX-DB-PROVIDER

- `KeyValProvider`: Redis-shaped API — strings (`get/set` with `NX/XX/EQ/NE`, `px`, `retain`), `copy`, `del`,
  `vdel` (fire-and-forget), numbers (`incr/decr/...`), hashes, lists (`rPush/lRange/lPop`), sets, sorted sets
  (`zAdd` with `incr`, `GT/LT`, `zRange` by score/lex, `zUnionStore/zInterStore` with weights), `eval(script)`,
  `flushdb`, `save` (`reference/xxscreeps/packages/xxscreeps/engine/db/storage/provider.ts:44-119`).
- `PubSubProvider`: `publish(key, msg)`, `subscribe(key, listener)` (`.../provider.ts:121-132`).
- `KeyvalScript(jsImpl, {lua})`: atomic server-side scripts; the local provider runs the JS source, Redis the lua
  (`reference/xxscreeps/packages/xxscreeps/engine/db/storage/script.ts:8-15`).

### XX-DB-INSTANCES

- `Database.connect()` -> global `{data, pubsub}` (users, code); `Shard.connect(db, name)` -> `{data, pubsub, scratch}`
  plus a `channel/game` subscription that keeps `shard.time` current
  (`reference/xxscreeps/packages/xxscreeps/engine/db/shard.ts:41-79`).
- Default URIs: `./screeps/db?socket=.db`, `./screeps/shard0?socket=.shard0.db` (file keyval + blob dir) and
  `local://...` pubsub/scratch (`reference/xxscreeps/packages/xxscreeps/config/config.ts:284-295`).
- `scratch` holds only per-run bookkeeping and is flushed when main starts (`reference/xxscreeps/packages/xxscreeps/engine/service/main.ts:46-49`).

### XX-DB-LOCAL

- One thread (whoever creates the responder first) hosts `LocalKeyValResponder` = an in-memory `Map` of strings,
  Maps, Sets, `SortedSet`s, Uint8Arrays (`reference/xxscreeps/packages/xxscreeps/engine/db/storage/local/keyval.ts:21-53`).
  Other threads/processes are `ResponderClient`s sending `{requestId, method, payload}` over a worker `MessagePort`
  or a unix socket (`reference/xxscreeps/packages/xxscreeps/engine/db/storage/local/responder.ts:100-157`); the host
  serves up to 16 requests concurrently (`.../local/responder.ts:194`).
- **PERF**: every keyval call from a processor worker or runner is a cross-thread message round-trip plus
  structured clone of arguments/results. Per-room bookkeeping costs several such calls (XX-PERF-SLOW).

### XX-DB-BLOB

Binary values (`Uint8Array`) bypass the Map and go to `BlobStorage`: in-memory cache of `SharedArrayBuffer`
copies tagged with a save generation; `set` copies unless the value is already a SAB and not `retain`
(`reference/xxscreeps/packages/xxscreeps/engine/db/storage/local/blob.ts:154-167`); `get` falls back to reading a
file per key (`blob.ts:104-145`); keys must match `^[a-zA-Z0-9/_-]*[a-zA-Z0-9_-]+$` (`blob.ts:236-241`).

### XX-DB-SAVE

- Main saves `db` and `shard` every `saveInterval` minutes of wall time (default 2) and on shutdown
  (`reference/xxscreeps/packages/xxscreeps/engine/service/main.ts:42,160-163`,
  `reference/xxscreeps/packages/xxscreeps/engine/service/launcher.ts:24-27`).
- Save = whole keyval Map serialized as one JSON document (tmp + rename) (`.../local/keyval.ts:687-720`) and every
  dirty blob written to its own file (tmp + rename, 500 in flight) (`.../local/blob.ts:180-224`).
- **GOTCHA**: there is no per-tick durability; a crash loses everything since the last save, and the JSON keyval
  and blob files are not saved atomically together.

### XX-DB-KEYS

| Store | Key | Type | Meaning | Source |
|---|---|---|---|---|
| shard data | `time` | string | current shard time | `engine/service/main.ts:119` |
| shard data | `terrain` | blob | world terrain blob | `engine/db/shard.ts:93-99` |
| shard data | `rooms` | set | all room names | `engine/service/main.ts:47` |
| shard data | `room0/<room>`, `room1/<room>` | blob | double-buffered room state | `engine/db/shard.ts:158-160` |
| shard data | `user/<id>/memory`, `user/<id>/segment<n>`, `user/<id>/flags`, `user/<id>/defaultPublicSegment` | blob/string | per-user runtime data | `mods/memory/model.ts:29-80`, `mods/flag/model.ts:27-55` |
| scratch | `processor/activeRooms` | zset | rooms processed every tick, score = #players with intents | `engine/processor/model.ts:27` |
| scratch | `processor/inactiveRooms` | zset | sleeping rooms, score = wake tick | `engine/processor/model.ts:28` |
| scratch | `tick<T>/processRooms` | zset | this tick's queue, score = runners still pending | `engine/processor/model.ts:36-37` |
| scratch | `tick<T>/processedRooms`, `tick<T>/processRoomsPending`, `tick<T>/finalizedRoomsPending`, `tick<T>/finalizeExtraRooms` | set/counter | phase bookkeeping | `engine/processor/model.ts:38-45` |
| scratch | `rooms/<room>/intents`, `rooms/<room>/finalIntents` | list of JSON | runner / inter-room intents | `engine/processor/model.ts:46-49` |
| scratch | `user/<id>/intentRooms`, `/presenceRooms`, `/visibleRooms` | set | user<->room relations | `engine/processor/model.ts:29-34` |
| scratch | `activeUsers`, `tick<T%2>/runnerUsers` | set | users to run | `engine/processor/model.ts:276`, `engine/runner/model.ts:65-66` |
| scratch | `user/<id>/controlledRooms`, `/reservedRooms`, `room/<r>/terminalIngress`, `user/<id>/visual<T%2>` | mod data | | `mods/controller/processor.ts:17-18`, `mods/market/processor.ts:21-29`, `mods/visual/model.ts:50` |

(paths relative to `reference/xxscreeps/packages/xxscreeps/`)

### XX-DB-DOUBLEBUF

- Room key `room${time % 2}/${name}` (`reference/xxscreeps/packages/xxscreeps/engine/db/shard.ts:158-160`).
- `loadRoomBlob(name, time)` only for `time ∈ {shard.time-1, shard.time}`; `saveRoomBlob` only for
  `{shard.time, shard.time+1}`; otherwise refresh `time` from storage and throw `Invalid time`
  (`.../engine/db/shard.ts:115-156`).
- `saveRoomBlob` also publishes `didUpdate` on `processor/room/<name>` (`.../engine/db/shard.ts:133-139`).
- `copyRoomFromPreviousTick(name, T+1)` copies slot T into slot T+1 (a blob reference copy in the local provider)
  (`.../engine/db/shard.ts:145-147`); used for unchanged rooms and before sleeping so both slots agree.

### XX-DB-CHANNEL-MUTEX

`Channel<Message>` = JSON-encoded pub/sub topic with `listen`, `listenFor(filter)` (one-shot promise), `publish`,
`subscribe` (`reference/xxscreeps/packages/xxscreeps/engine/db/channel.ts:9-55`). `Mutex` = keyval lock key
`mutex/<name>` + pub/sub wakeups, 10 s lock timeout (`reference/xxscreeps/packages/xxscreeps/engine/db/mutex.ts:9-29`);
main holds `main` for its lifetime and `game` per tick (`reference/xxscreeps/packages/xxscreeps/engine/service/main.ts:23-31,45,75`).

### XX-DB-SORTEDSET

Local `SortedSet` = array of members + `Map` of scores; EVERY `add`/`insert` re-sorts the whole array
(`O(n log n)`), `delete` is `indexOf + splice` (`O(n)`), range scans are linear from the start
(`reference/xxscreeps/packages/xxscreeps/engine/db/storage/local/sorted-set.ts:18-60,94-99`). Ties sort by member
name (`sorted-set.ts:95-98`) — so rooms with equal score are popped in lexicographic order.
**PERF**: the per-tick room queue is such a set and receives one `zAdd incr` per (player, intent room) per tick.

### XX-DB-REDIS

`@xxscreeps/redis` provides `storage` and registers `redis:` for keyval and pubsub; Redis >= 8.2 required for the
mutex (`SET ... IFEQ`) (`reference/xxscreeps/packages/redis/storage.ts:1-6`, `reference/xxscreeps/packages/redis/README.md:1-24`).
Blob values are copied into new `SharedArrayBuffer`s on read (`reference/xxscreeps/packages/redis/keyval.ts:22-30`).

---

## 6. Tick pipeline (main loop)

### XX-SVC-LAUNCH

Covered in XX-ARCH-SERVICES. `Worker.create(module)` runs `xxscreeps.js` as a trampoline and wires local pub/sub +
responder ports into each worker (`reference/xxscreeps/packages/xxscreeps/utility/worker.ts:6-23`).

### XX-SVC-BOOT

1. Main acquires `main` mutex, reads `rooms`, flushes scratch, `sAdd('initializeRooms', rooms)`
   (`reference/xxscreeps/packages/xxscreeps/engine/service/main.ts:45-50`).
2. Processor service waits for `mainConnected`, creates workers, sends each the world blob, then the workers drain
   `initializeRooms`: each room is loaded (no index init), `updateUserRoomRelationships(shard, room)` (with no
   previous -> every room is added to `processor/activeRooms`) and `refreshRoom` hooks run
   (`reference/xxscreeps/packages/xxscreeps/engine/service/processor.ts:40-124`,
   `reference/xxscreeps/packages/xxscreeps/engine/processor/worker.ts:64-71`,
   `reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:278-281`).
3. Main waits for `processorInitialized` with `zCard(activeRooms) == rooms.length`, then
   `begetRoomProcessQueue(shard, time)` (`.../engine/service/main.ts:55-71`), then `runShardInitializers` once
   (`.../engine/service/main.ts:138`).
- **GOTCHA**: after every restart every room is processed at least once (all start active), then rooms without
  players fall asleep (XX-PROC-SLEEP).

### XX-SVC-TICK

`tick()` (`reference/xxscreeps/packages/xxscreeps/engine/service/main.ts:74-132`):

1. Acquire `game` mutex; `time = shard.time`.
2. In parallel: `scratch.copy('activeUsers', 'tick{time%2}/runnerUsers')` and publish `process{time}` (no room list
   -> every idle worker starts draining rooms whose score is already 0) (`main.ts:79-84`); then publish `run{time}`
   (`main.ts:85`).
3. Arm the abandon timer (XX-SVC-ABANDON) (`main.ts:88-95`).
4. Loop on service messages: re-publish `process`/`run` if a processor/runner (re)connects; on `tickFinished`:
   `runShardTickProcessors(shard, time)` (`main.ts:113`), then in parallel `begetRoomProcessQueue(shard, time+1)`
   and `set('time', time+1)` + publish `tick{time+1}` + `shard.time = time+1` (`main.ts:114-123`).
5. Outside: log tick duration; save if due; sleep `max(0, tickSpeed - elapsed)` (interruptible) (`main.ts:147-175`).

### XX-SVC-ABANDON

After `processor.intentAbandonTimeout` ms (default 5000): `zInterStore(queue, [queue], weights [0])` sets every
pending count to 0, deletes the runner queue, publishes `process{time}` (`reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:296-309`).
A runner that publishes later sees `count < 0`, resets it to 0 and its intents stay in `rooms/<r>/intents`, so
they are consumed by that room's NEXT processing (`.../engine/processor/model.ts:116-122`) — XX-COMPAT-LATE-INTENTS.
A runner whose `time` no longer equals `shard.time` throws `User ... has been left behind` and the player is
reset (`reference/xxscreeps/packages/xxscreeps/engine/runner/instance.ts:179-184`).

### XX-SVC-TICKSPEED

`game.tickSpeed` (ms, default 250) is re-read from `.screepsrc.yaml` via `fs.watch` and applied immediately
(`reference/xxscreeps/packages/xxscreeps/engine/service/tick.ts:7-37`, `.../engine/service/main.ts:33-38`).

---

## 7. Processor

### XX-PROC-QUEUE

- `begetRoomProcessQueue(shard, T)` (atomic group) (`reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:166-201`):
  1. move sleeping rooms with wake score `<= T` into `activeRooms` with weight 0 (keeps existing scores);
  2. copy `activeRooms` (score = # non-system users with intent rights in the room) to `tick<T>/processRooms`;
  3. `processRoomsPending = card(queue)`; `processedRooms = members(queue)`;
  4. if there are zero rooms: publish `tickFinished` immediately.
- Runner completion for user U (`.../engine/processor/model.ts:106-130`): for each of U's intent rooms,
  `zAdd(queue, [-1, room], {if: 'XX', incr: true})` and `rPush rooms/<room>/intents JSON{userId, intents}`; rooms whose
  count reached 0 are published in one `process{time, roomNames}` message.
- Workers pop rooms with score exactly 0 using an atomic "pop lowest by score" script
  (`reference/xxscreeps/packages/xxscreeps/engine/db/async.ts:59-91`), ties lexicographic (XX-DB-SORTEDSET).
- Consequence: a room is processed as soon as all of its intent-capable players have run. Rooms with only system
  users (score 0) are processed at tick start, before any runner finishes.
- **PERF** (key idea): run/process overlap and per-room readiness instead of a global barrier.
- **GOTCHA**: rooms where a player only has vision (no `#hasIntent` object) do not wait for that player.

### XX-PROC-WORKERS

- Worker count = `clamp(1, processor.concurrency, singleThreaded ? 1 : ceil((sCard(users) - 3) / 2))`
  (`reference/xxscreeps/packages/xxscreeps/engine/service/processor.ts:60-62`). **PERF**: with 2 players you get
  one processor worker regardless of core count.
- On `process`: workers with affinity for the announced rooms get `checkAffinity = true`; then idle workers are
  activated (one per announced room, or all if no list) and loop `consumeRoomsQueue` -> `worker.responder({type:
  'process', roomName, time})` (`.../engine/service/processor.ts:132-171`).
- `consumeRoomsQueue` first tries the worker's affinity rooms (rooms it processed last tick), then any ready room,
  re-checking affinity when flagged (`.../engine/service/processor.ts:84-111`). After `finalize`, each worker's
  affinity = rooms it processed this tick (`.../engine/service/processor.ts:190-197`).

### XX-PROC-ROOMCACHE

The worker keeps the `Room` objects it processed this tick in `nextRoomCache`; at the next tick, if
`time == cacheTime + 1`, it reuses the in-memory object instead of `loadRoom` (`reference/xxscreeps/packages/xxscreeps/engine/processor/worker.ts:75-98`).
**PERF**: avoids blob decode for hot rooms. **GOTCHA**: non-schema JS properties set on objects during processing
(e.g. `controller.upgradePowerThisTick`, `creep.tickRawDamage`) survive on the cached object but vanish when the
room migrates to another worker or is reloaded; all such fields must be reset by the processor every tick, or the
simulation becomes worker-dependent. chitin: keep ALL state in the schema.

### XX-PROC-PHASES

`RoomProcessor.process(isFinalization = false)` (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:94-189`):

```
receivedUpdate = false; nextUpdate = Infinity                                  (95-96)
runWithState(state(T+1, [room])):
  room.#eventLog = []                                                          (100)
  for ii in 0 .. len(#objects) at loop start: obj[PreTick]?.(obj, ctx)          (103-111)
  room.#flushObjects(state)                                                    (112)
  for fn in roomTickProcessors (registration order): fn(room, ctx)              (115-117)
  room.#flushObjects(state)                                                    (118)
  for (user, payload) in intents (Map insertion order): runAsUser(user):        (121-163)
      local (Room) intents, then object intents  -- see XX-PROC-INTENT-DISPATCH
  room.#flushObjects(state)                                                    (164)
  Movement.dispatch(room)                                                      (167)
  for ii in 0 .. len(#objects) at loop start: obj[Tick]?.(obj, ctx)             (168-175)
  room.#flushObjects(state)                                                    (176)
await flushTasks()                                                             (180)
if !isFinalization: publish inter-room intents; roomDidProcess(T)              (183-187)
flushContext hooks                                                             (188)
```

- Objects inserted during PreTick/Tick (e.g. a dropped resource inserted "now") are NOT visited in the same loop
  because the loop bound is captured first (`room.ts:103-111,168-175`).
- `runAsUser(user)` sets `me` (so `.my` is relative to the issuing user) and flushes every room's find cache and
  the path cost-matrix cache (`reference/xxscreeps/packages/xxscreeps/game/index.ts:47-59`).

### XX-PROC-GAMETIME

The processor builds `GameState(world, T+1, [room])` (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:91`)
while the runtime uses `GameState(world, T, rooms)` (`reference/xxscreeps/packages/xxscreeps/driver/runtime/index.ts:111`).
So inside processors `Game.time` is the tick being produced. Expiry fields (`#ageTime`, `#nextDecayTime`,
`#downgradeTime`, `#spawnTime`...) are absolute times and the accessors compute `time > Game.time ? time - Game.time
: 0/undefined` (`reference/xxscreeps/packages/xxscreeps/game/object.ts:144-167`). **GOTCHA** when porting xxscreeps
formulas: subtract 1 relative to vanilla processor code that uses `gameTime` (vanilla processor `gameTime` is the
pre-increment time, e.g. `reference/engine/src/processor/intents/creeps/tick.js:87-95`).

### XX-PROC-INTENT-ORDER

`initializeIntentConstraints()` once per process (`reference/xxscreeps/packages/xxscreeps/engine/processor/index.ts:102-168`):

- `cmp(a, b)`: 0 if receivers are unrelated (neither same class nor subclass); -1 if `a.before ∋ b.intent` or
  `b.after ∋ a.intent`; +1 for the converse; else 0 (`index.ts:103-125`).
- A "modified selection sort" over the registration-ordered list (`index.ts:136-150`); `priority` = final index
  (`index.ts:155`). **GOTCHA**: not a guaranteed topological sort (no transitive closure, result depends on
  registration order); unrelated intents keep a registration-dependent order.
- `mask` = OR of one bit per distinct `type` name, bits assigned in first-seen order (`index.ts:152-154`).
- Getter per intent name: intended "single processor -> constant", but the condition is `infos.length === 0`
  (never true), so lookup is always `infos.find(info => instance instanceof info.receiver)` (`index.ts:160-167`).
  **PERF** minor; behaviour still correct.

Constraints declared by the classic mods (creep receiver) — compare with vanilla:

| Intent | xxscreeps constraints | Source |
|---|---|---|
| drop | before transfer | `mods/creep/processor.ts:107` |
| transfer | before withdraw | `mods/creep/processor.ts:273` |
| withdraw | before pickup | `mods/creep/processor.ts:289` |
| pull | before move | `mods/creep/processor.ts:247` |
| harvest | before move; type primary | `mods/harvestable/processor.ts:33-36` |
| attack | before harvest; primary | `mods/combat/processor.ts:22-25` |
| heal | before attack, rangedHeal; primary | `mods/combat/processor.ts:109-112` |
| rangedHeal | after heal; before attackController, rangedMassAttack, repair; primary+laser | `mods/combat/processor.ts:130-134` |
| rangedAttack | after build; laser | `mods/combat/processor.ts:45-48` |
| rangedMassAttack | before rangedAttack; laser | `mods/combat/processor.ts:68-71` |
| build | before attack, harvest, rangedMassAttack; primary+laser | `mods/construction/processor.ts:45-48` |
| repair | before build, rangedMassAttack; primary+laser | `mods/construction/processor.ts:111-114` |
| dismantle | before repair; primary | `mods/construction/processor.ts:79-82` |
| attackController | before dismantle, attack, harvest; primary | `mods/controller/processor.ts:79-82` |
| claimController | before reserveController | `mods/controller/processor.ts:102` |
| upgradeController | after build | `mods/controller/processor.ts:184` |
| tower attack/heal/repair | heal before repair before attack; primary | `mods/defense/processor.ts:20,41-44,63-66` |

(paths relative to `reference/xxscreeps/packages/xxscreeps/`). Vanilla instead runs a FIXED order
`drop, transfer, withdraw, pickup, heal, rangedHeal, dismantle, attack, harvest, move, repair, build,
rangedMassAttack, rangedAttack, say, suicide, claimController, upgradeController, reserveController,
attackController, generateSafeMode, signController, pull` with an explicit blocking table
(`reference/engine/src/processor/intents/creeps/intents.js:3-35`). The exclusion sets coincide for the creep combat
group (both suppress on *issued*, not successful, intents), but execution order differs — XX-COMPAT-INTENT-ORDER.

### XX-PROC-INTENT-DISPATCH

Inside one room, for each user payload (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:121-163`):

1. Users are iterated in `Map` insertion order = order in which their JSON payloads were `rPush`ed (runner
   completion order), then NPC payloads added by room tick processors (`.../engine/processor/worker.ts:102-104`,
   `reference/xxscreeps/packages/xxscreeps/mods/npc/processor.ts:42-46`). XX-COMPAT-USER-ORDER.
2. Multiple payloads of the same user are merged: local lists concatenated; object intents merged per id, later
   payload overwriting same-named intents (`.../engine/processor/room.ts:242-261`).
3. Local (Room-receiver) intents first: for each intent name (payload key order), for each argument tuple in
   order; skipped if the processor is `internal` and the payload is not (`room.ts:125-133`).
4. Object intents: for each id in payload order, `Game.getObjectById(id)` (this room only; missing -> skipped);
   entries = intents with a processor for this receiver class; sorted by `priority`; for each: skip if internal
   and payload not internal; skip if `mask & processor.mask`; else `mask |= processor.mask` BEFORE calling
   `process` (`room.ts:136-161`). So conflicts suppress on attempt, not on success.
5. Every processor re-runs the same `check*` used by the runtime and returns silently on failure
   (XX-PROC-DUAL-VALIDATION). Processors generally use `Game.getObjectById(id)!` and rely on the check to reject
   null targets (e.g. `reference/xxscreeps/packages/xxscreeps/mods/creep/processor.ts:237-245`).

### XX-PROC-INTENT-RUNTIME

`IntentManager` in the sandbox (`reference/xxscreeps/packages/xxscreeps/game/intents.ts:14-96`):

- `save(object, intent, ...args)`: throws if the object's buffer is detached; stores under
  `intentsByRoom[object.room.name].object[object.id][intent] = args` — one entry per (object, intent); a later call
  OVERWRITES the arguments; `cpu += 0.2` only when the slot was empty; returns `OK` (`intents.ts:46-60`).
- `remove(object, intent)` -> `OK` and `cpu -= 0.2`, else `ERR_NOT_FOUND` (backs `cancelOrder`) (`intents.ts:65-77`).
- `pushLocal(room, intent, ...args)` appends (room-level, e.g. `createConstructionSite`), `cpu += 0.2` (`intents.ts:81-88`).
- After the loop, the runtime emits `intentPayloads[roomName]` for each visible room that has intents
  (`reference/xxscreeps/packages/xxscreeps/driver/runtime/index.ts:190-199`), but the runner forwards only the
  payloads of the user's `intentRooms` (`reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:109-115`)
  — **GOTCHA**: intents recorded for a visible-only room are silently dropped (XX-COMPAT-VISION-INTENTS).

### XX-PROC-DUAL-VALIDATION

The player API method calls `chainIntentChecks(() => checkX(...), () => intents.save(...))` — first non-OK result
wins (`reference/xxscreeps/packages/xxscreeps/game/checks.ts:5-13`); the processor calls the same `checkX` and only
mutates on `OK` (`reference/xxscreeps/docs/architecture.md:135-166`). Example: `Creep.transfer`
(`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:409-422`) and the `transfer` processor
(`reference/xxscreeps/packages/xxscreeps/mods/creep/processor.ts:273-287`). The runtime-side check is UX only; the
processor never trusts the runner.

### XX-PROC-MOVEMENT

`reference/xxscreeps/packages/xxscreeps/engine/processor/movement.ts` + the creep `move` processor
(`reference/xxscreeps/packages/xxscreeps/mods/creep/processor.ts:115-228`):

```
announce(obj, dir, initial):                                             (movement.ts:48-72)
    (x,y) = obj.pos + offset(dir); if outside 0..49: return
    moves[obj] = {pos, id = (y<<8)|x, initial, resolved=false, moved=false}; byLocation[id].push(move)

dispatch(room):                                                          (movement.ts:74-173)
    for move in moves: if obj.room gone or !move.initial(commit, look): drop move
        // BUG as read: drop deletes byLocation[move.id] (all moves into that tile), movement.ts:94
    sort moves by priority desc, ties by Math.random() - 0.5           (movement.ts:168-169)
    for move: resolve(move, stack=[])
resolve(move, stack):
    if move.resolved: false; if stack[0] is move.obj: true (cycle closes); if stack has obj: false
    if terrain wall at target and no road: resolved, false
    push obj; ok = for o in lookAt(target) that is an obstacle for mover's user:
                        m = moves[o]; fail unless m && (m.moved || (!m.resolved && resolve(m, stack)))
                   and no already-moved conflicting obstacle moved into the same tile
    pop
    if stack empty or ok: resolved = true; moved = ok
    if ok: move.dispatch(target)
```

Creep `initial` callback (`creep/processor.ts:119-226`):
- If this creep pulls a creep that is moving onto this creep's current tile -> power `-Infinity`, weight 0 (it is
  pushed by the caboose); pull cycles are broken (`creep/processor.ts:123-139`).
- Else power = sum over the pull chain (towards pullers whose announced target equals the puller's position) of
  `calculatePower(member, MOVE, 1, 'fatigue')` (active MOVE parts x fatigue boost), weight = sum of
  `calculateWeight(member)` (`creep/processor.ts:140-159`). Power 0 -> no move.
- priority = `weight ? -weight/power : power`, minus 500 if the room is in safe mode owned by another user
  (`creep/processor.ts:165-176`).
- On success: `#moveObject`; base fatigue road 1 (and road `#nextDecayTime -= ROAD_WEAROUT * body.length`, floored
  at `Game.time`), swamp 10, plain 2; trample hostile construction sites (drop `floor(progress/2)` energy if
  progress > 1) unless a foreign safe mode is active; fatigue `+= weight * base` added to the HEAD of the pull
  chain (`creep/processor.ts:179-225`).
- `calculateWeight` = count of non-MOVE non-CARRY parts (dead included) + `ceil(usedCapacity / CARRY_CAPACITY)`
  (`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:670-675`).
- Fatigue recovery happens in the creep Tick processor: `power = calculatePower(MOVE, 2, 'fatigue')`, subtract from
  own fatigue then along the puller chain (`creep/processor.ts:345-360`).
- **PERF**: room-local; uses module-level maps cleared per dispatch (safe because the synchronous phase is not
  interleaved). **GOTCHA**: differs from vanilla in algorithm and randomness — XX-COMPAT-MOVE, XX-COMPAT-WEIGHT.

### XX-PROC-CONTEXT

`ProcessorContext` (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:34-65`, impl `room.ts:263-289`):

| Method | Semantics |
|---|---|
| `didUpdate()` | mark room dirty -> blob saved at finalize; otherwise the T blob is copied forward |
| `setActive()` | `didUpdate()` + `wakeAt(T+1)` |
| `wakeAt(t)` | `t == 0` ignored; `t < T+1` throws `Invalid wake time`; keeps the minimum |
| `sendRoomIntent(room, intent, ...args)` | queue an inter-room intent if the room exists in the world map (else silently dropped) |
| `task(promise, finalize?)` | register async work; `finalize(result)` runs later under the issuing user |
| `state`, `shard` | the GameState (time T+1) and storage handles |

### XX-PROC-TASKS

- After the synchronous phase, `flushTasks` awaits all task promises; if any has a finalizer, finalizers run in a
  fresh `runWithState`, grouped by user under `runAsUser`; repeats until no tasks remain
  (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:291-318`).
- This is the only sanctioned way for room code to touch global/other-room state:
  - GCL: `hincrBy(user info, 'gcl', upgradePower)` (`reference/xxscreeps/packages/xxscreeps/mods/controller/processor.ts:270`);
  - controlled/reserved room sets (`.../mods/controller/processor.ts:21-57`);
  - notifications (`.../mods/controller/processor.ts:216`);
  - terminal send: reads the destination room's STORED blob (state T) and an `incrBy` reservation counter, then the
    finalizer debits the source terminal and sends an inter-room `terminalSend`
    (`reference/xxscreeps/packages/xxscreeps/mods/market/processor.ts:14-47`);
  - invader spawn: loads neighbour rooms to filter exits (`reference/xxscreeps/packages/xxscreeps/mods/invader/processor.ts:23-...`).
- **GOTCHA**: tasks see other rooms' previous-tick state; results depend on async completion only through
  commutative operations (increments, set adds). Non-commutative cross-room logic is not expressible safely.

### XX-PROC-INTERROOM

- Queue: `sendRoomIntent` buffers per destination; after `process()` each destination gets
  `sAdd(tick<T>/processedRooms, dest)` and `rPush(rooms/<dest>/finalIntents, JSON)`; if `sAdd` returned 1 (dest not
  already being processed this tick) dest is added to `tick<T>/finalizeExtraRooms`
  (`reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:132-143`).
- `roomDidProcess` decrements `processRoomsPending`; at 0: `finalizedRoomsPending = SCARD(processedRooms)`, publish
  `finalize{T}` (`.../engine/processor/model.ts:203-219`).
- On `finalize`: every worker that processed rooms finalizes its own contexts (`finalize(false)`); then all workers
  race to pop `finalizeExtraRooms`: load blob T, new `RoomProcessor`, `process(true)` (FULL PreTick/RoomTick/Tick
  processing, no publishing), `finalize(true)` (`reference/xxscreeps/packages/xxscreeps/engine/processor/worker.ts:114-132`).
  `roomsDidFinalize` decrements by count; at 0 publish `tickFinished` (`.../engine/processor/model.ts:221-234`).
- Inter-room intents are dispatched to Room-receiver processors WITHOUT the internal gate, in list order
  (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:205-210`). Built-ins: `import` (insert an object
  serialized with `writeRoomObject`, used by creep border crossing and portals)
  (`reference/xxscreeps/packages/xxscreeps/engine/processor/movement.ts:21-25`), `observerObserve`
  (`reference/xxscreeps/packages/xxscreeps/mods/observer/processor.ts:20-24`), `terminalSend`
  (`reference/xxscreeps/packages/xxscreeps/mods/market/processor.ts:47-52`), `nukeArrive`
  (`reference/xxscreeps/packages/xxscreeps/mods/nuker/processor.ts:54-58`).
- Ordering: arrival rooms apply imports AFTER their own Tick processors, so an arriving creep is not ticked in the
  arrival tick; this mirrors vanilla where `interRoom` moves happen in the global phase after all rooms
  (`reference/engine/src/processor/global.js:31-43`). Order among several inter-room intents into one room =
  `rPush` order (worker completion order). **GOTCHA**: nondeterministic.
- **GOTCHA**: a sleeping room that receives an inter-room intent gets a full `process(true)` that tick, so its
  object Tick processors run on a tick they would otherwise skip.

### XX-PROC-FINALIZE

`finalize(didWake)` (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:191-240`):

1. Fetch `finalIntents` and await phase-1 task results (`room.ts:192-195`).
2. If there are inter-room intents or task finalizers: run phase-1 finalizers, then apply inter-room intents (`room.ts:196-211`).
3. `flushTasks()`, `#flushObjects`, `flushUsers(room)` (recompute `#users`, XX-PROC-USERS), `flushContext` (`room.ts:215-221`).
4. In parallel: `updateUserRoomRelationships`; `saveRoom(T+1)` if `receivedUpdate` else `copyRoomFromPreviousTick(T+1)`;
   if no players and `didWake` and `nextUpdate == T+1`: `zAdd(activeRooms, [0, room])` (`room.ts:223-234`).
5. If no players and `nextUpdate != T+1`: `sleepRoomUntil(nextUpdate - 1)` (after saving) (`room.ts:236-239`).

### XX-PROC-SLEEP

- A room with at least one non-system intent user stays in `activeRooms` forever (score = player count)
  (`reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:278-281`).
- Otherwise it sleeps unless something requested `T+1`: `sleepRoomUntil` copies the blob into the T+1 slot, removes
  it from `activeRooms`, and (unless `Infinity`) `zAdd(inactiveRooms, [wake, room], {up: 'LT'})`
  (`.../engine/processor/model.ts:284-294`).
- Wake sources: `wakeAt` deadlines (stored as `nextUpdate - 1`, picked up when `begetRoomProcessQueue(T)` sees
  `score <= T`, so the room is processed at the tick whose processor time equals the requested time);
  `pushIntentsForRoomNextTick` (score `shard.time + 1`) used by backend/shard processors
  (`.../engine/processor/model.ts:93-104`); inter-room intents (extra rooms).
- Typical wakers: creeps `wakeAt(#ageTime)`, tombstone decay, container decay, controller downgrade, action-log
  expiry, saying expiry (`reference/xxscreeps/packages/xxscreeps/mods/creep/processor.ts:34-49,306-321,378`,
  `reference/xxscreeps/packages/xxscreeps/mods/resource/processor/container.ts:8-28`). Any dropped `Resource` calls
  `setActive()` every tick until it is gone (`reference/xxscreeps/packages/xxscreeps/mods/resource/processor/resource.ts:45-51`).
- **PERF**: the main scalability lever for big worlds: empty rooms cost nothing per tick.

### XX-PROC-USERS

- `flushUsers(room)` recomputes `#users`: `presence` (every object `#user` + room owner), `intents` (`#hasIntent`),
  `vision` (`#providesVision`), `extra` (`#extraUsers`) (`reference/xxscreeps/packages/xxscreeps/game/room/room.ts:284-316`).
- `updateUserRoomRelationships(shard, room, previous)` diffs players (system users removed) and updates
  `user/<id>/intentRooms|presenceRooms|visibleRooms`, `sAdd activeUsers` for newly added intent players, and sets
  `activeRooms` score = intent player count when the intent set changed or on first call
  (`reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:237-282`).
- The runner uses `intentRooms` (where to publish) and `visibleRooms` (which blobs to load); users with no intent
  rooms are removed from `activeUsers` (`reference/xxscreeps/packages/xxscreeps/engine/service/runner.ts:101-108`).
- **GOTCHA**: vision changes caused during tick T (new creep, observer spy) affect what the user sees at T+1.

### XX-PROC-SHARD

- Shard tick processors run in the main loop after every room finalized and before `time` advances, all in
  parallel (`Promise.all`) (`reference/xxscreeps/packages/xxscreeps/engine/processor/shard.ts:26-28`,
  `reference/xxscreeps/packages/xxscreeps/engine/service/main.ts:113`). They may not touch room objects directly;
  they push internal intents for the next tick. Examples: notifications drain every 10 ticks
  (`reference/xxscreeps/packages/xxscreeps/mods/notifications/main.ts:9-48`), power bank placement
  (`reference/xxscreeps/packages/xxscreeps/mods/powerbank/main.ts:30-60`, `Math.random` rolls at `main.ts:16-24`),
  deposits (`reference/xxscreeps/packages/xxscreeps/mods/deposit/main.ts`).
- Shard initializers run once before the first tick (`reference/xxscreeps/packages/xxscreeps/engine/service/main.ts:138`).
- **PERF**: this is xxscreeps' only "global phase"; it is single-threaded in the main service.

### XX-PROC-NPC

- The npc mod's room tick processor runs, for each NPC user in `room['#npcData'].users`, the registered loop with
  per-(room, NPC) memory stored in the room blob (`#npcData.memory`), using `runForUser` (a real `Game` + intent
  manager); intents for this room are merged via `context.saveIntents`; a loop returning false deactivates the NPC
  (`reference/xxscreeps/packages/xxscreeps/mods/npc/processor.ts:20-48`, schema `reference/xxscreeps/packages/xxscreeps/mods/npc/game.ts:7-21`).
- Invader: `registerNPC('2', loop)` (`reference/xxscreeps/packages/xxscreeps/mods/invader/processor.ts:20`); spawn
  trigger based on `#cumulativeEnergyHarvested` with `Math.random` goal rolls and a neighbour-room check via `task`
  (`.../mods/invader/processor.ts:23-...`).
- **PERF**: NPC AI is room-local and runs without an isolate.

### XX-PROC-INTERNAL

- Processors registered with `internal: true` run only for payloads with `internal: true`
  (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:128,153`). Payloads from player runtimes never
  carry it; they come from the backend or shard processors via `pushIntentsForRoomNextTick(..., {internal: true})`.
- Examples: `placeSpawn`, `unspawn` (`reference/xxscreeps/packages/xxscreeps/mods/spawn/processor.ts:68-110`),
  `requestInvader`, `placePowerBank` (pushed as user `'1'`, `reference/xxscreeps/packages/xxscreeps/mods/powerbank/main.ts:53`),
  `placeDeposit`, and the inter-room ones.

### XX-PROC-OBJECTS

`reference/xxscreeps/packages/xxscreeps/game/room/room.ts`:
- `#insertObject(obj, now=false)`: `#beforeInsert`; `now` -> push to `#objects` + indices immediately (used by
  `drop()` so later code sees the pile), else queued (`room.ts:159-168`).
- `#removeObject(obj)` queues; returns whether it was newly queued (`room.ts:173-177`).
- `#flushObjects(state)`: apply queued inserts (indices + `state.insertObject`) then removals (swap-with-last,
  order NOT preserved), recursing while hooks queue more (`room.ts:107-154`). **GOTCHA**: `#objects` order changes on
  removal — any order-dependent iteration (PreTick/Tick order) diverges from vanilla object order.
- `#moveObject(obj, pos)`: immediate spatial-index update (`room.ts:183-200`).
- Indices: `#spatialIndex` (posId -> objects), `#lookIndex` (lookType -> objects), `#findCache` (`room.ts:23-31,234-271`).
- `#immediateObjects()` = objects minus queued removals plus queued inserts (`room.ts:72-80`).

### XX-PROC-EVENTLOG

- Stored in the room blob (`#eventLog`), reset at the start of `process()` (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:100`),
  appended with `appendEventLog` (`reference/xxscreeps/packages/xxscreeps/game/room/event-log.ts:24-29`);
  `Room.getEventLog(raw)` maps entries to `{event, objectId, data?}` (`event-log.ts:16-21,44-51`).
- Resetting does not call `didUpdate` — XX-COMPAT-EVENTLOG.

### XX-PROC-ACTIONLOG

`#actionLog` = vector `{type enum, x int8, y int8, time int32}`; `saveAction` updates the existing entry of the same
type or appends (`reference/xxscreeps/packages/xxscreeps/game/object.ts:177-199`). PreTick drops entries older than 10
ticks and schedules `wakeAt(min(time) + 10)` (`reference/xxscreeps/packages/xxscreeps/mods/creep/processor.ts:34-49`).

---

## 8. Runner and player runtime

### XX-RUN-SCHED

`reference/xxscreeps/packages/xxscreeps/engine/service/runner.ts:57-126`:

1. On `run{T}`: affinity iterator = users that already have a `PlayerInstance` (popped from `tick<T%2>/runnerUsers`
   one by one with an atomic script) (`runner.ts:69-72`).
2. When affinity users are exhausted and the queue is non-empty, wait `migrationTimeout` (50 ms) or until
   `tickFinished` (lets another runner process with a warm sandbox take them) (`runner.ts:74-86`).
3. Then any remaining user (`sPop`).
4. `Fn.divide(iterable, maxConcurrency)`: `maxConcurrency` lanes pull from the shared iterator
   (`runner.ts:88-90`, `reference/xxscreeps/packages/xxscreeps/functional/asyncIterable/divide.ts:8-36`);
   `maxConcurrency = runner.concurrency` (cpus+1) or 1 in `unsafe` mode (`runner.ts:42`).
5. Per user: create instance if needed; read `intentRooms`/`visibleRooms`; no intent rooms -> remove from
   `activeUsers`, skip; else `instance.run(T, visible, intent)` (`runner.ts:91-113`).
6. Instances not seen this tick are disposed ("migrated") (`runner.ts:116-122`).

### XX-RUN-INSTANCE

`reference/xxscreeps/packages/xxscreeps/engine/runner/instance.ts`:
- Subscribes to `runner/<user>` (eval / backend intents) and the code channel; code `switch`/`update` of the active
  branch marks the instance `stale` (`instance.ts:79-111`).
- `run()`: stale -> dispose sandbox, create a new one, and SKIP this tick ("player's fault") (`instance.ts:138-164`);
  no sandbox -> load code blobs + connectors `initialize`, `createSandbox`.
- Payload: `backendIntents`, `eval`, `cpu {bucket: floor(bucket), limit: 100, tickLimit: min(500, bucket)}`, `time`
  (`instance.ts:168-178`); throws "left behind" if `T != shard.time` (`instance.ts:182-184`); loads visible room
  blobs at T (`instance.ts:189-191`); resolves usernames of unseen `#users.presence/extra` (`instance.ts:192-209`);
  connectors `refresh`; `sandbox.run(payload)`.
- Success: bucket update, `publishRunnerIntentsForRooms`, usage, console, eval acks, connectors `save`
  (`instance.ts:222-239`). Failure (`error`/`disposed`/`timedOut`): stale (or immediate reset for internal errors),
  bucket penalized by `tickLimit`, error printed to console, and EMPTY intents published so rooms are not blocked
  (`instance.ts:240-271`).

### XX-RUN-PAYLOAD

`InitializationPayload {userId, codeBlob, shardName, terrainBlob, ...mods}`, `TickPayload {cpu, roomBlobs, time,
backendIntents?, eval, usernames?, ...mods}`, `TickResult {error?, console, evalAck?, intentPayloads, usage, ...mods}`
(`reference/xxscreeps/packages/xxscreeps/engine/runner/index.ts:8-44`). Mods extend these by declaration merging and fill
them in connectors (XX-MOD-HOOK-RUNNER).

### XX-RUN-SANDBOX

- Modes (`reference/xxscreeps/packages/xxscreeps/config/config.ts:168-176`, `reference/xxscreeps/packages/xxscreeps/driver/sandbox/index.ts:37-61`):
  `isolated` (default, isolated-vm), `unsafe` (node:vm in the runner's own isolate; no memory limit; ordinary symbols),
  `experimental` (`@isolated-vm/experimental`).
- isolated: `new ivm.Isolate({memoryLimit: runner.cpu.memoryLimit + terrainBlob MB})`
  (`reference/xxscreeps/packages/xxscreeps/driver/sandbox/isolated/index.ts:62-65`); context gets the native
  pathfinder module instance, `ivm-inspect` util, then the runtime script (webpack bundle of
  `driver/sandbox/isolated/runtime.ts` + game + constants, compiled once per process by `runOnce`, compiled again per
  isolate) runs and `initialize(isolate, context, data)` is applied with a copied payload (`isolated/index.ts:23-108`).
- `run(payload)`: `tick.apply(..., {arguments: {copy: true}, result: {copy: true}, timeout: tickLimit})`; CPU used =
  `isolate.cpuTime` delta; `Script execution timed out.` -> `timedOut`; disposal (incl. memory limit) -> `disposed`
  (`isolated/index.ts:121-153`).
- **PERF**: blobs are `SharedArrayBuffer`-backed so the structured copy into the isolate shares memory (the README's
  "instantly transferred") (`reference/xxscreeps/README.md:22-27`); but each isolate holds a full copy of the compiled
  game library and its own heap (256 MB limit by default).

### XX-RUN-TICK

`tick(data)` inside the sandbox (`reference/xxscreeps/packages/xxscreeps/driver/runtime/index.ts:97-228`):

1. For each blob: `RoomSchema.read` (materializes room + `#objects` shells) and `room['#initialize']()` (builds
   look/spatial indices, sets `object.room`) (`index.ts:101-105`).
2. Merge `usernames` into `userInfo`; `new GameState(world, T, rooms)` (objects map; `roomInitializer` hooks) (`index.ts:106-111`).
3. `runForPlayer(me, state, data, Game => ...)`: new `IntentManager`, `new Game(state, data)` (`gameInitializer`
   hooks, then `#addToMyGame` for every object whose `.my` is true) (`reference/xxscreeps/packages/xxscreeps/game/game.ts:90-109`).
4. `runtimeConnector.receive(data)`; `globalThis.Game = Game`; call `require('main').loop()` — re-reading `loop`
   every tick so bots may replace it; missing `loop` -> error; errors are printed with the stack trimmed at the
   engine boundary (`index.ts:116-138`).
5. Console eval requests run after the loop, results acked/echoed (`index.ts:140-172`); `Game` global unset.
6. Backend intents injected via `intents.save` (`index.ts:178-187`); per-room `intentPayloads` built (`index.ts:190-199`);
   `runtimeConnector.send(result)` (memory flush, flags, visuals...); console flushed (`index.ts:202-203`).
7. Detach every room buffer (`index.ts:206-208`). Any exception escaping -> `{result: 'error'}`.

### XX-RUN-GAME

`reference/xxscreeps/packages/xxscreeps/game/index.ts:32-98`, `reference/xxscreeps/packages/xxscreeps/game/game.ts`:
- `GameState {world, time, objects: Map<id, obj>, rooms}` shared by all `Game`s of a tick; constructor runs
  `roomInitializer` for each room (`game.ts:17-39`).
- `GameBase {rooms, time, map, getObjectById}` — `getObjectById` returns `null` when absent (`game.ts:44-66`).
- `Game extends GameBase`: `cpu` (hook), `powerCreeps = {}`, `shard {name, type:'normal', ptr:false}`,
  `gpl {level 0, progress 0, progressTotal Infinity}`, `notify` warns unless the notifications mod overrides it
  (`game.ts:71-126`).
- `runWithState` (swap global `Game` = GameBase), `runAsUser` (swap `me`, flush find caches + path cache),
  `runForUser` (NPC: Game without TickPayload), `runForPlayer` (sandbox), `runOneShot` (backend validation)
  (`game/index.ts:32-98`).

### XX-RUN-CPU

- `Game.cpu.limit` = constant `kCPU = 100` for every user (`reference/xxscreeps/packages/xxscreeps/engine/runner/instance.ts:37,174`).
- `tickLimit = min(runner.cpu.tickLimit (500), floor(bucket))` (`instance.ts:175`); enforced as the isolate timeout.
- Used CPU = isolate `cpuTime` delta (`reference/xxscreeps/packages/xxscreeps/driver/sandbox/isolated/index.ts:131-133`);
  bucket = `clamp(0, 10000, bucket - used + 100)` (no rounding) (`instance.ts:225`); failure: `clamp(0, 10000, bucket - 500) + 100` (`instance.ts:252`).
- `Game.cpu.getUsed()` = isolate WALL time since tick start (`reference/xxscreeps/packages/xxscreeps/driver/sandbox/isolated/runtime.ts:45`).
- `IntentManager.cpu` accumulates 0.2 per intent but is never added to usage (`reference/xxscreeps/packages/xxscreeps/game/intents.ts:9`).
- Vanilla: intent cost 0.2 except `say`/`pull` (`reference/driver/lib/runtime/runtime.js:60-91`), included in
  `getUsed` (`reference/driver/lib/runtime/runtime.js:202-204`) and charged as `ceil(usedCleanTime + intentsCpu)`
  against `user.cpu`-based refill (`reference/driver/lib/runtime/make.js:194-211`). XX-COMPAT-CPU.

### XX-RUN-MODULES

`makeEnvironment(modules, evaluate, compiler)` (`reference/xxscreeps/packages/xxscreeps/driver/runtime/module.ts:12-...`):
`main.js`/`main` -> flat CommonJS `require` (`require('./x')` and `require('x')` resolve to `x.js` if present),
`globalThis.require` set, `require.cache` exposed; `main.mjs`/`main.wasm` -> ES module loader with URL-style
resolution, `.json` modules, WASM with a WASI shim (`wasi_snapshot_preview1`), source maps. Code is stored as two
blobs (strings, buffers) (`reference/xxscreeps/packages/xxscreeps/engine/db/user/code-schema.ts:5-35`).

### XX-RUN-MEMORY

`reference/xxscreeps/packages/xxscreeps/mods/memory/memory.ts`:
- The raw Memory string lives in a `Uint16Array` on a `SharedArrayBuffer`, grown in 4096-unit steps, max
  `kMaxMemoryLength = 2*1024*1024` UTF-16 units (`memory.ts:5-9,30-34`).
- `Memory` getter (redefined as an accessor every tick by `receive`, `reference/xxscreeps/packages/xxscreeps/mods/memory/game.ts:44-56`):
  returns the cached parsed object of the PREVIOUS tick if `RawMemory.set` was not called (no re-parse); otherwise
  `JSON.parse` ('' -> `{}`, parse error -> `null`) (`memory.ts:187-201`).
- `flush()` at tick end: if `_parsed` exists, `crunch` it (delete `undefined` object properties, replace `undefined`
  array items with `null`) and `JSON.stringify` into the buffer; memhack (redefining `globalThis.Memory`) is detected by
  wrapping `Object.defineProperty` (`memory.ts:44-66,213-271,278-298`). Saved blob is dropped if >= 4 MiB
  (`reference/xxscreeps/packages/xxscreeps/mods/memory/model.ts:37-41`).
- Rationale: "Vanilla Screeps runs a flagrantly wasteful JSON.parse each tick" (`memory.ts:38-43`). XX-COMPAT-MEMORY.

### XX-RUN-SEGMENTS

Max 10 active segments, ids 0..99, 100 K UTF-16 units each (`reference/xxscreeps/packages/xxscreeps/mods/memory/memory.ts:7-9`);
`setActiveSegments` takes effect next tick through the connector `refresh` (`reference/xxscreeps/packages/xxscreeps/mods/memory/driver.ts:66-78,99-103`);
foreign segment tri-state (`undefined` no change / `null` clear / object) and public segments with a pub/sub
invalidation channel (`driver.ts:37-55,79-97,127-133`).

### XX-RUN-FLAGS

Flags are a per-user blob `user/<id>/flags` (vector of Flag structs keyed by name), loaded into the sandbox once;
`Game.flags` reuses the same objects every tick, inserting them into visible rooms; create/remove/setColor from the
player AND from backend routes (delivered over `user/<id>/flags` -> `TickPayload.flagIntents`) are applied inside
the sandbox at `send`, and the blob is written back only if changed
(`reference/xxscreeps/packages/xxscreeps/mods/flag/game.ts:16-131`, `reference/xxscreeps/packages/xxscreeps/mods/flag/driver.ts:18-45`,
`reference/xxscreeps/packages/xxscreeps/mods/flag/backend.ts:56-83`). XX-COMPAT-FLAGS.

### XX-RUN-VISUAL

Visual blobs per room (and `'*'`, `'map'`) from `TickResult.visuals`, filtered to existing rooms, stored in scratch
hash `user/<id>/visual<T%2>` and announced on `user/<id>/visual`; the backend room socket attaches them via the
`roomSocket` hook (`reference/xxscreeps/packages/xxscreeps/mods/visual/driver.ts:14-22`,
`reference/xxscreeps/packages/xxscreeps/mods/visual/model.ts:49-60`, `reference/xxscreeps/packages/xxscreeps/mods/visual/backend.ts:7-34`).

### XX-RUN-CONSOLE

Console output is buffered in the sandbox and flushed into `TickResult.console` (JSON lines `{fd, data}`), published on
`user/<id>/console` (`reference/xxscreeps/packages/xxscreeps/engine/runner/instance.ts:230-233`). Eval requests
(`requestRunnerEval`) round-trip through `runner/<user>` and `user/<user>/ack` with a timeout `max(500, 4*tickSpeed)`
(`reference/xxscreeps/packages/xxscreeps/engine/runner/model.ts:41-63`).

### XX-RUN-HARDEN

`Array.prototype[Symbol.iterator]` and all typed-array constructors/prototypes are frozen in the runtime to protect
shared terrain/blob memory (`reference/xxscreeps/packages/xxscreeps/driver/runtime/index.ts:22-58`). Engine state is in
private symbols (XX-SCHEMA-PRIVATE). Player code still shares a realm with the engine runtime, so the processor
re-validates everything.

### XX-RUN-FIND-CACHE

`room.find(type)` caches the unfiltered result per room per FIND constant (`reference/xxscreeps/packages/xxscreeps/game/room/room.ts:47-53`);
`runAsUser` clears all rooms' caches because results like `FIND_MY_*` depend on `me`
(`reference/xxscreeps/packages/xxscreeps/game/index.ts:47-52`); `#flushObjects` clears it on mutation (`room.ts:112`).

---

## 9. Pathfinder

### XX-PF-NATIVE

- Native C++ module (`reference/xxscreeps/packages/pathfinder/src/*.cc`, JPS + heap + open/closed) with Node and
  isolated-vm bindings (`reference/xxscreeps/packages/pathfinder/pf.ts`, `reference/xxscreeps/packages/pathfinder/iv.ts`).
- Terrain loaded once per process for the whole world (`reference/xxscreeps/packages/xxscreeps/driver/pathfinder/pathfinder.ts:29-36`,
  `reference/xxscreeps/packages/pathfinder/pathfinder.ts:45-56`); map size is `1 << (8*sizeof(room_location_t))`
  rooms (`reference/xxscreeps/packages/pathfinder/src/pf.h.cc:12-13,40`).
- Positions crossing the boundary are world coords `(wy << 16) | wx`, `wx = rx*50 + x`
  (`reference/xxscreeps/packages/xxscreeps/driver/pathfinder/pathfinder.ts:11-25`); room ids `(ry << 8) | rx`
  (`reference/xxscreeps/packages/pathfinder/pathfinder.ts:20-31`).
- Defaults: `plainCost 1, swampCost 5, heuristicWeight 1.2, maxOps 0x7fffffff, maxCost 0x7fffffff, maxRooms 16`; no
  goals -> empty result (`reference/xxscreeps/packages/pathfinder/pathfinder.ts:84-100`); path expanded from jump
  points step by step, reversed, origin removed (`pathfinder.ts:113-132`).

### XX-PF-ROOMSEARCH

`roomSearch` backs `Room.findPath`/`RoomPosition.findPathTo`/`moveTo` (`reference/xxscreeps/packages/xxscreeps/game/pathfinder/index.ts:51-125`):
cost matrices per `room:flags` key cached until the end of `runAsUser`; obstacles from `makeObstacleChecker` -> 255;
otherwise lowest `#pathCost` (roads) unless `ignoreRoads`; default `plainCost = 2, swampCost = 10` (1/5 with
`ignoreRoads`); `costCallback` receives a clone; goal range default `max(1, range ?? 1)`. `PathFinder.use()` is a no-op
(`index.ts:120-124`).

---

## 10. Backend

### XX-BE-SERVER

Koa + koa-router + sockjs; imports `backend`, `game`, `processor` slots; `initializeGameEnvironment()`;
`backendReady` hooks; middleware: conditional GET, error -> 500, context injection, body parser (8 MB), auth, mod
`middleware` hooks, routes (`reference/xxscreeps/packages/xxscreeps/backend/server.ts:16-65`).

### XX-BE-ROOMSOCKET

`reference/xxscreeps/packages/xxscreeps/backend/sockets/room.ts`:
- One shared subscription per room per backend process: initial `loadRoom`; listens to `didUpdate` and `tick`;
  throttled (`backend.socketThrottle`, 125 ms) refresh that reloads the blob ONLY if `didUpdate` was seen
  (`room.ts:52-121`).
- Per socket subscriber: `runOneShot(world, room, time, user)` renders every object via the `[Render]` chain with
  `previousTime`, JSON-diffs against that subscriber's previous frame, merges `roomSocket` hook extras and newly seen
  users' badges, sends `{objects, info, gameTime, users, ...}` (`room.ts:123-283`).
- `willSpawn` suppresses frames for 1 s to dodge a client race (`room.ts:211-221`).
- **PERF**: rendering and diffing are per subscriber (they are per-user views).

### XX-BE-ENDPOINTS

- `POST /api/game/place-spawn`: respawn timeout check, `willSpawn`, user must have no intent rooms, `runOneShot`
  validates with `checkCreateConstructionSite` after temporarily assigning the room/controller to the user, then
  `pushIntentsForRoomNextTick({local: {placeSpawn: [[x, y, name]]}, internal: true})`
  (`reference/xxscreeps/packages/xxscreeps/mods/spawn/backend.ts:130-183`).
- `POST /api/user/respawn`: `unspawn` internal intent into every presence room, flags cleared (`.../mods/spawn/backend.ts:185-206`).
- `POST /api/game/add-object-intent`: client object intents (`removeConstructionSite` -> `remove`) pushed for next
  tick as normal (non-internal) user intents (`reference/xxscreeps/packages/xxscreeps/backend/endpoints/game/intents.ts:43-67`).

---

## 11. Performance

### XX-PERF-FAST

What xxscreeps does to be fast (each with the mechanism):

1. **Room blobs as the unit of state and exchange** (XX-SCHEMA-*): one key per room per buffer slot; storage I/O
   is one get/set per room; blobs shard naturally (`reference/xxscreeps/README.md:12-38`).
2. **Zero-copy into sandboxes**: `SharedArrayBuffer` blobs cross into isolates without copying (XX-RUN-SANDBOX).
3. **Lazy overlay**: untouched fields never materialize; integers are direct typed-array loads (XX-SCHEMA-OVERLAY).
4. **Run/process overlap with per-room readiness** instead of a global barrier (XX-PROC-QUEUE).
5. **Room sleeping**: rooms without players are not processed unless a deadline or event wakes them; unchanged
   rooms are copied forward without re-serialization (XX-PROC-SLEEP, XX-PROC-FINALIZE).
6. **Processor room affinity + in-memory room reuse** across ticks (XX-PROC-WORKERS, XX-PROC-ROOMCACHE).
7. **Runner affinity**: sandboxes stay warm on the runner that has them; migration only after 50 ms (XX-RUN-SCHED).
8. **Memory not re-parsed every tick** (XX-RUN-MEMORY) and foreign segments decoded lazily.
9. **Native pathfinder with process-wide terrain**; per-user cost-matrix cache (XX-PF-*).
10. **`didUpdate` signal** skips blob writes and backend reloads for unchanged rooms (XX-DB-DOUBLEBUF, XX-BE-ROOMSOCKET).
11. **Precompiled reader/writer closures** per layout, writer unrolling (XX-SCHEMA-WRITE).
12. **Per-room spatial/look indices and find cache** (XX-PROC-OBJECTS, XX-RUN-FIND-CACHE).
13. **NPC AI in-process** without isolates (XX-PROC-NPC).

### XX-PERF-SLOW

Where xxscreeps is still slow or will not scale to ~1000 players / huge worlds:

1. **Storage RPC per room step**: popping a room (script eval), fetching intents (`lRange` + `vdel`), loading the blob,
   publishing inter-room intents, decrementing counters, saving the blob, updating user/room sets are all separate
   keyval round-trips to one host thread (XX-DB-LOCAL) or Redis (`reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:89-234`).
2. **Local sorted set re-sorts on every add** (XX-DB-SORTEDSET) while the queue receives one `zAdd incr` per
   (player, intent room) per tick -> roughly `O(P * R_p * N log N)` per tick.
3. **JSON intents**: runtime builds JS objects -> structured clone out of the isolate -> `JSON.stringify` -> keyval list
   -> `JSON.parse` in the processor (`.../engine/processor/model.ts:89-91,145-155`).
4. **Full room re-serialization** whenever anything in the room changed (XX-SCHEMA-WRITE), plus `flushUsers` and
   relationship diffs per room per tick.
5. **Per-player per-tick materialization**: every visible room is re-read, `#initialize` rebuilds indices, `GameState`
   builds an id map, and `new Game` scans every object to find `.my` ones (reads `#user` id -> string for each object)
   (XX-RUN-TICK) — `O(visible objects)` per player per tick even with lazy fields.
6. **One V8 isolate per player** with its own copy of the compiled engine library and heap (256 MB default limit);
   the runner is one Node thread orchestrating them (XX-RUN-SANDBOX).
7. **Processor parallelism capped at ceil(players/2)** and scheduled by IPC (XX-PROC-WORKERS).
8. **Stragglers**: a room waits for its slowest player; the whole tick waits for the slowest room, finalize is a
   second barrier, shard tick processors a third (XX-SVC-TICK).
9. **Backend renders and diffs per subscriber** (XX-BE-ROOMSOCKET).
10. **Iterator/closure-heavy helpers** (`Fn.pipe/map/filter`, per-move closures) on hot paths
    (`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:139-150`, `.../engine/processor/movement.ts:74-173`).
11. **Persistence**: whole keyval re-serialized as one JSON file every save; blobs written as one file each
    (XX-DB-SAVE).
12. **Dropped resources keep rooms active every tick** (`reference/xxscreeps/packages/xxscreeps/mods/resource/processor/resource.ts:50`).

---

## 12. Compatibility gaps (xxscreeps vs vanilla)

### XX-COMPAT-MOVE

xxscreeps resolves movement by sorting all moves by `power/weight` priority with `Math.random()` tie-breaks and a
recursive push resolver (XX-PROC-MOVEMENT; `reference/xxscreeps/packages/xxscreeps/engine/processor/movement.ts:168-169`).
Vanilla resolves per target tile with rates: `rate1` = number of moves targeting the mover's current tile (100 if a
swap), `rate2` pulled, `rate3` pulling, `rate4 = moves/weight`, sorted descending
(`reference/engine/src/processor/intents/movement.js:105-140`). Bots relying on vanilla tie-breaking (e.g. traffic
managers) behave differently. chitin must port vanilla `movement.js`, not xxscreeps.

### XX-COMPAT-WEIGHT

xxscreeps weight = non-MOVE non-CARRY parts + `ceil(used / 50)` (`reference/xxscreeps/packages/xxscreeps/mods/creep/creep.ts:670-675`).
Vanilla counts the CARRY parts actually needed from the END of the body, using boosted capacity, only for live parts
(`reference/engine/src/processor/intents/movement.js:41-58,235-239`). Differs for boosted CARRY and damaged bodies.

### XX-COMPAT-INTENT-ORDER

Constraint-derived per-object order (XX-PROC-INTENT-ORDER) vs vanilla's fixed `creepActions` list
(`reference/engine/src/processor/intents/creeps/intents.js:15-35`). The winner inside the `primary` exclusion group
comes out the same as vanilla's blocking table (heal > rangedHeal > attackController > dismantle > repair > build >
attack > harvest), but: (a) every pair WITHOUT a declared constraint (e.g. `withdraw` vs `build`, `harvest` vs
`transfer`, `say` vs anything) gets a registration-order-dependent position, whereas vanilla fixes the full order;
(b) declared divergences exist, e.g. `pull` is `before: 'move'` in xxscreeps
(`reference/xxscreeps/packages/xxscreeps/mods/creep/processor.ts:247`) but last in vanilla. Same-object effects
(resources withdrawn then spent in the same tick, event log order) can differ. chitin must reproduce vanilla's list.

### XX-COMPAT-USER-ORDER

Cross-user processing order inside a room is the order runner payloads arrived (XX-PROC-INTENT-DISPATCH step 1):
nondeterministic and different from vanilla. Contested actions (two players withdrawing the last energy) can resolve
differently run to run.

### XX-COMPAT-STALE-OBJECTS

Objects kept across ticks throw on access of non-memoized fields (XX-SCHEMA-DETACH) and cannot receive intents.
Vanilla keeps stale objects readable (they are plain objects rebuilt each tick). Common bot caches
(`global.cache = Game.getObjectById(...)`) break. chitin must keep stale objects readable with their last values.

### XX-COMPAT-MEMORY

- xxscreeps reuses last tick's parsed `Memory` object (only `undefined` cleanup via `crunch`) instead of re-parsing:
  functions, class instances, prototypes, `NaN/Infinity`, `Date`, `Map/Set`, shared references and cycles survive
  across ticks in xxscreeps but are normalized by JSON in vanilla (`reference/xxscreeps/packages/xxscreeps/mods/memory/memory.ts:187-201,234-247`).
- Vanilla parses on first access each tick and sets the root prototype to `null`, then redefines `Memory` as a data
  property (`reference/engine/src/game/game.js:479-500`); xxscreeps keeps an ordinary `Object` prototype and an
  accessor (`reference/xxscreeps/packages/xxscreeps/mods/memory/game.ts:51-55`).

### XX-COMPAT-CPU

Fixed `limit = 100`, no GCL CPU, intents never charged, no `Math.ceil`, `getUsed` measures wall time (XX-RUN-CPU).
Vanilla references in XX-RUN-CPU.

### XX-COMPAT-BORDER

On border crossing xxscreeps sets `fatigue = 0` and clears the action log, then removes the creep from the source
room unconditionally; if the destination room does not exist the import intent is dropped by `sendRoomIntent`
(`reference/xxscreeps/packages/xxscreeps/mods/creep/processor.ts:385-410`,
`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:263-267`). Vanilla keeps fatigue and skips the move
when the destination is not accessible (`reference/engine/src/processor/intents/creeps/tick.js:51-76`,
`reference/engine/src/processor/global.js:31-43`).

### XX-COMPAT-EVENTLOG

As read (not executed): `process()` clears `#eventLog` without `didUpdate`; if nothing else changes,
`finalize` copies the T blob forward, so tick T+1 sees the event log produced by the previous processing again
(`reference/xxscreeps/packages/xxscreeps/engine/processor/room.ts:100,227-229`). Vanilla's event log is per tick.
chitin: clear the event log explicitly every processed tick.

### XX-COMPAT-LATE-INTENTS

After intent abandonment, late runner payloads are applied in the room's next processing (XX-SVC-ABANDON). Vanilla
never applies a tick's intents to a later tick.

### XX-COMPAT-CHECK-ORDER

Return-code precedence of `check*` functions is still being aligned with vanilla (dozens of
`*-validation-precedence` changesets: `reference/xxscreeps/.changeset/transfer-validation-precedence.md`,
`reference/xxscreeps/.changeset/withdraw-validation-precedence.md`, `reference/xxscreeps/packages/xxscreeps/CHANGELOG.md:7-19`).
Never use xxscreeps check order as a reference for chitin.

### XX-COMPAT-WORLD-SIZE

Room coordinates are u8 (W127..E127, N127..S127) in `RoomPosition`, the pathfinder and room ids
(XX-SCHEMA-POS, XX-PF-NATIVE). chitin's "worlds larger than official shards" target needs wider internal room
coordinates while keeping the vanilla-visible API.

### XX-COMPAT-MISSING

Not implemented or stubbed: power creeps (`Game.powerCreeps = {}`, `PowerCreep` empty global)
(`reference/xxscreeps/packages/xxscreeps/game/game.ts:81`, `reference/xxscreeps/packages/xxscreeps/game/runtime.ts:13-14`);
`Game.gpl` fixed (`game.ts:88`); survival (`room.survivalInfo` undefined) (`reference/xxscreeps/packages/xxscreeps/game/room/room.ts:33-37`);
market orders/deal/credits (`reference/xxscreeps/packages/xxscreeps/mods/market/market.ts:3-38`); intershard resources all 0
(`reference/xxscreeps/packages/xxscreeps/mods/intershardResource/game.ts:10-15`); strongholds
(`reference/xxscreeps/packages/xxscreeps/mods/invader/invader-core.ts:39`); `PathFinder.use` no-op; deposit type
hash-derived instead of world-gen (`reference/xxscreeps/packages/xxscreeps/mods/deposit/main.ts:33-38`); nuke launch
notification (`reference/xxscreeps/packages/xxscreeps/mods/nuker/processor.ts:48-50`).

### XX-COMPAT-FLAGS

Flag changes are applied in the owner's runtime at the end of its tick (XX-RUN-FLAGS), and backend flag requests
only apply when the owner's sandbox runs. Vanilla processes flag intents server-side.

### XX-COMPAT-ID

Ids must be hex strings of <= 24 chars to fit `Id.format` (XX-SCHEMA-ID).

### XX-COMPAT-VISION-INTENTS

The runner publishes intent payloads only for the user's `intentRooms` (rooms containing one of the user's
`#hasIntent` objects) (`reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:109-115`,
`reference/xxscreeps/packages/xxscreeps/engine/service/runner.ts:102-110`). Intents the runtime recorded for any other
visible room — e.g. `Room.createConstructionSite` in a room seen only through an observer (an `ObserverSpy` provides
vision but not intents, `reference/xxscreeps/packages/xxscreeps/mods/observer/observer-spy.ts:12-15`) — are silently
discarded although the API returned `OK`. Vanilla saves intents under every room key present in the user's intent
object and activates those rooms (`reference/driver/lib/index.js:180-240`). chitin must route intents by the target
room, not by a precomputed per-user room set.

---

## 13. Recommendations

### XX-REC-COPY — What chitin should copy

1. **Manifest + DAG loader with deterministic order.** `Mod { id, dependencies, provides }`; DFS post-order with
   sorted siblings and cycle errors (XX-MOD-RESOLVE). For Rust mods: a `trait Mod { fn manifest(); fn register(&self,
   r: &mut Registry) }`; for JS mods: a manifest file + entry per slot. Persist the resolved order and treat it as part
   of the save format.
2. **Slots per execution context** (`game` = player-visible API + schema, `processor`, `driver`/runner connectors,
   `main`/global, `backend`, `storage`, `config`, `test`) so each service only loads what it executes (XX-MOD-PROVIDES).
   In chitin the `game` slot of JS mods must be loadable into our own JS engine's player contexts.
3. **Hook registry frozen after first use** ("took" semantics) (XX-MOD-HOOKREG): registration phase at startup,
   then immutable `Vec<Handler>` per hook; zero-cost dispatch on the hot path.
4. **Intent registration by (receiver kind, name, constraints, internal)** compiled at startup into a priority table
   and conflict bitmasks (XX-MOD-INTENT-REG/ORDER) — but compute with a real Kahn topological sort with deterministic
   tie-breaks, and let the `classic` mod declare vanilla's exact `creepActions` order and blocking table
   (`reference/engine/src/processor/intents/creeps/intents.js:3-35`) so 1:1 is a data declaration.
5. **Dual validation**: one `check_*` per action, used by the API (return code) and by the processor (XX-PROC-DUAL-VALIDATION).
6. **Room-local processing phases with an explicit context API** (`did_update`, `set_active`, `wake_at`,
   `send_room_intent`, deferred global effects) (XX-PROC-PHASES/CONTEXT). Rooms touch only themselves; cross-room
   effects are queued.
7. **Inter-room intents applied in a finalize phase after all rooms** (XX-PROC-INTERROOM), matching vanilla's global
   interRoom step; generalize to a typed "room message" bus.
8. **Room sleep/wake with deadline scheduling** (a min-heap of wake ticks) and copy-forward of unchanged rooms
   (XX-PROC-SLEEP). This is the core lever for very large worlds.
9. **Per-room readiness** (process a room as soon as all players with intent rights in it have run) instead of a
   global runner barrier (XX-PROC-QUEUE) — implemented in-process with atomic counters, not a sorted set over IPC.
10. **Schema-defined object state with extension points by path** (`Room`, `Room.objects`, `Room.eventLog`,
    `ResourceType`, `ActionLog.action`) (XX-MOD-SCHEMA-EXT), with layout versioning + archived layouts + upgrade on
    load (XX-SCHEMA-VERSION). Use stable type ids (by name) rather than registration-order indices.
11. **Immutable per-tick room snapshots shared zero-copy with player contexts** and lazy materialization of JS
    objects over them (XX-SCHEMA-OVERLAY, XX-RUN-SANDBOX) — in chitin: `Arc<RoomSnapshot>` read by host objects whose
    getters read Rust fields on demand.
12. **Runner/runtime connector pairs** to move per-user data (memory, segments, flags, visuals, GCL) in and out of the
    player context each tick (XX-MOD-HOOK-RUNNER).
13. **Prototype-level protocol on the base object** (`#layer`, `#captureDamage`, `#applyDamage`, `#pathCost`,
    `#providesVision`, `#hasIntent`...) as the cross-mod extension surface (XX-MOD-PROTO) — as Rust trait methods with
    default impls on an object-kind vtable.
14. **Process-wide shared terrain + native pathfinder** (XX-PF-NATIVE).
15. **`simulate()`-style harness** per mod (player / poke / tick) for parity tests against vanilla traces (XX-MOD-TEST).
16. **Backend extension**: routes, socket subscriptions, per-object render chain with `next()` and per-room
    `didUpdate` notifications (XX-MOD-HOOK-BACKEND, XX-MOD-RENDER).
17. **Worker affinity for hot rooms** (keep a room resident on the same core between ticks) (XX-PROC-WORKERS).

### XX-REC-AVOID — What chitin should avoid

1. **Services connected only through a keyval/pubsub store** with JSON intents and per-room RPCs (XX-PERF-SLOW 1-3).
   Use one process, shared memory, lock-free work queues and per-tick barriers; keep persistence out of the hot loop.
2. **Nondeterminism**: `Math.random()` tie-breaks in movement, user order = arrival order, inter-room intent order =
   completion order, `Math.random` ids (XX-COMPAT-MOVE/USER-ORDER, XX-SCHEMA-ID). Sort users by id and objects by
   insertion order; route all randomness through one seeded RNG per (room, tick) so parallel runs are reproducible.
3. **Diverging gameplay algorithms**: port vanilla movement, weight, intent order, CPU accounting, Memory semantics
   (XX-COMPAT-*). xxscreeps code is a structural reference only.
4. **The modified selection sort and the `infos.length === 0` getter** (XX-PROC-INTENT-ORDER).
5. **Full room re-serialization per changed room** (XX-SCHEMA-WRITE): keep rooms resident in Rust structures and
   snapshot/journal only dirty components.
6. **One isolate per player containing a full copy of the engine library**: in our own JS engine, share immutable
   builtin code/shapes across contexts and give each player only its heap.
7. **Throwing on stale objects** (XX-COMPAT-STALE-OBJECTS) and **reusing parsed Memory** (XX-COMPAT-MEMORY): optimize
   parsing speed, not semantics.
8. **Hidden non-schema state on cached objects** (XX-PROC-ROOMCACHE): all simulation state must be in the schema.
9. **Processor parallelism tied to player count** (XX-PROC-WORKERS) and runner/processor overlap via timeouts:
   size pools by cores; never abandon intents into a later tick (XX-COMPAT-LATE-INTENTS).
10. **Periodic whole-database JSON dumps** (XX-DB-SAVE): use checkpoint + per-tick journal.
11. **u8 room coordinates** (XX-COMPAT-WORLD-SIZE): use i16/i32 internally.
12. **Async tasks reading other rooms' previous-tick blobs** for cross-room decisions (terminal send capacity)
    (XX-PROC-TASKS): use an explicit deterministic global phase.
13. **Clearing the event log without marking the room dirty** (XX-COMPAT-EVENTLOG).
14. **Flags (or any gameplay state) mutated inside the player runtime** (XX-COMPAT-FLAGS): all authoritative state
    changes belong to the processor.
15. **Binary format tied to mod registration order** (variant/enum indices) (XX-SCHEMA-LAYOUT): persist an explicit
    name -> id table in the save.

---

## 14. Open questions

1. XX-COMPAT-EVENTLOG is derived from reading `room.ts`; not verified by running xxscreeps. Confirm whether some
   always-on processor calls `didUpdate` for rooms with players every tick (none found).
2. `movesByLocation.delete(move.id)` when a move's `initial` returns false removes every move targeting that tile
   (`reference/xxscreeps/packages/xxscreeps/engine/processor/movement.ts:94`); unclear whether intended — irrelevant
   for chitin (we port vanilla movement), noted for anyone reusing xxscreeps code.
3. Whether `zAdd(..., {if: 'XX', incr: true})` on a room missing from the queue (user's intent rooms changed mid-tick)
   returns `null` in all providers and silently drops that player's intents for the tick
   (`reference/xxscreeps/packages/xxscreeps/engine/processor/model.ts:112-120`).
4. Writer scratch-buffer padding (XX-SCHEMA-WRITE) — confirm stale bytes are actually observable in saved blobs.
5. Whether xxscreeps' room processing order of PreTick/Tick (by `#objects` index, which changes with swap-removal)
   matters for any vanilla-observable outcome beyond event log order; vanilla's own object iteration order must be
   specified in the processor specs.
