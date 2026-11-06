// mirror: reference/backend/lib/cli/system.js
// parity: FAITHFUL
// Verified against the official CLI by crates/server/tests/fixtures/oracle/cli (tools/oracle/cli.js).
//
// resetAllData regenerates the world `chitin new` described (vanilla reloads storage's db.original.json);
// runCronjob runs the engine's job (cron::cronjobs::JOBS, mod crons). chitin extras (ARCHITECTURE.md §8.6):
// runTicks, save, resetUserRuntime.
const util = require('util');
const common = require('@screeps/common');
const config = common.configManager.config;
const C = config.common.constants;
const db = common.storage.db;
const env = common.storage.env;
const pubsub = common.storage.pubsub;
const q = require('q');
const _ = require('lodash');
const utils = require('../utils');
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

exports.resetAllData = utils.withHelp([
    "resetAllData() - Wipe all world data and reset the database to the default state.",
    function resetAllData() {
        return common.storage.resetAllData();
    }
]);

exports.sendServerMessage = utils.withHelp([
    'sendServerMessage(message) - Send a text server message to all currently connected players.',
    function sendServerMessage(message) {
        return pubsub.publish('serverMessage', message);
    }
]);

exports.pauseSimulation = utils.withHelp([
    'pauseSimulation() - Stop main simulation loop execution.',
    function pauseSimulation() {
        return env.set(env.keys.MAIN_LOOP_PAUSED, '1').then(() => 'OK');
    }
]);

exports.resumeSimulation = utils.withHelp([
    'resumeSimulation() - Resume main simulation loop execution.',
    function resumeSimulation() {
        return env.set(env.keys.MAIN_LOOP_PAUSED, '0').then(() => 'OK');
    }
]);

exports.runCronjob = utils.withHelp([
    'runCronjob(jobName) - Run a cron job immediately.',
    function runCronjob(jobName) {
        if(!JSON.parse(natives.call('cronjobs', '[]')).includes(jobName)) {
            return q.reject(`Cronjob "${jobName}" not found`);
        }

        return q.when(call('runCronjob', [jobName])).then(() => 'OK');
    }
]);

exports.getTickDuration = utils.withHelp([
    'getTickDuration() - Show current minimal tick duration (in milliseconds).',
    function getTickDuration() {
        return env.get(env.keys.MAIN_LOOP_MIN_DURATION);
    }
]);

exports.setTickDuration = utils.withHelp([
    'setTickDuration(minimalDuration) - Set current minimal tick duration (in milliseconds).',
    function setTickDuration(minimalDuration) {
        const value = parseInt(minimalDuration);
        if(Number.isNaN(value) || !value) {
            return q.reject(`Invalid minimal duration ${minimalDuration}, integer expected.`)
        }
        return env.set(env.keys.MAIN_LOOP_MIN_DURATION, value)
            .then(pubsub.publish('setTickRate', value))
            .then(() => 'OK');
    }
]);

exports._help = utils.generateCliHelp('system.', exports);

// chitin extras (ARCHITECTURE.md §8.6), defined after `_help` so help(system) stays vanilla's text
exports.runTicks = utils.withHelp([
    'runTicks(n) - Run n ticks while the simulation is paused (Krebs).',
    function runTicks(n) {
        return call('runTicks', [n]).then(() => 'OK');
    }
]);

exports.save = utils.withHelp([
    'save() - Write a checkpoint of the world now (Krebs).',
    function save() {
        return call('save', []).then(() => 'OK');
    }
]);

exports.resetUserRuntime = utils.withHelp([
    'resetUserRuntime(username) - Restart the VM of a player (Krebs).',
    function resetUserRuntime(username) {
        return call('resetUserRuntime', [username]).then(() => 'OK');
    }
]);
