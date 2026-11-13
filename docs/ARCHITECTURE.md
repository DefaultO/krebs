# chitin — architecture (authoritative)

Status: final, 2026-10-20. This file is the contract between work packages. `docs/spec/*` stays the behavioural
authority (cited as `01 ACT-SET`, `02 §8.13`, …); where this file refines a spec rule it says so. It supersedes
`docs/design/arch-{scale,fidelity,delivery}.md`, which are not published.

Base: the scale design (cost model, immutable snapshots, lazy world natives, governor, incremental checkpoints),
with the fidelity design's live heap, system-realm intent path, per-query global graphs and oracle stack, and the
delivery design's V8 confinement, skeleton-first work split and pool additions.

Notation: *(est.)* = estimate to be replaced by measurement; *(target)* = acceptance gate. FROZEN = created by WP00
and changed only through an interface PR (§14.1). Hardware: target 16C/32T, 64 GB; dev box i9-11900K 8C/16T, 32 GB.
`T` = the tick being run (`Game.time`); `G = T+1` = stored game time after it.

---

## 1. Decisions

1. **One process, in-memory authoritative world, no DB/IPC on the hot path.** ~90 % of vanilla tick time is IPC/DB/JSON (01 §18).
2. **Vanilla barriers: runners(T) → rooms(T) → global(T) → time++ → TICK-ACCESSIBLE → CRON-STAGE → publish.** 01 §19.1 requires them; any user can target any room, so per-room readiness degenerates to the barrier and would hide ≤ 5 % of a tick.
3. **One `util::pool::Pool` (logical cores) runs both parallel phases; the runner phase uses `runner.threads` loops, default = physical cores.** Phases never overlap; physical-core default keeps thread-CPU billing free of SMT inflation.
4. **One V8 isolate per player, `v8::SharedIsolate`, locked by whichever worker runs it.** v8 152.2 supports it (`locker.rs`); migration is not a global reset. M1: shared LPT queue; M5: home-worker affinity + steal-largest.
5. **Pointer compression off by default; `tools/v8-ptrcomp.ps1` builds V8 from source with it (`runtime/ptrcomp`), a 4 GB cage per isolate.** The crate's ptrcomp puts all isolates in one 4 GB cage and has no IsolateGroup API, so `vendor/v8` gives every isolate a group of its own (`src/binding.cc`, `build.rs`); no prebuilt library exists. Its builtins are laid out by a profile of our own workload (builtins PGO), and its compilers keep optimized code off the global `Memory` cell that every tick replaces (§2.9).
6. **Startup snapshot = lodash 3.10.1 + our game-API bundle + JS-mod game slots, built at boot and kept in memory, never read from disk; natives are installed per isolate by `_init(natives)` and then deleted.** Isolate creation in ms; no external-reference table.
7. **World data reaches isolates as one immutable ASCII JSON blob per room snapshot (`Arc<[u8]>` → `String::new_external_onebyte_raw`, zero copy, shared by every viewer) plus a small per-user meta JSON; JS parses with the captured `JSON.parse` and runs the mirrored `game.js` eagerly.** V8's parser is the fastest object builder; fresh raw objects every tick are vanilla-observable (02 §3.8).
8. **Each document caches its ASCII JSON (`OnceLock`); a room's text is the concatenation of cached docs (FROZEN).** Serialization follows changed objects, not viewers. The text is put together only for a VM that parses the room whole (§6.6): one that held the room is sent its changed docs alone. A doc's place in the text follows from the lengths of the docs before it, should lazy per-object materialization (M5) want it.
9. **Everything world-sized reaches JS through lazy natives:** terrain (per-room copy on demand), accessibility, room status, mapgrid exit data (built in Rust once per TICK-ACCESSIBLE snapshot), market orders/history (per-resource JSON, built once per tick). Vanilla's per-player copies of `accessibleRooms`, the world grid and all orders are O(players × world).
10. **Intents leave through vanilla's path: the player isolate's `outMessage` is serialized with `ValueSerializer` and deserialized in the worker's *system isolate* (one per worker thread: lodash + `common/system.js` + `engine/utils.js`), which runs the `storeIntents` port and returns one JSON string.** Same as `ExternalCopy` → node realm (runtime.js:296, make.js:192): clone failures, `""+v`/`parseInt` on real NaN/Infinity/BigInt/Date, immunity to bot prototype patches; no second context per player.
11. **One CPU meter for `getUsed()`, deadlines and billing: OS thread CPU time (`Thread`, default); `Wall` reproduces the private server; `Fixed` advances 0.01 ms per call (reproducible runs).** 02 §8.13; `while (getUsed() < x)` loops end in deterministic runs.
12. **Deadlines: a watchdog polls worker CPU every 0.5 ms; at a deadline it calls `request_interrupt`, whose callback captures a 10-frame stack and calls `terminate_execution`; the `compileAndRun` boundary cancels termination and throws isolated-vm's text. A wall hard timeout `max(5000, mainLoopResetInterval)` resets the VM.** Uncatchable timeout with exact error text (14 pitfalls, RUN-TIMEOUT).
13. **`cpuMaxPerTick` keeps the vanilla default 500; the 1000-player config sets 100–150.** Tick time ≥ the slowest player; this vanilla knob is the only bound on `max(B)`.
14. **Memory governor: per-isolate limit = vanilla `256 MB + terrain MB` (configurable); a global budget; after P1, over budget ⇒ `low_memory_notification` on isolates idle this tick (largest first), then reset the largest.** 1000 × 256 MB does not fit; a reset is vanilla-reachable (02 VM-RESET-SUMMARY).
15. **No V8 flags (tests add `--random-seed`); microtasks `Explicit`, drained after output capture, an unhandled rejection fails the tick; sandbox `Date` virtual via native templates, `TZ=UTC`; `Atomics.wait` stays allowed.** ENV-MICROTASKS, CLK-SANDBOX-DATE; no unlisted deviation (the hard timeout bounds a waiting bot).
16. **Persisted world = vanilla DB documents verbatim: ordered `Obj` of `Val` with interned property names (`Atom`, `atom!` at call sites).** REC-MIRROR-DATA: mod fields, key order and wire shape come free; typed models failed before (13 ARCH-*).
17. **Room state is an immutable `Arc<RoomSnapshot>`; docs are `Arc<Doc>` shared until written (copy-on-write per doc); an unchanged room keeps `next == prev` (pointer).** No locks; runners, backend and checkpoints read consistent views; JSON caches survive.
18. **The processor runs on a per-room live heap: an `Rc<RefCell>` graph with JS identity, ported lodash 3.10.1 `merge/cloneDeep/isEqual/assign` and the `_.forEach` key snapshot; `Bulk` payloads hold live references and serialize at `execute`; persisted next = prev docs + applied `DbOp`s.** BULK-*, VIS-INMEM and the PROC-ERROR discard hold by construction.
19. **Ids and `seq` are assigned only in serial passes:** room inserts in P4 (RoomIdx order, then bulk counter order), global/cron/API inserts in call order. Id = bijective 60/96-bit mix of a counter, 15 hex (`private`) / 24 hex (`mmo`), skipping imported ids. Deterministic under any thread schedule; ID-LENGTH; no clock/RNG.
20. **Creation order is one global `seq: u64`; rooms keep docs in seq order; cross-room moves keep seq.** 11 ORDER-ROOM-OBJECTS.
21. **Rooms are found through a dense grid over the world bounding box (4 B/cell).** O(1); 256 KB at 256×256.
22. **Active set exactly vanilla (01 ACT-*), consumed at P3 start; force-update through a min-heap.** Staleness is player-visible; sleeping rooms cost 0.
23. **Global stage single-threaded over indexes; each vanilla `getInterRoom` query result becomes its own live graph.** O(work); same-document/different-object quirks reproduce (power-creep tombstone at the stale position).
24. **ORD-POLICY, RNG-SEED, CLOCK-POLICY and COMPAT-TABLE live in `engine::policy`; nothing else reads a clock or an unseeded RNG for game state (grep test).** 01 §16/§19A are normative.
25. **Crons run in CRON-STAGE at `vclock(T+1)` with cron RNG streams; independent units run on the pool.** 01 CRON-STAGE.
26. **`engine` has no V8 and no networking (it does own checkpoint/import file I/O); `RunJob/RunInput/RunOutput` and `Command` live in `engine`; only `runtime` and `server` link V8.** ~10 parallel worktrees on 32 GB cannot all build V8.
27. **Backend on `std::net`: thread per connection (+ writer thread per WebSocket), one broadcaster; reads use the published `WorldView` lock-free; writes are `Command` closures run on the tick thread in P0; one diff per room per tick, same bytes to every subscriber.** std-only; a slow client cannot stall the tick.
28. **Auth: stateless HMAC-SHA1 tokens (300 s, fresh `X-Token` per response); steam-ticket stub (`single-user:<name>` for local play, `trust`, `off`); screepsmod-auth password signin/register + HTTP Basic (PBKDF2-HMAC-SHA1).** Covers the official client, steamless client, rollup/grunt/screeps-api; std has no TLS.
29. **Persistence: incremental per-sector checkpoints written from a `WorldView` on a background thread (only sectors whose snapshot pointers changed) + an fsync'd account journal (users, auth, code) written before the HTTP reply.** Zero tick stall; code and accounts never lost; loss window = interval, like Loki autosave.
30. **Official Loki `db.json` import and export.** Reuse official worlds; export lets the official server replay chitin worlds.
31. **Mods: Rust mods in a compile-time registry frozen at startup; JS mods with a manifest, game slot in the snapshot, server slot in one privileged server isolate; one DFS load order (siblings by name) persisted in the world; no JS in the room/global phase.** xxscreeps shape (12 XX-REC-COPY), minimal.
32. **CLI: TCP line protocol on 127.0.0.1:21026, evaluated in the server isolate (JS ports of `backend/lib/cli/*.js` over natives).** Vanilla admin scripts are JS.
33. **World: exact inside W127..E127 × N127..S127 (65,536 rooms); `world.wideCoords` opt-in for larger worlds changes `__packedPos` (documented divergence); Rust uses `i32` room coords everywhere.** The 32-bit packing is bot-visible (03 pitfalls).
34. **Terrain: packed 625 B/room always; unpacked 2500 B per room lazily for Rust readers; isolates get per-room copies on demand.** No shared writable buffer a bot could corrupt.
35. **Memory, segments and InterShardMemory are `JsStr` (Latin-1 or UTF-16, lone surrogates kept); documents are UTF-8 (lone surrogate → U+FFFD, listed deviation J6).** RawMemory accepts any string and vanilla's RPC keeps it.
36. **Memory stays resident: each VM keeps its last Memory as a `Global<String>` keyed by `memory_version`; JS returns `undefined` when the end-of-tick string equals the input.** Unchanged Memory is never copied in or out.
37. **Each room runs under `catch_unwind`; a panic = COMPAT-CRASH-ROOM outcome + log + re-activation.** One bug cannot stop the world.
38. **The game API is our own JS, one file per vanilla file, each keeping its strictness, bundled webpack-3 style into one script named `<runtime>`, `include_str!`ed.** ENV-ENGINE-STRICTNESS, ENV-SNAPSHOT, 13 ARCH-HANDWRITTEN-API.
39. **Natives mirror the official addon (`nativeMod.search/loadTerrain/version`) and the isolated-vm pieces vanilla uses (`cpuTime`, `compileAndRun`, heap stats, `halt`).** The JS port runs unchanged in node against the official `native.node`, so the API oracle is pure JS-vs-JS.
40. **Testing backbone: node oracles run official code (processor over a fake driver using the real `bulk.js`, global stage, crons, the runtime bundle under an isolated-vm shim) with seeded RNG/vclock; fixtures committed; live fuzz mode; a recording mod makes the official server replayable exactly. No file is FAITHFUL without fixtures.** REC-ORACLE; the repo's `tools/*-gen-fixture.js` pattern.
41. **Skeleton-first delivery: WP00 creates every crate/module/JS file as a compiling stub with the FROZEN signatures, then implements the shared semantic core; every later WP edits only files it owns.** Parallel agents never touch the same file, and porters can test from day one.

---

## 2. Process, threads, tick pipeline

### 2.1 Threads (one process, `krebs.exe`)

| thread | count | role |
|---|---|---|
| tick (main) | 1 | owns `&mut World`; runs P0, P2, P4–P7; pool caller in P1/P3 (participant 0) |
| pool workers | logical − 1 | `util::pool`; runner jobs (P1, at most `runner.threads` at once), room jobs (P3), cron units, mapgen |
| watchdog | 1 | CPU deadlines, wall hard timeout; polls every `runtime.watchdogPollUs` (500) |
| V8 platform | 4 | `new_default_platform(4, false)`; GC/compile helpers |
| snapshot drop | 1 | frees the room snapshots `World::view` lets go of (replaced two views back): ~8 ms per tick of docs, blobs and JSON caches on the grown world, off the tick thread |
| server-js | 1 | privileged isolate: CLI sessions, JS-mod server slot |
| http accept + conns | 1 + 1/connection | HTTP/1.1 keep-alive; WebSocket = reader + writer thread; 128 KiB stacks; cap `http.maxConnections` (2048) |
| broadcaster | 1 | `TickReport` → room diffs, map views, user channels |
| persist | 1 | checkpoints, account journal |
| history writer | 0/1 | room-history mod only |

`main` sets `TZ=UTC`, calls `runtime::init_v8()`, and only then builds the pool and every other thread (PKU note in
`locker.rs`). The tick thread is the thread that initialized V8.

Measured, not levers (M1 bots, 8 runner threads, 8-core/16-thread desktop): 2 platform workers ≈ 4, 8 is ~10% slower,
`--single-threaded-gc` ~10% slower. Pinning runner loops to their own physical cores (affinity mask or ideal processor)
gains nothing on a quiet machine and adds straggler ticks under background load. P1 scales sub-linearly (per-bot JS
270 µs at 1 runner thread, 420 µs at 8) because it is memory-bound: one co-running streaming writer slows a lone
runner 4×, an ALU loop by 9%.

Also measured, not levers (screeps-sim region e10s30-e30s50, 3 bots): `--initial-old-space-size` 0 or 32 (vs 4),
`--no-parallel-scavenge`, `--minor-ms`, `--sparkplug-plus`, earlier Maglev/Turbofan thresholds and
`--no-lazy-feedback-allocation` all land within noise. The young generation is already at its ceiling (the ivm
formula's 4 MB gives 2 MB semi-spaces), and this V8 allocates every `JSON.parse` result young (no large-input
pretenuring), so scavenge cost follows what a tick keeps alive: parse ~0.6–1.1 MB and Game setup ~1.3–1.5 MB per
player, all of it live until the tick ends. The run phase's wall time is its slowest player; each player runs 22–33%
slower beside the other two than alone (memory-bound, as above).

Engine API inside a real player loop (DefaultO alone on that region, V8 sampler, 1 runner thread) is ~10% of the run,
and almost all of it is calls vanilla makes to builtins a player can replace, so it is not a lever either:
`console.addVisual`'s `JSON.stringify` per visual (2.1%), the Store constructor's `Object.entries`/`forEach`/
destructuring/`Object.defineProperties`/`Proxy` (1.9%), the lazy `defineGameObjectProperties` getters adding their own
`_x` properties (2.1%), RoomPosition's `Object.defineProperty` (2%, setup and loop), the Memory getters' `_` and
`Memory` reads (~1%), `Game.cpu.getUsed`'s thread-cycle syscall (0.7%) and PathFinder's JS glue (~1%, the native search
itself 5%). `find`/`lookAt` cost <0.2% themselves; their time is the bot's filter callbacks run by lodash. What could
be cut without a visible change (one lookup in `data(id)` for string ids, memoised room names in `fromWorldPosition`)
is under 0.5% together, below the bench's noise.

What a tick keeps alive is not a lever either (DefaultO alone, heap snapshots at the tick's start, after input parse and
after Game setup): parse keeps ~415 KB (3.2 k doc objects, the roomObjects/userObjects dictionaries), Game setup ~720 KB
(~320 KB of register and `Game.*` dictionaries, 1.35 k game objects and their RoomPositions, find and spatial arrays).
Keeping another 600 KB of constructor-made objects alive to the tick's end costs 0.1–0.3 ms of a 17 ms run over 12
interleaved rounds and adds ~0.1 MB per tick to what scavenges copy (a ballast of object literals costs nothing: its
allocation site gets pretenured). Scavenge time (~4 per run, ~0.35 ms each) follows the bot's own 6–9 MB per tick of
loop allocation. Reusing unchanged docs across ticks would save their parse instead (~1 ms per DefaultO run), but
`creep.body` hands the player the raw doc's array, which vanilla makes fresh every tick.

### 2.2 Pipeline (`crates/server/src/tick.rs`, mirror of `engine/src/main.js` + `runner.js`)

```
          ┌─────────────── tick T (world.time == T) ─────────────────────────────────────────────┐
 P0 API   │ drain Commands FIFO (backend, CLI); manual/global API intents; activations → active   │
 P1 RUN  ═│ users_to_run (TICK-GET-USERS + RUN-SKIP checks) → runtime.run_phase: per user build    │
          │ RunInput, run isolate, system isolate → RunOutput (intents parsed+routed on worker)  │
 P2 SAVE  │ serial, user creation order: billing, Memory/segments, console/visuals → report,     │
          │ intents → per-room buckets, global docs, notify; active ∪= rooms with intents         │
 P3 ROOMS═│ active = world.active.take(); pool.map_dyn(LPT by last cost): process_room           │
 P4 MERGE │ serial, RoomIdx order: ids/seq for inserts, install snapshots, user ops, foreign     │
          │ inserts, indexes/presence, activations (T+1), notifications, history                  │
 P5 GLOBAL│ inter-room moves → power → market (global.js order); activations (T+1)               │
 P6 POST  │ time = T+1; TICK-ACCESSIBLE (21-tick cadence, if dirty); CRON-STAGE at vclock(T+1);  │
          │ publish WorldView(T+1) + TickReport; checkpoint trigger; mainLoopStage hooks          │
 P7 PACE  │ wait max(tickRate − used, 0) applying Commands as they arrive; paused ⇒ P0 loop only │
          └──────────────────────────────────────────────────────────────────────────────────────┘
```

Barriers: P1→P2, P3→P4, after P5 (01 §19 item 1). Phase visibility (exactly vanilla):

| reader | sees |
|---|---|
| runners of T | state after global(T−1), crons at G=T, Commands applied before P1 (VIS-RUNTIME-SNAPSHOT) |
| room of T | same state + P2 user-doc saves of T + its intents; user docs frozen at P3 start, per-room clones (PROC-INPUT, VIS-USERS-STALE) |
| global of T | every room commit of T (GLOB-INPUT) |
| crons | post-global state, `vclock(T+1)` (CRON-STAGE) |

Allowed differences: TICK-WATCHDOG not replicated (the per-VM hard timeout bounds runs; rooms cannot hang);
TICK-ERROR cannot occur (COMPAT-CRASH-GLOBAL); TICK-EMPTY-ROOMS: missing set = empty.

### 2.3 P1 runner phase

* `engine::runner::make::users_to_run(&World, T) -> (Vec<RunJob>, Vec<RunOutput>)`: eligibility `active != 0 && cpu > 0`
  with Loki operator semantics, RUN-SKIP penalty, CPU-NEGATIVE-SKIP, RUN-DEACTIVATE (presence has no objects).
  Skipped users get their `RunOutput` directly.
* Jobs sorted LPT by last `used_dirty_ms` (vanilla's sort, never a game order). `run_phase` calls
  `pool.for_each(runner.threads, |_| worker_loop(pool::worker_index()))`; each loop pops the shared LPT cursor until
  empty. M5 replaces the cursor with per-worker home lists + steal-largest (`runtime/src/sched.rs` only).
* Per job, on the worker: `engine::runner::data::build_input(&World, job)` → lock isolate → `__tick` → capture
  output → unlock → system isolate `__store` → parse + route intents (`engine::intents`) → `RunOutput`.
* `World` is immutable during P1 (the tick thread is blocked as pool caller); workers read `&World` directly.
  Lazily built room blobs use `OnceLock`.

### 2.4 P3 room phase

* `pool.map_dyn(active.len(), |i| catch_unwind(|| processor::process_room(&ctx, r, &world.rooms[r], bucket[r])))`
  over `active` sorted by `world.index.room_cost_ns` descending. Rooms read only `&TickCtx` and their own
  `Arc<RoomSnapshot>`; everything that crosses rooms is returned in `RoomResult` (§5.1).
* Movement, NPC pathing, RNG stream and event log are per room (01 PROC-LOOP). One `pathfinder::PathFinder` per
  thread in a `thread_local!` (plus a per-thread stack for recursive searches).

### 2.5 P4/P5/P6

* P4 (`processor::install`): §5.5. P5 (`processor::global::run`): §5.6. P6 order: `time += 1` (TICK-INC-TIME);
  if `T+1 > last_accessible + 20` and the status-dirty flag is set (map edits, `openTime` deadlines), rebuild
  `AccessSnapshot` at `vclock(T+1)`; CRON-STAGE (§5.9); `published.set(world.view())`
  (O(rooms) pointer copy, ≈ 0.4 ms at 100 k rooms *(est.)*); `backend.on_tick(report)`; checkpoint trigger
  (wall clock, CLK-INFRA); mod `main_loop_stage('finish')` hooks; `next_tick_delay` hook.

### 2.6 Active-room set (exactly vanilla)

`ActiveSet { bits: Vec<u64>, list: Vec<RoomIdx> }`, insert without duplicates (ACT-SET).

| source (ACT-SOURCES) | written | processed |
|---|---|---|
| runner intents for the room (object or `room` intents) | P2 of T | T |
| API place-spawn, create-construction, add-object-intent, create-invader; CLI `bots.spawn` | P0 of T | T |
| processor self-activation (ACT-RULE 1–7 on pre-intent objects; none on crash) | P4 of T | T+1 |
| global inter-room move destination | P5 of T | T+1 |
| crons (roomsForceUpdate, power banks, invaders, deposits, strongholds) | P6 of T | T+1 |
| nuke launch into another room | — | **not activated** (XROOM-NUKE, private) |

`take()` at P3 start, so anything activated during P3–P6 lands in the next set. Intents naming rooms absent from
`RoomsMeta` are dropped in P2 and still billed (BUG-CRASH-UNDEF-ROOM, F). Inactive rooms are never touched: no
prescan, no timers, no event log, no map view (ACT-STALENESS). roomsForceUpdate pops
`index.force_update: BinaryHeap<Reverse<(u64, RoomIdx)>>` lazily (the room doc's `nextForceUpdateTime` stays
authoritative; stale entries are discarded on pop).

### 2.7 Order, randomness, clock (`engine::policy`, 01 §16)

* ORD-POLICY point 1: users of a room sorted as canonical array-index ids ascending (`"2"`, `"3"`), then players by
  `(order_key(seed, T, uid), uid bytes)`. Points 2–3: `users.intents` docs by owner rank, API docs (arrival order)
  before the runner doc; deal ties via the stable key `(distance, ownerRank, docIndex, dealIndex)`. Point 4: lodash
  `_.shuffle` draw pattern on the global stream. Test hook `TickCtx::order_override: Option<&[UserIdx]>`
  (ORD-REACHABLE replays).
* RNG-SEED: `seed(tag, parts)` exactly as 01 RNG-SEED; `util::rng::Rng::new(h)` already is `stream(h)` and
  `next_f64` is `random()`. Streams: room `(room, T)` created on first draw; global `(T)`; cron `(job, G, unit)`.
  lodash draw helpers (`random`, `shuffle` = n draws, `sample` = 1/0 draws) live in `policy.rs`.
  `Rng::shuffle` is not lodash-compatible and is banned for game state (grep test).
* Clock: `VClock::at(t)` (CLOCK-VIRTUAL); processors/global use `vclock(T)`, TICK-ACCESSIBLE/crons `vclock(T+1)`,
  API `vclock(G)`, sandbox CLK-SANDBOX-DATE. Real time only for CLK-NOTIFY, CLK-INFRA, CLK-API real rows and auth,
  always through `policy::real_now_ms()` with the CLOCK-POLICY row named at the call site; a grep test forbids
  `SystemTime::now` elsewhere (`Instant` for pacing/deadlines is fine).

### 2.8 Pause and tick rate

`env.paused` (CFG-PAUSE) and `env.tick_rate_ms` (CFG-TICKRATE, default 1000; benchmarks 0) are world state, set by
CLI `system.pauseSimulation/resumeSimulation/setTickDuration`. While paused the tick thread waits on the command
channel (100 ms timeout) and applies Commands, so uploads and API writes still land. `system.runTicks(n)`
(chitin extension, PROTO-DEVTOOLS) runs exactly n ticks while paused. Changing `nominalTickMs` rebases the vclock.

### 2.9 Cost model: why 1000 players on a 65 k–100 k-room world stay fast

```
T_tick ≈ max( Σᵢ(Bᵢ + oᵢ) / N_r , maxᵢ(Bᵢ + oᵢ) ) + Σ_active c_r / N + P_serial
```
`Bᵢ` bot CPU, `oᵢ` per-player overhead (meta + blob hand-off + `JSON.parse` + Game construction + intents out),
`N_r` runner threads, `c_r` room cost, `P_serial` = P0+P2+P4+P5+P6. chitin controls `o`, `c`, `P_serial` and that
sleeping rooms cost nothing; `max(B)` is bounded only by `cpuMaxPerTick` (D13).

| work per tick | complexity | constant *(est.)* | 1000 players, 65 k rooms, ~7 k active |
|---|---|---|---|
| pre-run checks | O(P) | 1 µs | 1 ms |
| per-user meta JSON | O(P × small) | 10–50 µs | 30 ms CPU |
| room blobs (first viewer builds) | O(changed docs) + concat | 0.7 µs/doc, ~10 GB/s copy | ~70 ms CPU |
| `JSON.parse` + eager Game construction | Σ visible objects | ~2–2.5 µs/object-view | 600 k views ⇒ ~1.5 s CPU |
| bot JS | Σ Bᵢ | — | ~5.4 s CPU |
| outMessage clone + sanitize + route | O(intents) | ~50 µs/player | 50 ms CPU |
| P3 rooms | O(active rooms × objects) | 20–300 µs/room | ~0.7 s CPU |
| P2 + P4 + P5 + P6 | O(players + changed rooms + global work) | — | ≤ 15 ms serial *(target)* |
| CRON-STAGE | O(due units); force-update heap | — | ~600 wakes/tick ⇒ ~10 ms CPU in P3 |
| backend diffs, checkpoints, history | off the tick thread | — | 0 on the critical path |

16 physical runner threads: (5.4 + 1.6) s / 16 ≈ 440 ms + rooms 0.7 s / 32 ≈ 22 ms + serial ≈ 15 ms ⇒
**≈ 2.1 ticks/s eager, ≈ 2.4 ticks/s with lazy delivery (≈ 0.6 µs/view)** *(est.)*. Unbilled overhead ≈ 30 % of
ΣB eager, ≤ 15 % lazy. Terms that grow with world size but not activity: memory (§3.9), the O(rooms) view copy
(0.4 ms at 100 k), TICK-ACCESSIBLE rebuilds only when dirty, and crons defined per bus room (every 301 ticks).

Targets (gates for WP19, measured by `tools/bench`):

| scenario | hardware | target |
|---|---|---|
| 2 simple bots (~4 ms), 2×2 sectors | dev box | ≥ 100 ticks/s (vanilla ≈ 3) |
| idle 65 k-room world, no players | dev box | ≤ 2 ms/tick + force-update wakes; RSS ≤ 1 GB |
| 100 bots × 5 ms, 10×10 sectors | 16C | ≥ 20 ticks/s |
| 1000 small bots (2 ms, ~200 visible objects) | 16C/64 GB | ≥ 5 ticks/s |
| 1000 mixed bots (ΣB ≈ 5.4 s), 256×256 rooms | 16C/64 GB | ≥ 2 ticks/s (M5) |
| dev-box proxy: 500 mixed bots | 8C/32 GB | ≥ 1 tick/s |
| `c_r` | — | ≤ 20 µs woken idle room; ≤ 300 µs RCL8 room with ~600 objects |
| `oᵢ` with no objects, no bot code | — | ≤ 150 µs |
| P5 global with 1000 trading players | — | ≤ 5 ms |
| isolate creation from snapshot | — | 1000 in < 5 s; baseline ≤ 5 MB each |

**Measured capacity** (2026-10-25; i9-11900K, 8 cores/16 threads, 32 GB; 8 runner threads). `chitin bench --scenario
<screeps-sim e10s30-e30s50> --tile N --ticks 200 --warmup 50`: N copies of that region's three bots, each
rebuilding from its spawns with empty Memory (ticks 50–250 of the rebuild, ~65 creeps per player at the end).

| copies | players | rooms (with walled fill) | rooms processed | ms/tick | Σ bot CPU | P1 run | P3 rooms | process memory |
|---|---|---|---|---|---|---|---|---|
| 1 | 3 | 279 (529) | 48 | 20.3 | 31.5 ms | 17.9 ms | 2.1 ms | 355 MB |
| 4 | 12 | 1,116 (2,809) | 190 | 41.6 | 172.8 ms | 33.6 ms | 6.3 ms | 1,033 MB |
| 9 | 27 | 2,511 (6,889) | 429 | 98.4 | 395.8 ms | 78.8 ms | 15.8 ms | 2,077 MB |

- Once the runner threads are full, a tick grows by ≈ 3.6 ms per player of this weight: 14.7 ms of bot CPU each, as
  the bots' own `Game.cpu.getUsed()` reports it under load (9.8 ms alone). The box turns ≈ 4.0 s of player CPU into
  1 s of tick.
- 8 runner threads are 3.4× one (334.4 ms/tick at 1): every run is 1.5–1.8× slower beside seven others
  (memory-bound, §2), input building included (0.4 ms per player alone, 1.5 ms under load; not a scan of the
  world). 16 threads (SMT): 87.2 ms/tick (−11%) but 15% more CPU billed to the players, so the default stays at
  physical cores.
- Rooms cost time only while active: P3 ≈ 37 µs of tick per processed room (25 µs since, below), ≈ 16 per player
  here. Idle rooms cost memory only. Memory ≈ 72 MB per player (VM heaps ≈ 50 MB physical each) plus ≈ 140 MB.
- At 1 tick/s on this box (linear projection): ≈ 4,000 ms ÷ average player CPU, so ≈ 270 players at this bench's
  15 ms (≈ 20 GB), ≈ 200 at 20 ms (shard3's cap), ≈ 80 at 50 ms, ≈ 40 at 100 ms. No tick is shorter than its slowest
  player, whom `cpuMaxPerTick` (500 ms) bounds. screeps.com the same day: shard0 204 players and 6,866 rooms at
  3.78 s/tick, shard1 251 and 10,512 at 4.25 s, shard3 991 (20 CPU cap) and 10,374 at 4.22 s.

**Grown world.** The table measures bots rebuilding from nothing. `chitin bench --save <dir>` writes the world as a
run ends (a checkpoint as `serve` writes one: rooms, users, code, Memory, segments) and `--load <dir>` starts from
one, so a world grown once is measured again and again from the same state (the bots' globals reset, as after a
code push; warm up before measuring). The 9-copy world above, run 1,500 ticks and saved, holds per copy: bot A
23 owned rooms (18 claimed in the run) and 13 reserved remotes, DefaultO 6 and 16, bot B 3 and 6; 4,495 creeps
in all. Measured from it (ticks 50–250 after loading): 215.6 ms/tick, 838 rooms processed, Σ bot CPU 1,277 ms
(47 ms per player), P1 177.7 ms, P3 21.8 ms, process memory 2.8 GB. Input building is now 80 ms/tick of CPU, store
40, capture 31. Growing it further stopped at the 4 GB cap 630 ticks on (27 heaps jumped 2.8 → 4.2 GB within 10
ticks), so ~3 GB is where 27 grown players of this kind sit.

**Serial phases on the grown world.** P6 cost 8–9 ms per tick, nearly all of it `World::view` freeing the snapshots
replaced two views back (their docs, blobs and doc JSON) as the spare view took the new ones. Those drops now go to the
`snapshot-drop` thread (§2.1) and P6 takes 0.2–0.9 ms; interleaved A/B, 3 rounds: 206.5 → 195.0 ms/tick. More
runner threads than cores buy throughput with player CPU, as decision 3 expects: 12 and 15 (vs 8) cut ms/tick 12% and
14% and raise Σ bot CPU 8% and 12%.

P4 spent ~2.2 of its ~2.9 ms on object-index upkeep: a merge over every doc of ~770 rooms (~130 k pointer reads into
cold docs) to find ~6 k changes. The merge now skips unchanged doc pointers without reading the doc and runs in P3
(§5.5), where it does not show in µs per room. Interleaved, 4 rounds: P4 2.98 → 1.62 ms per tick; P3 within noise
(`--p3-repeat 3`: 157.0 → 155.4 µs per room).

**Segment Heap.** The binary's manifest (`crates/server/chitin.manifest`, embedded by build.rs) opts the process into
Windows' Segment Heap. The NT heap's lock was contended on the grown world: input building took 19.7 ms of CPU per
tick on one runner thread and 79 ms on 8, and capture and store grew alike. Interleaved A/B,
4 rounds: input building 79 → 36 ms/tick of CPU, capture + store 64 → 33, JS Memory + out 34 → 23, P3 22.3 → 19.0 ms,
whole tick 209.8 → 196.1 ms. On one runner thread the JS stages and Σ bot CPU are unchanged (to 0.3%) and process memory
is 5% lower; on 8, Σ bot CPU rises 2.8% because the players' JS now overlaps more of the time (memory-bound, §2.1),
as with any cut to P1's non-JS work. `--p3-repeat 3`: 166.7 → 161.4 µs per room.

**Old-generation growth (opt-in).** Full GCs are ~10% of the grown world's CPU samples (concurrent marking, sweeping,
and on the runner thread a ~5 ms atomic pause plus ~7 ms of incremental steps each). A player's tick data (input parse
and Game setup, 4–5 MB) outlives the young generation and is promoted, so each VM runs a mark-compact every 3–4 runs
(`--trace-gc`: 385 in 50 ticks × 27 VMs, ~60 → ~43 MB each). V8 treats a heap capped near 256 MB as a small device
and grows the old generation at most 1.3× between full GCs (heap-controller.cc `MaxGrowingFactor`).
`CHITIN_V8_FLAGS=--heap-growing-percent=100` (2×) trades memory for speed. Over 6 interleaved rounds it cut ms/tick
by 5–13% and Σ bot CPU by 2.5–6.5% (players are billed for their VM's GC). Process memory rose by 460–560 MB (+17–21%,
~17–21 MB per VM), and that cost grows with the empires. 1.5× gains nothing measurable; `--sqrt-allocation-limits` is
as fast as 2× and needs 4.7 GB (+77%). Either way `heap_size_limit` is unchanged. The default stays V8's, because
memory per VM is what bounds the players per host.

**JIT recompiles.** Even 200 ticks after loading, Maglev and TurboFan compiles take ~8.6% of the grown
world's CPU samples (`--trace-opt --trace-deopt`: ~560 Maglev compiles a tick, ~21 per player). Few follow a deopt
(~29 a tick). Nearly all follow "dependent property cell changed" (~500 a tick): code that read the global `Memory`.
Each tick vanilla's game.js init redefines `Memory` as an accessor (data → accessor), and all three bots' memhack
deletes it and assigns last tick's object. On V8 either step replaces the global's property cell and deoptimizes
every function compiled against it. Both are what players see (the descriptor, their own code), so the server keeps
them. The compiles run on V8's platform threads; the deoptimized functions run in Sparkplug until re-optimized.
The opt-in build's V8 (`tools/v8-ptrcomp.patch`, below) compiles reads and writes of a global named `Memory` to the
global IC instead of the cell (Maglev's `TryBuildPropertyCell{Load,Store}`, TurboFan's `ReduceGlobalAccess`). The IC
follows a replaced cell, so nothing deoptimizes, and what JS sees is unchanged (conformance
`hot_code_sees_every_memory_replacement`). Over 60 ticks "dependent property cell changed" deopts fall 20,073 → 78
and Maglev compiles by half. Interleaved, 8 rounds on the grown world: ms/tick 251.3 → 212.4 (−15%), Σ bot CPU −9%;
from the scenario's start, 4 rounds: Σ bot CPU −5%, ms/tick within noise (86.1 → 89.0). A game was loading the
machine meanwhile, which makes the platform threads' compiles cost the runners more, so on a quiet machine the gain is
likely smaller. The default build against this one, same conditions, 4 rounds: 287.6 → 211.0 ms/tick (−27%).

**Young generation (opt-in).** The other way to keep a tick's data from being promoted is a young generation that holds
a whole run. `CHITIN_V8_FLAGS=--max-semi-space-size=8` (vs the ivm formula's 2 MB), 3 interleaved rounds: ms/tick
203.4 → 191.6 (−6%), Σ bot CPU −3%, process memory +450 MB (+18%); 16 MB: −10%, −6% and +1.1 GB (+45%). The flag
raises `heap_size_limit` by three times the added semi-space (18 MB at 8). Same trade as the old generation's growth,
so the default stays.

**Also measured on the grown world, not levers.** Run-phase scheduling: the 8 runner loops sit idle ~12 ms at the end
of a ~164 ms P1 (per-thread timelines, 40 ticks). Replayed on the recorded run times, any order planned from last
tick's times (or `lastUsedDirtyTime`) ends within ~1 ms of LPT, because a player's run time moves ~17% from tick to
tick; knowing this tick's times would reach ~151 ms. Creep docs: half of the docs J11 always sends fresh (creeps,
spawns, tombstones) are unchanged since the VM's last run, yet taking the unchanged creep docs over (a test build that
shares `creep.body`) cuts JS input parse only 125 → 105 ms/tick of CPU, ~1.6% of the run phase, and a fresh body
would give part of it back. Megamorphic load/store stubs are ~13% of the whole process's native samples and Sparkplug's
generic loads ~4%: game objects of one class take many maps because the lazy getters add their `_x` properties in the
order the player reads them (`Object.keys` shows it), plus the bots' own code.

**Pointer compression (opt-in build).** v152.2.0 publishes no Windows `_ptrcomp` build, and V8's default shared cage
puts all isolates in one 4 GB reservation. `tools/v8-ptrcomp.ps1` builds V8 from source with
`v8_enable_pointer_compression_shared_cage=false` and `v8__Isolate__New` passing `IsolateGroup::Create()` (a cage per
isolate), 8 interleaved rounds: ms/tick 201.3 → 183.0 (−9%), P1 −11%, Σ bot CPU −5.5% (input parse −11%, Game setup
−15%, player loop −10%), VM heaps physical 1540 → 1183 MB (−23%), process memory 2557 → 2286 MB (−11%), address space
59 → 196 GB (4 GB per isolate). Conformance passes, `thousand_isolates` too. Players see it: `getHeapStatistics()`
reports smaller sizes under the same `heap_size_limit`, so a bot holds more before the limit. The recipe adds what the
crate leaves out (`third_party/icu/common/icudtl.dat` and `third_party/rust/chromium_crates_io` at the commits
`v8/DEPS` pins, checked by sha256) and a real Python (V8's ICU step fails under the Store alias); the crate's build
script downloads Chromium's clang and Rust toolchains; ~40 min. Its version is `…-rusty-ptrcomp`: a startup snapshot
from the other library would kill the process (`magic_number_` CHECK), and the version is both in the snapshot cache
key and what `StartupData::is_valid` compares.

**Builtins PGO (in the opt-in build).** mksnapshot takes branch hints and block order for V8's builtins (ICs,
interpreter handlers, the built-in methods) from `tools/v8-ptrcomp.profile`: block counts recorded in the 27 VMs on the
grown world (50 + 100 ticks). Interleaved, 24 rounds: ms/tick −2.8%, Σ bot CPU −2.1% (faster in 16); from the
scenario's start, 5 rounds: 68.6 → 69.1, no change. Only the machine code's layout changes, not what it computes.
Chrome's profiles don't apply (their hashes are for the shared cage, sandbox and external code space). Builtin
reordering, V8's default with a profile, stays off: mksnapshot writes the Windows unwind table in builtin-id order and
clamps each entry to the previous builtin's end, so in a reordered blob entries invert and MSVC's linker refuses the
object (LNK1223); fixed in a test build (walking the blob's order), it was no faster than the hints alone (−2.2% vs
−3.1%, same 10 rounds). A changed builtin aborts mksnapshot (`--abort-on-bad-builtin-profile-data`), so a V8 upgrade
or GN arg change needs a new profile: build through the recipe with `GN_ARGS=v8_enable_builtins_profiling=true`, run
`CHITIN_V8_FLAGS=--turbo-profiling-output=<dir>/b.log chitin bench --load <grown world> --ticks 100 --warmup 50`
(the bench disposes its VMs at the end; `tools/v8-ptrcomp.patch` makes V8 write `b.log.<n>` per isolate instead of
truncating one file it never closes), and turn the concatenated logs into the profile with V8's
`tools/builtins-pgo/get_hints.py` (not in the crate; from tag 15.2.124.1), default thresholds.

**Room processing** (same bench, 27 players: 429 rooms per tick, ≈ 105 objects each). A room cost 554 µs of CPU, on 16
threads with 15.2 busy, so 15.6 ms of tick. The time went to converting every doc into the live heap and freeing it
(~20 %), small allocations (~11 %), SipHash lookups behind every `_.filter(roomObjects, …)` scan (~11 %) and the map
view's `to_val` copy. Now room-object scans walk the slots without lookups; maps keyed by ids hash with FxHash
(`util::hash`: room objects, the bulk store's index, `user_ids`); map view and event log serialize straight from the
live graph; integers print without `core::fmt`; and the allocator caches blocks up to 4 KiB per thread (alloc.rs:
creep docs and per-room tables sit just above the old 512 bytes). Interleaved A/B, 3 rounds: P3 14.6 → 10.8 ms/tick,
92.7 → 86.2 ms/tick. The allocator also cuts the runner's own stages: build input 37–44 → 23–25 ms CPU, capture + store
29–35 → 18–20. What is left is mostly memory traffic: the map view's JSON takes 4 µs alone and ~15 µs inside P3 (cold
live objects, 16 threads on 8 cores). `CHITIN_NATIVE_PROFILE_P3=1` samples P3 alone.

Room objects now start as their stored docs (`live::from_doc`, §3.2): primitive reads come off the doc and only the
first write or nested read copies it into the live heap. In the grown world 71 % of a tick's docs are never copied
(97 % of roads, 98 % of ramparts, every wall); creeps, labs, towers, links and dropped energy are written in the
prescan or their tick, spawns and extensions get `_off` from `_calc_spawns`. Whole-tick A/B runs from a saved world
still vary ±10 % (the bots are not deterministic), so `bench --p3-repeat N` runs each tick's rooms N more times on
the tick thread from the same snapshots and intents: identical input, ±2 %. On it a room costs 171 µs alone and
395 µs inside the parallel tick (2.3×: 16 threads on 8 cores, one memory bus). The first doc-backed version was
11 % slower alone (every read went through `from_val`, and `type_is` cloned the type string's `Arc`); with inline
primitive reads and `O::eq_str` it breaks even alone and saves 6 % in the tick (399.7/394.5 → 376.0/373.2 µs per
room), the memory traffic it removes being what the threads contend for.

**Lookups in the room handlers.** Movement's check called `checkObstacleAtXY` once per contested move, and each call
scanned every room object. Now the first call in a room sorts the objects by integer tile and later calls binary-search
the tile. Objects whose x/y are not integers 0–49 (strings, arrays, fractions, which vanilla's loose `==` may still
match) are tested at every tile, so the answer is the full scan's (unit test over all 2,500 tiles). Atom interning's
per-thread cache and movement's maps hash with FxHash. `{type: 's'}`-style matches compare the stored string in place
(`O::is_str`, `lodash_match_str`) instead of building a `V::str` and copying a doc's string out, and read-only scans
walk `room_objects` instead of a cloned list. Interleaved `--p3-repeat 3`, 4 rounds: 172.4 → 163.1 µs per room (−5.4 %;
the index and FxHash alone −3.0 %); inside the tick 376 → 364. (`RoomObjects::near` has since replaced this index, below.)

**One owned room.** At tick 560,421 of a one-bot match P3 is 15 rooms a tick, and one room of 531 objects (387 of
them roads and ramparts, 22 creeps) is over half its CPU: 1,292 of about 2,270 µs. The cost was not one scan but the
walk itself: every object tested against 30-odd type strings in the prescan and again in the tick loop, ids hashed per
object, a doc copied whole on its first write (`actionLog` every tick for creeps), and each creep's whole body copied
to read a few parts. Whole-tick A/B varies with the bot, so 2,600 ticks of this match's room intents were recorded and
replayed by a temporary patch (not in the tree): the same input for every binary, each room timed as its best of 5
repeats, and the binaries run back to back in pairs, because this machine's load moved a single run by ±25 %. A hash
per processed room (info, docs with seq, flags, event log, map view, presence, user refs, mover flags) and per tick
(every user doc, notifications, next seq) stayed byte-identical to the unchanged engine, all 40,931 lines.

What each change is worth, as the paired cost of taking that one out again (owned room, 8 rounds, median [min..max]):
`RoomObjects::near`, slots per tile kept exact through `insert`, `remove` and an `O::set`/`O::delete` of x or y, for
the scans that look for a place (`move.rs`, `execute`, `checkObstacleAtXY`, rampart, container and spawn lookups) +92 %
[84..102]; unboosted bodies read in place (`move_power`) +12 % [1..21]; the map view as tile numbers until `save` writes
the JSON, instead of an array per point, +9 % [5..13]; objects and body parts inside a doc read in place (`Props::Sub`)
+7 % [3..11]; `_.filter` for spawns and extensions answered from the prescan's lists while no key changed +7 % [2..9];
`quiet_tick`, which ends the tick of a stored road, wall or rampart with nothing to do on the values its handler would
read, and hands anything unusual to the handler, +5 % [4..11]; writes beside the doc instead of a copy of it (`Props::Doc`
now carries the list) +5 % [0..11]; the prescan and tick loop walking slots instead of hashing keys +4 % [2..7]; `save`
finding docs by slot instead of building an id index +2 % [1..8]. `near` falls back to a full scan while any object in
the room has a non-integer or non-number coordinate, so a loose `==` match cannot be missed; debug builds compare it
with the scan on every lookup, and a unit test walks 3,000 random changes against it. Text matrix keys as tile numbers,
atoms for the creep intent table, dispatch by `K` and `O::nulls` gave -1 to -2 % each, inside the noise, and -4.5 %
[-9.4..-0.6] together, so they stay as a group. Flat paths for `is_equal`, `clone_deep` and `merge` with stored user
docs, room info and flags gave -0.6 % [-2.6..0.4] alone but -8.7 % [-13.0..-5.4] with that group, so they stay too.
Rejected: `stored_in` for store reads (-1.2 %), a hash map
for `Bulk::updates` (+1.6 %, bulk.rs), a fast path in `calc_body_effectiveness` (+1.7 %, utils.rs), and reusing the
last snapshot's derived fields when no op touches them (-3.1 % [-14..37], no proof either way). Not proven and left in:
`eq_str` for `loose_eq(&o.get(k), &s("…"))` in 48 handler tests (0.0 % [-3.7..6.3]).

Interleaved, 3 rounds on the saved world with the bot running: P3 2.06 → 1.11 ms/tick (-46 %), 285.7 → 191.6 µs of CPU
per room, tick 13.70 → 12.90 ms (runs 12.9–15.0 against 12.7–13.1, which overlap). Replayed, paired, 6 rounds: owned
room 1,291.5 → 542.2 µs (-60 % [-66..-45]), all rooms 151.3 → 76.0 µs (-52 % [-58..-37]). What is left in the owned
room is `O::get` (about 10,000 calls a tick) and the creeps' own ticks.

**Game setup and store on the saved late world.** Match only: `runner.setupList`, off on the game server (see "What differs
in a match" below). Game setup walked `roomObjects` twice, once through `_.forEach` and once through for-in, on a dictionary
with no enum cache, and rebuilt tables that few calls read. With the option on, `__tick` hands over the raw docs as a list in
`roomObjects`' key order when the tick starts with lodash and `Object.prototype` as `_init` left them (the check doc reuse
already runs, which also ends for good when a mod defines object types), no id can sort as an array index and no room is
listed twice. On that path (`addRoomObject` in game.js) both walks use the list, `objectsByRoom` and `objectsByRoomKeys` are
internal Proxies that build a room's entry on its first read, the `ownedStructures` tables are not written, a room's
register is one literal, a doc's `type` and `room` are read once, a structure's table write is not read back, its `type`
tests end early, and roads and ramparts are constructed at a call site of their own. A tick that does not qualify, in a
match too, runs the merged code's statements, and with the option off the list is never built and those statements are all
there is: what the path adds to them, checked against the merged sources, is tests of a private flag and code that cannot
run. All Stores of a match VM share one Proxy handler while the global `Proxy` is the original; a replacement gets its own
for each Store.

The path holds against a bot that plays by the API, and against some that do not. Accessors on game classes run during Game
setup (`.my` sets `._my`), and `pristine()` runs once, before them, so an `Object.prototype` that was clean when the tick
started need not stay clean: most skips above test the name first (`id in Object.prototype`, `'off' in Object.prototype`,
`type in Object.prototype`) and then do vanilla's write or read. A room's first read can come after player code, so the docs
are grouped by room before the first constructor runs, the table is filled with no prototype and given its usual one at the
end, its entries in the two room tables are defined, not assigned, and the room leaves the list only once both tables hold
it, so a stack overflow in the build costs nothing. None of this closes the path: what still shows is listed below.
`tests/setup_parity.rs` (server) runs one bot that dumps everything Game setup builds, every object type on a scenario
world, with the option on and off, and the outputs must be equal; it and the `patched-*` and `store-patched-proxy` cases of
the API oracle (`--setup-list` runs them on the path) hold this.

Latin-1 strings out of the serialized outMessage are copied in one go when ASCII (visuals are long), instead of a char at a
time. `spawns/tick` compares `room_objects.version()` with the version taken before the prescan, not after it, so a
`process_object` hook that changes the room is seen, and `processor_oracle_without_history` replays the room fixtures with
the history mod off, the only way they reach `quiet_tick`.

**What differs in a match.** On the game server parity with vanilla's visible behaviour is a hard rule. A match world runs only
with the mods `mods/bare` starts it with, and `mods/bare/src/world.rs` writes its config. This is the whole list of what that
config changes against the game server's defaults, and of what a bot in a match can tell from vanilla because of it:
- `runner.semiSpaceMb` 16: the young generation holds a tick, which changes `heap_size_limit` in `Game.cpu.getHeapStatistics()`.
- `engine.cpuMaxPerTick` 300 and `persist.intervalS` of a year: the most any bot can use in a tick, and no checkpoints.
- `runner.setupList` true: Game setup's object list and one Store handler. A bot that leaves the prototypes and globals it
  was given alone sees everything Game setup builds as before, as the setup parity test holds, and can tell only by point 4.
  A bot that patches them on purpose can tell in the others, in its own VM only: none reaches another bot's VM, and
  none shows it more than vanilla's raw docs do. All are accepted because a match is where they cost nothing:
  1. The Store handler is reachable. A trap a Proxy finds on `Object.prototype` (`Object.prototype.has`, say) runs with
     `this` = the handler, and with one handler shared, a change to it reaches every Store of the VM.
  2. `pristine()` runs once, in `__tick`, and player code runs after it and before Game setup has taken or walked the list: a
     setter on `global.console` (`exports.init`'s `_.extend(globals, ...)`), a patched `Math.pow`, an accessor such as
     `Room.prototype.survivalInfo`, a getter on a class's global name. What such code does to `Object.prototype`,
     `Array.prototype` or lodash then reaches vanilla's statements and not the ones this path leaves out: an enumerable
     `Object.prototype` key is not walked as an object, a patched `_.forEach`, `Object.keys` or `Array.prototype.push` is not
     called for the room tables, and no setter of a table's name is called.
  3. The `ownedStructures` tables are not written, because the engine never reads them: `lookFor`, `lookAt`, `find`, paths and
     every other API read the `structures` tables, which hold every structure of a tile in vanilla's order. Two things show
     it. A tile with two owned structures, a rampart over a spawn say, makes one `push` fewer per structure past the first,
     which a `push` patched in the window of point 2 can count. And the tables are empty, which a getter on `Object.prototype`
     named after a room can read in the loop: `new Room(name)` reads `register.byRoom[name]`, so the getter gets `register.byRoom`.
  4. A room's first table read runs a Proxy trap and a build vanilla did before player code, so near the stack limit that
     read can throw `RangeError` where vanilla's has no call to make. The tables are not lost: the next read builds them.
  5. The room tables fill up on first reads, so they are not whole while the loop runs. The engine reads them by room name, but
     a getter on `Object.prototype` named after a room with no entry is called with the table as `this`, and that table, or
     one a setter got in the window of point 2, holds only the rooms read so far where vanilla's holds every room in sight.
  6. An error thrown inside Game setup, which takes an accessor on a game class, names this path's functions in its stack:
     `addRoomObject` and `addToFindCache`, not `addObjectToFindCache`.

Interleaved, 6 rounds, 4 of them merged first and 2 final first, medians, against the merged room code on fast V8, on a
loaded machine. As a share of the same run's `JS: player loop`, the bot's own work: Game setup 0.297 →
0.235 (-21 %, runs 0.294-0.304 against 0.232-0.248), store 0.038 → 0.028 (-25 %, runs 0.036-0.040 against 0.026-0.029). In
ms that was Game setup 1.99 → 1.76 and store 0.26 → 0.21, while every stage this work did not touch rose with the load
(build input +11 %, input parse +13 %, P3 rooms +7 %), so the whole tick did not show it: 12.00 → 12.80 ms, runs 10.5-16.0
against 11.0-15.9, which overlap. The player loop held against P3, 6.31 → 6.26 times P3 (runs 6.10-6.49 against 6.16-6.88).
A second run on what a match uses (`runner.semiSpaceMb` 16), 6 rounds, 3 each way round: tick 11.75 → 10.65 ms, the median
of the pairs -5 %, with pairs from -17 % to +7 %. So the whole tick gains about what the shares say, inside the noise of one run.
API oracle: 69 of 70 cases, `map-grid` fails as it did before this change, `require-hot-reload` reports STALE as before.
Match only, `runner.setupList` on against the merged code, both on `runner.semiSpaceMb` 16, 6 rounds each way round less one
that timed out, on a machine busy at 16 to 30 ms a tick: Game setup over the player loop 0.286 -> 0.235 (-18 %, runs
0.282-0.293 against 0.230-0.238), store over the player loop 0.033 -> 0.026 (-20 %). The whole tick stayed inside that load: the
median of the 5 pairs -1.6 %, pairs from -24 % to +51 %. With the option off against the merged code, 3 pairs: Game setup over
the player loop 0.285 -> 0.285, as it should be; store still 0.034 -> 0.027, from the ASCII copy in `runtime/src/store.rs`, which
is on both ways. API oracle with `--setup-list`: 69 of 70, the same `map-grid` failure and no other.

Tried and dropped, no gain shown: one copy of each room blob (build input 0.055 → 0.054 of the player loop, runs overlap),
one copy in `js_str` (capture 0.023 → 0.024), one slot lookup per `console.addVisual` call (about 0.5 % of JS samples in
a profile, 18 duplicated lines). Dropped because a player can see them: one lookup in `data(id)` (a getter on
`Object.prototype` named after an id that is gone counts vanilla's two reads) and one `type` read in the Structure
constructor. What is left in the engine's share is mostly V8 C++ that vanilla's semantics require, by share of JS samples:
`jsonParse(plans)` 4.0 %, `Object.defineProperty` in RoomPosition 3.9 %, `JSON.stringify` in `addVisual` 3.7 %,
`JSON.stringify(Memory)` 2.2 %, `Object.defineProperties` in Store 1.8 %.

Open, older than this work, from an accessor on `Object.prototype` named after an object's id or type: `__tick` fills
`roomObjects` and `userObjects` by assignment where vanilla's ExternalCopy defines them, so such a setter sees raw docs;
Game setup keeps `register._objects[i]` in a local where vanilla reads it back; and vanilla's per-type counter `c`, dropped
here, is a table such an accessor would see.

**The runner's own stages on a late one-bot world** (2026-11-13). A world from a bot test farm: one bot with 11 rooms and
about 545 creeps on 121 rooms, 53 rooms processed a tick, 38 rooms and 3,900 docs in its sight, the fixed meter, fast V8,
five other servers busy on the same machine. Of a 59 ms tick the bot's own loop was 30 ms. Around it, on the runner
thread: build input 2.6 ms (2.1 writing the JSON of the ~970 docs a tick changes, 0.55 concatenating 960 KB of room
texts that the VM never read, since doc reuse sends it a 465 KB plan), the plan 0.9 ms, store 1.9 ms. Now a doc's JSON
is written into a buffer its thread keeps and allocated once, each key quoted once per atom and plain strings copied in
one pass; P3 writes it for the docs it changes in a room that had a viewer; a room's text is put together only when a
VM asks for it (`natives.blob`); `DocCache` keeps per held doc whether it may be taken over; `sanitize_user_intents`
finds an object's names from its keys where it asked for all 66, and reads strings and whole numbers directly; the wire
reader borrows ASCII strings; and pool workers spin after a job only while jobs come back to back, which a tick's
never do. Interleaved, 4 rounds of 200 ticks: 59.3 → 55.3 ms/tick (runs 58.6–60.3 against 54.2–56.0), build input 2.59
→ 0.06, store 1.84 → 1.42, and the CPU of the whole process 100.2 → 87.1 ms a tick (96.9–103.3 against 84.4–90.5), which
is what counts when a farm fills every core. With the same seed both builds print the same console lines and export the
same world after 1,500 ticks, apart from what the bot writes from its own heap statistics and clock.

What that world's tick still holds, measured the same day: parsing the plan 3.0 ms (the 529 creeps are 372 KB of it,
fresh every tick as above, and 312 roads a tick change because a step wears them); `roomObjects`, `userObjects` and the
merge 2.5 ms; Game setup 13.6 ms, 11.2 of it the object loop at 2.9 µs an object; GC about 4.5 ms on the runner thread
(3.7 scavenges and 0.7 full collections a run on the default young generation, which `runner.semiSpaceMb` and
`--heap-growing-percent` above trade memory against). P3 costs 225 µs a room on one thread and 272 on sixteen;
`O::get` is a fifth of it at 131 k calls a tick, and two fifths of the entries it scans belong to keys the doc lacks.

Rejected: per-room readiness (above); overlapping P3(T) with P1(T+1) (players must see T's final state); binary
blobs with JS overlay views (rewrites every game file accessor, breaks the mirror rule, XX-COMPAT-STALE-OBJECTS);
building V8 objects from Rust (~60 ns/property, slower than `JSON.parse`, nothing shared).

---

## 3. World data model (FROZEN)

### 3.1 Persisted values (`crates/engine/src/doc.rs`)

```rust
/// Interned property name (process-local, never persisted). Only for property names: never intern ids or
/// user-controlled map keys. `atom!("store")` caches the Atom per call site (no shared key table).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Atom(u32);
impl Atom {
    pub fn intern(s: &str) -> Atom;            // append-only arena; lock-free hit path
    pub fn as_str(self) -> &'static str;
    pub fn array_index(self) -> Option<u32>;   // canonical array index ⇒ enumerated first (JS key order)
}
#[macro_export] macro_rules! atom { ($s:literal) => {{
    static A: std::sync::OnceLock<$crate::doc::Atom> = std::sync::OnceLock::new();
    *A.get_or_init(|| $crate::doc::Atom::intern($s)) }} }

/// Object / user / order id: ASCII, ≤ 24 bytes, inline, Copy.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id { len: u8, b: [u8; 24] }
impl Id { pub fn new(s: &str) -> Option<Id>; pub fn as_str(&self) -> &str; }

/// What the Loki JSON transport can carry. NaN/±Inf never appear (they persist as null).
#[derive(Clone, Debug, PartialEq)]
pub enum Val { Null, Bool(bool), Num(f64), Str(Arc<str>), Arr(Vec<Val>), Obj(Obj) }

/// Plain object in JS own-property order: array-index keys ascending, then insertion order.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Obj(Vec<(Atom, Val)>);
impl Obj {
    pub fn get(&self, k: Atom) -> Option<&Val>;
    pub fn set(&mut self, k: Atom, v: Val);          // existing key keeps its position
    pub fn remove(&mut self, k: Atom) -> Option<Val>;
    pub fn iter(&self) -> impl Iterator<Item = (Atom, &Val)>;
    pub fn len(&self) -> usize;
}
impl Val {
    pub fn from_json(j: &util::json::Json) -> Val;   // keys interned
    pub fn to_json(&self) -> util::json::Json;
    pub fn write_json(&self, out: &mut String);      // JSON.stringify-exact (util::num), non-ASCII as \uXXXX
}

/// One persisted document (rooms.objects, users.power_creeps, market.orders, …). Immutable once shared.
pub struct Doc { pub seq: u64, pub obj: Obj, json: std::sync::OnceLock<Box<str>> }
pub type DocRef = Arc<Doc>;
impl Doc {
    pub fn new(seq: u64, obj: Obj) -> DocRef;
    pub fn json(&self) -> &str;                      // cached ASCII write_json
    pub fn id(&self) -> Id;                          // obj._id
    pub fn kind(&self) -> &str;                      // obj.type
}
```

### 3.2 Live heap (`crates/engine/src/live.rs`) — one per room tick / global stage, `!Send` by design

```rust
#[derive(Clone)] pub enum V { Undef, Null, Bool(bool), Num(f64), Str(Arc<str>), Obj(O), Arr(A) }
#[derive(Clone)] pub struct O(Rc<Node>);              // JS identity = Rc::ptr_eq
struct Node { props: RefCell<Props>, watch: OnceCell<Watch> }   // `watch`: writes of x/y reach the room's tile index
enum Props {
    Doc(DocRef, Vec<(Atom, V)>),   // a room object reads off its stored doc; writes, and nested values once read (their
                                   // identity), go in the list beside it, and only what needs all keys copies it, once
    Sub(DocRef, u32, u32),         // an object inside a doc (`doc[key]`, or element `item` of that array), read in place
    Shared(Arc<Obj>),              // user doc, room info, flag: read in place, copied on the first write or nested read
    Live(Vec<(Atom, V)>),
}
#[derive(Clone)] pub struct A(Rc<RefCell<Vec<V>>>);
impl O {
    pub fn new() -> O;
    pub fn same(&self, other: &O) -> bool;
    pub fn get(&self, k: Atom) -> V;              // Undef when absent
    pub fn eq_str(&self, k: Atom, s: &str) -> bool;   // `o.k == "s"` without copying a string value
    pub fn has(&self, k: Atom) -> bool;
    pub fn set(&self, k: Atom, v: V);             // `o.k = v` (Undef keeps the key, like JS)
    pub fn delete(&self, k: Atom) -> bool;
    pub fn keys(&self) -> Vec<Atom>;              // snapshot (lodash _.forEach)
    pub fn num(&self, k: Atom) -> f64;            // ToNumber(o.k); Undef → NaN
    pub fn str(&self, k: Atom) -> Option<Arc<str>>;
}
pub fn from_val(v: &Val) -> V;                    // fresh graph, like a DB read
pub fn from_doc(d: &DocRef) -> O;                 // the same, copied only when needed (room objects at load)
pub fn from_shared(o: &Arc<Obj>) -> O;            // likewise (user docs, room info, flags)
pub fn to_val(v: &V) -> Option<Val>;              // JSON transport: Undef keys dropped, Undef in arrays → null,
                                                  // NaN/±Inf → null, -0 → 0
pub fn merge(dst: &V, src: &V) -> V;              // lodash 3.10.1 _.merge (baseMerge/baseMergeDeep, stackA/stackB)
pub fn clone_deep(v: &V) -> V;
pub fn is_equal(a: &V, b: &V) -> bool;
pub fn assign(dst: &O, src: &O);                  // _.assign / _.extend
pub fn truthy(v: &V) -> bool;
pub fn to_number(v: &V) -> f64;
pub fn to_js_string(v: &V) -> Arc<str>;           // ""+v (util::num, Array join, "[object Object]")
pub fn parse_int(v: &V) -> f64;
pub fn stringify(v: &V) -> String;                // JSON.stringify (history keeps `_` keys, as vanilla)
pub fn write_json(out: &mut String, v: &V, ascii: bool);   // to_val + Val serialization, without the copy
pub struct Crash(pub std::borrow::Cow<'static, str>);   // a vanilla `throw` in room/global processing

/// processor.js `roomObjects`: id-keyed, JS key order (integer-like temp keys first), snapshot iteration (PROC-ITER).
pub struct RoomObjects { /* slots in insertion order (holes once deleted) + FxHashMap<Arc<str>, slot> + tile index */ }
impl RoomObjects {
    pub fn get(&self, key: &str) -> Option<O>;
    pub fn insert(&mut self, key: Arc<str>, o: O);   // real ids and temp keys "1", "createdStructure3", …
    pub fn remove(&mut self, key: &str);
    pub fn keys(&self) -> Vec<Arc<str>>;             // snapshot for _.forEach
    pub fn iter(&self) -> impl Iterator<Item = &O>;  // key order, no lookups: scans that cannot change the map
    pub fn near(&self, x: &V, y: &V) -> impl Iterator<Item = &O>;  // what such a scan for a place has to test, same order
    pub fn key_order(&self) -> Vec<u32>;             // `keys` as slots; `at(slot)` is `get(key)` without hashing
}
```

### 3.3 Bulk writer (`crates/engine/src/bulk.rs`, mirror of `driver/lib/bulk.js` + storage `db.js` update rules)

```rust
pub enum Target<'a> { Obj(&'a O), Id(&'a str) }
pub struct Bulk { /* ops in call order, updates: id → payload O (live refs), inserts by counter, op count */ }
impl Bulk {
    pub fn new() -> Bulk;
    pub fn update(&mut self, t: Target, data: &V) -> Result<(), Crash>;   // object-valued key with Target::Id ⇒ Crash
    pub fn insert(&mut self, data: &V, id: Option<&str>) -> u32;          // temp counter, never reused (BUG-INSERT-COUNTER F)
    pub fn remove(&mut self, id: &str);
    pub fn inc(&mut self, t: Target, path: &[Atom], n: f64);              // live `o.k = (o.k||0)+n` for Target::Obj;
                                                                           // dotted key ⇒ nested (BUG-DOTKEY-INC F)
    pub fn add_to_set(&mut self, t: Target, key: Atom, v: &V);
    pub fn pull(&mut self, t: Target, key: Atom, v: &V);
    pub fn execute(self) -> Vec<DbOp>;          // payloads serialized NOW (to_val); vanilla op order
}
pub enum DbOp {
    Remove(Id),
    Inc(Id, Vec<Atom>, f64), AddToSet(Id, Atom, Val), Pull(Id, Atom, Val),
    Set(Id, Obj),                               // shallow _.extend(doc, $set)
    Insert(Obj),                                // `_id` assigned in the serial id pass unless given
}
pub trait DocStore {
    fn doc_mut(&mut self, id: &Id) -> Option<&mut Obj>;   // copy-on-write of the shared Arc<Doc>
    fn remove(&mut self, id: &Id);
    fn insert(&mut self, doc: Obj);                        // staged; seq/_id later
}
/// dbBulk: ops in order; update of a missing id is a no-op; per doc $set, $inc, $addToSet, $pull (Loki).
pub fn apply(store: &mut impl DocStore, ops: Vec<DbOp>);
```

How each vanilla storage rule is reproduced:

| rule (01 §10, 11 STORE-BULK-WRITER) | mechanism |
|---|---|
| `update(obj, data)` deep-merges into the live object, immediately visible (BULK-UPDATE, VIS-INMEM) | `update` runs `live::merge` on the target `O`; every handler shares the room heap |
| persisted value of an object-valued key: if the live key held a truthy object/array before the call, the payload aliases it and `execute` reads the **live sub-object at room end**; if the key was absent/falsy, lodash creates a fresh object, so it is a **value snapshot** of that update; if it held a truthy scalar the object value is lost (bulk.js:45-48 + baseMergeDeep) | payloads hold `V` references, `execute` serializes; identity follows from `Rc`, no special cases. **Refines 01 BULK-UPDATE**, whose "as of the last update call" wording is wrong for the alias case |
| scalar keys persist the value of the last `update` | payload holds the value |
| `_` keys and `$loki` stripped at update/insert time, recursively, including inside referenced live sub-objects; never again at execute (BULK-HIDDEN, 13 CORE-OLDHITS) | `remove_hidden` exactly where bulk.js calls it; `_` keys added later to an aliased sub-object persist (as vanilla) |
| `undefined` is a no-op, `null` clears; arrays merge index-wise (BULK-UNDEFINED, BULK-ARRAY-MERGE) | `merge` port; `to_val` drops `Undef` |
| unrecorded in-memory changes are lost next tick | live heap discarded; persisted docs change only through `apply` |
| `update(idString)` leaves memory untouched (VIS-USERS-STALE) | `Target::Id` |
| execute order: remove/inc/addToSet/pull in call order, then `$set` per id in first-touch order, then inserts in counter order (BULK-EXECUTE) | `execute` emits that order |
| update of a pending insert folds into it; remove of a pending insert cancels it | as bulk.js; counter never reused |
| NaN persists as null (01 Open question 6) | `to_val` |
| crash discards all buffered writes (COMPAT-CRASH-ROOM) | `process_room` returns before `execute`; fire-and-forget notifications kept |
| `roomInfo` saved only if `!_.isEqual` with the shallow clone taken at load (PROC-SAVE) | `live::is_equal` |
| each vanilla query result is a separate object graph (`getRoomObjects().users`, each `getInterRoom` result) | each becomes its own `from_val` graph |

### 3.4 World, rooms, terrain (`crates/engine/src/world.rs`)

```rust
pub type RoomIdx = u32;          // room creation order = `rooms` insertion order (ORDER-GAME-ROOMS)
pub type UserIdx = u32;          // `users` insertion order; NPC users "2"/"3" are ordinary entries

pub struct World {
    pub time: u64,                                   // env gameTime
    pub seed: u64,                                   // worldSeed (RNG-SEED)
    pub clock: VClock,
    pub compat: Arc<policy::Compat>,
    pub config: Arc<EngineConfig>,                   // CFG-ENGINE-DEFAULTS (cpuMaxPerTick, cpuBucketSize, …)
    pub meta: Arc<RoomsMeta>,
    pub terrain: Arc<TerrainStore>,
    pub access: Arc<AccessSnapshot>,
    pub rooms: Vec<Arc<RoomSnapshot>>,               // by RoomIdx
    pub users: Vec<Arc<UserState>>,                  // by UserIdx
    pub user_ids: Arc<HashMap<Id, UserIdx>>,
    pub usernames: Arc<HashMap<Box<str>, UserIdx>>,  // usernameLower
    pub market: Arc<MarketState>,
    pub env: Arc<EnvState>,
    pub active: ActiveSet,
    pub manual_intents: Vec<(RoomIdx, UserIdx, Box<str> /*object id*/, Obj)>,  // INT-MANUAL, for the next P2
    pub global_intents: Vec<GlobalIntentsDoc>,       // users.intents; consumed by P5
    pub index: WorldIndex,
    pub ids: ids::IdGen,
    pub next_seq: u64,
    pub mods: Arc<[Box<str>]>,                       // resolved mod order (persisted)
}

#[derive(Clone, Copy)]
pub struct VClock { pub base_ms: i64, pub base_tick: u64, pub nominal_tick_ms: u32 }
impl VClock { pub fn at(&self, t: u64) -> i64; pub fn rebase(&mut self, t: u64, nominal_tick_ms: u32); }

/// Map topology; replaced as a whole on map edits (world gen, CLI generateRoom/removeRoom).
pub struct RoomsMeta {
    pub x0: i32, pub y0: i32, pub w: u32, pub h: u32,   // bounding box, engine coords (room_name::RoomXY)
    pub grid: Vec<u32>,                                  // w*h cells: RoomIdx + 1, 0 = no room
    pub rooms: Vec<RoomMeta>,                            // by RoomIdx; removed rooms keep a tombstone slot
    pub world_size: u32,                                 // common::calc_world_size incl. BUG-WORLDSIZE
}
pub struct RoomMeta { pub name: Box<str>, pub xy: RoomXY, pub removed: bool }
impl RoomsMeta {
    pub fn idx(&self, xy: RoomXY) -> Option<RoomIdx>;
    pub fn by_name(&self, name: &str) -> Option<RoomIdx>;   // RoomXY::parse_engine + idx
}

pub struct TerrainStore {
    packed: Vec<RoomTerrain>,                           // 625 B per room (engine::terrain)
    raw: Vec<std::sync::OnceLock<Box<[u8; 2500]>>>,     // row-major codes, built on first use
    exits: Vec<std::sync::OnceLock<[u8; 4]>>,           // mapgrid.js t/r/b/l plain-tile counts
}
impl TerrainStore {
    pub fn get(&self, r: RoomIdx) -> &RoomTerrain;
    pub fn raw(&self, r: RoomIdx) -> &[u8; 2500];       // pathfinder terrain callback; source of isolate copies
    pub fn exit_counts(&self, r: RoomIdx) -> [u8; 4];
    pub fn heap_mb(&self) -> f64;                       // vanilla staticTerrainDataSize / 2^20 (heap limit formula)
}

/// TICK-ACCESSIBLE output (21-tick cadence, rebuilt only when dirty).
pub struct AccessSnapshot {
    pub version: u64,
    pub accessible: Vec<u64>,                           // bitset by RoomIdx at vclock(T+1)
    pub status: HashMap<RoomIdx, (RoomStatus, Option<i64>)>,   // non-normal rooms: novice/respawn/closed + timestamp
}
pub enum RoomStatus { Normal, Closed, Novice, Respawn }

/// One room at the end of some tick. Immutable; replaced, never mutated.
pub struct RoomSnapshot {
    pub info: Arc<Obj>,                                 // `rooms` doc (= processor roomInfo)
    pub objects: Arc<[DocRef]>,                         // rooms.objects of this room, ascending seq
    pub flags: Arc<[Arc<Obj>]>,                         // rooms.flags docs {_id,user,room,data}, creation order
    pub event_log: Arc<str>,                            // last processed tick's eventLog JSON (stale while inactive)
    pub map_view: Arc<str>,
    pub processed_at: u64,
    pub presence: Arc<[(UserIdx, Presence)]>,           // derived
    pub user_refs: Arc<[UserIdx]>,                      // users named by docs here (user, reservation.user, sign.user)
    pub has_movers: bool, pub has_terminal_send: bool, pub has_power_creeps: bool,
    blob: std::sync::OnceLock<RoomBlob>,                // §6.6, made for the first viewer
}
impl RoomSnapshot { pub fn blob(&self) -> &RoomBlob; pub fn viewed(&self) -> bool; }
/// The rooms doc as ASCII JSON and the docs in order. `text()` is ASCII
/// `{"room":<rooms doc>,"seq":[s0,…],"objects":[d0,d1,…]}`, put together on first use and shared by every copy.
pub struct RoomBlob { pub info: Arc<str>, pub docs: Arc<[DocRef]>, /* text */ }
impl RoomBlob { pub fn text(&self) -> &Arc<[u8]>; }

/// One user's footprint in one room (VIS-ROOMS, ORDER-RUNTIME-ROOMIDS).
#[derive(Clone, Default)]
pub struct Presence {
    pub objects: u32,                                   // docs with user == uid (incl. sites, tombstones)
    pub vision: u32,                                    // of those, not flag/constructionSite
    pub first_vision_seq: u64,
    pub observe: Option<(u64 /*observer seq*/, Box<str> /*observeRoom as stored, may be "null"*/)>,
}

pub struct ActiveSet { bits: Vec<u64>, list: Vec<RoomIdx> }
impl ActiveSet { pub fn insert(&mut self, r: RoomIdx); pub fn contains(&self, r: RoomIdx) -> bool; pub fn take(&mut self) -> Vec<RoomIdx>; }

/// A world mutation from the backend, CLI or server isolate; run on the tick thread in P0 (also while paused).
pub type CommandFn = Box<dyn FnOnce(&mut World) -> Result<util::json::Json, String> + Send>;
pub struct Command { pub apply: CommandFn, pub reply: Option<std::sync::mpsc::SyncSender<Result<util::json::Json, String>>> }

/// CFG-ENGINE-DEFAULTS as typed fields: cpu_max_per_tick (500), cpu_bucket_size (10000),
/// main_loop_reset_interval_ms (5000), history_chunk_size (20), socket_update_throttle_ms (200), …
pub struct EngineConfig { /* one pub field per vanilla config.engine key chitin reads */ }
```

Dense grid: a 256×256 world costs 256 KB; empty cells of sparse worlds cost 4 B each.

### 3.5 Users and per-user state

```rust
pub struct UserState {
    pub id: Id, pub seq: u64,
    pub doc: Arc<Obj>,                       // `users` doc (11 COL-USERS): cpu, cpuAvailable, gcl, power, money,
                                             // resources, rooms, active, badge, activeSegments, lastUsedCpu, …
    pub auth: Arc<Obj>,                      // password hash/salt, steam id: never sent to clients
    pub code: Arc<CodeState>,
    pub memory: JsStr, pub memory_version: u64,          // env memory:<uid>; version bumps on every write
    pub segments: Arc<BTreeMap<u8, JsStr>>,             // memorySegments:<uid>
    pub public_segments: Arc<str>,                      // publicMemorySegments:<uid> ("1,2,3")
    pub inter_shard: Option<JsStr>,                     // mmo profile only
    pub console_queue: Arc<[Arc<Obj>]>,                 // users.console, ORDER-CONSOLE-COMMANDS (newest-first)
    pub power_creeps: Arc<[DocRef]>,                    // users.power_creeps, ORDER-POWER-CREEPS
    pub inbox: Arc<Inbox>,                              // users.notifications + users.messages
}
pub struct CodeState { pub branches: Vec<Arc<Obj>> /*users.code docs*/, pub active_world: Option<usize>,
                       pub active_sim: Option<usize>, pub version: u64 /*++ on any change of the active world branch*/ }
pub struct Inbox { pub notifications: Vec<Arc<Obj>>, pub messages: Vec<Arc<Obj>> }
```

| state | home | written by |
|---|---|---|
| cpu, cpuAvailable, lastUsedCpu, lastUsedDirtyTime, segment settings | `doc` | P2 (CPU-BUCKET-UPDATE) |
| gcl, power, rooms[] | `doc` | P4 from room `user_ops` (RoomIdx order; $inc/$addToSet/$pull commute) |
| money, resources | `doc` | P5 market, crons |
| code branches, version | `code` | P0 (API), journaled |
| Memory, segments, InterShardMemory | `memory`, `segments`, `inter_shard` | P2 (only when changed) |
| console commands | `console_queue` | P0 (API `/console`, `/memory` POST as hidden expression), consumed by the run |
| console output, cpu stats, visuals | `TickReport` (not world state) | P2 |
| flags | room snapshots (`rooms.flags`) | room phase; backend flag API via P0 |
| notifications, messages | `inbox` | P2 (notify), P4 (processor), API |
| power creeps (unspawned) | `power_creeps` | P5, API, crons |

`util::jsstr` (FROZEN):
```rust
/// A JS string kept exactly: Latin-1 when every UTF-16 unit < 0x100, else UTF-16 (lone surrogates survive).
#[derive(Clone, PartialEq, Eq)]
pub enum JsStr { Latin1(Arc<[u8]>), Utf16(Arc<[u16]>) }
impl JsStr {
    pub fn from_str(s: &str) -> JsStr;
    pub fn utf16_len(&self) -> usize;                 // 2 MiB / 100 KiB limits count UTF-16 units
    pub fn write_json_quoted(&self, out: &mut String);// JSON.stringify of the string (lone surrogates as \udXXX)
    pub fn to_string_lossy(&self) -> String;
}
```

### 3.6 Market, env, time

```rust
pub struct MarketState {
    pub orders: Arc<[DocRef]>,                // market.orders, creation order; prices ×1000
    pub stats: Arc<[Arc<Obj>]>,               // market.stats (calcMarketStats)
    pub transactions: Arc<[DocRef]>,          // capped by purgeTransactions semantics
    pub money: Arc<[Arc<Obj>]>, pub resources: Arc<[Arc<Obj>]>,   // users.money / users.resources history
    orders_json: Vec<std::sync::OnceLock<Arc<str>>>,              // per resource ordinal + "all", built lazily
    history_json: Vec<std::sync::OnceLock<Arc<str>>>,
}
pub struct EnvState { pub paused: bool, pub tick_rate_ms: u32, pub last_accessible_update: u64, pub status_dirty: bool }
pub struct GlobalIntentsDoc { pub user: UserIdx, pub from_api: bool, pub arrival: u32, pub intents: Obj }
```
A new `Arc<MarketState>` is installed whenever orders change (P5, crons, API), so the lazy JSON is per version.
`world.time` is the only tick counter; visuals live in the backend (CLK-VISUAL-TTL), not in `World`.

### 3.7 Ids and sequence numbers (`crates/engine/src/ids.rs`)

```rust
pub struct IdGen { counter: u64, len: u8 /*15 | 24*/, key: u64 /*from worldSeed*/, imported: Option<HashSet<Id>> }
impl IdGen {
    pub fn new(world_seed: u64, profile: policy::Profile) -> IdGen;
    pub fn next(&mut self) -> Id;             // lowercase hex of a 4-round Feistel permutation of counter (60/96 bits),
                                              // zero-padded; skips ids in `imported`; never integer-like
    pub fn mark_imported(&mut self, id: Id);
}
```
Called only from serial code: P4 id pass, P5, CRON-STAGE, P0 commands, import. `seq = world.next_seq++` at the same
moment. Rooms and users have their own creation order (RoomIdx, UserIdx + `seq`).

### 3.8 Derived indexes and published views

```rust
/// Tick thread only; never persisted; rebuilt on load; updated in P4/P5/crons/P0 from changed docs only.
pub struct WorldIndex {
    pub presence: Vec<Arc<BTreeMap<RoomIdx, Presence>>>,   // by UserIdx (Arc: shared with WorldView, COW)
    pub objects: HashMap<Id, ObjLoc>,                       // every rooms.objects doc (intent routing, API lookups)
    pub movers: BTreeSet<RoomIdx>,                          // rooms holding a creep/PC with interRoom
    pub terminal_sends: BTreeSet<RoomIdx>,
    pub power_creep_rooms: BTreeSet<RoomIdx>,               // spawned power creeps
    pub force_update: std::collections::BinaryHeap<std::cmp::Reverse<(u64, RoomIdx)>>,
    pub room_cost_ns: Vec<u32>,                             // last P3 cost, LPT order
}
pub struct ObjLoc { pub room: RoomIdx, pub seq: u64, pub owner: Option<UserIdx> }

/// The only thing backend, persistence and history read. Consistent (one tick), immutable, shares data.
#[derive(Clone)]
pub struct WorldView {
    pub time: u64, pub vnow: i64,
    pub meta: Arc<RoomsMeta>, pub terrain: Arc<TerrainStore>, pub access: Arc<AccessSnapshot>,
    pub rooms: Arc<[Arc<RoomSnapshot>]>,
    pub users: Arc<[Arc<UserState>]>, pub user_ids: Arc<HashMap<Id, UserIdx>>, pub usernames: Arc<HashMap<Box<str>, UserIdx>>,
    pub presence: Arc<[Arc<BTreeMap<RoomIdx, Presence>>]>,
    pub market: Arc<MarketState>, pub env: Arc<EnvState>,
    pub persist: Arc<PersistMeta>,            // seed, clock, next_seq, id counter, active list, mods
}
impl World { pub fn view(&self) -> WorldView; }
```
Every index has a property test against a brute-force scan of the same `World`.

### 3.9 Memory footprint *(est.)*

| item | per unit | 256×256 world, 1000 players |
|---|---|---|
| room skeleton (meta, `rooms` doc, ~5 NPC docs, packed terrain, snapshot) | ≈ 3.3 KB | ≈ 210 MB |
| unpacked terrain (rooms ever touched by Rust) | 2.5 KB | ≤ 160 MB |
| owned/remote room objects (≈ 400 docs × 450 B) | ≈ 180 KB | 3 k rooms ⇒ 540 MB |
| cached doc JSON + room blobs | ≈ 1.6 × docs | ≈ 1 GB |
| `WorldIndex.objects` | ≈ 70 B/doc | ≈ 90 MB |
| isolate baseline (snapshot: lodash + API) | ≈ 3–5 MB | ≈ 4 GB |
| bot heaps | 5–300 MB | ≈ 15–25 GB (governed, D14) |
| Memory + segments | ≤ 2 MB + ≤ 10 MB | ≈ 0.7 GB |
| **total** | | **≈ 22–32 GB ⇒ 64 GB target; the 32 GB dev box holds ≈ 300–500 real bots** |

---

## 4. Intents (FROZEN)

### 4.1 Inside the player isolate

`engine/game/game.js` keeps vanilla's `intents` object exactly (02 §12.1, 01 INT-LIST): null-prototype `list`,
`set` last-wins, `push`/`pushByName` with caps (INT-LIMITS), 0.2 CPU per first set, `say`/`pull` free, `remove`
refund. `driver/runtime.js` builds vanilla's `outMessage` (null prototype: `type`, `error`, `usedTime`,
`usedCleanTime`, `usedDirtyTime`, `intentsList`, `intentsCpu`, `memory`, `console{log,results}`, `visual`,
`memorySegments`, `activeSegments`, `activeForeignSegment`, `defaultPublicSegment`, `publicSegments`).
`JSON.stringify(Memory)` and segment validation run inside the meter as in vanilla (02 CPU-MEASURE, SEG-SAVE).

### 4.2 Out of the isolate (runtime host, on the worker)

1. Read `outMessage.memory` directly as `JsStr` (cloning a string cannot fail, so skipping it is equivalent).
2. `ValueSerializer::write_value(outMessage)` in the player context. `DataCloneError` (function, symbol, …) ⇒
   RUN-TOTAL-ERROR exactly as `ExternalCopy` (tick discarded per 02 §13.2).
3. `perform_microtask_checkpoint`; a pending unhandled rejection ⇒ total failure (ENV-MICROTASKS). Unlock.
4. `runtime/src/store.rs` reads the ValueSerializer bytes itself when they are JSON-shaped (undefined, null,
   booleans, numbers, strings, plain objects, dense arrays, each object once, no array-index / Object.prototype key
   names, no throw) and runs the same steps in Rust (`engine::system` sanitizers), giving the Json step 5 describes
   without a second isolate; every bot measured so far takes this path for every run. Anything else (Dates, Maps,
   BigInts, holes, shared or cyclic references, vanilla's prototype pollution, every throw) goes to the reference:
   lock the worker's system isolate; `ValueDeserializer::read_value`; call `__store(out, userId)`
   (`js/host/system.js`), which runs the ports of `utils.storeIntents` (object → room through the native
   `objectRoom(id)`, see §4.3), `system.js` sanitizers with `customIntentTypes`, and `intentsCount` over the raw list.
   `store.rs` tests compare both paths on generated outMessages.
5. `__store` returns one ASCII JSON string (FROZEN):

```json
{"type":"done","error":null,"usedTime":3,"usedCleanTime":2.41,"usedDirtyTime":4,
 "intents":{"W1N1":{"5f3a…":{"move":{"id":"undefined","direction":3}},"room":{"createFlag":[…]}},
            "global":{"createOrder":[…]},"notify":[{"message":"…","groupInterval":0}]},
 "count":7,
 "console":{"log":["…"],"results":["…"]},"visual":{"W1N1":"…","":"…","map":"…"},
 "activeSegments":[1,2],"activeForeignSegment":null,"defaultPublicSegment":null,"publicSegments":"1,2"}
```
   `intents` is the vanilla `storeIntents` result with first-appearance key order. Segments (`memorySegments`) and
   InterShardMemory are read from the deserialized object as `JsStr` (lossless), not through JSON.
6. Rust parses with `util::json` (key order kept) into `UserIntents` on the worker.

### 4.3 Rust types and routing (`crates/engine/src/intents.rs`)

```rust
pub type IntentName = Atom;
pub struct UserIntents {
    pub rooms: Vec<(RoomIdx, Vec<(Box<str> /*object id | "room"*/, Obj /*name → sanitized value*/)>)>,
    pub global: Option<Obj>,                  // name → Val::Arr
    pub notify: Vec<Val>,
    pub count: u32,                           // CPU-BUCKET-UPDATE count over the raw list
}
pub fn parse_store_output(json: &util::json::Json, meta: &RoomsMeta) -> UserIntents;   // unknown rooms dropped (F)

pub struct RoomIntents { pub users: Vec<UserRoomIntents> }          // arrival order; processor applies ORD-POLICY
pub struct UserRoomIntents { pub user: UserIdx, pub objects: Vec<(Box<str>, Obj)>, pub manual: Vec<(Box<str>, Obj)> }
```

* `objectRoom(id)` (system-isolate native, INT-STORE): `world.index.objects[id]` is accepted iff
  `owner == user` (vanilla `userObjects`) or `room ∈ visible rooms of this run` (vanilla `roomObjects`), returning
  the room name at tick start; otherwise `undefined` and the intents of that id are dropped.
* P2 (`runner::make::save_run`): per `(room, objects)` push `UserRoomIntents` into `bucket[room]` and
  `active.insert(room)`; `global` → `GlobalIntentsDoc{from_api:false}`; `notify` → `_.take(20)`, groupInterval
  clamp, 500-char cut, real-clock bucket (INT-NOTIFY, CLK-NOTIFY). Then `world.manual_intents` for that room/user are
  merged in as `manual` (INT-MANUAL `_.extend` per object at processing).
* The processor sees per object one `Obj` (intent name → sanitized value, every declared field present, as
  `system.js` emits it) and dispatches by name in the fixed per-type order (06 creepActions etc.), per user in
  ORD-POLICY order and per object in first-set order, so 13 ARCH-ONE-SLOT-INTENT-MAPS and ARCH-TYPE-GROUPED-INTENTS
  cannot recur.

### 4.4 API-originated intents

`POST /api/game/add-object-intent` and `add-global-intent` become P0 Commands that sanitize with `engine::system`
(Rust port of `common/lib/system.js` for JSON inputs, fixture-identical to the JS port) and append to
`world.manual_intents` (activating the room) or `world.global_intents{from_api:true}` in arrival order (ORD-POINTS 2).

### 4.5 Limits

Only vanilla's per-type caps (INT-LIMITS, in the JS API) plus the driver's `notify` `_.take(20)`; no global cap.
Non-vanilla safety caps, set far above real bots and reported as RUN-TOTAL-ERROR with text `intents too large`:
serialized `outMessage` ≤ 32 MiB, console output ≤ 1 MiB per user per tick.

---

## 5. Processor

### 5.1 Entry, context, results (`crates/engine/src/processor.rs`, FROZEN signatures)

```rust
pub struct TickCtx<'w> {
    pub time: u64, pub vnow: i64, pub seed: u64,
    pub compat: &'w policy::Compat, pub config: &'w EngineConfig,
    pub meta: &'w RoomsMeta, pub terrain: &'w TerrainStore, pub access: &'w AccessSnapshot,
    pub users: &'w [Arc<UserState>], pub user_ids: &'w HashMap<Id, UserIdx>,   // as of P3 start
    pub hooks: &'w hooks::ProcessorHooks,
    pub order_override: Option<&'w [UserIdx]>,
}
pub fn process_room(ctx: &TickCtx, room: RoomIdx, prev: &Arc<RoomSnapshot>, intents: RoomIntents) -> RoomResult;

pub struct RoomResult {
    pub room: RoomIdx,
    pub next: Option<RoomDraft>,              // None ⇒ keep prev (unchanged, or crashed)
    pub inserts: Vec<Obj>,                    // bulk inserts in counter order; seq/_id in P4
    pub crashed: Option<live::Crash>,
    pub activate_next: bool,                  // ACT-RULE 1–7 (false on crash)
    pub user_ops: Vec<bulk::DbOp>,            // bulkUsers.execute()
    pub foreign_inserts: Vec<(RoomIdx, Obj)>, // nuke launch (BULK-COLLECTION-WIDE)
    pub notifications: Vec<(UserIdx, Box<str>)>,   // fire-and-forget, kept on crash
    pub history: Option<Box<str>>,            // objectsToHistory JSON when ACT-RULE fired and the history mod is on
    pub stats: Vec<(UserIdx, Atom, f64)>,     // stats.inc → mods only (POST-STATS)
    pub cost_ns: u32,
    pub index: Option<(Arc<RoomSnapshot>, Vec<IndexOp>)>, // process_room: §3.8 object-index changes vs the snapshot it read
}
pub struct RoomDraft {
    pub info: Arc<Obj>, pub objects: Vec<DocRef> /*without inserts*/, pub flags: Arc<[Arc<Obj>]>,
    pub event_log: Arc<str>, pub map_view: Arc<str>,
    pub presence: Vec<(UserIdx, Presence)>,   // over existing docs; P4 patches first_vision_seq for inserts
    pub user_refs: Vec<UserIdx>, pub has_movers: bool, pub has_terminal_send: bool, pub has_power_creeps: bool,
}

/// processor.js `scope` (fields = its keys, snake_cased) + movement.js module state + RNG + outputs.
pub struct RoomScope<'a> {
    pub ctx: &'a TickCtx<'a>, pub room: RoomIdx, pub room_name: &'a str,
    pub room_objects: live::RoomObjects, pub room_terrain: &'a RoomTerrain, pub game_time: u64,
    pub room_info: live::O, pub room_controller: Option<live::O>,
    pub users: Vec<(UserIdx, live::O)>,       // per-room clones of referenced user docs
    pub flags: Vec<live::O>,
    pub bulk: Bulk, pub bulk_users: Bulk, pub bulk_flags: Bulk,
    pub event_log: Vec<live::V>, pub map_view: MapView,   // {key: [tile, …]} until `save` writes the JSON
    pub energy_available: f64,
    pub room_spawns: Vec<live::O>, pub room_extensions: Vec<live::O>,
    pub keepers: Vec<live::O>, pub invaders: Vec<live::O>, pub invader_core: Option<live::O>,
    pub objects_to_history: Vec<live::O>,
    pub movement: intents::movement::Movement,
    pub rng: policy::LazyRng,                 // room stream, seeded on first draw
    pub out: RoomResult,
}
```

Handler convention (every mirror file): `module.exports = function(object, intent, scope)` becomes
`pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash>`; tick files export
`pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash>`; helper modules (`_die`, `_damage`,
`_create_energy`, …) export the vanilla function names with the vanilla argument lists. Statements are ported in
order with JS value semantics through `live` (NaN propagation, `||` defaults, `parseInt` truncation). Every
reachable vanilla `undefined.x` returns `Err(Crash)` at the same point; COMPAT rows are decided only through
`ctx.compat.fix("BUG-…")` at the throw point. Each file starts with `//! mirror: reference/engine/src/processor/<path>`
and `//! parity: MISSING | FAITHFUL | DIVERGENT(<ID>)`.

### 5.2 `process_room` step order (exactly `processor.js`, 01 §9)

1. **Load** (PROC-INPUT): snapshot docs → `RoomObjects` (seq order, `from_doc`), `roomInfo`, owners' user docs
   (each its own graph), flags, terrain, intents.
2. **Prescan** (PROC-PRESCAN): `_actionLog`/`_ticksToLive`, per-type actionLog reset, collect
   keepers/invaders/core/controller/spawns/extensions/nukes, observers `observeRoom = null`, ACT-RULE on the
   pre-intent set, `hooks.process_object`.
3. PROC-NUKE-PRETICK. 4. PROC-NPC-PRETICK (keepers, invaders, core; `_.assign(npc, stored)` merge rule).
5. PROC-CALC-SPAWNS (BUG-CRASH-CALC-SPAWNS via compat).
6. **Intents** (PROC-INTENTS): users by ORD-POLICY; per user `objects` `_.extend`ed with `manual`; per key in
   first-set order: `room` → `room/intents`, else `hooks.pre_object_intents`, skip missing/`_skip`/foreign-owned
   (INT-OWNERSHIP), dispatch by type (INT-DISPATCH, incl. the inline cases of processor.js:250-311: observer,
   rampart setPublic, terminal send, nuker, factory, controller), INT-NOTIFY-ATTACKED, `hooks.post_object_intents`.
7. `movement::check` (PROC-MOVE-CHECK). 8. PROC-ENERGY-AVAILABLE.
9. **Tick loop** over a key snapshot (PROC-TICK-LOOP): type tick, collapse timer, powerBank/deposit decay, history
   capture, mapView entry, `hooks.post_object`.
10. **Save** (PROC-SAVE): `hooks.room_end`; `bulk/bulk_users/bulk_flags.execute()`; `apply` objects and flags ops
    to a COW `RoomDocs` store over prev; eventLog/mapView JSON; ACT-RULE 7; roomInfo if `!isEqual`.
    `bulkUsersPowerCreeps` is never executed (vanilla line 23).
11. If no op was produced and eventLog, mapView and roomInfo are byte-identical: `next = None` (prev stays
    pointer-equal, its blob stays cached). A `Crash` at any step returns `next = None`, keeps notifications,
    `activate_next = false` (COMPAT-CRASH-ROOM); the bucket is consumed (INT-CLEAR).

### 5.3 Module layout (one file per vanilla file; `-`/camelCase → snake_case; leading `_` kept)

```
crates/engine/src/processor.rs                         processor.js
crates/engine/src/utils.rs                             engine/src/utils.js (processor half: checkConstructionSite,
                                                       checkControllerAvailability, checkStructureAgainstController,
                                                       calcResources, calcBodyEffectiveness, dist, calcRoomsDistance,
                                                       calcTerminalEnergyCost, calcNeededGcl, calcTotalReactionsTime,
                                                       capacityForResource, calcReward, getReactionVariants,
                                                       getDirection, getOffsetsByDirection, calcCreepCost, isAtEdge,
                                                       comparatorDistance, sendAttackingNotification)
crates/engine/src/processor/global.rs                  processor/global.js
crates/engine/src/processor/common/fake_runtime.rs     processor/common/fake-runtime.js
crates/engine/src/processor/global_intents/{market,power}.rs
crates/engine/src/processor/global_intents/power/{_die_power_creep,create_power_creep,delete_power_creep,
    rename_power_creep,spawn_power_creep,suicide_power_creep,upgrade_power_creep}.rs
crates/engine/src/processor/intents/{_calc_spawns,_create_energy,_damage,movement}.rs
  constructed_walls/tick  construction_sites/tick  containers/tick  controllers/{activate_safe_mode,tick,unclaim}
  creeps/{_add_fatigue,_clear_newbie_walls,_die,_drop_resources_without_space,_recalc_body,attack,attack_controller,
          build,claim_controller,dismantle,drop,generate_safe_mode,harvest,heal,intents,move,pickup,pull,
          ranged_attack,ranged_heal,ranged_mass_attack,repair,reserve_controller,say,sign_controller,suicide,tick,
          transfer,upgrade_controller,withdraw}
  creeps/invaders/{find_attack,flee,healer,pretick,shoot_at_will}  creeps/keepers/pretick
  {deposits,energy,extensions,extractors,keeper_lairs,minerals,portals,roads,ruins,sources,storages,tombstones}/tick
  factories/{produce,tick}  flags/intents
  invader_core/{attack_controller,create_creep,destroy,intents,pretick,reserve_controller,tick,transfer,upgrade_controller}
  invader_core/stronghold/{creeps,defence,stronghold}
  labs/{boost_creep,intents,reverse_reaction,run_reaction,tick,unboost_creep}  links/{intents,tick,transfer}
  nukers/launch_nuke  nukes/{pretick,tick}
  power_creeps/{drop,enable_room,intents,move,pickup,renew,say,tick,transfer,use_power,withdraw}
  power_spawns/{intents,process_power}  ramparts/{set_public,tick}
  room/{create_construction_site,create_flag,destroy_structure,gen_energy,intents,remove_construction_site,remove_flag}
  spawns/{_born_creep,_charge_energy,cancel_spawning,create_creep,intents,recycle_creep,renew_creep,set_spawn_directions,tick}
  structures/_destroy  terminal/{send,tick}  towers/{attack,heal,intents,repair,tick}
crates/engine/src/strongholds.rs                       common/lib/strongholds.js (templates)
```
`intents.rs` files keep vanilla's fixed orders and blocking tables as `const` arrays (06 creepActions).

### 5.4 Movement (`intents/movement.rs`)

Statement-by-statement port of `movement.js` with its module globals as fields of `Movement` (per room, inside
`RoomScope`): `matrix` keyed `y*50+x`, registered moves, pull links, refs to `room_objects`/terrain. `add` from
creep/power-creep move, `check` once after intents, `execute` per creep in its tick. rate1..rate4 ranks, stable
sort in registration order, weight from live parts counted from the end of the body; never xxscreeps' random ties
(12 XX-COMPAT-MOVE). Exit tiles set `interRoom` and push EVENT_EXIT; P5 completes the move.

### 5.5 P4 install and cross-room effects (`processor::install`)

In RoomIdx order over `RoomResult`s:
1. `inserts` (counter order) get `seq = next_seq++` and `_id = ids.next()` unless given; appended to
   `draft.objects` (larger seq ⇒ order kept); build the `Arc<RoomSnapshot>` (patch presence for inserts).
2. `user_ops` applied to the users collection with Loki semantics (COW `Arc<UserState>`).
3. Index/presence updates from removed/inserted ids and `user` changes; `room_cost_ns`. The object-index changes (a
   seq-ordered merge of old and new docs that skips unchanged doc pointers) are worked out by `process_room`, so in P3
   and in parallel; P4 applies them in order (a removal only while the entry still names this room and seq) and merges
   only the inserts itself, or the whole room when its old snapshot is not the one P3 read.
4. `activate_next` → `world.active` (processed T+1); notifications → inbox (real clock); history → writer.
Then a second pass applies `foreign_inserts` to their target rooms (new seq/_id, target **not** activated).

| effect | room phase | applied |
|---|---|---|
| edge exit / portal | creep tick sets `interRoom`; `has_movers` | P5 step 1: doc moved with the same seq to its sorted position, `room/x/y` set, `interRoom: null`, destination activated T+1; missing/inaccessible destination ⇒ skipped, `interRoom` kept (XROOM-INACCESSIBLE) |
| terminal `send` | stored on the terminal; `has_terminal_send` | P5 market step |
| launchNuke | nuker drained; nuke doc in `foreign_inserts` | P4 second pass; target sees it at T+1 when processed |
| observer | `observeRoom` persisted | presence: visible to the owner at T+1 |
| GCL / power / rooms[] | `user_ops` | P4 |

### 5.6 Global stage (`processor/global.rs` + `global_intents/**`, mirror of `global.js`)

```rust
pub struct GlobalCtx<'h> { pub time: u64, pub vnow: i64, pub hooks: &'h hooks::ProcessorHooks,
                           pub order_override: Option<&'h [UserIdx]> }       // seed/compat/config come from World
pub fn run(w: &mut World, ctx: &GlobalCtx);
```
Inputs are built as the five vanilla `getInterRoom` results, each
its own live graph, from indexes: movers with `interRoom` (seq order); `accessibleRooms` at `vclock(T)`; the
terminals, power spawns and spawned power creeps it dereferences; orders; user power creeps; `users.intents` sorted
by ORD-POLICY; referenced users plus every order terminal's owner (BUG-CRASH-MKT-USER, F). Order (GLOB-ORDER):
inter-room moves → `power.rs` (six intent types, then the death sweep, GLOB-POWER-DEATH) → `market.rs` (sends,
per-doc market intents, stable distance sort, `_.shuffle(directDeals)` on the global stream, order maintenance)
→ execute the nine bulk writers in vanilla's declared order (`bulkMarketOrders` before
`bulkMarketIntershardOrders`) against COW room snapshots/users/market → clear `users.intents` →
`hooks.global_stage`. Inserts (tombstones, spawned PCs with explicit `_id`) get seq/_id in call order.

### 5.7 NPC AI

Keepers, invaders and the invader core run in PROC-NPC-PRETICK and return `{objectId: intents}` as vanilla;
`common/fake_runtime.rs` ports `fake-runtime.js` (RoomPosition helpers, `findPath` with its path cache,
`memory_move` stored on the creep doc) over the thread-local `pathfinder::PathFinder` and `TerrainStore::raw`.
Strongholds (`invader_core/stronghold/*`) run in the room phase. Randomness only at RNG-SITES (room stream). Raid
and stronghold spawning are crons (§5.9). No invented AI (13 REC-NO-INVENTION).

### 5.8 Event log, map view, history, roomInfo

Event log: `Vec<V>` serialized once per processed room, ASCII-escaped (so it can be a one-byte external string), into
`RoomSnapshot.event_log`; runners get the string for
`Room.getEventLog(raw)`; unprocessed rooms keep the old one (ACT-STALENESS). Map view: PROC-TICK-LOOP item 5 →
`RoomSnapshot.map_view` → `roomMap2:` channel and `map-stats`. History (built-in mod, off by default): creeps
deep-copied without `_actionLog/_ticksToLive`, non-public `say` dropped, stringified at room end, sent to
`server/src/history.rs` (mirror of `driver/lib/history.js`, chunk/base rules, opt-in retention).

### 5.9 Crons (CRON-STAGE; `engine/src/cron/{cronjobs,strongholds}.rs`)

Order: roomsForceUpdate, genPowerBanks, genInvaders, purgeTransactions, recreateNpcOrders, calcMarketStats,
deletePowerCreeps, genDeposits, genStrongholds, expandStrongholds, then mod crons in registration order. Each runs
when `vclock(G) − last[job] > period × 1000` (`last` in memory, 0 at start). Per-unit closures (room, sector
centre, terminal room) run on the pool with their own cron stream and return write lists applied in unit order;
expandStrongholds is sequential in core creation order. Writes go through the same `apply`/insert/id-pass helpers
and activate rooms for G. BUG-DEPOSIT-REGEX / BUG-SH-HIGHWAY-REGEX are C rows. `sendNotifications` runs on the
backend's real 60 s timer (CLK-NOTIFY).

---

## 6. Runtime (V8)

### 6.1 What v8 152.2.0 provides (checked in the crate source)

* `unsafe OwnedIsolate::try_into_shared()` → `SharedIsolate`; `lock()` → `Locker`. Rules: no `Weak` handles, no
  cppgc, the isolate must be current, and every locking thread must be created after `V8::initialize`.
  `Global`s are allowed (dropped lazily under the next lock). Migration costs a ThreadManager archive/restore
  (~9 % on trivial requests per `locker.rs`; ≪ 1 % for ms-scale runs *(est.)*).
* `thread_safe_handle()` → `IsolateHandle::{terminate_execution, cancel_terminate_execution, request_interrupt}`.
* `Isolate::snapshot_creator(Option<external_refs>, Option<CreateParams>)`, `create_blob(FunctionCodeHandling::Keep)`,
  `CreateParams::snapshot_blob`, `heap_limits(initial, max)`.
* `unsafe String::new_external_onebyte_raw(scope, ptr, len, unsafe extern "C" fn(*mut char, usize))`;
  `new_external_twobyte`. ValueSerializer/ValueDeserializer. `add_near_heap_limit_callback`,
  `get_heap_statistics`, `low_memory_notification`, `set_microtasks_policy`, `perform_microtask_checkpoint`,
  `set_promise_reject_callback`, ArrayBuffer backing stores from Rust memory (no sandbox in the default build).
* Not provided: a per-isolate CPU clock or platform clock override (⇒ OS FFI meter, JS-level `Date`), IsolateGroup.

### 6.2 Runtime API (`crates/runtime/src/lib.rs` + `crates/engine/src/runner/mod.rs`, FROZEN)

```rust
// engine/src/runner/mod.rs — no V8
pub struct RunJob { pub user: UserIdx, pub expected_ms: f32, pub limit: f64 /*Game.cpu.limit*/,
                   pub tick_limit: f64 /*CPU-LIMIT: min(bucket + cpu, cpuMaxPerTick); Infinity for cpu 0*/, pub bucket: f64 }
pub struct RunInput {
    pub user: UserIdx,
    pub meta: String,                          // per-user JSON, §6.6
    pub blobs: Vec<Option<RoomBlob>>,          // parallel to meta.rooms (None for names that are not rooms)
    pub event_logs: Vec<Arc<str>>,             // parallel to the vision rooms
    pub visible: Vec<RoomIdx>,                 // sorted; INT-STORE routing
    pub memory: JsStr, pub memory_version: u64,
    pub segments: Vec<(u8, JsStr)>, pub foreign_segment: Option<(Box<str>, u8, JsStr)>,
    pub code: Arc<CodeBundle>,                 // shipped only when the VM's code version differs
    pub vnow: i64, pub nominal_tick_ms: u32,
}
pub struct CodeBundle { pub version: u64, pub modules: Vec<(Box<str> /*unescaped name*/, Module)> }
pub enum Module { Text(Arc<str>), Binary(Arc<[u8]>) }
pub enum RunStatus { Done, PartialError /*saved, billed*/, TotalError /*discarded, unbilled*/, Skipped, Deactivated,
                     Reset(ResetReason) /*halt, heap, hard timeout, governor: discarded*/ }
pub struct RunOutput {
    pub user: UserIdx, pub status: RunStatus, pub error: Option<String>,
    pub intents: Option<intents::UserIntents>,       // None ⇒ discarded
    pub memory: Option<JsStr>,                       // Some only if saved and changed
    pub segments: Vec<(u8, JsStr)>, pub seg_settings: Option<Val>, pub inter_shard: Option<JsStr>,
    pub console_log: Vec<String>, pub console_results: Vec<String>,
    pub visuals: Vec<(Box<str> /*room | "" | "map"*/, Arc<str>)>,
    pub used_time: u32, pub used_clean_ms: f64, pub used_dirty_ms: u32,
}
pub fn build_input(w: &World, job: &RunJob) -> RunInput;   // runner/data.rs

// runtime/src/lib.rs
pub fn init_v8();                                   // first call in main: TZ=UTC, platform(4), V8::initialize
pub enum Meter { Thread, Wall, Fixed { ms_per_tick: f64 } }
pub struct RuntimeConfig {
    pub runner_threads: usize, pub heap_mb: usize /*256*/, pub heap_budget_mb: usize, pub meter: Meter,
    pub hard_timeout_ms: u64, pub idle_evict_s: u64 /*180*/, pub watchdog_poll_us: u64,
    pub profile: policy::Profile, pub shard_name: Box<str>,
}
pub struct Bundle { pub files: &'static [(&'static str, &'static str)], pub mod_game_js: Vec<(Box<str>, Box<str>)> }
pub struct Runtime { /* vms, snapshot blobs, system isolates per worker, watchdog, scheduler */ }
impl Runtime {
    pub fn new(cfg: RuntimeConfig, bundle: Bundle) -> Runtime;
    pub fn run_phase(&self, pool: &util::pool::Pool, w: &World, jobs: &[RunJob]) -> Vec<RunOutput>;
    pub fn after_phase(&self, w: &World);           // governor + idle eviction
    pub fn reset_user(&self, user: UserIdx, reason: &str);
    pub fn server(&self) -> &ServerIsolate;         // CLI + JS-mod server slot (§10)
    pub fn stats(&self) -> RuntimeStats;            // heap totals, resets, migrations, parse/build ms
}

// runtime/src/server_isolate.rs — the privileged isolate (CLI, JS-mod server slot), one Mutex-guarded SharedIsolate
pub struct ServerIsolate { /* … */ }
impl ServerIsolate {
    pub fn start(&self, view: Arc<dyn Fn() -> WorldView + Send + Sync>, commands: std::sync::mpsc::Sender<Command>,
                 server_js: &[(Box<str> /*mod*/, Box<str> /*source*/)]);
    pub fn open_session(&self) -> u64;                       // one context per CLI connection (CLI-SANDBOX)
    pub fn eval(&self, session: u64, line: &str) -> String;  // CLI-OUTPUT formatted
    pub fn close_session(&self, session: u64);
    pub fn routes(&self) -> Vec<(Box<str> /*method*/, Box<str> /*path*/, u32 /*handler*/)>;
    pub fn call_route(&self, handler: u32, request: util::json::Json) -> util::json::Json;   // 50 ms deadline
    pub fn run_crons(&self, w: &mut World, g: u64);          // CRON-STAGE, scoped world API, cron streams
    pub fn on_tick(&self, view: &WorldView);                 // chitin.on('tick'|'roomsDone')
}
```

### 6.3 Isolate lifecycle (`runtime/src/host.rs`, mirror of `user-vm.js`)

* **Create** on first run or after a reset: `Isolate::new(CreateParams::default().snapshot_blob(blob)
  .heap_limits(0, limit))`, `set_microtasks_policy(Explicit)`, near-heap-limit callback, promise-reject callback,
  `_init(natives)` (captures natives in closures, deletes bootstrap globals: VM-BOOT), JS-mod `playerSandbox`
  registrations, then `try_into_shared()`. ≈ 1–3 ms, ≈ 3–5 MB *(est.)*.
* **Global-reset triggers** (02 VM-RESET-SUMMARY): `code.version` changed (monotonic counter, fixes
  VM-CODE-VERSION-RESET as the spec recommends); `Game.cpu.halt()`; heap exhaustion; wall hard timeout; idle
  ≥ `idle_evict_s` real seconds (VM-IDLE-EVICT); governor; CLI `system.resetUserRuntime`. Not: thread migration,
  hourly restart (TOPO-RESTART), multi-runner splits (VM-MULTI-RUNNER). Classes are still created on each VM's first
  `make` (GLB-CLASSES-ONCE), so prototype patches persist exactly as vanilla.

### 6.4 Threads, scheduling, migration (`runtime/src/sched.rs`)

`run_phase` runs `runner.threads` worker loops on the pool. M1: one shared LPT cursor. M5: each user has a home
worker (last thread); each worker drains its home list (largest first) then steals the largest job from the most
loaded list; a stolen user's home moves. A job never runs twice in a tick, so locks are uncontended. Each worker
slot (`pool::worker_index()`) owns: a watchdog slot, a system isolate (`SharedIsolate`, locked only by that worker
after the player isolate is unlocked), a `PathFinder` stack.

### 6.5 Snapshots and bundles

* **Player snapshot**: one script named `<runtime>` = bundler prelude + every `js/**` game/driver/common file
  wrapped as `function(module, exports, require){<source>}` (no added directive, each file keeps its strictness:
  ENV-ENGINE-STRICTNESS) + `common/constants` from `engine::constants::constants_json()` (byte-identical to
  official) + JS-mod game slots in load order. Only top-level evaluation is snapshotted (ENV-SNAPSHOT).
* **System snapshot**: lodash, `common/system.js`, `engine/utils.js`, `host/system.js`.
* Built at boot and held in memory, one blob per process for each `sha1(all JS + v8 version)`. There is no cache on
  disk: a snapshot is code, V8 checks only its version, and the data folder can come from anyone.

### 6.6 Per-tick data delivery (decision: shared per-room JSON in external strings)

**Per-user meta** (`runner/data.rs` builds, `js/host/prelude.js` consumes; FROZEN shape):

```json
{"time":1234,"user":{…users doc minus auth…},"cpu":500,"cpuBucket":9000,"codeVersion":7,  // cpu/cpuBucket as data.js (CPU-LIMIT)
 "rooms":[["W1N1",1,0],["W2N1",1,5],["null",1,-1],["W5N5",0,9]],   // [name, vision, RoomIdx|-1]: vision rooms in
                                                                    // ORDER-RUNTIME-ROOMIDS order, then load-only
                                                                    // rooms (own sites/flags only)
 "flags":[…rooms.flags docs…],"powerCreeps":{…},"users":{"<id>":{"_id":"…","username":"…","badge":{}}},
 "transactions":{"outgoing":[…100],"incoming":[…100]},"consoleCommands":[…],"myOrders":[…],
 "segments":{"activeSegments":[…],"foreign":null,"publicSegments":"…"},"shard":"shard0"}
```
`roomIds` = presence rooms with `vision > 0` and observer targets, ordered by `min(first_vision_seq, observer seq)`
(ORDER-RUNTIME-ROOMIDS, incl. the `"null"`/`"undefined"` quirk names). `users` = data.js `userIdsHash` (self,
`user_refs` of vision rooms, transaction counterparties), projected to `{_id, username, badge}` (never full docs).

**Room blobs**: `RoomSnapshot::blob()` (§3.4), made once per snapshot for its first viewer: the rooms doc's JSON and
the docs, whose JSON P3 writes as it makes them when the room had a viewer (`RoomSnapshot::viewed`). `__tick` gets
`true` for a room and asks `natives.blob(i)` for its text when it parses the room whole; the text is put together on
the first such call and shared by every VM after it. Every blob text, event log, Memory and segment enters V8 through `new_external_onebyte_raw(ptr, len, drop_fn)` with
`ptr = Arc::into_raw(arc.clone())` and `drop_fn` = `Arc::from_raw` + drop (zero copies; each isolate holds one strong
count until its GC frees the string); non-Latin-1 `JsStr` uses `new_external_twobyte`. A global atomic counts live
blob bytes (alloc/drop) for the governor.

**JS assembly** (`__tick`, the JS half of `data.js` + `runtime.js _start`): parse each blob with the captured
`JSON.parse`; `roomObjects` = foreign docs (`user !== me`) of vision rooms in roomIds order (blob = seq order), then
own docs of all blobs k-way-merged by `seq` (ORDER-RUNTIME-OBJECTS); `rooms` = vision rooms' `room` docs in RoomIdx
order (ORDER-GAME-ROOMS); `roomEventLog` from the event-log strings; then the mirrored `game.js` runs unchanged:
fresh game objects every tick, lazy getters by id, truthy-only memo, find caches (GAME-IDENTITY, GAME-CONSTRUCT).
Old cached game objects keep working because getters read the current `runtimeData`. Raw docs unchanged since the
VM's last run are taken over instead of parsed (doc reuse, below).

| option | per-isolate | 1000 players, 600 k object-views | fidelity risk | decision |
|---|---|---|---|---|
| A. shared per-room JSON, eager | `JSON.parse` ~0.5 µs + construction ~2 µs per object | ≈ 1.5 s CPU ≈ 95 ms wall on 16 | none; raw docs identical | **M1–M4** |
| A′. same blobs, lazy per-object parse of `json[offsets[i]..]` on first `data(id)`; construction from a compact index | ~0.6 µs | ≈ 0.4–0.6 s | internal to prelude/game.js; gated by every API oracle case | **M5 lever (WP20)** |
| B. binary + overlay views | ~20 ns/field | ≈ 0.5 s | high (every accessor rewritten, stale objects) | rejected |
| C. V8 objects built from Rust | ~60 ns/property | ≈ 1.8 s | medium, nothing shared | rejected |

Cost outside the meter, as vanilla (02 CPU-MEASURE).

A′ measured against three bots (region e10s30-e30s50, raw docs read through `runtimeData.roomObjects` after Game setup,
per tick): bot A 96%, DefaultO 87%, bot B 7–11%. Bots that scan their structures every tick read nearly every
doc, so A′ would parse them anyway, one at a time; it pays only for light bots.

**Doc reuse (J11)**: on the same region 98% of a user's docs are JSON-identical to its previous tick (97.5% the same
`Arc`). Each VM keeps the docs it assembled last run (`heldRooms` in prelude.js); the host keeps the matching
`DocRef`s per room (`runtime/src/doc_reuse.rs` `DocCache`, reset with the VM) and sends one ASCII JSON string `[generation, [blob index, rooms doc, ops, changed docs…], …]` for the rooms the VM also had:
ops `k, n` take held docs k..k+n (with their seqs), `-1 - seq` takes the next changed doc. Docs match on seq plus
`Arc::ptr_eq` or equal JSON (a held `Arc` is never mutated in place). A generation mismatch (the VM did not finish
taking the last input) makes the JS side parse every blob's text (`natives.blob`). Vanilla parses every doc each tick, so the player must
never reach a taken-over doc:
- never taken over: docs whose getters return a piece of the doc (`creep.body`, `spawning.directions` of spawns and
  invader cores, a tombstone's power creep `powers`: kinds creep, powerCreep, spawn, invaderCore, tombstone), and docs
  with `effects`, which engine constructors pass to `RoomObject.call`, a property the player can shadow on that
  function. Every other getter returns primitives or new objects; no engine code writes into a doc.
- reuse stops for good (`natives.docsOff`) when a mod defines object types, or when at the start of a tick a builtin
  that engine code hands raw docs or their parts to is not as `_init` left it: `Object.prototype`, `Array.prototype`,
  `Function.prototype`, the array iterator prototypes and lodash's wrapper prototype exactly (prototype and own
  properties, so no accessor added for a key a doc lacks), and the existing properties of `Object`, `Array`, `JSON`,
  `Reflect`, lodash and `lodash.prototype` plus the global bindings of `Object`, `Array`, `JSON`, `Reflect`,
  `Symbol`, `Function` (new properties allowed: `_.mixin` keeps reuse). A property counts as changed by any part of its
  descriptor, its enumerable bit included. Checked with captured functions (~30 µs).
  One bot of the three patches a global prototype and runs without reuse; the other two keep it.
- left: a patch installed and removed within one tick can capture raw docs (say a `_.sum` wrapper seeing a container's
  store), and writes to them then show in later ticks. Oracle cases `doc-reuse-identity`, `doc-reuse-patched-leaks`,
  `doc-reuse-shadowed-call` fail when the exclusions, the check or the `effects` rule are removed.

Measured (interleaved, 5 rounds, medians): DefaultO alone, input parse 1.45 → 0.88 ms per run; region, 20.3 → 19.7
ms/tick, DefaultO 17.2 → 16.8 ms/run, bot A 14.5 → 13.6, bot B 6.1 → 6.1. A self-check run (each planned room
also parsed whole and compared, 250 region ticks, ~370 k docs taken over) found no difference, so neither engine nor
bot code wrote into a held doc.

### 6.7 Natives (JS ↔ Rust surface, FROZEN; FunctionTemplates so `toString` prints `[native code]`)

```
_init(natives) — captured in closures by js/host/prelude.js, then `delete global._init`:
  cpuTime() → ms (f64), the configured meter                                  [vanilla _isolate.cpuTime]
  compileAndRun(code, filename, timeoutMs, kind)  kind: 'module'|'loop'|'console'|'sandbox'
      Script::compile with ScriptOrigin(filename); runs in the player context under a pushed deadline
      (timeoutMs per MOD-TIMEOUT; none for 'console'/'sandbox' beyond the hard timeout). On its own deadline:
      cancel_terminate_execution at this boundary and throw Error('Script execution timed out.') carrying the
      interrupt-captured stack (OBS-STACK-TIMEOUT); outer deadlines propagate untouched.
  halt()                       → marks the VM for disposal, terminate_execution            [_halt]
  docsOff()                    → the host stops sending doc reuse plans to this VM (§6.6 J11)
  blob(i)                      → the text of the run's room blob i | undefined (§6.6; put together on first use)
  heapStats()                  → isolated-vm's 10 fields in order, `adjustment` rule       [ENV-HEAP-STATS]
  nativeMod = {version: 11, loadTerrain(){}, search(origin, goals, cb, plainCost, swampCost, maxRooms, maxOps,
               maxCost, flee, heuristicWeight)}   — exact pf.cc argument list; runs crates/pathfinder on this
               thread's PathFinder; roomCallback called back into JS; CostMatrix `_bits` read live as &[Cell<u8>]
               from the Uint8Array backing store (RoomCost::Matrix)                     [_nativeMod]
  terrain(roomName)            → fresh Uint8Array(2500) copy | undefined (JS caches per VM)
  worldSize()                  → meta.world_size                                         [_worldSize]
  isAccessible(roomName)       → bool (AccessSnapshot)
  roomStatus(roomName)         → {status, timestamp} | undefined (non-normal only)
  gridData(x, y)               → {t?,r?,b?,l?} | undefined (mapgrid.js gridData entry; accessible rooms only)
  marketOrders(resourceType)   → JSON string `{id: order}` ('all' allowed) | undefined
  marketOrder(id)              → JSON string | undefined
  marketHistory(resourceType)  → JSON string
  now()                        → vclock(T) + min(realMsSinceRunStart, nominalTickMs − 1)   [CLK-SANDBOX-DATE]
  base64ToBuffer(b64)          → Uint8Array with the internal Buffer prototype            [ENV-BINARY-BUFFER]
  shardName, profile           → strings
  setupList                    → boolean, runner.setupList (§2.9 "What differs in a match")
host → JS:  __tick(meta, blobs /*true (a room: natives.blob(i)) | string | null*/, eventLogs, memory|undefined,
            segments /*{id: string}*/, foreignSegment /*{username,id,data}|undefined*/,
            modules /*{name: string|Uint8Array}|undefined*/, plans /*doc reuse, §6.6*/|undefined) → outMessage
system isolate:  objectRoom(id) → roomName | undefined;  __store(out, userId) → JSON string (§4.2)
```
`js/engine/game/map.js` and `js/driver/mapgrid.js` keep vanilla's algorithms (findRoute A* over a 60×60 window,
`isRoomAvailable`, `getRoomStatus`) but read through `isAccessible`/`roomStatus`/`gridData`/`terrain` instead of a
parsed `accessibleRooms` array and a per-VM world grid. `market.js` keeps `cachedOrders[rt] =
JSON.parse(natives.marketOrders(rt) || '{}')` — semantically identical to vanilla's `JSON.parse(JSON.stringify(…))`.

### 6.8 Memory, RawMemory, segments, InterShardMemory

* Memory: `game.js`/`runtime.js` getter verbatim (MEM-*): `JSON.parse` through the current global, null-prototype
  root, redefinition as a data property, memhack via `RawMemory._parsed`. The VM keeps the last Memory string as a
  `Global<String>` with its `memory_version`; `__tick` receives `undefined` when the versions match. End of tick:
  stringify inside the meter; equal to the input ⇒ `outMessage.memory = undefined` (no copy, no persist). Over
  2 MiB (UTF-16 units) ⇒ RUN-MEMORY-LIMIT. Backend memory POST = hidden console expression (as vanilla).
* Segments: ≤ 10 active, loaded next tick, `≤ 100*1024` units, every key present in `RawMemory.segments` re-saved,
  validation after the user try (total failure on violation) (SEG-*). Foreign/public per SEG-* and ID-FOREIGN-SEG0 by
  profile.
* InterShardMemory: `mmo` profile only (ENV-GLOBALS-ABSENT under `private`): `getLocal()` → `''` when unset,
  `setLocal` ≤ 100 KiB persisted per user, `getRemote(shard)` → `null` (single shard).

### 6.9 Console and errors

`console.js` mirrored (null-prototype, `log`/`logUnsafe` only, CON-LOG-FORMAT). Console commands run after the loop,
newest-first, via `compileAndRun(kind:'console')` with the `_console<vms>_<i>` name (virtual ms). Error formatting
mirrors `runtime-driver.js` (CON-ERR-FORMAT, wrapper `(function __module(module,exports){ ` ⇒ +36 columns on line 1,
bare module names). Output → `TickReport` → `user:<id>/console`. Users `2`/`3` log to stdout only.

### 6.10 CPU accounting and termination

* Meter (`runtime/src/cpu.rs` over `util::sys`): Windows `QueryThreadCycleTime` × ns/cycle calibrated against QPC
  at boot (`GetThreadTimes` is 15.6 ms coarse); Linux `CLOCK_THREAD_CPUTIME_ID`, watchdog via
  `pthread_getcpuclockid`. `Fixed`: counter +0.01 ms per `cpuTime()` call; billing = `ms_per_tick` + 0.2/intent.
* `startTime = cpuTime()` after Game construction; `getUsed() = cpuTime() − startTime + intents.cpu`.
* Watchdog (`runtime/src/watchdog.rs`): per worker slot a stack of `(cpu_deadline_ns, wall_deadline)` + thread clock
  + `IsolateHandle`; it sleeps with std's high-resolution waitable timer (Windows 10+; `timeBeginPeriod(1)` FFI
  fallback); termination latency ≤ poll + V8 interrupt latency (≤ 2 ms *(target)*). After a timeout the
  runtime continues as vanilla: Memory and earlier intents saved, tick billed (OUT-FAILURE-MATRIX). Hard timeout:
  terminate, reset VM, output dropped, ungraceful text.
* Billing in P2 (`runner/make.rs`): CPU-BUCKET-UPDATE verbatim (`ceil(usedClean + 0.2·count)`, `$set` semantics,
  no lower bound), RUN-SKIP and CPU-NEGATIVE-SKIP decided before scheduling.

### 6.11 Heap limits and governor (`runtime/src/governor.rs`)

`limit = cfg.heap_mb + terrain.heap_mb()` (vanilla formula, visible via `heap_size_limit`). Near-limit callback:
`terminate_execution`, return current + 32 MB for unwinding, mark for disposal ⇒ `Reset(Heap)` with
`Isolate has exhausted v8 heap space` (VM-HEAP-LIMIT); nothing saved, unbilled. After P1: if
`Σ used_heap + live blob bytes > heap_budget_mb` (default 75 % of RAM minus world memory): `low_memory_notification`
on isolates not run this tick, largest first; still over ⇒ reset the largest (`Reset(Governor)`, logged).

### 6.12 Modules and code

`require` mirrors `game.js:551-594` (flat namespace, `./` strip, `require.cache` seeded with lodash, `-1` cycle
marker, delete on throw, `Unknown module`, legacy no-loop mode, lodash `_.isFunction` loop check ⇒ async loops never
run). Code lives in `UserState.code` (`users.code` branches, `$DOT$`/`$SLASH$`/`$BACKSLASH$` escaping, timestamps in
vclock ms); `CodeBundle` is shipped only when `code.version` differs from the VM's (then the VM resets). Binary
modules decoded once per version into a `Uint8Array` with the Buffer prototype; `WebAssembly` is V8's. Code is in
checkpoints and the account journal.

### 6.13 Output extraction cost *(est.)*

ValueSerializer write + read ~10–30 µs for 10 KB, `__store` ~20 µs, Rust parse/route ~10 µs: ~50 µs per player, all
on workers.

---

## 7. Game API JS package (`crates/runtime/js/`)

### 7.1 Layout (one file per vanilla file)

```
vendor/lodash.js                 reference/lodash-3.10.1/lodash.js verbatim (MIT) — the `_` global and engine `_`
common/system.js                 common/lib/system.js            (sloppy)
engine/utils.js                  engine/src/utils.js             (sloppy; runtime half + storeIntents)
engine/game/{console,constants,construction-sites,creeps,custom-prototypes,deposits,flags,game,map,market,minerals,
             names,nukes,path-finder,path-utils,power-creeps,resources,rooms,ruins,sources,store,structures,
             tombstones}.js  engine/src/game/*.js (game.js, path-finder.js, path-utils.js strict; Room.Terrain fn strict)
driver/{runtime,runtime-driver,system-functions,mapgrid}.js   driver/lib/runtime/*.js (sloppy)
driver/path-finder.js            driver/lib/path-finder.js       (strict)
host/prelude.js                  not a mirror: module table, _init(natives), __tick (JS half of data.js + _start/run)
host/system.js                   not a mirror: system isolate __store (make.js:192 storeIntents + intentsCount)
cli/{server,sandbox,help,system,map,bots,strongholds,storage,inspect}.js   backend/lib/cli/*.js + util.inspect subset
```
`data.js`, `make.js`, `user-vm.js` are host-side: `engine/src/runner/{data,make}.rs`, `runtime/src/host.rs`.
Embedding: `runtime/src/bundle.rs` = `static FILES: &[(&str, &str)]` of `include_str!`s (list owned by WP00).

### 7.2 Port rules

* Our own transcription, statement by statement, keeping the original call graph (engine methods call patchable
  public paths, e.g. `destroy()` → `this.room.find`). Header `// mirror: <path>` + `// parity: …`; anchors
  `// vanilla: rooms.js:NNN` per function.
* Deliberate internal deviations (none changes results for well-formed calls), listed in the parity report:

| # | deviation | reason |
|---|---|---|
| J1 | `staticTerrainData[room]` reads go through `natives.terrain` (per-VM cache of copies) | D34 |
| J2 | `getRawBuffer(dest)` uses the captured `TypedArray.prototype.set` | a hostile `set` would see the internal view |
| J3 | spatial registers and the world map grid are lazy; map/market data via natives. Spatial arrays start empty (vanilla `new Array(2500)`); the per-index arrays are made and pushed to during Game setup as in vanilla, but kept in a Map until `lookAt`/`lookForAt` first reads that spatial array, then stored (presized), so arrays nobody reads (every `ownedStructures`/`spawns` one) stay empty; only a user accessor on `Array.prototype` indices sees the stores at that read instead | GAME-CONSTRUCT allows; D9; dictionary elements are written through the V8 runtime |
| J4 | runtime-internal stack frames carry chitin line/col | no vendoring; bots map only their own frames |
| J5 | runtime data parsed with captured builtins; Memory still uses the global `JSON.parse` (GLB-SYSFUNC) | vanilla data arrives via ExternalCopy |
| J6 | lone surrogates in document strings (names, signs, flags, code) become U+FFFD | Rust strings; Memory/segments/ISM lossless (D35) |
| J7 | (M5) lazy raw-doc materialization | gated by every API oracle case passing unchanged |
| J8 | `world.wideCoords` worlds: `__packedPos` uses the wide packing | opt-in, D33 |
| J9 | the engine reads its own classes as plain globals (`new RoomPosition(…)`, `RoomObject.call`) where vanilla reads `globals.X`; `if(globals.X)` in `make()` is `if(hasX())`; game.js reads the global `require` through `natives.globalRequire()` | speed, same values: see below |
| J10 | `new RoomPosition(x, y, roomName)` keeps `utils.roomNameToXY(roomName)` per name (up to 4096) while `String.prototype.substr`/`charAt` and the global `parseInt` are the originals; the offsets go to locals, so the shared `[x, y]` is never written | no substrings or array per position. Patches see every call (`patched-roomname-parse`); only an accessor installed on those three, returning the original, sees fewer reads |
| J11 | raw docs unchanged since the VM's last run are taken over instead of parsed again (§6.6 doc reuse) | the player never reaches a taken-over doc: see §6.6 for what is excluded, when reuse stops, and the one case left |

J9: vanilla redefines `Memory` on the global every tick (accessor, then data on first access). V8 treats both as a
reconfiguration of a prototype (the global object is the global proxy's prototype) and invalidates the proxy's
prototype-chain validity cell, so every `globals.X` load site missed its inline cache once per tick (~110 runtime IC
misses per VM per tick in `chitin bench`, M1 bot). A plain global read uses the property cell directly and is not
affected. The classes are defined with `Object.defineProperty(globals, X, {enumerable: true, value})`: non-writable,
non-configurable, and a lexical `let X` at global level throws for such a name, so the read yields the same object.
Every name involved is either undeclared in the module (resolves to the global) or the class's own local. Inside
`make()` the class name is that local (hoisted, undefined before its line), so the "already made" check is a
module-level `hasX()`: `typeof X !== 'undefined' && !!X`, i.e. `!!globals.X` (a data property, so reading it twice is
unobservable). `require` is writable, may be an accessor, and every bundled module has a `require` parameter: the
prelude's `globalRequire()` is `G.require`, vanilla's single [[Get]] on the global, from one load site instead of a
dozen; the one call, `globals.require('main')`, keeps its receiver.
Other writable globals (`Memory`, `Game`, `_`, `console`) and engine/utils.js (also loaded in the system isolate,
`globals` is a parameter) still read through `globals`.

### 7.3 Differential oracle (`tools/oracle/api/`, node 22 at `E:\screeps\node22\node-v22.22.3-win-x64\node.exe`)

* **Official side**: `E:\screeps\server\node_modules\@screeps\driver` runtime bundle (or `reference/engine/src/game`
  + `reference/driver/lib/runtime`) in a node `vm` context; a ~150-line isolated-vm shim provides `_ivm`
  (`ExternalCopy` → `structuredClone`), `_isolate.cpuTime` (deterministic counter), `compileScriptSync` →
  `vm.Script({filename})`, heap stats, and `_nativeMod` = the official `native.node`.
* **Our side**: our bundle in another context with node implementations of §6.7 natives (official addon for
  `search`, fixture data for terrain/status/grid/market).
* **Case** = runtimeData ticks (hand-written, generated, or recorded by `tools/oracle/record`) + bot modules (probe
  bots calling every API with edge arguments, real bot bundles); `Math.random` seeded and `Date` fixed by a
  playerSandbox-style hook. Compared per tick: `intentsList` (deep, key order), Memory string, console, visuals,
  segments, thrown-error text with `<runtime>` frames stripped. CPU fields excluded.
* **Coverage gate**: ≥ 1 case per exported/prototype method of every file, and one per return-code/error row of
  specs 03/04/05. JS WPs develop and accept entirely in node; no V8 build needed.

### 7.4 Host conformance (`crates/runtime/tests/conformance.rs`)

The V8 host must produce the same `outMessage` as the node-hosted run for the same bundle and inputs. Dedicated
cases: timeout mid-loop (intents and Memory kept, `finally` not run), nested `require` timeout, heap kill, unhandled
rejection, Memory > 2 MiB, 11th segment, clone failure on a function in intents, `Date` virtualization, `halt`,
code-version reset, lone-surrogate Memory/segment round trip.

---

## 8. Backend (`crates/backend`, no V8)

### 8.1 Networking

* `http.rs`: HTTP/1.1 keep-alive, `Content-Length` and chunked bodies (10 MiB cap, 64 KiB until a request shows
  a valid token; code upload 5 MiB), JSON and urlencoded bodies, q-json envelope (`{ok:1,…}`; errors HTTP 200 `{error}`; only auth failure 401) (HTTP-ENVELOPE),
  CORS only for a `chrome-extension://` origin or the request's own host (HTTP-CORS is permissive in the original): any
  other Origin gets no CORS headers, and a POST or a socket from it gets 403; a server bound to loopback refuses any
  Host but localhost or a loopback address; server-password gate exempting `/api/version` and socket routes
  (HTTP-MIDDLEWARE-ORDER, 10 Q5).
* `ws.rs` RFC 6455 (accept key via `util::sha1` + `util::base64`), ping/pong, fragmentation, close.
  `sockjs.rs`: SockJS 0.3.24 `/socket/info`, `/socket/<server>/<session>/websocket` (`o`/`a[…]`/`h`/`c`, extended
  escaping, 25 s heartbeat) and raw `/socket/websocket`; no XHR fallbacks (SOCK-FALLBACK = NICE).
* WebSocket: reader thread (frames, `auth`/`subscribe`/`gzip` processed in order, fixing vanilla's subscribe race)
  + writer thread draining a bounded queue (4 MiB); overflow drops the connection's room baselines and re-sends a
  `getDiff([], …)` snapshot when drained (10 §20). 1000 clients ≈ 2000 mostly idle threads, ~256 MB reserved
  stacks; an IOCP/epoll rewrite only if the sockets bench shows > 5 % CPU in switches.

### 8.2 World access (`crates/backend/src/lib.rs`, FROZEN)

```rust
pub use engine::world::{Command, CommandFn};
/// http.rs defines Request/Response; handlers are plain fns (mods add more through the registry).
pub struct Route { pub method: &'static str, pub path: &'static str, pub handler: fn(&Request, &ApiCtx) -> Response }
pub struct ApiCtx { pub published: Arc<Published>, pub commands: std::sync::mpsc::Sender<Command>, pub user: Option<UserIdx> }
pub struct Published { /* Mutex<WorldView> + Condvar */ }
impl Published { pub fn get(&self) -> WorldView; pub fn set(&self, v: WorldView); pub fn wait_after(&self, t: u64) -> WorldView; }
pub struct TickReport {
    pub view: WorldView,
    pub processed: Arc<[RoomIdx]>,
    pub console: Vec<(UserIdx, Vec<String> /*log*/, Vec<String> /*results*/, Option<String> /*error*/)>,
    pub cpu: Vec<(UserIdx, u32 /*used*/, u32 /*memory bytes*/)>,
    pub visuals: Vec<(UserIdx, Box<str>, Arc<str>)>,
    pub user_events: Vec<UserEvent>,           // code/branch changes, resources/money, new messages, memory changed
}
pub trait CliEval: Send + Sync { fn open(&self) -> Box<dyn CliSession>; }      // implemented by server over the server isolate
pub trait CliSession: Send { fn eval(&mut self, line: &str) -> String; }
pub struct BackendDeps { pub published: Arc<Published>, pub commands: std::sync::mpsc::Sender<Command>,
                         pub cli: Arc<dyn CliEval>, pub config: BackendConfig, pub routes: Vec<Route> }
pub fn start(deps: BackendDeps) -> Backend;
impl Backend { pub fn on_tick(&self, report: TickReport); }    // never blocks; coalesces when the broadcaster lags
pub fn call<R: Send + 'static>(tx: &std::sync::mpsc::Sender<Command>, f: impl FnOnce(&mut World) -> Result<R, String> + Send + 'static) -> Result<R, String>;
```
Reads (`room-terrain`, `room-status`, `map-stats`, profiles, memory GET, …) use `published.get()` on the connection
thread. Writes run the mirrored handler body inside a Command at the next P0 (also while paused; 10 s timeout ⇒
`{error:'timeout'}`); rejection strings are vanilla's.

### 8.3 Endpoints (mirror `backend/lib/game/api/*.js`; tiers in 10 §§3–13)

| file | routes |
|---|---|
| `server.rs` (game/server.js) | `/api/version` (protocol 14, `serverData.features`, `useNativeAuth:false`, user count), `/api/servers/list` stub, router assembly |
| `api/auth.rs` | `auth/steam-ticket` (stub), `auth/me`, `auth/query-token` stub |
| `api/authmod.rs` (screepsmod-auth surface) | `auth/signin`, `user/password`, `register/submit`, Basic-auth middleware |
| `api/register.rs` | `register/check-email`, `check-username`, `set-username` |
| `api/user.rs` | `world-start-room`, `world-status`, `branches`, `code` GET/POST, `set-active-branch`, `clone-branch`, `delete-branch`, `memory` GET/POST (`gz:`), `memory-segment` GET/POST, `console`, `notify-prefs`, `tutorial-done`, `email`, `badge`, `respawn`, `respawn-prohibited-rooms`, `find`, `rooms`, `stats`, `overview`, `money-history`, `set-steam-visible`, `decorations/*` stubs |
| `api/game.rs` | `room-terrain`, `room-status`, `rooms`, `map-stats`, `room-overview`, `gen-unique-object-name`, `check-unique-object-name`, `place-spawn`, `create-flag`, `change-flag`, `change-flag-color`, `remove-flag`, `gen-unique-flag-name`, `check-unique-flag-name`, `add-object-intent`, `add-global-intent`, `create-construction`, `set-notify-when-attacked`, `create-invader`, `remove-invader`, `time`, `tick`, `world-size`, `room-decorations`/`find-object` stubs |
| `api/market.rs` | `orders-index`, `orders`, `my-orders`, `stats` |
| `api/power_creeps.rs` | `list`, `create`, `delete`, `cancel-delete`, `upgrade`, `rename`, `experimentation` |
| `api/leaderboard.rs`, `api/badge.rs`, `api/user_messages.rs` | as vanilla (`messages/{send,list,index,mark-read,unread-count}`, `user/badge-svg`) |
| `png.rs` | `/assets/map/*.png` from terrain (FMT-MAP-PNG, `util::deflate`), cached |

User objects sent to clients are always `{_id, username, badge}` (the Loki projection leak is not replicated).

### 8.4 Socket channels (`socket/{server,rooms,map,user,system}.rs`, `broadcast.rs`)

* `server.rs`: greeting `time <ms>` + `protocol 14`; `auth <token>` → `auth ok <new>` | `auth failed`;
  `gzip on|off` (`gz:` + base64(zlib) only when shorter, PROTO-SOCKET-GZIP); subscribe/unsubscribe.
* `rooms.rs` `room:<name>`: subscribe snapshot `{objects: getDiff([], docs), users, flags, info:{mode:'world'}}`
  without `gameTime`/visual (SOCK-ROOM-SUBSCRIBE); per tick `{objects: diff, flags, gameTime, visual(gameTime−1),
  users?}`. The broadcaster computes `common::get_diff` once per room per tick (pointer-equal docs skipped) and sends
  the same bytes to every subscriber whose baseline is the previous push; throttled subscribers get a cumulative
  diff (`socketUpdateThrottle` 200 ms). A broadcast takes the view published last, not its report's: a CLI write
  publishes one with no report, and a room subscribed since must not see a value go back. 2-room limit per user across connections, excess chosen with a shuffle,
  `err@room:X` (SOCK-ROOM-LIMIT).
* `map.rs` `roomMap2:<room>` = `map_view`. `user.rs`: `user:<id>/console|cpu|code|set-active-branch|newMessage|
  memory/<path>|resources|money` (cpu per user, not globally throttled; memory paths re-parsed only when Memory
  changed and the path is watched); authenticated subscriptions only. `system.rs`: `server-message`.

### 8.5 Auth (`authlib.rs`, `api/auth.rs`, `api/authmod.rs`)

* Token = `<uid>.<expiryHex>.<hex(HMAC-SHA1(secret, uid.expiry))>`, 300 s, secret from `util::sys::os_random` at
  world creation (persisted). Fresh `X-Token` on every authenticated response; `X-Username` must be non-empty
  (value ignored); `?_token=` accepted (superset) (AUTH-TOKEN-MW).
* Steam: `auth.steam = "single-user:<name>"` (default when bound to loopback: any ticket ⇒ that user, created on
  first use), `"trust"` (SteamID from the session-ticket header, layout verified against a captured ticket; logs
  a warning when not loopback), `"off"`. Native-mode tickets ⇒ `'authentication method is not supported'`.
* Passwords: PBKDF2-HMAC-SHA1, 100 k iterations, 16-byte salt, in `UserState.auth`; `/api/auth/me` reports
  `password: true`. Imported screepsmod-auth hashes are not verified; CLI `setPassword` resets.

### 8.6 Admin CLI

`backend/src/cli.rs`: TCP on `cli_host:cli_port` (127.0.0.1:21026), greeting and `< ` output rules (CLI-SERVER,
CLI-OUTPUT), one session per connection via `CliEval`. `runtime/js/cli/*.js` port `system`, `map`, `bots`,
`strongholds`, `storage` (`storage.db[coll].find/findOne/count/update/insert/removeWhere` with the Loki operator
subset incl. `$ne: null`, over WorldView reads + Commands; `storage.env.get/set` for modelled keys), `help`, and a
`util.inspect` subset (depth 2). Extras: `system.runTicks(n)`, `system.save()`, `system.resetUserRuntime(user)`.

### 8.7 Client and tool compatibility

| client / tool | flow | provided by |
|---|---|---|
| official Steam client | FLOW-CONNECT/FIRST-LOGIN/VIEW/DEPLOY | steam `single-user`, SockJS, §8.3–8.4 (M1) |
| steamless client | password signin, `serverData.features` | `authmod.rs` (M1) |
| screeps-api, rollup-plugin-screeps | `auth/signin` → token, `POST user/code {branch, modules}`, X-Token rotation | M1 |
| grunt-screeps | HTTP Basic + `POST user/code` | `authmod.rs` (M1) |
| node tools | raw `/socket/websocket` | `sockjs.rs` |

---

## 9. Persistence (`crates/engine/src/persist.rs`, `loki.rs`)

* **Model**: memory is authoritative. Checkpoints every `persist.interval_s` (60 s real), on `system.save()` and
  at clean shutdown (`util::sys::on_ctrl_c`). Capture = the WorldView published in P6 (pointer copy, no stall); the
  persist thread writes while the world keeps ticking (COW keeps old versions alive, ≈ 75 MB extra *(est.)*).
* **Account journal** `<data>/journal/accounts.log`: frames `u32 len | u32 crc32 | JSON record` for user
  creation, auth/badge changes, code uploads and branch ops; appended and fsync'd before the HTTP reply; replayed
  idempotently after the newest checkpoint (`journalSeq`).
* **Format** (`<data>/world.bin`): magic `CHITINW\x01`, then sections `tag u8 | len u64 | payload | crc32 u32`:
  `E` the env keys import reads (with `chitin`: seed, clock, idCounter, nextSeq, profile, mods), `K` one user's
  `memory:`/`memorySegments:`/`publicMemorySegments:`, `T` terrain (625 B packed per room), `C` one Loki collection
  as `(loki, doc)*`. Values are tagged binary (zigzag varint integers, raw f64, length-prefixed UTF-8), object keys
  numbered per section. Loading decodes into the Loki `Db` shape and builds through `loki::build`, so the store and
  `db.json` resume identically. Derived env keys (terrainData, accessibleRooms, roomStatusData) are not stored.
  Measured on 20×20 sectors (40 804 rooms, 135k objects, 300 bots): 0.24 s / 37.5 MB / 0.36 s load, against
  4.7 s + write / 154 MB / 0.9 s load for the db.json checkpoint it replaced (`crates/engine/tests/persist_bench.rs`).
  *(not built: per-sector segments rewritten on pointer change, a per-tick change log. Bot Memory churn makes a
  per-tick log larger than a snapshot every few seconds; terrain, 2/3 of the file, is the next thing to split out.)*
* **Atomicity / crash safety**: temp file → fsync → the old file renamed `world.bin.prev` → rename. Recovery loads
  `world.bin`, else (missing, or a checksum fails) `world.bin.prev`, else a legacy `<data>/db.json`, which is then
  migrated (store written, renamed `db.json.migrated`). Loss ≤ one interval of game state (same as Loki autosave).
  Restart = global reset for every VM (as vanilla).
* **Loki import** (`chitin import db.json`): `collections[].data` in file order defines RoomIdx (rooms), seq
  (rooms.objects) and UserIdx; ids kept (`IdGen::mark_imported`); env keys `gameTime`, `memory:*`,
  `memorySegments:*`, `publicMemorySegments:*`, `roomEventLog`, `mapView:*`, `activeRooms`, `tickRate`,
  `mainLoopPaused`; `$loki`/`meta` dropped; module names unescaped; legacy `rooms.active` → active set (v9 migration);
  `type:'terrain'` set. **Export** writes the inverse so the official server can load a chitin world.
  Acceptance: import `launcher/init_dist/db.json`, run 1 tick, equal `rooms.objects` to the official first tick.

---

## 10. Mods

### 10.1 Rust mods (compile time)

```rust
// crates/engine/src/hooks.rs — FROZEN (engine side; engine cannot see backend types)
#[derive(Default)]
pub struct ProcessorHooks {
    pub intent_types: Vec<(&'static str, &'static [(&'static str, system::FieldType)])>,   // customIntentTypes
    pub object_intents: Vec<(&'static str /*object type*/, &'static str /*intent*/, fn(&O, &V, &mut RoomScope) -> Result<(), Crash>)>,
    pub object_ticks: Vec<(&'static str /*custom type*/, fn(&O, &mut RoomScope) -> Result<(), Crash>)>,
    pub process_object: Vec<fn(&O, &mut RoomScope)>,                                     // processObject (prescan)
    pub pre_object_intents: Vec<fn(Option<&O>, UserIdx, &V, &mut RoomScope)>,
    pub post_object_intents: Vec<fn(&O, UserIdx, &V, &mut RoomScope)>,
    pub post_object: Vec<fn(&O, &mut RoomScope)>,                                        // postProcessObject
    pub room_end: Vec<fn(&mut RoomScope)>,                                               // processRoom
    pub activation_rules: Vec<fn(&O, &RoomScope) -> bool>,
    pub global_stage: Vec<fn(&mut World, &GlobalCtx)>,                                   // after market
}
#[derive(Default)]
pub struct EngineHooks {
    pub processor: ProcessorHooks,
    pub crons: Vec<(&'static str, u32 /*period s, vclock*/, fn(&mut World, &mut cron::CronCtx))>,
    pub gen_room: Vec<fn(&mut mapgen::GeneratedRoom, &mut util::rng::Rng)>,             // world-gen post-hook
    pub main_loop_stage: Vec<fn(Stage, &WorldView)>,
    pub next_tick_delay: Option<fn(used_ms: u64) -> u64>,
}

// crates/backend/src/mods.rs — FROZEN (everything a mod can register)
pub struct Manifest { pub name: &'static str, pub version: &'static str, pub dependencies: &'static [&'static str] }
pub trait Mod: Sync { fn manifest(&self) -> Manifest; fn register(&self, r: &mut Registry); }
#[derive(Default)]
pub struct Registry {
    pub engine: engine::hooks::EngineHooks,
    pub routes: Vec<Route>,                                  // HTTP routes
    pub server_data: Vec<(&'static str, util::json::Json)>,  // /api/version features, customObjectTypes, renderer
    pub socket_channels: Vec<(&'static str /*prefix*/, ChannelFn)>,
    pub js_game: Vec<&'static str>,                          // JS appended to the game slot
}
```
`server/src/mods.rs`: `static MODS: &[&dyn Mod] = &[&mods::history::MOD, …]`, each behind a cargo feature. With
no mods every call site is an empty-slice loop. Built-ins: `history` (room history, §5.8, `/room-history`),
`devtools` (`/api/dev/control`).

Mod crates: every crate in `mods/*/` (a git submodule, a workspace member through modlist's path dependency) exports `pub static MOD`. `crates/modlist` depends on
all of them and lists them as `modlist::MODS`; its build.rs keeps both in sync with the folder (a new or renamed
crate rewrites the generated block in its Cargo.toml and stops the build once, since Cargo resolves manifests
before build scripts). The loader looks names up in the built-ins and `modlist::MODS` alike; two mods with one
name are a startup error. `mods/example` is the template; `mods/README.md` is the author's guide.

Registry fields at runtime: `server_data` entries are `config.backend.<path> = value` assignments applied in load
order to vanilla's defaults (game/server.js:31-44, socket/server.js:17-25); `/api/version` reports getServerData()
of the result, and `socketUpdateThrottle` is the broadcaster's wait (lodash `_.throttle` coercion).
`socket_channels` are offered every subscribe after the built-ins (socket/server.js calls every module's
`onSubscribe`) and polled for each subscriber after every broadcast. `js_game` entries become the §10.2 game slots
`mods/<mod>.js`, `mods/<mod>.2.js`, …

### 10.2 JS mods

`<data>/mods/<name>/mod.json`: `{"name","version","dependencies":[],"game":"game.js"?,"server":"server.js"?,
"intentTypes":{…}?}`.
* **game slot**: evaluated in the snapshot after the game API, in load order; may call the vanilla
  `registerCustomObjectPrototype` equivalent (02 HOOK-CUSTOM-OBJECT), add globals, patch prototypes, and register a
  per-tick `playerSandbox(sandbox, userId)` run before `startTime`, unbilled (02 HOOK-PLAYER-SANDBOX).
* **server slot**: evaluated once in the server isolate with `chitin = { route(method, path, fn), cron(name,
  periodS, fn), cli(name, fn), on('tick'|'roomsDone'|'userRegistered', fn), world /* WorldView read helpers */,
  db /* Command-building mutations: insert/update/remove objects, set user fields, activate */ }`. Routes run on
  connection threads under the server-isolate mutex (50 ms deadline); crons run in CRON-STAGE with a scoped world
  API and `random()` from their cron stream.
* `intentTypes` feed both sanitizers (`customIntentTypes`); handlers for custom intents need a Rust mod (no JS in
  the room/global phase).

### 10.3 Load order

Rust and JS mods form one graph: DFS post-order, siblings sorted by name, cycles/missing deps are startup errors
(12 XX-MOD-RESOLVE). The resolved list is stored in `World.mods`; loading a world with a different list is refused
unless `--mods-changed`. A mod that changes vanilla rules marks the world `modified` in `/api/version`.

---

## 11. World generation (`crates/engine/src/mapgen.rs`)

* `generate_room(rng, xy, edges: [&[u8]; 4], opts) -> Result<GeneratedRoom, String>` ports `cli/map.js` generateRoom
  statement by statement (11 GEN-ROOM-ENTRY…GEN-MINERAL): exit matching, `_genExit`, wall/swamp tables, fill,
  double-buffered smoothing, 8-connected flood check, sources with the 1000-try restart, keeper lairs (LIFO depth
  search + `_.shuffle`), controller loop, emit order, terrain code 3, mineral type/exclusion/density, same rejection
  messages. Not bit-identical to the JS draw order (GEN-RNG-ORDER: unobservable); distributions and constraints are
  verified against `map.js`. GEN-POST: highway rooms get `bus:true` and `depositType` (SECTOR-DEPOSIT-TYPE),
  `type:'terrain'` always stored. Same file: `removeRoom`, `openRoom`/`closeRoom` (`openTime` in vclock),
  `updateTerrainData`.
* `pub struct WorldSpec { pub sectors_x: Range<i32>, pub sectors_y: Range<i32>, pub seed: u64, pub wide_coords: bool,
  pub open: bool, pub portals: Option<PortalSpec> }`;
  `pub fn generate_world(spec: &WorldSpec, pool: &Pool, hooks: &EngineHooks) -> World`.
  * Layout (11 §10): rooms in whole 10×10 sectors, row-major creation order (fixes RoomIdx); highway rooms where the
    displayed number ≡ 0 mod 10 (double highway at the W0/E0 and N0/S0 seams); SK rooms = sector centre 3×3 minus
    the centre (keeper lairs, 3 sources of 4000, mineral, unowned extractor, `sourceKeepers:true`); sector centre =
    3 sources, mineral, extractor, no controller; other rooms = controller, 1–2 sources, mineral. Outward edges of
    the bounding box get no exits (closed border); the terrain blob padding follows TERRAIN-WORLD-BLOB.
  * Parallel and deterministic: pass 1 generates every shared edge once with `seed("gen-edge", edgeId)` (each edge
    has exactly one generator ⇒ vanilla's "neighbour exists ⇒ copy" rule always holds); pass 2 generates every
    room interior on the pool with `seed("gen-room", name)`; a room failing the flood check 1000 times regenerates
    its exits from a derived stream (counted, logged).
  * Portals: none by default (the reference generator has none); `PortalSpec { every_n_sectors }` places official-
    shaped OBJ-PORTAL clusters (no `decayTime`) in sector centres paired with a distant centre; mods use `gen_room`.
* Sizes *(est.)*: 2×2 sectors (~441 rooms) < 1 s; 256×256 ≈ 30 s on 16 threads (~5 ms/room); 100 k rooms requires
  `wide_coords`. Memory ≈ 3.3 KB/room idle (§3.9). M1 default: `--world 2x2 --seed 1`.

---

## 12. Testing

### 12.1 Principles

* The oracle is official code run from `E:\screeps\server\node_modules\@screeps\*` (env `SCREEPS_ROOT`) or
  `E:\chitin\reference`, never copied into the repo; node 22 (the official `native.node` ABI).
* Both sides use RNG-SEED streams and `vclock`; `tools/oracle/lib/rng.js` (BigInt) is fixture-tested against
  `engine::policy`.
* Fixtures are generated by `tools/oracle/gen/*.js` and the area runners, committed under
  `crates/<crate>/tests/fixtures/<area>/`; `CHITIN_ORACLE=1` fuzz mode spawns node on random scenarios and writes
  failing ones as new fixtures for the owner.
* `tools/parity-report.js` collects every mirror file's `parity:` header with its fixture count into
  `docs/parity-status.md` (generated). **No file is FAITHFUL without fixtures.**

### 12.2 Processor / global / cron oracle (`tools/oracle/{processor,global,cron}.js`)

* The official `engine/src/processor.js` runs unchanged against a fake `@screeps/driver` injected through
  `Module._resolveFilename`: `queue.fetch` yields the scenario room once; `get*` readers return scenario data; the
  bulk writers are the **real** `driver/lib/bulk.js` over a capturing store that applies ops with
  `tools/oracle/lib/loki-update.js` (~40-line mirror of `storage/lib/db.js` updateDocument/dbBulk, cross-checked once
  against `lokijs`); activateRoom, notifications,
  history, mapView, eventLog, roomInfo captured; `pathFinder` = official glue + `native.node`; `Math.random` = room
  stream, `Date.now` = `vclock(T)`.
* Protocol: NDJSON on stdin/stdout. Scenario (FROZEN, `engine/src/scenario.rs` reads/writes it):
```json
{"kind":"room","worldSeed":"0x1","clock":{"baseMs":0,"baseTick":0,"nominalTickMs":1000},"gameTime":1234,
 "room":"W1N1","roomDoc":{},"terrain":"<2500 chars>","objects":[/*creation order*/],"flags":[],"users":{"<id>":{}},
 "intents":[["<uid>",{"objects":{},"objectsManual":null}]],   // processing order, explicit
 "accessibleRooms":["W1N1"],"roomStatusData":{},"worldSize":12,"compat":"private","tags":["creeps/harvest.js:12"]}
```
  Result: `{ops:{objects,users,flags}, docsAfter, flagsAfter, roomInfo, eventLog, mapView, history, activated,
  notifications, foreignInserts, crashed}`. `docsAfter` compared as canonical JSON with key order significant; the
  first diff is printed with the case's file:line tags. Global scenarios carry the five `getInterRoom` results and
  ordered `users.intents`; cron scenarios a world fragment and `G`.
* Rust runners: `crates/engine/tests/oracle_{processor,global,cron,bulk}.rs` replay committed fixtures.

### 12.3 Unit-level differential suites

| suite (generator) | oracle | covers |
|---|---|---|
| `gen/lodash-fixture.js` | lodash 3.10.1 | merge/cloneDeep/isEqual/assign/forEach snapshot over random graphs (identity, arrays, undefined, NaN, cycles) |
| `gen/bulk-fixture.js` | real `bulk.js` + updateDocument mirror | random op sequences: live state after each op and persisted docs |
| `gen/policy-fixture.js` | JS RNG-SEED port | seed/stream/order_key/vclock/lodash draw counts |
| `gen/system-fixture.js` | `common/lib/system.js` | Rust (JSON inputs) and JS (any values, node vm) sanitizers, storeIntents |
| `gen/diff-fixture.js` | `common/index.js` getDiff | diff shapes incl. dropped nested deletions (FMT-ROOM-DIFF) |
| `gen/mapgen-check.js` | `cli/map.js` | constraints + distribution histograms (χ² within tolerance) over 2 k rooms per class |
| pathfinder (existing) | official `native.node` | bit-identical search |

### 12.4 Golden whole-server replays (`tools/oracle/golden/`, `server/tests/golden.rs`)

`tools/oracle/record/` is a screepsmod for `E:\screeps\server`: seeds `Math.random` with the room stream before
each `processRoom` and the global stream around `global`, maps `Date.now` to vclock in engine processes, records the
per-tick user fetch order with `runner_threads = 1` (`runnerLoopStage('runUser')`), and dumps per-tick room
inputs/outputs plus a DB snapshot every K ticks. chitin replays the same world (Loki import) with the same code,
`order_override` = recorded order and the `Fixed` meter, comparing `rooms.objects`, `users` and `market` per tick
**exactly**. Ticks hitting an F row are skipped. A fixed-seed golden race (m1 bot + scripted rival, 5000 ticks →
world hash) guards refactors (13 ARCH-GOLDEN-RACE).

### 12.5 Real-bot smoke tests (`tools/smoke/`, `server/tests/smoke.rs`, nightly)

Bots: Overmind (built bundle), hivemind and a third real bot, plus JS and wasm starter bots. Deploy via HTTP, one world per bot + a 3-bot free-for-all,
20 k ticks at full speed, on the imported default world and a generated 2×2 world. Pass: no runtime error text the
same bot does not also produce on the official server under the same seed/world; RCL/GCL at 5 k/10 k/20 k within
±10 % of the official run (recorded by WP18 from `E:\screeps\harness`); no unexpected VM resets; CPU/tick within
±20 % with the `Thread` meter; heap/CPU stats logged.

### 12.6 Benchmarks (`tools/bench/`, `cargo bench`, gates in §2.9)

`proc_room` (c_r on recorded idle/RCL4/RCL8-war rooms), `runner_overhead` (o_i with a no-op bot, payloads
20 KB…2 MB), `isolates_1000`, `world_65k` (generate, idle 10 k ticks, publish, checkpoint), `scale_1000` (1000
synthetic bots: scripted CPU profiles + real simple bots, 256×256), `vanilla_compare` (2 bots, 2×2 vs the official
harness), `sockets_1000` (1000 SockJS clients × 2 rooms at 10 ticks/s: broadcaster < 10 % of a core, push latency
≤ 50 ms after roomsDone).

---

## 13. Source tree

```
Cargo.toml                       workspace: util, pathfinder, engine, runtime, backend, server; v8 =152.2.0 only in runtime
krebs.example.json               config (util::json): host/ports, world, compat, runner, persist, mods, auth
crates/util/src/
  base64 deflate json num rng sha1 (existing)
  pool.rs                        (existing) + worker_index(), for_each_dyn, map_dyn (batch 1)
  sys.rs                         NEW OS FFI: thread_cpu_ns, ThreadClock, physical_cores, os_random, on_ctrl_c
  hmac.rs                        NEW hmac_sha1, pbkdf2_sha1
  jsstr.rs                       NEW JsStr (§3.5)
crates/pathfinder/               (existing) native PathFinder port
crates/engine/src/               game rules + world model; no V8, no I/O
  lib.rs                         module tree
  constants.rs room_name.rs terrain.rs   (existing)
  doc.rs live.rs bulk.rs ids.rs policy.rs  core semantics (§3.1–3.3, §3.7, §2.7)
  world.rs                       §3.4–3.8 types + view(), presence/index maintenance, AccessSnapshot build
  common.rs                      common/index.js: getDiff, calcWorldSize, terrain codecs
  utils.rs                       engine/src/utils.js processor half
  system.rs                      common/lib/system.js (Rust, JSON inputs)
  intents.rs                     §4.3
  strongholds.rs                 common/lib/strongholds.js
  hooks.rs                       §10.1 engine hooks
  scenario.rs                    oracle adapter: scenario JSON ⇄ process_room / global / cron
  processor.rs, processor/**     §5.3
  runner/mod.rs                  RunJob, RunInput, RunOutput, CodeBundle
  runner/data.rs                 driver/lib/runtime/data.js (DB half): meta JSON, blob list
  runner/make.rs                 driver/lib/runtime/make.js (host half): users_to_run, save_run, billing, routing
  cron/cronjobs.rs               backend/lib/cronjobs.js
  cron/strongholds.rs            backend/lib/strongholds.js
  mapgen.rs                      backend/lib/cli/map.js + world layout
  persist.rs loki.rs             §9 checkpoints/journal/recovery; db.json import/export
crates/runtime/                  V8 host (deps: v8, engine, pathfinder, util)
  src/lib.rs                     §6.2 API, init_v8
  src/host.rs                    user-vm.js: isolates, snapshots, resets, system isolates
  src/sched.rs                   runner-phase scheduling (LPT; affinity/steal in M5)
  src/marshal.rs                 RunInput → V8 (external strings), outMessage → system isolate → RunOutput
  src/natives.rs                 cpuTime, compileAndRun, halt, heapStats, now/Date, base64ToBuffer, objectRoom
  src/natives_world.rs           terrain, nativeMod bridge, isAccessible, roomStatus, gridData, worldSize
  src/natives_market.rs          marketOrders, marketOrder, marketHistory
  src/watchdog.rs src/cpu.rs     deadlines/interrupts; meters
  src/governor.rs                heap budget policy
  src/server_isolate.rs          CLI sessions, JS-mod server slot
  src/bundle.rs                  include_str! file table
  js/**                          §7.1
  tests/conformance.rs
crates/backend/src/              HTTP/WS/SockJS, API, sockets, CLI transport, mod registry types (no V8)
  lib.rs http.rs ws.rs sockjs.rs authlib.rs server.rs broadcast.rs png.rs cli.rs mods.rs
  api/{auth,authmod,badge,game,leaderboard,market,power_creeps,register,user_messages,user}.rs
  socket/{server,rooms,map,user,system}.rs
crates/server/                   binary `chitin`
  src/main.rs                    launcher: config, init_v8 before threads, `serve`, `new`, `import`, `export`, `user add`
  src/config.rs                  krebs.json + flags; CFG-ENGINE-DEFAULTS
  src/tick.rs                    §2.2 pipeline (engine main.js + runner.js)
  src/mods.rs src/mods/**        Rust mod list, JS mod discovery, load order; built-in mods
  src/history.rs                 driver/lib/history.js (history mod writer)
  tests/{m1,golden,smoke}.rs
tools/
  v8-junction.ps1 (existing), v8-ptrcomp.{ps1,patch,profile}, parity-report.js
  oracle/lib/{rng,vclock,loki-update,ivm-shim,fake-driver}.js   oracle/gen/*.js   oracle/{processor,global,cron}.js
  oracle/api/**  oracle/record/**  oracle/golden/**
  bots/m1/main.js   bench/**   smoke/**
vendor/v8/                       the v8 152.2.0 crate without its V8 tree (v8-ptrcomp.ps1 adds it); a cage per isolate
docs/ARCHITECTURE.md  docs/spec/*  docs/parity-status.md (generated)
```

Crate dependencies: `util ← pathfinder ← engine ← {runtime (+v8), backend} ← server`. Only `runtime` and `server`
build V8 (`tools/v8-junction.ps1` when target/ is on another drive than `vendor/v8`, cargo `jobs = 4`).

---

## 14. Work packages, waves, milestones

### 14.1 Rules for parallel agents

1. Each WP runs in its own git worktree/branch and edits only its owned files, its fixtures under
   `crates/<crate>/tests/fixtures/<wp-area>/`, and its `tools/` paths listed below.
2. WP00 writes every `Cargo.toml`, every `mod` declaration and `runtime/src/bundle.rs` complete (every file mirrors a
   known vanilla file) and they are then frozen, owned by the integrator WP11; where a `mod` block sits at the top
   of an owned file (e.g. `processor.rs`), the owner does not edit that block. A new file = interface PR. Dispatch
   and router tables WP00 writes belong to the file's owner afterwards (`processor.rs` → WP09, `server.rs` → WP04).
3. A missing piece owned by another WP is a stub you do not fill; test against oracle fixtures instead.
4. FROZEN items change only through a small interface PR merged by WP11; everyone rebases. Additive changes
   (new private helpers, new enum variants at the end of non-exhaustive internal enums) stay in the owner's file.
5. CI per WP: `cargo test` on non-V8 crates always; the V8 crates (`runtime`, `server`) only for WP08, WP11, WP17,
   WP18, WP19, WP21.
6. A WP is done when its files are FAITHFUL or DIVERGENT-with-ID and their oracle cases pass.
7. After WP00b, `doc.rs live.rs bulk.rs ids.rs policy.rs` are owned by WP03 (fixes from fuzz failures).
8. Sizes: S ≈ 1 agent-day, M ≈ 2–4, L ≈ 5–8, XL > 8 (rough).

### 14.2 Wave 0 (one agent, two tagged commits)

| WP | scope | owns | acceptance | size |
|---|---|---|---|---|
| **WP00a skeleton** | every crate and file of §13 as a compiling stub with mirror + `parity: MISSING` headers; all FROZEN types/signatures of §§3–10 with `todo!()` bodies; processor intent dispatch, router and channel tables calling stub fns; JS stubs + bundle manifest; lodash copy; `tools/parity-report.js`, `chitin.example.json`. Tag `w0-skeleton` | all manifests, module trees, `bundle.rs`, dispatch/router tables, every stub | `cargo build --workspace` incl. v8 on Windows; existing util/engine/pathfinder tests green; `cargo doc` shows every FROZEN item; parity report lists every file MISSING | M |
| **WP00b core semantics** | util `pool` (worker_index, for_each_dyn, map_dyn), `sys.rs`, `hmac.rs`, `jsstr.rs`; engine `doc.rs`, `live.rs`, `bulk.rs`, `ids.rs`, `policy.rs`; `tools/oracle/lib/{rng,vclock,loki-update}.js`, `tools/oracle/gen/{lodash,bulk,policy}-fixture.js`. Tag `w0-core` | those files | lodash and bulk fixture suites identical (incl. alias/absent-key/truthy-scalar, hidden keys, temp ids, op order); RNG/order-key/draw vectors equal rng.js; `thread_cpu_ns` within 5 % of a busy-loop reference; PBKDF2 RFC 6070 vectors; `for_each_dyn` claims in index order | M |

### 14.3 Wave 1 — M1 sprint (12 agents; `[s]` branches from `w0-skeleton`, `[c]` from `w0-core`)

| WP | scope | owns | frozen deps | acceptance | size |
|---|---|---|---|---|---|
| **WP01 world services** [c] | world bodies, presence/index maintenance, view, AccessSnapshot + exit counts, blobs, per-user meta | `engine/src/{world,common}.rs`, `engine/src/runner/data.rs`, `tools/oracle/gen/diff-fixture.js` | §3, §6.6 | getDiff fixtures; ORDER-RUNTIME-ROOMIDS/OBJECTS + VIS-ROOMS cases (observer, sites-only room, `"null"` observeRoom); blob = node `JSON.stringify` of the docs, offsets slice-parse; `view()` 65 k rooms < 1 ms; index-vs-scan property tests; exit counts = `mapgrid.js` | M |
| **WP02 intents & sanitizer** [c] | system.js in Rust and JS, system-isolate glue, storeIntents output parsing | `engine/src/{system,intents}.rs`, `runtime/js/common/system.js`, `runtime/js/host/system.js`, `tools/oracle/gen/system-fixture.js` | §4 | Rust and JS sanitizers identical to official on fixtures (JS incl. NaN/Infinity/BigInt/Date/getters in node vm); `__store` output → `UserIntents` incl. room grouping, unknown rooms dropped, raw-list count | S |
| **WP03 oracle harness** [c] | fake driver, processor/global/cron runners, recorder mod, scenario adapter, Rust oracle runners; owner of core semantics after WP00b | `tools/oracle/{lib/fake-driver.js,processor.js,global.js,cron.js,record/**}`, `engine/src/scenario.rs`, `engine/tests/oracle_*.rs`, core files (rule 7) | §12.2 | official processRoom/global/cron run deterministically on 20 sample scenarios; Rust runner diffs with tags; fuzz mode; bulk fuzz 500 cases; recorder captures 100 ticks with `runner_threads=1` | L |
| **WP04 backend** [s] (M1 subset first) | networking, auth, M1 routes (FLOW-CONNECT/FIRST-LOGIN/VIEW/DEPLOY), sockets, broadcaster, PNG | `backend/src/**` except `cli.rs`, `mods.rs` | §8.2, `WorldView` | envelope/401/CORS/password-gate tests; SockJS framing + raw ws; gz; steam `single-user`; signin/register/Basic; M1 routes vs spec 10 shapes on a fake world; room snapshot + diff stream vs a scripted publisher; PNG decodes | L |
| **WP05 JS runtime core** [s] | host prelude, driver runtime files, utils.js, core game files, API oracle harness | `runtime/js/host/prelude.js`, `runtime/js/driver/{runtime,runtime-driver,system-functions,mapgrid}.js`, `runtime/js/engine/utils.js`, `runtime/js/engine/game/{game,console,constants,names,resources,custom-prototypes,store}.js`, `tools/oracle/{lib/ivm-shim.js,api/**}` | §6.6–6.9, §7 | API oracle: Game construction keys/order, require/wrapper/+36 col, Memory/RawMemory/segments failure matrix, console, intent CPU, `__tick` assembly orders | L |
| **WP06 JS rooms & map** [s] | rooms, positions, paths, map, flags, sites, sources, minerals | `runtime/js/engine/game/{rooms,path-finder,path-utils,map,flags,construction-sites,sources,minerals}.js`, `runtime/js/driver/path-finder.js` | §6.7, §7 | API oracle per method incl. spec 03 return-code rows; M1 subset first: Room find/look*/createConstructionSite, RoomPosition, findPath, PathFinder/CostMatrix, Source | L |
| **WP07 JS objects** [s] | creeps, structures, power creeps, market, tombstones, ruins, deposits, nukes | `runtime/js/engine/game/{creeps,structures,power-creeps,market,tombstones,ruins,deposits,nukes}.js` | §6.7, §7 | API oracle incl. every spec 04/05 return-code row; M1 subset first: Creep move/moveTo/harvest/transfer/upgradeController/build/repair/pickup/drop/say/suicide, StructureSpawn.spawnCreep/Spawning, controller, extension | L |
| **WP08 runtime host** [s] | V8 init, snapshots, isolates, system isolates, natives core + world, marshal, watchdog, meters, LPT scheduler | `runtime/src/{lib,host,sched,marshal,natives,natives_world,watchdog,cpu}.rs`, `runtime/tests/conformance.rs` | §6 | 1000 isolates < 5 s, baseline ≤ 5 MB; infinite loop dies at deadline ± 2 ms (Thread), uncatchable, earlier intents/Memory kept; nested require timeout; heap blow-up → Reset; external strings shared (pointer check); same user on 2 threads; clone failure → TotalError; pfSearch bridge passes pathfinder fixtures; conformance suite green on a fixture bundle | L |
| **WP09 processor loop + creeps** [c] | processor.js, utils.js (processor half, complete), movement, `_damage`, `_create_energy`, all non-NPC creep files | `engine/src/{processor,utils}.rs`, `processor/intents/{movement,_damage,_create_energy}.rs`, `processor/intents/creeps/*.rs` | §5 | oracle fixtures per file; M1 subset first: loop order, move, harvest, transfer, withdraw, pickup, drop, build, repair, upgradeController, say, suicide, tick, `_die`, `_recalc_body`, `_add_fatigue`, `_clear_newbie_walls`, `_drop_resources_without_space`; `c_r` bench | XL |
| **WP10 processor structures + room** [c] | every other `processor/intents/*` dir except `creeps/`, `power_creeps/`, `invader_core/` | `processor/intents/_calc_spawns.rs` + dirs `spawns controllers sources minerals energy extensions extractors roads constructed_walls ramparts containers storages links towers labs factories power_spawns terminal nukers nukes keeper_lairs portals deposits construction_sites tombstones ruins room flags structures` | §5 | oracle per file; M1 subset first: `_calc_spawns`, spawns/{intents,create_creep,tick,_born_creep,_charge_energy}, controllers/tick, sources/tick, energy/tick, extensions/tick, construction_sites/tick, room/{intents,create_construction_site,remove_construction_site}, roads/containers/tombstones/ruins tick, structures/_destroy | XL |
| **WP11 tick loop + integration** [c] | launcher, config, P0–P7, pre-run checks, P2 save/billing/routing, publish/`TickReport`, pacing/pause, CLI wiring; integrates landing WPs; owns manifests and module trees after WP00 | `server/src/{main,config,tick}.rs`, `engine/src/runner/make.rs`, `server/tests/m1.rs`, `tools/bots/m1/main.js` | everything | ACT-SOURCES timing tests; barrier-visibility tests; TICK-GET-USERS Loki-operator tests; CPU-BUCKET-UPDATE/RUN-SKIP matrix; 10 k ticks on stubs; **M1** | L |
| **WP12 world generation** [s] | generateRoom port, layout, parallel edge/room passes | `engine/src/mapgen.rs`, `tools/oracle/gen/mapgen-check.js` | §11 | distributions vs map.js (wall ratio ±2 %, sources, 100 % flood connectivity); exits match neighbours; deterministic by seed; 256×256 < 60 s on the dev box | M |

`processor::install` (P4) lives in `processor.rs` (WP09); `processor/global.rs` is an M1 stub returning immediately.

### 14.4 Wave 2 — parity (after M1; wave-1 owners continue the rest of their files)

| WP | scope | owns | acceptance | size |
|---|---|---|---|---|
| WP04/05/06/07/09/10 (cont.) | remaining routes and `socket/system.rs`; remaining JS methods; remaining processor files (combat creeps, all structures) | as wave 1 | oracle + coverage gate green for every owned file | — |
| WP08 (cont.) | `natives_market.rs`, InterShardMemory (mmo), heap-stats exact shape, conformance completion | + `runtime/src/natives_market.rs` | §7.4 list green | M |
| **WP13 power creeps + global** | room-side PC intents; global.rs; market; power | `processor/intents/power_creeps/**`, `processor/global.rs`, `processor/global_intents/**` | global oracle (sends, deal distance ties, shuffle draws, order maintenance, PC lifecycle, double tombstone); ORD points 2–4 with order override | L |
| **WP14 NPC + strongholds** | fake runtime, keepers, invaders, invader core, strongholds | `processor/common/fake_runtime.rs`, `processor/intents/creeps/{keepers,invaders}/**`, `processor/intents/invader_core/**`, `engine/src/strongholds.rs` | oracle incl. multi-tick NPC chases, stronghold deploy/collapse, every RNG site draw count | L |
| **WP15 crons** | all cron jobs | `engine/src/cron/**` | cron oracle on world fragments with cron streams; force-update cadence 105/126 ticks | M |
| **WP16 persistence** | checkpoints, journal, recovery, Loki import/export | `engine/src/{persist,loki}.rs` | kill -9 at random points → recovery to the last manifest + journal; import + 1 tick = official; export round trip | M |
| **WP17 CLI + server isolate** | server isolate, CLI JS ports, TCP transport | `runtime/src/server_isolate.rs`, `runtime/js/cli/**`, `backend/src/cli.rs` | every vanilla CLI command reachable; output-format cases; storage query operators | M |
| **WP18 golden + smoke** | recorder replays, golden race, real-bot harness, official baselines | `tools/oracle/golden/**`, `tools/smoke/**`, `server/tests/{golden,smoke}.rs` | §12.4/§12.5 harnesses run; baselines recorded | M |

### 14.5 Wave 3 — scale and mods

| WP | scope | owns (handed over) | acceptance | size |
|---|---|---|---|---|
| **WP19 scale** | affinity + steal, governor, broadcaster fan-out, benches, measurement-driven fixes filed to owners | `runtime/src/{sched,governor}.rs` (from WP08), `backend/src/broadcast.rs` (from WP04), `tools/bench/**` | §2.9 targets (M5) | L |
| **WP20 lazy delivery (A′)** | per-object lazy parse + compact construction index | `engine/src/runner/data.rs` (from WP01), `runtime/js/host/prelude.js`, `runtime/js/engine/game/game.js` (from WP05) | every API oracle case unchanged; ≤ 1 µs/object-view | M |
| **WP21 mods** | registry wiring, JS mod loader, server-slot API, built-in history/devtools/compat-tools, examples | `backend/src/mods.rs`, `server/src/{mods,history}.rs`, `server/src/mods/**`, `mods/examples/**` | example Rust mod (custom intent + custom object type) and JS mod (custom prototype + route + cron) pass; deterministic load order persisted | M |
| WP06 (cont.) | `wideCoords` RoomPosition packing | `rooms.js` | 320×320 world plays; packing round trips | S |

### 14.6 Milestones

* **M1 — vertical slice (end of wave 1).** `server/tests/m1.rs`: `chitin new --world 2x2 --seed 1`
  generates the world; `chitin serve` on 127.0.0.1:21025; register via `register/submit` + `auth/signin`;
  `place-spawn`; `POST /api/user/code` with `tools/bots/m1/main.js` (harvest, upgrade, spawn using `Game.spawns`,
  `spawnCreep`, `Memory`, `find(FIND_SOURCES)`, `moveTo`, `harvest`, `transfer`, `upgradeController`, `build`);
  3000 ticks at tick rate 0 reach RCL ≥ 2 with ≥ 4 creeps alive and no runtime/engine errors; a SockJS test client
  receives a consistent `room:` diff stream; manually: the official Steam client (steam `single-user`) and the
  steamless client connect, show the map, the room with moving creeps, and console output. Processor files outside
  the M1 subsets are stubs that ignore their intents and log once. No persistence, NPCs, market or crons.
* **M2 — engine parity.** Every processor, global and cron file FAITHFUL with fixtures; COMPAT rows tested in both
  profiles; golden replays of 3 recorded worlds match exactly for 10 k ticks; a 50 k-tick run with 4 deterministic
  bots plus NPCs has no internal errors.
* **M3 — runtime/API parity + durability.** Every JS file FAITHFUL with the coverage gate; conformance green;
  persistence kill tests and `db.json` import pass; CLI complete; Overmind, hivemind and a third real bot meet §12.5.
* **M4 — clients, tools, mods.** All spec 10 MVP/MVP1/UI/TOOL items; steamless client and grunt/rollup deploys
  verified; mods v1 with examples and history.
* **M5 — scale.** §2.9 targets on 16C/64 GB (or the dev-box proxy); 1000 sockets; `wideCoords` on a 320×320 world.

Critical path to M1: WP00a → WP00b → {WP09, WP10 M1 subsets} ∥ {WP08, WP05/06/07 M1 subsets, WP04 M1, WP12,
WP01, WP02} → WP11 integration.

### 14.7 Open risks (owner)

1. Eager Game construction above ~2.5 µs/object ⇒ unbilled overhead > 30 % of ΣB: WP19 measures, WP20 lazy path.
2. External-string retention and V8 external-memory GC pressure across 1000 isolates: WP08 counters, WP19 governor.
3. SMT billing inflation if `runner.threads` > physical cores: WP19 decides after measurement (default physical).
4. V8 15.2 vs node 22's V8 12.4 (new builtins bots may feature-detect, stack/error texts): WP08 conformance, WP18 smoke.
5. Live-heap load cost for 600-object rooms (`from_val` of every doc): WP09 `proc_room` bench; fallback = convert docs
   on first access by id, prescan reading only `type`.
6. Base-order assumption (01/11 open question 1): WP03 recorder confirms on captured official ticks.
7. Thread-per-connection at 1000 clients: WP19 `sockets_1000`; IOCP/epoll only past 5 % CPU.
8. Steam `trust` ticket layout: WP04 with a captured ticket (M1 uses `single-user`, no parsing).
