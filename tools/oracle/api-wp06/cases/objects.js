// Source, Mineral, Flag, ConstructionSite: getters and their truthy-only caches, toString/toJSON, memory, flag and
// site methods (spec 03 SOURCE-PROPS, MINERAL-PROPS, FLAG-*, CS-*, CORE-GAMEOBJ-GETTER).
'use strict';
const { base } = require('../world');

module.exports = [{
    name: 'objects-source-mineral',
    tags: ['game/sources.js:26', 'game/minerals.js:26'],
    world: base({ objects: base().objects.concat([
        { _id: 'm2', type: 'mineral', room: 'W1N2', x: 40, y: 40, mineralType: 'O', mineralAmount: 0, density: 1, nextRegenerationTime: 1500,
          effects: [{ effect: 1001, endTime: 1100 }, { effect: 1002, power: 6, level: 2, endTime: 900 }] },
    ]) }),
    ticks: [{ probes: [
        ['source', () => {
            const s = Game.getObjectById('s1'), s2 = Game.getObjectById('s2');
            return [s.energy, s.energyCapacity, s.ticksToRegeneration, s2.ticksToRegeneration, s2.energy, String(s), s.id,
                Object.keys(s), Object.keys(s2), JSON.stringify(s), JSON.stringify(s2), Object.keys(s2), s instanceof RoomObject];
        }],
        ['source props', () => {
            const d = Object.getOwnPropertyDescriptor(Source.prototype, 'energy');
            const s = Game.getObjectById('s1');
            let assign;
            try { s.energy = 5; assign = s.energy; } catch (e) { assign = e.constructor.name; }
            return [d.enumerable, d.configurable, typeof d.set, Object.keys(Source.prototype), assign, Source.prototype.constructor === Source,
                Object.getPrototypeOf(Source.prototype) === RoomObject.prototype];
        }],
        ['mineral', () => {
            const m = Game.getObjectById('m1'), m2 = Game.getObjectById('m2');
            return [m.mineralType, m.mineralAmount, m.density, m.ticksToRegeneration, String(m), m2.mineralAmount, m2.ticksToRegeneration,
                m2.effects, JSON.stringify(m), Object.keys(m), Object.keys(Mineral.prototype)];
        }],
        ['stale', () => {
            const s = Game.getObjectById('s1');
            return [Game.getObjectById('s4'), s.toJSON(), s.room === Game.rooms.W1N1];
        }],
    ] }, {
        world: { objects: base().objects.filter(o => o._id != 's1').map(o => o._id == 's2' ? Object.assign({}, o, { energy: 77 }) : o) },
        probes: [
            ['next tick', () => [Game.getObjectById('s2').energy, typeof Source, Game.getObjectById('s1')]],
        ],
    }],
}, {
    name: 'objects-stale-across-ticks',
    tags: ['game/sources.js:35', 'game/construction-sites.js:36'],
    world: base(),
    ticks: [{ probes: [
        ['keep', () => { global.keep = [Game.getObjectById('s1'), Game.getObjectById('s2'), Game.getObjectById('cs1')]; global.keep[0].energy; return 1; }],
    ] }, {
        world: { objects: base().objects.filter(o => o._id != 'cs1').map(o => o._id == 's1' ? Object.assign({}, o, { energy: 5 }) :
            o._id == 's2' ? Object.assign({}, o, { energy: 9 }) : o) },
        probes: [
            ['stale reads', () => {
                const [s1, s2, cs] = global.keep;
                let csp;
                try { csp = cs.progress; } catch (e) { csp = e.message; }
                return [s1.energy, s2.energy, csp, s1.room === Game.rooms.W1N1, s1 === Game.getObjectById('s1')];
            }],
        ],
    }],
}, {
    name: 'objects-construction-site',
    tags: ['game/construction-sites.js:27', 'game/construction-sites.js:46', 'game/construction-sites.js:50'],
    world: base({ objects: base().objects.concat([
        { _id: 'cs4', type: 'constructionSite', room: 'W1N2', x: 30, y: 31, user: 'u2', structureType: 'extension', progress: 1, progressTotal: 3000 },
        { _id: 'cs5', type: 'constructionSite', room: 'W1N1', x: 31, y: 31, structureType: 'road', progress: 0, progressTotal: 300 },
    ]) }),
    ticks: [{ probes: [
        ['props', () => ['cs1', 'cs2', 'cs3', 'cs4'].map(id => {
            const c = Game.getObjectById(id);
            return [c.progress, c.progressTotal, c.structureType, c.name, c.owner, c.my, String(c), c.room && c.room.name, Object.keys(c)];
        })],
        ['ownerless', () => { const c = Game.getObjectById('cs5'); let o; try { o = c.owner; } catch (e) { o = e.constructor.name; } return [c.my, o]; }],
        ['remove', () => ['cs1', 'cs2', 'cs3', 'cs4', 'cs5'].map(id => Game.getObjectById(id).remove())],
        ['game', () => [Object.keys(Game.constructionSites), Game.rooms.W1N1.find(FIND_MY_CONSTRUCTION_SITES), Game.rooms.W1N1.find(FIND_HOSTILE_CONSTRUCTION_SITES)]],
        ['json', () => JSON.stringify(Game.getObjectById('cs1'))],
    ] }],
}, {
    name: 'objects-flags',
    tags: ['game/flags.js:20', 'game/flags.js:31', 'game/flags.js:53', 'game/flags.js:63', 'game/flags.js:76'],
    world: base(),
    ticks: [{ probes: [
        ['props', () => Object.keys(Game.flags).map(n => { const f = Game.flags[n]; return [f.name, f.color, f.secondaryColor, f.pos, String(f), JSON.stringify(f), f.room && f.room.name, f.id]; })],
        ['memory', () => [Game.flags.Flag1.memory, Game.flags['My|Flag'].memory, Game.flags.Far.memory, Object.keys(Memory.flags),
            (Game.flags.Edge.memory = {z: 1}, Memory.flags.Edge)]],
        ['methods', () => {
            const f = Game.flags.Flag1;
            return [f.setColor(COLOR_RED), f.setColor(COLOR_RED, COLOR_GREEN), f.setColor(11), f.setColor(1, 11), f.setColor('1'),
                f.setPosition(10, 11), f.setPosition(new RoomPosition(5, 6, 'W1N2')), f.setPosition(Game.creeps.Harv1), f.setPosition(),
                f.setPosition({x: 1, y: 1}), f.setPosition(60, 70), Game.flags.Far.setPosition(3, 4), Game.flags.Far.remove(), f.remove()];
        }],
        ['after', () => [Game.flags.Flag1.color, Game.flags.Flag1.pos]],
    ] }],
}, {
    name: 'objects-flag-memory-shapes',
    tags: ['game/flags.js:31', 'game/rooms.js:551'],
    world: base({ memory: '{"flags":"undefined","rooms":5}' }),
    ticks: [{ probes: [
        ['flag mem', () => [Game.flags.Flag1.memory, Memory.flags]],
        ['room mem', () => { let s; try { Game.rooms.W1N1.memory = 1; } catch (e) { s = e.message; } return [Game.rooms.W1N1.memory, s]; }],
    ] }],
}];
