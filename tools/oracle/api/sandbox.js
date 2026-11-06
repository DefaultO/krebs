// A fresh player context for either side of the API oracle, with the playerSandbox-style determinism hook
// (ARCHITECTURE.md §7.3): Math.random is a seeded mulberry32 and `Date` reads a fixed per-tick clock. Both are
// installed before any runtime code runs, so lodash's runInContext captures the seeded Math.random too.
'use strict';
const vm = require('vm');

function createSandbox() {
    const ctx = vm.createContext({});
    const state = vm.runInContext(`(function (RealDate, M, G) {
        var state = {a: 1, now: 0};
        M.random = function random() {
            var t = state.a = (state.a + 0x6d2b79f5) >>> 0;
            t = Math.imul(t ^ (t >>> 15), t | 1);
            t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
            return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
        };
        function Date() {
            if (!new.target) return new RealDate(state.now).toString();
            return arguments.length ? Reflect.construct(RealDate, arguments, new.target) : new RealDate(state.now);
        }
        Date.prototype = RealDate.prototype;
        Date.now = function now() { return state.now; };
        Date.parse = RealDate.parse;
        Date.UTC = RealDate.UTC;
        G.Date = Date;
        return state;
    })(Date, Math, globalThis)`, ctx);
    return {
        ctx,
        // seed: uint32; now: virtual ms (vclock)
        setTick(seed, now) {
            state.a = seed >>> 0;
            state.now = now;
        },
    };
}

module.exports = { createSandbox };
