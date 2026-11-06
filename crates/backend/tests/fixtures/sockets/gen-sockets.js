// Socket oracle: the official socket modules (rooms.js, map.js, user.js) on the official storage db.js over an
// in-memory Loki DB. Connections subscribe, then one `roomsDone` broadcast runs after DB changes; every message
// each connection receives is recorded. server.rs `oracle::official_sockets` replays it on chitin.
//   node gen-sockets.js <SCREEPS_ROOT, e.g. E:\screeps\server>      (node 22)
'use strict';
const path = require('path');
const fs = require('fs');
const ROOT = process.argv[2];
const nm = p => path.join(ROOT, 'node_modules', p);
const FIXED = 1600000000000 + 1000 * 1000;
const RealDate = Date;
global.Date = class extends RealDate {
    constructor(...a) { if (a.length) super(...a); else super(FIXED); }
    static now() { return FIXED; }
};
Math.random = () => 0.5;

const common = require(nm('@screeps/common'));
const config = common.configManager.config;
const EventEmitter = require('events').EventEmitter;
config.storage = {};
config.backend = Object.assign(new EventEmitter(), {customIntentTypes: {}, socketUpdateThrottle: 200});
const loki = require(nm('lokijs'));
const ldb = new loki('memory');
ldb.loadDatabase = (opts, cb) => cb();

const A = 'a11ce0000000001', B = 'b0b000000000002', C = 'ca4010000000003';
const flat = '0'.repeat(2500);
const world = {
    users: [
        {_id: '2', username: 'Invader', usernameLower: 'invader', cpu: 0},
        {_id: '3', username: 'Source Keeper', usernameLower: 'source keeper', cpu: 0},
        {_id: A, username: 'alice', usernameLower: 'alice', cpu: 100, money: 12345, resources: {token: 2, credits: 7},
            badge: {type: 1, color1: '#ff0000', color2: '#00ff00', color3: '#0000ff', param: 0, flip: false}, email: 'a@x.io'},
        {_id: B, username: 'bob', usernameLower: 'bob', cpu: 100, badge: {type: 2, color1: 1, color2: 2, color3: 3, param: 10, flip: true}},
        {_id: C, username: 'carol', usernameLower: 'carol', cpu: 100},
    ],
    'users.code': [], 'users.console': [], 'users.messages': [], 'users.power_creeps': [], 'market.orders': [], 'market.stats': [],
    rooms: [{_id: 'W1N1', status: 'normal'}, {_id: 'W2N1', status: 'normal'}, {_id: 'W1N2', status: 'normal'}],
    'rooms.objects': [
        {_id: 'ctrl1', type: 'controller', room: 'W1N1', x: 20, y: 20, level: 0},
        {_id: 'ctrl2', type: 'controller', room: 'W2N1', x: 20, y: 20, level: 2, user: A},
        {_id: 'spawn2', type: 'spawn', room: 'W2N1', x: 25, y: 25, name: 'Spawn1', user: A, store: {energy: 300}},
        {_id: 'creep2', type: 'creep', room: 'W2N1', x: 26, y: 25, name: 'c1', user: A, store: {energy: 10, H: 1}, body: [{type: 'move', hits: 100}, {type: 'work', hits: 100}]},
        {_id: 'site2', type: 'constructionSite', room: 'W2N1', x: 27, y: 25, structureType: 'extension', user: A, progress: 0, progressTotal: 3000},
        {_id: 'ctrl3', type: 'controller', room: 'W1N2', x: 20, y: 20, level: 0, reservation: {user: B, endTime: 500}, sign: {user: B, text: 'hi', time: 5, datetime: 1}},
        {_id: 'core3', type: 'invaderCore', room: 'W1N2', x: 5, y: 5, level: 2, user: '2'},
    ],
    'rooms.flags': [{_id: 'flags-a', user: A, room: 'W2N1', data: 'Flag1~1~2~10~10'}],
    'rooms.terrain': ['W1N1', 'W2N1', 'W1N2'].map(room => ({_id: 't-' + room, room, type: 'terrain', terrain: flat})),
    env: {gameTime: 1000, ['memory:' + A]: JSON.stringify({a: {b: [1, 2]}, s: 'x'}), memorySegments: {},
        'mapView:W2N1': '{"w":[],"r":[[1,1]],"a11ce0000000001":[[25,25],[26,25]]}', 'mapView:W1N2': '{"c":[[20,20]]}'},
};
// one roomsDone after these changes (applied with the storage API, as the processor's commit would)
const tick = {
    gameTime: 1001,
    set: [['creep2', {store: {energy: 20}, x: 27}]],
    insert: [{_id: 'creepB', type: 'creep', room: 'W2N1', x: 30, y: 30, name: 'b1', user: B, store: {}}],
    remove: ['site2'],
    memory: {[A]: JSON.stringify({a: {b: 'changed'}, s: 'x'})},
    visuals: [[A, '', '{"t":"g"}\n'], [A, 'W2N1', '{"t":"r"}\n'], [B, 'W1N1', '{"t":"x"}\n']],
    mapView: {'W2N1': '{"w":[],"r":[[1,1]],"a11ce0000000001":[[25,25],[27,25]],"b0b000000000002":[[30,30]]}'},
};
// connections: [name, user, subscriptions in order]
const conns = [
    ['a', A, ['room:W2N1', 'roomMap2:W2N1', 'user:' + A + '/cpu', 'user:' + A + '/memory/a.b', 'user:' + A + '/resources', 'roomMap2:W9N9']],
    ['b', C, ['room:W2N1']],
    ['c', B, ['room:W1N2', 'room:W1N1']],
];

const ADAPTIVE = new Set(['users.console', 'users.power_creeps', 'users.money', 'users.resources']);
for (const name of config.common.dbCollections) ldb.addCollection(name, {adaptiveBinaryIndices: ADAPTIVE.has(name)});
for (const name of Object.keys(world)) {
    if (name == 'env') continue;
    world[name].forEach(d => ldb.getCollection(name).insert(JSON.parse(JSON.stringify(d))));
}
const envData = {gameTime: '1000'};
for (const k in world.env) if (k != 'gameTime' && k != 'memorySegments') envData[k] = world.env[k];
const env = ldb.addCollection('env');
env.insert({databaseVersion: 9, data: envData});
env.insert({data: {}});

const dbjs = require(nm('@screeps/storage/lib/db.js'));
config.storage.getDb = () => ldb;
const copy = r => r === undefined ? r : JSON.parse(JSON.stringify(r));
// Q promises, as the storage RpcClient returns them
const Q = require(nm('q'));
const p = (fn, ...a) => {
    const d = Q.defer();
    fn(...JSON.parse(JSON.stringify(a)), (e, r) => e ? d.reject(e) : d.resolve(copy(r)));
    return d.promise;
};
const settle = (ms = 60) => new Promise(r => setTimeout(r, ms));

async function main() {
    await config.storage.loadDb();
    const st = common.storage;
    for (const c of config.common.dbCollections) {
        st.db[c] = {};
        for (const m of ['find', 'findOne', 'by', 'clear', 'count', 'ensureIndex', 'removeWhere', 'insert']) st.db[c][m] = (...a) => p(dbjs.dbRequest, c, m, a);
        st.db[c].update = (q, u, o) => p(dbjs.dbUpdate, c, q, u, o);
        st.db[c].findEx = (q, o) => p(dbjs.dbFindEx, c, q, o);
    }
    const envFns = {get: 'dbEnvGet', mget: 'dbEnvMget', set: 'dbEnvSet', setex: 'dbEnvSetex', hget: 'dbEnvHget', hset: 'dbEnvHset'};
    for (const k in envFns) st.env[k] = (...a) => p(dbjs[envFns[k]], ...a);

    // socket/server.js plumbing: listen(regexp, fn) + emit(channel, data) to registered connections
    const listeners = [];
    const registered = {}; // conn name → Set(channel)
    const out = {};
    const connObjs = {};
    const emit = (channel, data) => {
        for (const name in connObjs) if (registered[name].has(channel)) connObjs[name]._writeEvent(channel, data);
    };
    const listen = (regexp, fn) => listeners.push({regexp, fn});
    const sock = n => require(nm('@screeps/backend/lib/game/socket/' + n));
    const modules = [sock('system'), sock('rooms'), sock('user'), sock('map')].map(m => m(listen, emit));

    for (const [name, uid, subs] of conns) {
        out[name] = [];
        registered[name] = new Set();
        const conn = connObjs[name] = Object.assign(new EventEmitter(), {
            readyState: 1,
            _writeEventRaw: m => out[name].push(m),
            _writeEvent: (ch, data) => out[name].push(JSON.stringify([ch, data])),
            write: m => out[name].push(m),
        });
        const user = await st.db.users.findOne({_id: uid});
        for (const channel of subs) {
            for (const m of modules) if (m.onSubscribe(channel, user, conn)) registered[name].add(channel);
            await settle();
        }
    }
    const subscribed = JSON.parse(JSON.stringify(out));
    for (const name in out) out[name].length = 0;

    // the tick: DB changes, env keys, then roomsDone (every module's throttled handler fires on the first call)
    for (const [id, $set] of tick.set) await st.db['rooms.objects'].update({_id: id}, {$set});
    for (const doc of tick.insert) await st.db['rooms.objects'].insert(doc);
    for (const id of tick.remove) await st.db['rooms.objects'].removeWhere({_id: id});
    await st.env.set('gameTime', '' + tick.gameTime);
    for (const uid in tick.memory) await st.env.set('memory:' + uid, tick.memory[uid]);
    for (const [uid, room, v] of tick.visuals) await st.env.set(`roomVisual:${uid},${room},${tick.gameTime - 1}`, v);
    for (const room in tick.mapView) await st.env.set('mapView:' + room, tick.mapView[room]);
    for (const l of listeners) if (l.regexp.exec('roomsDone')) l.fn(undefined, null);
    await settle(300);

    fs.writeFileSync(path.join(__dirname, 'sockets.json'), JSON.stringify({fixed: FIXED, world, tick, conns, subscribed, broadcast: out}, null, 1) + '\n');
    process.exit(0);
}
process.on('unhandledRejection', e => console.error('rejection', e));
main().catch(e => { console.error(e); process.exit(1); });
