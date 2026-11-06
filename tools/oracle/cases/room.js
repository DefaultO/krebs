// Room-processor oracle cases for room intents and the passive ticks of the M1 room objects (WP10:
// room/*, controllers/tick, sources/tick, energy/tick, construction-sites/tick, roads/containers/tombstones).
// Regenerate: node22 tools/oracle/processor.js gen tools/oracle/cases/room.js
'use strict';
const { scenario } = require('../lib/scenario');

function base(name, tags, { level = 2, ...opts } = {}) {
  const s = scenario(name, Object.assign({ tags }, opts));
  s.ctrl = s.controller(25, 25, { user: 'u1', level, progress: 100 });
  return s;
}

const cases = [];
const add = (s) => cases.push(s.build());

{
  const s = base('createConstructionSite: extension, road on swamp, and a site on a wall', ['room/create-construction-site.js'], {
    terrain: { swamps: [[12, 12]], walls: [[13, 13]] },
  });
  s.roomIntent('u1', 'createConstructionSite', { x: 10, y: 10, structureType: 'extension' });
  s.roomIntent('u1', 'createConstructionSite', { x: 12, y: 12, structureType: 'road' });
  s.roomIntent('u1', 'createConstructionSite', { x: 13, y: 13, structureType: 'extension' });
  add(s);
}
{
  const s = base('createConstructionSite: RCL limit, foreign controller user, spawn name', ['room/create-construction-site.js:20'], { level: 1 });
  s.user('u2');
  s.roomIntent('u1', 'createConstructionSite', { x: 10, y: 10, structureType: 'extension' });
  s.roomIntent('u1', 'createConstructionSite', { x: 30, y: 30, structureType: 'spawn', name: 'Second' });
  s.roomIntent('u2', 'createConstructionSite', { x: 11, y: 11, structureType: 'road' });
  add(s);
}
{
  const s = base('removeConstructionSite and the site tick', ['room/remove-construction-site.js', 'construction-sites/tick.js']);
  const a = s.site(10, 10, 'road', { progress: 20 });
  s.site(11, 11, 'extension');
  s.roomIntent('u1', 'removeConstructionSite', { id: a });
  add(s);
}
{
  const s = base('createFlag and removeFlag', ['room/create-flag.js', 'room/remove-flag.js', 'room/intents.js:44']);
  s.flag('u1', 'Old', 5, 5);
  s.flag('u1', 'Keep', 6, 6, 2, 3);
  s.roomIntent('u1', 'removeFlag', { name: 'Old' });
  s.roomIntent('u1', 'createFlag', { x: 7, y: 7, name: 'New', color: 4, secondaryColor: 5 });
  add(s);
}
{
  const s = base('controller tick: downgrade warning and level loss', ['controllers/tick.js:40']);
  s.objects.find((o) => o._id === s.ctrl).downgradeTime = 1000 + 3000;
  add(s);
}
{
  const s = scenario('controller tick: downgrade from level 2', { tags: ['controllers/tick.js:50'] });
  s.controller(25, 25, { user: 'u1', level: 2, progress: 500, downgradeTime: 999 });
  s.user('u1', { rooms: ['W1N1'] });
  add(s);
}
{
  const s = base('source tick: regeneration timer starts and ends', ['sources/tick.js']);
  s.source(10, 10, { energy: 100 });
  s.source(40, 40, { energy: 10, nextRegenerationTime: 1001 });
  add(s);
}
{
  const s = base('energy tick: piles decay, a 1-unit pile vanishes', ['energy/tick.js']);
  s.drop(5, 5, 1);
  s.drop(6, 6, 2500);
  s.drop(7, 7, 300, 'H');
  add(s);
}
{
  const s = base('road, container and tombstone ticks: decay and removal', ['roads/tick.js', 'containers/tick.js', 'tombstones/tick.js']);
  s.road(10, 10, { hits: 100, nextDecayTime: 1000 });
  s.container(11, 11, { nextDecayTime: 1000, store: { energy: 10 } });
  s.tombstone(12, 12, { decayTime: 1000, store: { energy: 40 } });
  add(s);
}

module.exports = cases;
