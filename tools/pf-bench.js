'use strict';
// Times the official native PathFinder on the recorded fixture queries (the successful ones), for
// comparison with `cargo test -p pathfinder --release --test differential -- --ignored --nocapture`.
//
//   E:\screeps\node22\node-v22.22.3-win-x64\node.exe tools/pf-bench.js [rounds]
//
// Calls the addon's `search` directly with world coordinates and a minimal room callback, so the
// time is the C++ search plus the unavoidable JS callback round trips, without the JS glue.
const fs = require('fs');
const path = require('path');

const DRIVER = process.env.SCREEPS_DRIVER || 'E:/screeps/server/node_modules/@screeps/driver';
const native = require(DRIVER + '/native/build/Release/native.node');
const glue = require(DRIVER + '/lib/path-finder.js');
const FIX = path.join(__dirname, '../crates/pathfinder/tests/fixtures');
const ROUNDS = Number(process.argv[2]) || 20;

// Official world coordinate of a room: E0 -> 128, W0 -> 127 (path-finder.js parseRoomName)
function roomXY(name) {
    const m = /^([WE])(\d+)([NS])(\d+)$/.exec(name);
    return { xx: m[1] === 'W' ? 127 - Number(m[2]) : 128 + Number(m[2]), yy: m[3] === 'N' ? 127 - Number(m[4]) : 128 + Number(m[4]) };
}
const wpos = (room, x, y) => { const r = roomXY(room); return { xx: r.xx * 50 + Number(x), yy: r.yy * 50 + Number(y) }; };

const rooms = [], matrices = [], profiles = [];
for (const line of fs.readFileSync(path.join(FIX, 'world.txt'), 'utf8').split('\n')) {
    const t = line.split(' ');
    if (t[0] === 'room') rooms.push({ room: t[1], terrain: t[2] });
    else if (t[0] === 'matrix') matrices.push(new Uint8Array(Buffer.from(t[2], 'hex')));
    else if (t[0] === 'profile') {
        const map = new Map();
        for (const e of t.slice(3).filter(Boolean)) {
            const [r, c] = e.split('=');
            const { xx, yy } = roomXY(r);
            map.set(xx * 256 + yy, c === 'F' ? false : c === 'X' ? new Uint8Array(100) : c === 'T' ? 'T' : matrices[Number(c.slice(1))]);
        }
        // 'shared': every matrix is copied into one scratch array that is returned (see the generator)
        const scratch = new Uint8Array(2500);
        const shared = (xx, yy) => {
            const v = map.get(xx * 256 + yy);
            if (v instanceof Uint8Array && v.length === 2500) { scratch.set(v); return scratch; }
            return v;
        };
        profiles.push(t[2] === 'none' ? undefined : t[2] === 'shared' ? shared : (xx, yy) => map.get(xx * 256 + yy));
    }
}
glue.init(native, rooms);

const cases = [];
for (const file of fs.readdirSync(FIX).filter(f => /^cases-\d+\.txt$/.test(f)).sort()) {
    for (const line of fs.readFileSync(path.join(FIX, file), 'utf8').split('\n')) {
        if (!line || line.includes(' = err ')) continue;
        const t = line.split(' ');
        let i = 1;
        const origin = wpos(t[i++], t[i++], t[i++]);
        ++i; // G
        const goals = [];
        for (let n = Number(t[i++]); n > 0; --n) goals.push({ range: Number(t[i++]), pos: wpos(t[i++], t[i++], t[i++]) });
        i += 8; // J ...
        ++i; // N
        const [plain, swamp, maxRooms, maxOps, maxCost, flee, weight] = t.slice(i, i + 7).map(Number);
        i += 7;
        ++i; // P
        const cb = profiles[Number(t[i++])];
        cases.push([origin, goals, cb, plain, swamp, maxRooms, maxOps, maxCost, flee === 1, weight]);
    }
}

// Same subsets as the Rust bench: "typical" = maxOps <= 2000 (the default), "long" = the rest.
function bench(name, list) {
    const runAll = () => { let ops = 0; for (const c of list) { const r = native.search(...c); if (r && r.ops) ops += r.ops; } return ops; };
    runAll(); runAll();
    const start = process.hrtime.bigint();
    let ops = 0;
    for (let r = 0; r < ROUNDS; ++r) ops += runAll();
    const ns = Number(process.hrtime.bigint() - start);
    const n = ROUNDS * list.length;
    console.log(`native ${name}: ${n} searches, ${(ns / 1e3 / n).toFixed(3)} us/search, ${(ns / ops).toFixed(1)} ns/op (${ops} ops)`);
}
bench('all', cases);
bench('typical', cases.filter(c => c[6] <= 2000));
bench('long', cases.filter(c => c[6] > 2000));
