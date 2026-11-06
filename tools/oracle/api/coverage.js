// `run.js --coverage`: V8 precise coverage (inspector Profiler) over our bundle scripts, mapped back to the
// crates/runtime/js files they came from. Lists every function of a landed (non-substituted) file that no case
// called — the §7.3 coverage gate's to-do list ("≥ 1 case per exported/prototype method of every file").
'use strict';
const inspector = require('inspector');

const session = new inspector.Session();
const scripts = new Map();   // script url -> [{path, start, end, source}]

function post(method, params) {
    let res, err;
    session.post(method, params, (e, r) => { err = e; res = r; });
    if (err) throw err;
    return res;
}

function start() {
    session.connect();
    post('Profiler.enable');
    post('Profiler.startPreciseCoverage', { callCount: true, detailed: false });
}

// one compiled bundle: `modules` = [{path, start, end, substituted}] offsets of each file's source in it
function register(url, text, modules) {
    scripts.set(url, modules.filter((m) => !m.substituted).map((m) => Object.assign({ source: text.slice(m.start, m.end) }, m)));
}

function report(filter) {
    const { result } = post('Profiler.takePreciseCoverage');
    const fns = new Map();   // "path:offset" -> {path, line, name, text, count}
    for (const s of result) {
        const mods = scripts.get(s.url);
        if (!mods) continue;
        for (const f of s.functions) {
            const r = f.ranges[0];
            const m = mods.find((x) => r.startOffset >= x.start && r.startOffset < x.end);
            if (!m) continue;
            const off = r.startOffset - m.start;
            const key = `${m.path}:${off}`;
            if (!fns.has(key)) {
                const before = m.source.slice(0, off);
                const line = before.split('\n').length;
                const text = m.source.slice(off).split('\n')[0].trim().slice(0, 70);
                fns.set(key, { path: m.path, line, name: f.functionName, text, count: 0 });
            }
            fns.get(key).count += r.count;
        }
    }
    const byFile = new Map();
    for (const f of fns.values()) {
        if ((filter && !f.path.includes(filter)) || f.path.startsWith('vendor/')) continue;
        if (!byFile.has(f.path)) byFile.set(f.path, { total: 0, missed: [] });
        const e = byFile.get(f.path);
        e.total++;
        if (!f.count) e.missed.push(f);
    }
    console.log('\ncoverage (functions of landed files never called by any case):');
    for (const [p, e] of [...byFile].sort()) {
        console.log(`  ${p}: ${e.total - e.missed.length}/${e.total} functions called`);
        for (const f of e.missed.sort((a, b) => a.line - b.line)) console.log(`      ${p}:${f.line} ${f.name || '(anonymous)'}  ${f.text}`);
    }
}

module.exports = { start, register, report, enabled: () => session !== null };
