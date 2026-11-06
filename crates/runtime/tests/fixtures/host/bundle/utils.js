// Loader fixture, bundled as the engine utils module (sloppy).
exports.sloppy = (function () { return this === global; })();
exports.driver = require('~runtime-driver');
exports.name = 'utils';
