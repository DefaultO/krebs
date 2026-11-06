#!/usr/bin/env node
// Smoke check of the prelude's system-isolate entry (host/prelude.js `_initSystem`, §4.2/§6.5): a system bundle
// (lodash, common/system.js, constants, engine/utils.js, a stand-in host/system.js exposing utils.storeIntents)
// boots in a fresh context, leaves no bootstrap globals and routes/sanitizes a sample like make.js:192.
//   node tools/oracle/api/system-smoke.js
'use strict';
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const { ROOT, reference } = require('../lib/common');
const { isStub } = require('./bundle');

const JS = path.join(ROOT, 'crates', 'runtime', 'js');
const read = (p) => fs.readFileSync(p, 'utf8');
const define = (p, src) => `__chitinDefine(${JSON.stringify(p)}, function(module, exports, require) {\n${src}\n});\n`;
const ours = (p, ref) => (isStub(read(path.join(JS, p))) ? read(reference(ref)) : read(path.join(JS, p)));

let text = read(path.join(JS, 'host', 'prelude.js'));
text += define('vendor/lodash.js', read(path.join(JS, 'vendor', 'lodash.js')));
text += define('common/system.js', ours('common/system.js', 'common/lib/system.js'));
text += define('common/constants.js', `module.exports = ${read(path.join(ROOT, 'crates/engine/tests/fixtures/constants.json'))};`);
text += define('engine/utils.js', read(path.join(JS, 'engine', 'utils.js')));
text += define('host/system.js', "exports.storeIntents = require('@screeps/engine/src/utils').storeIntents; exports.natives = global._natives;");

const ctx = vm.createContext({});
new vm.Script(text, { filename: '<system>' }).runInContext(ctx);
const G = vm.runInContext('globalThis', ctx);
const natives = { objectRoom: () => undefined };
const sys = G._initSystem(natives);
assert.strictEqual(sys.natives, natives);
assert.deepStrictEqual(Object.getOwnPropertyNames(G).filter((k) => k.startsWith('_')), []);   // bootstrap globals gone

const runtimeData = {
    userObjects: { a1: { _id: 'a1', room: 'W1N1' } },
    roomObjects: { b2: { _id: 'b2', room: 'W2N1' } },
};
const list = {
    a1: { move: { direction: '3', extra: 1 }, say: { message: 5, isPublic: 1 } },
    b2: { attack: { id: 7 } },
    zz: { move: { direction: 1 } },
    room: { createFlag: [{ roomName: 'W1N1', x: '5', y: 6.7, name: 'F', color: 1, secondaryColor: 2 }] },
    notify: [{ message: 'hi', groupInterval: '3' }],
    global: { createOrder: [{ type: 'sell', resourceType: 'energy', price: 0.1234, totalAmount: 10, roomName: 'W1N1' }] },
};
const got = JSON.parse(JSON.stringify(sys.storeIntents('u', vm.runInContext(`(${JSON.stringify(list)})`, ctx), runtimeData, {})));
assert.deepStrictEqual(got.W1N1.a1.move, { id: 'undefined', direction: 3 });   // system.js sanitizer fields
assert.deepStrictEqual(Object.keys(got), ['W1N1', 'W2N1', 'notify', 'global']);
console.log('system bundle ok:', JSON.stringify(got));
