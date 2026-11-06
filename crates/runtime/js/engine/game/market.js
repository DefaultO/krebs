// mirror: reference/engine/src/game/market.js
// parity: DIVERGENT(J3)
// Statement-by-statement port (sloppy mode, like the original). J3: the world order book and price history are
// not copied into runtimeData; the three vanilla reads of `runtimeData.market.orders[rt]`, `.orders.all[id]` and
// `.history[rt]` go through the runtime driver's natives (`marketOrders`/`marketOrder`/`marketHistory`,
// ARCHITECTURE.md 6.7), which return the same documents as JSON text. The order-book and history caches parse that
// text once (6.7) instead of vanilla's JSON.stringify + JSON.parse round trip of the copied map: same values, but a
// bot-patched JSON.stringify is not called there. Every other statement is vanilla; the unused `require('util')` of
// the original (webpack's util polyfill) is dropped. `// vanilla: market.js:N` anchors use the vanilla line numbers.
var _ = require('lodash'),
    utils = require('../utils'),
    driver = utils.getRuntimeDriver(),
    C = driver.constants;

// Captured at load: parsing host data must not go through bot-patchable globals (J5).
var jsonParse = JSON.parse, objectKeys = Object.keys;

// J3: the JSON text of vanilla `runtimeData.market.<map>[v]` (orders: marketOrders, data.js getCachedMarketOrders
// {id: order}, prices x1000; history: marketHistory, data.js getCachedMarketHistory [stats doc]) or undefined. The key
// is converted exactly once, like the member access (`['all']` reads 'all'), so the natives only see strings.
function _nativeJson(native, v) {
    var key = objectKeys({[v]: 0})[0];
    return key === undefined ? undefined : driver[native](key);
}

// J3: the value of vanilla `runtimeData.market.orders.all[v]` (marketOrder). A key the native does not know
// resolves through a plain object, as on the parsed vanilla map (`deal('constructor', ...)` reads Object).
function _nativeOrder(v) {
    var key = objectKeys({[v]: 0})[0];
    var json = key === undefined ? undefined : driver.marketOrder(key);
    return json === undefined ? ({})[key] : jsonParse(json);
}

// vanilla: market.js:8
exports.make = function(runtimeData, intents, register) {

    var ordersCreatedDuringTick = 0, cachedOrders = {}, cachedHistory = {},
        _incomingTransactions, _outgoingTransactions, _orders;

    // vanilla: market.js:13
    function _getOrders(resourceType) {
        if(!resourceType) {
            resourceType = 'all';
        }
        if(!cachedOrders[resourceType]) {
            if(resourceType != 'all' && !_.contains(C.RESOURCES_ALL, resourceType) && !_.contains(C.INTERSHARD_RESOURCES, resourceType)) {
                return {};
            }
            // J3 (ARCHITECTURE.md 6.7): vanilla `JSON.parse(JSON.stringify(runtimeData.market.orders[resourceType]) || '{}')`;
            // the native returns that JSON text, so only the outer parse remains (an inherited Object.prototype key
            // never gets here: the cache check above returns it).
            cachedOrders[resourceType] = JSON.parse(_nativeJson('marketOrders', resourceType) || '{}');
            for(var i in cachedOrders[resourceType]) {
                cachedOrders[resourceType][i].price /= 1000;
            }
        }
        return cachedOrders[resourceType];
    }

    var market = {

        // vanilla: market.js:31
        calcTransactionCost: register.wrapFn(function(amount, roomName1, roomName2) {
            var distance = utils.calcRoomsDistance(roomName1, roomName2, true);
            return utils.calcTerminalEnergyCost(amount, distance);
        }),

        // vanilla: market.js:36
        getAllOrders: register.wrapFn(function(filter) {
            var orders = _getOrders(filter && filter.resourceType);
            return _.filter(orders, filter);
        }),

        // vanilla: market.js:41
        getHistory: register.wrapFn(function(resourceType) {
            if(!resourceType) {
                resourceType = 'all';
            }

            if(!cachedHistory[resourceType]) {
                if(resourceType != 'all' && !_.contains(C.RESOURCES_ALL, resourceType) && !_.contains(C.INTERSHARD_RESOURCES, resourceType)) {
                    return {};
                }
                // J3: vanilla `JSON.parse(JSON.stringify(runtimeData.market.history[resourceType] || {}))`, as for orders
                cachedHistory[resourceType] = JSON.parse(_nativeJson('marketHistory', resourceType) || '{}');
            }
            return cachedHistory[resourceType];
        }),

        // vanilla: market.js:55
        getOrderById: register.wrapFn(function(id) {
            if(this.orders[id]) {
                return JSON.parse(JSON.stringify(this.orders[id]));
            }
            const order = _nativeOrder(id);
            if(order) {
                const result = JSON.parse(JSON.stringify(order));
                result.price /= 1000;
                return result;
            }
            return null;
        }),

        // vanilla: market.js:68
        createOrder: register.wrapFn(function(type, resourceType, price, totalAmount, roomName) {
            if(_.isObject(type)) {
                var {type, resourceType, price, totalAmount, roomName} = type;
            }
            if(!_.contains(C.RESOURCES_ALL, resourceType) && !_.contains(C.INTERSHARD_RESOURCES, resourceType)) {
                return C.ERR_INVALID_ARGS;
            }
            if(type != C.ORDER_BUY && type != C.ORDER_SELL) {
                return C.ERR_INVALID_ARGS;
            }
            price = parseFloat(price);
            totalAmount = parseInt(totalAmount);
            if(!price || price <= 0 || !totalAmount) {
                return C.ERR_INVALID_ARGS;
            }
            if(price * totalAmount * C.MARKET_FEE > this.credits) {
                return C.ERR_NOT_ENOUGH_RESOURCES;
            }
            if(!_.contains(C.INTERSHARD_RESOURCES, resourceType) &&
                (!roomName || !_.any(runtimeData.userObjects, {type: 'terminal', room: roomName}))) {
                return C.ERR_NOT_OWNER;
            }
            if(_.size(this.orders) + ordersCreatedDuringTick >= C.MARKET_MAX_ORDERS) {
                return C.ERR_FULL;
            }
            ordersCreatedDuringTick++;
            intents.pushByName('global', 'createOrder', {
                type, resourceType, price, totalAmount, roomName
            });
            return C.OK;
        }),

        // vanilla: market.js:100
        cancelOrder: register.wrapFn(function(orderId) {
            if(!this.orders[orderId]) {
                return C.ERR_INVALID_ARGS;
            }
            intents.pushByName('global', 'cancelOrder', {orderId}, 50);
            return C.OK;
        }),

        // vanilla: market.js:108
        deal: register.wrapFn(function(orderId, amount, targetRoomName) {
            var order = _nativeOrder(orderId);
            if(!order) {
                return C.ERR_INVALID_ARGS;
            }
            amount = parseInt(amount);
            if(!amount || amount < 0) {
                return C.ERR_INVALID_ARGS;
            }
            if(_.contains(C.INTERSHARD_RESOURCES, order.resourceType)) {
                if(order.type == C.ORDER_BUY && (runtimeData.user.resources[order.resourceType]||0) < amount) {
                    return C.ERR_NOT_ENOUGH_RESOURCES;
                }
            }
            else {
                if(!targetRoomName) {
                    return C.ERR_INVALID_ARGS;
                }
                var terminal = _.find(runtimeData.userObjects, {type: 'terminal', room: targetRoomName}),
                    transferCost = this.calcTransactionCost(amount, targetRoomName, order.roomName);
                if(!terminal) {
                    return C.ERR_NOT_OWNER;
                }
                if(!terminal.store || terminal.store[C.RESOURCE_ENERGY] < transferCost) {
                    return C.ERR_NOT_ENOUGH_RESOURCES;
                }
                if(terminal.cooldownTime > runtimeData.time) {
                    return C.ERR_TIRED;
                }
                if(order.type == C.ORDER_BUY) {
                    if(order.resourceType != C.RESOURCE_ENERGY && (!terminal.store[order.resourceType] || terminal.store[order.resourceType] < amount) ||
                         order.resourceType == C.RESOURCE_ENERGY && terminal.store[C.RESOURCE_ENERGY] < amount + transferCost) {
                        return C.ERR_NOT_ENOUGH_RESOURCES;
                    }
                }
            }

            if(order.type == C.ORDER_SELL && (runtimeData.user.money || 0) < amount * order.price) {
                return C.ERR_NOT_ENOUGH_RESOURCES;
            }

            if(!intents.pushByName('global', 'deal', {orderId, targetRoomName, amount}, 10)) {
                return C.ERR_FULL;
            }
            return C.OK;
        }),

        // vanilla: market.js:155
        changeOrderPrice: register.wrapFn(function(orderId, newPrice) {
            var order = this.orders[orderId];
            if(!order) {
                return C.ERR_INVALID_ARGS;
            }
            newPrice = parseFloat(newPrice);
            if(!newPrice || newPrice <= 0) {
                return C.ERR_INVALID_ARGS;
            }
            if(newPrice > order.price && (newPrice - order.price) * order.remainingAmount * C.MARKET_FEE > this.credits) {
                return C.ERR_NOT_ENOUGH_RESOURCES;
            }

            intents.pushByName('global', 'changeOrderPrice', {
                orderId, newPrice
            }, 50);
            return C.OK;
        }),

        // vanilla: market.js:174
        extendOrder: register.wrapFn(function(orderId, addAmount) {
            var order = this.orders[orderId];
            if(!order) {
                return C.ERR_INVALID_ARGS;
            }
            addAmount = parseInt(addAmount);
            if(!addAmount || addAmount <= 0) {
                return C.ERR_INVALID_ARGS;
            }
            if(order.price * addAmount * C.MARKET_FEE > this.credits) {
                return C.ERR_NOT_ENOUGH_RESOURCES;
            }

            intents.pushByName('global', 'extendOrder', {
                orderId, addAmount
            }, 50);
            return C.OK;
        }),
    };

    // vanilla: market.js:194
    Object.defineProperties(market, {

        credits: {
            enumerable: true,
            get() {
                return (runtimeData.user.money || 0)/1000;
            }
        },

        incomingTransactions: {
            enumerable: true,
            get() {
                if(!_incomingTransactions) {
                    _incomingTransactions = _.map(runtimeData.transactions.incoming || [], i => {
                        i.transactionId = "" + i._id;
                        delete i._id;
                        i.sender = i.sender ? {username: runtimeData.users[i.sender].username} : undefined;
                        i.recipient = i.recipient ? {username: runtimeData.users[i.recipient].username} : undefined;
                        return i;
                    });
                }
                return _incomingTransactions;
            }
        },

        outgoingTransactions: {
            enumerable: true,
            get() {
                if(!_outgoingTransactions) {
                    _outgoingTransactions = _.map(runtimeData.transactions.outgoing || [], i => {
                        i.transactionId = ""+i._id;
                        delete i._id;
                        i.sender = i.sender ? {username: runtimeData.users[i.sender].username} : undefined;
                        i.recipient = i.recipient ? {username: runtimeData.users[i.recipient].username} : undefined;
                        return i;
                    });
                }
                return _outgoingTransactions;
            }
        },

        orders: {
            enumerable: true,
            get() {
                if(!_orders) {
                    _orders = _(runtimeData.market.myOrders).map(i => {
                        i.id = ""+i._id;
                        delete i._id;
                        delete i.user;
                        i.price /= 1000;
                        return i;
                    }).indexBy('id').value();
                }
                return _orders;
            }
        }
    });

    return market;
};
