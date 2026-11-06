// driver/lib/path-finder.js init(): terrain packing into the native's 2-bit column-major layout, room-name parsing
// and the binary version check (spec 09 TERRAIN-PF-PACKED, PF-API-INIT, COORD-PF-PARSE). init is host-side in
// vanilla, so the case sets world.exposeDriverPathFinder and probes the module through global.__driverPathFinder.
'use strict';
const { base } = require('../world');

module.exports = [{
    name: 'driver-path-finder-init',
    tags: ['lib/path-finder.js:10', 'lib/path-finder.js:23'],
    world: base({ exposeDriverPathFinder: true }),
    ticks: [{ probes: [
        ['init', () => {
            const pf = __driverPathFinder, out = [];
            const terrain = n => { let s = ''; for (let i = 0; i < 2500; i++) s += (i * 7 + n) % 4; return s; };
            let captured;
            const mod = {version: 11, loadTerrain: d => { captured = d; }};
            pf.init(mod, [{room: 'W1N1', terrain: terrain(1)}, {room: 'E127S127', terrain: terrain(2)}, {room: 'W127N127', terrain: terrain(3)},
                {room: 'E0S0', terrain: Array.from(terrain(0)).map(Number)}]);
            out.push(captured.map(r => [r.room, Array.from(r.bits).slice(0, 40), r.bits.length, Array.from(r.bits).reduce((a, b) => (a * 31 + b) % 1000003, 0)]));
            for (const [m, rooms] of [[{version: 10, loadTerrain() {}}, []], [mod, [{room: 'W128N0', terrain: ''}]], [mod, [{room: 'w1n1', terrain: ''}]],
                [mod, [{room: 'E1S1x', terrain: ''}]], [mod, [{room: 'W0N0', terrain: 'x'.repeat(2500)}]]]) {
                try { pf.init(m, rooms); out.push(captured.length && captured.map(r => [r.room, r.bits[0]])); } catch (e) { out.push(e.message); }
            }
            return out;
        }],
    ] }],
}];
