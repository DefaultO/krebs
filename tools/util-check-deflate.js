// Dev check: node zlib must inflate util::deflate's output.
//   cargo test -p util --test deflate -- --ignored export_for_node
//   node tools/util-check-deflate.js
// Also prints our compressed size next to zlib level 6 for comparison.
'use strict';
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const dir = path.join(__dirname, '..', 'target', 'util-deflate-check');
let failed = 0;
for (const f of fs.readdirSync(dir).filter((f) => f.endsWith('.orig'))) {
  const base = f.slice(0, -5);
  const orig = fs.readFileSync(path.join(dir, f));
  const checks = { raw: zlib.inflateRawSync, zlib: zlib.inflateSync, gz: zlib.gunzipSync };
  for (const [ext, fn] of Object.entries(checks)) {
    const c = fs.readFileSync(path.join(dir, `${base}.${ext}`));
    let ok = false;
    try { ok = fn(c).equals(orig); } catch (e) { console.log(`${base}.${ext}: ${e.message}`); }
    if (!ok) { failed++; console.log(`FAIL ${base}.${ext}`); }
  }
  const ours = fs.statSync(path.join(dir, `${base}.raw`)).size;
  console.log(`${base}: ${orig.length} -> ours ${ours}, zlib -6 ${zlib.deflateRawSync(orig).length}`);
}
console.log(failed ? `${failed} failures` : 'all ok');
process.exit(failed ? 1 : 0);
