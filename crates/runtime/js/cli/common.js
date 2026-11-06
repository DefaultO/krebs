// not a mirror: the parts of @screeps/common (reference/common/index.js) the CLI files use: `storage`,
// `configManager.config.common.{constants, bots}`, `checkTerrain`, `getGametime`, `roomNameToXY`.
'use strict';
var natives = __cli;

exports.storage = require('./storage');

exports.configManager = {
    config: {
        common: {
            constants: require('@screeps/common/lib/constants'),
            // mods.json `bots` (name -> AI directory)
            bots: JSON.parse(natives.call('bots', '[]')),
        },
    },
};

// common/index.js:61
exports.checkTerrain = function(terrain, x, y, mask) {
    return (parseInt(terrain.charAt(y*50 + x)) & mask) > 0;
};

// common/index.js:65
exports.getGametime = function() {
    return exports.storage.env.get(exports.storage.env.keys.GAMETIME).then(data => parseInt(data));
};
