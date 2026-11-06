// Regenerates flags.json: the flag string codec of reference/backend/lib/game/api/game.js (getFlags `parse`,
// lines 141-150, and setFlags' serializer, lines 178-181, copied verbatim; they are closures, not exports).
//   node gen-flags.js <SCREEPS_ROOT>   (node 22; lodash from the server's node_modules)
const path = require('path');
const fs = require('fs');
const _ = require(path.join(process.argv[2], 'node_modules', 'lodash'));
var parse = (data) => {
    if(!data.length) {
        return [];
    }
    return _.map(data.split("|"), j => {
        var item = j.split("~");
        item[0] = item[0].replace(/\$VLINE\$/g,"|").replace(/\$TILDE\$/g,"~");
        return item;
    });
};
var serialize = (flags) => _.map(flags, i => {
    i[0] = i[0].replace(/\|/g,"$VLINE$").replace(/~/g,"$TILDE$");
    return i.join("~");
}).join("|");
const inputs = ['', 'Flag1~1~2~10~10', 'a$VLINE$b~3~3~1~1|c$TILDE$d$TILDE$~4~5~2.5~3', 'x$VLINE$$VLINE$~1~1~1~1|y~2~2~2~2|z', 'only'];
const out = inputs.map(data => {
  const parsed = parse(data);
  return {data, parsed: JSON.parse(JSON.stringify(parsed)), reserialized: serialize(parsed)};
});
fs.writeFileSync(path.join(__dirname, 'flags.json'), JSON.stringify(out, null, 1) + '\n');
