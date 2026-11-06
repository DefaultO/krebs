// Room-processor oracle cases for the M1 creep intents (WP09). Regenerate the fixture with
//   E:\screeps\node22\node-v22.22.3-win-x64\node.exe tools/oracle/processor.js gen tools/oracle/cases/creeps.js
// Tags name the vanilla lines a case pins (reference/engine/src/processor/intents/…).
'use strict';
const { scenario } = require('../lib/scenario');

// An owned RCL2 room: controller, two sources, one spawn.
function base(name, tags, opts) {
  const s = scenario(name, Object.assign({ tags }, opts));
  s.user('u1');
  s.ctrl = s.controller(25, 25, { user: 'u1', level: 2, progress: 1000 });
  s.src = s.source(10, 10);
  s.src2 = s.source(40, 40, { energy: 5 });
  s.spawn1 = s.spawn(20, 30);
  return s;
}

const cases = [];
const add = (s) => cases.push(s.build());

{
  const s = base('harvest: 2 work parts from a full source', ['creeps/harvest.js:43', 'sources/tick.js']);
  const c = s.creep(11, 10, { body: 'work,work,carry,move' });
  s.intent('u1', c, 'harvest', { id: s.src });
  add(s);
}
{
  const s = base('harvest: source with less energy than the work power', ['creeps/harvest.js:43']);
  const c = s.creep(41, 40, { body: 'work,work,work,carry,move', store: { energy: 10 } });
  s.intent('u1', c, 'harvest', { id: s.src2 });
  add(s);
}
{
  const s = base('harvest: full creep drops the overflow on its tile', ['creeps/harvest.js:52', '_create-energy.js']);
  const c = s.creep(11, 11, { body: 'work,work,carry,move', store: { energy: 49 } });
  s.intent('u1', c, 'harvest', { id: s.src });
  add(s);
}
{
  const s = base('harvest: out of range is ignored', ['creeps/harvest.js:20']);
  const c = s.creep(13, 10, { body: 'work,carry,move' });
  s.intent('u1', c, 'harvest', { id: s.src });
  add(s);
}
{
  const s = base('move: plain step and fatigue', ['creeps/move.js', 'movement.js', 'creeps/tick.js']);
  const c = s.creep(15, 15, { body: 'work,carry,move' });
  s.intent('u1', c, 'move', { direction: 3 });
  add(s);
}
{
  const s = base('move: two creeps into the same tile', ['movement.js:check']);
  const a = s.creep(15, 15, { body: 'move,move' });
  const b = s.creep(17, 15, { body: 'work,move', name: 'second' });
  s.intent('u1', a, 'move', { direction: 3 });
  s.intent('u1', b, 'move', { direction: 7 });
  add(s);
}
{
  const s = base('move: swap places', ['movement.js:check']);
  const a = s.creep(15, 15, { body: 'move' });
  const b = s.creep(16, 15, { body: 'move', name: 'b' });
  s.intent('u1', a, 'move', { direction: 3 });
  s.intent('u1', b, 'move', { direction: 7 });
  add(s);
}
{
  const s = base('move: onto a swamp and a road', ['movement.js:execute', 'creeps/_add_fatigue.js'], {
    terrain: { swamps: [[16, 16]] },
  });
  const a = s.creep(15, 15, { body: 'work,work,move' });
  s.road(14, 14);
  const b = s.creep(15, 13, { body: 'work,work,move', name: 'onroad' });
  s.intent('u1', a, 'move', { direction: 4 });
  s.intent('u1', b, 'move', { direction: 6 });
  add(s);
}
{
  const s = base('transfer: energy to the spawn', ['creeps/transfer.js']);
  s.objects.find((o) => o._id === s.spawn1).store.energy = 200;
  const c = s.creep(21, 30, { store: { energy: 50 } });
  s.intent('u1', c, 'transfer', { id: s.spawn1, resourceType: 'energy', amount: 50 });
  add(s);
}
{
  const s = base('transfer: amount clamped to the target free capacity', ['creeps/transfer.js:39']);
  const ext = s.extension(22, 31, { energy: 40 });
  const c = s.creep(21, 31, { body: 'carry,carry,move', store: { energy: 100 } });
  s.intent('u1', c, 'transfer', { id: ext, resourceType: 'energy', amount: 100 });
  add(s);
}
{
  const s = base('withdraw: from a container, clamped to the free capacity', ['creeps/withdraw.js:13']);
  const cont = s.container(12, 12, { store: { energy: 500 } });
  const c = s.creep(12, 13, { body: 'carry,carry,move', store: { energy: 30 } });
  s.intent('u1', c, 'withdraw', { id: cont, resourceType: 'energy', amount: 100 });
  add(s);
}
{
  const s = base('withdraw: no amount sanitizes to null, Math.min(null, space) withdraws 0', ['creeps/withdraw.js:13']);
  const cont = s.container(12, 12, { store: { energy: 500 } });
  const c = s.creep(12, 13, { body: 'carry,carry,move', store: {} });
  s.intent('u1', c, 'withdraw', { id: cont, resourceType: 'energy' });
  add(s);
}
{
  const s = base('withdraw: a target without a store crashes the room (BUG-CRASH-WITHDRAW, private)', ['creeps/withdraw.js:43']);
  const c = s.creep(11, 11, { body: 'carry,move', store: {} });
  s.intent('u1', c, 'withdraw', { id: s.src, resourceType: 'energy', amount: 10 });
  add(s);
}
{
  const s = base('pickup: a drop larger than the free capacity', ['creeps/pickup.js', 'energy/tick.js']);
  const d = s.drop(30, 30, 120);
  const c = s.creep(30, 31, { body: 'carry,move', store: { energy: 10 } });
  s.intent('u1', c, 'pickup', { id: d });
  add(s);
}
{
  const s = base('drop: onto an existing pile, then decay', ['creeps/drop.js', '_create-energy.js', 'energy/tick.js']);
  s.drop(30, 30, 1000);
  const c = s.creep(30, 30, { body: 'carry,move', store: { energy: 50 } });
  s.intent('u1', c, 'drop', { resourceType: 'energy', amount: 30 });
  add(s);
}
{
  const s = base('build: progress on an extension site', ['creeps/build.js']);
  const site = s.site(26, 28, 'extension', { progress: 100 });
  const c = s.creep(26, 29, { body: 'work,work,carry,move', store: { energy: 50 } });
  s.intent('u1', c, 'build', { id: site });
  add(s);
}
{
  const s = base('build: completes a road', ['creeps/build.js:105']);
  const site = s.site(26, 28, 'road', { progress: 299 });
  const c = s.creep(26, 29, { body: 'work,carry,move', store: { energy: 50 } });
  s.intent('u1', c, 'build', { id: site });
  add(s);
}
{
  const s = base('repair: a damaged road', ['creeps/repair.js']);
  const r = s.road(18, 18, { hits: 1000 });
  const c = s.creep(18, 19, { body: 'work,carry,move', store: { energy: 50 } });
  s.intent('u1', c, 'repair', { id: r });
  add(s);
}
{
  const s = base('upgradeController: progress and gcl', ['creeps/upgradeController.js']);
  const c = s.creep(24, 24, { body: 'work,work,carry,move', store: { energy: 50 } });
  s.intent('u1', c, 'upgradeController', { id: s.ctrl });
  add(s);
}
{
  const s = base('upgradeController: level up to 3', ['creeps/upgradeController.js:60']);
  s.objects.find((o) => o._id === s.ctrl).progress = 44999;
  const c = s.creep(24, 24, { body: 'work,carry,move', store: { energy: 50 } });
  s.intent('u1', c, 'upgradeController', { id: s.ctrl });
  add(s);
}
{
  const s = base('say and suicide', ['creeps/say.js', 'creeps/suicide.js', 'creeps/_die.js']);
  const a = s.creep(5, 5, { body: 'move', store: {} });
  const b = s.creep(7, 7, { body: 'work,carry,move', store: { energy: 30 }, name: 'dying' });
  s.intent('u1', a, 'say', { message: 'hello', isPublic: true });
  s.intent('u1', b, 'suicide', {});
  add(s);
}
{
  const s = base('tick: creep dies of old age', ['creeps/tick.js', 'creeps/_die.js']);
  s.creep(5, 5, { body: 'work,carry,move', store: { energy: 20 }, ageTime: 1000 });
  add(s);
}
{
  // The 1-unit pile (temp id 1) decays first in the tick loop and cancels its insert; the tombstone then
  // gets counter 1 again. Persisted docs agree, but chitin never reuses temp ids: fRows must flag it.
  const s = base('temp id reuse: a fresh 1-unit pile decays, then a tombstone (BUG-INSERT-COUNTER)', ['bulk.js:78', 'energy/tick.js:12']);
  const c = s.creep(30, 30, { body: 'carry,move', store: { energy: 5 } });
  s.intent('u1', c, 'drop', { resourceType: 'energy', amount: 1 });
  s.creep(5, 5, { body: 'work,carry,move', store: { energy: 20 }, ageTime: 1000, name: 'old' });
  add(s);
}
{
  // Neutral room: whoever ORD-POLICY puts first empties the 5-energy source.
  const s = scenario('harvest: two users race for the last energy (ORD-POLICY)', { tags: ['processor.js:229', 'creeps/harvest.js:43'] });
  const src = s.source(10, 10, { energy: 5 });
  const a = s.creep(11, 10, { user: 'u1', body: 'work,work,work,carry,move' });
  const b = s.creep(9, 10, { user: 'u2', body: 'work,work,work,carry,move', name: 'rival' });
  s.intent('u2', b, 'harvest', { id: src });
  s.intent('u1', a, 'harvest', { id: src });
  add(s);
}
{
  const s = scenario('harvest: the same race in an explicit reversed order', { tags: ['processor.js:229'], order: null });
  const src = s.source(10, 10, { energy: 5 });
  const a = s.creep(11, 10, { user: 'u1', body: 'work,work,work,carry,move' });
  const b = s.creep(9, 10, { user: 'u2', body: 'work,work,work,carry,move', name: 'rival' });
  s.intent('u2', b, 'harvest', { id: src });
  s.intent('u1', a, 'harvest', { id: src });
  const policy = s.build().scenario.intents.map(([u]) => u);
  s.order = policy.reverse();
  add(s);
}

module.exports = cases;
