// WP02 differential suite (ARCHITECTURE.md §12.3). Runs the official sanitizers and storeIntents
// (reference/common/lib/system.js, engine/src/utils.js, the intentsCount of driver/lib/runtime/make.js) and ours
// (crates/runtime/js/common/system.js, host/system.js __store) on the same inputs, each case in fresh node vm
// realms, and exits 1 on any difference. Then writes the Rust fixtures:
//   crates/engine/tests/fixtures/system/sanitize.ndjson  JSON input → official sanitizeUserIntents (engine::system)
//   crates/engine/tests/fixtures/system/store.ndjson     world line, then our __store output → the UserIntents the
//                                                        official storeIntents result routes to (engine::intents)
// JS sanitizer cases hold values of every kind (undefined, NaN, ±Infinity, -0, BigInt, Date, wrappers, symbols,
// functions, proxies, getters and toString/valueOf/Symbol.toPrimitive traps that log their calls); compared:
// return value, the mutated room result, Object.prototype pollution, the call log and the thrown error.
// __store cases: outMessages cloned with v8.serialize (V8's ValueSerializer, as isolated-vm's ExternalCopy and
// our host) and rebuilt in each realm; ours also runs every case in one long-lived realm (pollution undone).
// Usage: node tools/oracle/gen/system-fixture.js
'use strict';
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const v8 = require('v8');
const { ROOT, reference, prng, writeFixture } = require('../lib/common');

const JS = path.join(ROOT, 'crates', 'runtime', 'js');
const g = prng(0x5157e3);
let failures = 0;

// ---------------------------------------------------------------------------------------------- realms

const scripts = new Map();
function script(file) {
  if (!scripts.has(file)) {
    const src = fs.readFileSync(file, 'utf8');
    scripts.set(file, new vm.Script(`(function (exports, require, module) {${src}\n})`, { filename: file }));
  }
  return scripts.get(file);
}

// A fresh realm with a CommonJS loader: `files` maps a require string to a path, `extra` to a module factory.
function realm(files, extra = {}) {
  const ctx = vm.createContext({});
  const run = (code) => vm.runInContext(code, ctx);
  run('globalThis.global = globalThis;');
  const cache = {};
  const require = (req) => {
    if (req in cache) return cache[req].exports;
    if (req in extra) return extra[req]();
    if (!files[req]) throw new Error(`no module ${req}`);
    const module = (cache[req] = run('({exports: {}})'));
    script(files[req]).runInContext(ctx).call(module.exports, module.exports, require, module);
    return module.exports;
  };
  return { ctx, run, require };
}

const MAKE = fs.readFileSync(reference('driver/lib/runtime/make.js'), 'utf8');
const INTENTS_COUNT = /const intentsCount = (function\(intentsList\) \{[\s\S]*?\r?\n\})\r?\n/.exec(MAKE)[1];

function official() {
  const r = realm(
    {
      lodash: reference('lodash-3.10.1/lodash.js'),
      './constants': reference('common/lib/constants.js'),
      system: reference('common/lib/system.js'),
      utils: reference('engine/src/utils.js'),
    },
    { '~runtime-driver': () => ({ system: r.require('system'), constants: r.require('./constants') }) },
  );
  r.system = r.require('system');
  r.utils = r.require('utils');
  r.intentsCount = r.run(`(${INTENTS_COUNT})`);
  return r;
}

function ours() {
  const r = realm({
    lodash: path.join(JS, 'vendor', 'lodash.js'),
    // stand-in for the bundle's common/constants (engine::constants::constants_json, byte-identical)
    './constants': reference('common/lib/constants.js'),
    '../common/system': path.join(JS, 'common', 'system.js'),
    host: path.join(JS, 'host', 'system.js'),
  });
  r.system = r.require('../common/system');
  r.require('host');
  return r;
}

// ---------------------------------------------------------------------------------------------- comparison

// Realm-independent description of any value (identity as #n back-references, getters not invoked).
function describe(v, seen = new Map()) {
  switch (typeof v) {
    case 'undefined': return 'undefined';
    case 'number': return Object.is(v, -0) ? '-0' : String(v);
    case 'bigint': return `${v}n`;
    case 'string': return JSON.stringify(v);
    case 'boolean': return String(v);
    case 'symbol': return v.toString();
    case 'function': return `function ${v.name}`;
  }
  if (v === null) return 'null';
  if (seen.has(v)) return `#${seen.get(v)}`;
  seen.set(v, seen.size);
  const tag = Object.prototype.toString.call(v);
  let head = Object.getPrototypeOf(v) === null ? 'null-proto' : tag;
  if (tag === '[object Date]') head += Date.prototype.getTime.call(v);
  const props = Reflect.ownKeys(v).map((k) => {
    const d = Object.getOwnPropertyDescriptor(v, k);
    return `${typeof k === 'symbol' ? k.toString() : JSON.stringify(k)}:${'value' in d ? describe(d.value, seen) : 'accessor'}`;
  });
  return `${head}{${props.join(',')}}`;
}

const errText = (e) => (e !== null && typeof e === 'object' ? `${e.name}: ${e.message}` : `thrown ${describe(e)}`);

function check(what, a, b, detail) {
  if (a === b) return true;
  if (failures++ < 10) console.error(`MISMATCH ${what}\n  official: ${a}\n  ours:     ${b}\n  case: ${detail}`);
  return false;
}

// ---------------------------------------------------------------------------------------------- JS sanitizer cases

const SCHEMAS = (() => {
  const r = official();
  return JSON.parse(r.run('JSON.stringify')(r.run(`(${fs.readFileSync(reference('common/lib/system.js'), 'utf8')
    .match(/const intentTypes = (\{[\s\S]*?\r?\n\});/)[1]})`)));
})();
const NAMES = Object.keys(SCHEMAS);
const ROOM_NAMES = ['createConstructionSite', 'createFlag', 'destroyStructure', 'removeConstructionSite', 'removeFlag'];
const CUSTOM_SRC = `{myIntent: {a: 'string', b: 'price', c: 'number[]', d: 'bodypart[]', e: 'userText', f: 'boolean',
  g: 'userString', h: 'string[]', i: 'number'}, move: {q: 'string'}}`;
const CUSTOM_FIELDS = 'abcdefghi'.split('');
const SCHEMA = (name) => (SCHEMAS[name] ? Object.keys(SCHEMAS[name]) : name === 'myIntent' ? CUSTOM_FIELDS : []);

const PRIMS = ['undefined', 'null', 'NaN', 'Infinity', '-Infinity', '-0', '0', '1', '7', '42', '1.5', '-1.5', '-0.5',
  '0.0004', '0.0005', '1.0005', '1e21', '-1e21', '5e-7', '1e300', '123456.7895', '2 ** 53 + 2', '0.1 + 0.2', 'true',
  'false', '10n', '0n', '-5n', '2n ** 70n', '""', '"0"', '"-0"', '"12abc"', '" 0x1F"', '"  -12.9e3"', '"Infinity"',
  '"1e1000"', '"0b11"', '"move"', '"work"', '"carry"', '"tough"', '"MOVE"', '"energy"', '"W1N1"', '"é€😀"', '"\\ud83d"',
  `"${'x'.repeat(99)}😀"`, `"${'x'.repeat(999)}😀"`, `"${'é'.repeat(150)}"`, `"${'y'.repeat(1200)}"`, '" "',
  '"\\u00a07"', '"null"', '"true"'];
const OBJS = ['[]', '[1, "2", null, undefined, 3.7]', '["move", "work", "x", "carry", null, "heal", "claim"]',
  '[["move"], 1]', '[, "move"]', '[[1, 2], [3]]', '{}', '{length: 2, 0: "move", 1: "tough", 2: "work"}',
  '{length: "2", 0: "move"}', '{a: "heal", b: 3, length: 1.5}', '{length: -0, 0: "move"}',
  '{0: "work", length: 1e300}', '{length: "move"}', '{toString: "x"}', '{valueOf: 1}', '[{toString: 1}]',
  'new Date(0)', 'new Date(NaN)', 'new Number(5)', 'new String("move")', 'new Boolean(false)', 'Object(3n)',
  'Object.create(null)', '/re/g', 'Symbol("s")', 'function f() {}', 'Object.assign(["move", "work"], {extra: 1})',
  '{toString() { log.push("toString"); return "21.9"; }}',
  '{valueOf() { log.push("valueOf"); return 4.2; }, toString() { log.push("toString"); return "x"; }}',
  '{[Symbol.toPrimitive](hint) { log.push("prim:" + hint); return hint === "number" ? 2.6 : "3.3"; }}',
  '{toString() { log.push("throw"); throw new RangeError("trap"); }}',
  '{valueOf() { return {}; }, toString() { return {}; }}',
  'new Proxy(["move", "claim"], {get(t, k, r) { log.push("proxy:" + String(k)); return Reflect.get(t, k, r); }})',
  '{get length() { log.push("len"); return 2; }, 0: "move", get 1() { log.push("1"); return "work"; }}'];

const value = () => (g.chance(0.6) ? g.pick(PRIMS) : g.pick(OBJS));
const prop = (k, v, tag = k) =>
  g.chance(0.15) ? `get ${JSON.stringify(k)}() { log.push("get ${tag}"); return ${v}; }` : `${JSON.stringify(k)}: ${v}`;

// An intent object literal: some schema fields, extra fields, sometimes fields on its prototype.
function intent(name, roomField) {
  if (g.chance(0.08)) return value();
  const parts = [];
  for (const f of SCHEMA(name)) {
    if (f === 'roomName' && roomField) parts.push(prop(f, roomField()));
    else if (g.chance(0.7)) parts.push(prop(f, value()));
  }
  if (g.chance(0.2)) parts.push(prop(g.pick(['zzz', 'room', 'roomName', 'id']), value()));
  if (g.chance(0.05)) parts.push('["__proto__"]: 1');
  if (g.chance(0.05)) parts.push(`__proto__: {${SCHEMA(name).length ? SCHEMA(name)[0] : 'id'}: "inherited"}`);
  for (let i = parts.length - 1; i > 0; i--) {
    const j = g.int(i + 1);
    [parts[i], parts[j]] = [parts[j], parts[i]];
  }
  return `{${parts.join(', ')}}`;
}

const intentList = (name, roomField) =>
  `[${Array.from({ length: g.int(4) }, () => (g.chance(0.06) ? g.pick(['null', 'undefined']) : intent(name, roomField))).join(', ')}]`;

function input(names, roomField, iterable) {
  const parts = names.map((name) => {
    const r = g.rand();
    const v = iterable
      ? r < 0.85 ? intentList(name, roomField) : r < 0.9 ? '"ab"' : r < 0.95 ? intent(name, roomField) : g.pick(['0', '""', 'null'])
      : r < 0.6 ? intent(name, roomField) : r < 0.9 ? intentList(name, roomField) : g.pick(['0', '""', 'null', 'false', 'NaN']);
    return prop(name, v);
  });
  return `{${parts.join(', ')}}`;
}

const ROOMS = ['"W1N1"', '"W2N3"', '"__proto__"', '"constructor"', '"toString"', '"hasOwnProperty"', '"notify"',
  '"global"', '"5"', '"0"', '"room"', 'undefined', '1e21', 'null', '{toString() { log.push("room"); return "W1N1"; }}'];
const RESULTS = ['{}', '{}', '{W1N1: {abc: {move: {id: "x"}}}, notify: [{message: "m"}], global: {cancelOrder: [{orderId: "o"}]}}',
  '{W1N1: {room: {createFlag: [{x: 1}]}}, W2N3: {}}'];

function userCase() {
  const pool = NAMES.concat(['myIntent', 'unknownIntent', 'constructor', 'toString', 'room']);
  const names = Array.from({ length: 1 + g.int(4) }, () => g.pick(pool));
  const custom = g.chance(0.5) ? CUSTOM_SRC : 'undefined';
  return `{op: "user", input: ${input([...new Set(names)], null, false)}, custom: ${custom}}`;
}

function roomCase() {
  const pool = ROOM_NAMES.concat(ROOM_NAMES, ['myIntent', 'unknownIntent', 'move', 'notify']);
  const names = Array.from({ length: 1 + g.int(3) }, () => g.pick(pool));
  const custom = g.chance(0.4) ? CUSTOM_SRC : 'undefined';
  const roomField = () => (g.chance(0.6) ? g.pick(ROOMS.slice(0, 2)) : g.chance(0.5) ? g.pick(ROOMS) : value());
  return `{op: "room", input: ${input([...new Set(names)], roomField, true)}, result: ${g.pick(RESULTS)}, custom: ${custom}}`;
}

const POLLUTION = '[Object.prototype.room, Object.room, Object.prototype.toString.room, Object.prototype.hasOwnProperty.room]';

function runDirect(r, src) {
  const log = r.run('[]');
  const c = r.run(`(function (log) { return ${src}; })`)(log);
  let ret, err;
  try {
    ret = c.op === 'user' ? r.system.sanitizeUserIntents(c.input, c.custom) : r.system.sanitizeUserRoomIntents(c.input, c.result, c.custom);
  } catch (e) {
    err = errText(e);
  }
  return describe({ ret, err, result: c.result, log: Array.from(log), polluted: r.run(POLLUTION) });
}

let jsCases = 0;
for (let i = 0; i < 2500; i++) {
  const src = i % 5 < 3 ? userCase() : roomCase();
  check('sanitizer', runDirect(official(), src), runDirect(ours(), src), src);
  jsCases++;
}
console.log(`JS sanitizer: ${jsCases} cases`);

// ---------------------------------------------------------------------------------------------- __store cases

const USER = 'a1b2c3d4e5f6a7b';
// Fixture world: [name, removed]; RoomIdx = position.
const WORLD = [['W1N1', false], ['W2N1', false], ['W1N2', false], ['E0S0', false], ['E5S5', false], ['W9N9', true]];
const META_INDEX = new Map(WORLD.map(([n, removed], i) => [n, removed ? -1 : i]).filter(([, i]) => i >= 0));
const OBJECT_ROOMS = ['W1N1', 'W2N1', 'W1N2', 'E0S0', 'E5S5', 'W9N9', 'W3N3'];
const STORE_ROOMS = OBJECT_ROOMS.concat(['undefined', '__proto__', 'constructor', 'toString', 'notify', 'global', '5',
  'w1n1', 'W01N1']);
const INHERITED = ['constructor', 'toString', '__proto__', 'hasOwnProperty', 'valueOf'];
const OBJECT_INTENTS = NAMES.filter((n) => !ROOM_NAMES.includes(n) && !['notify', 'createOrder', 'deal', 'cancelOrder'].includes(n));
const GLOBAL_INTENTS = ['createOrder', 'deal', 'cancelOrder', 'changeOrderPrice', 'extendOrder', 'createPowerCreep',
  'deletePowerCreep', 'renamePowerCreep', 'spawnPowerCreep', 'suicidePowerCreep', 'upgradePowerCreep'];

const hex = (n) => Array.from({ length: n }, () => '0123456789abcdef'[g.int(16)]).join('');
const STR_VALUES = ['', '0', '12abc', ' 0x1F', 'move', 'work', 'energy', 'W1N1', 'é€😀', '\ud83d', 'x'.repeat(99) + '😀',
  'y'.repeat(1100), 'Infinity'];
const NUM_VALUES = [NaN, Infinity, -Infinity, -0, 0, 1, 7, 1.5, -0.5, 0.0004, 1e21, 5e-7, 123456.7895];

function data(depth = 0) {
  const r = g.rand();
  if (r < 0.1) return undefined;
  if (r < 0.15) return null;
  if (r < 0.2) return g.chance(0.5);
  if (r < 0.45) return g.pick(NUM_VALUES);
  if (r < 0.7) return g.pick(STR_VALUES);
  if (r < 0.75) return g.pick([10n, 0n, -3n]);
  if (r < 0.8) return g.pick([new Date(0), new Date(NaN), new Number(5), new String('ab'), new Boolean(false), Object(2n), /x/g]);
  if (depth > 1 || r < 0.9) return g.chance(0.5) ? ['move', 'work', 'x', 3.5, null] : [1, '2', [3]];
  const o = { length: 1, 0: 'carry', a: data(depth + 1) };
  if (g.chance(0.15)) o.toString = 'x'; // an own toString of any value makes ToPrimitive throw
  return o;
}

// An intent data object as the engine builds it: schema fields with player-supplied values.
function intentData(name, room) {
  const o = {};
  for (const f of SCHEMA(name)) if (g.chance(0.85)) o[f] = f === 'roomName' && room ? room() : data();
  if (g.chance(0.1)) o.extra = data();
  return o;
}

function storeCase() {
  const owned = new Map();
  const visible = new Map();
  const list = Object.create(null);
  const keys = [];
  const n = 1 + g.int(6);
  for (let i = 0; i < n; i++) {
    const r = g.rand();
    if (r < 0.1) keys.push('room');
    else if (r < 0.17) keys.push('global');
    else if (r < 0.24) keys.push('notify');
    else if (r < 0.3) keys.push(g.pick(INHERITED));
    else if (r < 0.33) keys.push(String(g.int(100)));
    else {
      const id = hex(15);
      const room = g.pick(OBJECT_ROOMS);
      if (r < 0.75) owned.set(id, room);
      else if (r < 0.92) visible.set(id, room);
      keys.push(id);
    }
  }
  const roomName = () => (g.chance(0.75) ? g.pick(STORE_ROOMS.slice(0, 7)) : g.chance(0.8) ? g.pick(STORE_ROOMS) : data());
  for (const k of keys) {
    if (k === 'room') {
      list.room = Object.create(null);
      for (const name of new Set(Array.from({ length: 1 + g.int(3) }, () => g.pick(ROOM_NAMES.concat(['unknownIntent', 'myIntent']))))) {
        list.room[name] = Array.from({ length: 1 + g.int(3) }, () => intentData(name, roomName));
      }
    } else if (k === 'global') {
      list.global = Object.create(null);
      for (const name of new Set(Array.from({ length: 1 + g.int(3) }, () => g.pick(GLOBAL_INTENTS.concat(['unknownIntent']))))) {
        list.global[name] = Array.from({ length: 1 + g.int(3) }, () => intentData(name));
        if (name === 'createOrder' && g.chance(0.85)) for (const o of list.global[name]) if (typeof o.price === 'bigint') o.price = 1.5;
      }
    } else if (k === 'notify') {
      list.notify = Array.from({ length: g.int(25) }, () => ({ message: data(), groupInterval: data() }));
    } else {
      const o = Object.create(null);
      for (const name of new Set(Array.from({ length: 1 + g.int(4) }, () => g.pick(OBJECT_INTENTS.concat(['say', 'pull', 'unknown', 'constructor', 'myIntent']))))) {
        o[name] = g.chance(0.05) ? [intentData(name), intentData(name)] : intentData(name);
      }
      list[k] = o;
    }
  }
  const out = Object.create(null);
  out.type = g.chance(0.8) ? 'done' : 'error';
  if (out.type === 'error') out.error = 'Error: boom\n    at main:1:1 é';
  out.usedTime = g.int(20);
  out.usedCleanTime = g.rand() * 10;
  out.usedDirtyTime = g.int(30);
  out.intentsList = list;
  out.intentsCpu = 0.2 * keys.length;
  out.memory = { data: '{"a":1}', userId: USER };
  out.console = { log: g.chance(0.5) ? ['hello é😀', '<b>x</b>'] : [], results: g.chance(0.2) ? ['1'] : [] };
  if (g.chance(0.3)) out.visual = { W1N1: 'v1', map: 'm' };
  out.activeSegments = g.pick([undefined, [1, 2], []]);
  out.activeForeignSegment = g.pick([undefined, null, { username: 'bob', id: 3 }, { username: 'é', id: undefined }]);
  out.defaultPublicSegment = g.pick([undefined, null, 5]);
  if (g.chance(0.3)) out.publicSegments = '1,2';
  if (g.chance(0.3)) out.memorySegments = { 3: 'seg' };
  return { out, owned, visible, custom: g.chance(0.3) };
}

// Re-creates a ValueSerializer copy inside realm `r` (key order, own "__proto__" keys, wrappers, holes).
function rebuild(v, C, seen = new Map()) {
  if (v === null || typeof v !== 'object') return v;
  if (seen.has(v)) return seen.get(v);
  const tag = Object.prototype.toString.call(v);
  const o = {
    '[object Date]': () => new C.Date(v.getTime()),
    '[object Number]': () => C.Object(Number.prototype.valueOf.call(v)),
    '[object String]': () => C.Object(String.prototype.valueOf.call(v)),
    '[object Boolean]': () => C.Object(Boolean.prototype.valueOf.call(v)),
    '[object BigInt]': () => C.Object(BigInt.prototype.valueOf.call(v)),
    '[object RegExp]': () => new C.RegExp(v.source, v.flags),
    '[object Array]': () => new C.Array(v.length),
    '[object Object]': () => new C.Object(),
  }[tag]();
  seen.set(v, o);
  if (tag !== '[object String]') {
    for (const k of Object.keys(v)) {
      C.Object.defineProperty(o, k, { value: rebuild(v[k], C, seen), enumerable: true, writable: true, configurable: true });
    }
  }
  return o;
}

const realmCtors = (r) => r.run('({Object, Array, Date, RegExp})');

function officialStore(c, clone) {
  const r = official();
  const o = rebuild(clone, realmCtors(r));
  const docs = (m) => Object.fromEntries([...m].map(([id, room]) => [id, { _id: id, room }]));
  // data.js: userObjects = mapById(own docs); roomObjects = _.extend(mapById(room docs), userObjects)
  const data = r.run(`(${JSON.stringify({ userObjects: docs(c.owned), roomObjects: { ...docs(c.visible), ...docs(c.owned) } })})`);
  try {
    const intents = r.utils.storeIntents(USER, o.intentsList, data, r.run(c.custom ? `(${CUSTOM_SRC})` : '({})'));
    return { intents: r.run('JSON').stringify(intents), count: r.intentsCount(o.intentsList) };
  } catch (e) {
    return { err: errText(e) };
  }
}

function ourStore(r, c, clone) {
  r.ctx.objectRoom = (id) => (c.owned.has(id) ? c.owned.get(id) : c.visible.get(id));
  r.ctx.__setCustomIntentTypes(r.run(c.custom ? `(${CUSTOM_SRC})` : '({})'));
  try {
    const s = r.ctx.__store(rebuild(clone, realmCtors(r)), USER);
    if (!/^[\x00-\x7f]*$/.test(s)) throw new Error('__store output is not ASCII');
    return { s, p: JSON.parse(s) };
  } catch (e) {
    return { err: errText(e) };
  }
}

const shared = ours();
// The source tag is what tools/parity-report.js counts (never "<dir>/utils.js": WP05/WP09 own those mirrors).
const storeLines = [{ source: 'common/lib/system.js via storeIntents + intentsCount', rooms: WORLD }];
let storeCases = 0;
let storeErrors = 0;
for (let i = 0; i < 2000; i++) {
  const c = storeCase();
  let clone;
  try {
    clone = v8.deserialize(v8.serialize(c.out));
  } catch (e) {
    continue; // DataCloneError: a total failure before __store
  }
  const want = officialStore(c, clone);
  const detail = `store case ${i}`;
  for (const got of [ourStore(ours(), c, clone), ourStore(shared, c, clone)]) {
    if (want.err || got.err) {
      check('store error', want.err, got.err, detail);
      continue;
    }
    check('store intents', want.intents, JSON.stringify(got.p.intents), detail);
    check('store count', want.count, got.p.count, detail);
    for (const f of ['type', 'usedTime', 'usedCleanTime', 'usedDirtyTime', 'console', 'visual', 'activeSegments',
      'activeForeignSegment', 'defaultPublicSegment', 'publicSegments']) {
      check(`store ${f}`, JSON.stringify(clone[f]), JSON.stringify(got.p[f]), detail);
    }
    check('store error field', JSON.stringify(clone.error === undefined ? null : clone.error), JSON.stringify(got.p.error), detail);
  }
  storeCases++;
  if (want.err) {
    storeErrors++;
    continue;
  }
  // saveUserIntents split (driver/lib/index.js:180-243) with rooms that are not world rooms dropped.
  const intents = JSON.parse(want.intents);
  const expect = { rooms: [], global: null, notify: [], count: want.count };
  for (const k of Object.keys(intents)) {
    if (k === 'notify') expect.notify = Array.isArray(intents.notify) ? intents.notify : [];
    else if (k === 'global') expect.global = intents.global;
    else if (META_INDEX.has(k)) expect.rooms.push([META_INDEX.get(k), Object.keys(intents[k]).map((id) => [id, intents[k][id]])]);
  }
  storeLines.push({ out: ourStore(ours(), c, clone).s, expect });
}
console.log(`__store: ${storeCases} cases (${storeErrors} throwing)`);

// ---------------------------------------------------------------------------------------------- Rust JSON fixtures

const JSON_VALUES = ['0', '-0', '1', '7', '1.5', '-1.5', '-0.5', '0.0004', '0.0005', '1.0005', '1e21', '-1e21', '1e-7',
  '5e-7', '1e300', '123456.7895', '9007199254740993', '4294967296.5', '2.675', '1.005', '100.5', '-2.5', 'true', 'false',
  'null', '""', '"0"', '"-0"', '"12abc"', '" 0x1F"', '"  -12.9e3"', '"Infinity"', '"-Infinity"', '"1e1000"', '"0b11"',
  '"0o7"', '"move"', '"work"', '"carry"', '"tough"', '"claim"', '"ranged_attack"', '"MOVE"', '"W1N1"', '"é€😀"',
  '"\\ud83d"', '"\\ud83d\\ude00"', `"${'x'.repeat(99)}😀"`, `"${'x'.repeat(999)}😀"`, `"${'é'.repeat(150)}"`,
  `"${'y'.repeat(1200)}"`, '"\\u00a07"', '"\\n5"', '"null"', '"[object Object]"', '[]', '[1,"2",null,3.7]',
  '["move","work","x","carry",null,"heal","claim"]', '[["move"],1]', '[[1,2],[3]]', '[{}]', '[true,false]', '{}',
  '{"length":2,"0":"move","1":"tough","2":"work"}', '{"length":"2","0":"move"}', '{"a":"heal","b":3,"length":1.5}',
  '{"length":-0,"0":"move"}', '{"0":"work","length":1e300}', '{"length":"move"}', '{"b":"work","a":"carry","1":"heal","0":"move"}',
  '{"__proto__":"move"}', '{"toString":"x"}', '{"valueOf":1}', '[{"toString":1}]', '{"length":3,"2":"heal","5":"move"}'];

function jsonIntent(name) {
  if (g.chance(0.08)) return g.pick(JSON_VALUES);
  const parts = [];
  for (const f of SCHEMA(name)) if (g.chance(0.75)) parts.push(`${JSON.stringify(f)}:${g.pick(JSON_VALUES)}`);
  if (g.chance(0.2)) parts.push(`"zzz":${g.pick(JSON_VALUES)}`);
  return `{${parts.join(',')}}`;
}

const sanitizeLines = [{ source: 'common/lib/system.js sanitizeUserIntents' }];
const r = official();
const parse = r.run('JSON.parse');
const customs = { true: r.run(`(${CUSTOM_SRC})`), false: undefined };
for (let i = 0; i < 4000; i++) {
  const pool = NAMES.concat(['myIntent', 'unknownIntent', 'constructor']);
  const names = [...new Set(Array.from({ length: 1 + (i % 3 ? 0 : g.int(4)) }, () => g.pick(pool)))];
  const text = `{${names.map((n) => {
    const v = g.rand() < 0.6 ? jsonIntent(n) : `[${Array.from({ length: g.int(3) }, () => (g.chance(0.05) ? 'null' : jsonIntent(n))).join(',')}]`;
    return `${JSON.stringify(n)}:${g.chance(0.05) ? g.pick(['0', '""', 'null', 'false']) : v}`;
  }).join(',')}}`;
  const custom = g.chance(0.4);
  const line = { in: text, custom };
  try {
    line.out = r.run('JSON').stringify(r.system.sanitizeUserIntents(parse(text), customs[custom]));
  } catch (e) {
    line.err = errText(e);
  }
  sanitizeLines.push(line);
}

// Deterministic sweep: every value through every transform (custom myIntent holds all nine; the built-ins cover
// them without customIntentTypes), as a plain intent and inside an array; then whole inputs that are not objects.
const EXTRA_JSON_VALUES = ['"\\u30007"', '"\\ufeff7"', '"\\u180e7"', '"\\u20287"', '"\\u2029-7"', '"+5"', '".5"', '"5."',
  '"-0x10"', '"0x"', '"0X1g"', '"1_000"', '"infinity"', '"+Infinity"', '" \\t\\r\\n"', '"\\u00a0"', '999999999999999900000',
  '123456789012345680000', '1e-6', '0.000001', '-1e-7', '1.7976931348623157e308', '5e-324', '-5e-324', '0.5', '1.5e300',
  '[[[]]]', '[[null],[]]', '[-0,1e21]', '[" 7",{"valueOf":1}]', '{"length":1,"0":null}', '{"length":2,"1":"move","0":"work"}',
  '{"length":0,"0":"move"}', '{"length":"1","1":"heal"}', '{"length":null,"0":"move"}', '{"constructor":{"prototype":1},"0":"move"}',
  '{"01":"move","1":"claim","length":2}', '{"move":"work"}', '{"a":{"toString":1}}', '[1,[{"toString":null}]]'];
const ALL = JSON_VALUES.concat(EXTRA_JSON_VALUES);
const fields = (keys, v) => `{${keys.map((k) => `${JSON.stringify(k)}:${v}`).join(',')}}`;
const sweep = [];
for (const v of ALL) {
  const custom = fields(CUSTOM_FIELDS, v);
  sweep.push([`{"myIntent":${custom}}`, true], [`{"myIntent":[${custom},{}]}`, true]);
  sweep.push([`{"createCreep":${fields(['name', 'body', 'energyStructures', 'directions'], v)},` +
    `"notify":${fields(['message', 'groupInterval'], v)},"changeOrderPrice":${fields(['orderId', 'newPrice'], v)},` +
    `"say":${fields(['message', 'isPublic'], v)}}`, false]);
}
for (const v of ['null', '0', '""', 'false', 'true', '5', '"move"', '[]', '[{"move":{}}]', '{"__proto__":{"move":{"id":"x"}}}',
  '{"move":null,"suicide":[null]}', '{"say":[{},null]}']) {
  sweep.push([v, false], [v, true]);
}
for (const [text, custom] of sweep) {
  const line = { in: text, custom };
  try {
    line.out = r.run('JSON').stringify(r.system.sanitizeUserIntents(parse(text), customs[custom]));
  } catch (e) {
    line.err = errText(e);
  }
  sanitizeLines.push(line);
}
console.log(`Rust sanitizer fixture: ${sanitizeLines.length - 1} cases`);

if (failures) {
  console.error(`${failures} mismatches; fixtures not written`);
  process.exit(1);
}
writeFixture('system/sanitize.ndjson', sanitizeLines);
writeFixture('system/store.ndjson', storeLines);
