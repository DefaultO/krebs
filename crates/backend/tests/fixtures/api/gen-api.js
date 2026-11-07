// API oracle: runs the official backend routes (express + q-json-response + passport token auth) over the
// official storage db.js on an in-memory Loki DB holding `world`, replays `steps` and records every response
// and, after each write, the DB. The Rust side (server.rs `oracle` test) loads the same world and compares.
//   node gen-api.js <SCREEPS_ROOT, e.g. E:\screeps\server>      (node 22)
// Covers (the official route modules it mounts): api/auth.js, api/user.js, api/user-messages.js, api/register.js,
// api/game.js, api/market.js, api/power-creeps.js, api/leaderboard.js.
'use strict';
const path = require('path');
const fs = require('fs');
const http = require('http');
const ROOT = process.argv[2];
const nm = p => path.join(ROOT, 'node_modules', p);

// Fixed clocks: Date.now / new Date() = vclock(1000) of the Rust world (base 1.6e12 + 1000 ticks × 1000 ms).
const FIXED = 1600000000000 + 1000 * 1000;
const RealDate = Date;
global.Date = class extends RealDate {
    constructor(...a) { if (a.length) super(...a); else super(FIXED); }
    static now() { return FIXED; }
};
let rnd = 0;
Math.random = () => { rnd = (rnd + 0.37) % 1; return rnd; };

const common = require(nm('@screeps/common'));
const config = common.configManager.config;
const EventEmitter = require('events').EventEmitter;
config.storage = {};
config.backend = Object.assign(new EventEmitter(), {customIntentTypes: {}, socketUpdateThrottle: 200});
const loki = require(nm('lokijs'));
const ldb = new loki('memory');
ldb.loadDatabase = (opts, cb) => cb();

const A = 'a11ce0000000001', B = 'b0b000000000002', CAROL = 'ca4010000000003', DAVE = 'da7e00000000004';
const terrain = (walls) => {
    let t = '';
    for (let y = 0; y < 50; y++) for (let x = 0; x < 50; x++) {
        const border = x == 0 || y == 0 || x == 49 || y == 49;
        t += walls[`${x},${y}`] !== undefined ? walls[`${x},${y}`] : (border && !(y >= 20 && y <= 30) ? 1 : 0);
    }
    return t;
};
const world = {
    users: [
        {_id: '2', username: 'Invader', usernameLower: 'invader', cpu: 0, active: 0},
        {_id: '3', username: 'Source Keeper', usernameLower: 'source keeper', cpu: 0, active: 0},
        {_id: A, username: 'alice', usernameLower: 'alice', email: 'alice@x.io', cpu: 100, active: 10000, gcl: 1000, money: 12345, resources: {token: 2},
            badge: {type: 1, color1: '#ff0000', color2: '#00ff00', color3: '#0000ff', param: 0, flip: false}, steam: {id: '7656', displayName: 'A', ownership: [1]}, customBadge: {path1: 'M0', path2: 'M1'}, power: 50000, powerExperimentations: 2},
        {_id: B, username: 'bob', usernameLower: 'bob', cpu: 100, bot: 'simplebot', notifyPrefs: {sendOnline: true}},
        {_id: CAROL, steam: {id: '99'}, cpu: 100, cpuAvailable: 0, gcl: 0, powerExperimentations: 30},
        {_id: DAVE, username: 'dave', usernameLower: 'dave', cpu: 100},
        {_id: 'sys000000000005', username: 'Screeps', usernameLower: 'screeps', cpu: 0, active: 0},
    ],
    'users.code': [
        {_id: 'code-a1', user: A, branch: 'default', modules: {main: 'loop', 'lib$DOT$util': 'u'}, activeWorld: true, activeSim: true, timestamp: 5},
        {_id: 'code-a2', user: A, branch: 'dev', modules: {main: 'dev'}, timestamp: 6},
        {_id: 'code-b1', user: B, branch: 'default', modules: {main: ''}, activeWorld: true, activeSim: true},
        {_id: 'code-d1', user: DAVE, branch: 'default', modules: {main: ''}, activeWorld: true, activeSim: true},
    ],
    'users.console': [],
    'users.messages': [],
    'users.power_creeps': [
        {_id: 'pc1', name: 'Op', className: 'operator', user: A, level: 2, hitsMax: 3000, store: {}, storeCapacity: 300, spawnCooldownTime: 0, powers: {1: {level: 2}}},
        {_id: 'pc2', name: 'Spawned', className: 'operator', user: A, level: 0, hitsMax: 1000, store: {}, storeCapacity: 100, spawnCooldownTime: null, powers: {}},
    ],
    'market.orders': [
        {_id: 'o1', created: 5, active: true, type: 'sell', resourceType: 'energy', price: 1500, user: A, roomName: 'W2N1', amount: 100, remainingAmount: 100, totalAmount: 100},
        {_id: 'o2', created: 6, active: true, type: 'buy', resourceType: 'H', price: 20000, user: B, roomName: 'W1N2', amount: 5, remainingAmount: 5, totalAmount: 5},
        {_id: 'o3', created: 7, active: false, type: 'sell', resourceType: 'energy', price: 1000, user: A, roomName: 'W2N1', amount: 0, remainingAmount: 0, totalAmount: 10},
        {_id: 'o4', created: 8, active: true, type: 'buy', resourceType: 'energy', price: 999, roomName: 'W5N5', amount: 1000, remainingAmount: 1000, totalAmount: 1000},
    ],
    'market.stats': [
        {_id: 's1', resourceType: 'energy', date: '2020-09-10', transactions: 1, volume: 10, avgPrice: 1.5, stddevPrice: 0},
        {_id: 's2', resourceType: 'energy', date: '2020-09-12', transactions: 2, volume: 20, avgPrice: 1.25, stddevPrice: 0.25},
        {_id: 's3', resourceType: 'H', date: '2020-09-11', transactions: 1, volume: 1, avgPrice: 20, stddevPrice: 0},
    ],
    rooms: [
        {_id: 'W1N1', status: 'normal'},
        {_id: 'W2N1', status: 'normal'},
        {_id: 'W1N2', status: 'normal', novice: 1700000000000},
        {_id: 'W5N5', status: 'normal'},
        {_id: 'W3N1', status: 'out of borders'},
        {_id: 'W3N2', status: 'normal', openTime: FIXED + 5000},
    ],
    'rooms.objects': [
        {_id: 'ctrl1', type: 'controller', room: 'W1N1', x: 20, y: 20, level: 0},
        {_id: 'src1', type: 'source', room: 'W1N1', x: 10, y: 10, energy: 3000, energyCapacity: 3000},
        {_id: 'src2', type: 'source', room: 'W1N1', x: 30, y: 30, energy: 3000, energyCapacity: 3000, invaderHarvested: 50},
        {_id: 'min1', type: 'mineral', room: 'W1N1', x: 40, y: 40, mineralType: 'H', density: 3},
        {_id: 'oldext', type: 'extension', room: 'W1N1', x: 5, y: 5, user: '3', hits: 100, hitsMax: 1000, store: {energy: 7}},
        {_id: 'oldroad', type: 'road', room: 'W1N1', x: 6, y: 5, hits: 100, hitsMax: 5000},
        {_id: 'oldcreep', type: 'creep', room: 'W1N1', x: 7, y: 5, user: '2', name: 'inv'},
        {_id: 'ctrl2', type: 'controller', room: 'W2N1', x: 20, y: 20, level: 2, user: A, safeMode: 99999, sign: {user: A, text: 'mine', time: 1, datetime: 2}},
        {_id: 'spawn2', type: 'spawn', room: 'W2N1', x: 25, y: 25, name: 'Spawn1', user: A, store: {energy: 300}, notifyWhenAttacked: true},
        {_id: 'creep2', type: 'creep', room: 'W2N1', x: 26, y: 25, name: 'c1', user: A, store: {energy: 10}},
        {_id: 'site2', type: 'constructionSite', room: 'W2N1', x: 27, y: 25, structureType: 'extension', name: '', user: A, progress: 0, progressTotal: 3000},
        {_id: 'ext2', type: 'extension', room: 'W2N1', x: 29, y: 25, user: A, hits: 1000, hitsMax: 1000, store: {energy: 50}},
        {_id: 'ruin2', type: 'ruin', room: 'W2N1', x: 40, y: 40, user: A, structure: {id: 'gone', type: 'road', hits: 0, user: A}, store: {}, destroyTime: 1, decayTime: 2},
        {_id: 'src3', type: 'source', room: 'W2N1', x: 10, y: 40},
        {_id: 'ctrl3', type: 'controller', room: 'W1N2', x: 20, y: 20, level: 0, reservation: {user: B, endTime: 500}, sign: {user: B, text: 'hi', time: 5, datetime: 1}},
        {_id: 'core3', type: 'invaderCore', room: 'W1N2', x: 5, y: 5, level: 2, user: '2'},
        {_id: 'min3', type: 'mineral', room: 'W1N2', x: 9, y: 9, mineralType: 'O', density: 1},
        {_id: 'ctrl5', type: 'controller', room: 'W5N5', x: 20, y: 20, level: 0},
        {_id: 'ctrl31', type: 'controller', room: 'W3N1', x: 20, y: 20, level: 0},
        {_id: 'ctrl32', type: 'controller', room: 'W3N2', x: 20, y: 20, level: 0},
        {_id: 'pc2', type: 'powerCreep', room: 'W2N1', x: 28, y: 28, user: A, name: 'Spawned', className: 'operator', level: 0, hits: 1000, hitsMax: 1000, powers: {}, ageTime: 6000},
    ],
    'rooms.flags': [
        {_id: 'flags-a', user: A, room: 'W2N1', data: 'Flag1~1~2~10~10|Home~5~5~25~26'},
    ],
    'rooms.terrain': ['W1N1', 'W2N1', 'W1N2', 'W5N5', 'W3N1', 'W3N2'].map((room, i) => ({
        _id: 'terrain-' + room, room, type: 'terrain',
        terrain: terrain(room == 'W2N1' ? {'30,30': 3, '31,31': 2, '32,32': 1} : room == 'W1N1' ? {'11,11': 1, '0,24': 0} : {}),
    })),
    env: {gameTime: 1000, ['memory:' + A]: JSON.stringify({a: {b: [1, {c: null}], s: 'str'}, n: 5}), memorySegments: {[A]: {3: 'seg three'}}},
};

// STORE-INDEX-MODE: shipped DBs store lazy indices; collections created at runtime are adaptive
const ADAPTIVE = new Set(['users.console', 'users.power_creeps', 'users.money', 'users.resources']);
for (const name of config.common.dbCollections) ldb.addCollection(name, {adaptiveBinaryIndices: ADAPTIVE.has(name)});
for (const name of Object.keys(world)) {
    if (name == 'env') continue;
    const coll = ldb.getCollection(name);
    world[name].forEach(d => coll.insert(JSON.parse(JSON.stringify(d))));
}
const env = ldb.addCollection('env');
env.insert({databaseVersion: 9, data: {gameTime: '1000', ['memory:' + A]: world.env['memory:' + A],
    ['memorySegments:' + A]: world.env.memorySegments[A],
    ['auth_tok-' + A]: A, ['auth_tok-' + B]: B, ['auth_tok-' + CAROL]: CAROL, ['auth_tok-' + DAVE]: DAVE}});
env.insert({data: {}});

const dbjs = require(nm('@screeps/storage/lib/db.js'));
config.storage.getDb = () => ldb;
// results cross the storage RPC as JSON in the real server: callers get copies, never the stored docs
const copy = r => r === undefined ? r : JSON.parse(JSON.stringify(r));
// ...and as Q promises (RpcClient.request returns `q.defer().promise`): their prototype methods are enumerable,
// which `for (i in promise)` in utils.respawnUser observes (52 `rooms.update({_id: undefined})` calls)
const Q = require(nm('q'));
const p = (fn, ...a) => {
    const d = Q.defer();
    fn(...JSON.parse(JSON.stringify(a)), (e, r) => e ? d.reject(e) : d.resolve(copy(r)));
    return d.promise;
};
const published = [];
async function main() {
    await config.storage.loadDb();
    const st = common.storage;
    for (const c of config.common.dbCollections) {
        st.db[c] = {};
        for (const m of ['find', 'findOne', 'by', 'clear', 'count', 'ensureIndex', 'removeWhere', 'insert']) {
            st.db[c][m] = (...a) => p(dbjs.dbRequest, c, m, a);
        }
        st.db[c].update = (q, u, o) => p(dbjs.dbUpdate, c, q, u, o);
        st.db[c].bulk = b => p(dbjs.dbBulk, c, b);
        st.db[c].findEx = (q, o) => p(dbjs.dbFindEx, c, q, o);
    }
    const envFns = {get: 'dbEnvGet', mget: 'dbEnvMget', set: 'dbEnvSet', setex: 'dbEnvSetex', expire: 'dbEnvExpire', ttl: 'dbEnvTtl', del: 'dbEnvDel',
        hmget: 'dbEnvHmget', hmset: 'dbEnvHmset', hget: 'dbEnvHget', hset: 'dbEnvHset', sadd: 'dbEnvSadd', smembers: 'dbEnvSmembers'};
    for (const k in envFns) st.env[k] = (...a) => p(dbjs[envFns[k]], ...a);
    st.pubsub.publish = (channel, data) => { published.push([channel, data]); return Promise.resolve(); };
    st.pubsub.subscribe = () => {};

    const express = require(nm('express'));
    const bodyParser = require(nm('body-parser'));
    const api = n => require(nm('@screeps/backend/lib/game/api/' + n));
    const app = express();
    app.use(bodyParser.urlencoded({limit: '8mb', extended: true}));
    app.use(bodyParser.json({limit: '8mb'}));
    const auth = api('auth');
    auth.setup(app, false);
    const router = express.Router();
    router.use('/auth', auth.router);
    router.use('/user', api('user'));
    router.use('/register', api('register'));
    router.use('/game', api('game'));
    router.use('/leaderboard', api('leaderboard'));
    app.use('/api', router);
    const server = app.listen(0, '127.0.0.1');
    await new Promise(r => server.on('listening', r));
    const port = server.address().port;

    const send = (method, url, as, body) => new Promise((res, rej) => {
        const headers = {'Content-Type': 'application/json'};
        if (as) { headers['X-Token'] = 'tok-' + as; headers['X-Username'] = 'x'; }
        const req = http.request({host: '127.0.0.1', port, method, path: url, headers}, r => {
            let b = '';
            r.on('data', d => b += d);
            r.on('end', () => res({status: r.statusCode, body: b}));
        });
        req.on('error', rej);
        if (body !== undefined) req.write(JSON.stringify(body));
        req.end();
    });
    const dump = () => {
        const out = {};
        for (const c of ['users', 'users.code', 'users.console', 'users.messages', 'rooms', 'rooms.objects', 'rooms.flags', 'users.power_creeps', 'market.orders', 'users.notifications']) {
            const coll = ldb.getCollection(c);
            // adaptive collections: the order readers see is the `user` index order (find({user}) per user)
            const docs = ADAPTIVE.has(c) ? [].concat(...ldb.getCollection('users').find().map(u => coll.find({user: u._id}))) : coll.find();
            out[c] = docs.map(d => { const x = Object.assign({}, d); delete x.$loki; delete x.meta; return JSON.parse(JSON.stringify(x)); });
        }
        const e = env.get(1).data;
        out.env = {};
        for (const k of Object.keys(e).sort()) if (/^(memory|memorySegments):/.test(k)) out.env[k] = e[k];
        return out;
    };

    const G = (url, as) => ['GET', url, as];
    const P = (url, as, body) => ['POST', url, as, body];
    const steps = [
        G('/api/auth/me', A), G('/api/auth/me', CAROL), G('/api/auth/me'),
        G('/api/user/world-status', A), G('/api/user/world-status', B), G('/api/user/world-status', DAVE),
        G('/api/user/world-start-room', A), G('/api/user/world-start-room', DAVE),
        G('/api/user/respawn-prohibited-rooms', A),
        G('/api/user/branches', A), G('/api/user/branches', CAROL),
        G('/api/user/code', A), G('/api/user/code?branch=dev', A), G('/api/user/code?branch=$activeSim', A), G('/api/user/code?branch=nope', A),
        P('/api/user/code', A, {branch: 'dev', modules: {main: 'x', 'a.b': 'y', '.h': 'z', 'p/q': 'w'}, _hash: 'h1'}),
        P('/api/user/code', A, {modules: {main: ''}}),
        P('/api/user/code', A, {branch: 'zzz', modules: {main: 'q'}}),
        P('/api/user/set-active-branch', A, {activeName: 'activeWorld', branch: 'dev'}),
        P('/api/user/set-active-branch', A, {activeName: 'activeWorld', branch: 'none'}),
        P('/api/user/set-active-branch', A, {activeName: 'other', branch: 'dev'}),
        P('/api/user/clone-branch', A, {branch: 'dev', newName: 'copy'}),
        P('/api/user/clone-branch', A, {newName: 'fresh', defaultModules: {main: 'm', 'x.y': '1'}}),
        P('/api/user/clone-branch', A, {branch: 'x'.repeat(31), newName: 'n'}),
        P('/api/user/delete-branch', A, {branch: 'copy'}),
        P('/api/user/delete-branch', A, {branch: 'dev'}),
        G('/api/user/branches', A),
        G('/api/user/memory', A), G('/api/user/memory?path=a.b', A), G('/api/user/memory?path=missing', A), G('/api/user/memory?path=a.x.y', A),
        G('/api/user/memory', B),
        P('/api/user/memory', A, {path: 'a.b', value: 5}), P('/api/user/memory', A, {value: {x: 1}}), P('/api/user/memory', A, {path: "q'r"}),
        P('/api/user/console', A, {expression: 'Game.time'}), P('/api/user/console', A, {expression: 'x'.repeat(1023)}),
        G('/api/user/memory-segment?segment=3', A), G('/api/user/memory-segment?segment=4', A), G('/api/user/memory-segment?segment=100', A),
        P('/api/user/memory-segment', A, {segment: '7', data: 'hello'}), P('/api/user/memory-segment', A, {segment: 7.9, data: 'x'.repeat(102401)}),
        P('/api/user/badge', B, {badge: {type: 3, param: '-5', color1: '#AbCdEf', color2: '#ff0000', color3: '#000000', flip: 1}}),
        P('/api/user/badge', A, {badge: {type: {path1: 'M0', path2: 'M1'}, param: 0, color1: '#000000', color2: '#000000', color3: '#000000'}}),
        P('/api/user/badge', A, {badge: {type: 25, param: 0, color1: '#000000', color2: '#000000', color3: '#000000'}}),
        G('/api/user/find?username=ALICE'), G('/api/user/find?id=' + B), G('/api/user/find'),
        G('/api/user/rooms?id=' + A), G('/api/user/rooms'),
        G('/api/user/stats?interval=8'), G('/api/user/stats?interval=7'),
        G('/api/user/overview', A),
        P('/api/user/notify-prefs', B, {disabled: 0, interval: '60', errorsInterval: 7, sendOnline: null}),
        P('/api/user/email', B, {email: 'BOB@x.io'}), P('/api/user/email', DAVE, {email: 'alice@x.io'}), P('/api/user/email', DAVE, {email: 'nope'}),
        P('/api/user/tutorial-done', A, {}),
        G('/api/user/money-history', A),
        G('/api/user/badge-svg?username=ALICE'), G('/api/user/badge-svg?username=alice&border=1'), G('/api/user/badge-svg?username=dave'),
        G('/api/user/badge-svg?username=nobody'), G('/api/user/badge-svg'),
        P('/api/user/set-steam-visible', A, {visible: false}),
        G('/api/register/check-email?email=ALICE@x.io'), G('/api/register/check-email?email=new@x.io'), G('/api/register/check-email'),
        G('/api/register/check-username?username=Alice'), G('/api/register/check-username?username=zed'),
        P('/api/register/set-username', A, {username: 'zed'}),
        P('/api/register/set-username', CAROL, {username: 'ab'}),
        P('/api/register/set-username', CAROL, {username: 'Carol_1', email: 'C@Example.com'}),
        G('/api/auth/me', CAROL),
        P('/api/game/map-stats', A, {rooms: ['W1N2', 'W2N1', 'W1N1', 'W9N9', 'W3N2'], statName: 'owner0'}),
        P('/api/game/map-stats', A, {rooms: 'W1N1', statName: 'owner0'}),
        G('/api/game/room-status?room=W1N2', A), G('/api/game/room-status?room=W7N7', A),
        G('/api/game/room-terrain?room=W2N1&encoded=1'), G('/api/game/room-terrain?room=W9N9'),
        P('/api/game/rooms', undefined, {rooms: ['W2N1', 'W1N1', 'W9N9', 'W2N1']}),
        G('/api/game/time'), G('/api/game/world-size'),
        G('/api/game/room-overview?room=W2N1', A), G('/api/game/room-overview?room=W1N1', A),
        P('/api/game/gen-unique-object-name', A, {type: 'spawn'}), P('/api/game/gen-unique-object-name', A, {type: 'flag'}),
        P('/api/game/check-unique-object-name', A, {type: 'spawn', name: 'Spawn1'}), P('/api/game/check-unique-object-name', A, {type: 'spawn', name: 'Other'}),
        P('/api/game/place-spawn', DAVE, {room: 'W3N1', x: 25, y: 25, name: 'S'}),
        P('/api/game/place-spawn', DAVE, {room: 'W3N2', x: 25, y: 25, name: 'S'}),
        P('/api/game/place-spawn', DAVE, {room: 'W2N1', x: 25, y: 25, name: 'S'}),
        P('/api/game/place-spawn', DAVE, {room: 'W1N1', x: 11, y: 11, name: 'S'}),
        P('/api/game/place-spawn', DAVE, {room: 'W1N1', x: 1, y: 20, name: 'S'}),
        P('/api/game/place-spawn', DAVE, {room: 'W1N1', x: '25', y: 25, name: 'Spawn9'}),
        P('/api/game/place-spawn', DAVE, {room: 'W5N5', x: 25, y: 25}),
        G('/api/user/world-status', DAVE),
        P('/api/game/create-construction', A, {room: 'W2N1', x: 30, y: 30, structureType: 'road'}),
        P('/api/game/create-construction', A, {room: 'W2N1', x: 31, y: 31, structureType: 'road'}),
        P('/api/game/create-construction', A, {room: 'W2N1', x: 32, y: 32, structureType: 'extension'}),
        P('/api/game/create-construction', A, {room: 'W2N1', x: 33, y: 33, structureType: 'tower'}),
        P('/api/game/create-construction', A, {room: 'W2N1', x: 34, y: 34, structureType: 'spawn'}),
        P('/api/game/create-construction', A, {room: 'W2N1', x: 35, y: 35, structureType: 'spawn', name: 'S2'}),
        P('/api/game/create-construction', B, {room: 'W2N1', x: 36, y: 36, structureType: 'road'}),
        P('/api/game/create-construction', A, {room: 'W2N1', x: 25, y: 25, structureType: 'rampart'}),
        P('/api/game/gen-unique-flag-name', A, {}), P('/api/game/check-unique-flag-name', A, {name: 'Home'}),
        P('/api/game/create-flag', A, {room: 'W1N1', x: 3, y: 4.5, name: 'Home', color: 2, secondaryColor: 3}),
        P('/api/game/create-flag', A, {room: 'W1N1', x: '3', y: 4, name: 'X', color: 2, secondaryColor: 3}),
        P('/api/game/create-flag', A, {room: 'W9N9', x: 3, y: 4, name: 'X', color: 2, secondaryColor: 3}),
        P('/api/game/change-flag-color', A, {room: 'W2N1', name: 'Flag1', color: 7, secondaryColor: 8}),
        P('/api/game/change-flag-color', A, {room: 'W2N1', name: 'Flag1', color: 11, secondaryColor: 8}),
        P('/api/game/remove-flag', A, {room: 'W1N1', name: 'Home'}),
        P('/api/game/set-notify-when-attacked', A, {_id: 'spawn2', enabled: false}),
        P('/api/game/set-notify-when-attacked', B, {_id: 'spawn2', enabled: false}),
        P('/api/game/set-notify-when-attacked', B, {_id: 'src3', enabled: true}),
        P('/api/game/create-invader', A, {room: 'W1N2', x: 5, y: 5, size: 'small', type: 'Melee'}),
        P('/api/game/create-invader', A, {room: 'W2N1', x: 5, y: 6, size: 'big', type: 'Ranged', boosted: true}),
        P('/api/game/create-invader', B, {room: 'W1N2', x: 7, y: 7, size: 'small', type: 'Healer'}),
        P('/api/user/messages/send', A, {respondent: B, text: 'hi bob'}),
        P('/api/user/messages/send', A, {respondent: 'nobody', text: 'x'}),
        P('/api/user/messages/send', A, {respondent: 'sys000000000005', text: 'hello system'}),
        P('/api/user/messages/send', A, {respondent: 'sys000000000005', text: 'again'}),
        G('/api/user/messages/unread-count', B), G('/api/user/messages/list?respondent=' + A, B),
        G('/api/leaderboard/list'), G('/api/leaderboard/find?season=x'), G('/api/leaderboard/seasons'),
        // market and power creeps
        G('/api/game/market/orders-index', A), G('/api/game/market/orders?resourceType=energy', A), G('/api/game/market/orders', A),
        G('/api/game/market/my-orders', A), G('/api/game/market/stats?resourceType=energy', A), G('/api/game/market/stats', A),
        G('/api/game/power-creeps/list', A),
        P('/api/game/power-creeps/create', A, {className: 'operator', name: 'Op'}),
        P('/api/game/power-creeps/create', A, {className: 'x', name: 'N'}),
        P('/api/game/power-creeps/create', DAVE, {className: 'operator', name: 'N'}),
        P('/api/game/power-creeps/create', A, {className: 'operator', name: 'New'.repeat(20)}),
        P('/api/game/power-creeps/upgrade', A, {id: 'pc1', powers: {1: 1}}),
        P('/api/game/power-creeps/upgrade', A, {id: 'pc1', powers: {99: 1}}),
        P('/api/game/power-creeps/upgrade', A, {id: 'pc1', powers: {1: 6}}),
        P('/api/game/power-creeps/upgrade', A, {id: 'pc1', powers: {1: '3'}}),
        P('/api/game/power-creeps/upgrade', A, {id: 'pc1', powers: 5}),
        P('/api/game/power-creeps/upgrade', A, {id: 'nope', powers: {}}),
        P('/api/game/power-creeps/upgrade', A, {id: 'pc1', powers: {1: 2, 2: 1, 3: 5}}),
        P('/api/game/power-creeps/upgrade', A, {id: 'pc1', powers: {2: 1}}),
        P('/api/game/power-creeps/upgrade', A, {id: 'pc1', powers: {1: 2, 2: 1}}),
        P('/api/game/power-creeps/upgrade', A, {id: 'pc2', powers: {1: 1}}),
        G('/api/game/power-creeps/list', A),
        P('/api/game/power-creeps/rename', A, {id: 'pc1', name: 'Renamed'}),
        P('/api/game/power-creeps/rename', A, {id: 'pc2', name: 'x'}),
        P('/api/game/power-creeps/rename', A, {id: 'pc1', name: 'Spawned'}),
        P('/api/game/power-creeps/delete', A, {id: 'pc2'}),
        P('/api/game/power-creeps/delete', B, {id: 'pc1'}),
        P('/api/game/power-creeps/delete', A, {id: 'pc1'}),
        P('/api/game/power-creeps/delete', A, {id: 'pc1'}),
        P('/api/game/power-creeps/cancel-delete', A, {id: 'pc1'}),
        P('/api/game/power-creeps/cancel-delete', A, {id: 'pc1'}),
        P('/api/game/power-creeps/experimentation', A, {}),
        P('/api/game/power-creeps/experimentation', DAVE, {}),
        P('/api/game/power-creeps/delete', A, {id: 'pc1'}),
        // absent query values: the storage RPC's JSON drops `{k: undefined}` (match-all clauses)
        G('/api/game/room-status', A), G('/api/game/room-overview', A), G('/api/game/room-terrain'),
        G('/api/user/messages/list', B),
        P('/api/user/clone-branch', B, {defaultModules: {main: 'bot'}}),
        P('/api/user/set-active-branch', DAVE, {activeName: 'activeSim'}),
        P('/api/user/delete-branch', A, {}),
        P('/api/game/remove-invader', A, {}),
        P('/api/user/respawn', CAROL, {}), P('/api/user/respawn', DAVE, {}), P('/api/user/respawn', A, {}),
        G('/api/user/world-status', A), G('/api/user/world-status', DAVE),
    ];
    const out = [];
    let last = dump();
    for (const [method, url, as, body] of steps) {
        published.length = 0;
        const res = await send(method, url, as, body);
        const step = {method, url, as: as || null, body: body === undefined ? null : body, status: res.status, response: res.body};
        // collections this step changed (whole collection, as stored)
        const now = dump();
        step.db = {};
        for (const c in now) if (JSON.stringify(now[c]) != JSON.stringify(last[c])) step.db[c] = now[c];
        last = now;
        step.published = published.slice();
        out.push(step);
    }
    server.close();
    fs.writeFileSync(path.join(__dirname, 'api.json'), JSON.stringify({fixed: FIXED, world, steps: out}, null, 1) + '\n');
    process.exit(0);
}
process.on('unhandledRejection', () => {}); // vanilla logs these (e.g. q.reject() in sendMessageNotification)
main().catch(e => { console.error(e); process.exit(1); });
