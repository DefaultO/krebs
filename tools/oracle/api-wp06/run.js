// Runs every WP06 API case on the vanilla files and on chitin's ports and compares what the probes observed.
//   E:\screeps\node22\node-v22.22.3-win-x64\node.exe tools/oracle/api-wp06/run.js [--write] [name-filter]
// --write records the vanilla observations as fixtures (crates/runtime/tests/fixtures/api-rooms/<case>.json, with the
// case input so another harness can replay them); without it the chitin side is also checked against those fixtures.
'use strict';
const fs = require('fs');
const path = require('path');
const { runCase, ROOT } = require('./harness');

const FIXTURES = path.join(ROOT, 'crates', 'runtime', 'tests', 'fixtures', 'api-rooms');
const args = process.argv.slice(2);
const write = args.includes('--write');
const filter = args.find(a => !a.startsWith('--'));

function firstDiff(a, b, p = '$') {
    if (JSON.stringify(a) === JSON.stringify(b)) return null;
    if (a && b && typeof a === 'object' && typeof b === 'object') {
        const keys = [...new Set([...Object.keys(a), ...Object.keys(b)])];
        if (JSON.stringify(Object.keys(a)) !== JSON.stringify(Object.keys(b))) return `${p}: keys ${JSON.stringify(Object.keys(a))} vs ${JSON.stringify(Object.keys(b))}`;
        for (const k of keys) { const d = firstDiff(a[k], b[k], `${p}.${k}`); if (d) return d; }
    }
    return `${p}: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`.slice(0, 600);
}

const cases = [];
for (const f of fs.readdirSync(path.join(__dirname, 'cases')).sort()) {
    for (const c of [].concat(require(path.join(__dirname, 'cases', f)))) cases.push(c);
}

let fail = 0, total = 0;
for (const kase of cases) {
    if (filter && !kase.name.includes(filter)) continue;
    total++;
    const vanilla = runCase('vanilla', kase);
    const chitin = runCase('chitin', kase);
    const fixturePath = path.join(FIXTURES, kase.name + '.json');
    const errs = [];
    const d = firstDiff(vanilla, chitin);
    if (d) errs.push('vanilla vs chitin ' + d);
    if (write) {
        fs.mkdirSync(FIXTURES, { recursive: true });
        const ticks = kase.ticks.map((t, i) => ({
            world: t.world, probes: t.probes.map(([l, fn]) => [l, fn.toString()]), expect: vanilla[i],
        }));
        fs.writeFileSync(fixturePath, JSON.stringify({ name: kase.name, tags: kase.tags, world: kase.world, ticks }, null, 1) + '\n');
    } else if (fs.existsSync(fixturePath)) {
        const fx = JSON.parse(fs.readFileSync(fixturePath, 'utf8'));
        const fd = firstDiff(fx.ticks.map(t => t.expect), chitin);
        if (fd) errs.push('fixture vs chitin ' + fd);
    } else {
        errs.push('no fixture (run with --write)');
    }
    if (errs.length) { fail++; console.log(`FAIL ${kase.name}\n  ${errs.join('\n  ')}`); }
    else console.log(`ok   ${kase.name}`);
}
console.log(`${total - fail}/${total} cases passed`);
process.exit(fail ? 1 : 0);
