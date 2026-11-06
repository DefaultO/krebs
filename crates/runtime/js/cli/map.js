// mirror: reference/backend/lib/cli/map.js
// parity: FAITHFUL
// Verified against the official CLI by crates/server/tests/fixtures/oracle/cli (tools/oracle/cli.js).
//
// The map edits run in the server: mapgen.rs ports generateRoom, openRoom, closeRoom, removeRoom and
// updateTerrainData statement by statement and owns the terrain and the room list (openRoom keeps its timestamp on
// the virtual clock, CLOCK-VIRTUAL); the backend renders the map PNGs from terrain, so updateRoomImageAssets has
// nothing to write. Results are map.js's (`{modified: 1}` / `{}` from the rooms update).
const common = require('@screeps/common');
const db = common.storage.db;
const utils = require('../utils');
const q = require('q');
const natives = __cli;

function call(name, args) {
    // a server-side failure is a rejection with its message, as vanilla's string rejections
    return new Promise((res, rej) => {
        try {
            res(JSON.parse(natives.call(name, JSON.stringify(args))));
        } catch (e) {
            rej(e.message);
        }
    });
}

exports.generateRoom = utils.withHelp([
    `generateRoom(roomName, [opts]) - Generate a new room at the specified location. 'opts' is an object with the following optional properties:\r
    * exits - an object with exit coordinates arrays, e.g. {top: [20,21,23], right: [], bottom: [27,28,29,40,41]}, default is random\r
    * terrainType - the type of generated landscape, a number from 1 to 28, default is random\r
    * swampType - the type of generated swamp configuration, a number from 0 to 14, default is random\r
    * sources - the amount of sources in the room, default is random from 1 to 2\r
    * mineral - the type of the mineral deposit in this room or false if no mineral, default is random type\r
    * controller - whether this room should have the controller, default is true\r
    * keeperLairs - whether this room should have source keeper lairs, default is false`,
    function generateRoom(roomName, opts) {
        opts = opts || {};

        opts.exits = opts.exits || {};

        if(!/^[WE]\d+[NS]\d+$/.test(roomName)) {
            return q.reject("Invalid room name");
        }

        return call('generateRoom', [roomName, opts])
            .then(() => exports.updateRoomImageAssets(roomName))
            .then(() => exports.updateTerrainData())
            .then(() => 'OK');
    }
]);

exports.openRoom = utils.withHelp([
    "openRoom(roomName, [timestamp]) - Make a room available for use. Specify a timestamp in the future if you want it to be opened later automatically.",
    function openRoom(roomName, timestamp) {

        return call('openRoom', [roomName, timestamp]);
    }
]);

exports.closeRoom = utils.withHelp([
    "closeRoom(roomName) - Make a room not available.",
    function closeRoom(roomName) {
        return call('closeRoom', [roomName]);
    }
]);

exports.removeRoom = utils.withHelp([
    "removeRoom(roomName) - Delete the room and all room objects from the database.",
    function removeRoom(roomName) {
        return db.rooms.findOne({_id: roomName})
            .then(data => !data ? q.reject(`The room ${roomName} is not found`) :
                call('removeRoom', [roomName])
                    .then(() => exports.updateTerrainData())
                    .then(() => call('removeRoomImage', [roomName]))
                    .then(() => 'OK'));
    }
]);

exports.updateRoomImageAssets = utils.withHelp([
    "updateRoomImageAssets(roomName) - Update images in assets folder for the specified room.",
    function updateRoomImageAssets(roomName) {
        return call('updateRoomImageAssets', [roomName]);
    }
]);

exports.updateTerrainData = utils.withHelp([
    "updateTerrainData() - Update cached world terrain data.",
    function updateTerrainData() {
        return call('updateTerrainData', []).then(() => 'OK');
    }
]);


exports._help = utils.generateCliHelp('map.', exports);
