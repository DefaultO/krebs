// Whether two Stores have one Proxy handler: a trap found on Object.prototype runs with `this` = handler.
module.exports.loop = function () {
    var spawn = Game.spawns[Object.keys(Game.spawns)[0]];
    var stores = [spawn.store, new Store(spawn)];
    var seen = [];
    Object.prototype.has = function (t, k) { seen.push(this); return Reflect.has(t, k); };
    stores.forEach(st => 'energy' in st);
    delete Object.prototype.has;
    console.log('probe', Game.time, 'stores', stores.length, 'same handler', seen.length > 1 && seen[0] === seen[1]);
};
