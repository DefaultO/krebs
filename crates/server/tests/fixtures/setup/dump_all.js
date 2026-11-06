// Everything Game setup builds, read through the API, a line per table: register and room tables through find, look*,
// getObjectById and the calls that read objectsByRoom, Game.* in key order, on every object in sight. Its only intents are
// the construction sites of the build test.
var FINDS = Object.keys(global).filter(function (k) { return k.indexOf('FIND_') === 0; }).sort();
var LOOKS = Object.keys(global).filter(function (k) { return k.indexOf('LOOK_') === 0; }).sort();
var CLASSES = ['Creep', 'PowerCreep', 'Source', 'Mineral', 'Deposit', 'Resource', 'Tombstone', 'Ruin', 'Nuke', 'ConstructionSite', 'Flag', 'StructureSpawn', 'StructureRampart', 'StructureRoad', 'StructureTower', 'StructureExtension', 'StructureContainer', 'StructureWall', 'StructureLink', 'StructureStorage', 'StructureObserver', 'StructurePowerSpawn', 'StructureLab', 'StructureTerminal', 'StructureNuker', 'StructureFactory', 'StructurePowerBank', 'StructurePortal', 'StructureKeeperLair', 'StructureInvaderCore', 'StructureController', 'StructureExtractor', 'OwnedStructure', 'Structure', 'RoomObject'];
var TYPES = ['spawn', 'extension', 'road', 'constructedWall', 'rampart', 'link', 'storage', 'tower', 'observer', 'powerSpawn', 'extractor', 'lab', 'terminal', 'container', 'nuker', 'factory'];

function print(tag, text) {
    console.log('probe ' + Game.time + ' ' + tag + ' ' + text);
}

function label(o) {
    return o && (o.id || o.name || o.code) || String(o);
}

function describe(o) {
    var kinds = CLASSES.filter(function (c) { return o instanceof global[c]; }).join('+');
    var out = [kinds, label(o), o.pos && o.pos.roomName + ':' + o.pos.x + ',' + o.pos.y, o.room && o.room.name, o.structureType, o.my, o.owner && o.owner.username, o.hits, o.hitsMax, Object.keys(o).join('/')];
    if (o.store) out.push(JSON.stringify(o.store), 'energy' in o.store, o.store.getCapacity('energy'), o.store.getUsedCapacity('energy'), o.store.H);
    if (o.isActive) out.push(o.isActive());
    if (o.spawning !== undefined) out.push(!!o.spawning);
    if (o.body) out.push(o.body.length);
    return out.join(' ');
}

function section(tag, list) {
    print(tag, list.length + ': ' + list.join(' | '));
}

function safely(tag, fn) {
    try {
        fn();
    } catch (e) {
        print(tag, 'threw ' + e);
    }
}

function dumpRoom(name) {
    var room = Game.rooms[name], all = [];
    print('room ' + name, [room.energyAvailable, room.energyCapacityAvailable, Object.keys(room).join('/'), !!room.controller, !!room.storage, !!room.terminal].join(' '));
    FINDS.forEach(function (k) {
        var found = room.find(global[k]);
        section(name + ' ' + k, found.map(function (o) { return label(o) + (o.x !== undefined ? '@' + o.x + ',' + o.y : ''); }));
    });
    room.find(FIND_STRUCTURES).concat(room.find(FIND_CREEPS), room.find(FIND_HOSTILE_POWER_CREEPS), room.find(FIND_MY_POWER_CREEPS), room.find(FIND_SOURCES), room.find(FIND_MINERALS), room.find(FIND_DEPOSITS), room.find(FIND_DROPPED_RESOURCES), room.find(FIND_TOMBSTONES), room.find(FIND_RUINS), room.find(FIND_NUKES), room.find(FIND_CONSTRUCTION_SITES), room.find(FIND_FLAGS)).forEach(function (o) {
        all.push(o);
        print('object', describe(o));
    });
    section(name + ' lookAtArea', room.lookAtArea(0, 0, 49, 49, true).filter(function (e) { return e.type !== 'terrain'; }).map(function (e) { return e.x + ',' + e.y + ':' + e.type + ':' + label(e[e.type]); }));
    LOOKS.forEach(function (k) {
        if (k === 'LOOK_TERRAIN') return;
        safely(name + ' ' + k, function () {
            section(name + ' ' + k, room.lookForAtArea(global[k], 0, 0, 49, 49, true).map(function (e) { return e.x + ',' + e.y + ':' + label(e[global[k]]); }));
        });
    });
    // tiles with more than one structure, a rampart over something: both ways of asking, and what building there answers
    var tiles = {};
    room.find(FIND_STRUCTURES).forEach(function (s) {
        var key = s.pos.x + ',' + s.pos.y;
        tiles[key] = (tiles[key] || 0) + 1;
    });
    Object.keys(tiles).filter(function (key) { return tiles[key] > 1; }).forEach(function (key) {
        var xy = key.split(','), x = +xy[0], y = +xy[1], pos = room.getPositionAt(x, y);
        section(name + ' tile ' + key, pos.lookFor(LOOK_STRUCTURES).map(label).concat(room.lookForAt(LOOK_STRUCTURES, x, y).map(label), pos.look().map(function (e) { return e.type + ':' + label(e[e.type]); })));
    });
    return all;
}

function paths(name) {
    var room = Game.rooms[name];
    [[8, 9, 24, 11], [3, 3, 45, 45], [18, 9, 18, 11], [9, 10, 13, 10], [41, 36, 41, 38]].forEach(function (p) {
        [true, false].forEach(function (ignoreCreeps) {
            var from = new RoomPosition(p[0], p[1], name), to = new RoomPosition(p[2], p[3], name);
            section(name + ' path ' + p + ' ' + ignoreCreeps, room.findPath(from, to, {ignoreCreeps: ignoreCreeps}).map(function (s) { return s.x + ',' + s.y; }));
        });
    });
    var matrix = PathFinder.search(new RoomPosition(8, 9, name), {pos: new RoomPosition(24, 11, name), range: 1}, {maxRooms: 1});
    section(name + ' search', matrix.path.map(function (s) { return s.x + ',' + s.y; }));
}

module.exports.loop = function () {
    print('keys Game', Object.keys(Game).join());
    ['creeps', 'powerCreeps', 'spawns', 'structures', 'flags', 'constructionSites', 'rooms'].forEach(function (k) {
        print('keys ' + k, Object.keys(Game[k]).join());
    });
    var seen = [];
    Object.keys(Game.rooms).forEach(function (name) {
        seen = seen.concat(dumpRoom(name));
    });
    section('byId', seen.map(function (o) {
        return o.id ? Game.getObjectById(o.id) === o : '-';
    }));
    section('Game.structures', Object.keys(Game.structures).map(function (k) { return describe(Game.structures[k]); }));
    section('Game.creeps', Object.keys(Game.creeps).map(function (k) { return describe(Game.creeps[k]); }));
    section('Game.spawns', Object.keys(Game.spawns).map(function (k) { return describe(Game.spawns[k]); }));
    section('Game.flags', Object.keys(Game.flags).map(function (k) { return describe(Game.flags[k]); }));
    section('Game.constructionSites', Object.keys(Game.constructionSites).map(function (k) { return describe(Game.constructionSites[k]); }));
    // ids that fall outside the list's rules, and rooms with objects but no vision
    section('short ids', ['1234', 'short1', 'tmp000000000000000000001', 'W3N1'].map(function (id) { var o = Game.getObjectById(id); return id + ':' + (o && describe(o)); }));
    Object.keys(Game.rooms).forEach(function (name) {
        var room = Game.rooms[name];
        safely(name + ' paths', function () { paths(name); });
        section(name + ' build', [[10, 10], [12, 10], [18, 10], [20, 10], [32, 32], [41, 37], [41, 35], [0, 0]].map(function (xy) {
            return TYPES.map(function (t) { return room.createConstructionSite(xy[0], xy[1], t) === OK ? 'ok' : 'no'; }).join('');
        }));
    });
    section('no vision', [String(Game.rooms.W3N1), Object.keys(Game.constructionSites).filter(function (k) { return Game.constructionSites[k].pos.roomName === 'W3N1'; }).length]);
};
