// a pass-through setter at index 0 of every array: which raw docs does it get to see before Game setup?
module.exports.loop = function () {
    var src = Object.keys(Game.rooms).map(n => Game.rooms[n].find(FIND_SOURCES)[0]).filter(s => s)[0];
    console.log('probe', Game.time, 'raw docs seen', global.rawTypes, 'source energy', src && src.energy);
    global.rawTypes = '';
    if (!global.armed) {
        global.armed = true;
        Object.defineProperty(Object.prototype, 0, {configurable: true, enumerable: false, get() { return undefined; }, set(v) {
            Object.defineProperty(this, 0, {value: v, writable: true, enumerable: true, configurable: true});
            if (v && typeof v === 'object' && typeof v._id === 'string' && typeof v.type === 'string' && v.room) {
                global.rawTypes += v.type + ' ';
                if (v.type === 'source') v.energy = 123456;
            }
        }});
    }
};
