// One comparable shape for a run of either side (ARCHITECTURE.md §7.3): intentsList (deep, key order, via the
// graph codec), Memory string, console, visuals, segments, segment settings and the error text with every
// runtime-internal stack frame removed. CPU fields (usedTime, usedCleanTime, usedDirtyTime) are not compared;
// intentsCpu is (it is exact: 0.2 per intent).
'use strict';
const { encode } = require('../lib/common');

// frames of the runtime itself (both sides' bundles are `<runtime>`; CON-ERR-FORMAT escapes `<` to `&lt;`), of
// node internals and of the oracle's own host code (absolute paths)
const FRAME = /^\s*at /;
const RUNTIME_FRAME = /<runtime>|&lt;runtime>|<isolated-vm>|&lt;isolated-vm>|\(node:|[A-Za-z]:[\\/]|\(\//;
// builtin frames below the last user frame belong to the host's call into the run (oracle harness only)
const BUILTIN_FRAME = /\((<anonymous>|&lt;anonymous>)\)\s*$/;

function errorText(text) {
    if (text === undefined || text === null) return text === undefined ? null : text;
    const lines = String(text).split('\n').filter((l) => !(FRAME.test(l) && RUNTIME_FRAME.test(l)));
    let end = lines.length && lines[lines.length - 1] === '' ? lines.length - 1 : lines.length;
    while (end && FRAME.test(lines[end - 1]) && BUILTIN_FRAME.test(lines[end - 1])) lines.splice(--end, 1);
    return lines.join('\n');
}

function graph(v) {
    try {
        return encode(v, { holes: true });
    } catch (e) {
        return ['unencodable', String(e.message)];
    }
}

// raw = {status, out?, error?} from vms.js
function normalize(raw) {
    if (!raw.out) return { status: raw.status, error: errorText(raw.error) };
    const o = raw.out;
    const r = {
        status: raw.status,
        error: errorText(o.error),
        memory: typeof o.memory === 'string' ? o.memory : o.memory && o.memory.data,
        intentsList: graph(o.intentsList),
        intentsCpu: o.intentsCpu,
        // probes often log caught errors' stacks: their runtime frames are not compared either
        console: o.console && {
            log: (o.console.log || []).map((m) => Object.assign({}, m, { message: errorText(m.message) })),
            results: (o.console.results || []).map(errorText),
        },
    };
    for (const k of ['memorySegments', 'visual', 'activeSegments', 'activeForeignSegment', 'defaultPublicSegment', 'publicSegments']) {
        if (o[k] !== undefined) r[k] = graph(o[k]);
    }
    return r;
}

// first difference as a path, or null
function diff(a, b, at = '$') {
    if (Object.is(a, b)) return null;
    if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null || Array.isArray(a) !== Array.isArray(b)) {
        return `${at}: ${JSON.stringify(a)} != ${JSON.stringify(b)}`;
    }
    const ka = Object.keys(a), kb = Object.keys(b);
    if (ka.join('\u0000') !== kb.join('\u0000')) return `${at} keys: ${JSON.stringify(ka)} != ${JSON.stringify(kb)}`;
    for (const k of ka) {
        const d = diff(a[k], b[k], `${at}.${k}`);
        if (d) return d;
    }
    return null;
}

module.exports = { normalize, diff, errorText };
