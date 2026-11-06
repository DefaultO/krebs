// Regenerates svg.json with the official badge.getBadgeSvg:
//   node gen-svg.js <SCREEPS_ROOT, e.g. E:\screeps\server>   (node 22)
const path = require('path');
const fs = require('fs');
// badge.js only needs lodash; its module also exports the palette through getBadgeSvg's colour lookup
const badge = require(path.join(process.argv[2], 'node_modules', '@screeps', 'backend', 'lib', 'game', 'api', 'badge'));
const cases = [];
let k = 0;
for (let type = 1; type <= 24; type++) {
    for (const param of [-100, -37, 0, 41.5, 100]) {
        for (const flip of [false, true]) {
            const b = {type, color1: '#112233', color2: k % 3 == 0 ? 25 : '#445566', color3: k % 5 == 0 ? 79 : '#778899', param, flip};
            const border = k++ % 2 == 0;
            cases.push({badge: b, border, svg: badge.getBadgeSvg(b, border)});
        }
    }
}
const custom = {type: {path1: 'M 0 0 L 10 10', path2: ''}, color1: 0, color2: '#abcdef', color3: 40, param: 0, flip: true};
cases.push({badge: custom, border: true, svg: badge.getBadgeSvg(custom, true)});
// the palette, read back through numeric colours
const palette = [];
for (let i = 0; i < 80; i++) {
    const svg = badge.getBadgeSvg({type: 1, color1: i, color2: i, color3: i, param: 0}, false);
    palette.push(/fill="(#[0-9a-f]{6})"/.exec(svg)[1]);
}
fs.writeFileSync(path.join(__dirname, 'svg.json'), JSON.stringify({palette, cases}) + '\n');
