// Writes crates/engine/src/strongholds.json from reference/common/lib/strongholds.js (stronghold templates,
// core/container rewards, amounts, densities): game data, embedded by crates/engine/src/strongholds.rs.
//   node tools/engine-gen-strongholds.js
'use strict';
const fs = require('fs');
const path = require('path');
const root = path.join(__dirname, '..');
const data = require(path.join(root, 'reference/common/lib/strongholds.js'));
fs.writeFileSync(path.join(root, 'crates/engine/src/strongholds.json'), JSON.stringify(data) + '\n');
