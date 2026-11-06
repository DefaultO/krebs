// Dumps JSON.stringify(require('reference/common/lib/constants.js')) to
// crates/engine/tests/fixtures/constants.json, and reports any value JSON cannot carry.
// Usage: node tools/engine-gen-constants-fixture.js
'use strict';
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const C = require(path.join(root, 'reference/common/lib/constants.js'));

const lossy = [];
(function walk(v, at) {
    if (typeof v === 'function' || v === undefined || (typeof v === 'number' && !Number.isFinite(v))) {
        lossy.push(`${at}: ${String(v)}`);
    } else if (v && typeof v === 'object') {
        for (const k of Object.keys(v)) walk(v[k], `${at}.${k}`);
    }
})(C, 'C');
if (lossy.length) {
    console.error('values JSON cannot represent:\n  ' + lossy.join('\n  '));
    process.exitCode = 1;
}

const out = path.join(root, 'crates/engine/tests/fixtures/constants.json');
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, JSON.stringify(C));
console.log(`wrote ${out} (${Object.keys(C).length} keys)`);
