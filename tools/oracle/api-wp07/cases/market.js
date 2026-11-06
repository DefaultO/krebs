'use strict';
// Game.market (game/market.js) cases. The vanilla side reads runtimeData.market; the chitin side reads the same
// data through the runtime-driver natives the harness installs (side.js).
const { D } = require('../world');
const { probe, code } = require('./_lib');
const { ROOMS, tick } = require('./_world');

const cases = [];
const add = (name, tags, ticks) => cases.push({ name, tags, rooms: ROOMS, ticks });

const order = (o) => Object.assign({
    created: 900, createdTimestamp: 1599990000000, active: true, type: 'sell', amount: 1000, remainingAmount: 1000,
    totalAmount: 1000, resourceType: 'H', price: 1500, roomName: 'W9N9', user: 'u2',
}, o);
const ORDERS = [
    order({ id: 'o1sellH' }),
    order({ id: 'o2buyH', type: 'buy', price: 800, roomName: 'W3N3' }),
    order({ id: 'o3sellE', resourceType: 'energy', price: 20, roomName: 'W40N40' }),
    order({ id: 'o4buyE', resourceType: 'energy', type: 'buy', price: 30, roomName: 'W2N1' }),
    order({ id: 'o5token', resourceType: 'token', price: 5000000000, roomName: undefined }),
    order({ id: 'o6buyToken', resourceType: 'token', type: 'buy', price: 4000000000, roomName: undefined }),
    order({ id: 'myO1', user: 'u1', roomName: 'W1N1', price: 2000 }),
    order({ id: 'myO2', user: 'u1', type: 'buy', resourceType: 'energy', price: 10, roomName: 'W1N1', remainingAmount: 500 }),
];
const marketData = () => {
    const orders = { all: {} };
    for (const o of ORDERS) {
        orders.all[o.id] = o;
        (orders[o.resourceType] = orders[o.resourceType] || {})[o.id] = o;
    }
    const stat = (resourceType, date, avgPrice) => ({ resourceType, date, transactions: 10, volume: 1000, avgPrice, stddevPrice: 0.1 });
    const history = { all: [stat('H', '2020-09-12', 1.5), stat('energy', '2020-09-12', 0.02)], H: [stat('H', '2020-09-12', 1.5)], energy: [stat('energy', '2020-09-12', 0.02)] };
    const myOrders = ORDERS.filter(o => o.user === 'u1').map(o => Object.assign({ _id: o.id }, o, { id: undefined }));
    return { orders, history, myOrders };
};
const transactions = {
    outgoing: [{ _id: 't1', time: 990, sender: 'u1', recipient: 'u2', resourceType: 'H', amount: 10, from: 'W1N1', to: 'W2N1', description: 'gift' }],
    incoming: [{ _id: 't2', time: 995, sender: 'u2', recipient: 'u1', resourceType: 'energy', amount: 5, from: 'W2N1', to: 'W1N1', order: { id: 'o4buyE', type: 'buy', price: 0.03 } },
        { _id: 't3', time: 996, sender: undefined, recipient: 'u1', resourceType: 'energy', amount: 1, from: 'W2N1', to: 'W1N1' }],
};
const terminals = (o) => o.push(
    D.structure('terminal', { _id: 'termM', x: 40, y: 20, user: 'u1', store: { energy: 2000, H: 300 }, storeCapacity: 300000 }),
    D.structure('terminal', { _id: 'termCoolM', room: 'W5N1', x: 40, y: 20, user: 'u1', store: { energy: 100000, H: 300 }, storeCapacity: 300000, cooldownTime: 1005 }),
);
const opts = (extra) => Object.assign({ market: marketData(), transactions, user: { money: 5000000, resources: { token: 2 } } }, extra);

add('market', ['game/market.js:8', 'game/market.js:13', 'game/market.js:31', 'game/market.js:36', 'game/market.js:41',
    'game/market.js:55', 'game/market.js:194'], [
    tick(code(function() {
        var m = Game.market;
        P('keys', () => Object.keys(m));
        P('credits', () => m.credits);
        P('orders', () => m.orders);
        P('orders same', () => m.orders === m.orders);
        P('incoming', () => m.incomingTransactions);
        P('outgoing', () => m.outgoingTransactions);
        P('all', () => m.getAllOrders());
        P('all same', () => m.getAllOrders() === m.getAllOrders());
        P('by resource', () => m.getAllOrders({resourceType: 'H'}));
        P('by type', () => m.getAllOrders({type: 'buy'}));
        P('by fn', () => m.getAllOrders(o => o.price > 1));
        P('energy', () => m.getAllOrders({resourceType: 'energy', type: 'sell'}));
        P('token', () => m.getAllOrders({resourceType: 'token'}));
        P('none', () => m.getAllOrders({resourceType: 'O'}));
        P('invalid', () => m.getAllOrders({resourceType: 'foo'}));
        P('proto key', () => typeof m.getAllOrders({resourceType: 'toString'}));
        P('history', () => m.getHistory());
        P('history H', () => m.getHistory('H'));
        P('history O', () => m.getHistory('O'));
        P('history foo', () => m.getHistory('foo'));
        P('history same', () => m.getHistory('H') === m.getHistory('H'));
        // J3 natives get the property key vanilla's member access would use (one ToPropertyKey)
        P('history [all]', () => m.getHistory(['all']));
        P('history String all', () => m.getHistory(new String('all')));
        P('history valueOf all toString H', () => m.getHistory({valueOf() { return 'all'; }, toString() { return 'energy'; }}));
        P('history valueOf all toString constructor', () => m.getHistory({valueOf() { return 'all'; }, toString() { return 'constructor'; }}));
        P('orders [all]', () => m.getAllOrders({resourceType: ['all']}));
        P('orders valueOf all toString H', () => { m.getAllOrders({resourceType: {valueOf() { return 'all'; }, toString() { return 'token'; }}}); return m.getAllOrders({resourceType: 'token'}); });
        ['myO1', 'o1sellH', 'o5token', 'nope', 'constructor', '__proto__', 'toString', 5, undefined, null].forEach(id =>
            P('byId ' + String(id), () => { var r = m.getOrderById(id); return typeof r === 'function' ? 'fn' : r; }));
        P('byId String obj', () => m.getOrderById(new String('o1sellH')));
        P('byId toString obj', () => m.getOrderById({toString() { return 'o2buyH'; }}));
        P('byId symbol', () => m.getOrderById(Symbol('x')));
        P('byId [id]', () => m.getOrderById(['o3sellE']));
        P('byId hasOwnProperty', () => typeof m.getOrderById('hasOwnProperty'));
        P('byId mutation isolated', () => { m.getOrderById('o1sellH').price = 1; return m.getOrderById('o1sellH').price; });
        P('cost', () => [m.calcTransactionCost(1000, 'W1N1', 'W5N1'), m.calcTransactionCost(1000, 'W1N1', 'E5S5'), m.calcTransactionCost(0, 'W1N1', 'W1N1')]);
        P('mutate cached', () => { m.getAllOrders()[0].price = 99; return m.getAllOrders({resourceType: 'H'})[0].price; });
    }), opts(), terminals),
    // J3 key conversion on cold caches: the first read of 'all' comes through a non-string key
    tick(code(function() {
        var m = Game.market;
        P('history [all] cold', () => m.getHistory(['all']));
        P('orders String all cold', () => [m.getAllOrders({resourceType: new String('all')}), m.getAllOrders().length]);
        P('history empty all', () => m.getHistory());
    }), opts({ market: Object.assign(marketData(), { history: { all: [] } }) }), terminals),
]);

add('market-intents', ['game/market.js:68', 'game/market.js:100', 'game/market.js:108', 'game/market.js:155', 'game/market.js:174'], [
    tick(probe([
        "Game.market.createOrder('sell', 'H', 1.5, 100, 'W1N1')",
        "Game.market.createOrder({type: 'buy', resourceType: 'energy', price: '0.5', totalAmount: '1000.9', roomName: 'W1N1'})",
        "Game.market.createOrder('sell', 'foo', 1, 100, 'W1N1')",
        "Game.market.createOrder('trade', 'H', 1, 100, 'W1N1')",
        "Game.market.createOrder('sell', 'H', 0, 100, 'W1N1')",
        "Game.market.createOrder('sell', 'H', -1, 100, 'W1N1')",
        "Game.market.createOrder('sell', 'H', 'abc', 100, 'W1N1')",
        "Game.market.createOrder('sell', 'H', 1, 0, 'W1N1')",
        "Game.market.createOrder('sell', 'H', 1, '0.5', 'W1N1')",
        "Game.market.createOrder('sell', 'H', 1e6, 1e6, 'W1N1')",
        "Game.market.createOrder('sell', 'H', 1, 100, 'W2N1')",
        "Game.market.createOrder('sell', 'H', 1, 100)",
        "Game.market.createOrder('sell', 'token', 1, 1)",
        "Game.market.createOrder({type: 'sell'})",
        "Game.market.cancelOrder('myO1')", "Game.market.cancelOrder('o1sellH')", "Game.market.cancelOrder()",
        "Game.market.changeOrderPrice('myO1', 2.5)", "Game.market.changeOrderPrice('myO1', '1')", "Game.market.changeOrderPrice('myO1', 0)",
        "Game.market.changeOrderPrice('myO1', 1e9)", "Game.market.changeOrderPrice('o1sellH', 1)", "Game.market.changeOrderPrice('myO2', 1e5)",
        "Game.market.extendOrder('myO1', 100)", "Game.market.extendOrder('myO1', '100.7')", "Game.market.extendOrder('myO1', 0)",
        "Game.market.extendOrder('myO1', -5)", "Game.market.extendOrder('myO1', 1e9)", "Game.market.extendOrder('nope', 1)",
        "Game.market.deal('o1sellH', 100, 'W1N1')", "Game.market.deal('o1sellH', 100)", "Game.market.deal('o1sellH', 100, 'W2N1')",
        "Game.market.deal('o1sellH', '50.5', 'W1N1')", "Game.market.deal('o1sellH', 0, 'W1N1')", "Game.market.deal('o1sellH', -1, 'W1N1')",
        "Game.market.deal('o1sellH', 'x', 'W1N1')", "Game.market.deal('o1sellH', 100000, 'W1N1')", "Game.market.deal('o1sellH', 100, 'W5N1')",
        "Game.market.deal('o2buyH', 100, 'W1N1')", "Game.market.deal('o2buyH', 301, 'W1N1')", "Game.market.deal('o4buyE', 1900, 'W1N1')",
        "Game.market.deal('o4buyE', 1990, 'W1N1')", "Game.market.deal('o3sellE', 1000, 'W1N1')",
        "Game.market.deal('o5token', 1)", "Game.market.deal('o6buyToken', 2)", "Game.market.deal('o6buyToken', 3)",
        "Game.market.deal('nope', 1, 'W1N1')", "Game.market.deal('constructor', 1, 'W1N1')", "Game.market.deal('myO1', 1, 'W1N1')",
        "Game.market.deal('o1sellH', 1, 'W1N1')", "Game.market.deal('o1sellH', 1, 'W1N1')", "Game.market.deal('o1sellH', 1, 'W1N1')",
        "Game.market.deal('o1sellH', 1, 'W1N1')", "Game.market.deal('o1sellH', 1, 'W1N1')", "Game.market.deal('o1sellH', 1, 'W1N1')",
    ]), opts(), terminals),
    // MARKET_MAX_ORDERS counts own orders plus orders created this tick; no money at all
    tick(probe(["Game.market.createOrder('sell', 'H', 0.001, 1, 'W1N1')", "Game.market.createOrder('sell', 'H', 0.001, 1, 'W1N1')",
        "Game.market.credits", "Object.keys(Game.market.orders).length"]), opts({
        market: Object.assign(marketData(), { myOrders: Array.from({ length: 299 }, (_, i) => order({ _id: 'mo' + i, user: 'u1', roomName: 'W1N1' })) }),
    }), terminals),
    tick(probe(["Game.market.createOrder('sell', 'H', 1, 1, 'W1N1')", "Game.market.deal('o1sellH', 1, 'W1N1')", "Game.market.credits",
        "Game.market.deal('o6buyToken', 1)"]), opts({ user: { money: undefined, resources: {} } }), terminals),
]);

module.exports = cases;
