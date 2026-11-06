// M1 bot (ARCHITECTURE.md §14.6): harvest, upgrade, spawn and build with the vanilla API only
// (Game.spawns, spawnCreep, Memory, find(FIND_SOURCES), moveTo, harvest, transfer, upgradeController, build,
// createConstructionSite). Every creep is a worker: it fills up at its source, then feeds spawn/extensions,
// builds sites, and upgrades. Half the workers upgrade first so the controller always progresses.
// Owner: WP11. Deployed by crates/server/tests/m1.rs.

var WANT = 6;
var EXTENSION_SPOTS = [[-2, -2], [2, -2], [-2, 2], [2, 2], [0, -2], [0, 2], [-2, 0], [2, 0], [-3, -1], [3, 1]];

function body(energy) {
    if (energy >= 400) return [WORK, WORK, CARRY, CARRY, MOVE, MOVE];
    if (energy >= 300) return [WORK, WORK, CARRY, MOVE];
    return [WORK, CARRY, MOVE];
}

function spawnWorkers(spawn, count) {
    if (spawn.spawning || count >= WANT) return;
    var room = spawn.room;
    // with workers alive, wait for a full room; with none, take whatever a minimal body costs
    var energy = count > 0 ? room.energyCapacityAvailable : room.energyAvailable;
    if (room.energyAvailable < Math.min(energy, 400) && count > 0) return;
    var name = 'w' + Game.time;
    var role = count % 2 ? 'upgrader' : 'harvester';
    spawn.spawnCreep(body(Math.min(energy, room.energyAvailable)), name, {memory: {role: role, source: count}});
}

function placeExtensions(spawn) {
    var room = spawn.room;
    var level = room.controller ? room.controller.level : 0;
    var allowed = CONTROLLER_STRUCTURES[STRUCTURE_EXTENSION][level] || 0;
    var have = room.find(FIND_MY_STRUCTURES).filter(function (s) { return s.structureType == STRUCTURE_EXTENSION; }).length +
        room.find(FIND_MY_CONSTRUCTION_SITES).length;
    for (var i = 0; i < EXTENSION_SPOTS.length && have < allowed; i++) {
        var x = spawn.pos.x + EXTENSION_SPOTS[i][0], y = spawn.pos.y + EXTENSION_SPOTS[i][1];
        if (x < 2 || x > 47 || y < 2 || y > 47) continue;
        if (room.createConstructionSite(x, y, STRUCTURE_EXTENSION) == OK) return; // one per tick
    }
}

function needsEnergy(s) {
    return (s.structureType == STRUCTURE_SPAWN || s.structureType == STRUCTURE_EXTENSION) &&
        s.store.getFreeCapacity(RESOURCE_ENERGY) > 0;
}

function work(creep, sources) {
    var mem = creep.memory;
    if (mem.working && creep.store[RESOURCE_ENERGY] == 0) mem.working = false;
    if (!mem.working && creep.store.getFreeCapacity() == 0) mem.working = true;

    if (!mem.working) {
        var source = sources[(mem.source || 0) % sources.length];
        if (creep.harvest(source) == ERR_NOT_IN_RANGE) creep.moveTo(source);
        return;
    }
    var room = creep.room;
    var controller = room.controller;
    if (mem.role == 'upgrader' && controller && controller.my) {
        if (creep.upgradeController(controller) == ERR_NOT_IN_RANGE) creep.moveTo(controller);
        return;
    }
    var sink = creep.pos.findClosestByRange(FIND_MY_STRUCTURES, {filter: needsEnergy});
    if (sink) {
        if (creep.transfer(sink, RESOURCE_ENERGY) == ERR_NOT_IN_RANGE) creep.moveTo(sink);
        return;
    }
    var site = creep.pos.findClosestByRange(FIND_MY_CONSTRUCTION_SITES);
    if (site) {
        if (creep.build(site) == ERR_NOT_IN_RANGE) creep.moveTo(site);
        return;
    }
    if (controller && controller.my && creep.upgradeController(controller) == ERR_NOT_IN_RANGE) creep.moveTo(controller);
}

module.exports.loop = function () {
    for (var name in Memory.creeps) {
        if (!Game.creeps[name]) delete Memory.creeps[name];
    }
    var spawn = Game.spawns[Object.keys(Game.spawns)[0]];
    if (!spawn) return;
    var sources = spawn.room.find(FIND_SOURCES);
    var count = 0;
    for (var n in Game.creeps) {
        var creep = Game.creeps[n];
        count++;
        if (!creep.spawning) work(creep, sources);
    }
    spawnWorkers(spawn, count);
    if (Game.time % 10 == 0) placeExtensions(spawn);
};
