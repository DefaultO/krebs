// A first read of a room table near the stack limit, caught by the bot; then, at normal depth, two ordinary calls that read the table.
function deep(n, f) {
    try { return deep(n + 1, f); }
    catch (e) { try { f(); } catch (e2) {} throw e; }
}
module.exports.loop = function () {
    var spawn = Game.spawns[Object.keys(Game.spawns)[0]], room = spawn.room, x = spawn.pos.x, y = spawn.pos.y;
    try { deep(0, function () { spawn.isActive(); }); } catch (e) {}
    var from = new RoomPosition(x - 1, y, room.name), to = new RoomPosition(x + 1, y, room.name);
    console.log('probe', Game.time, 'container on the spawn tile', room.createConstructionSite(x, y, 'container'),
        'path around the spawn', from.findPathTo(to, {ignoreCreeps: true}).map(s => s.x - x + ',' + (s.y - y)).join(' '));
};
