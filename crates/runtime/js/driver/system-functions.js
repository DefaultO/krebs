// mirror: reference/driver/lib/runtime/system-functions.js
// parity: FAITHFUL
// vanilla: system-functions.js:1 (GLB-SYSFUNC) - builtins captured at _init, before any player code.
const systemFunctions = {
    Object: {
        create: Object.create,
        keys: Object.keys,
        setPrototypeOf: Object.setPrototypeOf
    },
    JSON: {
        stringify: JSON.stringify,
        parse: JSON.parse
    },
    Math: {
        ceil: Math.ceil
    },
    Array: {
        push: Function.call.bind(Array.prototype.push),
        join: Function.call.bind(Array.prototype.join)
    }
};

// vanilla: system-functions.js:19
function freezeDeep(obj) {
    Object.freeze(obj);
    for(const p of Object.getOwnPropertyNames(obj)) {
        if(typeof obj[p] === 'object') {
            freezeDeep(obj[p]);
        }
    }
}
freezeDeep(systemFunctions);

Object.assign(exports, systemFunctions);
