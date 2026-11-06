// Regenerates colors.json with the official utils.createTerrainColorsMap(terrain, false):
//   node gen-colors.js <SCREEPS_ROOT, e.g. E:\screeps\server>   (node 22)
const path = require('path');
const fs = require('fs');
const utils = require(path.join(process.argv[2], 'node_modules', '@screeps', 'backend', 'lib', 'utils'));
let terrain = '';
for (let y = 0; y < 50; y++) {
  for (let x = 0; x < 50; x++) {
    // walls on most of the border, swamps, both bits, a few open border tiles
    const border = x == 0 || y == 0 || x == 49 || y == 49;
    let c = border ? ((x + y) % 7 == 0 ? 0 : 1) : ((x * 7 + y * 3) % 11 == 0 ? 2 : (x * y) % 13 == 0 ? 3 : (x + 2 * y) % 17 == 0 ? 1 : 0);
    terrain += c;
  }
}
const map = utils.createTerrainColorsMap(terrain, false);
const colors = [];
for (let y = 0; y < 50; y++) {
  colors.push([]);
  for (let x = 0; x < 50; x++) colors[y].push(map[y][x]);
}
fs.writeFileSync(path.join(__dirname, 'colors.json'), JSON.stringify({terrain, colors}) + '\n');
