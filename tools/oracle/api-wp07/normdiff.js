'use strict';
// Token-level diff of a WP07 port against its vanilla file: comments and whitespace are ignored, so any
// reported hunk is a real code difference that must be a documented deviation.
//   node tools/oracle/api-wp07/normdiff.js [creeps structures ...]
const fs = require('fs');
const path = require('path');
const { reference } = require('../lib/common');
const { OWNED } = require('./side');

// Tokens: strings/templates/regex-ish literals kept whole, comments dropped, whitespace dropped.
function tokens(src) {
    const out = [];
    const re = /\/\/[^\n]*|\/\*[\s\S]*?\*\/|`(?:\\[\s\S]|[^`\\])*`|'(?:\\.|[^'\\])*'|"(?:\\.|[^"\\])*"|[A-Za-z_$][\w$]*|\d[\d.]*|\s+|[^\s]/g;
    let m;
    while ((m = re.exec(src))) {
        const t = m[0];
        if (/^\s/.test(t) || t.startsWith('//') || t.startsWith('/*')) continue;
        out.push(t);
    }
    return out;
}

function diff(a, b) {
    // LCS on token arrays (files are small enough for O(n*m) on a row-compressed table).
    const n = a.length, m = b.length;
    const dp = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) {
        dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
    const hunks = [];
    let i = 0, j = 0, cur = null;
    const flush = () => { if (cur) { hunks.push(cur); cur = null; } };
    while (i < n || j < m) {
        if (i < n && j < m && a[i] === b[j]) { flush(); i++; j++; continue; }
        cur = cur || { at: i, del: [], add: [] };
        if (j < m && (i >= n || dp[i][j + 1] >= dp[i + 1][j])) cur.add.push(b[j++]);
        else cur.del.push(a[i++]);
    }
    flush();
    return hunks.map(h => ({ context: a.slice(Math.max(0, h.at - 8), h.at).join(' '), del: h.del.join(' '), add: h.add.join(' ') }));
}

const names = process.argv.slice(2).length ? process.argv.slice(2) : OWNED;
let total = 0;
for (const f of names) {
    const ours = fs.readFileSync(path.join(__dirname, '..', '..', '..', 'crates/runtime/js/engine/game', f + '.js'), 'utf8');
    const vanilla = fs.readFileSync(reference(`engine/src/game/${f}.js`), 'utf8');
    const hunks = diff(tokens(vanilla), tokens(ours));
    total += hunks.length;
    console.log(`${f}.js: ${hunks.length} hunk(s)`);
    for (const h of hunks) console.log(`  after: ${h.context}\n   - ${h.del}\n   + ${h.add}`);
}
process.exit(total ? 1 : 0);
