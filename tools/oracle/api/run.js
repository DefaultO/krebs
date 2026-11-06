#!/usr/bin/env node
// API oracle runner (ARCHITECTURE.md §7.3): every case of tools/oracle/api/cases/*.js runs tick by tick on the
// official runtime (reference sources under the isolated-vm shim) and on chitin's bundle (crates/runtime/js with
// node natives); the normalized outputs must be identical.
//   node tools/oracle/api/run.js [name-filter …] [--write] [--verbose] [--coverage] [--real-bundle] [--setup-list]
// --write  (re)writes crates/runtime/tests/fixtures/api/<case>.json: per tick the §6.6 __tick input and the
//          official normalized output (the host conformance suite and parity-report read these).
// --coverage lists the functions of landed (non-stub) files no selected case calls (§7.3 coverage gate).
// --real-bundle runs the official side on the installed webpack build (bundle.js officialBundle): a cross-check of the
//          reference-source loader's webpack emulation. Fixtures are neither written nor checked in this mode.
// --setup-list runs chitin's side with runner.setupList on, as a match does (ARCHITECTURE.md "What differs in a match").
//          Fixtures are neither written nor checked in this mode.
// Use node 22 (E:\screeps\node22\…\node.exe) for cases that need the official native pathfinder.
// Adding a case: drop a file in cases/ exporting an array of {name, tags, world, ticks[, needsNative, caveat]};
// see cases/_lib.js. `caveat` (copied into the fixture) names what the node oracle cannot model exactly.
'use strict';
const fs = require('fs');
const path = require('path');
const { ROOT } = require('../lib/common');
const W = require('./world');
const { OfficialVM, OurVM, nativeAvailable } = require('./vms');
const { normalize, diff } = require('./normalize');

const args = process.argv.slice(2);
const write = args.includes('--write');
const verbose = args.includes('--verbose');
const coverage = args.includes('--coverage');
const realBundle = args.includes('--real-bundle');
if (realBundle) process.env.ORACLE_REAL_BUNDLE = '1';
const setupList = args.includes('--setup-list');
if (setupList) process.env.ORACLE_SETUP_LIST = '1';
if (coverage) {
    process.env.ORACLE_COVERAGE = '1';
    require('./coverage').start();
}
const filters = args.filter((a) => !a.startsWith('--'));
const FIXTURES = path.join(ROOT, 'crates', 'runtime', 'tests', 'fixtures', 'api');

function loadCases() {
    const dir = path.join(__dirname, 'cases');
    const out = [];
    for (const f of fs.readdirSync(dir).sort()) {
        if (!f.endsWith('.js') || f.startsWith('_')) continue;
        // a fresh _lib per file: its object ids then depend on that file only (adding a case file does not
        // renumber every other fixture)
        delete require.cache[path.join(dir, '_lib.js')];
        for (const c of require(path.join(dir, f))) out.push(Object.assign({ file: f }, c));
    }
    return out.filter((c) => !filters.length || filters.some((f) => c.name.includes(f)));
}

const clone = (v) => structuredClone(v);

function applyPatch(w, patch, t) {
    if (typeof patch === 'function') patch(w, t);
    else Object.assign(w, clone(patch || {}));
}

function runCase(c) {
    const ticks = typeof c.ticks === 'number' ? Array(c.ticks).fill({}) : c.ticks || [{}];
    const base = W.world(clone(c.world));
    const sides = {
        official: { w: clone(base), Vm: OfficialVM },
        ours: { w: clone(base), Vm: OurVM },
    };
    let fixtureVm = {};
    let substituted = [];
    const fixtureTicks = [];
    const failures = [];
    ticks.forEach((patch, t) => {
        const res = {};
        for (const [name, s] of Object.entries(sides)) {
            applyPatch(s.w, patch, t);
            // fresh VM at the start, after a halt (make.js:280-284) or on a newer code version (user-vm.js:23-25;
            // chitin resets on any change of the version, the same thing when versions only grow)
            if (!s.vm || (s.w.code.timestamp || 0) > s.codeVersion) {
                s.vm = new s.Vm(s.w, c.name);
                if (name === 'official') {
                    fixtureVm = {};
                    res.newVm = true;   // a replaying host creates a fresh VM before this tick
                }
                else substituted = s.vm.substituted;
            }
            s.codeVersion = s.w.code.timestamp || 0;
            if (name === 'official') {
                const made = W.chitinInput(s.w, fixtureVm);   // the canonical input sequence (official world)
                if (made) fixtureVm = made.vm;
                res.input = made && made.input;
            }
            const raw = s.vm.tick(s.w, t);
            const n = normalize(raw);
            res[name] = n;
            if (raw.status === 'halted') s.vm = null;
            W.applyOutput(s.w, Object.assign({}, raw.out, { status: n.status, memory: n.memory }));
            s.w.time += 1;
        }
        if (res.official.status === 'done' || res.official.status === 'error') fixtureVm.memory = res.official.memory;
        else if (res.official.status !== 'inactive') fixtureVm.memory = null;
        // fixture JSON: absent = undefined; binary modules as {binary: base64} (the host passes their bytes)
        const input = res.input && Object.assign({}, res.input, res.input.modules && {
            modules: Object.fromEntries(Object.entries(res.input.modules).map(([k, v]) =>
                [k, typeof v === 'string' ? v : { binary: Buffer.from(v).toString('base64') }])),
        });
        fixtureTicks.push(Object.assign(res.newVm ? { newVm: true } : {}, { input, expected: res.official }));
        const d = diff(res.official, res.ours);
        if (d) failures.push({ tick: t, diff: d, official: res.official, ours: res.ours });
        if (verbose) console.log(`  tick ${t}:`, JSON.stringify(res.official));
    });
    return { failures, fixtureTicks, substituted };
}

let failed = 0;
const cases = loadCases();
let skipped = 0;
for (const c of cases) {
    if (c.needsNative && !nativeAvailable()) {
        skipped++;
        console.log(`skip ${c.name}: needs the official native pathfinder (node 22)`);
        continue;
    }
    let r;
    try {
        r = runCase(c);
    } catch (e) {
        failed++;
        console.log(`FAIL ${c.name} (${c.file}): harness error\n${e.stack}`);
        continue;
    }
    const file = path.join(FIXTURES, `${c.name}.json`);
    const fixture = JSON.stringify(Object.assign({ name: c.name, tags: c.tags || [] }, c.caveat ? { caveat: c.caveat } : {}, { substituted: r.substituted, ticks: r.fixtureTicks }), null, 1) + '\n';
    if (realBundle || setupList) {
        // engine 4.3.0 build, or a match's path: its outputs are no fixtures
    } else if (write) {
        fs.mkdirSync(FIXTURES, { recursive: true });
        fs.writeFileSync(file, fixture);
    } else if (fs.existsSync(file) && fs.readFileSync(file, 'utf8') !== fixture) {
        console.log(`STALE ${c.name}: fixture differs from the live official run (rerun with --write)`);
    }
    if (r.failures.length) {
        failed++;
        const f = r.failures[0];
        console.log(`FAIL ${c.name} (${c.file}) tick ${f.tick}: ${f.diff}`);
        if (verbose) console.log('  official:', JSON.stringify(f.official), '\n  ours:    ', JSON.stringify(f.ours));
    } else {
        console.log(`ok   ${c.name}`);
    }
}
console.log(`${cases.length - failed - skipped}/${cases.length} cases identical${skipped ? `, ${skipped} skipped` : ''}`);
if (coverage) require('./coverage').report();
process.exitCode = failed ? 1 : 0;
