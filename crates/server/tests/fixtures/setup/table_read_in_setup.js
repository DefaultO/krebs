// An accessor on a game class runs inside Game setup (FIND_MY_STRUCTURES reads .my, which sets ._my) and reads the room
// tables there. The spawned creep's doc follows the spawn's, so the room's list is not complete if it is still growing.
module.exports.loop = function () {
    var room = Game.rooms[Object.keys(Game.rooms)[0]], spawn = Game.spawns[Object.keys(Game.spawns)[0]];
    if (!global.armed) {
        global.armed = true;
        Object.defineProperty(StructureSpawn.prototype, '_my', {configurable: true, get() { return this.__m; },
            set(v) { this.__m = v; global.inSetup = this.room.createConstructionSite(this.pos.x, this.pos.y, 'container'); }});
    }
    if (!Object.keys(Game.creeps).length) {
        spawn.spawnCreep([MOVE], 'c1');
        return;
    }
    console.log('probe', global.inSetup, room.createConstructionSite(spawn.pos.x, spawn.pos.y, 'container'), spawn.isActive());
};
