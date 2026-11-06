// Cron oracle cases (WP15: engine/src/cron/**, backend/lib/cronjobs.js + strongholds.js). Regenerate:
//   node22 tools/oracle/cron.js gen tools/oracle/cases/cron.js
'use strict';
const { cronScenario } = require('../lib/scenario');

const cases = [];
const add = (s) => cases.push(s.build());
const G = 50000;

{
  const s = cronScenario('roomsForceUpdate: due, not due, active and out-of-borders rooms', 'roomsForceUpdate', {
    time: G, tags: ['backend/lib/cronjobs.js:178'], env: { activeRooms: ['W3N1'] } });
  s.addRoom('W1N1');
  s.addRoom('W2N1', { nextForceUpdateTime: G + 10 });
  s.addRoom('W3N1');
  s.addRoom('W4N1', { status: 'out of borders' });
  s.addRoom('W5N1', { nextForceUpdateTime: G });
  add(s);
}
{
  const s = cronScenario('genPowerBanks: first visit sets the timer, a due room gets a bank', 'genPowerBanks', {
    time: G, tags: ['backend/lib/cronjobs.js:194'] });
  s.addRoom('W10N1', { bus: true });
  s.addRoom('W10N2', { bus: true, powerBankTime: G - 1, terrain: { walls: (x, y) => x > 20 && x < 30 && y > 20 && y < 30 } });
  s.addRoom('W10N3', { bus: true, powerBankTime: G + 100 });
  s.addRoom('W11N1');
  add(s);
}
{
  const s = cronScenario('genInvaders: a harvested room next to a core gets a raid', 'genInvaders', {
    time: G, tags: ['backend/lib/cronjobs.js:376'] });
  s.addRoom('W2N2', { invaderGoal: 1000 });
  s.addRoom('W2N1');
  s.addRoom('W3N2');
  s.addRoom('W5N5');
  s.source(10, 10, { room: 'W2N2', invaderHarvested: 800 });
  s.source(30, 30, { room: 'W2N2', invaderHarvested: 300 });
  s.controller(25, 25, { room: 'W2N2', user: 'u1', level: 5 });
  s.controller(25, 25, { room: 'W2N1', user: 'u2', level: 3 });
  s.object('invaderCore', 20, 20, { room: 'W5N5', user: '2', level: 2, hits: 100000, hitsMax: 100000 });
  add(s);
}
{
  const s = cronScenario('purgeTransactions keeps the newest 100 per sender and recipient', 'purgeTransactions', {
    time: G, tags: ['backend/lib/cronjobs.js:445'] });
  for (let i = 0; i < 105; i++) {
    s.db.transactions.push({ _id: `tx${i}`, time: 1000 + (i * 37) % 105, sender: i % 3 ? 'u1' : 'u2', recipient: 'u3', resourceType: 'energy', amount: 1, from: 'W1N1', to: 'W2N2' });
  }
  add(s);
}
{
  const s = cronScenario('recreateNpcOrders: an NPC terminal gets fresh orders', 'recreateNpcOrders', {
    time: G, tags: ['backend/lib/cronjobs.js:39'] });
  s.addRoom('W5N5');
  s.addRoom('W15N5', { nextNpcMarketOrder: G + 1000 });
  s.terminal(25, 25, { room: 'W5N5', user: null, store: {} });
  s.terminal(25, 25, { room: 'W15N5', user: null, store: {} });
  s.addOrder({ user: null, roomName: 'W5N5', resourceType: 'H', price: 3000, totalAmount: 1000 });
  add(s);
}
{
  const s = cronScenario('calcMarketStats over the last 14 days of sells', 'calcMarketStats', {
    time: G, tags: ['backend/lib/cronjobs.js:472'] });
  const day = 24 * 3600 * 1000;
  const now = s.clock.baseMs + G * 1000;
  for (const [i, ago] of [0, 1, 1, 3, 20].entries()) {
    s.db['users.money'].push({ _id: `m${i}`, date: new Date(now - ago * day).toISOString(), tick: G - ago, user: 'u1', type: 'market.sell',
      balance: 10, change: 2 + i, market: { resourceType: i % 2 ? 'H' : 'energy', price: 0.5 + i / 10, amount: 4 + i } });
  }
  s.db['users.money'].push({ _id: 'buy', date: new Date(now).toISOString(), tick: G, user: 'u2', type: 'market.buy', balance: 1, change: -1, market: { resourceType: 'H', price: 1, amount: 1 } });
  add(s);
}
{
  const s = cronScenario('deletePowerCreeps refunds the power level', 'deletePowerCreeps', {
    time: G, tags: ['backend/lib/cronjobs.js:533'] });
  s.user('u1', { power: 5000 });
  s.powerCreep('u1', 'Old', { level: 2, deleteTime: s.clock.baseMs + G * 1000 - 1 });
  s.powerCreep('u1', 'Young', { level: 1, deleteTime: s.clock.baseMs + G * 1000 + 1000 });
  add(s);
}
{
  const s = cronScenario('genDeposits: a W5N5 sector gets a deposit on a free bus room', 'genDeposits', {
    time: G, tags: ['backend/lib/cronjobs.js:574', 'BUG-DEPOSIT-REGEX'] });
  s.addRoom('W5N5');
  s.addRoom('W0N3', { bus: true, depositType: 'mist', terrain: { walls: (x, y) => x > 10 && x < 40 && y > 10 && y < 40 && (x + y) % 3 === 0 } });
  s.addRoom('W4N0', { bus: true, depositType: 'metal', terrain: { walls: (x, y) => x > 10 && x < 40 && y > 10 && y < 40 && (x * y) % 4 === 0 } });
  add(s);
}

module.exports = cases;
