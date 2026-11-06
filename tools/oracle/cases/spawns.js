// Room-processor oracle cases for spawns and the energy they spend (WP10: spawns/*, _calc_spawns,
// extensions). Regenerate: node22 tools/oracle/processor.js gen tools/oracle/cases/spawns.js
'use strict';
const { scenario } = require('../lib/scenario');

function base(name, tags, { level = 2, ...opts } = {}) {
  const s = scenario(name, Object.assign({ tags }, opts));
  s.ctrl = s.controller(25, 25, { user: 'u1', level, progress: 100 });
  s.src = s.source(10, 10);
  return s;
}

const cases = [];
const add = (s) => cases.push(s.build());

{
  const s = base('createCreep: from the spawn alone', ['spawns/create-creep.js', 'spawns/_charge-energy.js']);
  const sp = s.spawn(20, 30, { energy: 300 });
  s.intent('u1', sp, 'createCreep', { name: 'w1', body: ['work', 'carry', 'move', 'move'] });
  add(s);
}
{
  const s = base('createCreep: energyStructures order drains extensions first', ['spawns/_charge-energy.js:20']);
  const sp = s.spawn(20, 30, { energy: 100 });
  const e1 = s.extension(22, 30, { energy: 50 });
  const e2 = s.extension(23, 30, { energy: 50 });
  s.intent('u1', sp, 'createCreep', { name: 'w2', body: ['work', 'carry', 'move'], energyStructures: [e2, e1, sp] });
  add(s);
}
{
  const s = base('createCreep: not enough energy', ['spawns/_charge-energy.js']);
  const sp = s.spawn(20, 30, { energy: 120 });
  s.extension(22, 30, { energy: 10 });
  s.intent('u1', sp, 'createCreep', { name: 'w3', body: ['work', 'work', 'move'] });
  add(s);
}
{
  const s = base('createCreep: directions and a busy spawn', ['spawns/create-creep.js:22'], { level: 7 });
  const sp = s.spawn(20, 30, { energy: 300 });
  const busy = s.spawn(30, 30, { energy: 300, spawning: { name: 'old', needTime: 9, spawnTime: 1005 } });
  s.intent('u1', sp, 'createCreep', { name: 'w4', body: ['move'], directions: [2, 2, 6] });
  s.intent('u1', busy, 'createCreep', { name: 'w5', body: ['move'] });
  add(s);
}
{
  const s = base('spawn tick: the creep is born to the first free direction', ['spawns/tick.js:18', 'spawns/_born-creep.js']);
  const sp = s.spawn(20, 30, { energy: 300, spawning: { name: 'baby', needTime: 9, spawnTime: 1001 } });
  s.creep(20, 30, { name: 'baby', body: 'work,carry,move', spawning: true, store: { energy: 0 }, ageTime: undefined });
  s.wall(20, 29);
  add(s);
  void sp;
}
{
  const s = base('spawn tick: born with directions, spawn regenerates energy', ['spawns/_born-creep.js', 'spawns/tick.js:44']);
  s.spawn(20, 30, { energy: 100, spawning: { name: 'kid', needTime: 3, spawnTime: 1000, directions: [5, 1] } });
  s.creep(20, 30, { name: 'kid', body: 'move', spawning: true, store: { energy: 0 }, ageTime: undefined });
  add(s);
}
{
  const s = base('_calc_spawns: a third spawn above RCL limits is turned off', ['_calc_spawns.js'], { level: 1 });
  s.spawn(20, 30, { energy: 300 });
  s.spawn(26, 26, { energy: 300 });
  s.extension(30, 30, { energy: 50 });
  add(s);
}

module.exports = cases;
