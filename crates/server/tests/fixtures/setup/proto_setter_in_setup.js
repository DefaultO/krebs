// An accessor on the controller's class puts a setter on Object.prototype during Game setup, where a tick that started
// clean has already chosen how to build the tables; the spawn's doc comes after the controller's. Vanilla's writes reach
// the setter, and its read back of the structures table gets nothing, so Game setup fails. The loop takes the setter off
// so that the next tick starts clean again.
var spawnId;
module.exports.loop = function () {
    console.log('probe', Game.time, global.calls, Object.keys(Game.spawns).length);
    global.calls = 0;
    spawnId = spawnId || Game.spawns[Object.keys(Game.spawns)[0]].id;
    if (!global.armed) {
        global.armed = true;
        Object.defineProperty(StructureController.prototype, '_my', {configurable: true, get() { return this.__m; },
            set(v) {
                this.__m = v;
                if (Game.time % 2) return;
                Object.defineProperty(Object.prototype, spawnId, {configurable: true, get() { return global.fake; }, set(x) { global.calls++; }});
            }});
        global.fake = {name: 'fake', id: 'fake'};
    }
    delete Object.prototype[spawnId];
};
