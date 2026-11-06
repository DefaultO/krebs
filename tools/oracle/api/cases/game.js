// Game construction (02 GAME-*): keys, order, descriptors, memoized getters, registries, Game.shard.
'use strict';
const { basicWorld, probeMain, doc, ME, OTHER } = require('./_lib');

module.exports = [
    {
        name: 'game-shape',
        tags: ['game/game.js:136', 'game/game.js:171', 'game/game.js:174', 'game/game.js:435', 'runtime/runtime.js:216'],
        world: basicWorld(probeMain({
            keys: 'Object.keys(Game)',
            cpuKeys: 'Object.keys(Game.cpu)',
            cpu: '[Game.cpu.limit, Game.cpu.tickLimit, Game.cpu.bucket, Game.cpuLimit, typeof Game.cpu.getHeapStatistics, typeof Game.cpu.halt]',
            gcl: 'Game.gcl',
            gpl: 'Game.gpl',
            time: 'Game.time',
            resources: 'Game.resources',
            shard: '[Object.keys(Game.shard), Game.shard.name, Game.shard.type, Game.shard.ptr, Object.getPrototypeOf(Game.shard)]',
            creeps: 'Object.keys(Game.creeps)',
            spawns: 'Object.keys(Game.spawns)',
            structures: 'Object.keys(Game.structures).map(id => Game.structures[id].structureType)',
            rooms: 'Object.keys(Game.rooms)',
            byId: '[Game.getObjectById("nope"), typeof Game.getObjectById("toString"), Game.getObjectById(Game.spawns.Spawn1.id) === Game.spawns.Spawn1]',
            notifyOk: 'Game.notify("hello", 5)',
            constants: '[OK, ERR_FULL, typeof BODYPART_COST, Object.keys(global).includes("FIND_CREEPS")]',
        })),
        ticks: 2,
    },
    {
        name: 'game-construct-registry',
        tags: ['game/game.js:184', 'game/game.js:260', 'game/game.js:325', 'game/game.js:366', 'game/game.js:397'],
        world: basicWorld(probeMain({
            energy: '[Game.rooms.W1N1.energyAvailable, Game.rooms.W1N1.energyCapacityAvailable, Game.rooms.W2N1.energyAvailable]',
            memoKeys: 'Object.keys(Game.creeps.Harvester1)',
            siteKeys: 'Object.keys(Game.constructionSites).map(i => [Game.constructionSites[i].structureType, !!Game.constructionSites[i].room])',
            flags: 'Object.keys(Game.flags).map(n => [n, Game.flags[n].color, !!Game.flags[n].room])',
            flagById: 'Game.getObjectById("flag_Home") === Game.flags.Home',
            find: '[Game.rooms.W1N1.find(FIND_MY_CREEPS).length, Game.rooms.W1N1.find(FIND_HOSTILE_CREEPS).length]',
            owner: 'Game.rooms.W1N1.find(FIND_HOSTILE_CREEPS)[0].owner',
            controller: 'Game.rooms.W1N1.controller.my',
            dup: 'Object.keys(Game.creeps).length',
        }), {
            flags: [{ _id: 'f1', user: ME, room: 'W1N1', data: 'Home~1~2~10~12|Far$VLINE$Away~3~4~5~5' },
                { _id: 'f2', user: ME, room: 'W9N9', data: 'Lost~5~5~1~1' }],
        }),
        ticks: 1,
    },
];

// sites and duplicate names are added to the registry case's world
const reg = module.exports[1].world;
reg.objects.push(
    doc.site('W1N1', 30, 31, 'extension', ME),
    doc.site('W5N5', 30, 31, 'road', ME),
    doc.creep('W1N1', 12, 12, 'Harvester1', ME),
    doc.extension('W1N1', 27, 27, ME, { off: true }),
    doc.extension('W2N1', 27, 27, OTHER),
);
reg.rooms.push({ _id: 'W5N5', status: 'normal' });
