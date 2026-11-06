// Memory segments (02 SEG-*): preload, re-save of every key, setters' validation, save-time total failures,
// public/default/foreign segments across ticks.
'use strict';
const { basicWorld, ME, OTHER } = require('./_lib');

const T = ['runtime/runtime.js:129', 'runtime/runtime.js:191', 'runtime/runtime.js:250'];

const withSegments = (main, extra = {}) => {
    const w = basicWorld(main, extra);
    w.users.find((u) => u._id === ME).activeSegments = [5, 0, 7];
    return w;
};

module.exports = [
    {
        name: 'segments-basic',
        tags: T,
        world: withSegments(`
module.exports.loop = function () {
    console.log('keys', Object.keys(RawMemory.segments), JSON.stringify(RawMemory.segments), Object.getPrototypeOf(RawMemory.segments));
    var d = Object.getOwnPropertyDescriptor(RawMemory, 'segments');
    console.log('desc', d.writable, d.enumerable, d.configurable);
    RawMemory.segments = {};
    RawMemory.segments[5] = 'five@' + Game.time;
    RawMemory.segments['42'] = 'new';
    delete RawMemory.segments[0];
    if (Game.time === 100) RawMemory.setActiveSegments([1, '2', 3.9, '4abc']);
    if (Game.time === 101) RawMemory.setActiveSegments([]);
};`, { segments: { 0: 'zero', 5: 'five', 1: 'one' } }),
        ticks: 3,
    },
    {
        name: 'segments-setters',
        tags: T.concat(['runtime/runtime.js:148', 'runtime/runtime.js:163', 'runtime/runtime.js:174']),
        world: withSegments(`
function t(f) { try { f(); return 'ok'; } catch (e) { return e.message; } }
module.exports.loop = function () {
    console.log(t(() => RawMemory.setActiveSegments('1,2')));
    console.log(t(() => RawMemory.setActiveSegments([0,1,2,3,4,5,6,7,8,9,10])));
    console.log(t(() => RawMemory.setActiveSegments([1, 'x'])));
    console.log(t(() => RawMemory.setActiveSegments([100])));
    console.log(t(() => RawMemory.setActiveSegments([-1])));
    console.log(t(() => RawMemory.setPublicSegments({})));
    console.log(t(() => RawMemory.setPublicSegments([1, 1, 99])));
    console.log(t(() => RawMemory.setPublicSegments([0,1,2,3,4,5,6,7,8,9,10,11])));
    console.log(t(() => RawMemory.setDefaultPublicSegment('abc')));
    console.log(t(() => RawMemory.setDefaultPublicSegment(150)));
    console.log(t(() => RawMemory.setDefaultPublicSegment('7')));
    console.log(t(() => RawMemory.setActiveForeignSegment('Bob', 'zz')));
    console.log(t(() => RawMemory.setActiveForeignSegment('Bob')));
    if (Game.time === 101) console.log(t(() => RawMemory.setDefaultPublicSegment(null)), t(() => RawMemory.setActiveForeignSegment(null)));
};`),
        ticks: 2,
    },
    {
        name: 'segments-save-errors',
        tags: T.concat(['runtime/runtime.js:252']),
        world: withSegments(`
module.exports.loop = function () {
    Game.notify('lost ' + Game.time);
    Memory.t = Game.time;
    RawMemory.setActiveSegments([9]);
    var k = Game.time - 100;
    if (k === 0) for (var i = 10; i < 18; i++) RawMemory.segments[i] = '';
    if (k === 1) RawMemory.segments.abc = 'bad key';
    if (k === 2) RawMemory.segments[3] = 3;
    if (k === 3) RawMemory.segments[4] = 'x'.repeat(100 * 1024 + 1);
    if (k === 4) RawMemory.segments[100] = 'id too big';
    if (k === 5) RawMemory.segments['5abc'] = 'saved as 5';
};`, { segments: { 0: 'zero', 5: 'five' } }),
        ticks: 7,
    },
    {
        name: 'segments-foreign',
        tags: T.concat(['runtime/runtime.js:197']),
        world: basicWorld(`
module.exports.loop = function () {
    var f = RawMemory.foreignSegment;
    console.log('foreign', f === undefined ? 'absent' : JSON.stringify(f), f ? Object.getPrototypeOf(f) : '-', f ? Object.keys(f) : '-');
    if (Game.time === 100) RawMemory.setActiveForeignSegment('Bob', 3);
    if (Game.time === 102) RawMemory.setActiveForeignSegment('Bob');
    if (Game.time === 104) RawMemory.setActiveForeignSegment('Nobody', 1);
    if (Game.time === 105) RawMemory.setActiveForeignSegment(null);
    if (Game.time === 106) RawMemory.setActiveForeignSegment('Bob', 7);
    if (f && f.id === 7) console.log('unwritten', f.data === undefined, 'data' in f, f.data === null);
};`, {
            publicSegments: { [OTHER]: '3,4,7' },
            foreignSegments: { [OTHER]: { 3: 'bob three', 4: 'bob four' } },
        }),
        ticks: 9,
    },
];

// Bob's default public segment (used when no id is given)
for (const c of module.exports) {
    const bob = c.world.users.find((u) => u._id === OTHER);
    bob.defaultPublicSegment = 4;
}
