// Regenerates quote.json with sockjs 0.3.24's utils.quote (message escaping inside `a[...]` frames):
//   node gen-quote.js <SCREEPS_ROOT, e.g. E:\screeps\server>
const path = require('path');
const fs = require('fs');
const quote = require(path.join(process.argv[2], 'node_modules', 'sockjs', 'lib', 'utils')).quote;
const inputs = ['plain', 'a"b\\c\n\t\u0001', '\u007f\u00e9\u00ff\u0100', '\u200b\u200c\u200f\u2010\u2027\u2028\u202f\u2030',
  '\u205f\u2060\u206f\u2070', '\uffef\ufff0\ufff9\uffff', '\ud83d\ude00', 'ok\u2029end', '["room:W1N1",{"objects":{}}]'];
fs.writeFileSync(path.join(__dirname, 'quote.json'), JSON.stringify(inputs.map(s => [s, quote(s)])) + '\n');
