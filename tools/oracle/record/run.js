// Records N ticks of the official server with the chitin recorder (index.js), in an isolated server
// directory (never the user's E:\screeps\server data): the launcher's default world
// (reference/launcher/init_dist/db.json: four bots with code), built-in Loki storage, runner_threads 1,
// processors_cnt 1, tick duration 50 ms, crons off, ports 21125/21126.
//   E:\screeps\node22\node-v22.22.3-win-x64\node.exe tools/oracle/record/run.js [ticks=100] [dir]
// Output: <dir>/record/{rooms,global,order}.ndjson and snapshot-<T>.json; then check it with
//   node22 tools/oracle/record/check.js <dir>/record
'use strict';
const fs = require('fs');
const os = require('os');
const path = require('path');
const cp = require('child_process');
const { reference } = require('../lib/common');

const SCREEPS_ROOT = process.env.SCREEPS_ROOT || 'E:/screeps/server';
const ticks = Number(process.argv[2] || 100);
const dir = path.resolve(process.argv[3] || path.join(os.tmpdir(), 'chitin-record-run'));
const out = path.join(dir, 'record');
const recorder = path.join(__dirname, 'index.js');

fs.rmSync(dir, { recursive: true, force: true });
fs.mkdirSync(dir, { recursive: true });
const db = JSON.parse(fs.readFileSync(reference('launcher/init_dist/db.json'), 'utf8'));
db.collections.find((c) => c.name === 'env').data[0].data.tickRate = 50;
fs.writeFileSync(path.join(dir, 'db.json'), JSON.stringify(db));
fs.writeFileSync(path.join(dir, 'mods.json'), JSON.stringify({ mods: [recorder], bots: {} }));
fs.writeFileSync(path.join(dir, '.screepsrc'), [
  'steam_api_key = none', 'port = 21125', 'host = 127.0.0.1', 'cli_port = 21126', 'cli_host = 127.0.0.1', 'runners_cnt = 1',
  'runner_threads = 1', 'processors_cnt = 1', 'logdir = logs', 'modfile = mods.json', `assetdir = ${reference('launcher/init_dist/assets')}`,
  'db = db.json', 'storage_disabled = false', 'restart_interval = 0', '',
].join('\n'));

const launcher = cp.spawn(process.execPath, [path.join(SCREEPS_ROOT, 'node_modules/@screeps/launcher/bin/screeps.js'), 'start'], {
  cwd: dir,
  env: Object.assign({}, process.env, { NODE_OPTIONS: `--require=${recorder}`, CHITIN_RECORD_DIR: out }),
  stdio: ['ignore', 'inherit', 'inherit'],
});

const lines = (f) => {
  try {
    return fs.readFileSync(path.join(out, f), 'utf8').split('\n').filter(Boolean).length;
  } catch (e) {
    return 0;
  }
};
const started = Date.now();
const stop = (code) => {
  if (process.platform === 'win32') cp.spawnSync('taskkill', ['/pid', String(launcher.pid), '/T', '/F'], { stdio: 'ignore' });
  else launcher.kill('SIGTERM');
  console.log(`recorded ${lines('global.ndjson')} ticks (${lines('rooms.ndjson')} rooms, ${lines('order.ndjson')} runs) into ${out}`);
  process.exit(code);
};
const timer = setInterval(() => {
  if (lines('global.ndjson') >= ticks) return stop(0);
  if (Date.now() - started > Math.max(120000, ticks * 2000)) {
    console.error('timed out; see', path.join(dir, 'logs'));
    stop(1);
  }
}, 1000);
launcher.on('exit', (code) => {
  clearInterval(timer);
  console.error(`launcher exited (${code}) early; see`, path.join(dir, 'logs'));
  process.exit(1);
});
