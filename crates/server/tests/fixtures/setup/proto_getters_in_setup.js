// As proto_setter_in_setup.js, for the names Game setup's fast path skips reading: `off`, read on every doc, and a
// structure's type, read in the table of custom object types.
module.exports.loop = function () {
    console.log('probe', Game.time, global.off, global.road);
    global.off = global.road = 0;
    if (!global.armed) {
        global.armed = true;
        Object.defineProperty(StructureController.prototype, '_my', {configurable: true, get() { return this.__m; },
            set(v) {
                this.__m = v;
                Object.defineProperty(Object.prototype, 'off', {configurable: true, get() { global.off++; return undefined; }});
                Object.defineProperty(Object.prototype, 'controller', {configurable: true, get() { global.road++; return undefined; }});
                Object.defineProperty(Object.prototype, 'spawn', {configurable: true, get() { global.road++; return undefined; }});
            }});
    }
    delete Object.prototype.off;
    delete Object.prototype.controller;
    delete Object.prototype.spawn;
};
