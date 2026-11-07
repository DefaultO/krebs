# 14 — JavaScript engine requirements (chitin's in-house JS engine)

Status: research spec. It answers one question: **what must chitin's own JavaScript engine (Rust std only) implement, and how exactly must it behave, so that bots written for the official Screeps server run unchanged and behave the same?**

The requirements come from four kinds of evidence:

1. **The official runtime**: what the engine/driver code itself uses and how it hosts player code (V8 via isolated-vm).
2. **The runtime's library**: lodash 3.10.1, which runs inside the player realm.
3. **A corpus scan of real bots**: syntax and builtin usage, counted per repository.
4. **Behaviour probes**: V8's observable behaviour, checked on a local Node.

Runtime *semantics* (require, Memory, console, CPU billing, global reset) are fully specified in `02-runtime-sandbox.md`. This spec does not repeat them. It lists only the **engine-level** consequences, cross-references the 02 IDs, and resolves several of 02's open questions from the isolated-vm source (§2.9, §2.10, §6.3).

---

## 0. Sources, method and conventions

### 0.1 Sources

* `reference/<pkg>/<path>:<line>`: the official packages (engine @8097782, driver @cf63d8a, backend 3.3.0, launcher 4.2.0, docs, lodash-3.10.1, xxscreeps @d1e3bad).
* **[ivm]** `E:/screeps/server/node_modules/isolated-vm/src/...`: the installed **isolated-vm 6.1.2**. This is exactly the version pinned by the driver lockfile (`reference/driver/package-lock.json:2589-2601`). It is outside `reference/`, but it is the only source for termination, microtask and stack-rendering behaviour.
* **[bundle]** `E:/screeps/server/node_modules/@screeps/driver/build/runtime.bundle.js`: the installed, already-built webpack 3 runtime bundle (driver 5.3.0 = reference driver version, engine 4.3.0 vs reference 4.3.2; 28 786 lines, modules `/* 0 */`…`/* 65 */`). It is cited as `[bundle] m<N>:<line>` and used only to confirm what webpack emitted (§2.14); rules are still cited to `reference/` sources.
* **Bot corpus**:
  * `E:/screeps-game/reference/bots/<group>/<repo>/...`, abbreviated as `bots/<group>/<repo>/...`;
  * the primary bots Overmind (as `Overmind-dev/`) and hivemind (as `hivemind/`).
* **[V8 src]**: V8 source paths (e.g. `src/objects/call-site-info.cc`, `third_party/v8/builtins/array-sort.tq`, `src/base/ieee754.cc`, `src/numbers/conversions.cc`, `src/numbers/math-random.cc`) are named as **porting pointers only**. V8 is not in the local reference set, so every behaviour attributed to V8 here is backed by a [probe] vector, or flagged in §10.
* **[probe]**: behaviour observed by running local **Node 26.8.1 (V8 14.x)** through `node:vm` with `displayErrors:false`, mimicking the driver's script wrapper and filenames. The V8 formats probed here have been stable for many V8 versions. Items whose stability across the target range (V8 12.4 → 14) is uncertain are listed in §10.

### 0.2 Corpus method (evidence counts)

* **Scope**: 500 cloned repos. Bot code was taken from the groups `full-bot`, `starter`, `library`, `gist` and `discovered`, plus the 3 primary bots.
* **Exclusions**:
  * Screeps **Arena** repos;
  * non-sandbox repos (server mods, web clients, API clients, type packages);
  * empty clones;
  * non-JS bots (Rust/Python/Kotlin sources).
* **Result: N = 355 JS/TS bot repos**: 189 JS-only, 166 with TypeScript sources, 28 with a built `dist/` bundle.
* **Parsing**: every `.js/.mjs/.cjs/.ts` file was parsed with the TypeScript compiler API (AST walk, not regex).
* **Skipped files and directories**:
  * `node_modules`, `.yarn` (vendored yarn releases polluted early counts), tests/mocks/docs;
  * build configs (`rollup.config.*` etc.);
  * the repo-root `scripts/ tools/ bin/ server/` directories.
* **Notation**: `R=n (js a / ts b / dist c)` means *n* of the 355 repos use the feature: *a* in hand-written JS (deployed as-is), *b* in TS sources, *c* in built bundles.
* **How TS code reaches the engine**: TS sources reach the engine only after compilation, so their syntax depends on `tsconfig` `target`. The observed targets:

  | Target | Repos |
  |---|---|
  | es2017 | 45 |
  | es2018 | 54 |
  | es2019 | 3 |
  | es2020 | 7 |
  | es2021+ | 9 |
  | es5 | 6 |
  | es6/es2015 | 17 |
  | none (TS default es5) | 22 |

  So **TS repos mostly emit ES2017/ES2018 syntax**: classes, getters, async, for-of and spread are native, while optional chaining and class fields are down-levelled.
* **Bundles**: the **esbuild** bundles present in the corpus emit **ES2020–ES2022 syntax natively** (optional chaining, `??`, `??=`, class `static {}` blocks, `catch {}`). Examples: `bots/full-bot/Mirroar__hivemind/dist/main.js:1809`, `bots/full-bot/bencbartlett__Overmind/dist/main.js:10119`, `bots/full-bot/The-International-Screeps-Bot__The-International-Open-Source/dist/main.js`.

Counts are lower bounds on "uses" and upper bounds on "needs". A feature found only in TS source with target < its ES year is down-levelled and does not reach the engine. Evidence lines state when that applies.

### 0.3 Markers

* **GOTCHA**: a subtle 1:1 pitfall.
* **PERF**: performance-relevant structure. Each says what is per-player, what is shared, what is hot, and what the original does slowly.
* Priorities:
  * **P0**: must have to run typical bots.
  * **P1**: needed by a notable minority of bots, or for exact parity of common outputs.
  * **P2**: rare, or only needed for full V8 parity.

---

## 1. Inventory

| ID | Area | Item | Pri | Evidence (R = repos of 355) / source |
|---|---|---|---|---|
| **ENV — engine era & hosting contract** |||||
| ENV-TARGET-V8 | env | Target = V8 of Node ≥ 22 (isolated-vm 6.1.2), i.e. ES2024 baseline | P0 | driver/package.json:11; driver/package-lock.json:2589-2601 |
| ENV-ONE-REALM | env | Runtime (engine `game/*`, lodash) and player code share ONE realm/global; builtins are mutable and shared | P0 | driver/lib/runtime/runtime.js:216-231; user-vm.js:59 |
| ENV-PERSISTENT-HEAP | env | Per-player heap persists across ticks; reset rules (→02 VM-*) | P0 | driver/lib/runtime/user-vm.js:13-101 |
| ENV-HEAP-LIMIT | env | Hard per-player heap limit (256 MB + terrain); exceeding disposes VM | P0 | driver/lib/runtime/user-vm.js:30; make.js:285-288 |
| ENV-HEAP-STATS | env | `getHeapStatistics()` object: 10 fields, fixed order, V8 semantics | P0 | [ivm] module/isolate_handle.cc:346-377; R=31 txt |
| ENV-GLOBALS-PRESENT | env | Exactly the V8 builtin set (ES + WebAssembly + Intl + V8 extras) | P0 | §2.4 |
| ENV-GLOBALS-ABSENT | env | No timers, queueMicrotask, structuredClone, TextEncoder/Decoder, atob/btoa, performance, Buffer, process, self/window | P0 | §2.5; bots polyfill these |
| ENV-CONSOLE-REPLACED | env | V8 `console` replaced by fake console (no error/warn/info) | P0 | engine/src/game/game.js:470-473; R=58 call console.error/warn |
| ENV-SCRIPT-ORIGIN | env | Script name = module name; wrapper adds +36 cols on line 1 | P0 | driver/lib/runtime/runtime-driver.js:23,67 |
| ENV-SLOPPY-DEFAULT | env | Modules are sloppy unless `'use strict'` directive; console eval is sloppy direct eval | P0 | runtime-driver.js:59,67; R=90 use strict files |
| ENV-ENGINE-STRICTNESS | env | Each runtime module keeps its own mode (webpack 3 adds no directive). Strict: `game.js`, `game/path-finder.js`, `game/path-utils.js`, `Room.Terrain`, `driver/lib/path-finder.js`. Everything else is sloppy. chitin's native API functions must copy their source module's mode (receiver coercion, silent failed writes, `.caller`) | P0 | driver/webpack.config.js:6; driver/package.json:16; game.js:1; game/path-finder.js:1; path-utils.js:1; rooms.js:1223; driver/lib/path-finder.js:1; market.js:83-97 |
| ENV-CODEGEN | env | `eval`/`Function` allowed (deny only if source > memoryLimit/8 chars) | P1 | [ivm] isolate/environment.cc:169-182,392,483 |
| ENV-TERMINATION | env | CPU-timeout = uncatchable termination, unwinds to the runSync boundary, then cancelled; `finally` does not run | P0 | [ivm] isolate/run_with_timeout.h:39-166 |
| ENV-MICROTASKS | env | Promise jobs drained only at the outer task epilogue, after tick output is captured; unhandled rejection fails the whole run | P1 | [ivm] environment.cc:490-501; three_phase_task.cc:182,225-258 |
| ENV-CPU-CLOCK | env | `cpuTime`: thread CPU time of finished segments + steady-clock delta of the running one | P0 | [ivm] isolate/executor.cc:47-104; runtime.js:25-27 |
| ENV-SNAPSHOT | env | Runtime pre-initialized from a V8 startup snapshot (`<runtime>`) | P1 | driver/make-runtime-snapshot.js:5; user-vm.js:30,154 |
| ENV-BINARY-BUFFER | env | Binary module value = buffer@4.9.1 `Buffer` (a Uint8Array subclass) | P1 | runtime-driver.js:13-15; driver/package-lock.json:405-406 |
| ENV-ESM-NONE | env | No ES modules; import/export are SyntaxErrors (xxscreeps differs) | P0 | runtime-driver.js:67; xxscreeps/packages/xxscreeps/driver/runtime/module.ts:12-30 |
| **SYN — syntax** |||||
| SYN-ES5 | syntax | Full ES5.1 (var, function, closures, `arguments`, getters in literals, labelled stmts, regex literals) | P0 | R=355 |
| SYN-STRICT | syntax | Strict and sloppy mode both exact (directive prologue in wrapper fn) | P0 | R=90 (js 80) strict files; ~100+ sloppy JS repos |
| SYN-ANNEXB | syntax | Sloppy legacy: block function decls, `__proto__` in literals, legacy octal, `<!--` | P1 | fn-in-block R=10; `__proto__` R=16 |
| SYN-LET-CONST | syntax | `let`/`const`, TDZ, per-iteration loop bindings | P0 | R=315/309 |
| SYN-ARROW | syntax | Arrow functions (lexical this/arguments) | P0 | R=325 |
| SYN-TEMPLATE | syntax | Template literals | P0 | R=257 |
| SYN-TAGGED-TEMPLATE | syntax | Tagged templates, `String.raw`, template-object caching | P1 | R=9 (ts 7), String.raw R=4 |
| SYN-CLASS | syntax | `class`, `extends`, `super`, methods, static methods | P0 | R=186 (js 72, dist 15) |
| SYN-ACCESSORS | syntax | get/set in classes and object literals | P0 | class getters R=114, setters R=30, literal getters R=14 |
| SYN-CLASS-FIELDS | syntax | Public instance/static fields (`[[Define]]` semantics) | P0 | R=101/78 (mostly TS, down-levelled); native in esbuild bundles |
| SYN-STATIC-BLOCK | syntax | `static { }` | P0 | dist R=3 (hivemind, Overmind, International esbuild bundles) |
| SYN-PRIVATE | syntax | `#x`, `#m()`, `#x in o` | P1 | R=1 (ts) |
| SYN-DESTRUCTURING | syntax | Object/array patterns in decls, params, assignment | P0 | R=125/115; params R=81 |
| SYN-SPREAD-REST | syntax | Spread in calls/arrays, rest params | P0 | R=119/108/80 |
| SYN-OBJ-SPREAD-REST | syntax | `{...o}`, `{a, ...rest}` | P0 | R=71 / 7 |
| SYN-DEFAULT-PARAMS | syntax | Default parameters (separate scope) | P0 | R=185 |
| SYN-COMPUTED-SHORTHAND | syntax | Computed keys, shorthand props/methods | P0 | R=128/174 |
| SYN-FOR-OF | syntax | for-of with the iterator protocol | P0 | R=217 |
| SYN-FOR-IN | syntax | for-in incl. prototype chain, deletion during iteration | P0 | R=290 |
| SYN-GENERATORS | syntax | `function*`, `yield`, `yield*`; generator objects live across ticks | P0 | R=21 (TooAngel in-game); `yield*` R=5 |
| SYN-ASYNC | syntax | async functions / await | P1 | R=24 (mostly tooling; International dist 2) |
| SYN-ASYNC-ITER | syntax | for-await, async generators | P2 | R=1 |
| SYN-OPTIONAL-CHAIN | syntax | `?.` (member, call, element) | P0 | R=102 (js 6, ts 96, dist 5; hivemind esbuild dist 207 sites) |
| SYN-NULLISH | syntax | `??` | P0 | R=62 (hivemind dist 43 sites) |
| SYN-LOGICAL-ASSIGN | syntax | `??=`, `\|\|=`, `&&=` | P0 | R=21 (International dist 10) |
| SYN-EXPONENT | syntax | `**`, `**=` | P0 | R=19 |
| SYN-NUMERIC-LIT | syntax | `0b`/`0o`, numeric separators, legacy octal (sloppy) | P0 | binary R=11, separators R=19 (TS sources; tsc down-levels them below es2021, esbuild keeps them at esnext) |
| SYN-BIGINT-LIT | syntax | `10n` | P2 | R=1 (ScreepsDotNet) |
| SYN-OPTIONAL-CATCH | syntax | `catch {}` | P0 | R=17 (dist 3) |
| SYN-LABELS | syntax | Labelled statements, labelled break/continue | P0 | R=26 / 24 |
| SYN-WITH | syntax | `with` (sloppy only) | P2 | R=1 (TS, not deployable) |
| SYN-DEBUGGER | syntax | `debugger` = no-op | P1 | R=3 |
| SYN-NEW-TARGET | syntax | `new.target` | P1 | R=1 |
| SYN-REGEX-LIT | syntax | Regex literals & flags g i m (P0), u s y (P1), d v (P2) | P0 | regex R=174; g 143, m 90, i 17, u 5, s 4, y 2 |
| SYN-DIRECT-EVAL | syntax | Direct eval with caller scope (var leakage in sloppy) | P1 | R=14 |
| SYN-FUNCTION-CTOR | syntax | `Function(...)`/`new Function(...)` incl. array params | P1 | R=7/5; lodash `_.template` |
| SYN-TOPLEVEL-RETURN | syntax | `return` at module top level is legal (function wrapper) | P1 | runtime-driver.js:67 |
| SYN-HASHBANG | syntax | `#!` at module start is a SyntaxError (inside wrapper) | P2 | runtime-driver.js:67 |
| **BI — builtins** |||||
| BI-OBJECT | builtin | Object statics incl. defineProperty/getOwnPropertyDescriptor/entries/fromEntries | P0 | keys R=188, values 120, defineProperty 96, entries 59 |
| BI-FUNCTION | builtin | call/apply/bind, `name`, `length`, own props, `.caller` | P0 | call R=115, apply 85, bind 34 |
| BI-ARRAY | builtin | Array incl. ES2016–2023 methods (includes, flat, flatMap, at, findLast) | P0 | push 257, filter 258, includes 154, at 17, flat 11 |
| BI-ARRAY-SORT | builtin | V8 TimSort exactly (stable, comparator call sequence) | P0 | sort R=183; `.sort()` R=39; boolean comparators R=4 |
| BI-STRING | builtin | String incl. Annex B `substr`, padStart, repeat, at, replaceAll, raw | P0 | substr R=77, padStart 33, repeat 27 |
| BI-STRING-UNICODE | builtin | Full Unicode case mapping, normalize, codePointAt/fromCodePoint | P1 | fromCodePoint R=14, codePointAt 11 |
| BI-NUMBER | builtin | Number statics, parseInt/parseFloat, isInteger/isFinite | P0 | parseInt R=161, Number.isInteger 43 |
| BI-NUM-TOSTRING | builtin | Shortest round-trip `ToString(Number)`; V8 radix algorithm | P0 | radix toString R=58; `Math.random().toString(36)` R=10 |
| BI-NUM-FORMAT | builtin | Exact toFixed/toPrecision/toExponential | P0 | toFixed R=111, toPrecision 8 (hivemind 28 sites) |
| BI-MATH | builtin | Math incl. ES2015 functions; fdlibm-exact transcendentals | P0 | Math.* R=231 |
| BI-MATH-RANDOM | builtin | Math.random: uniform [0,1), 52-bit, unseeded (xorshift128+ in V8) | P0 | R=127 + lodash + engine names.js |
| BI-JSON-PARSE | builtin | JSON.parse incl. reviver, `__proto__` key, big numbers | P0 | R=78 + Memory |
| BI-JSON-STRINGIFY | builtin | JSON.stringify: well-formed escapes, replacer, indent, toJSON | P0 | R=151 + Memory |
| BI-DATE | builtin | Date.now/getTime (P0), parse/format/local TZ (P1) | P0 | Date R=70, Date.now 31 |
| BI-REGEXP | builtin | Irregexp semantics; lookbehind/named groups/sticky/unicode/dotAll (P1); v flag (P2) | P0 | exec R=130, test 65, match 68, replace 93 |
| BI-MAP-SET | builtin | Map/Set (insertion order, SameValueZero) | P0 | Set R=81, Map 74 |
| BI-WEAK | builtin | WeakMap/WeakSet (P1); WeakRef/FinalizationRegistry (P2) | P1 | WeakMap R=8, WeakSet 2, WeakRef 0; lodash metaMap |
| BI-SYMBOL | builtin | Symbol + well-knowns (iterator, toStringTag, toPrimitive, hasInstance, species) | P0 | R=17; Symbol.iterator 13 |
| BI-ITERATORS | builtin | Iterator protocol objects (Array/Map/Set/String iterators) | P0 | for-of R=217 |
| BI-PROXY | builtin | Proxy (all traps; engine `Store` uses `get`) | P0 | engine/src/game/store.js:87-96; R=12 |
| BI-REFLECT | builtin | Reflect.* | P0 | R=19 (TS `__decorate` probes `Reflect.decorate`) |
| BI-PROMISE | builtin | Promise (+ all/allSettled/any/race/resolve/finally) | P1 | R=11 (in-game use rare) |
| BI-TYPEDARRAY | builtin | All typed arrays; CostMatrix = Uint8Array(2500) + Uint32Array view | P0 | Uint8Array R=31, Int32Array 20, Uint16Array 15, Uint32Array 12 |
| BI-ARRAYBUFFER | builtin | ArrayBuffer incl. detach (wasm memory.grow) and isView | P0 | R=14; wasm-bindgen glue |
| BI-DATAVIEW | builtin | DataView incl. Big(U)Int64 | P1 | R=2 |
| BI-BIGINT | builtin | BigInt + BigInt64Array; wasm i64 interop | P2 | R=2 |
| BI-INTL | builtin | toLocaleString (en-US), localeCompare (ICU root collation), DateTimeFormat timeZone | P1 | toLocaleString R=14-16, localeCompare 13, Intl 3 |
| BI-ERROR | builtin | Error hierarchy, message/cause, AggregateError, `Error.prototype.toString` | P0 | Error R=170, `.stack` R=158 |
| BI-ERROR-V8 | builtin | Error.captureStackTrace / stackTraceLimit / prepareStackTrace | P1 | R=2/2/2; wasm-bindgen sets stackTraceLimit |
| BI-GLOBALTHIS | builtin | `globalThis` and host alias `global` | P0 | global R=184, globalThis 12 |
| BI-URI | builtin | encodeURIComponent/decodeURIComponent/encodeURI/decodeURI | P1 | R=18/8; obfuscator decoder |
| BI-ANNEXB-GLOBALS | builtin | escape/unescape | P2 | R=3/2 |
| BI-WASM | builtin | WebAssembly sync API (Module/Instance/Memory/Table, errors, imports/exports) | P1 | R=11 + 4 repos ship .wasm; Rust/.NET bots need it |
| BI-SHAREDMEM | builtin | Atomics exists in V8. **Krebs:** no SharedArrayBuffer global, no `Atomics.waitAsync`, and `Atomics.wait` throws (§5.15) | P2 | R=1 |
| BI-ES2024PLUS | builtin | groupBy, Set methods, iterator helpers, Promise.withResolvers, Array.fromAsync. **Krebs:** no resizable buffers and no `transfer` (§5.15) | P2 | V8 12.x ships them; R≈0 |
| **OBS — observable V8 behaviours** |||||
| OBS-STACK-FORMAT | observable | `err.stack` = header + `\n    at …` frames; file = module name | P0 | `.stack` R=158; ErrorMapper regex R=80; source maps R=89 |
| OBS-STACK-NAMES | observable | Frame naming: `Type.method [as x]`, `new X`, `get x`, `async f`, `eval at`, `<anonymous>` | P0 | [probe] §6.1 |
| OBS-STACK-LIMIT | observable | `Error.stackTraceLimit` default 10; lazy formatting; header at first access | P0 | [probe] |
| OBS-STACK-TIMEOUT | observable | Timeout error's `stack` is rendered by isolated-vm (different format, truncates at anonymous frames) | P1 | [ivm] isolate/stack_trace.cc:79-195; run_with_timeout.h:52 |
| OBS-ERROR-MESSAGES | observable | V8 message texts (TypeError/RangeError/SyntaxError templates) | P1 | console output parity |
| OBS-ERROR-TOSTRING | observable | `String(err)`, `name`/`message` defaults, own props `stack`,`message` | P0 | console.log uses `toString()` |
| OBS-FN-TOSTRING | observable | Exact source slice; natives `function f() { [native code] }`; `Function` ctor text | P0 | lodash isNative; obfuscator self-defence; R=15 inspect toString |
| OBS-TOSTRINGTAG | observable | `Object.prototype.toString` tags incl. AsyncFunction/GeneratorFunction | P0 | lodash type checks |
| OBS-PROP-ORDER | observable | OwnPropertyKeys order: array indices ascending, then strings by insertion, then symbols | P0 | every for-in/keys/JSON |
| OBS-FOR-IN-ORDER | observable | for-in = own keys order, then each prototype's, de-duplicated, enumerable only | P0 | R=290; engine toJSON |
| OBS-JSON-FORMAT | observable | Exact JSON text (number format, escapes, indent) | P0 | Memory string bytes |
| OBS-DATE-TZ-LOCALE | observable | Local TZ & default locale of the host process | P1 | getHours R=11; toLocaleString R=16 |
| OBS-STRING-UTF16 | observable | Strings = UTF-16 code units incl. lone surrogates; `.length` limits | P0 | Memory 2 MB/segment 100 KB by `.length` |
| OBS-ARGUMENTS | observable | Mapped (sloppy) / unmapped (strict) arguments; own `callee` | P0 | R=70; lodash isArguments |
| OBS-FN-OWN-PROPS | observable | Own props of functions (`length`,`name`,`prototype`), `.caller`/`.arguments` | P1 | screeps-profiler copies own props |
| OBS-TYPEOF | observable | typeof results (`document`→undefined, class→function) | P0 | env sniffing |
| OBS-REGEX-BACKTRACK | observable | Backtracking regex semantics incl. catastrophic patterns | P0 | Overmind self-defending `(((.+)+)+)+$` |
| OBS-HOST-FIDELITY | observable | Game API objects must look like ordinary JS (descriptors, toString, prototype chains) | P0 | profilers wrap prototypes |
| **LD — lodash 3.10.1 engine touch-points** |||||
| LD-INSTANCE | lodash | Player `_` = `runInContext()` of runtime lodash 3.10.1, created once per VM | P0 | engine/src/game/game.js:475-477; `_.` R=250 |
| LD-ROOT | lodash | Root/global detection (`global`, `self`, `window`, `this`) | P0 | lodash-3.10.1/lodash.js:250-274 |
| LD-ISNATIVE | lodash | `reIsNative` built from `Function.prototype.toString` of natives | P0 | lodash.js:723,741-744,3942-3945,8833-8840 |
| LD-ISFUNCTION | lodash | `isFunction` via `[object Function]` tag → async/generator fns are NOT functions | P0 | lodash.js:8709-8713; game.js:506,520 |
| LD-TIMERS | lodash | debounce/throttle/delay/defer need setTimeout → TypeError in sandbox | P1 | lodash.js:1773,7709 |
| LD-TEMPLATE | lodash | `_.template` uses `Function(arrayOfNames, src)` + `//# sourceURL` | P2 | lodash.js:10745-10809; R=2 |
| LD-RANDOM | lodash | random/shuffle/sample use the captured `Math.random`; `_.random` float path uses Number→String length | P1 | lodash.js:769,2501-2503,10193-10196; R=24/18/16 |
| LD-KEYS | lodash | `_.keys` = native keys or shimKeys (array-likes, prototypes) | P0 | lodash.js:4369-4387,9671-9678 |
| LD-SORTBY | lodash | sortBy/sortByOrder stable via index tie-break over native sort | P0 | lodash.js:425-460,2602-2611; R=56 |
| LD-ISARGUMENTS | lodash | Needs own non-enumerable `callee` on arguments objects | P0 | lodash.js:8468-8471 |
| LD-CAPTURED | lodash | Natives captured at `runInContext` time (before player code) | P1 | lodash.js:747-769 |
| **PERF — performance-relevant structure** |||||
| PERF-SCALE-HEAPS | perf | ~1000 persistent player heaps; per-heap baseline footprint | P0 | user-vm.js:30 |
| PERF-VM-CREATE | perf | Cheap realm+runtime instantiation (snapshot / CoW shared builtins) | P0 | make-runtime-snapshot.js; user-vm.js:30-96 |
| PERF-COMPILE | perf | Lazy compile of 1–1.6 MB single-module bundles on global reset (billed) | P0 | hivemind dist 1.56 MB; International 1.23 MB |
| PERF-CPU-METER | perf | CPU accounting granularity and V8-comparable costs | P0 | Game.cpu.getUsed R=154 |
| PERF-PROP-ACCESS | perf | Inline-cached property access, prototype getters with `_x` instance caches | P0 | engine/src/utils.js:509-544; class getters R=114 |
| PERF-PROXY-STORE | perf | `store[...]` goes through Proxy get trap on every access | P0 | engine/src/game/store.js:87-96 |
| PERF-ROOMPOS | perf | RoomPosition: int-packed own data prop + prototype accessors | P0 | engine/src/game/rooms.js:1276-1347; `new RoomPosition` R=178 |
| PERF-ENUM | perf | for-in/Object.keys over Game.creeps/Memory (enum caches) | P0 | for-in R=290 |
| PERF-CLOSURES | perf | Closure-heavy lodash/Array callbacks | P0 | `_.filter` R=159, `.filter` 258 |
| PERF-JSON | perf | JSON.parse/stringify of up to 2 MB per player per tick | P0 | runtime.js:247; game.js:485 |
| PERF-ARRAYS | perf | Dense arrays, for-of over arrays without iterator allocation | P0 | for-of R=217 |
| PERF-MAPSET | perf | Hash maps for Map/Set with insertion order | P1 | R=81/74 |
| PERF-STRINGS | perf | String building (ropes), RoomVisual JSON, console logs | P1 | RoomVisual R=88 |
| PERF-TYPEDARRAY | perf | CostMatrix clone/serialize, terrain buffers | P0 | `new CostMatrix` R=105 |
| PERF-EXCEPTIONS | perf | try/catch per creep/room; stack capture cost | P1 | `safe()` wrappers; ErrorMapper |
| PERF-GC | perf | Per-heap generational GC, no cross-player pauses | P0 | — |
| PERF-WASM | perf | Sync compile of 0.1–0.3 MB wasm on reset; near-native execution | P1 | International 105 KB; Ytirity 255 KB |
| PERF-SOURCEMAP | perf | First-error source-map parse (≈1 MB JSON + VLQ) | P1 | hivemind ErrorMapper ">30 CPU" comment |

**Inventory count: 138 items.**

---

## 2. Engine era and hosting contract (ENV-*)

### 2.1 ENV-TARGET-V8: which V8/ECMAScript to emulate

* The driver depends on `isolated-vm` at git commit `cb93efcc…` (`reference/driver/package.json:11`). That resolves to **isolated-vm 6.1.2**, whose `engines` field is `node >= 22.0.0` (`reference/driver/package-lock.json:2589-2601`).
* The standalone `screeps` 4.3.0 package installed at `E:/screeps/server` declares `"node": ">=22.9.0"`.
* The launcher GUI uses Electron 40.8.1 (`reference/launcher/package.json:34`). That is not the sandbox.
* xxscreeps targets `node >=20` with isolated-vm 6.0.2 (`reference/xxscreeps/packages/xxscreeps/package.json:12-13,36`).
* `reference/docs/source/architecture.md:8` still says "Node.js 8.9.3" and describes `vm.runInContext` (`:48`). **This is obsolete** and contradicted by the driver code.
* **Rule**: emulate the **V8 shipped in Node 22 LTS (V8 12.4)** as the floor. Language level = **ES2024 complete** plus V8-shipped Stage-4 items of that era. ES2025+ features shipped by later Node (24: V8 13.6) are P2.
* **GOTCHA**: the private-server operator chooses the Node version, so V8 version-dependent details (error message texts, `stack` descriptor shape) vary in the wild. chitin pins one (§10 OQ-1).

### 2.2 ENV-ONE-REALM: runtime and player share one realm

* The runtime bundle (webpack 3 build of `driver/lib/runtime/runtime.js` plus `engine/src/game/*` and lodash; `reference/driver/webpack.config.js:6`) and all player modules run in the **same V8 context**:
  * `game.init(global, …)` uses the context global as `globals` (`reference/driver/lib/runtime/runtime.js:216-218`);
  * `global` is set to the context global (`reference/driver/lib/runtime/user-vm.js:59`).
* Consequences the engine must reproduce:
  * **GOTCHA**: player mutations of builtins change engine API behaviour. Examples: patching `Array.prototype`, adding enumerable props to `Object.prototype`, replacing `JSON.parse` or `Math.random`.
    * Memory is parsed with the *global* `JSON.parse`, looked up at call time (`reference/engine/src/game/game.js:485`).
    * Creep auto-names use the global `Math.random` (`reference/engine/src/game/names.js:9-13`).
    * Only output marshalling uses frozen captured copies: `systemFunctions` (`reference/driver/lib/runtime/system-functions.js:1-28`) for the end-of-tick Memory stringify (`runtime.js:247`), and game.js's own captured `jsonStringify/jsonParse` (`game.js:12-13`) for constants/resources.
  * Engine prototypes (`Creep`, `RoomPosition`, …) are ordinary JS functions in that realm. The globals are defined with `Object.defineProperty(globals,'X',{enumerable:true,value:X})`, so they are non-writable and non-configurable (02 GLB-CLASS-DESCRIPTORS). Their prototypes are writable: R=93 repos patch `Creep.prototype`, 63 `Room.prototype`, 39 `RoomPosition.prototype`.
  * chitin may implement the API natively, but OBS-HOST-FIDELITY (§6.9) applies.

### 2.3 ENV-PERSISTENT-HEAP, ENV-HEAP-LIMIT, ENV-HEAP-STATS

* One isolate per player is reused across ticks. Reset rules are in 02 VM-* (`user-vm.js:13-101`). The engine needs:
  * heap objects (closures, generators, Maps, typed arrays, compiled code) survive between host calls indefinitely;
  * the heap can be dropped atomically ("global reset").
* **Limit**: `memoryLimit: 256 + staticTerrainDataSize/1024/1024` MB (`user-vm.js:30`).
  * isolated-vm checks after each mark-sweep: if `used_heap_size + extra_allocated_memory > limit + misc`, it first forces a critical GC, then terminates (`[ivm] isolate/environment.cc` `MarkSweepCompactEpilogue`, around `:184-200`).
  * The resulting errors (`Isolate has exhausted v8 heap space`, `Array buffer allocation failed`) clear the VM (`reference/driver/lib/runtime/make.js:285-292`).
* **`Game.cpu.getHeapStatistics()`** returns `isolate.getHeapStatisticsSync()` (`runtime.js:228-230`). The object has exactly these own enumerable data properties, **in this order** (`[ivm] module/isolate_handle.cc:366-375`):

  | Order | Field | Value |
  |---|---|---|
  | 1 | `total_heap_size` | |
  | 2 | `total_heap_size_executable` | |
  | 3 | `total_physical_size` | |
  | 4 | `total_available_size` | V8 value − `adjustment` |
  | 5 | `used_heap_size` | |
  | 6 | `heap_size_limit` | V8 value − `adjustment` |
  | 7 | `malloced_memory` | |
  | 8 | `peak_malloced_memory` | |
  | 9 | `does_zap_garbage` | 0/1 |
  | 10 | `externally_allocated_size` | isolate's extra allocated memory (ArrayBuffers, wasm memory) |

  `adjustment = heap_size_limit − initialHeapSizeLimit` (`:357`). The docs' sample values are at `reference/docs/api/source/Game.md:200-212`.
* **Bot dependence**:
  * R=31 repos read it, 25 read the field names.
  * Overmind: `(total_heap_size + externally_allocated_size) / heap_size_limit` (`Overmind-dev/src/utilities/utils.ts:279-296`).
  * International: `total_heap_size / heap_size_limit` (`bots/full-bot/The-International-…/src/international/commands.ts:313`).
  * Bots call `Game.cpu.halt()` when the ratio is high (R=12 call halt).
* **Rule**: chitin must keep a per-player byte accounting:
  * `total_heap_size` ≈ live + fragmentation bytes;
  * `externally_allocated_size` = backing stores (typed arrays, wasm memories);
  * `heap_size_limit` = configured limit.
  * It must kill the VM on overflow.
* **GOTCHA**: `typeof Game.cpu.getHeapStatistics === 'function'` is used as an "IVM is enabled" probe (`utils.ts:294-296`).

### 2.4 ENV-GLOBALS-PRESENT

The player's global object contains:

1. **Every V8 builtin of the target version, as a fresh context gets it, less what §5.15 takes out in Krebs.** This includes:
   * `Object`…`Proxy`, `Reflect`, `Atomics`;
   * `WeakRef`, `FinalizationRegistry`, `AggregateError`, `Iterator`;
   * `WebAssembly`;
   * `Intl` (Node is built with full ICU);
   * Annex B `escape`/`unescape`;
   * `globalThis`.
   * The V8 `console` builtin is then **overwritten**.
2. **Host additions**:
   * `global` (`user-vm.js:59`);
   * engine globals (02 GLB-LIST): `Game`, `Memory` (accessor), `RawMemory`, `console`, `_`, `require`, game classes, all constants re-copied each tick;
   * the leaked `__module` and `module` (02 GLB-LEAKS).
3. **Bootstrap globals deleted before player code**: `_ivm`, `_isolate`, `_context`, `_init`, `_evalFn`, `_start`, `_setStaticTerrainData`, `_worldSize`, `_nativeMod`, `_constants`, `_halt`, `_customObjectPrototypes`, `_customIntentTypes` (`user-vm.js:42-56`).

* `gc` is **absent**: no `--expose-gc`. Overmind guards it with `if (global.gc)` (`Overmind-dev/src/memory/Memory.ts:117`).

### 2.5 ENV-GLOBALS-ABSENT (GOTCHA: providing them changes bot behaviour)

* isolated-vm contexts expose only V8 builtins. Absent:
  * `setTimeout`/`setInterval`/`setImmediate`/`clearTimeout`, `queueMicrotask`;
  * `structuredClone`;
  * `TextEncoder`/`TextDecoder`, `atob`/`btoa`;
  * `performance`, `Buffer`, `process`, `URL`, `crypto`;
  * `self`/`window`/`navigator`, `fetch`.
* Bots detect absence and polyfill, so chitin must keep them **absent by default**. A mod may add them.
* Evidence of reliance:
  * **wasm-bindgen glue**: `typeof TextEncoder !== 'undefined' ? new TextEncoder('utf-8') : {encode: () => { throw … }}` (`bots/full-bot/The-International-…/src/wasm/pkg/commiebot_wasm.js:36,102`), and the bot installs a polyfill first (`src/other/wasmInit.ts:3-4`). The Rust starter does the same (`bots/starter/rustyscreeps__screeps-starter-rust/js_src/main.js:2`).
  * **ScreepsDotNet**: `global.TextDecoder = global.TextDecoder || TextDecoder` (`bots/discovered/thomasfn__ScreepsDotNet/Bootloader/src/polyfill-textencoder.js:3-4`).
  * **Async starter**: ships its own `setTimeout`/`setInterval`/Promise polyfills (`bots/starter/Bunabun__screeps-async-typescript-starter/src/polyfills/*`, README `:53-55`).
  * **lodash**: timer functions throw `TypeError: setTimeout is not a function` ([probe], LD-TIMERS).
  * **Profilers**: use `performance.now()` only when `Game.rooms.sim` (browser simulator) (`Overmind-dev/src/profiler/screeps-profiler.js:93-99`).

### 2.6 ENV-CONSOLE-REPLACED

* `console` is replaced every tick by `Object.create(null, {log, logUnsafe, commandResult, addVisual, getVisualSize, clearVisual, getVisual})` (`reference/engine/src/game/console.js:10-96`, `game.js:470-473`).
* **GOTCHA**: `console.error`, `console.warn`, `console.info` and `console.debug` are `undefined`, so calling them throws `TypeError: console.error is not a function`.
  * R=58 repos call one of them, usually in tooling or guarded.
  * The Rust starter re-installs `console.error` **every tick**: "need to freshly override the fake console object each tick" (`bots/starter/rustyscreeps__screeps-starter-rust/js_src/main.js:9-26,32,68`).
* The `console.log` argument formatting is a runtime rule (02 CON-*). It depends on these engine semantics:
  * `i && i.toString` truthiness;
  * `toString` of every builtin;
  * `JSON.stringify` for falsy values. So `console.log(NaN)` prints `null` and `console.log('')` prints `""` (`console.js:21-25`).

### 2.7 ENV-SCRIPT-ORIGIN, ENV-SLOPPY-DEFAULT, ENV-ESM-NONE (→02 MOD-WRAPPER)

* **Module script**: the source text is `(function __module(module,exports){ ` + code + `\n})(__module, __module.exports)`, compiled with `{filename: <moduleName>}` (`reference/driver/lib/runtime/runtime-driver.js:23,67`).
* Engine requirements that follow:
  * **Script name** is the bare module name (`main`, `role.harvester`, `main.js.map`). It appears verbatim in stack frames.
  * **Line numbers** are unchanged. **Columns on line 1 are +36** (the prefix length). This matters for minified single-line bundles, because error mappers take V8 columns as-is.
  * The code is a **FunctionBody**, which means:
    * a leading `'use strict'` (comments allowed before it) is that function's directive and makes the module strict;
    * top-level `var`/`function`/`let` are function-local (not globals);
    * an undeclared assignment in sloppy mode creates a **global property** that persists across ticks;
    * top-level `this` is the global object (sloppy) or `undefined` (strict);
    * top-level `return` is legal;
    * `import`/`export`, `import.meta`, top-level `await` and a leading `#!` are **SyntaxErrors**.
  * Dynamic `import()` parses, but there is no host module loader. Its exact failure is a §10 OQ. R=4 TS repos use it, all down-levelled by bundlers.
  * The trailing `\n` makes a final `//` comment safe.
* **`__mainLoop` script**: code `module.exports.loop();`. The loop call site is `__mainLoop:1:52`, and the IIFE call is `__mainLoop:2:3` ([probe] §6.1).
* **Console commands**: `(function(code,module,exports) { return "" + eval(code); })(<JSON-quoted code>, __module, __module.exports)`, a **direct eval in a sloppy function** (`runtime-driver.js:59`). So:
  * `var` in a console command is local to that invocation;
  * an undeclared assignment creates a global;
  * the result is converted with `"" + value`, which calls ToPrimitive and **throws** for Symbols.
  * Script name: `_console<ms>_<i>` (`game.js:540`).
* Corpus: R=90 repos have files starting with `'use strict'` (js 80, dist 14). rollup/tsc CJS output always starts with it. A large share of hand-written JS bots run **sloppy**.
* **ESM**: the official server has no ESM. xxscreeps supports `main.mjs`/`main.wasm` ESM entry points (`reference/xxscreeps/packages/xxscreeps/driver/runtime/module.ts:12-30,32-206`); that is a mod-level option for chitin. R=35 JS repos contain `import` syntax in *source* files, all bundled before upload.

### 2.8 ENV-CODEGEN

* isolated-vm creates contexts with `AllowCodeGenerationFromStrings(false)` and installs `ModifyCodeGenerationFromStringsCallback`. The callback allows codegen unless `source.length > memory_limit/8`: about 33.5 M chars at 256 MB (`[ivm] isolate/environment.cc:169-182,392,483`).
* So `eval`, `new Function`, `Function(...)`, `setTimeout(string)` (moot) and `_.template` all work.
* Users:
  * the runtime itself: `utils.defineGameObjectProperties` builds getters via direct `eval` (`reference/engine/src/utils.js:516-532`); custom object prototypes are `eval`ed (`runtime.js:34-44,51-53`);
  * the webpack 3 `global` shim (`[bundle] m6:15134-15143`) contains `Function("return this")()`, but only as a fallback. It first runs the **sloppy** `g = (function() { return this; })()`, which already yields the global object, so `g || Function(...)` short-circuits and no codegen happens. This shim needs sloppy mode (ENV-ENGINE-STRICTNESS);
  * bots, R=14 direct eval. Examples:
    * `eval(flagname)` reading local scope (`bots/full-bot/YektaLeblebici__screeps/buildernew.js:30`);
    * remote debugger `eval(cmd)` (`Overmind-dev/src/debug/remoteDebugger.ts:73`);
    * wasm-bindgen `eval(getStringFromWasm0(...))` (`bots/full-bot/The-International-…/src/wasm/pkg/commiebot_wasm.js:452`);
    * `new Function(opts.roomCallback)` (`bots/full-bot/AlinaNova21__ZeSwarm/src/GenState/actions.js:48`).
* chitin needs a full parser and compiler at runtime. It must emit V8's `Function`-constructor source text (OBS-FN-TOSTRING) and V8's eval frame names (OBS-STACK-NAMES).

### 2.9 ENV-TERMINATION (CPU limit) (→02 MOD-TIMEOUT)

* Every `evalCode` (each module's first evaluation, and the loop call) runs with `timeout = max(30, cpu + 5)` ms. There is no timeout when `cpu` is Infinity/0/undefined, or for console commands (`runtime-driver.js:40-45,58-61`).
* isolated-vm mechanics (`[ivm] isolate/run_with_timeout.h`):
  1. A timer thread (steady clock, pausable) fires. It queues an interrupt that captures `CurrentStackTrace(isolate, 10)` and calls `TerminateExecution()` (`:39-56`).
  2. Termination is an **uncatchable** unwinding. User `catch` and `finally` blocks **do not run**. Generators and iterators are not closed.
  3. When the terminated `runSync` returns, the timeout path calls `CancelTerminateExecution()` once `terminate_depth` reaches 0, then throws `RuntimeGenericError("Script execution timed out.", stack)` (`:158-166`).
     * The error is a **catchable** exception at the `runSync` call site, i.e. inside the runtime's `evalCode`.
     * evalCode rewrites the message to `Script execution timed out: CPU time limit reached` (`runtime-driver.js:105-107`).
* **Consequence (1:1)**: everything done before the interrupt stays:
  * intents already queued;
  * Memory object mutations, which are still serialized (`runtime.js:246-248`; 02 §13);
  * global heap mutations.

  Half-finished data structures in the global heap therefore persist into the next tick. **GOTCHA**: bots that rely on `finally` for cleanup (locks, `depth--` in profilers such as `bots/tool/screepers__screeps-profiler/screeps-profiler.js:113-131`) keep inconsistent state after a timeout. chitin's interrupt must be:
  * observable only as an abrupt stop;
  * delivered at any loop back-edge or call;
  * also delivered inside long-running builtins (sort, JSON, regex), as V8 interrupt checks are.
* Other terminations:
  * **halt**: `Game.cpu.halt()` disposes the isolate synchronously and the tick is discarded (`runtime.js:20-23`, `make.js:280-284`).
  * **Heap limit**: same handling (§2.3).
* **PERF**: interrupt checks must be cheap. A counter or deadline poll at back-edges and function entries is enough; it doubles as the CPU meter (PERF-CPU-METER).

### 2.10 ENV-MICROTASKS (resolves 02 OQ-4)

* V8's default `MicrotasksPolicy` is kAuto; isolated-vm never changes it (no `SetMicrotasksPolicy` in [ivm]).
* isolated-vm runs `PerformMicrotaskCheckpoint()` in `TaskEpilogue()`, after the async task's Phase2 (`[ivm] isolate/three_phase_task.cc:182`, `environment.cc:490-501`).
* **Recursive (same-isolate) `runSync` calls skip the epilogue**: `if (!is_recursive) error = TaskEpilogue()` (`three_phase_task.cc:225-258`). The runtime's `evalCode` uses exactly such recursive calls (`runtime.js:30-32`).
* Therefore, in one player tick:
  1. Host calls `run.apply(...)` (`make.js:187`). This invokes the `ivm.Reference` function (`runtime.js:233`).
  2. User code runs. Each `.then` callback or `await` continuation is queued, **not run**.
  3. The function builds `outMessage`: memory string, intents, console output (`runtime.js:246-296`). `new ivm.ExternalCopy(outMessage)` serializes at construction (`:296`).
  4. The function returns. With kAuto, microtasks may run when the call depth reaches 0. In any case the epilogue checkpoint drains them, **after the output was captured**.
* Effects:
  * Continuations after the first `await` cannot affect this tick's intents, Memory or console. `console.log` writes into `messages[id]`, which `makeConsole` resets at the next tick's `_start` (`console.js:7`), so those logs are lost.
  * Intents issued in a continuation are pushed to the previous tick's `intents` object, so they are lost too.
  * Global-heap mutations persist. Memory mutations persist only if the bot reuses the Memory object (memhack, R=32-38).
* **Unhandled rejection = failed tick**. `PromiseRejectCallback` records rejections without a handler, and `TaskEpilogue` returns the first one still unhandled as the task's error (`environment.cc:148-159,496-501`).
  * The host-side `run.apply` promise rejects. `make.js` catches it and throws `{error: error.stack || error}` (`make.js:279-294`), so **that tick's intents, Memory and segments are discarded**.
  * **GOTCHA**: the handles are weak (`SetWeak`, `:155`), so GC timing could in theory hide a rejection.
* **Rule for chitin**:
  * one job queue per player realm;
  * drain it after output capture;
  * then, if any rejection remains unhandled, mark the run failed.
  * Never drain at nested script-evaluation boundaries.
* Corpus: in-game Promise use is rare. R=11 repos reference `Promise`; the cases found are tooling, wasm-bindgen glue, or polyfilled tick-schedulers. The async starter compiles async/await down to generators plus its own `PromisePoly` for exactly this reason. Hence P1.

### 2.11 ENV-CPU-CLOCK

* `nowCpuTime() = Number(isolate.cpuTime)/1e6` ms (`runtime.js:25-27`).
* `isolate.cpuTime` = accumulated `CLOCK_THREAD_CPUTIME_ID` time of finished execution segments, plus `steady_clock::now() − segment_start` for the running segment (`[ivm] isolate/executor.cc:47-104`, `environment.cc:529-535`). It includes compile time (scripts are compiled inside the isolate on first `require`) and GC time on that thread.
* `Game.cpu.getUsed()` = `nowCpuTime() + intents.cpu − startTime` (`runtime.js:202-204`). It is called extremely often: R=154 repos, profilers call it twice per wrapped function call.
* **Rule**: provide a monotonic per-player CPU clock in ms (float) with sub-µs cost per read. Determinism is not required for parity; PERF-CPU-METER covers calibration.

### 2.12 ENV-SNAPSHOT

* The runtime bundle is turned into a V8 startup snapshot with script name `<runtime>` (`reference/driver/make-runtime-snapshot.js:5`). Each new isolate boots from it (`user-vm.js:30`), falling back to compiling the bundle (`:34-37`).
* Engine-visible effects:
  * runtime frames in stacks say `(<runtime>:L:C)`; without a snapshot, isolated-vm's default name is `<isolated-vm>` (`[ivm] module/evaluation.h:23`);
  * VM creation is cheap.
* chitin equivalent: immutable shared builtins and runtime code, plus per-player copy-on-write state (PERF-VM-CREATE).

### 2.13 ENV-BINARY-BUFFER

* A binary module's `require` value is `driver.bufferFromBase64(b64)` = `Buffer.from(b64,'base64')` (`runtime-driver.js:13-15`, `engine/src/game/game.js:561-562`).
* `Buffer` is webpack 3's `node-libs-browser` polyfill **buffer@4.9.1** (`reference/driver/package-lock.json:405-406`). With typed-array support, that is a `Uint8Array` instance whose prototype chain is `Buffer.prototype → Uint8Array.prototype`.
* Engine needs:
  * `new WebAssembly.Module(buf)` accepts any ArrayBufferView;
  * `instanceof Uint8Array` is true;
  * `Object.prototype.toString` gives `[object Uint8Array]`.
* The docs' comment "This will return an ArrayBuffer" (`reference/docs/source/modules.md:82`) is wrong. Bots never depend on Buffer methods (R=0 found).
* Users: R=7 `require('*wasm*'/'*_bg')`, plus Rust/.NET loaders (§5.14).

### 2.14 ENV-ENGINE-STRICTNESS: strict or sloppy mode of each runtime module (resolves 05 §16 OQ-2)

**How the bundle keeps each module's own mode**

* The runtime is a **webpack 3** bundle with entry `./lib/runtime/runtime.js` (`reference/driver/webpack.config.js:6`; `"webpack": "^3.11.0"`, `reference/driver/package.json:16`).
* Webpack 3 wraps each CommonJS module in `function(module, exports, __webpack_require__) { <source> }` and **adds no `'use strict'` of its own**. Only ES-module ("harmony") sources get one, and the runtime has none.
* A module's own leading directive therefore stays the first statement of its wrapper function. Each module keeps its own mode, and so does each function inside it.
* When webpack adds a `/* WEBPACK VAR INJECTION */(function(global){…})` wrapper, it hoists the directive **above** that wrapper, so the inner function inherits strictness (`[bundle] m18:16377-16378`).
* The built bundle has exactly 7 directives in its 66 modules (`grep "use strict"` on [bundle]). They match the source table below.

**Mode per source module**

| Mode | Source (what bots reach through it) | Evidence |
|---|---|---|
| strict | `engine/src/game/game.js`, the whole file: `Game.*` functions (`getObjectById`, `notify`, `cpu.*`, …), `requireFn`, the console-command loop | `reference/engine/src/game/game.js:1`; `[bundle] m26:20339` |
| strict | `engine/src/game/path-finder.js`: `PathFinder.search`, `PathFinder.CostMatrix` and its methods | `reference/engine/src/game/path-finder.js:1`; `[bundle] m60:27690` |
| strict | `engine/src/game/path-utils.js`: `OpenClosed`/`Heap` used by `Game.map` route search | `reference/engine/src/game/path-utils.js:1` (required at `map.js:5`); `[bundle] m28:21327` |
| strict (that function only) | the `Room.Terrain` constructor and the `get`/`getRawBuffer` closures nested in it. The rest of `rooms.js` is sloppy | `reference/engine/src/game/rooms.js:1223-1241`; `[bundle] m2:14369` |
| strict | `driver/lib/path-finder.js`: the native `PathFinder` glue, including the `roomCallback` trampoline | `reference/driver/lib/path-finder.js:1,115-129` (required at `runtime-driver.js:124`); `[bundle] m24:20175` |
| strict | the webpack polyfills `buffer@4.9.1` and `base64-js` (ENV-BINARY-BUFFER) | `[bundle] m18:16377`, `m19:18174` |
| **sloppy** | every other `engine/src/game/*.js`, including `creeps.js`, `structures.js`, `market.js`, `power-creeps.js`, `store.js`, `flags.js`, `map.js`, `console.js`, `construction-sites.js`, and the rest of `rooms.js` (`Room`, `RoomPosition`, `RoomVisual`, …) | no directive in any of these files |
| **sloppy** | `engine/src/utils.js`, including the `eval`-built lazy getters. A direct eval inherits the caller's mode, so these getters are sloppy too | `reference/engine/src/utils.js:516-532` |
| **sloppy** | `driver/lib/runtime/runtime.js`, `runtime-driver.js`, `system-functions.js`, `mapgrid.js` | no directive; `[bundle] m13`, `m9`, `m17` |
| **sloppy** | lodash 3.10.1, `@screeps/common` `constants`/`system`, `@screeps/pathfinding`, and the webpack `global`, `process` and `util` shims | no directive; `[bundle] m1`, `m23`, `m22`, `m6`, `m14`, `m30` |

* No sloppy module contains a `class` body, so no class-strict islands exist outside `path-utils.js` (grep `class ` over these sources).
* `register.wrapFn` is the identity function: the runtime passes `undefined` as `_sandboxedFunctionWrapper` (`runtime.js:216-231`, `game.js:449,468,74`). So every API function **is** the raw function from its source module, with that module's mode. The console's `makeConsole` wrapper is also the identity (`runtime.js:222`).

**Observable consequences (all P0 for 1:1)**

1. **Detached or receiver-less calls get `this === globalThis`** in sloppy API functions (ECMA-262 OrdinaryCallBindThis), not `undefined`. Examples:
   * `const {createOrder} = Game.market; createOrder(ORDER_SELL, RESOURCE_ENERGY, 0.1, 1000, 'W1N1')`:
     * `price * totalAmount * MARKET_FEE > this.credits` compares against `globalThis.credits`. That is normally `undefined`, so the comparison is false and the credit check **passes** (`market.js:83`);
     * `_.size(this.orders)` is `_.size(globalThis.orders)`, normally `_.size(undefined) === 0`, so the `MARKET_MAX_ORDERS` check passes (`market.js:90`);
     * the terminal-ownership check does not use `this` and still applies (`market.js:86-89`);
     * the call pushes the `createOrder` intent and returns `OK` (`market.js:93-97`). A strict engine would throw a TypeError here instead.
     * **GOTCHA**: a bot that has its own global variables named `credits` or `orders` changes the outcome, because those are read instead.
   * Detached `cancelOrder`, `changeOrderPrice`, `extendOrder` and `getOrderById` throw `TypeError: Cannot read properties of undefined (reading '<orderId>')` at `this.orders[id]` (`market.js:101,156,175,56`). **GOTCHA**: the message names the **order id**, not `'orders'` (a strict engine would say `reading 'orders'`) ([probe]).
   * Detached `deal` (`market.js:108-148`) uses `this` only for non-intershard resources:
     * for intershard resources it works normally;
     * otherwise, after the order, amount and `targetRoomName` checks return their error codes, it throws `TypeError: this.calcTransactionCost is not a function` (`market.js:127`, [probe]).
   * Detached `Game.map.findExit` throws `TypeError: this.findRoute is not a function` (`map.js:188-189`).
   * **Strict counterparts**:
     * `Room.Terrain('W1N1')` without `new`: for an unknown room it first throws `Could not access room …` (`rooms.js:1226-1228`); for a known room it throws `TypeError: Cannot set properties of undefined (setting 'get')` (`rooms.js:1230`). Sloppy code would instead have put `get`/`getRawBuffer` on the global object and returned `undefined`.
     * `PathFinder.CostMatrix()` without `new` throws `TypeError: Cannot set properties of undefined (setting '_bits')` (`path-finder.js:18-19`).
2. **Primitive receivers are boxed** in sloppy API functions (`typeof this === 'object'` for `fn.call(5)`), while `undefined` and `null` become the global object. Strict functions (table above) receive the value unchanged ([probe]: `B.call(5)` gives `object` (sloppy) vs `number` (strict)).
3. **Failed writes and deletes inside sloppy engine code are silent.** `[[Set]]` returning false (target non-writable, getter-only, frozen or non-extensible) is ignored, and so is a failed `delete`. Strict code throws a TypeError. Example:
   * the lazy getters from `defineGameObjectProperties` cache with `this['_<name>'] = …` and return `this['_<name>']` (`utils.js:520-524`);
   * on a frozen game object (`Object.freeze(creep)`) whose cache is still empty, the write fails silently and the getter returns `undefined` instead of throwing.
4. **`Function.prototype.caller`/`.arguments`**:
   * on a **strict** engine function (`PathFinder.search`, `Game.getObjectById`, `Room.Terrain`) they throw `TypeError: 'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions…`;
   * on a **sloppy** one (`Game.market.createOrder`, `Creep.prototype.move`) they return `null` when the function is not on the stack.
   * Seen from a user sloppy callback, `cb.caller` is `null` when a strict engine function or a V8 builtin invoked it (e.g. `roomCallback`, invoked from `driver/lib/path-finder.js:122`). When sloppy engine JS invoked it, `cb.caller` is that engine function object (e.g. lodash's loop for `room.find(type, {filter})`, `rooms.js:648-649`) ([probe]).
   * **GOTCHA**: V8 builtins behave like *strict* functions here ([probe]: `Math.max.caller` throws). So a chitin API function implemented "natively" without a sloppy flag would throw where the original returns `null`.
   * Corpus: no bot reads `Function.prototype.caller`. Every `.caller` hit is the `pos.caller` field of a source-map logger (e.g. `bots/full-bot/bencbartlett__Overmind/src/console/log.ts:88`). So point 4 is **P2** within this P0 item; points 1–3 are P0.

**Rule for chitin**

* Every native (Rust-implemented) API function carries a `strict: bool` copied from its source module per the table above. It is never a blanket default.
* At call entry, a sloppy native applies OrdinaryCallBindThis: `undefined`/`null` become the player realm's global object, and primitives go through `ToObject`. A strict native leaves `this` unchanged.
* Every `this`-dependent read in a native goes through ordinary `[[Get]]` on that coerced receiver, so user-defined globals such as `credits` leak in exactly as in the original.
* Every JS-visible `[[Set]]`/`[[Delete]]` that a native performs uses the flag: sloppy ignores a `false` result, strict throws a TypeError with the V8 message (OBS-ERROR-MESSAGES).
* `.caller`/`.arguments` on the function object follow the flag (point 4). They are not the builtin default.
* Natives that call user callbacks must reproduce point 4: `cb.caller` is `null` under a strict engine caller and the engine function under a sloppy one.
* **PERF**: the flag is a per-function constant checked once at call entry. It costs nothing on the hot paths (`Creep.prototype.*` and `Room.prototype.find` are sloppy; `PathFinder.search` and `CostMatrix` are strict).

---

## 3. Syntax requirements (SYN-*)

Parse and execute per ECMA-262 (2024). Details that are easy to get wrong:

### 3.1 SYN-STRICT / SYN-ANNEXB

* Implement both modes per spec, including these sloppy-only behaviours:
  * implicit globals;
  * `this` coercion to the global object in plain calls;
  * mapped `arguments` (parameter aliasing);
  * silent failure of writes to non-writable or getter-only properties (strict: TypeError);
  * `delete` of non-configurable properties returns false (strict: TypeError);
  * `arguments.callee` allowed (strict: TypeError on access).
* **Annex B (sloppy only)**:
  * **B.3.2 block-level function declaration semantics**: R=10, e.g. `bots/full-bot/Tyrant7__Screeps`, which has 6 files;
  * `__proto__: v` in object literals sets the prototype (R=3), and duplicate `__proto__` is a SyntaxError;
  * legacy octal literals and escapes: none found;
  * HTML-like comments: none found.
* **GOTCHA**: engine game code is mixed strict and sloppy, and the webpack 3 bundle keeps each module's own mode.
  * Strict: `game.js`, `game/path-finder.js`, `game/path-utils.js`, the `Room.Terrain` function and `driver/lib/path-finder.js` (`game.js:1`, `path-finder.js:1`, `path-utils.js:1`, `rooms.js:1223`, `driver/lib/path-finder.js:1`).
  * Sloppy: everything else, including `creeps.js`, `structures.js`, `market.js`, the rest of `rooms.js`, `utils.js` and `driver/lib/runtime/*.js`.
  * chitin's API functions must copy each module's mode. The user-visible outcomes are: detached calls see `this === globalThis`, primitive receivers are boxed, failed writes are silent, and `.caller` behaviour differs. Full table and rules: §2.14 ENV-ENGINE-STRICTNESS.

### 3.2 Classes (SYN-CLASS, SYN-ACCESSORS, SYN-CLASS-FIELDS, SYN-STATIC-BLOCK, SYN-PRIVATE)

* Class bodies are always strict. Class constructors throw when called without `new`.
* Class methods are non-enumerable. Getters and setters on prototypes are non-enumerable for class syntax, but **enumerable when defined via object literals or `Object.defineProperty` with `enumerable:true`**. Engine getters are enumerable (`engine/src/utils.js:512-520`).
* Fields:
  * field initializers use `[[DefineOwnProperty]]`;
  * each field initializer runs as its own method-like scope, with `this` = instance and `arguments` forbidden;
  * static fields and `static {}` blocks run in textual order at class definition.
* esbuild output uses `static { this.cache = {}; }` (`bots/full-bot/Mirroar__hivemind/dist/main.js:1809-1812`).
* TS decorators (R=18) compile to helper calls:
  * `__decorate`: tsc; probes `typeof Reflect.decorate`, uses `Object.getOwnPropertyDescriptor`/`defineProperty`;
  * `__decorateClass`: esbuild.

  So no decorator syntax is needed.
* `extends` of builtins (e.g. `class X extends Error`, `extends Array`) needs proper `new.target` and prototype setup. Error subclass stack headers use the instance `name` at first `stack` access ([probe]: `MyErr: q`).

### 3.3 Generators and async (SYN-GENERATORS, SYN-ASYNC, SYN-ASYNC-ITER)

* Generators are used in game code, e.g. `RoomPosition.prototype.getAllAdjacentPositions = function* () {...}` (`bots/full-bot/TooAngel__screeps/src/prototype_roomPosition.js:98-111,253`). TooAngel is published as the npm private-server bot `screeps-bot-tooangel` with `"screeps_bot": true` (`bots/full-bot/TooAngel__screeps/package.json:2,6`); hivemind sets the same flag (`hivemind/package.json:7`).
* Suspended generator objects may be stored in globals and resumed on later ticks, so frames must be heap objects.
* Async functions are allowed (P1), with the microtask timing of ENV-MICROTASKS.
* **GOTCHA (lodash)**: `module.exports.loop = async function(){}` is **never called**:
  * the runtime checks `_.isFunction(mainExports.loop)` (`game.js:506-507,520`);
  * lodash 3's `isFunction` tests `Object.prototype.toString` against `[object Function]` (`lodash.js:8709-8713`);
  * async functions report `[object AsyncFunction]` ([probe]: `_.isFunction(async fn) === false`).

  The same holds for generator functions. The `require` cache is also recreated each tick in that case (`game.js:504-512`, 02 MOD-NO-LOOP).

### 3.4 Modern operators (SYN-OPTIONAL-CHAIN, SYN-NULLISH, SYN-LOGICAL-ASSIGN, SYN-EXPONENT)

* Short-circuit chains per spec:
  * `a?.b.c()` short-circuits the whole chain;
  * `a?.b()` keeps `this = a`;
  * `delete a?.b` is allowed;
  * `??` cannot mix with `||`/`&&` without parentheses (SyntaxError).
* `x ??= y` assigns only if nullish, and evaluates `x`'s reference once.
* Native occurrences in deployed bundles:
  * Mirroar hivemind esbuild `dist/main.js`: 207 `?.`, 43 `??`;
  * International `dist/main.js`: 10 logical assignments.

### 3.5 Labels, regex literals, numeric literals

* **Labels**: labelled `continue` on loops, and labelled blocks with `break` (R=24 use labelled jumps, e.g. `break missions;` in `bots/full-bot/glitchassassin__screeps/src/Missions/Control.ts:47-60` and `break a;` in the International esbuild bundle `dist/main.js:2340`).
* **Regex literals**:
  * early errors per flag set;
  * flags `g i m s u y` (P0/P1), `d` and `v` (P2);
  * the literal creates a **new RegExp object per evaluation** (ES5+ semantics; matters for `lastIndex` with `g`/`y`).
* **Numeric literals**:
  * separators are a parse-time feature;
  * `0b`/`0o` must be supported;
  * legacy octal `012` and `08`/`09` decimal fallback are sloppy-only.

### 3.6 SYN-DIRECT-EVAL / SYN-FUNCTION-CTOR

* **Direct eval**:
  * sees the caller's lexical environment;
  * sloppy `var` declarations leak into the caller's function scope;
  * strict eval gets its own variable environment;
  * the completion value is returned;
  * `eval` aliases (`(0,eval)(s)`) are indirect, i.e. global scope.
* **`Function(p1, p2, body)`**:
  * builds the source text `function anonymous(<p1>,<p2>\n) {\n<body>\n}` ([probe]);
  * parameters are ToString'd, so an **array** argument becomes `"a,b"`, which lodash relies on (`lodash.js:10809`);
  * it is a sloppy function in global scope.

---

## 4. Cross-reference to spec 02 (runtime semantics that sit on these engine features)

| This spec | 02 IDs that consume it |
|---|---|
| ENV-SCRIPT-ORIGIN, OBS-STACK-* | MOD-WRAPPER, CON-ERR-FORMAT, JS-STACK-FORMAT |
| ENV-TERMINATION, OBS-STACK-TIMEOUT | MOD-TIMEOUT, VM-HARD-TIMEOUT, §13 save matrix; resolves 02 §16 OQ-3 |
| ENV-MICROTASKS | JS-ASYNC; resolves 02 §16 OQ-4 |
| ENV-GLOBALS-*, ENV-CONSOLE-REPLACED | GLB-LIST, GLB-ABSENT, CON-* |
| ENV-HEAP-*, ENV-PERSISTENT-HEAP | VM-ISOLATE-PER-USER, VM-HEAP-LIMIT, VM-DISPOSE-ERRORS |
| BI-JSON-*, OBS-PROP-ORDER, PERF-JSON | Memory/RawMemory items, §13 serialization |
| BI-MATH, BI-MATH-RANDOM, BI-DATE | JS-MATH, JS-DATE |
| ENV-SLOPPY-DEFAULT, SYN-STRICT | JS-STRICTNESS |
| ENV-ENGINE-STRICTNESS | JS-STRICTNESS (02 §15.6 lists only `game.js`/`path-finder.js` as strict; §2.14 is the full table); resolves 05 §16 OQ-2 |
| LD-* | GLB-LODASH, MOD-REQUIRE-LODASH, MOD-MAIN-LOOP |
| BI-WASM, ENV-BINARY-BUFFER | MOD-BINARY, MOD-WASM; answers part of 02 §16 OQ-8 |

---

## 5. Builtins (BI-*)

The default is ECMA-262 semantics. Below are the items where bots depend on **implementation-specific** or easy-to-miss behaviour.

### 5.1 BI-OBJECT / BI-REFLECT / BI-PROXY

* All Object statics are needed. Usage: keys R=188, values 120, defineProperty 96, entries 59, getOwnPropertyNames 57, getOwnPropertyDescriptor 47, assign 70, create 24, freeze 14, setPrototypeOf 6, fromEntries 4, hasOwn 1.
* `Object.prototype.__proto__` accessor (R=16). The engine itself sets `Memory.__proto__ = null` via the setter (`game.js:486`), so the top-level `Memory` object has a **null prototype**.
  * **GOTCHA**: `Memory.hasOwnProperty(...)` throws TypeError, and `console.log(Memory)` prints JSON, because `toString` is missing.
* **Proxy**: all 13 traps with invariant checks.
  * The engine wraps every `Store` instance: `new Proxy(this, {get(target, name){ if (target[name] !== undefined) return target[name]; if (C.RESOURCES_ALL.indexOf(name) !== -1) return 0; }})` (`engine/src/game/store.js:87-96`).
  * Observable consequences:
    * `typeof store === 'object'`;
    * `store instanceof Store` is true (via the default `getPrototypeOf`);
    * `Object.keys(store)` = nonzero resources;
    * `'energy' in store` is false when energy is 0;
    * `store.foo` is undefined;
    * `JSON.stringify(store)` = own enumerable amounts.
  * Symbol keys hit the `indexOf` path harmlessly.
* **Reflect**: R=19 (most via the TS `__decorate` helper's `typeof Reflect === "object" && typeof Reflect.decorate === "function"` probe; `Reflect.decorate` must be **undefined**). Also used: ownKeys 9, has 9, set 8, getOwnPropertyDescriptor 8, construct 2.

### 5.2 BI-FUNCTION / OBS-FN-OWN-PROPS

* `Function.prototype.call.bind(Function.prototype.bind)` is used by the engine (`game.js:10`). `require` is therefore a **bound function**:
  * `require.name === "bound requireFn"`;
  * `require.toString()` = `function () { [native code] }`;
  * it has user-settable own props `cache` and `timestamp`.
* [probe] own property names on V8 14:
  * sloppy and strict functions, classes and generators: `length, name, prototype`;
  * arrows: `length, name`.
  * `f.caller` and `f.arguments` work through `Function.prototype` accessors (sloppy: `null` / an Arguments object).
  * screeps-profiler copies own props except `['length','name','arguments','caller','prototype']` (`bots/tool/screepers__screeps-profiler/screeps-profiler.js:96,146-150`). Confirm V8 12.4 behaviour in §10 OQ-5.
* **Name inference** (`name` property): it follows the spec's NamedEvaluation. V8 additionally infers *stack-frame* names (`module.exports.loop`), which is a separate mechanism (OBS-STACK-NAMES).

### 5.3 BI-ARRAY / BI-ARRAY-SORT

* All ES2023 Array methods are needed. Usage: push 257, filter 258, map 218, forEach 201, includes 154, slice 139, concat 134, shift 131, splice 117, reduce 118, some 114, find 293 (incl. `Room.find`), fill 79, from 50, every 53, at 17, flatMap 12, flat 11, findLast 4.
* **Sort algorithm (BI-ARRAY-SORT, GOTCHA)**:
  * V8 implements `Array.prototype.sort` with **TimSort** in Torque (`third_party/v8/builtins/array-sort.tq`). It is a stable merge sort over runs, with binary insertion sort for short runs.
  * Pre-processing: copy into a work array, drop holes, count `undefined` (sorted to the end), call the comparator, write back. Holes end up at the end.
  * The default comparator compares `ToString` values by code units ([probe]: `[10,9,1,undefined,'b',null,'a',100,,2].sort()` → `[1,10,100,2,9,'a','b',null,undefined,<hole>]`).
* With a **consistent** comparator, any stable sort gives identical results. With an **inconsistent** one (booleans, random, NaN), only V8's exact algorithm reproduces the order and the sequence of comparator calls.
  * Found: `creeps.sort((a, b) => a.hits > b.hits)` (`bots/full-bot/BraydenF__screeps/Creep.class.js:781,784`) and `targets.sort((a,b)=>a.hits>=b.hits)` (`bots/full-bot/Zerahan__RTS_Bot/Bot_v1/handler.tower.js:64`). R=4.
  * [probe]: `[3,1,2,5,4,1,9,0,7,6,8,2,3].sort((a,b)=>a>b)` returns the input order unchanged: booleans coerce to 0/1, and the result is never < 0.
  * **Rule**: port V8's `array-sort.tq` exactly. That includes the comparator invocation order, the `ToNumber` of the comparator result, and the min-run/galloping constants.
* lodash `sortBy` is order-deterministic regardless (LD-SORTBY).

### 5.4 BI-STRING / BI-STRING-UNICODE / OBS-STRING-UTF16

* Strings are sequences of UTF-16 code units. Lone surrogates are legal everywhere and round-trip through `JSON.stringify`/`parse` as `\udXXX` escapes (BI-JSON-STRINGIFY).
* Limits use code-unit `.length`:
  * Memory is rejected when `> 2*1024*1024` (`driver/lib/index.js:155-156`);
  * `RawMemory.set` throws at `> 2 MB` (`runtime.js:113-115`);
  * segments throw at `> 100*1024` (`runtime.js:264`);
  * uploaded code is limited by `JSON.stringify(modules).length > 5 MB` (`backend/lib/game/api/user.js:84-85`).
* Bot packers depend on 16-bit code units:
  * `utf15` uses code points `[0x30, 0x8030)` (`bots/library/screepers__utf15/README.md`); R=7;
  * LZString `compressToUTF16`, R=5;
  * ScreepsDotNet uses base32768.
* `String.fromCharCode` (R=30) and `charCodeAt` (R=56) must be exact.
* Needed methods: Annex B `substr` (R=77), `padStart` 33/`padEnd` 17, `repeat` 27, `startsWith` 48/`endsWith` 23, `at`, `replaceAll` 2, `matchAll` 2, `trimStart`/`trimEnd`, `normalize` (NFC/NFD tables, P1), `localeCompare` (BI-INTL), `toUpperCase`/`toLowerCase` with full Unicode special casing (`'ß'.toUpperCase()==='SS'`) (P1; resources and names are ASCII).
* **GOTCHA**: `String.prototype.split` with a regex and a limit, `replace` with `$<name>`/`$&`/`$1` patterns and function replacers, and `search` with a string argument (compiled to a RegExp) must follow spec (§5.9).

### 5.5 BI-NUMBER / BI-NUM-TOSTRING / BI-NUM-FORMAT (P0, exact)

* **`Number::toString(x)` radix 10**: shortest round-trip per ECMA-262 §6.1.6.1.20 (V8 uses Grisu3 with a bignum fallback; Ryu-equivalent output). Exponent thresholds: `x >= 1e21` gives `1e+21`, and `x < 1e-6` gives `1e-7` style.
  * [probe]: `0.1+0.2` → `0.30000000000000004`; `2**70` → `1.1805916207174113e+21`; `-0` → `0`.
  * It is used by Memory JSON, string concatenation, `console.log`, and lodash `_.random`'s float path (`(rand + '').length`, `lodash.js:10195`).
* **Radix ≠ 10** (R=58, e.g. `Math.random().toString(36)` R=10, `.toString(16)`): the spec leaves the fractional digits implementation-approximated.
  * V8's `DoubleToRadixCString` (`src/numbers/conversions.cc`) generates digits until the delta bound is reached, with its own rounding.
  * [probe]: `(0.1).toString(3)` = `0.0022002200220022002200220022002201`; `(0.5).toString(2)` = `0.1`; `(25).toString(36)` = `p`; `(-255).toString(2)` = `-11111111`.
  * **Rule: port V8's algorithm.** Integer parts are exact.
* **`toFixed(f)`** (R=111; hivemind/Overmind format CPU with it): exact per spec. The chosen integer `n` minimizes `n/10^f − x`; on a tie, the larger `n` wins; the value is computed on the exact binary value.
  * [probe]: `(1.005).toFixed(2)`=`1.00`, `(1.45).toFixed(1)`=`1.4`, `(0.5).toFixed(0)`=`1`, `(2.5).toFixed(0)`=`3`, `(-1.5).toFixed(0)`=`-2`, `(-0).toFixed(2)`=`0.00`, `(1e21).toFixed(2)`=`1e+21`, `(123.456).toFixed(10)`=`123.4560000000`, `(-1e-7).toFixed(10)`=`-0.0000001000`.
  * The range is 0..100, otherwise `RangeError: toFixed() digits argument must be between 0 and 100`.
* **`toPrecision(p)`** (hivemind 28 sites, e.g. `Game.cpu.getUsed().toPrecision(3)`; `hivemind/dist/main.js:4225`) and **`toExponential`**: exact, spec algorithm.
  * [probe]: `(123.456).toPrecision(3)`=`123`, `(1e-7).toPrecision(3)`=`1.00e-7`, `(0.000001).toPrecision(3)`=`0.00000100`, `(5e-324).toPrecision(3)`=`4.94e-324`.
* **Overmind** truncates by matching a regex on `this.toString()` (`Overmind-dev/src/prototypes/Miscellaneous.ts:13-16`), so it depends on shortest-digit output.
* **Parsing**:
  * `parseInt` follows the spec: `parseInt('08')`=8, `parseInt('0x1f')`=31, `parseInt('1e3')`=1;
  * `parseFloat` and `Number('…')` must be correctly rounded (strtod); `Number('0b11')`=3, `Number('')`=0, `+'1_000'`=NaN;
  * `parseInt(s, radix)` is used by R=9.

### 5.6 BI-MATH / BI-MATH-RANDOM

* **Transcendentals**: `Math.sin/cos/tan/atan/atan2/exp/log/log1p/expm1/cbrt/sinh/…/pow` must be **bit-identical to V8**, which uses fdlibm ports in `src/base/ieee754.cc`. `Math.pow` also uses V8's implementation.
  * Usage: sin 43, cos 41, atan2 10, pow 31, sqrt 34, hypot 6, log 7, exp 7.
  * Why it matters: game formulas `floor` such results, e.g. the GCL level (`game.js:130`).
* **Rounding**:
  * `Math.round` is ties toward +∞ (`Math.round(-0.5)` = `-0`, `Math.round(2.5)` = 3, `Math.round(-2.5)` = −2);
  * `Math.max()` = −∞;
  * `Math.sign(-0)` = −0.
* **`Math.random`** (R=127; also lodash and the engine):
  * [V8 src] `src/numbers/math-random.cc`: xorshift128+ seeded from OS entropy, refilling a 64-entry cache. The double is built from `state0 >> 12` as the mantissa of a [1,2) double, minus 1, i.e. 52 random bits in [0,1).
  * Exact sequences are **not** reproducible on the original (it is unseeded), so parity only requires the same distribution and precision.
  * chitin may seed deterministically as a mod.
  * **GOTCHA**: because of ENV-ONE-REALM, a bot that replaces `Math.random` also changes engine-generated creep names (`names.js:9-13`), **but not** lodash's `_.random/_.shuffle/_.sample`, which captured `nativeRandom` at `runInContext` (`lodash.js:769`).

### 5.7 BI-JSON-PARSE / BI-JSON-STRINGIFY / OBS-JSON-FORMAT (P0, exact bytes)

* The Memory string is player state, so its byte-exact text matters. Size limits are checked on it, and segments hold player-encoded JSON. Rules:
* **Stringify, no indent**:
  * no whitespace;
  * keys in OwnPropertyKeys order (OBS-PROP-ORDER);
  * numbers via `Number::toString`, with non-finite numbers as `null`; `-0` → `0`;
  * `undefined`, functions and symbols are omitted in objects and become `null` in arrays;
  * holes → `null`.
* **Escapes**:
  * `"` → `\"`, `\` → `\\`;
  * `\b \f \n \r \t` use their short escapes;
  * other code units below 0x20 → `\u00XX` with **lowercase** hex;
  * **lone surrogates → `\udXXX` lowercase** (ES2019 well-formed stringify);
  * U+2028/2029 and U+007F are emitted **raw**;
  * `/` is not escaped.
  * [probe]: `{"g":"\ud800","h":"<U+2028 raw>","k":"\u0007"}`.
* **Indent** (R=26 use `JSON.stringify(x, null, 2)`):
  * newline + indent per level;
  * `": "` after keys;
  * empty `{}` and `[]` stay compact.
* **toJSON and replacers**:
  * `toJSON(key)` is called with the property key, or `""` at the root ([probe]);
  * a replacer array filters keys at every level.
* **Errors**:
  * Map/Set → `{}`; typed arrays → index-keyed objects; Date → ISO string, or `null` if invalid.
  * **BigInt throws** `TypeError: Do not know how to serialize a BigInt`.
  * **Cycles throw** `TypeError: Converting circular structure to JSON` + V8's path description ([probe]). An end-of-tick throw from Memory stringify discards the whole run (02 §13).
* **Parse**:
  * spec grammar only: no trailing commas, no comments;
  * duplicate keys: the last wins;
  * a `"__proto__"` key becomes an **own** data property (not the prototype);
  * numbers are correctly rounded (`1e400` → Infinity, `-0` stays `-0`);
  * the reviver order is post-order, with array indices then keys in order ([probe]: `c,b,0,1,a,`).
  * V8 12+ error text format: `SyntaxError: Expected property name or '}' in JSON at position 1 (line 1 column 2)`. The text differs across V8 versions (§10 OQ-2).

### 5.8 BI-DATE / OBS-DATE-TZ-LOCALE

* **P0**: `Date.now()` and `new Date().getTime()` give wall-clock ms. Used by bots (R=31) and by the engine (`game.js:540`, `creeps.js:853`, `structures.js:1364-1365`, `power-creeps.js:54`).
* **P1, parsing** (`Date.parse` and `new Date(string)`):
  * ISO date-only strings are **UTC**;
  * ISO date-time strings without an offset are **local time**;
  * V8's legacy parser fallback accepts forms like `'2020/01/01'` and `'Jan 1 2020'` (both local) ([probe]);
  * invalid input → NaN, and `String(invalid)` = `Invalid Date`.
* **P1, formatting**:
  * `toString()` = `Thu Jan 01 1970 00:00:00 GMT+0000 (Coordinated Universal Time)`, where the zone name comes from ICU and the default locale;
  * `toISOString`, `toUTCString` = `Thu, 01 Jan 1970 00:00:00 GMT`;
  * `toJSON`.
* Local-time getters such as `getHours` (R=11) depend on the host TZ. The official TZ is unknown; recommend **UTC** (§10 OQ-3).

### 5.9 BI-REGEXP / OBS-REGEX-BACKTRACK

* **Semantics**: Irregexp, a backtracking matcher with spec semantics:
  * `lastIndex` handling for `g`/`y`, and the `exec` result shape (`index`, `input`, `groups`);
  * case-insensitive matching with Canonicalize (simple case folding under `u`);
  * `\b` and `\B` (used e.g. in `toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",")`, `bots/full-bot/KungFuClan__KungFuScreeps/src/RoomVisuals/RoomVisual.Api.ts:231`).
* **Usage**: lookahead R=13. Lookbehind, named groups (R=1), sticky (2), unicode (5), dotAll (4) and `\p{…}` (0) are P1, all ES2018-level.
* **`RegExp(string)` construction** at runtime: lodash builds `reIsNative` (`lodash.js:741-744`), and Overmind builds patterns dynamically.
* **GOTCHA (catastrophic backtracking is part of the contract)**: Overmind's `Overmind_obfuscated.js` (javascript-obfuscator "selfDefending") executes at global reset. Decoded with the file's own string decoder:

  ```
  f.toString().search('(((.+)+)+)+$').toString().constructor(f).search('(((.+)+)+)+$')
  ```

  (`Overmind-dev/src/Overmind_obfuscated.js:1`). On the single-line source this matches at index 0 immediately. If `Function.prototype.toString` returned text containing a newline, the nested quantifiers would backtrack exponentially and burn the CPU limit (by design).
  * **Rule**: exact source text (OBS-FN-TOSTRING), and greedy-first backtracking order identical to Irregexp.
  * A regex engine without backtracking (NFA/DFA) is **not** acceptable for parity: back-references and lookarounds need it too.
* **PERF**: interrupt checks inside the regex engine are required (ENV-TERMINATION).

### 5.10 BI-MAP-SET / BI-WEAK / BI-SYMBOL / BI-ITERATORS

* **Map/Set** (R=74/81): insertion-ordered; SameValueZero keys (`NaN` equals `NaN`; `-0` is normalized to `+0`); live iteration that sees entries added during iteration; `size`; `forEach`. ES2025 Set methods (`union`, …) ship in V8 12.2+ and are P2.
* **WeakMap/WeakSet** (P1): lodash creates `metaMap = WeakMap && new WeakMap` (`lodash.js:787`). WeakRef and FinalizationRegistry: R=0 (P2).
* **Symbol**:
  * `Symbol()`, `Symbol.for`/`keyFor`, and `description` (R=13 `.description`, partly unrelated);
  * well-knowns: `iterator` (R=13 implement or consume), `asyncIterator`, `hasInstance`, `toPrimitive`, `toStringTag` (R=5), `species`, `isConcatSpreadable`, `match`/`replace`/`search`/`split`, `unscopables`.
  * Symbols are excluded from `for-in`/`Object.keys`/`JSON`. `String(sym)` works, but `"" + sym` throws.
* **Iterators**: Array/Map/Set/String iterators with `%IteratorPrototype%`, and `return()` called on early exit from for-of/destructuring. Iterator helpers (V8 12.2) are P2.

### 5.11 BI-TYPEDARRAY / BI-ARRAYBUFFER / BI-DATAVIEW / BI-BIGINT

* **CostMatrix** (P0, R=105 `new PathFinder.CostMatrix`), `engine/src/game/path-finder.js:18-47`:
  * `_bits = new Uint8Array(2500)`;
  * `set` does `this._bits[x*50+y] = Math.min(Math.max(0, val), 255)` after `x|0, y|0`, so ToUint8 truncates and NaN → 0;
  * `clone` = `new Uint8Array(this._bits)`;
  * `serialize` = `Array.prototype.slice.apply(new Uint32Array(this._bits.buffer))`, i.e. **625 little-endian uint32 words**;
  * `deserialize` = `new Uint8Array(new Uint32Array(data).buffer)`.
* **Endianness**: typed arrays must be little-endian, because serialized CostMatrices are stored in Memory and segments by bots.
* **PathFinder callback**: the native PathFinder accepts any typed array whose element length is 2500 as a room cost matrix (`ret._bits`; `driver/lib/path-finder.js:116-127`, `driver/native/src/pf.cc:48-56`).
* **Terrain**: `Room.Terrain#getRawBuffer(dest)` calls `%TypedArray%.prototype.set.call(dest, array)` on a Uint8Array view of the shared terrain buffer (`engine/src/game/rooms.js:1223-1242`, `:389-398`).
* **Usage**: Uint8Array 31, Int32Array 20, Uint16Array 15, Uint32Array 12, Int16Array 7, Float32Array 4, Float64Array 3, Int8Array 3, ArrayBuffer 14. Bots use `subarray`, `set`, `fill`, `slice`, `buffer`, `byteLength`.
* **Detach**: an ArrayBuffer must be detachable. `WebAssembly.Memory.grow` detaches the old buffer and wasm-bindgen detects it with `cachedUint8Memory0.byteLength === 0` (`bots/full-bot/The-International-…/src/wasm/pkg/commiebot_wasm.js:29-34`).
* **DataView** (P1): ScreepsDotNet writes with `setBigUint64(ptr, BigInt(..), true)` (`bots/discovered/thomasfn__ScreepsDotNet/Bootloader/src/bootloader.ts:283-300`). **BigInt** is P2: R=2, both wasm hosts.
* `ArrayBuffer.isView` R=3. Resizable ArrayBuffer and `transfer` are P2, and Krebs leaves them out (§5.15).

### 5.12 BI-INTL (P1/P2)

* **Default locale**: on Linux servers with `LANG=C`/`en_US` it is `en-US` ([probe] shows the host locale leaks: `de-DE` on the probe machine). Recommend fixed **en-US**.
* **Uses**:
  * `n.toLocaleString()` (R=14) → `1,234,567`.
  * `localeCompare` (R=13; sorting creep names, ids and dates, e.g. `bots/full-bot/AlexeyKozhemiakin__screeps/console-commands.js:748`). This is ICU root collation, not code-unit order. [probe]: `['b','A','a','B','_','1','é','e']` sorts to `["_","1","a","A","b","B","e","é"]`, and `'a'.localeCompare('B') = -1`.
  * `new Date().toLocaleString('en-US', {timeZone:'Australia/Melbourne'})` (`bots/full-bot/BenTaylorPublic__screeps/src/reporting/report-controller.ts:26`), `'Asia/Jerusalem'` with `hour12:false` (`bots/full-bot/Denloob__dci-screeps/src/prototypes/prototype.spawn.js:113`), `'en-GB'`. An unknown zone throws RangeError.
  * `new Intl.NumberFormat().format()` (R=2), `Intl.Segmenter` (R=0 in-game).
* **Rule**:
  * **P1**: en-US number grouping, and root collation for at least Latin-1 with ICU tertiary strength ordering;
  * **P2**: full CLDR/ICU, IANA tz database.
* **GOTCHA**: the comparison result values (−1/0/1) and tie behaviour feed into V8 TimSort.

### 5.13 BI-ERROR / BI-ERROR-V8 / OBS-ERROR-TOSTRING

* **`Error.prototype.toString`**:
  * `name` defaults to `"Error"`;
  * empty message → just the name;
  * an empty name with an empty message gives `""` ([probe]).
* **Own properties** of an instance: `stack` and `message`, both non-enumerable, so `JSON.stringify(err)` = `{}`. `message` is absent when the constructor got `undefined`. `cause` comes from the options object.
* **V8 extras** (P1):
  * `Error.captureStackTrace(obj, fn?)` installs `obj.stack` and omits frames above `fn`. Used by ScreepsDotNet glue (`bots/discovered/thomasfn__ScreepsDotNet/ScreepsDotNet.API/js/world/bootloader.js:1625`) and snippets.
  * `Error.stackTraceLimit`: number, default 10. 0 → header only. It is read at capture time. Set to 20 by `bots/full-bot/tiennou__Overmind/src/main.ts:50`, and by the wasm-bindgen panic hook (`bots/full-bot/The-International-…/src/wasm/pkg/commiebot_wasm_bg.js:221-222`).
  * `Error.prepareStackTrace(err, callSites)` gets CallSite objects with `getFileName/getLineNumber/getColumnNumber/getFunctionName/getTypeName/getMethodName/isNative/isToplevel/isEval/isConstructor/…` (`bots/library/screepers__screeps-snippets/src/globals/JavaScript/determineFileFunctionLineWithinCode.js:8-22`).
* **[probe] V8 14**: `stack` is an own **accessor** pair on error instances, and formatting is **lazy**. The header uses the name and message at **first access**: `e.message='b'` before the first access gives `Error: b`. V8 12.4 may expose a data property (§10 OQ-4).

### 5.14 BI-WASM (P1; P0 for Rust/.NET/C bots)

* **Corpus**:
  * R=11 repos call the WebAssembly API, and 4 ship `.wasm` files;
  * Rust/wasm-bindgen loaders (rustyscreeps starter, crustacean, OlekRia, Azaril screeps-pack, glitchassassin screeps-clockwork, International), .NET (ScreepsDotNet), and small hand-loaded modules (`Ureimu`, `1755616537` priority-queue `.wasm`);
  * **all synchronous**: `new WebAssembly.Module(bytes)` + `new WebAssembly.Instance(module, imports)`, or wasm-bindgen `initSync({module})`. Examples: `bots/starter/rustyscreeps__screeps-starter-rust/js_src/main.js:73-75`, `bots/full-bot/The-International-…/src/other/wasmInit.ts:6-8`, `bots/discovered/thomasfn__ScreepsDotNet/Bootloader/src/bootloader.ts:180-190`.
  * `WebAssembly.instantiate` (async): R=1, P2.
* **Required surface**:
  * `Module` (+ static `imports`/`exports`/`customSections`), `Instance` (`exports` frozen, null-prototype namespace), `Memory` (`buffer`, `grow` → detach), `Table`, `Global`;
  * `CompileError`/`LinkError`/`RuntimeError`, checked with `instanceof` (`bots/full-bot/InfinityDevTech__crustacean/js_src/main.js:190-194`);
  * JS↔wasm value conversion (i32 ToInt32, f64; **i64 ↔ BigInt**).
* **Proposals**:
  * which proposals the corpus `.wasm` files use was not determined (OQ-11). P1 = MVP plus the post-MVP features V8 12.4 enables by default and toolchains emit without extra flags: mutable-globals, sign-ext, non-trapping float-to-int, multi-value, bulk-memory, reference-types;
  * P2: SIMD, exceptions, GC, memory64.
* **Rust starter specifics**:
  * it loads over several ticks (bucket-gated) and does `delete require.cache[MODULE_NAME]` to free bytes;
  * it **replaces `module.exports.loop`** at runtime (`js_src/main.js:62-81`), which works because the runtime reads `mainExports.loop` every tick (`game.js:519-532`);
  * it calls `Game.cpu.halt()` after a wasm panic (`:34-39`).
* Wasm frames in `stack`: V8 format `at <name> (wasm://wasm/<hash>:wasm-function[<i>]:0x<off>)` (P2).

### 5.15 Absent-by-design and P2 builtins

* `escape`/`unescape` (P2, Annex B) exist in V8. Keep them.
* `Atomics` exists (P2). The driver itself prefers SharedArrayBuffer for terrain (`make.js:34`), but that buffer is host-side.
* **What Krebs takes out of a bot's context**, ahead of everything else (`LOCK_BUFFERS` in `crates/runtime/src/host.rs`). The memory limit on a VM lives in the array buffer allocator, and V8 allocates resizable buffers without it, so one bot could commit memory until the host ran out; shrinking one also unmapped memory the path finder was reading. The official Node has these, so a bot written for it would notice:
  * the `SharedArrayBuffer` global, and `grow` and `constructor` on its prototype (a shared buffer from `WebAssembly.Memory` still exists);
  * `ArrayBuffer.prototype.resize`, `transfer` and `transferToFixedLength`;
  * `Atomics.waitAsync`; `Atomics.wait` throws a TypeError, because a sleeping thread uses no CPU and would sit out the limit;
  * `new ArrayBuffer(n, {maxByteLength})` makes a fixed buffer of `n` bytes: `ArrayBuffer` is a Proxy that passes on the length alone.
* ES2024+ library additions that V8 12.4 ships: `Object.groupBy`/`Map.groupBy`, `Promise.withResolvers`, `Array.fromAsync`, Set methods, iterator helpers, `String.prototype.isWellFormed`/`toWellFormed`, and RegExp `v` flag. P2 (R≈0), but cheap.

---

## 6. Observable behaviours (OBS-*)

### 6.1 OBS-STACK-FORMAT / OBS-STACK-NAMES (P0)

**Who parses stacks:**

* The canonical **ErrorMapper** regex (TS starter lineage, R=80):

  ```
  /^\s+at\s+(.+?\s+)?\(?([0-z._\-\\/]+):(\d+):(\d+)\)?$/gm
  ```

  It maps only frames whose file is exactly `main` through `require('main.js.map')` (`hivemind/src/utils/ErrorMapper.ts:43-52`; another bot uses `/main:(\d+):(\d+)/g`).
* Overmind's logger matches `/([^ ]*) \(([^:]*):([0-9]*):([0-9]*)\)/` (`Overmind-dev/src/console/log.ts:52`).
* R=89 repos construct a `SourceMapConsumer`, and R=88 `require` a `*.map` module.

**Stack text** = `ToString(err)` at first access, then for each of up to `Error.stackTraceLimit` frames: `"\n    at " + frame`. A frame is:

* `<fnDesc> (<location>)`; or
* `<location>` alone, when the function has no name and it is not a method or constructor call.

Locations:

* `<script>:<line>:<col>`: both 1-based; `<script>` is the module name.
* `<anonymous>`: for builtins, e.g. `at Array.map (<anonymous>)`.
* `eval at <fnDesc> (<location>), <anonymous>:<l>:<c>`: for eval and `Function` code.

**Test vectors** ([probe]; script names and the wrapper exactly as the driver uses them):

| Situation | Frame text |
|---|---|
| named function `function plain(){throw…}` | `at plain (main:2:25)` |
| assigned anonymous fn `module.exports.loop = function(){}` | `at module.exports.loop (main:4:37)` |
| anonymous fn/arrow returned by a factory, then called as `module.exports.loop()` | `at Object.loop (main:2:40)` |
| arrow inside a callback | `at main:1:200` (no name, no parens) |
| builtin calling back | `at Array.map (<anonymous>)`, `at Array.sort (<anonymous>)`, `at JSON.parse (<anonymous>)`, `at String.repeat (<anonymous>)`, `at Number.toFixed (<anonymous>)` |
| object-literal method `{meth(){}}` | `at Object.meth (main:3:29)` |
| nested literal `o2.a.b()` | `at Object.b (main:9:38)` |
| computed literal key `{['comp'+'uted']: function(){}}` | `at Object.computed (main:3:86)` |
| prototype method `Creep.prototype.moveTo = function(){}` | `at Creep.moveTo (main:3:44)` |
| getter via `defineProperty(Creep.prototype,'foo',{get: function(){}})` or `{get(){}}` | `at Creep.get (main:1:92)` |
| class getter `get bar(){}` on an instance | `at get bar (main:1:95)` |
| class static method | `at Foo.s (main:1:139)` |
| class method | `at Foo.m (main:1:182)` |
| constructor | `at new B (main:10:62)` |
| Proxy trap `get(t,k){}` | `at Object.get (main:4:41)` |
| bound function | `at main:8:31` |
| generator body + resume | `at gen (main:7:24)` then `at gen.next (<anonymous>)` |
| async after await | `at af (main:5:40)` … `at async outer (main:6:25)` |
| direct eval code | `at eval (eval at module.exports.ev (main:11:27), <anonymous>:1:20)` |
| `new Function` body | `at eval (eval at module.exports.nf (main:12:27), <anonymous>:3:7)` (line 3: two header lines) |
| module top level | `at __module (broken:1:42)` then `at broken:2:3` |
| loop call site | `at __module (__mainLoop:1:52)` then `at __mainLoop:2:3` |
| lodash in runtime | `at arrayFilter (<runtime>:L:C)`, `at lodash.filter (<runtime>:L:C)`, `at <runtime>:L:C` |
| recursion overflow | `RangeError: Maximum call stack size exceeded` + repeated `at rec (…)` |

**Naming algorithm.** This is empirical: it reproduces every [probe] vector above and below on V8 14. The V8 source to port is `src/objects/call-site-info.cc` (`SerializeJSStackFrame`/`AppendMethodCall`) plus `JSReceiver::GetConstructorName` [V8 src]; check it against V8 12.4 (§10 OQ-13).

```
frame(f):
  s = ""
  if f.isAsync:        s += "async "
  if f.isConstructor:  return s + "new " + (fnName || "<anonymous>") + loc
  if receiver is undefined/null/the global object:          // top-level call
      return fnName ? s + fnName + loc : s + bareLocation
  typeName   = constructorName(receiver)
  methodName = first key K on receiver or its proto chain with a DATA value === the function
  if fnName == "":  return s + typeName + "." + (methodName || "<anonymous>") + loc
  name = isIdentifierName(fnName) ? typeName + "." + fnName : fnName
  if methodName && methodName != lastDotSegment(fnName): name += " [as " + methodName + "]"
  return s + name + loc
```

Definitions:

* `fnName`: the function's debug name. That is the `name` property's value, or V8's parser-*inferred* name, which keeps dotted paths and drops `.prototype`. Examples: `module.exports.loop = function(){}` → `module.exports.loop`; `D.prototype.foo = function(){}` → `D.foo`; `o2.x = function(){}` → `o2.x`.
* Names that are **not** IdentifierNames get **no** type prefix: dotted inferred names, `get bar`/`set v`, `#pm`, `[sy]`, `5`.
* `constructorName(receiver)`, in this precedence:
  1. if the receiver is a function: the function's own name (`Z.named [as st]`, static `B.s`);
  2. else the name of the function that constructed it (from its hidden class), if non-empty and not `Object` (`W.f` even after `W.prototype` was replaced; `C2.m`);
  3. else `obj[Symbol.toStringTag]` as an own/inherited data property (`Tagged.f`);
  4. else `obj.constructor.name` if non-empty;
  5. else `Object` (`new (class{})().m()` → `Object.m`).

  Primitive receivers give the wrapper name (`plain3.call(5)` → `Number.plain3`).

More [probe] vectors:

| Situation | Frame |
|---|---|
| named fn as prop | `Object.g [as f]` |
| `C.prototype.foo = function bar(){}` | `C.bar [as foo]` |
| `var fn=function(){}; E.prototype.foo=fn` | `E.fn [as foo]` |
| null-prototype receiver | `Object.nf [as f]` |
| `plain2.call({})` | `Object.plain2` |
| arrow stored as prop | `Object.arrow` |
| class field arrow `h = () => …` | `F.h` |
| static field fn | `G.h` |
| class setter | `set v` |
| literal getter | `get gg` |
| defineProperty `{get: function qq(){}}` | `I.qq` |
| async class method after await | `J.am` |
| private method | `#pm` |
| symbol-keyed method | `[sy]` |
| numeric-keyed method | `5` |
| array receiver | `Array.ff [as f]` |
| `m2.beta()` where the fn was inferred `m1.alpha` | `m1.alpha [as beta]` |
| bound method `o7.bm.bind(o7)()` | `Object.bm` |
| strict call with `undefined`/explicit `globalThis` receiver | `su` / `gr` |
| anonymous callback called plainly by a library function | bare location |

**Required engine features**:

* per-function source positions (line/col of the throw or call position, measured on the exact source text **including the wrapper prefix**);
* the call position of every frame (the column of the property-access name for calls such as `__m.loop()`, [probe] col 5);
* inferred names;
* receiver type names;
* lazy formatting.

**GOTCHA**: column numbers are V8's 1-based "position of the call/throw token". Source-map tools subtract 1 or not inconsistently, so chitin must reproduce V8 columns, not "fix" them.

**PERF**: capturing a structured trace (≤ 10 frames) at `new Error` must be cheap. Formatting is deferred to the first `stack` read. Bots create Errors on every failed operation (`safe()` wrappers, profilers' `new Error().stack` caller lookup).

### 6.2 OBS-STACK-LIMIT

* Default 10 frames. Frames from the runtime (`<runtime>`) count against the limit.
* The loop frame plus the `__mainLoop` frames take 2, and the remaining runtime frames are below them. The exact runtime frame text below `__mainLoop:2:3` depends on chitin's runtime (§10 OQ-6). Error mappers ignore them.

### 6.3 OBS-STACK-TIMEOUT (resolves 02 OQ-3)

* The timeout exception's `stack` is **not** V8-formatted. isolated-vm attaches a native accessor that renders `name + ": " + message + frames` on every read (`[ivm] isolate/stack_trace.cc:79-110`). So the message rewritten by the driver (`runtime-driver.js:105-107`) **does** appear: `Error: Script execution timed out: CPU time limit reached`.
* Frames come from `CurrentStackTrace(isolate, 10)` captured at interrupt time (`run_with_timeout.h:52`) and rendered by `RenderSingleStack` (`stack_trace.cc:149-195`):
  * each frame is `\n    at ` + (`new ` + name, or name) + ` (` + script:line:col + `)`;
  * **no `Type.` prefix and no `[as x]`**;
  * an **anonymous frame prints the bare location and stops rendering further frames** (`return ss.str()` inside the loop, `:183-188`);
  * wasm frames are `name (<WASM>line:col)`.
* That text then goes through the driver's uncaught-error processing (02 CON-ERR-FORMAT), and make.js detects `/CPU limit reached/` (`make.js:262`).

### 6.4 OBS-ERROR-MESSAGES (P1)

* Console output parity needs V8's message templates ([probe], V8 14). Examples:
  * `Cannot read properties of undefined (reading 'x')`
  * `Cannot set properties of undefined (setting 'x')`
  * `o.f is not a function`
  * `X is not defined`
  * `Assignment to constant variable.`
  * `Invalid array length`
  * `Invalid count value: -1`
  * `Cannot convert a Symbol value to a string`
  * `(intermediate value)(intermediate value).constructor.f is not a constructor` (V8 renders the callee expression text)
  * `Converting circular structure to JSON\n    --> starting at object with constructor 'Object'\n    --- property 'a' closes the circle`
  * `The number 1.5 cannot be converted to a BigInt because it is not an integer`
* Bots rarely branch on messages. One that does: `make.js` greps `/Isolate is disposed/`, `/Array buffer allocation failed/` on host errors.
* **Rule**: adopt V8 12.4's `messages.h` templates verbatim (P1), including the "callee expression rendering" for `is not a function`.

### 6.5 OBS-FN-TOSTRING (P0)

* **User functions, classes, methods, getters, arrows, async functions and generators**: the **exact source slice** from the first token of the function or class syntax to the closing `}`, including comments and whitespace.
  * Method shorthand starts at the name: `m ( ) { }`.
  * Getter: `get x(){return 1}`.
  * Class: `class   A  extends Object { m(){ return 1 } }`.
* **Builtins and host natives**: `function <name>() { [native code] }`. Native getters: `function get size() { [native code] }`.
* **Bound functions and callable Proxies**: `function () { [native code] }`.
* **`Function` constructor**: `function anonymous(<params>\n) {\n<body>\n}`.
* Dependents:
  * **lodash `reIsNative`** is derived from `Function.prototype.toString.call(Object.prototype.hasOwnProperty)` and tested on candidates (`lodash.js:741-744,8838-8839`). If chitin's native format differs, every `getNative` returns undefined. lodash then silently uses slower fallbacks (no `Set` cache, shim keys, `new Date().getTime()` for `_.now`), giving the same results at lower speed.
  * **javascript-obfuscator self-defence** (Overmind) needs single-line exactness (§5.9). Its base64 decoder also reads `charCodeAt(i+10)` of the decoder function's own source text and treats code 10 (`\n`) specially (`Overmind_obfuscated.js:1`).
  * International derives class names: `creepClass.toString().match(/ (\w+)/)[1]` (`bots/full-bot/The-International-…/src/other/profilerRegister.ts:270`).
  * screeps-profiler overrides `wrappedFunction.toString` (`screeps-profiler.js:143-144`).
* **GOTCHA**: functions created by chitin's native runtime but backed by JS in the original return `[native code]` in chitin, whereas the original returns the webpack bundle's source. Examples: `Creep.prototype.moveTo`, `RoomPosition`, `require` (a bound function, already native-looking). No corpus bot depends on the engine's source text; accept this, but keep `isNative`-style probes in mind (§10 OQ-7).

### 6.6 OBS-TOSTRINGTAG (P0)

* [probe] `Object.prototype.toString.call(x)` results:

  | `x` | Result |
  |---|---|
  | async fn | `[object AsyncFunction]` |
  | generator fn | `[object GeneratorFunction]` |
  | class, callable Proxy | `[object Function]` |
  | arguments | `[object Arguments]` |
  | Map | `[object Map]` |
  | Promise | `[object Promise]` |
  | Symbol | `[object Symbol]` |
  | BigInt | `[object BigInt]` |
  | Uint8Array | `[object Uint8Array]` |
  | null | `[object Null]` |

* lodash type predicates depend on these tags (`lodash.js:63-71,8709-8713`, LD-ISFUNCTION).

### 6.7 OBS-PROP-ORDER / OBS-FOR-IN-ORDER (P0)

* `[[OwnPropertyKeys]]` order:
  1. integer indices (canonical numeric strings < 2^32−1) ascending;
  2. other strings in creation order;
  3. symbols in creation order.
* Deleting and re-adding a key moves it to the end. [probe]: `{z;5;y;1}`, then delete `z`, re-add `z` → `['1','5','y','z']`.
* `'4294967295'`, `'-1'` and `'01'` are strings. Numeric-literal keys are canonicalized (`9007199254740993` → `'9007199254740992'`).
* V8 keeps insertion order even in dictionary mode. chitin's object model must preserve it at any size; Memory objects routinely have thousands of keys.
* **for-in**:
  * own enumerable string keys in the order above, then each prototype's enumerable keys not already seen (shadowed or non-enumerable names suppress later ones);
  * keys deleted before being visited are skipped;
  * keys added during iteration are not guaranteed (V8: not visited).
* The engine's `toJSON` uses for-in over instance + prototype getters (`engine/src/utils.js:535-543`), so object JSON key order = own `_cache` keys (skipped, they start with `_`) then prototype getter definition order.
* Iterating `Game.creeps`, `Memory.creeps` and `Game.rooms` in this order determines spawn/intent order in most bots, and therefore which of two same-tick intents wins (cross-ref the processor specs).

### 6.8 OBS-ARGUMENTS / OBS-TYPEOF

* `arguments`:
  * own `callee` (non-enumerable) in both modes; strict mode's is a throwing accessor;
  * mapped arguments (sloppy, simple parameter list) alias the parameters;
  * `Symbol.iterator` is own.
  * lodash `isArguments` requires `hasOwnProperty('callee') && !propertyIsEnumerable('callee')` (`lodash.js:8468-8471`).
  * Used by R=70 repos; `arguments.callee` by 6.
* typeof:
  * `typeof undeclared` → `'undefined'` (environment sniffing: `typeof window/self/global/module/exports/define/setTimeout/TextEncoder`);
  * `typeof class{}` → `'function'`;
  * `typeof null` → `'object'`;
  * `typeof 1n` → `'bigint'`.

### 6.9 OBS-HOST-FIDELITY (P0)

Profilers and prototype patchers introspect engine objects:

* screeps-profiler walks `Object.getOwnPropertyNames(proto)` and wraps:
  * `writable` function values;
  * `configurable` accessors (non-configurable ones are skipped).

  (`bots/tool/screepers__screeps-profiler/screeps-profiler.js:165-218`; Overmind ships a copy.)
* Engine getters are `configurable:false, enumerable:true` unless opts say otherwise (`engine/src/utils.js:516-533`). Methods are plain assignments, so they are writable, enumerable and configurable.
* RoomPosition has an own non-enumerable writable `__packedPos` and enumerable accessors on the prototype (`engine/src/game/rooms.js:1288-1347`). Bots read `__packedPos` directly (R=8, e.g. `bots/library/NesCafe62__screeps-pathfinding/pathing.js:901-902`).

**Rule**: a chitin game object must be an ordinary JS object as far as the realm can observe:

* same own/prototype property sets, **creation order**, attributes, and prototype chain;
* `instanceof` and `constructor` behave the same;
* `toString`/`toJSON` behave the same.

Native acceleration (hidden internal slots) is allowed only where unobservable. The detailed descriptors are owned by the API specs (03-05); the engine must merely support that.

---

## 7. lodash 3.10.1 touch-points (LD-*)

### 7.1 LD-INSTANCE / LD-ROOT / LD-CAPTURED

* The runtime requires `lodash` (^3.10.1 in the driver, exactly `3.10.1` in the engine; `reference/engine/package.json:18`).
* Inside the webpack module wrapper, lodash finds:
  * `freeExports`/`freeModule`;
  * `freeGlobal = typeof global == 'object' && global.Object && global`;
  * `root = freeGlobal || … || this` (`lodash.js:250-274`).

  webpack 3 rewrites the free `global` to its `buildin/global.js` shim (`(function(){return this})()` || `Function("return this")()`), so `root` = the context global.
* Player `_` = `_.runInContext()` called once per VM (`game.js:475-477`). `runInContext` captures from `root` **at that moment, before any player code runs** (`lodash.js:700-769`):
  * `Array, Date, Error, Function, Math, Number, Object, RegExp, String, TypeError`;
  * `fnToString`, `objToString`, `hasOwnProperty`;
  * `ArrayBuffer`, `clearTimeout` (undefined), `parseFloat`, `Set` (native-checked), `setTimeout` (undefined), `Uint8Array`, `WeakMap`;
  * `nativeCreate/IsArray/IsFinite/Keys/Now/ParseInt/Random`.
* So later player monkey-patches of these builtins do not affect `_`. Patches of **prototype methods** that lodash calls dynamically still do, e.g. `array.sort` in `baseSortBy` (`lodash.js:2605`).
* `require('lodash')` returns the same `_` (`game.js:510`).
* chitin should run the real lodash 3.10.1 source in the player realm. It is ~12k lines, so it is a good snapshot candidate (ENV-SNAPSHOT) and gives exact semantics for free. That requires §5/§6 to be exact, notably LD-ISNATIVE and OBS-TOSTRINGTAG.

### 7.2 LD-ISNATIVE (P0)

* `reIsNative = RegExp('^' + fnToString.call(hasOwnProperty).replace(/[\\^$.*+?()[\]{}|]/g,'\\$&').replace(/hasOwnProperty|(function).*?(?=\\\()| for .+?(?=\\\])/g,'$1.*?') + '$')` (`lodash.js:741-744`).
* With V8's text this yields `^function.*?\(\) \{ \[native code\] \}$`.
* [probe] It matches every V8 builtin, including bound functions and callable Proxies, and rejects user functions.
* Results are used by `getNative` (`lodash.js:3942-3945`) for `Set`, `WeakMap`, `Object.create`, `Array.isArray`, `Object.keys` and `Date.now`.

### 7.3 LD-ISFUNCTION (P0, GOTCHA)

* `isFunction(v) = isObject(v) && objToString.call(v) == '[object Function]'` (`lodash.js:8709-8713`).
* [probe] It is false for async and generator functions, and true for classes, arrows and callable Proxies.
* Engine consequence: §3.3 (an async loop is never executed).

### 7.4 LD-TIMERS (P1)

* `_.delay`, `_.defer`, `_.debounce` and `_.throttle` call the captured `setTimeout`, which is undefined, so they throw `TypeError: setTimeout is not a function` ([probe]; `lodash.js:1773,7709`).

### 7.5 LD-TEMPLATE (P2)

* `_.template` compiles via `Function(importsKeys, sourceURL + 'return ' + source).apply(undefined, importsValues)` (`lodash.js:10809`).
* `importsKeys` is an **array**, so its ToString gives comma-joined parameters.
* The source carries a `//# sourceURL=lodash.templateSources[n]` comment (`:10745-10748`). V8 uses that as the script name in stack frames (P2).
* R=2.

### 7.6 LD-RANDOM (P1)

* `_.random(a,b)` = `min + floor(nativeRandom()*(max-min+1))` (`lodash.js:2501-2503`).
* The float path adds `parseFloat('1e-' + ((rand+'').length - 1))` (`:10193-10196`), so it depends on shortest-digit `Number→String` of the random value.
* `shuffle` and `sample` use `baseRandom`.
* Usage: R=24/18/16.

### 7.7 LD-KEYS / LD-SORTBY / LD-ISARGUMENTS (P0)

* **`_.keys`** (`lodash.js:9671-9678`) uses `shimKeys` (`:4369-4387`) when:
  * `object.constructor.prototype === object`; or
  * the object is array-like (numeric `length`).

  `shimKeys` walks `keysIn` (for-in order) and filters to own keys or indices. Otherwise it uses `nativeKeys` = `Object.keys`.
  * **GOTCHA**: any object with a numeric `length` property (including Memory sub-objects) is iterated by index by `_.forEach`/`_.map`. That is lodash semantics and must not be "fixed".
* **`_.sortBy`** builds `{criteria, index, value}` records and sorts them natively with `compareAscending` (`criteria`, then `index`; `lodash.js:425-460,2602-2611`). It is deterministic under any correct sort, but the comparator call sequence still comes from the native sort.
* **LD-ISARGUMENTS**: see §6.8.

---

## 8. Performance requirements (PERF-*)

Scale target: ~1000 players on a 16-core PC at a high tick rate. In the original, each player tick crosses the Node↔isolate boundary several times, copying the full runtime data in (`make.js:144-160`) and the output out. The engine-level items:

### 8.1 PERF-SCALE-HEAPS / PERF-GC / PERF-VM-CREATE

* **Per-player**: the heap, compiled code (per-player closures over shared bytecode), job queue, CPU meter and termination flag.
* **Shared, immutable, read-only from JS**:
  * builtins' code;
  * the runtime/lodash bytecode;
  * terrain (`staticTerrainData` is one shared buffer in the original, `make.js:34-47`);
  * per-tick room snapshots.
* Baseline footprint per idle player must be small. 1000 × V8-isolate (~2–5 MB min + runtime snapshot) is what the original pays.
* **Rule**:
  * builtin objects and the runtime are instantiated per realm lazily or copy-on-write from a frozen template (the analogue of the V8 snapshot);
  * GC is per heap, generational (most per-tick garbage is young: Game wrappers, intent objects, lodash temporaries);
  * no cross-player pause;
  * an idle heap can be compacted or serialized.
* **Thread model**: one player's code runs on one OS thread at a time. Different players run in parallel. Heaps must not share mutable JS objects (no global locks on property access).
* **What the original does slowly**:
  * per-tick `ExternalCopy` of all runtime data, *including code modules*, into the isolate;
  * per-tick eager construction of wrappers for every visible object;
  * per-tick deep copy of all constants into the global (`game.js:435`).

  These are runtime-design issues (02 §15b), not engine features.

### 8.2 PERF-COMPILE

* Real bundles are 1.0–1.6 MB single modules:
  * hivemind `dist/main.js` 1,564,753 B;
  * International 1,230,445 B;
  * Overmind esbuild 903,406 B;
  * hivemind's `main.js.map.js` adds 1.1 MB.
* They are compiled on every global reset inside the player's CPU time (ENV-CPU-CLOCK).
* V8 uses lazy parsing: a pre-parse of inner functions plus full compile on first call. chitin needs the same: a fast pre-parse for syntax errors and scope analysis, and lazy bytecode generation per function. Otherwise reset ticks exceed the CPU limit and bots that budget around resets (bucket checks before loading wasm, `bots/starter/rustyscreeps__screeps-starter-rust/js_src/main.js:67`) diverge.
* Cache compiled code per (player, module, code timestamp), as the original does (`runtime-driver.js:64-94`), and share bytecode between players running identical code: e.g. many players running the same open-source bot, or the same lodash.

### 8.3 PERF-CPU-METER (P0 design decision)

* Bots throttle on `Game.cpu.getUsed()`, `Game.cpu.bucket` and `tickLimit` (R=154).
* An interpreter that is 10–50× slower than V8's optimizing JIT would make the same bot "use" 10–50× the CPU, changing its behaviour: skipped tasks, empty bucket, `CPU bucket is empty` skips.
* Options:
  * (a) a real JIT;
  * (b) report CPU in **V8-calibrated units**: instruction/operation counting with per-op costs fitted to V8 timings of real bots. This is also deterministic, which is good for testing and replays;
  * (c) wall-clock.
* The interrupt/back-edge counter of ENV-TERMINATION is the natural place to meter. Decision: §10 OQ-8.

### 8.4 PERF-PROP-ACCESS / PERF-ROOMPOS / PERF-PROXY-STORE

Hot paths in every bot:

* **Engine lazy getters**: `get(){ if(!this['_'+name]) this['_'+name] = fn(dataFn(this.id), this.id); return this['_'+name]; }` (`engine/src/utils.js:516-526`). Each first access adds an own `_name` property to the instance, so shapes evolve per object.
  * Note the truthiness check: falsy values such as `0`, `''` and `false` are recomputed on every access, e.g. `creep.fatigue === 0`. Fast prototype-getter calls plus monomorphic instance-property caches are essential.
  * Inline caches/hidden classes, or an equivalent shape system, are required for this to be fast.
* **RoomPosition** (`new RoomPosition` R=178, 4983 sites): constructor = `roomNameToXY` + bounds checks + `Object.defineProperty(this,'__packedPos',…)`; x/y/roomName are prototype accessors doing bit ops (`rooms.js:1276-1347`). `defineProperty` in a constructor must be fast, or special-cased while staying observably identical.
* **Store Proxy** (`store.js:87-96`): every `creep.store.energy` / `store[RESOURCE_X]` goes through a JS `get` trap plus `target[name]` plus possibly `RESOURCES_ALL.indexOf(name)`, a linear scan of ~85 strings. Proxy `get` with a JS handler must avoid allocation. chitin may special-case this handler shape, but must keep trap semantics if a player wraps or inspects the store.
* **Getter-heavy bot patterns**: class getters R=114 (2002 sites), `Object.defineProperty(X.prototype, …, {get})` R=46 (1192 sites), prototype extensions of `Creep` R=93 and `Room` R=63.

### 8.5 PERF-ENUM / PERF-CLOSURES / PERF-ARRAYS / PERF-MAPSET

* `for-in` R=290 (7.3k sites) and `Object.keys/values/entries` R=188/120/59, over `Game.creeps`, `Game.rooms`, `Memory.*`. Enum caches per shape (V8 "enum cache") make these cheap; dictionary objects with thousands of keys must enumerate in O(n) without re-sorting.
* **lodash callbacks** (`_.filter` R=159, `_.sum` 97, `_.forEach` 56, `_.sortBy` 56, `_.map` 55, `_.min/_.max`, `_.groupBy` 44): closure creation and calls from lodash's generic loops (`baseEach` → `keys` → callback via `bindCallback`). Calls must be cheap, and small closures must avoid heap-allocating environments when no free variables escape.
* **Native array methods** (filter 258, map 218, forEach 201, find, reduce, some): dense-array fast paths. `for-of` over arrays (R=217; Overmind/International dist 500+ sites each) must not allocate iterator result objects in the steady state.
* **Map/Set**: hash tables with insertion-order chains (V8 OrderedHashTable); deletes must not make iteration O(capacity).

### 8.6 PERF-JSON

* Without memhack, `Memory` is fully `JSON.parse`d on first access each tick (`game.js:485`). `JSON.stringify` runs at the end of every tick in which Memory was accessed (`runtime.js:246-248`). That is up to 2 MB each way, per player, per tick.
* With 1000 players, JSON is a top-3 CPU consumer. Requirements:
  * single-pass parsers and serializers;
  * fast-path number formatting;
  * key interning;
  * direct construction of dictionary objects for large objects.
* R=32–38 repos use the **memhack** (`delete global.Memory; global.Memory = lastMemory; RawMemory._parsed = lastMemory;`, `Overmind-dev/src/memory/Memory.ts:94-104`; `hivemind/dist/main.js:39423-39429`) to skip the parse. This needs:
  * `delete` of the configurable `Memory` accessor on the global;
  * ordinary assignment re-creating a data property;
  * `RawMemory` being an extensible null-prototype object (`runtime.js:102`).

### 8.7 PERF-STRINGS / PERF-TYPEDARRAY / PERF-EXCEPTIONS / PERF-WASM / PERF-SOURCEMAP

* **Strings**: incremental concatenation in loops (RoomVisual JSON lines, `console.log` building, hivemind reports) needs a rope or builder representation with lazy flattening. `RoomVisual` appends `JSON.stringify(data)+"\n"` per call into a per-room string capped at 500 KB (`engine/src/game/console.js:57-75`).
* **Typed arrays**: CostMatrix `clone`/`serialize`/`deserialize` per room per tick (R=105) need memcpy-speed typed-array copies and fast `Array.prototype.slice.apply(typedArray)`.
* **Exceptions**: many bots wrap each creep or room in try/catch. Entering `try` must be free. Throwing captures ≤10 frames, but formats lazily.
* **Wasm**: synchronous compile of 0.1–0.3 MB modules on reset is billed CPU. The Rust starter spreads it over ticks and gates it on bucket ≥ 1500 (`js_src/main.js:62-80`). A baseline compiler (V8 Liftoff analogue) is enough for parity; an optimizing tier is optional.
* **Source maps**: on the first error after a reset, ErrorMapper bots `JSON.parse` a ~1 MB map and VLQ-decode it. hivemind warns ">30 CPU" (`hivemind/src/utils/ErrorMapper.ts:30`). This is pure JSON + string/charCode work: another reason for fast JSON and string indexing.

---

## 9. Prioritized feature list (with evidence counts; N = 355 bot repos)

### P0: required to run typical bots

| # | Feature | Evidence |
|---|---|---|
| 1 | ES5.1 core + strict **and** sloppy modes, Annex B `substr`/`__proto__`; native API functions copy their source module's mode (ENV-ENGINE-STRICTNESS) | all repos; `'use strict'` files R=90; substr 77; `__proto__` 16; §2.14 |
| 2 | let/const, arrow, template, classes (+extends/super), accessors, destructuring, spread/rest (incl. object), default params, computed/shorthand, for-of, labels | R=315 / 325 / 257 / 186 / 114 / 125 / 119 / 71 / 185 / 128 / 217 / 26 |
| 3 | Class fields + `static {}` blocks, optional catch, `?.`, `??`, `??=`/`\|\|=`/`&&=`, `**`, `0b/0o`, numeric separators | native in esbuild bundles (hivemind 207 `?.`, International 12 static blocks); TS sources R=101/102/62/21 |
| 4 | Generators | R=21 (TooAngel in-game) |
| 5 | Script origin = module name, +36 col wrapper, V8 stack text and frame naming, stackTraceLimit=10, lazy header | `.stack` R=158; ErrorMapper R=80; source maps R=89 |
| 6 | Exact `Function.prototype.toString` (source slices + native format) | lodash isNative (all lodash users R=250); Overmind self-defence |
| 7 | Backtracking RegExp with spec semantics (g,i,m; lookahead) | regex R=174 |
| 8 | Object/Array/String/Number/Math/JSON per spec, incl. ES2017–2023 methods | keys 188, includes 154, padStart 33, at 17, flat 11, values 120, entries 59 |
| 9 | V8 TimSort, exact | sort R=183; inconsistent comparators R=4 |
| 10 | Shortest round-trip number→string, V8 radix algorithm, exact toFixed/toPrecision | toFixed R=111; radix R=58; toPrecision (hivemind) |
| 11 | Exact JSON.stringify/parse text (well-formed escapes, order, indent) | stringify R=151, parse 78, Memory everywhere |
| 12 | Property order & for-in order identical to V8 | for-in R=290; keys 188 |
| 13 | Map/Set, Symbol (+iterator, toStringTag, toPrimitive), iterator protocol | Set 81, Map 74, Symbol 17, Symbol.iterator 13 |
| 14 | Proxy (engine Store) and Reflect | engine store.js; Proxy R=12; Reflect 19 |
| 15 | Typed arrays (LE), ArrayBuffer (+detach), CostMatrix semantics | Uint8Array 31, Int32Array 20, CostMatrix 105 |
| 16 | `Object.prototype.toString` tags (AsyncFunction/GeneratorFunction/…) | lodash type checks |
| 17 | Math with fdlibm-identical results; `Math.random` uniform 52-bit | Math R=231; random 127 |
| 18 | Error types, toString, own props; `globalThis` + `global` | Error 170; global 184 |
| 19 | Exactly-V8 global set; absent web/Node globals; console replaced | console.error/warn R=58; polyfills in wasm bots |
| 20 | Persistent heaps, heap limit + `getHeapStatistics` shape, uncatchable CPU termination, CPU clock | getHeapStatistics R=31; halt 12; getUsed 154 |
| 21 | Host-object fidelity (descriptors, order, instanceof) | profilers R≈15 (`screeps-profiler` dep), prototype patching R=93 |
| 22 | PERF: ICs/shapes, fast getters, Proxy get, enum caches, closures, JSON, lazy compile, per-player GC, cheap VM creation, CPU metering | §8 |

### P1: needed for a notable minority, or for exact output parity

| # | Feature | Evidence |
|---|---|---|
| 1 | async/await + Promise with ENV-MICROTASKS timing; unhandled rejection fails the run | async R=24 (mostly tooling), Promise 11 |
| 2 | Direct eval, `Function` constructor | eval R=14, Function 7/5 |
| 3 | WebAssembly sync API (+i64/BigInt boundary minimal), `DataView` | WebAssembly R=11 + 4 shipping .wasm; DataView 2 |
| 4 | WeakMap/WeakSet | 8 / 2; lodash metaMap |
| 5 | ES2018 regex: lookbehind, named groups, `s`, `u`, `y`, `\p{}` | u 5, s 4, y 2, named 1 |
| 6 | Tagged templates + `String.raw` | 9 / 4 |
| 7 | `Error.captureStackTrace` / `stackTraceLimit` writes / `prepareStackTrace` + CallSite API; isolated-vm timeout stack rendering | 2/2/2 + wasm glue |
| 8 | V8 error message templates | console parity |
| 9 | Intl subset: en-US `toLocaleString`, root-collation `localeCompare`, basic `DateTimeFormat` | 14 / 13 / 3 |
| 10 | Date parsing (ISO + V8 legacy formats), local-time (UTC), `toString` format | Date 70, getHours 11 |
| 11 | Full Unicode case mapping, `normalize`, code-point APIs | fromCodePoint 14, codePointAt 11 |
| 12 | URI functions | encodeURIComponent 18, decode 8 (+ obfuscator decoder) |
| 13 | `new.target`, `debugger` no-op, top-level `return` | 1 / 3 / wrapper |
| 14 | Function own-prop set, `.caller`/`.arguments` accessors | screeps-profiler |
| 15 | Binary module = Uint8Array subclass (buffer@4.9.1 shape) | wasm loaders R=7 |
| 16 | PERF: strings (ropes), Map/Set tables, exceptions, wasm baseline compiler, source-map decode speed | §8.7 |

### P2: rare, or for full V8 parity

* BigInt and BigInt64 arrays: R=2, wasm hosts.
* Async iteration / for-await: R=1.
* Private class members: R=1.
* `with`: R=1, TS only.
* RegExp `d`/`v` flags.
* WeakRef/FinalizationRegistry: R=0.
* Atomics (SharedArrayBuffer is left out, §5.15).
* ES2024+ library additions: groupBy, Set methods, iterator helpers, `Promise.withResolvers`, `Array.fromAsync` (resizable buffers are left out, §5.15).
* `escape`/`unescape`.
* Full ICU/CLDR + IANA time zones.
* `WebAssembly.instantiate` (async) and wasm SIMD/EH/GC.
* `_.template` sourceURL frame naming.
* Wasm stack frames.
* ESM entry (`main.mjs`): official = none, xxscreeps = yes; a mod option.

---

## 10. Open questions

1. **OQ-1, exact V8 version to pin.**
   * The reference supports Node ≥ 22. The official MMO's Node/V8 version is unknown, and it may still run an older isolated-vm.
   * The V8-version-dependent items:
     * error message texts (JSON.parse messages changed format in V8 10–12);
     * the `Error.stack` descriptor shape;
     * whether sloppy functions have own `arguments`/`caller`;
     * ES2024/2025 library availability;
     * `Iterator` global.
   * Proposal: pin Node 22.x / V8 12.4 and re-run the [probe] vectors on it.
2. **OQ-2, message texts.** V8 12.4 vs 14 differences for `JSON.parse` and `is not a function` callee rendering. The [probe] used V8 14. Verify on Node 22.
3. **OQ-3, timezone and locale** of the official runner processes (local-time Date APIs, `toLocaleString()` default, `Date#toString` zone name). Proposal: UTC + en-US.
4. **OQ-4, `stack` property shape.** V8 14 shows an own get/set accessor. Older V8 used a data property. This is observable via `getOwnPropertyDescriptor`, but no corpus bot inspects it.
5. **OQ-5, function own properties** on V8 12.4: whether sloppy functions expose own `arguments`/`caller` (V8 14: they do not).
6. **OQ-6, runtime frames below `__mainLoop:2:3`.** The official text would be `Object.exports.evalCode (<runtime>:…)` and similar, depending on the webpack bundle. It is unknowable without the built bundle and irrelevant to mappers, but visible in console errors. Proposal: emit nothing below the `__mainLoop` frames, then apply 02 CON-ERR-FORMAT.
7. **OQ-7, engine API functions' `toString` and `_.isNative(Creep.prototype.move)`.** These are JS source in the original and `[native code]` if chitin implements them natively. No bot found depends on this. Decide whether to keep a JS shim for source text. Whichever way this is decided, a native implementation must still copy its source module's strictness (§2.14 ENV-ENGINE-STRICTNESS).
8. **OQ-8, CPU model.** Wall-clock (honest, nondeterministic) vs V8-calibrated op counting (deterministic, parity of throttling behaviour) vs a JIT. This affects bucket dynamics for every bot and needs a calibration corpus (e.g. hivemind, Overmind, International, TooAngel).
9. **OQ-9, microtask kAuto detail.** isolated-vm leaves V8's policy at kAuto, so promise jobs may also run when the call depth returns to 0 right after the `run` reference returns, i.e. before `TaskEpilogue`. Both points are after output capture, so the observable behaviour is the same. Confirm empirically that no checkpoint happens inside `runSync` with timeout (the code says no for recursive calls).
10. **OQ-10, dynamic `import()` inside the wrapper.** It is syntactically valid in a script. With no host import hook, V8 rejects the promise ("Not supported"), and that rejection would then fail the tick (ENV-MICROTASKS). Verify, then replicate.
11. **OQ-11, wasm feature set** that current rustc/wasm-bindgen and .NET WASI outputs require (bulk-memory, reference-types and multi-value are assumed). A scan of the shipped `.wasm` files' feature sections was not done.
12. **OQ-12, does the official MMO use the same `console.js`** (no `console.error`)? Evidence that it does: the Rust starter's comment about re-installing `console.error` each tick on the official server.
13. **OQ-13, frame-naming algorithm source.** §6.1's pseudo-code is derived from 60+ [probe] vectors on V8 14, not from V8 source.
    * Port `CallSiteInfo::GetTypeName/GetMethodName/GetFunctionName`, `AppendMethodCall` and `JSReceiver::GetConstructorName` from the pinned V8 (OQ-1).
    * Re-run the vectors. Known sensitive spots: the IdentifierName test for the type prefix, the "last dot segment" rule for `[as x]`, and constructor-name precedence.
