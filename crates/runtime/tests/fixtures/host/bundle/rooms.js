'use strict';
// Loader fixture, bundled as the engine's game rooms module: relative, package and vanilla-relative requires.
var utils = require('../utils');
exports.strict = (function () { return this === undefined; })();
exports.self = require('./rooms') === module.exports;
exports.utils = utils.name;
exports.driver = require('~runtime-driver').name;
exports.pathFinder = require('@screeps/engine/src/utils').driver.pf.name;
exports.lodash = require('lodash') === require('../../vendor/lodash');
exports.constantsOK = require('@screeps/common/lib/constants').OK;
exports.thisIsExports = this === exports;
exports.missing = (function () {
    try {
        require('./nope');
    } catch (e) {
        return e.message + ' ' + e.code;
    }
})();
exports.name = 'rooms';
