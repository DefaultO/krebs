// A small bot to get you into a match. It is yours: change anything and upload it again.
// Screeps calls the loop below once per tick. This one makes a few workers. They take energy
// from a source, bring it to the spawn so that it can make more, and then give it to the
// room's controller.
// The game's own guide is at https://docs.screeps.com

const WORKERS = 6;

module.exports.loop = function () {
  for (const name in Memory.creeps) {
    if (!Game.creeps[name]) delete Memory.creeps[name];
  }

  const creeps = Object.values(Game.creeps);
  if (creeps.length < WORKERS) {
    for (const spawn of Object.values(Game.spawns)) {
      spawn.spawnCreep([WORK, CARRY, MOVE], 'worker' + Game.time);
    }
  }

  for (const creep of creeps) {
    work(creep);
  }
};

function work(creep) {
  if (creep.store.getUsedCapacity() === 0) creep.memory.full = false;
  if (creep.store.getFreeCapacity() === 0) creep.memory.full = true;

  if (!creep.memory.full) {
    const source = creep.pos.findClosestByPath(FIND_SOURCES_ACTIVE);
    if (source && creep.harvest(source) === ERR_NOT_IN_RANGE) creep.moveTo(source);
    return;
  }

  const hungry = creep.pos.findClosestByPath(FIND_MY_SPAWNS, {
    filter: spawn => spawn.store.getFreeCapacity(RESOURCE_ENERGY) > 0,
  });
  if (hungry) {
    if (creep.transfer(hungry, RESOURCE_ENERGY) === ERR_NOT_IN_RANGE) creep.moveTo(hungry);
    return;
  }

  const controller = creep.room.controller;
  if (controller && creep.upgradeController(controller) === ERR_NOT_IN_RANGE) creep.moveTo(controller);
}
