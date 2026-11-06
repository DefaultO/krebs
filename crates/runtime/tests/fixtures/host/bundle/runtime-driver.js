// Loader fixture, bundled as the driver's runtime-driver module: `../path-finder` only resolves in the vanilla
// layout (driver/lib/runtime -> driver/lib), and the common system module through its package path.
exports.pf = require('../path-finder');
exports.system = require('@screeps/common/lib/system');
exports.name = 'runtime-driver';
