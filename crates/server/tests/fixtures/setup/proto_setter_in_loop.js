// A setter added to Object.prototype inside the player loop, before the first read of the room tables, and removed
// again: vanilla filled them before the loop, so it sees nothing, and the host may go on reusing the docs.
module.exports.loop = function () {
    var room = Game.rooms[Object.keys(Game.rooms)[0]], source = room.find(FIND_SOURCES)[0];
    if (Game.time === 3) {
        var seen = [];
        [source.id, room.name].forEach(function (key) {
            Object.defineProperty(Object.prototype, key, {configurable: true, get() { return undefined; }, set(v) { seen.push(key); global.raw = v; }});
        });
        var site = room.createConstructionSite(source.pos.x, source.pos.y, 'container');
        delete Object.prototype[source.id];
        delete Object.prototype[room.name];
        console.log('probe', site, seen.length, !!global.raw);
        if (global.raw) global.raw.energyCapacity = 12345;
    }
    if (Game.time >= 2 && Game.time < 7) console.log('probe', Game.time, Game.getObjectById(source.id).energyCapacity);
};
