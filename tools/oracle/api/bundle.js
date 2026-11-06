// Bundle texts for both sides of the API oracle (ARCHITECTURE.md §7.3).
//   official: the reference runtime (driver/lib/runtime, engine/src/game, common, lodash, @screeps/pathfinding,
//             buffer) wrapped webpack-3 style into one `<runtime>` script whose entry is runtime.js.
//   ours:     exactly what runtime/src/host.rs builds (host/prelude.js header), from crates/runtime/js in
//             bundle.rs FILES order. HYBRID: a file that is still an empty stub (its owner has not landed it) is
//             replaced by its reference source plus the minimal HYBRID_PATCHES that adapt vanilla's runtimeData
//             reads to chitin's natives, so every landed file can be tested against official before the rest.
'use strict';
const fs = require('fs');
const path = require('path');
const { ROOT, reference } = require('../lib/common');

const SCREEPS_ROOT = process.env.SCREEPS_ROOT || 'E:/screeps/server';
const nodeModule = (rel) => path.join(SCREEPS_ROOT, 'node_modules', rel);
const read = (p) => fs.readFileSync(p, 'utf8').replace(/\r\n/g, '\n');

// npm packages the reference runtime bundles: [id, dir, main, files]
function packages() {
    const pf = reference('pathfinding');
    const pfFiles = ['index.js'];
    (function walk(d) {
        for (const e of fs.readdirSync(path.join(pf, d), { withFileTypes: true })) {
            const r = path.posix.join(d, e.name);
            if (e.isDirectory()) walk(r);
            else if (r.endsWith('.js')) pfFiles.push(r);
        }
    })('src');
    return [
        ['@screeps/pathfinding', pf, 'index.js', pfFiles],
        ['heap', nodeModule('heap'), 'index.js', ['index.js', 'lib/heap.js']],
        ['buffer', nodeModule('buffer'), 'index.js', ['index.js']],
        ['base64-js', nodeModule('base64-js'), 'index.js', ['index.js']],
        ['ieee754', nodeModule('ieee754'), 'index.js', ['index.js']],
        ['isarray', nodeModule('isarray'), 'index.js', ['index.js']],
    ];
}

// [id, source] of every module of a package. The bare id re-exports `./<last segment>/<main>` so relative
// requires inside the package resolve against `<id>/<file>` with the prelude's resolver.
function packageModules([id, dir, main, files]) {
    const base = id.split('/').pop();
    const out = [[id, `module.exports = require('./${base}/${main.replace(/\.js$/, '')}');`]];
    for (const f of files) out.push([`${id}/${f.replace(/\.js$/, '')}`, read(path.join(dir, f))]);
    return out;
}

// vanilla id -> reference file
function referenceModules() {
    const mods = [
        ['lodash', reference('lodash-3.10.1/lodash.js')],
        ['@screeps/common/lib/constants', reference('common/lib/constants.js')],
        ['@screeps/common/lib/system', reference('common/lib/system.js')],
        ['@screeps/engine/src/utils', reference('engine/src/utils.js')],
        ['@screeps/driver/lib/path-finder', reference('driver/lib/path-finder.js')],
    ];
    for (const f of ['runtime', 'runtime-driver', 'system-functions', 'mapgrid']) {
        mods.push([`@screeps/driver/lib/runtime/${f}`, reference(`driver/lib/runtime/${f}.js`)]);
    }
    for (const f of fs.readdirSync(reference('engine/src/game')).sort()) {
        if (f.endsWith('.js')) mods.push([`@screeps/engine/src/game/${f.slice(0, -3)}`, reference(`engine/src/game/${f}`)]);
    }
    return mods.map(([id, p]) => [id, read(p)]);
}

// The prelude's resolver, shared by the official loader (same semantics on both sides).
const RESOLVE = `function resolve(from, spec) {
    if (spec === '~runtime-driver') return '@screeps/driver/lib/runtime/runtime-driver';
    if (spec.charAt(0) !== '.') return spec;
    var parts = from.split('/'), segs = spec.split('/');
    parts.pop();
    for (var i = 0; i < segs.length; i++) {
        if (segs[i] === '..') parts.pop();
        else if (segs[i] !== '.') parts[parts.length] = segs[i];
    }
    return parts.join('/').replace(/\\.js$/, '');
}`;

// Official: webpack 3 semantics (lazy evaluation, partial exports stay installed on a throw, `global` and
// `Buffer` injected as wrapper parameters, `util` an empty shim, @screeps/core missing, lodash's AMD branch taken).
// ORACLE_REAL_BUNDLE (run.js --real-bundle): the installed @screeps/driver build/runtime.bundle.js itself — the
// exact webpack output, but built from the installed engine (4.3.0; the reference sources are 4.3.2).
function officialBundle() {
    if (process.env.ORACLE_REAL_BUNDLE) return read(nodeModule('@screeps/driver/build/runtime.bundle.js'));
    const mods = [...referenceModules(), ['util', 'module.exports = {};'], ...packages().flatMap(packageModules)];
    let s = `(function (G) {
var defs = Object.create(null), installed = Object.create(null);
${RESOLVE}
function load(id, spec) {
    if (installed[id]) return installed[id].exports;
    if (!defs[id]) { var e = new Error('Cannot find module "' + spec + '"'); e.code = 'MODULE_NOT_FOUND'; throw e; }
    var module = installed[id] = {i: id, l: false, exports: {}};
    var Buffer = id === '@screeps/driver/lib/runtime/runtime-driver' ? load('buffer', 'buffer').Buffer : undefined;
    var req = function require(s) { return load(resolve(id, s), s); }, define;
    if (id === 'lodash') {
        // webpack compiles lodash's \`typeof define == 'function' && typeof define.amd == 'object' && define.amd\`
        // to \`true\` and \`define(f)\` to \`module.exports = f.call(exports, require, exports, module)\`: lodash
        // runs \`root._ = _\` (the runtime's lodash becomes the global \`_\`) and never sets \`_._\`.
        define = function (f) { var r = f.call(module.exports, req, module.exports, module); if (r !== undefined) module.exports = r; };
        define.amd = {};
    }
    defs[id].call(module.exports, module, module.exports, req, G, Buffer, define);
    module.l = true;
    return module.exports;
}
`;
    for (const [id, src] of mods) s += `defs[${JSON.stringify(id)}] = function (module, exports, require, global, Buffer, define) {\n${src}\n};\n`;
    return s + `load('@screeps/driver/lib/runtime/runtime', 'runtime');\n})(this);\n`;
}

// --- ours -------------------------------------------------------------------------------------------------------

const JS = path.join(ROOT, 'crates', 'runtime', 'js');

// bundle.rs FILES, in order
function bundleFiles() {
    const rs = read(path.join(ROOT, 'crates', 'runtime', 'src', 'bundle.rs'));
    return [...rs.matchAll(/\("([^"]+)", include_str!/g)].map((m) => m[1]);
}

// "empty stub" = nothing but comments and directives
const isStub = (src) => src.replace(/\/\/[^\n]*/g, '').replace(/['"]use strict['"];?/g, '').trim() === '';

// our path -> reference file, for HYBRID substitution
function referencePath(p) {
    if (p.startsWith('engine/')) return reference(`engine/src/${p.slice(7)}`);
    if (p.startsWith('common/')) return reference(`common/lib/${p.slice(7)}`);
    if (p === 'driver/path-finder.js') return reference('driver/lib/path-finder.js');
    if (p.startsWith('driver/')) return reference(`driver/lib/runtime/${p.slice(7)}`);
    return null;
}

// Vanilla runtimeData fields chitin serves through natives instead (J1, J3): in a substituted reference file
// they read the case's world through the `~oracle` module. [file, from, to]; `from` must occur.
const HYBRID_PATCHES = [
    ['engine/game/rooms.js', 'TerrainConstructor || (()=>{', 'TerrainConstructor || (TerrainConstructor = Uint8Array) || (()=>{'],
    ['engine/game/map.js', 'JSON.parse(runtimeData.accessibleRooms)', "require('~oracle').accessibleRooms"],
    ['engine/game/map.js', 'runtimeData.roomStatusData', "require('~oracle').roomStatusData"],
    ['engine/game/structures.js', 'runtimeData.roomStatusData', "require('~oracle').roomStatusData"],
    ['engine/game/market.js', 'runtimeData.market.orders', "require('~oracle').market.orders"],
    ['engine/game/market.js', 'runtimeData.market.history', "require('~oracle').market.history"],
];

function hybridSource(file) {
    let src = read(referencePath(file));
    for (const [f, from, to] of HYBRID_PATCHES) {
        if (f !== file) continue;
        if (!src.includes(from)) throw new Error(`hybrid patch for ${file} no longer applies: ${from}`);
        src = src.split(from).join(to);
    }
    return src;
}

const define = (p, src) => `__chitinDefine(${JSON.stringify(p)}, function(module, exports, require) {\n${src}\n});\n`;

// runtime/src/host.rs blank_doc_comments: lodash's JSDoc blocks become empty lines (not @license ones)
function blankDocComments(src) {
    const lines = src.split('\n'), out = [];
    for (let i = 0; i < lines.length; i++) {
        if (lines[i].trim() === '/**') {
            let j = i + 1;
            while (j < lines.length && lines[j].trimStart().startsWith('*') && lines[j].trim() !== '*/') j++;
            if (j < lines.length && lines[j].trim() === '*/' && !lines.slice(i, j + 1).some((l) => l.includes('@license'))) {
                for (let k = i; k <= j; k++) out.push('');
                i = j;
                continue;
            }
        }
        out.push(lines[i]);
    }
    return out.join('\n');
}

// `oracleData` = {accessibleRooms, roomStatusData, market} of the case world (the `~oracle` module); `mods` =
// {customObjectPrototypes, customIntentTypes} registered through a JS-mod game slot (mods/oracle.js).
// Returns {text, substituted: [paths replaced by reference sources]}.
function ourBundle(oracleData, mods) {
    const substituted = [];
    let text = read(path.join(JS, 'host', 'prelude.js')) + '\n';
    // [{path, start, end, substituted}]: where each file's source sits in `text` (run.js --coverage)
    const modules = [{ path: 'host/prelude.js', start: 0, end: text.length - 1, substituted: false }];
    for (const p of bundleFiles()) {
        if (p.startsWith('host/') || p.startsWith('cli/')) continue;
        let src = read(path.join(JS, p));
        if (p === 'vendor/lodash.js') src = blankDocComments(src);
        if (isStub(src) && referencePath(p)) {
            src = hybridSource(p);
            substituted.push(p);
        }
        const start = text.length + define(p, '').indexOf('\n') + 1;
        modules.push({ path: p, start, end: start + src.length, substituted: substituted.includes(p) });
        text += define(p, src);
    }
    const constants = read(path.join(ROOT, 'crates', 'engine', 'tests', 'fixtures', 'constants.json'));
    text += define('common/constants.js', `module.exports = ${constants};`);
    if (mods && (mods.customObjectPrototypes.length || Object.keys(mods.customIntentTypes).length)) {
        // a mod may reach engine modules while it loads: the runtime driver must already see every bootstrap global
        text += define('mods/oracle.js', `if (!require('@screeps/engine/src/utils').getRuntimeDriver().natives) throw new Error('mod loaded before the natives');
exports.customObjectPrototypes = ${JSON.stringify(mods.customObjectPrototypes)};
exports.customIntentTypes = ${JSON.stringify(mods.customIntentTypes)};`);
    }
    if (substituted.length) {
        // reference packages the substituted vanilla files need (not part of chitin's bundle)
        text += define('util', 'module.exports = {};');
        for (const [id, src] of packages().slice(0, 2).flatMap(packageModules)) text += define(id, src);
        text += define('~oracle', `module.exports = ${JSON.stringify(oracleData)};`);
    }
    return { text, substituted, modules };
}

module.exports = { officialBundle, ourBundle, bundleFiles, isStub, SCREEPS_ROOT };
