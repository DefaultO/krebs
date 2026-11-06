// Global-stage oracle cases (WP13: processor/global.js, global-intents/{market,power}/*). Regenerate:
//   node22 tools/oracle/global.js gen tools/oracle/cases/global.js
'use strict';
const { globalScenario } = require('../lib/scenario');

const cases = [];
const add = (s) => cases.push(s.build());

{
  const s = globalScenario('inter-room move: accessible and out-of-borders destinations', { tags: ['global.js:33'] });
  s.addRoom('W1N1');
  s.addRoom('W2N1');
  s.addRoom('W3N1', { status: 'out of borders' });
  s.creep(0, 25, { room: 'W1N1', interRoom: { room: 'W2N1', x: 49, y: 25 } });
  s.creep(0, 26, { room: 'W2N1', name: 'stuck', interRoom: { room: 'W3N1', x: 49, y: 26 } });
  s.creep(5, 5, { room: 'W1N1', name: 'still' });
  add(s);
}
{
  const s = globalScenario('inter-room move: swap, closed by openTime, opened, unknown room, non-object interRoom', { tags: ['global.js:33'] });
  s.addRoom('W1N1');
  s.addRoom('W2N1');
  s.addRoom('W3N1', { openTime: 9e15 });
  s.addRoom('W4N1', { openTime: 1 });
  s.creep(10, 10, { room: 'W2N1', name: 'early' });
  s.creep(0, 25, { room: 'W2N1', name: 'a', interRoom: { room: 'W1N1', x: 49, y: 25 } });
  s.creep(49, 25, { room: 'W1N1', name: 'b', interRoom: { room: 'W2N1', x: 0, y: 25 } });
  s.creep(49, 30, { room: 'W2N1', name: 'closed', interRoom: { room: 'W3N1', x: 0, y: 30 } });
  s.creep(49, 31, { room: 'W3N1', name: 'opened', interRoom: { room: 'W4N1', x: 0, y: 31 } });
  s.creep(25, 0, { room: 'W1N1', name: 'nowhere', interRoom: { room: 'W9N9', x: 25, y: 49 } });
  s.creep(25, 1, { room: 'W1N1', name: 'string', interRoom: 'W2N1' });
  add(s);
}
{
  const s = globalScenario('inter-room move: power creep and two creeps into one room keep creation order', { tags: ['global.js:33'] });
  s.addRoom('W1N1');
  s.addRoom('W1N2');
  s.creep(5, 5, { room: 'W1N2', name: 'resident' });
  s.creep(20, 0, { room: 'W1N1', name: 'x1', interRoom: { room: 'W1N2', x: 20, y: 49 } });
  s.object('powerCreep', 21, 0, { room: 'W1N1', user: 'u1', name: 'Op', interRoom: { room: 'W1N2', x: 21, y: 49 } });
  s.creep(30, 30, { room: 'W1N2', name: 'late' });
  s.creep(22, 0, { room: 'W1N1', name: 'x2', interRoom: { room: 'W1N2', x: 22, y: 49 } });
  add(s);
}
{
  const s = globalScenario('terminal send: energy with distance fee, description escaped', { tags: ['global-intents/market.js:69'] });
  s.addRoom('W1N1');
  s.addRoom('W5N1');
  s.terminal(20, 20, { room: 'W1N1', store: { energy: 10000, H: 500 }, send: { targetRoomName: 'W5N1', resourceType: 'H', amount: 300, description: '<b>hi' } });
  s.terminal(20, 20, { room: 'W5N1', user: 'u2', store: { energy: 0 } });
  add(s);
}
{
  const s = globalScenario('createOrder: fee charged, order and money history inserted', { tags: ['global-intents/market.js:105'] });
  s.addRoom('W1N1');
  s.user('u1', { money: 1000000 });
  s.terminal(20, 20, { room: 'W1N1', store: { energy: 1000 } });
  s.globalIntents('u1', { createOrder: [{ type: 'sell', resourceType: 'energy', price: 0.5, totalAmount: 1000, roomName: 'W1N1' }] });
  add(s);
}
{
  const s = globalScenario('deal: buying from a sell order through terminals', { tags: ['global-intents/market.js:289'] });
  s.addRoom('W1N1');
  s.addRoom('W3N3');
  s.user('u1', { money: 0 });
  s.user('u2', { money: 5000000 });
  s.terminal(20, 20, { room: 'W1N1', store: { energy: 5000, H: 2000 } });
  s.terminal(20, 20, { room: 'W3N3', user: 'u2', store: { energy: 5000 } });
  const order = s.addOrder({ user: 'u1', type: 'sell', resourceType: 'H', price: 2000, totalAmount: 1500, roomName: 'W1N1' });
  s.globalIntents('u2', { deal: [{ orderId: order, amount: 700, targetRoomName: 'W3N3' }] });
  add(s);
}
{
  const s = globalScenario('cancelOrder, changeOrderPrice and an empty order removed', { tags: ['global-intents/market.js:251', 'global-intents/market.js:497'] });
  s.addRoom('W1N1');
  s.user('u1', { money: 2000000 });
  s.terminal(20, 20, { room: 'W1N1', store: { energy: 5000 } });
  const a = s.addOrder({ user: 'u1', resourceType: 'energy', price: 100, totalAmount: 1000 });
  const b = s.addOrder({ user: 'u1', resourceType: 'energy', price: 100, totalAmount: 1000 });
  s.addOrder({ user: 'u1', resourceType: 'energy', price: 100, totalAmount: 0, remainingAmount: 0, amount: 0 });
  s.globalIntents('u1', { cancelOrder: [{ orderId: a }], changeOrderPrice: [{ orderId: b, newPrice: 0.3 }] });
  add(s);
}
{
  const s = globalScenario('createPowerCreep: power level allows one creep', { tags: ['global-intents/power/createPowerCreep.js'] });
  s.user('u1', { power: 1000 });
  s.user('u2', { power: 0 });
  s.globalIntents('u1', { createPowerCreep: [{ name: 'Op', className: 'operator' }, { name: 'Op2', className: 'operator' }] });
  s.globalIntents('u2', { createPowerCreep: [{ name: 'Nope', className: 'operator' }] });
  add(s);
}
{
  const s = globalScenario('spawnPowerCreep at a power spawn', { tags: ['global-intents/power/spawnPowerCreep.js'] });
  s.addRoom('W1N1');
  s.user('u1', { power: 4000 });
  const ps = s.object('powerSpawn', 22, 22, { user: 'u1', store: { energy: 0, power: 0 }, hits: 5000, hitsMax: 5000 });
  s.powerCreep('u1', 'Op', { level: 1, powers: { 1: { level: 1 } } });
  s.globalIntents('u1', { spawnPowerCreep: [{ id: ps, name: 'Op' }] });
  add(s);
}

module.exports = cases;
