// Room-processor oracle cases for NPC AI (WP14: creeps/keepers, creeps/invaders, fake-runtime pathfinding
// over the official native pathfinder) and manual intents. Regenerate:
//   node22 tools/oracle/processor.js gen tools/oracle/cases/npc.js
'use strict';
const { scenario } = require('../lib/scenario');

const cases = [];
const add = (s) => cases.push(s.build());
const KEEPER_BODY = 'tough,tough,move,move,attack,attack,ranged_attack,ranged_attack';

{
  const s = scenario('keeper walks to its source (fake-runtime moveTo, path cached on the creep)', {
    room: 'W5N4', tags: ['creeps/keepers/pretick.js:46', 'common/fake-runtime.js:143'],
    terrain: { walls: (x, y) => y === 22 && x > 24 && x < 31 } });
  s.source(30, 20);
  s.object('keeperLair', 10, 20, { nextSpawnTime: null });
  s.creep(25, 24, { user: '3', name: 'Keeper1', body: KEEPER_BODY, notifyWhenAttacked: false });
  add(s);
}
{
  const s = scenario('keeper attacks a creep in range and stays at its source', {
    room: 'W5N4', tags: ['creeps/keepers/pretick.js:60'] });
  const src = s.source(30, 20);
  s.creep(30, 21, { user: '3', name: 'Keeper2', body: KEEPER_BODY, memory_sourceId: src, notifyWhenAttacked: false });
  s.creep(31, 22, { user: 'u1', name: 'victim', body: 'work,carry,move' });
  add(s);
}
{
  const s = scenario('invader flees or attacks a nearby creep', { tags: ['creeps/invaders/pretick.js'] });
  s.controller(25, 25, { user: 'u1', level: 3 });
  s.creep(10, 10, { user: '2', name: 'invader_W1N1_1', body: 'move,move,attack,attack', notifyWhenAttacked: false });
  s.creep(12, 11, { user: 'u1', name: 'defender', body: 'work,carry,move' });
  s.intent('u1', s.objects[2]._id, 'move', { direction: 7 });
  add(s);
}
{
  const s = scenario('manual intents replace a creep\'s runtime intents (INT-MANUAL)', { tags: ['processor.js:231'] });
  s.controller(25, 25, { user: 'u1', level: 2 });
  const src = s.source(10, 10);
  const c = s.creep(11, 10, { body: 'work,carry,move' });
  s.intent('u1', c, 'harvest', { id: src });
  s.manualIntent('u1', c, 'move', { direction: 3 });
  add(s);
}

module.exports = cases;
