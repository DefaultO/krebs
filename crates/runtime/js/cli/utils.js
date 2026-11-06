// not a mirror: the functions of reference/backend/lib/utils.js the CLI files call, ported verbatim
// (withHelp, generateCliHelp, translateModulesToDb, reloadBotUsers, activateRoom) or answered by the server
// (loadBot reads the bot directory; respawnUser is the backend's own port, which replicates its quirks).
'use strict';
const common = require('@screeps/common');
const db = common.storage.db;
const env = common.storage.env;
const natives = __cli;

// utils.js:62
exports.translateModulesToDb = function(modules) {
    modules = modules || {};

    for(var key in modules) {
        var newKey = key.replace(/\./g,'$DOT$');
        newKey = newKey.replace(/\//g,'$SLASH$');
        newKey = newKey.replace(/\\/g,'$BACKSLASH$');

        if(newKey[0] == '$') {
            delete modules[key];
            continue;
        }

        if(newKey != key) {
            modules[newKey] = modules[key];
            delete modules[key];
        }
    }

    if(!modules.main) {
        modules.main = '';
    }

    return modules;
};

// utils.js:115 (GEN-RESPAWN)
exports.respawnUser = function(userId) {
    return new Promise((res, rej) => {
        try {
            res(JSON.parse(natives.call('respawnUser', JSON.stringify([userId]))));
        } catch (e) {
            rej(e.message);
        }
    });
};

// utils.js:170
exports.withHelp = function(array) {
    var fn = array[1];
    fn._help = array[0];
    return fn;
};

// utils.js:176
exports.generateCliHelp = function(prefix, container) {
    return `Available methods:\r\n`+Object.keys(container).filter(i => typeof container[i] == 'function').map(i => ' - ' + prefix + (container[i]._help || i)).join('\r\n');
};

// utils.js:250: the bot directory's `*.js` files (main.js required); the errors are vanilla's
exports.loadBot = function(name) {
    var modules = JSON.parse(natives.call('loadBot', JSON.stringify([name])));
    return exports.translateModulesToDb(modules);
};

// utils.js:271
exports.reloadBotUsers = function(name) {

    return db.users.find({bot: name})
        .then(users => {
            if(!users.length) {
                return 'No bot players found';
            }
            var modules = exports.loadBot(name);
            var timestamp = Date.now();

            return db['users.code'].insert(users.map(i => ({
                user: i._id,
                branch: 't'+timestamp,
                timestamp,
                activeWorld: true,
                activeSim: true,
                modules
            })))
            .then(() => db['users.code'].removeWhere({$and: [
                {user: {$in: users.map(i => i._id)}},
                {branch: {$ne: 't'+timestamp}}
            ]}))
                .then(() => 'Reloaded scripts for users: '+users.map(i => i.username).join(', '));
        })
};

// utils.js:311
exports.activateRoom = function activateRoom(room) {
    return env.sadd(env.keys.ACTIVE_ROOMS, room);
};
