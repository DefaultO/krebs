// Small game modules owned by WP05: Resource/Energy (resources.js), unique creep names (names.js), mod custom
// object prototypes (custom-prototypes.js, HOOK-CUSTOM-OBJECT) and the engine utils they reach (utils.js).
'use strict';
const { basicWorld, doc, ME, OTHER, id } = require('./_lib');

const resources = basicWorld(`
module.exports.loop = function () {
    var r = Game.rooms.W1N1.find(FIND_DROPPED_RESOURCES);
    console.log('found', r.length, Resource === Energy, r[0] instanceof RoomObject);
    r.forEach(function (x) {
        console.log(String(x), x.resourceType, x.amount, x.energy, JSON.stringify(x), Object.keys(x), x.pos.x, x.room.name);
    });
    var d = Object.getOwnPropertyDescriptor(Resource.prototype, 'amount');
    console.log('desc', typeof d.get, d.set, d.enumerable, d.configurable, Object.getOwnPropertyDescriptor(global, 'Resource').writable);
};`);
resources.objects.push(
    doc.energy('W1N1', 3, 3, 150),
    { _id: id('r'), type: 'energy', room: 'W1N1', x: 4, y: 4, resourceType: 'H', H: 77 },
    { _id: id('r'), type: 'energy', room: 'W1N1', x: 4, y: 5, energy: 0 },
);

const names = basicWorld(`
module.exports.loop = function () {
    var s = Game.spawns.Spawn1;
    console.log('names', s.createCreep([MOVE]), s.createCreep([MOVE]), Math.random());
};`);

const portalGate = [
    'portalGate', 'PortalGate', {
        properties: {
            power: '(o) => o.power',
            label: 'function (o, id) { return o.label + "/" + id.length; }',
        },
        prototypeExtender: 'prototypeExtender(proto, scope, {utils}) { proto.describe = function () { return "gate " + this.power + " " + typeof utils.getDiff + " " + typeof scope.runtimeData; }; }',
        userOwned: true,
        findConstant: 10100,
        lookConstant: 'portalGates',
    },
];
const plain = ['marker', 'Marker', { properties: {}, prototypeExtender: 'function (proto) { proto.toString = function () { return "[marker]"; }; }' }];
const custom = basicWorld(`
module.exports.loop = function () {
    var gates = Game.rooms.W1N1.find(10100);
    console.log('gates', gates.length, typeof PortalGate, Object.getOwnPropertyDescriptor(global, 'PortalGate').writable);
    gates.forEach(function (g) {
        console.log(g instanceof RoomObject, g.power, g.label, g.my, JSON.stringify(g.owner), g.describe(), JSON.stringify(g), Object.keys(g));
    });
    var m = Game.getObjectById('MARKER');
    console.log('marker', String(m), m instanceof Marker, Game.rooms.W1N1.find(10101).length);
};`, { customObjectPrototypes: [portalGate, plain] });
custom.objects.push(
    { _id: id('g'), type: 'portalGate', room: 'W1N1', x: 40, y: 40, user: ME, power: 5, label: 'mine' },
    { _id: id('g'), type: 'portalGate', room: 'W1N1', x: 41, y: 40, user: OTHER, power: 0, label: 'theirs' },
    { _id: 'MARKER', type: 'marker', room: 'W1N1', x: 1, y: 1 },
);

module.exports = [
    { name: 'objects-resource', tags: ['game/resources.js:8', 'game/game.js:345', 'src/utils.js:509'], world: resources, ticks: 2 },
    { name: 'objects-names', tags: ['game/names.js:4', 'src/utils.js:114'], world: names, ticks: 2 },
    { name: 'objects-custom-prototypes', tags: ['game/custom-prototypes.js:8', 'game/game.js:52', 'game/game.js:384', 'runtime/runtime.js:34'], world: custom, ticks: 2 },
];
