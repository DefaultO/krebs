// Regenerates modules.json with the official translateModulesToDb/FromDb:
//   node gen-modules.js <SCREEPS_ROOT, e.g. E:\screeps\server>   (node 22)
const path = require('path');
const fs = require('fs');
const utils = require(path.join(process.argv[2], 'node_modules', '@screeps', 'backend', 'lib', 'utils'));
const inputs = [
  {main: 'x', 'a.b': 1},
  {'lib/x.js': 2, '.hidden': 3, $dollar: 4, main: ''},
  {'a\\b': 1, '/abs': 2, 'x$DOT$y': 5, 'x.y': 6},
  {},
  {'1': 'n', 'b.c': 2, '0': 'z', main: 0},
  {'7.5': 'num', 'q$SLASH$r': 1, 's$BACKSLASH$t': 2, main: null},
  {'a.b.c': {binary: 'AAAA'}, main: 'm'},
];
const clone = o => JSON.parse(JSON.stringify(o));
const out = inputs.map(i => ({in: i, toDb: utils.translateModulesToDb(clone(i)), fromDb: utils.translateModulesFromDb(clone(i))}));
fs.writeFileSync(path.join(__dirname, 'modules.json'), JSON.stringify(out, null, 1) + '\n');
