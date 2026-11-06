// A replaced global Proxy gets a handler of its own for each Store, as in vanilla.
module.exports.loop = function () {
    var spawn = Game.spawns[Object.keys(Game.spawns)[0]];
    if (!global.armed) {
        global.armed = true; global.seen = new Set(); global.made = 0; global.calls = 0;
        var Real = Proxy;
        global.Proxy = function (target, handler) {
            global.made++; global.seen.add(handler);
            var get = handler.get;
            handler.get = function (a, b, c) { global.calls++; return get.call(this, a, b, c); };
            return new Real(target, handler);
        };
        return;
    }
    global.calls = 0;
    var energy = spawn.store.energy;
    if (Game.time < 7) console.log('probe', global.made, global.seen.size, global.calls);
};
