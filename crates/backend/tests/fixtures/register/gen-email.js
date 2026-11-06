// Regenerates email.json: register.js / user.js e-mail regex on edge inputs.   node gen-email.js
const fs = require('fs');
const path = require('path');
const re = /^[\w\d-\.\+&]+\@[\w\d\-\.&]+\.[\w\d\-\.&]{2,}$/;
const inputs = ['a@b.cd', 'a@b.c', 'a@.cd', 'a@bcd', '@b.cd', 'a.b+c&d-e_f@x-y.z.ab', 'a@b..cd', 'a@b.c.d', 'a b@c.de',
  'a@b@c.de', 'é@b.cd', 'a@b.cé', 'A@B.CD', 'a@b.cd ', 'a@-.--', 'a@b.cd\n', 'a+@b&.c&', 'a@b.c_'];
fs.writeFileSync(path.join(__dirname, 'email.json'), JSON.stringify(inputs.map(i => [i, re.test(i)])) + '\n');
