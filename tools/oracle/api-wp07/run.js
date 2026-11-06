'use strict';
// WP07 API differential runner (node 22: the official native.node pathfinder addon needs its ABI).
//
//   node tools/oracle/api-wp07/run.js [filter]          vanilla vs chitin, live
//   node tools/oracle/api-wp07/run.js --write [filter]  also (re)write fixtures from the vanilla side
//   node tools/oracle/api-wp07/run.js --fixtures        chitin side against the committed fixtures only
//
// Case modules (cases/*.js) export [{name, tags, rooms: {name: terrain}, ticks: [runtimeData]}]; the
// memory of tick n+1 is the memory string saved by tick n (as driver make.js does). Fixtures are written to
// crates/runtime/tests/fixtures/api-wp07/<name>.json: {case, tags, rooms, worldSize, ticks: [{runtimeData,
// expect}]}, expect = {journal, error, intents, memory, console}. runtimeData cpu/cpuBucket null = Infinity.
process.env.TZ = 'UTC'; // Date#toString in getters (controller sign) must not depend on the host zone
const fs = require('fs');
const path = require('path');
const { createSide, loadTerrain } = require('./side');

const FIXTURES = path.join(__dirname, '..', '..', '..', 'crates', 'runtime', 'tests', 'fixtures', 'api-wp07');
const args = process.argv.slice(2);
const write = args.includes('--write');
const fromFixtures = args.includes('--fixtures');
const filter = args.find(a => !a.startsWith('--'));

// Probe helper every case module can `require('probe')`: records label + value (or the thrown error).
const PROBE = `module.exports = function(label, fn) {
    var v;
    try { v = fn(); } catch (e) {
        var msg = (e && e.constructor && e.constructor.name) + ': ' + (e && e.message);
        if (typeof __probe === 'function') __probe(label, msg, true); else console.log(label + ' ! ' + msg);
        return;
    }
    if (typeof __probe === 'function') __probe(label, v); else console.log(label + ' = ' + JSON.stringify(v));
    return v;
};`;

function loadCases() {
    const dir = path.join(__dirname, 'cases');
    const out = [];
    for (const f of fs.readdirSync(dir).filter(f => f.endsWith('.js') && !f.startsWith('_')).sort()) {
        for (const c of require(path.join(dir, f))) out.push(c);
    }
    return out.filter(c => !filter || c.name.includes(filter));
}

function runTicks(variant, c, ticks, expectFn) {
    loadTerrain(Object.keys(c.rooms).map(room => ({ room, terrain: c.rooms[room] })));
    const side = createSide(variant, { rooms: c.rooms, worldSize: c.worldSize || 12 });
    let memory = null, lastCode = null, stamp = 0;
    const results = [];
    for (let i = 0; i < ticks.length; i++) {
        const data = JSON.parse(JSON.stringify(ticks[i]));
        delete data.keepMemory;
        if (memory !== null && !ticks[i].keepMemory) data.userMemory.data = memory;
        // New code => new userCodeTimestamp, so game.js rebuilds `require` (the VM itself is kept).
        if (!ticks[i].keepMemory) {
            const c = JSON.stringify(data.userCode);
            if (c !== lastCode) stamp++;
            lastCode = c;
            data.userCodeTimestamp = stamp;
        }
        data.userCode = Object.assign({ probe: PROBE }, data.userCode);
        const r = side.tick(data, { seed: i + 1 });
        memory = r.memory;
        results.push({ runtimeData: data, expect: r });
        if (expectFn) expectFn(i, r);
    }
    return results;
}

function firstDiff(a, b, where) {
    for (const k of Object.keys(a)) {
        const x = JSON.stringify(a[k]), y = JSON.stringify(b[k]);
        if (x === y) continue;
        if (Array.isArray(a[k]) && Array.isArray(b[k])) {
            for (let i = 0; i < Math.max(a[k].length, b[k].length); i++) {
                if (JSON.stringify(a[k][i]) !== JSON.stringify(b[k][i])) {
                    return `${where} ${k}[${i}]\n  vanilla: ${JSON.stringify(a[k][i])}\n  chitin:  ${JSON.stringify(b[k][i])}`;
                }
            }
        }
        return `${where} ${k}\n  vanilla: ${x}\n  chitin:  ${y}`;
    }
    return null;
}

let failed = 0, passed = 0;
for (const c of loadCases()) {
    const file = path.join(FIXTURES, c.name + '.json');
    let expected;
    try {
        if (fromFixtures) {
            const fx = JSON.parse(fs.readFileSync(file, 'utf8'));
            c.rooms = fx.rooms;
            expected = fx.ticks;
        } else {
            expected = runTicks('vanilla', c, c.ticks);
        }
        const ticks = expected.map(t => t.runtimeData);
        ticks.forEach(t => { t.keepMemory = true; });
        let diff = null;
        runTicks('chitin', c, ticks, (i, r) => {
            diff = diff || firstDiff(expected[i].expect, r, `tick ${i}`);
        });
        if (diff) {
            failed++;
            console.log(`FAIL ${c.name}: ${diff}`);
        } else {
            passed++;
            console.log(`ok   ${c.name} (${expected.reduce((n, t) => n + t.expect.journal.length, 0)} journal entries)`);
        }
        if (write && !fromFixtures) {
            fs.mkdirSync(FIXTURES, { recursive: true });
            expected.forEach(t => { delete t.runtimeData.keepMemory; delete t.runtimeData.userCode.probe; });
            // One line per tick / room so diffs stay readable without inflating the file.
            const lines = [
                `{"case":${JSON.stringify(c.name)},"tags":${JSON.stringify(c.tags)},"worldSize":${c.worldSize || 12},"rooms":{`,
                Object.keys(c.rooms).map(r => `${JSON.stringify(r)}:${JSON.stringify(c.rooms[r])}`).join(',\n'),
                '},"ticks":[',
                expected.map(t => JSON.stringify(t)).join(',\n'),
                ']}',
            ];
            fs.writeFileSync(file, lines.join('\n') + '\n');
        }
    } catch (e) {
        failed++;
        console.log(`FAIL ${c.name}: harness error ${e && e.stack}`);
    }
}
console.log(`${passed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
