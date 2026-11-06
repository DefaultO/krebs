// Builtins patched in the loop, before a room's first table read, and restored before the tick ends: that read must call none
// of them, and what it builds must have its usual prototype, whenever it happens.
module.exports.loop = function () {
    var spawn = Game.spawns[Object.keys(Game.spawns)[0]], calls = 0, table = null;
    var names = ['keys', 'create', 'defineProperty', 'setPrototypeOf'], real = names.map(function (n) { return Object[n]; });
    var RealProxy = Proxy;
    names.forEach(function (n, i) {
        Object[n] = function () { calls++; return real[i].apply(this, arguments); };
    });
    var active = spawn.isActive();
    names.forEach(function (n, i) { Object[n] = real[i]; });
    // an empty name finds no entry, so the getter gets the table the engine asked
    Object.defineProperty(Object.prototype, 'W0N0', {configurable: true, get() { table = table || this; return undefined; }});
    try { Room.prototype.createConstructionSite.call({name: 'W0N0'}, 0, 0, STRUCTURE_ROAD); } catch (e) {}
    delete Object.prototype.W0N0;
    var room = spawn.room.name, usual = table && Object.getPrototypeOf(table[room]) === Object.prototype;
    console.log('probe', Game.time, calls, active, usual);
};
