// Host-contract fixture prelude for the runtime conformance tests (not the game runtime prelude), in the
// runtime bundle format: top-level code only registers the modules the host appends with __chitinDefine.
// _init(natives) loads the fixture runtime, whose own _init(natives) returns __tick; _initSystem(natives)
// loads the fixture __store. require resolves js paths, the vanilla package prefixes, lodash and
// ~runtime-driver; a module that threw stays cached (webpack 3).
// (No file paths in these comments: tools/parity-report counts them as fixture tags.)
(function (G) {
var modules = Object.create(null);
var hasOwn = Object.prototype.hasOwnProperty;
var cache = Object.create(null);
var aliases = {"lodash": "vendor/lodash.js", "~runtime-driver": "driver/runtime-driver.js"};
var prefixes = [["@screeps/engine/src/", "engine/"], ["@screeps/common/lib/", "common/"],
    ["@screeps/driver/lib/runtime/", "driver/"], ["@screeps/driver/lib/", "driver/"]];
function file(id) { return hasOwn.call(modules, id) ? id : hasOwn.call(modules, id + ".js") ? id + ".js" : null; }
function norm(path) {
    var out = [], parts = path.split("/");
    for (var i = 0; i < parts.length; i++) {
        if (parts[i] === "" || parts[i] === ".") continue;
        if (parts[i] === "..") out.pop(); else out.push(parts[i]);
    }
    return out.join("/");
}
function dirname(p) { return p.slice(0, p.lastIndexOf("/") + 1); }
function fromVanilla(p) {
    for (var i = 0; i < prefixes.length; i++) {
        if (p.indexOf(prefixes[i][0]) === 0) return prefixes[i][1] + p.slice(prefixes[i][0].length);
    }
    return p;
}
function toVanilla(id) {
    if (id === "driver/path-finder.js") return "@screeps/driver/lib/path-finder.js";
    for (var i = 0; i < prefixes.length; i++) {
        if (id.indexOf(prefixes[i][1]) === 0) return prefixes[i][0] + id.slice(prefixes[i][1].length);
    }
    return id;
}
function resolve(from, req) {
    if (req.charAt(0) === ".") {
        return file(norm(dirname(from) + req)) || file(fromVanilla(norm(dirname(toVanilla(from)) + req)));
    }
    if (hasOwn.call(aliases, req)) return aliases[req];
    return file(fromVanilla(req));
}
function load(id) {
    if (cache[id]) return cache[id].exports;
    var module = cache[id] = {id: id, exports: {}, loaded: false};
    modules[id].call(module.exports, module, module.exports, function require(req) {
        var to = resolve(id, "" + req);
        if (to === null) {
            var e = new Error("Cannot find module '" + req + "'");
            e.code = "MODULE_NOT_FOUND";
            throw e;
        }
        return load(to);
    });
    module.loaded = true;
    return module.exports;
}
G.global = G;
G.__chitinDefine = function (path, fn) { modules[path] = fn; };
G._init = function (natives) {
    delete G.__chitinDefine;
    load("driver/runtime.js");   // replaces global._init with the fixture runtime's
    return G._init(natives);
};
G._initSystem = function () {
    delete G.__chitinDefine;
    delete G._init;
    delete G._initSystem;
    load("host/system.js");
};
})(this);
