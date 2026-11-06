//! Port of reference/common/lib/constants.js.
//!
//! Types: return codes are `i32`, terrain masks `u8` (they apply to raw terrain bytes), other
//! integers `u32`, millisecond durations `u64`, fractional values `f64`, string values `&str`.
//! JS objects keyed by small integers (RCL, density, ...) are arrays indexed by that integer,
//! `None` where the JS object has no such key. JS objects keyed by strings are slices in source
//! order, because that order reaches player code through `constants_json`, plus O(1) lookup
//! functions for the simulation.

use std::fmt::{self, Display, Write as _};
use std::str::FromStr;

use BodyPart::*;
use PerLevel::{Fixed, Levels};
use ResourceType::*;
use StructureType::*;

macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $s:literal,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        #[repr(u8)]
        pub enum $name { $($variant,)* }

        impl $name {
            /// Every variant in index order.
            pub const ALL: &'static [$name] = &[$($name::$variant,)*];
            pub const COUNT: usize = Self::ALL.len();

            pub const fn as_str(self) -> &'static str {
                match self { $($name::$variant => $s,)* }
            }

            /// Dense index for array-backed stores.
            pub const fn index(self) -> usize {
                self as usize
            }

            pub fn from_index(i: usize) -> Option<Self> {
                Self::ALL.get(i).copied()
            }
        }

        impl FromStr for $name {
            type Err = ();
            fn from_str(s: &str) -> Result<Self, ()> {
                match s { $($s => Ok($name::$variant),)* _ => Err(()) }
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Json for $name {
            fn json(&self) -> String {
                self.as_str().json()
            }
        }
    };
}

string_enum! {
    /// Every resource in `RESOURCES_ALL` order. Intershard resources (`token`, `pixel`, ...) are
    /// account-level and not part of this set.
    #[allow(clippy::upper_case_acronyms)]
    ResourceType {
        Energy = "energy",
        Power = "power",
        H = "H",
        O = "O",
        U = "U",
        K = "K",
        L = "L",
        Z = "Z",
        X = "X",
        G = "G",
        OH = "OH",
        ZK = "ZK",
        UL = "UL",
        UH = "UH",
        UO = "UO",
        KH = "KH",
        KO = "KO",
        LH = "LH",
        LO = "LO",
        ZH = "ZH",
        ZO = "ZO",
        GH = "GH",
        GO = "GO",
        UH2O = "UH2O",
        UHO2 = "UHO2",
        KH2O = "KH2O",
        KHO2 = "KHO2",
        LH2O = "LH2O",
        LHO2 = "LHO2",
        ZH2O = "ZH2O",
        ZHO2 = "ZHO2",
        GH2O = "GH2O",
        GHO2 = "GHO2",
        XUH2O = "XUH2O",
        XUHO2 = "XUHO2",
        XKH2O = "XKH2O",
        XKHO2 = "XKHO2",
        XLH2O = "XLH2O",
        XLHO2 = "XLHO2",
        XZH2O = "XZH2O",
        XZHO2 = "XZHO2",
        XGH2O = "XGH2O",
        XGHO2 = "XGHO2",
        Ops = "ops",
        Silicon = "silicon",
        Metal = "metal",
        Biomass = "biomass",
        Mist = "mist",
        UtriumBar = "utrium_bar",
        LemergiumBar = "lemergium_bar",
        ZynthiumBar = "zynthium_bar",
        KeaniumBar = "keanium_bar",
        GhodiumMelt = "ghodium_melt",
        Oxidant = "oxidant",
        Reductant = "reductant",
        Purifier = "purifier",
        Battery = "battery",
        Composite = "composite",
        Crystal = "crystal",
        Liquid = "liquid",
        Wire = "wire",
        Switch = "switch",
        Transistor = "transistor",
        Microchip = "microchip",
        Circuit = "circuit",
        Device = "device",
        Cell = "cell",
        Phlegm = "phlegm",
        Tissue = "tissue",
        Muscle = "muscle",
        Organoid = "organoid",
        Organism = "organism",
        Alloy = "alloy",
        Tube = "tube",
        Fixtures = "fixtures",
        Frame = "frame",
        Hydraulics = "hydraulics",
        Machine = "machine",
        Condensate = "condensate",
        Concentrate = "concentrate",
        Extract = "extract",
        Spirit = "spirit",
        Emanation = "emanation",
        Essence = "essence",
    }
}

string_enum! {
    /// Every `STRUCTURE_*` value in declaration order.
    StructureType {
        Spawn = "spawn",
        Extension = "extension",
        Road = "road",
        Wall = "constructedWall",
        Rampart = "rampart",
        KeeperLair = "keeperLair",
        Portal = "portal",
        Controller = "controller",
        Link = "link",
        Storage = "storage",
        Tower = "tower",
        Observer = "observer",
        PowerBank = "powerBank",
        PowerSpawn = "powerSpawn",
        Extractor = "extractor",
        Lab = "lab",
        Terminal = "terminal",
        Container = "container",
        Nuker = "nuker",
        Factory = "factory",
        InvaderCore = "invaderCore",
    }
}

string_enum! {
    /// Body parts in `BODYPARTS_ALL` order.
    BodyPart {
        Move = "move",
        Work = "work",
        Carry = "carry",
        Attack = "attack",
        RangedAttack = "ranged_attack",
        Tough = "tough",
        Heal = "heal",
        Claim = "claim",
    }
}

/// An integer-keyed JS object with keys `first..first + N`, as an array indexed by key.
const fn keys_from<T: Copy, const N: usize, const M: usize>(first: usize, v: [T; N]) -> [Option<T>; M] {
    let mut out = [None; M];
    let mut i = 0;
    while i < N {
        out[first + i] = Some(v[i]);
        i += 1;
    }
    out
}

/// Indexes a `(key, value)` table by `key as usize` at compile time.
macro_rules! by_key {
    ($len:expr, $table:expr) => {{
        let mut out = [None; $len];
        let mut i = 0;
        while i < $table.len() {
            out[$table[i].0 as usize] = Some($table[i].1);
            i += 1;
        }
        out
    }};
}

pub const OK: i32 = 0;
pub const ERR_NOT_OWNER: i32 = -1;
pub const ERR_NO_PATH: i32 = -2;
pub const ERR_NAME_EXISTS: i32 = -3;
pub const ERR_BUSY: i32 = -4;
pub const ERR_NOT_FOUND: i32 = -5;
pub const ERR_NOT_ENOUGH_ENERGY: i32 = -6;
pub const ERR_NOT_ENOUGH_RESOURCES: i32 = -6;
pub const ERR_INVALID_TARGET: i32 = -7;
pub const ERR_FULL: i32 = -8;
pub const ERR_NOT_IN_RANGE: i32 = -9;
pub const ERR_INVALID_ARGS: i32 = -10;
pub const ERR_TIRED: i32 = -11;
pub const ERR_NO_BODYPART: i32 = -12;
pub const ERR_NOT_ENOUGH_EXTENSIONS: i32 = -6;
pub const ERR_RCL_NOT_ENOUGH: i32 = -14;
pub const ERR_GCL_NOT_ENOUGH: i32 = -15;
pub const ERR_ACCESS_DENIED: i32 = -16;

pub const FIND_EXIT_TOP: u32 = 1;
pub const FIND_EXIT_RIGHT: u32 = 3;
pub const FIND_EXIT_BOTTOM: u32 = 5;
pub const FIND_EXIT_LEFT: u32 = 7;
pub const FIND_EXIT: u32 = 10;
pub const FIND_CREEPS: u32 = 101;
pub const FIND_MY_CREEPS: u32 = 102;
pub const FIND_HOSTILE_CREEPS: u32 = 103;
pub const FIND_SOURCES_ACTIVE: u32 = 104;
pub const FIND_SOURCES: u32 = 105;
pub const FIND_DROPPED_RESOURCES: u32 = 106;
pub const FIND_STRUCTURES: u32 = 107;
pub const FIND_MY_STRUCTURES: u32 = 108;
pub const FIND_HOSTILE_STRUCTURES: u32 = 109;
pub const FIND_FLAGS: u32 = 110;
pub const FIND_CONSTRUCTION_SITES: u32 = 111;
pub const FIND_MY_SPAWNS: u32 = 112;
pub const FIND_HOSTILE_SPAWNS: u32 = 113;
pub const FIND_MY_CONSTRUCTION_SITES: u32 = 114;
pub const FIND_HOSTILE_CONSTRUCTION_SITES: u32 = 115;
pub const FIND_MINERALS: u32 = 116;
pub const FIND_NUKES: u32 = 117;
pub const FIND_TOMBSTONES: u32 = 118;
pub const FIND_POWER_CREEPS: u32 = 119;
pub const FIND_MY_POWER_CREEPS: u32 = 120;
pub const FIND_HOSTILE_POWER_CREEPS: u32 = 121;
pub const FIND_DEPOSITS: u32 = 122;
pub const FIND_RUINS: u32 = 123;

pub const TOP: u32 = 1;
pub const TOP_RIGHT: u32 = 2;
pub const RIGHT: u32 = 3;
pub const BOTTOM_RIGHT: u32 = 4;
pub const BOTTOM: u32 = 5;
pub const BOTTOM_LEFT: u32 = 6;
pub const LEFT: u32 = 7;
pub const TOP_LEFT: u32 = 8;

pub const COLOR_RED: u32 = 1;
pub const COLOR_PURPLE: u32 = 2;
pub const COLOR_BLUE: u32 = 3;
pub const COLOR_CYAN: u32 = 4;
pub const COLOR_GREEN: u32 = 5;
pub const COLOR_YELLOW: u32 = 6;
pub const COLOR_ORANGE: u32 = 7;
pub const COLOR_BROWN: u32 = 8;
pub const COLOR_GREY: u32 = 9;
pub const COLOR_WHITE: u32 = 10;

pub const LOOK_CREEPS: &str = "creep";
pub const LOOK_ENERGY: &str = "energy";
pub const LOOK_RESOURCES: &str = "resource";
pub const LOOK_SOURCES: &str = "source";
pub const LOOK_MINERALS: &str = "mineral";
pub const LOOK_DEPOSITS: &str = "deposit";
pub const LOOK_STRUCTURES: &str = "structure";
pub const LOOK_FLAGS: &str = "flag";
pub const LOOK_CONSTRUCTION_SITES: &str = "constructionSite";
pub const LOOK_NUKES: &str = "nuke";
pub const LOOK_TERRAIN: &str = "terrain";
pub const LOOK_TOMBSTONES: &str = "tombstone";
pub const LOOK_POWER_CREEPS: &str = "powerCreep";
pub const LOOK_RUINS: &str = "ruin";

pub const OBSTACLE_OBJECT_TYPES: &[&str] = &[
    "spawn", "creep", "powerCreep", "source", "mineral", "deposit", "controller", "constructedWall",
    "extension", "link", "storage", "tower", "observer", "powerSpawn", "powerBank", "lab",
    "terminal", "nuker", "factory", "invaderCore",
];

pub const MOVE: &str = Move.as_str();
pub const WORK: &str = Work.as_str();
pub const CARRY: &str = Carry.as_str();
pub const ATTACK: &str = Attack.as_str();
pub const RANGED_ATTACK: &str = RangedAttack.as_str();
pub const TOUGH: &str = Tough.as_str();
pub const HEAL: &str = Heal.as_str();
pub const CLAIM: &str = Claim.as_str();

pub const BODYPART_COST: &[(BodyPart, u32)] = &[
    (Move, 50),
    (Work, 100),
    (Attack, 80),
    (Carry, 50),
    (Heal, 250),
    (RangedAttack, 150),
    (Tough, 10),
    (Claim, 600),
];

static BODYPART_COST_BY_PART: [Option<u32>; BodyPart::COUNT] = by_key!(BodyPart::COUNT, BODYPART_COST);

impl BodyPart {
    /// `BODYPART_COST[part]`.
    pub fn cost(self) -> u32 {
        BODYPART_COST_BY_PART[self as usize].unwrap_or(0)
    }
}

// WORLD_WIDTH and WORLD_HEIGHT are deprecated upstream in favour of Game.map.getWorldSize().
pub const WORLD_WIDTH: u32 = 202;
pub const WORLD_HEIGHT: u32 = 202;

pub const CREEP_LIFE_TIME: u32 = 1500;
pub const CREEP_CLAIM_LIFE_TIME: u32 = 600;
pub const CREEP_CORPSE_RATE: f64 = 0.2;
pub const CREEP_PART_MAX_ENERGY: u32 = 125;

pub const CARRY_CAPACITY: u32 = 50;
pub const HARVEST_POWER: u32 = 2;
pub const HARVEST_MINERAL_POWER: u32 = 1;
pub const HARVEST_DEPOSIT_POWER: u32 = 1;
pub const REPAIR_POWER: u32 = 100;
pub const DISMANTLE_POWER: u32 = 50;
pub const BUILD_POWER: u32 = 5;
pub const ATTACK_POWER: u32 = 30;
pub const UPGRADE_CONTROLLER_POWER: u32 = 1;
pub const RANGED_ATTACK_POWER: u32 = 10;
pub const HEAL_POWER: u32 = 12;
pub const RANGED_HEAL_POWER: u32 = 4;
pub const REPAIR_COST: f64 = 0.01;
pub const DISMANTLE_COST: f64 = 0.005;

pub const RAMPART_DECAY_AMOUNT: u32 = 300;
pub const RAMPART_DECAY_TIME: u32 = 100;
pub const RAMPART_HITS: u32 = 1;
pub const RAMPART_HITS_MAX: [Option<u32>; 9] =
    keys_from(2, [300000, 1000000, 3000000, 10000000, 30000000, 100000000, 300000000]);

pub const ENERGY_REGEN_TIME: u32 = 300;
pub const ENERGY_DECAY: u32 = 1000;

pub const SPAWN_HITS: u32 = 5000;
pub const SPAWN_ENERGY_START: u32 = 300;
pub const SPAWN_ENERGY_CAPACITY: u32 = 300;
pub const CREEP_SPAWN_TIME: u32 = 3;
pub const SPAWN_RENEW_RATIO: f64 = 1.2;

pub const SOURCE_ENERGY_CAPACITY: u32 = 3000;
pub const SOURCE_ENERGY_NEUTRAL_CAPACITY: u32 = 1500;
pub const SOURCE_ENERGY_KEEPER_CAPACITY: u32 = 4000;

pub const WALL_HITS: u32 = 1;
pub const WALL_HITS_MAX: u32 = 300000000;

pub const EXTENSION_HITS: u32 = 1000;
/// Indexed by RCL 0..=8.
pub const EXTENSION_ENERGY_CAPACITY: [u32; 9] = [50, 50, 50, 50, 50, 50, 50, 100, 200];

pub const ROAD_HITS: u32 = 5000;
pub const ROAD_WEAROUT: u32 = 1;
pub const ROAD_WEAROUT_POWER_CREEP: u32 = 100;
pub const ROAD_DECAY_AMOUNT: u32 = 100;
pub const ROAD_DECAY_TIME: u32 = 1000;

pub const LINK_HITS: u32 = 1000;
pub const LINK_HITS_MAX: u32 = 1000;
pub const LINK_CAPACITY: u32 = 800;
pub const LINK_COOLDOWN: u32 = 1;
pub const LINK_LOSS_RATIO: f64 = 0.03;

pub const STORAGE_CAPACITY: u32 = 1000000;
pub const STORAGE_HITS: u32 = 10000;

pub const STRUCTURE_SPAWN: &str = Spawn.as_str();
pub const STRUCTURE_EXTENSION: &str = Extension.as_str();
pub const STRUCTURE_ROAD: &str = Road.as_str();
pub const STRUCTURE_WALL: &str = Wall.as_str();
pub const STRUCTURE_RAMPART: &str = Rampart.as_str();
pub const STRUCTURE_KEEPER_LAIR: &str = KeeperLair.as_str();
pub const STRUCTURE_PORTAL: &str = Portal.as_str();
pub const STRUCTURE_CONTROLLER: &str = Controller.as_str();
pub const STRUCTURE_LINK: &str = Link.as_str();
pub const STRUCTURE_STORAGE: &str = Storage.as_str();
pub const STRUCTURE_TOWER: &str = Tower.as_str();
pub const STRUCTURE_OBSERVER: &str = Observer.as_str();
pub const STRUCTURE_POWER_BANK: &str = PowerBank.as_str();
pub const STRUCTURE_POWER_SPAWN: &str = PowerSpawn.as_str();
pub const STRUCTURE_EXTRACTOR: &str = Extractor.as_str();
pub const STRUCTURE_LAB: &str = Lab.as_str();
pub const STRUCTURE_TERMINAL: &str = Terminal.as_str();
pub const STRUCTURE_CONTAINER: &str = Container.as_str();
pub const STRUCTURE_NUKER: &str = Nuker.as_str();
pub const STRUCTURE_FACTORY: &str = Factory.as_str();
pub const STRUCTURE_INVADER_CORE: &str = InvaderCore.as_str();

pub const CONSTRUCTION_COST: &[(StructureType, u32)] = &[
    (Spawn, 15000),
    (Extension, 3000),
    (Road, 300),
    (Wall, 1),
    (Rampart, 1),
    (Link, 5000),
    (Storage, 30000),
    (Tower, 5000),
    (Observer, 8000),
    (PowerSpawn, 100000),
    (Extractor, 5000),
    (Lab, 50000),
    (Terminal, 100000),
    (Container, 5000),
    (Nuker, 100000),
    (Factory, 100000),
];
pub const CONSTRUCTION_COST_ROAD_SWAMP_RATIO: u32 = 5;
pub const CONSTRUCTION_COST_ROAD_WALL_RATIO: u32 = 150;

static CONSTRUCTION_COST_BY_TYPE: [Option<u32>; StructureType::COUNT] =
    by_key!(StructureType::COUNT, CONSTRUCTION_COST);

/// `CONSTRUCTION_COST[type]`; `None` for types that cannot be built.
pub fn construction_cost(t: StructureType) -> Option<u32> {
    CONSTRUCTION_COST_BY_TYPE[t as usize]
}

/// Indexed by RCL; keys 1..=7.
pub const CONTROLLER_LEVELS: [Option<u32>; 8] =
    keys_from(1, [200, 45000, 135000, 405000, 1215000, 3645000, 10935000]);

/// Per structure type, indexed by RCL. Some rows have no key 0, which JS reads as `undefined`.
pub const CONTROLLER_STRUCTURES: &[(StructureType, [Option<u32>; 9])] = &[
    (Spawn, keys_from(0, [0, 1, 1, 1, 1, 1, 1, 2, 3])),
    (Extension, keys_from(0, [0, 0, 5, 10, 20, 30, 40, 50, 60])),
    (Link, keys_from(1, [0, 0, 0, 0, 2, 3, 4, 6])),
    (Road, keys_from(0, [2500, 2500, 2500, 2500, 2500, 2500, 2500, 2500, 2500])),
    (Wall, keys_from(1, [0, 2500, 2500, 2500, 2500, 2500, 2500, 2500])),
    (Rampart, keys_from(1, [0, 2500, 2500, 2500, 2500, 2500, 2500, 2500])),
    (Storage, keys_from(1, [0, 0, 0, 1, 1, 1, 1, 1])),
    (Tower, keys_from(1, [0, 0, 1, 1, 2, 2, 3, 6])),
    (Observer, keys_from(1, [0, 0, 0, 0, 0, 0, 0, 1])),
    (PowerSpawn, keys_from(1, [0, 0, 0, 0, 0, 0, 0, 1])),
    (Extractor, keys_from(1, [0, 0, 0, 0, 0, 1, 1, 1])),
    (Terminal, keys_from(1, [0, 0, 0, 0, 0, 1, 1, 1])),
    (Lab, keys_from(1, [0, 0, 0, 0, 0, 3, 6, 10])),
    (Container, keys_from(0, [5, 5, 5, 5, 5, 5, 5, 5, 5])),
    (Nuker, keys_from(1, [0, 0, 0, 0, 0, 0, 0, 1])),
    (Factory, keys_from(1, [0, 0, 0, 0, 0, 0, 1, 1])),
];

static CONTROLLER_STRUCTURES_BY_TYPE: [Option<[Option<u32>; 9]>; StructureType::COUNT] =
    by_key!(StructureType::COUNT, CONTROLLER_STRUCTURES);

/// `CONTROLLER_STRUCTURES[type][rcl]`; `None` where JS yields `undefined`.
pub fn controller_structures(t: StructureType, rcl: usize) -> Option<u32> {
    CONTROLLER_STRUCTURES_BY_TYPE[t as usize]?.get(rcl).copied().flatten()
}

/// Indexed by RCL; keys 1..=8.
pub const CONTROLLER_DOWNGRADE: [Option<u32>; 9] =
    keys_from(1, [20000, 10000, 20000, 40000, 80000, 120000, 150000, 200000]);
pub const CONTROLLER_DOWNGRADE_RESTORE: u32 = 100;
pub const CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD: u32 = 5000;
pub const CONTROLLER_CLAIM_DOWNGRADE: u32 = 300;
pub const CONTROLLER_RESERVE: u32 = 1;
pub const CONTROLLER_RESERVE_MAX: u32 = 5000;
pub const CONTROLLER_MAX_UPGRADE_PER_TICK: u32 = 15;
pub const CONTROLLER_ATTACK_BLOCKED_UPGRADE: u32 = 1000;
pub const CONTROLLER_NUKE_BLOCKED_UPGRADE: u32 = 200;

pub const SAFE_MODE_DURATION: u32 = 20000;
pub const SAFE_MODE_COOLDOWN: u32 = 50000;
pub const SAFE_MODE_COST: u32 = 1000;

pub const TOWER_HITS: u32 = 3000;
pub const TOWER_CAPACITY: u32 = 1000;
pub const TOWER_ENERGY_COST: u32 = 10;
pub const TOWER_POWER_ATTACK: u32 = 600;
pub const TOWER_POWER_HEAL: u32 = 400;
pub const TOWER_POWER_REPAIR: u32 = 800;
pub const TOWER_OPTIMAL_RANGE: u32 = 5;
pub const TOWER_FALLOFF_RANGE: u32 = 20;
pub const TOWER_FALLOFF: f64 = 0.75;

pub const OBSERVER_HITS: u32 = 500;
pub const OBSERVER_RANGE: u32 = 10;

pub const POWER_BANK_HITS: u32 = 2000000;
pub const POWER_BANK_CAPACITY_MAX: u32 = 5000;
pub const POWER_BANK_CAPACITY_MIN: u32 = 500;
pub const POWER_BANK_CAPACITY_CRIT: f64 = 0.3;
pub const POWER_BANK_DECAY: u32 = 5000;
pub const POWER_BANK_HIT_BACK: f64 = 0.5;

pub const POWER_SPAWN_HITS: u32 = 5000;
pub const POWER_SPAWN_ENERGY_CAPACITY: u32 = 5000;
pub const POWER_SPAWN_POWER_CAPACITY: u32 = 100;
pub const POWER_SPAWN_ENERGY_RATIO: u32 = 50;

pub const EXTRACTOR_HITS: u32 = 500;
pub const EXTRACTOR_COOLDOWN: u32 = 5;

pub const LAB_HITS: u32 = 500;
pub const LAB_MINERAL_CAPACITY: u32 = 3000;
pub const LAB_ENERGY_CAPACITY: u32 = 2000;
pub const LAB_BOOST_ENERGY: u32 = 20;
pub const LAB_BOOST_MINERAL: u32 = 30;
/// Not used upstream either.
pub const LAB_COOLDOWN: u32 = 10;
pub const LAB_REACTION_AMOUNT: u32 = 5;
pub const LAB_UNBOOST_ENERGY: u32 = 0;
pub const LAB_UNBOOST_MINERAL: u32 = 15;

pub const GCL_POW: f64 = 2.4;
pub const GCL_MULTIPLY: u32 = 1000000;
pub const GCL_NOVICE: u32 = 3;

/// `null` upstream.
pub const MODE_SIMULATION: Option<&str> = None;
/// `null` upstream.
pub const MODE_WORLD: Option<&str> = None;

pub const TERRAIN_MASK_WALL: u8 = 1;
pub const TERRAIN_MASK_SWAMP: u8 = 2;
pub const TERRAIN_MASK_LAVA: u8 = 4;

pub const MAX_CONSTRUCTION_SITES: u32 = 100;
pub const MAX_CREEP_SIZE: u32 = 50;

pub const MINERAL_REGEN_TIME: u32 = 50000;
pub const MINERAL_MIN_AMOUNT: &[(ResourceType, u32)] = &[
    (H, 35000),
    (O, 35000),
    (L, 35000),
    (K, 35000),
    (Z, 35000),
    (U, 35000),
    (X, 35000),
];
pub const MINERAL_RANDOM_FACTOR: u32 = 2;

/// Indexed by density 1..=4.
pub const MINERAL_DENSITY: [Option<u32>; 5] = keys_from(1, [15000, 35000, 70000, 100000]);
/// Indexed by density 1..=4.
pub const MINERAL_DENSITY_PROBABILITY: [Option<f64>; 5] = keys_from(1, [0.1, 0.5, 0.9, 1.0]);
pub const MINERAL_DENSITY_CHANGE: f64 = 0.05;

pub const DENSITY_LOW: u32 = 1;
pub const DENSITY_MODERATE: u32 = 2;
pub const DENSITY_HIGH: u32 = 3;
pub const DENSITY_ULTRA: u32 = 4;

pub const DEPOSIT_EXHAUST_MULTIPLY: f64 = 0.001;
pub const DEPOSIT_EXHAUST_POW: f64 = 1.2;
pub const DEPOSIT_DECAY_TIME: u32 = 50000;

pub const TERMINAL_CAPACITY: u32 = 300000;
pub const TERMINAL_HITS: u32 = 3000;
pub const TERMINAL_SEND_COST: f64 = 0.1;
pub const TERMINAL_MIN_SEND: u32 = 100;
pub const TERMINAL_COOLDOWN: u32 = 10;

pub const CONTAINER_HITS: u32 = 250000;
pub const CONTAINER_CAPACITY: u32 = 2000;
pub const CONTAINER_DECAY: u32 = 5000;
pub const CONTAINER_DECAY_TIME: u32 = 100;
pub const CONTAINER_DECAY_TIME_OWNED: u32 = 500;

pub const NUKER_HITS: u32 = 1000;
pub const NUKER_COOLDOWN: u32 = 100000;
pub const NUKER_ENERGY_CAPACITY: u32 = 300000;
pub const NUKER_GHODIUM_CAPACITY: u32 = 5000;
pub const NUKE_LAND_TIME: u32 = 50000;
pub const NUKE_RANGE: u32 = 10;
/// Indexed by range from the landing tile; keys 0 and 2.
pub const NUKE_DAMAGE: [Option<u32>; 3] = [Some(10000000), None, Some(5000000)];

pub const FACTORY_HITS: u32 = 1000;
pub const FACTORY_CAPACITY: u32 = 50000;

pub const TOMBSTONE_DECAY_PER_PART: u32 = 5;
pub const TOMBSTONE_DECAY_POWER_CREEP: u32 = 500;

pub const RUIN_DECAY: u32 = 500;
pub const RUIN_DECAY_STRUCTURES: &[(StructureType, u32)] = &[(PowerBank, 10)];

pub const PORTAL_DECAY: u32 = 30000;

pub const ORDER_SELL: &str = "sell";
pub const ORDER_BUY: &str = "buy";

pub const MARKET_FEE: f64 = 0.05;

pub const MARKET_MAX_ORDERS: u32 = 300;
pub const MARKET_ORDER_LIFE_TIME: u64 = 1000 * 60 * 60 * 24 * 30;

pub const FLAGS_LIMIT: u32 = 10000;

pub const SUBSCRIPTION_TOKEN: &str = "token";
pub const CPU_UNLOCK: &str = "cpuUnlock";
pub const PIXEL: &str = "pixel";
pub const ACCESS_KEY: &str = "accessKey";

pub const PIXEL_CPU_COST: u32 = 10000;

pub const RESOURCE_ENERGY: &str = Energy.as_str();
pub const RESOURCE_POWER: &str = Power.as_str();

pub const RESOURCE_HYDROGEN: &str = H.as_str();
pub const RESOURCE_OXYGEN: &str = O.as_str();
pub const RESOURCE_UTRIUM: &str = U.as_str();
pub const RESOURCE_LEMERGIUM: &str = L.as_str();
pub const RESOURCE_KEANIUM: &str = K.as_str();
pub const RESOURCE_ZYNTHIUM: &str = Z.as_str();
pub const RESOURCE_CATALYST: &str = X.as_str();
pub const RESOURCE_GHODIUM: &str = G.as_str();

pub const RESOURCE_SILICON: &str = Silicon.as_str();
pub const RESOURCE_METAL: &str = Metal.as_str();
pub const RESOURCE_BIOMASS: &str = Biomass.as_str();
pub const RESOURCE_MIST: &str = Mist.as_str();

pub const RESOURCE_HYDROXIDE: &str = OH.as_str();
pub const RESOURCE_ZYNTHIUM_KEANITE: &str = ZK.as_str();
pub const RESOURCE_UTRIUM_LEMERGITE: &str = UL.as_str();

pub const RESOURCE_UTRIUM_HYDRIDE: &str = UH.as_str();
pub const RESOURCE_UTRIUM_OXIDE: &str = UO.as_str();
pub const RESOURCE_KEANIUM_HYDRIDE: &str = KH.as_str();
pub const RESOURCE_KEANIUM_OXIDE: &str = KO.as_str();
pub const RESOURCE_LEMERGIUM_HYDRIDE: &str = LH.as_str();
pub const RESOURCE_LEMERGIUM_OXIDE: &str = LO.as_str();
pub const RESOURCE_ZYNTHIUM_HYDRIDE: &str = ZH.as_str();
pub const RESOURCE_ZYNTHIUM_OXIDE: &str = ZO.as_str();
pub const RESOURCE_GHODIUM_HYDRIDE: &str = GH.as_str();
pub const RESOURCE_GHODIUM_OXIDE: &str = GO.as_str();

pub const RESOURCE_UTRIUM_ACID: &str = UH2O.as_str();
pub const RESOURCE_UTRIUM_ALKALIDE: &str = UHO2.as_str();
pub const RESOURCE_KEANIUM_ACID: &str = KH2O.as_str();
pub const RESOURCE_KEANIUM_ALKALIDE: &str = KHO2.as_str();
pub const RESOURCE_LEMERGIUM_ACID: &str = LH2O.as_str();
pub const RESOURCE_LEMERGIUM_ALKALIDE: &str = LHO2.as_str();
pub const RESOURCE_ZYNTHIUM_ACID: &str = ZH2O.as_str();
pub const RESOURCE_ZYNTHIUM_ALKALIDE: &str = ZHO2.as_str();
pub const RESOURCE_GHODIUM_ACID: &str = GH2O.as_str();
pub const RESOURCE_GHODIUM_ALKALIDE: &str = GHO2.as_str();

pub const RESOURCE_CATALYZED_UTRIUM_ACID: &str = XUH2O.as_str();
pub const RESOURCE_CATALYZED_UTRIUM_ALKALIDE: &str = XUHO2.as_str();
pub const RESOURCE_CATALYZED_KEANIUM_ACID: &str = XKH2O.as_str();
pub const RESOURCE_CATALYZED_KEANIUM_ALKALIDE: &str = XKHO2.as_str();
pub const RESOURCE_CATALYZED_LEMERGIUM_ACID: &str = XLH2O.as_str();
pub const RESOURCE_CATALYZED_LEMERGIUM_ALKALIDE: &str = XLHO2.as_str();
pub const RESOURCE_CATALYZED_ZYNTHIUM_ACID: &str = XZH2O.as_str();
pub const RESOURCE_CATALYZED_ZYNTHIUM_ALKALIDE: &str = XZHO2.as_str();
pub const RESOURCE_CATALYZED_GHODIUM_ACID: &str = XGH2O.as_str();
pub const RESOURCE_CATALYZED_GHODIUM_ALKALIDE: &str = XGHO2.as_str();

pub const RESOURCE_OPS: &str = Ops.as_str();

pub const RESOURCE_UTRIUM_BAR: &str = UtriumBar.as_str();
pub const RESOURCE_LEMERGIUM_BAR: &str = LemergiumBar.as_str();
pub const RESOURCE_ZYNTHIUM_BAR: &str = ZynthiumBar.as_str();
pub const RESOURCE_KEANIUM_BAR: &str = KeaniumBar.as_str();
pub const RESOURCE_GHODIUM_MELT: &str = GhodiumMelt.as_str();
pub const RESOURCE_OXIDANT: &str = Oxidant.as_str();
pub const RESOURCE_REDUCTANT: &str = Reductant.as_str();
pub const RESOURCE_PURIFIER: &str = Purifier.as_str();
pub const RESOURCE_BATTERY: &str = Battery.as_str();

pub const RESOURCE_COMPOSITE: &str = Composite.as_str();
pub const RESOURCE_CRYSTAL: &str = Crystal.as_str();
pub const RESOURCE_LIQUID: &str = Liquid.as_str();

pub const RESOURCE_WIRE: &str = Wire.as_str();
pub const RESOURCE_SWITCH: &str = Switch.as_str();
pub const RESOURCE_TRANSISTOR: &str = Transistor.as_str();
pub const RESOURCE_MICROCHIP: &str = Microchip.as_str();
pub const RESOURCE_CIRCUIT: &str = Circuit.as_str();
pub const RESOURCE_DEVICE: &str = Device.as_str();

pub const RESOURCE_CELL: &str = Cell.as_str();
pub const RESOURCE_PHLEGM: &str = Phlegm.as_str();
pub const RESOURCE_TISSUE: &str = Tissue.as_str();
pub const RESOURCE_MUSCLE: &str = Muscle.as_str();
pub const RESOURCE_ORGANOID: &str = Organoid.as_str();
pub const RESOURCE_ORGANISM: &str = Organism.as_str();

pub const RESOURCE_ALLOY: &str = Alloy.as_str();
pub const RESOURCE_TUBE: &str = Tube.as_str();
pub const RESOURCE_FIXTURES: &str = Fixtures.as_str();
pub const RESOURCE_FRAME: &str = Frame.as_str();
pub const RESOURCE_HYDRAULICS: &str = Hydraulics.as_str();
pub const RESOURCE_MACHINE: &str = Machine.as_str();

pub const RESOURCE_CONDENSATE: &str = Condensate.as_str();
pub const RESOURCE_CONCENTRATE: &str = Concentrate.as_str();
pub const RESOURCE_EXTRACT: &str = Extract.as_str();
pub const RESOURCE_SPIRIT: &str = Spirit.as_str();
pub const RESOURCE_EMANATION: &str = Emanation.as_str();
pub const RESOURCE_ESSENCE: &str = Essence.as_str();

/// `REACTIONS[a][b]` is the compound labs make from `a` and `b`.
pub const REACTIONS: &[(ResourceType, &[(ResourceType, ResourceType)])] = &[
    (H, &[(O, OH), (L, LH), (K, KH), (U, UH), (Z, ZH), (G, GH)]),
    (O, &[(H, OH), (L, LO), (K, KO), (U, UO), (Z, ZO), (G, GO)]),
    (Z, &[(K, ZK), (H, ZH), (O, ZO)]),
    (L, &[(U, UL), (H, LH), (O, LO)]),
    (K, &[(Z, ZK), (H, KH), (O, KO)]),
    (G, &[(H, GH), (O, GO)]),
    (U, &[(L, UL), (H, UH), (O, UO)]),
    (
        OH,
        &[
            (UH, UH2O),
            (UO, UHO2),
            (ZH, ZH2O),
            (ZO, ZHO2),
            (KH, KH2O),
            (KO, KHO2),
            (LH, LH2O),
            (LO, LHO2),
            (GH, GH2O),
            (GO, GHO2),
        ],
    ),
    (
        X,
        &[
            (UH2O, XUH2O),
            (UHO2, XUHO2),
            (LH2O, XLH2O),
            (LHO2, XLHO2),
            (KH2O, XKH2O),
            (KHO2, XKHO2),
            (ZH2O, XZH2O),
            (ZHO2, XZHO2),
            (GH2O, XGH2O),
            (GHO2, XGHO2),
        ],
    ),
    (ZK, &[(UL, G)]),
    (UL, &[(ZK, G)]),
    (LH, &[(OH, LH2O)]),
    (ZH, &[(OH, ZH2O)]),
    (GH, &[(OH, GH2O)]),
    (KH, &[(OH, KH2O)]),
    (UH, &[(OH, UH2O)]),
    (LO, &[(OH, LHO2)]),
    (ZO, &[(OH, ZHO2)]),
    (KO, &[(OH, KHO2)]),
    (UO, &[(OH, UHO2)]),
    (GO, &[(OH, GHO2)]),
    (LH2O, &[(X, XLH2O)]),
    (KH2O, &[(X, XKH2O)]),
    (ZH2O, &[(X, XZH2O)]),
    (UH2O, &[(X, XUH2O)]),
    (GH2O, &[(X, XGH2O)]),
    (LHO2, &[(X, XLHO2)]),
    (UHO2, &[(X, XUHO2)]),
    (KHO2, &[(X, XKHO2)]),
    (ZHO2, &[(X, XZHO2)]),
    (GHO2, &[(X, XGHO2)]),
];

static REACTIONS_BY_PAIR: [[Option<ResourceType>; ResourceType::COUNT]; ResourceType::COUNT] = {
    let mut out = [[None; ResourceType::COUNT]; ResourceType::COUNT];
    let mut i = 0;
    while i < REACTIONS.len() {
        let (a, row) = REACTIONS[i];
        let mut j = 0;
        while j < row.len() {
            let (b, product) = row[j];
            out[a as usize][b as usize] = Some(product);
            j += 1;
        }
        i += 1;
    }
    out
};

/// `REACTIONS[a][b]`.
pub fn reaction(a: ResourceType, b: ResourceType) -> Option<ResourceType> {
    REACTIONS_BY_PAIR[a as usize][b as usize]
}

/// `(action, multiplier)` pairs of one boost compound.
pub type BoostEffects = &'static [(&'static str, f64)];

/// `BOOSTS[part][resource][action]` is the multiplier for that action.
pub const BOOSTS: &[(BodyPart, &[(ResourceType, BoostEffects)])] = &[
    (
        Work,
        &[
            (UO, &[("harvest", 3.0)]),
            (UHO2, &[("harvest", 5.0)]),
            (XUHO2, &[("harvest", 7.0)]),
            (LH, &[("build", 1.5), ("repair", 1.5)]),
            (LH2O, &[("build", 1.8), ("repair", 1.8)]),
            (XLH2O, &[("build", 2.0), ("repair", 2.0)]),
            (ZH, &[("dismantle", 2.0)]),
            (ZH2O, &[("dismantle", 3.0)]),
            (XZH2O, &[("dismantle", 4.0)]),
            (GH, &[("upgradeController", 1.5)]),
            (GH2O, &[("upgradeController", 1.8)]),
            (XGH2O, &[("upgradeController", 2.0)]),
        ],
    ),
    (Attack, &[(UH, &[("attack", 2.0)]), (UH2O, &[("attack", 3.0)]), (XUH2O, &[("attack", 4.0)])]),
    (
        RangedAttack,
        &[
            (KO, &[("rangedAttack", 2.0), ("rangedMassAttack", 2.0)]),
            (KHO2, &[("rangedAttack", 3.0), ("rangedMassAttack", 3.0)]),
            (XKHO2, &[("rangedAttack", 4.0), ("rangedMassAttack", 4.0)]),
        ],
    ),
    (
        Heal,
        &[
            (LO, &[("heal", 2.0), ("rangedHeal", 2.0)]),
            (LHO2, &[("heal", 3.0), ("rangedHeal", 3.0)]),
            (XLHO2, &[("heal", 4.0), ("rangedHeal", 4.0)]),
        ],
    ),
    (Carry, &[(KH, &[("capacity", 2.0)]), (KH2O, &[("capacity", 3.0)]), (XKH2O, &[("capacity", 4.0)])]),
    (Move, &[(ZO, &[("fatigue", 2.0)]), (ZHO2, &[("fatigue", 3.0)]), (XZHO2, &[("fatigue", 4.0)])]),
    (Tough, &[(GO, &[("damage", 0.7)]), (GHO2, &[("damage", 0.5)]), (XGHO2, &[("damage", 0.3)])]),
];

// Every compound boosts exactly one part type, so the table can be keyed by resource alone.
static BOOSTS_BY_RESOURCE: [Option<(BodyPart, BoostEffects)>; ResourceType::COUNT] = {
    let mut out = [None; ResourceType::COUNT];
    let mut i = 0;
    while i < BOOSTS.len() {
        let (part, row) = BOOSTS[i];
        let mut j = 0;
        while j < row.len() {
            let (resource, effects) = row[j];
            out[resource as usize] = Some((part, effects));
            j += 1;
        }
        i += 1;
    }
    out
};

/// `BOOSTS[part][resource][action]`.
// ponytail: actions are matched as strings (at most two per boost); intern them as an enum if
// boost lookups show up in profiles.
pub fn boost(part: BodyPart, resource: ResourceType, action: &str) -> Option<f64> {
    let (p, effects) = BOOSTS_BY_RESOURCE[resource as usize]?;
    if p != part {
        return None;
    }
    effects.iter().find(|e| e.0 == action).map(|e| e.1)
}

pub const REACTION_TIME: &[(ResourceType, u32)] = &[
    (OH, 20),
    (ZK, 5),
    (UL, 5),
    (G, 5),
    (UH, 10),
    (UH2O, 5),
    (XUH2O, 60),
    (UO, 10),
    (UHO2, 5),
    (XUHO2, 60),
    (KH, 10),
    (KH2O, 5),
    (XKH2O, 60),
    (KO, 10),
    (KHO2, 5),
    (XKHO2, 60),
    (LH, 15),
    (LH2O, 10),
    (XLH2O, 65),
    (LO, 10),
    (LHO2, 5),
    (XLHO2, 60),
    (ZH, 20),
    (ZH2O, 40),
    (XZH2O, 160),
    (ZO, 10),
    (ZHO2, 5),
    (XZHO2, 60),
    (GH, 10),
    (GH2O, 15),
    (XGH2O, 80),
    (GO, 10),
    (GHO2, 30),
    (XGHO2, 150),
];

static REACTION_TIME_BY_RESOURCE: [Option<u32>; ResourceType::COUNT] =
    by_key!(ResourceType::COUNT, REACTION_TIME);

/// `REACTION_TIME[compound]`.
pub fn reaction_time(r: ResourceType) -> Option<u32> {
    REACTION_TIME_BY_RESOURCE[r as usize]
}

pub const PORTAL_UNSTABLE: u64 = 10 * 24 * 3600 * 1000;
pub const PORTAL_MIN_TIMEOUT: u64 = 12 * 24 * 3600 * 1000;
pub const PORTAL_MAX_TIMEOUT: u64 = 22 * 24 * 3600 * 1000;

pub const POWER_BANK_RESPAWN_TIME: u32 = 50000;

pub const INVADERS_ENERGY_GOAL: u32 = 100000;

pub const SYSTEM_USERNAME: &str = "Screeps";

// SIGN_NOVICE_AREA and SIGN_RESPAWN_AREA are deprecated upstream in favour of SIGN_PLANNED_AREA.
pub const SIGN_NOVICE_AREA: &str = "A new Novice or Respawn Area is being planned somewhere in this sector. Please make sure all important rooms are reserved.";
pub const SIGN_RESPAWN_AREA: &str = "A new Novice or Respawn Area is being planned somewhere in this sector. Please make sure all important rooms are reserved.";
pub const SIGN_PLANNED_AREA: &str = "A new Novice or Respawn Area is being planned somewhere in this sector. Please make sure all important rooms are reserved.";

pub const EVENT_ATTACK: u32 = 1;
pub const EVENT_OBJECT_DESTROYED: u32 = 2;
pub const EVENT_ATTACK_CONTROLLER: u32 = 3;
pub const EVENT_BUILD: u32 = 4;
pub const EVENT_HARVEST: u32 = 5;
pub const EVENT_HEAL: u32 = 6;
pub const EVENT_REPAIR: u32 = 7;
pub const EVENT_RESERVE_CONTROLLER: u32 = 8;
pub const EVENT_UPGRADE_CONTROLLER: u32 = 9;
pub const EVENT_EXIT: u32 = 10;
pub const EVENT_POWER: u32 = 11;
pub const EVENT_TRANSFER: u32 = 12;

pub const EVENT_ATTACK_TYPE_MELEE: u32 = 1;
pub const EVENT_ATTACK_TYPE_RANGED: u32 = 2;
pub const EVENT_ATTACK_TYPE_RANGED_MASS: u32 = 3;
pub const EVENT_ATTACK_TYPE_DISMANTLE: u32 = 4;
pub const EVENT_ATTACK_TYPE_HIT_BACK: u32 = 5;
pub const EVENT_ATTACK_TYPE_NUKE: u32 = 6;

pub const EVENT_HEAL_TYPE_MELEE: u32 = 1;
pub const EVENT_HEAL_TYPE_RANGED: u32 = 2;

pub const POWER_LEVEL_MULTIPLY: u32 = 1000;
pub const POWER_LEVEL_POW: u32 = 2;
pub const POWER_CREEP_SPAWN_COOLDOWN: u64 = 8 * 3600 * 1000;
pub const POWER_CREEP_DELETE_COOLDOWN: u64 = 24 * 3600 * 1000;
pub const POWER_CREEP_MAX_LEVEL: u32 = 25;
pub const POWER_CREEP_LIFE_TIME: u32 = 5000;

/// `POWER_CLASS.OPERATOR`.
pub const POWER_CLASS_OPERATOR: &str = "operator";

pub const PWR_GENERATE_OPS: u32 = 1;
pub const PWR_OPERATE_SPAWN: u32 = 2;
pub const PWR_OPERATE_TOWER: u32 = 3;
pub const PWR_OPERATE_STORAGE: u32 = 4;
pub const PWR_OPERATE_LAB: u32 = 5;
pub const PWR_OPERATE_EXTENSION: u32 = 6;
pub const PWR_OPERATE_OBSERVER: u32 = 7;
pub const PWR_OPERATE_TERMINAL: u32 = 8;
pub const PWR_DISRUPT_SPAWN: u32 = 9;
pub const PWR_DISRUPT_TOWER: u32 = 10;
pub const PWR_DISRUPT_SOURCE: u32 = 11;
pub const PWR_SHIELD: u32 = 12;
pub const PWR_REGEN_SOURCE: u32 = 13;
pub const PWR_REGEN_MINERAL: u32 = 14;
pub const PWR_DISRUPT_TERMINAL: u32 = 15;
pub const PWR_OPERATE_POWER: u32 = 16;
pub const PWR_FORTIFY: u32 = 17;
pub const PWR_OPERATE_CONTROLLER: u32 = 18;
pub const PWR_OPERATE_FACTORY: u32 = 19;

pub const EFFECT_INVULNERABILITY: u32 = 1001;
pub const EFFECT_COLLAPSE_TIMER: u32 = 1002;

pub const INVADER_CORE_HITS: u32 = 100000;
/// Indexed by core level 0..=5.
pub const INVADER_CORE_CREEP_SPAWN_TIME: [u32; 6] = [0, 0, 6, 3, 2, 1];
/// Indexed by core level; keys 1..=5.
pub const INVADER_CORE_EXPAND_TIME: [Option<u32>; 6] = keys_from(1, [4000, 3500, 3000, 2500, 2000]);
pub const INVADER_CORE_CONTROLLER_POWER: u32 = 2;
pub const INVADER_CORE_CONTROLLER_DOWNGRADE: u32 = 5000;
/// Indexed by stronghold level 0..=5.
pub const STRONGHOLD_RAMPART_HITS: [u32; 6] = [0, 100000, 200000, 500000, 1000000, 2000000];
pub const STRONGHOLD_DECAY_TICKS: u32 = 75000;

/// `duration` and `ops` are a single number for some powers and per-level for others.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PerLevel {
    Fixed(u32),
    Levels([u32; 5]),
}

impl PerLevel {
    /// The value at power level `level` (0-based).
    pub fn at(self, level: usize) -> u32 {
        match self {
            PerLevel::Fixed(v) => v,
            PerLevel::Levels(v) => v[level],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerInfo {
    pub power: u32,
    pub class_name: &'static str,
    pub level: [u32; 5],
    pub cooldown: u32,
    pub duration: Option<PerLevel>,
    pub range: Option<u32>,
    pub ops: Option<PerLevel>,
    pub effect: Option<[f64; 5]>,
    pub energy: Option<u32>,
    pub period: Option<u32>,
    /// Key order of this entry in constants.js. It varies between powers and player code sees
    /// it, so `constants_json` follows it.
    pub keys: &'static [&'static str],
}

const OPERATOR: PowerInfo = PowerInfo {
    power: 0,
    class_name: POWER_CLASS_OPERATOR,
    level: [0, 2, 7, 14, 22],
    cooldown: 0,
    duration: None,
    range: None,
    ops: None,
    effect: None,
    energy: None,
    period: None,
    keys: &[],
};

/// `POWER_INFO`, indexed by `PWR_*` - 1. See `power_info`.
pub const POWER_INFO: &[PowerInfo] = &[
    PowerInfo {
        power: PWR_GENERATE_OPS,
        cooldown: 50,
        effect: Some([1.0, 2.0, 4.0, 6.0, 8.0]),
        keys: &["className", "level", "cooldown", "effect"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_OPERATE_SPAWN,
        cooldown: 300,
        duration: Some(Fixed(1000)),
        range: Some(3),
        ops: Some(Fixed(100)),
        effect: Some([0.9, 0.7, 0.5, 0.35, 0.2]),
        keys: &["className", "level", "cooldown", "duration", "range", "ops", "effect"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_OPERATE_TOWER,
        cooldown: 10,
        duration: Some(Fixed(100)),
        range: Some(3),
        ops: Some(Fixed(10)),
        effect: Some([1.1, 1.2, 1.3, 1.4, 1.5]),
        keys: &["className", "level", "cooldown", "duration", "range", "ops", "effect"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_OPERATE_STORAGE,
        cooldown: 800,
        duration: Some(Fixed(1000)),
        range: Some(3),
        ops: Some(Fixed(100)),
        effect: Some([500000.0, 1000000.0, 2000000.0, 4000000.0, 7000000.0]),
        keys: &["className", "level", "cooldown", "duration", "range", "ops", "effect"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_OPERATE_LAB,
        cooldown: 50,
        duration: Some(Fixed(1000)),
        range: Some(3),
        ops: Some(Fixed(10)),
        effect: Some([2.0, 4.0, 6.0, 8.0, 10.0]),
        keys: &["className", "level", "cooldown", "duration", "range", "ops", "effect"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_OPERATE_EXTENSION,
        cooldown: 50,
        range: Some(3),
        ops: Some(Fixed(2)),
        effect: Some([0.2, 0.4, 0.6, 0.8, 1.0]),
        keys: &["className", "level", "cooldown", "range", "ops", "effect"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_OPERATE_OBSERVER,
        cooldown: 400,
        duration: Some(Levels([200, 400, 600, 800, 1000])),
        range: Some(3),
        ops: Some(Fixed(10)),
        keys: &["className", "level", "cooldown", "duration", "range", "ops"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_OPERATE_TERMINAL,
        cooldown: 500,
        duration: Some(Fixed(1000)),
        range: Some(3),
        ops: Some(Fixed(100)),
        effect: Some([0.9, 0.8, 0.7, 0.6, 0.5]),
        keys: &["className", "level", "cooldown", "duration", "range", "ops", "effect"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_DISRUPT_SPAWN,
        cooldown: 5,
        range: Some(20),
        ops: Some(Fixed(10)),
        duration: Some(Levels([1, 2, 3, 4, 5])),
        keys: &["className", "level", "cooldown", "range", "ops", "duration"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_DISRUPT_TOWER,
        cooldown: 0,
        duration: Some(Fixed(5)),
        range: Some(50),
        ops: Some(Fixed(10)),
        effect: Some([0.9, 0.8, 0.7, 0.6, 0.5]),
        keys: &["className", "level", "cooldown", "duration", "range", "ops", "effect"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_DISRUPT_SOURCE,
        cooldown: 100,
        range: Some(3),
        ops: Some(Fixed(100)),
        duration: Some(Levels([100, 200, 300, 400, 500])),
        keys: &["className", "level", "cooldown", "range", "ops", "duration"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_SHIELD,
        effect: Some([5000.0, 10000.0, 15000.0, 20000.0, 25000.0]),
        duration: Some(Fixed(50)),
        cooldown: 20,
        energy: Some(100),
        keys: &["className", "level", "effect", "duration", "cooldown", "energy"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_REGEN_SOURCE,
        level: [10, 11, 12, 14, 22],
        cooldown: 100,
        duration: Some(Fixed(300)),
        range: Some(3),
        effect: Some([50.0, 100.0, 150.0, 200.0, 250.0]),
        period: Some(15),
        keys: &["className", "level", "cooldown", "duration", "range", "effect", "period"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_REGEN_MINERAL,
        level: [10, 11, 12, 14, 22],
        cooldown: 100,
        duration: Some(Fixed(100)),
        range: Some(3),
        effect: Some([2.0, 4.0, 6.0, 8.0, 10.0]),
        period: Some(10),
        keys: &["className", "level", "cooldown", "duration", "range", "effect", "period"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_DISRUPT_TERMINAL,
        level: [20, 21, 22, 23, 24],
        cooldown: 8,
        duration: Some(Fixed(10)),
        range: Some(50),
        ops: Some(Levels([50, 40, 30, 20, 10])),
        keys: &["className", "level", "cooldown", "duration", "range", "ops"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_OPERATE_POWER,
        level: [10, 11, 12, 14, 22],
        cooldown: 800,
        range: Some(3),
        duration: Some(Fixed(1000)),
        ops: Some(Fixed(200)),
        effect: Some([1.0, 2.0, 3.0, 4.0, 5.0]),
        keys: &["className", "level", "cooldown", "range", "duration", "ops", "effect"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_FORTIFY,
        cooldown: 5,
        range: Some(3),
        ops: Some(Fixed(5)),
        duration: Some(Levels([1, 2, 3, 4, 5])),
        keys: &["className", "level", "cooldown", "range", "ops", "duration"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_OPERATE_CONTROLLER,
        level: [20, 21, 22, 23, 24],
        cooldown: 800,
        range: Some(3),
        duration: Some(Fixed(1000)),
        ops: Some(Fixed(200)),
        effect: Some([10.0, 20.0, 30.0, 40.0, 50.0]),
        keys: &["className", "level", "cooldown", "range", "duration", "ops", "effect"],
        ..OPERATOR
    },
    PowerInfo {
        power: PWR_OPERATE_FACTORY,
        cooldown: 800,
        range: Some(3),
        duration: Some(Fixed(1000)),
        ops: Some(Fixed(100)),
        keys: &["className", "level", "cooldown", "range", "duration", "ops"],
        ..OPERATOR
    },
];

/// `POWER_INFO[pwr]`.
pub fn power_info(pwr: u32) -> Option<&'static PowerInfo> {
    POWER_INFO.get((pwr as usize).wrapping_sub(1))
}

pub const BODYPARTS_ALL: &[BodyPart] = BodyPart::ALL;
pub const RESOURCES_ALL: &[ResourceType] = ResourceType::ALL;
pub const COLORS_ALL: [u32; 10] = [
    COLOR_RED,
    COLOR_PURPLE,
    COLOR_BLUE,
    COLOR_CYAN,
    COLOR_GREEN,
    COLOR_YELLOW,
    COLOR_ORANGE,
    COLOR_BROWN,
    COLOR_GREY,
    COLOR_WHITE,
];
pub const INTERSHARD_RESOURCES: [&str; 4] = [SUBSCRIPTION_TOKEN, CPU_UNLOCK, PIXEL, ACCESS_KEY];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Commodity {
    pub product: ResourceType,
    /// Required factory level; `None` for commodities any factory can make.
    pub level: Option<u32>,
    pub amount: u32,
    pub cooldown: u32,
    pub components: &'static [(ResourceType, u32)],
}

const fn commodity_entry(
    product: ResourceType,
    level: Option<u32>,
    amount: u32,
    cooldown: u32,
    components: &'static [(ResourceType, u32)],
) -> Commodity {
    Commodity { product, level, amount, cooldown, components }
}

/// `COMMODITIES` in source order. See `commodity`.
pub const COMMODITIES: &[Commodity] = &[
    commodity_entry(UtriumBar, None, 100, 20, &[(U, 500), (Energy, 200)]),
    commodity_entry(U, None, 500, 20, &[(UtriumBar, 100), (Energy, 200)]),
    commodity_entry(LemergiumBar, None, 100, 20, &[(L, 500), (Energy, 200)]),
    commodity_entry(L, None, 500, 20, &[(LemergiumBar, 100), (Energy, 200)]),
    commodity_entry(ZynthiumBar, None, 100, 20, &[(Z, 500), (Energy, 200)]),
    commodity_entry(Z, None, 500, 20, &[(ZynthiumBar, 100), (Energy, 200)]),
    commodity_entry(KeaniumBar, None, 100, 20, &[(K, 500), (Energy, 200)]),
    commodity_entry(K, None, 500, 20, &[(KeaniumBar, 100), (Energy, 200)]),
    commodity_entry(GhodiumMelt, None, 100, 20, &[(G, 500), (Energy, 200)]),
    commodity_entry(G, None, 500, 20, &[(GhodiumMelt, 100), (Energy, 200)]),
    commodity_entry(Oxidant, None, 100, 20, &[(O, 500), (Energy, 200)]),
    commodity_entry(O, None, 500, 20, &[(Oxidant, 100), (Energy, 200)]),
    commodity_entry(Reductant, None, 100, 20, &[(H, 500), (Energy, 200)]),
    commodity_entry(H, None, 500, 20, &[(Reductant, 100), (Energy, 200)]),
    commodity_entry(Purifier, None, 100, 20, &[(X, 500), (Energy, 200)]),
    commodity_entry(X, None, 500, 20, &[(Purifier, 100), (Energy, 200)]),
    commodity_entry(Battery, None, 50, 10, &[(Energy, 600)]),
    commodity_entry(Energy, None, 500, 10, &[(Battery, 50)]),
    commodity_entry(Composite, Some(1), 20, 50, &[(UtriumBar, 20), (ZynthiumBar, 20), (Energy, 20)]),
    commodity_entry(
        Crystal,
        Some(2),
        6,
        21,
        &[(LemergiumBar, 6), (KeaniumBar, 6), (Purifier, 6), (Energy, 45)],
    ),
    commodity_entry(
        Liquid,
        Some(3),
        12,
        60,
        &[(Oxidant, 12), (Reductant, 12), (GhodiumMelt, 12), (Energy, 90)],
    ),
    commodity_entry(Wire, None, 20, 8, &[(UtriumBar, 20), (Silicon, 100), (Energy, 40)]),
    commodity_entry(Switch, Some(1), 5, 70, &[(Wire, 40), (Oxidant, 95), (UtriumBar, 35), (Energy, 20)]),
    commodity_entry(
        Transistor,
        Some(2),
        1,
        59,
        &[(Switch, 4), (Wire, 15), (Reductant, 85), (Energy, 8)],
    ),
    commodity_entry(
        Microchip,
        Some(3),
        1,
        250,
        &[(Transistor, 2), (Composite, 50), (Wire, 117), (Purifier, 25), (Energy, 16)],
    ),
    commodity_entry(
        Circuit,
        Some(4),
        1,
        800,
        &[(Microchip, 1), (Transistor, 5), (Switch, 4), (Oxidant, 115), (Energy, 32)],
    ),
    commodity_entry(
        Device,
        Some(5),
        1,
        600,
        &[(Circuit, 1), (Microchip, 3), (Crystal, 110), (GhodiumMelt, 150), (Energy, 64)],
    ),
    commodity_entry(Cell, None, 20, 8, &[(LemergiumBar, 20), (Biomass, 100), (Energy, 40)]),
    commodity_entry(Phlegm, Some(1), 2, 35, &[(Cell, 20), (Oxidant, 36), (LemergiumBar, 16), (Energy, 8)]),
    commodity_entry(Tissue, Some(2), 2, 164, &[(Phlegm, 10), (Cell, 10), (Reductant, 110), (Energy, 16)]),
    commodity_entry(
        Muscle,
        Some(3),
        1,
        250,
        &[(Tissue, 3), (Phlegm, 3), (ZynthiumBar, 50), (Reductant, 50), (Energy, 16)],
    ),
    commodity_entry(
        Organoid,
        Some(4),
        1,
        800,
        &[(Muscle, 1), (Tissue, 5), (Purifier, 208), (Oxidant, 256), (Energy, 32)],
    ),
    commodity_entry(
        Organism,
        Some(5),
        1,
        600,
        &[(Organoid, 1), (Liquid, 150), (Tissue, 6), (Cell, 310), (Energy, 64)],
    ),
    commodity_entry(Alloy, None, 20, 8, &[(ZynthiumBar, 20), (Metal, 100), (Energy, 40)]),
    commodity_entry(Tube, Some(1), 2, 45, &[(Alloy, 40), (ZynthiumBar, 16), (Energy, 8)]),
    commodity_entry(
        Fixtures,
        Some(2),
        1,
        115,
        &[(Composite, 20), (Alloy, 41), (Oxidant, 161), (Energy, 8)],
    ),
    commodity_entry(
        Frame,
        Some(3),
        1,
        125,
        &[(Fixtures, 2), (Tube, 4), (Reductant, 330), (ZynthiumBar, 31), (Energy, 16)],
    ),
    commodity_entry(
        Hydraulics,
        Some(4),
        1,
        800,
        &[(Liquid, 150), (Fixtures, 3), (Tube, 15), (Purifier, 208), (Energy, 32)],
    ),
    commodity_entry(
        Machine,
        Some(5),
        1,
        600,
        &[(Hydraulics, 1), (Frame, 2), (Fixtures, 3), (Tube, 12), (Energy, 64)],
    ),
    commodity_entry(Condensate, None, 20, 8, &[(KeaniumBar, 20), (Mist, 100), (Energy, 40)]),
    commodity_entry(
        Concentrate,
        Some(1),
        3,
        41,
        &[(Condensate, 30), (KeaniumBar, 15), (Reductant, 54), (Energy, 12)],
    ),
    commodity_entry(
        Extract,
        Some(2),
        2,
        128,
        &[(Concentrate, 10), (Condensate, 30), (Oxidant, 60), (Energy, 16)],
    ),
    commodity_entry(
        Spirit,
        Some(3),
        1,
        200,
        &[(Extract, 2), (Concentrate, 6), (Reductant, 90), (Purifier, 20), (Energy, 16)],
    ),
    commodity_entry(
        Emanation,
        Some(4),
        1,
        800,
        &[(Spirit, 2), (Extract, 2), (Concentrate, 3), (KeaniumBar, 112), (Energy, 32)],
    ),
    commodity_entry(
        Essence,
        Some(5),
        1,
        600,
        &[(Emanation, 1), (Spirit, 3), (Crystal, 110), (GhodiumMelt, 150), (Energy, 64)],
    ),
];

static COMMODITY_INDEX: [Option<u8>; ResourceType::COUNT] = {
    let mut out = [None; ResourceType::COUNT];
    let mut i = 0;
    while i < COMMODITIES.len() {
        out[COMMODITIES[i].product as usize] = Some(i as u8);
        i += 1;
    }
    out
};

/// `COMMODITIES[product]`.
pub fn commodity(product: ResourceType) -> Option<&'static Commodity> {
    COMMODITY_INDEX[product as usize].map(|i| &COMMODITIES[i as usize])
}

// ---------------------------------------------------------------------------------------------
// constants_json

/// Serialization matching `JSON.stringify` for the values in this file.
trait Json {
    fn json(&self) -> String;
}

macro_rules! int_json {
    ($($t:ty),*) => {$(
        impl Json for $t {
            fn json(&self) -> String {
                self.to_string()
            }
        }
    )*};
}
int_json!(u8, u32, i32, u64);

impl Json for f64 {
    // Rust prints the shortest round-trip digits like JS does, and whole numbers without ".0".
    // ponytail: JS switches to exponent form below 1e-6 and from 1e21; no constant gets there.
    fn json(&self) -> String {
        format!("{self}")
    }
}

impl Json for &str {
    // ponytail: no escaping; no constant contains a quote, backslash or control character.
    fn json(&self) -> String {
        format!("\"{self}\"")
    }
}

impl Json for Option<&str> {
    fn json(&self) -> String {
        self.map_or_else(|| "null".to_string(), |s| s.json())
    }
}

impl Json for PerLevel {
    fn json(&self) -> String {
        match self {
            Fixed(v) => v.json(),
            Levels(v) => v.json(),
        }
    }
}

impl<T: Json> Json for [T] {
    fn json(&self) -> String {
        let items: Vec<String> = self.iter().map(Json::json).collect();
        format!("[{}]", items.join(","))
    }
}

impl<T: Json, const N: usize> Json for [T; N] {
    fn json(&self) -> String {
        self.as_slice().json()
    }
}

/// A JSON object from `(key, already-serialized value)` pairs.
fn obj<K: Display>(entries: impl IntoIterator<Item = (K, String)>) -> String {
    let mut s = String::from("{");
    for (i, (k, v)) in entries.into_iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "\"{k}\":{v}");
    }
    s.push('}');
    s
}

/// A string-keyed table as a JSON object.
fn table<K: Display, V: Json>(t: &[(K, V)]) -> String {
    obj(t.iter().map(|(k, v)| (k, v.json())))
}

/// An integer-keyed table as a JSON object, skipping missing keys.
fn levels<T: Json>(t: &[Option<T>]) -> String {
    obj(t.iter().enumerate().filter_map(|(i, v)| v.as_ref().map(|v| (i, v.json()))))
}

fn power_info_json(p: &PowerInfo) -> String {
    obj(p.keys.iter().map(|&k| {
        let v = match k {
            "className" => Some(p.class_name.json()),
            "level" => Some(p.level.json()),
            "cooldown" => Some(p.cooldown.json()),
            "duration" => p.duration.map(|v| v.json()),
            "range" => p.range.map(|v| v.json()),
            "ops" => p.ops.map(|v| v.json()),
            "effect" => p.effect.map(|v| v.json()),
            "energy" => p.energy.map(|v| v.json()),
            "period" => p.period.map(|v| v.json()),
            _ => None,
        };
        (k, v.unwrap_or_else(|| panic!("POWER_INFO[{}] lists key {k} without a value", p.power)))
    }))
}

fn commodity_json(c: &Commodity) -> String {
    let mut fields = Vec::new();
    if let Some(level) = c.level {
        fields.push(("level", level.json()));
    }
    fields.push(("amount", c.amount.json()));
    fields.push(("cooldown", c.cooldown.json()));
    fields.push(("components", table(c.components)));
    obj(fields)
}

/// `JSON.stringify(require('@screeps/common/lib/constants'))`, byte for byte. The official
/// runtime defines the player globals from exactly this text
/// (reference/engine/src/game/game.js:435), so key order is part of the contract.
///
/// Every value in constants.js survives JSON: there are no functions, `undefined` or
/// non-finite numbers, and `MODE_SIMULATION` / `MODE_WORLD` are plain `null`.
pub fn constants_json() -> String {
    let mut o: Vec<(&str, String)> = Vec::with_capacity(430);
    macro_rules! put {
        ($($name:ident),* $(,)?) => { $( o.push((stringify!($name), $name.json())); )* };
    }
    macro_rules! put_as {
        ($name:ident, $json:expr) => {
            o.push((stringify!($name), $json))
        };
    }

    put!(
        OK, ERR_NOT_OWNER, ERR_NO_PATH, ERR_NAME_EXISTS, ERR_BUSY, ERR_NOT_FOUND,
        ERR_NOT_ENOUGH_ENERGY, ERR_NOT_ENOUGH_RESOURCES, ERR_INVALID_TARGET, ERR_FULL,
        ERR_NOT_IN_RANGE, ERR_INVALID_ARGS, ERR_TIRED, ERR_NO_BODYPART, ERR_NOT_ENOUGH_EXTENSIONS,
        ERR_RCL_NOT_ENOUGH, ERR_GCL_NOT_ENOUGH, ERR_ACCESS_DENIED,
        FIND_EXIT_TOP, FIND_EXIT_RIGHT, FIND_EXIT_BOTTOM, FIND_EXIT_LEFT, FIND_EXIT, FIND_CREEPS,
        FIND_MY_CREEPS, FIND_HOSTILE_CREEPS, FIND_SOURCES_ACTIVE, FIND_SOURCES,
        FIND_DROPPED_RESOURCES, FIND_STRUCTURES, FIND_MY_STRUCTURES, FIND_HOSTILE_STRUCTURES,
        FIND_FLAGS, FIND_CONSTRUCTION_SITES, FIND_MY_SPAWNS, FIND_HOSTILE_SPAWNS,
        FIND_MY_CONSTRUCTION_SITES, FIND_HOSTILE_CONSTRUCTION_SITES, FIND_MINERALS, FIND_NUKES,
        FIND_TOMBSTONES, FIND_POWER_CREEPS, FIND_MY_POWER_CREEPS, FIND_HOSTILE_POWER_CREEPS,
        FIND_DEPOSITS, FIND_RUINS,
        TOP, TOP_RIGHT, RIGHT, BOTTOM_RIGHT, BOTTOM, BOTTOM_LEFT, LEFT, TOP_LEFT,
        COLOR_RED, COLOR_PURPLE, COLOR_BLUE, COLOR_CYAN, COLOR_GREEN, COLOR_YELLOW, COLOR_ORANGE,
        COLOR_BROWN, COLOR_GREY, COLOR_WHITE,
        LOOK_CREEPS, LOOK_ENERGY, LOOK_RESOURCES, LOOK_SOURCES, LOOK_MINERALS, LOOK_DEPOSITS,
        LOOK_STRUCTURES, LOOK_FLAGS, LOOK_CONSTRUCTION_SITES, LOOK_NUKES, LOOK_TERRAIN,
        LOOK_TOMBSTONES, LOOK_POWER_CREEPS, LOOK_RUINS,
        OBSTACLE_OBJECT_TYPES,
        MOVE, WORK, CARRY, ATTACK, RANGED_ATTACK, TOUGH, HEAL, CLAIM,
    );
    put_as!(BODYPART_COST, table(BODYPART_COST));
    put!(
        WORLD_WIDTH, WORLD_HEIGHT, CREEP_LIFE_TIME, CREEP_CLAIM_LIFE_TIME, CREEP_CORPSE_RATE,
        CREEP_PART_MAX_ENERGY, CARRY_CAPACITY, HARVEST_POWER, HARVEST_MINERAL_POWER,
        HARVEST_DEPOSIT_POWER, REPAIR_POWER, DISMANTLE_POWER, BUILD_POWER, ATTACK_POWER,
        UPGRADE_CONTROLLER_POWER, RANGED_ATTACK_POWER, HEAL_POWER, RANGED_HEAL_POWER, REPAIR_COST,
        DISMANTLE_COST, RAMPART_DECAY_AMOUNT, RAMPART_DECAY_TIME, RAMPART_HITS,
    );
    put_as!(RAMPART_HITS_MAX, levels(&RAMPART_HITS_MAX));
    put!(
        ENERGY_REGEN_TIME, ENERGY_DECAY, SPAWN_HITS, SPAWN_ENERGY_START, SPAWN_ENERGY_CAPACITY,
        CREEP_SPAWN_TIME, SPAWN_RENEW_RATIO, SOURCE_ENERGY_CAPACITY, SOURCE_ENERGY_NEUTRAL_CAPACITY,
        SOURCE_ENERGY_KEEPER_CAPACITY, WALL_HITS, WALL_HITS_MAX, EXTENSION_HITS,
    );
    put_as!(EXTENSION_ENERGY_CAPACITY, levels(&EXTENSION_ENERGY_CAPACITY.map(Some)));
    put!(
        ROAD_HITS, ROAD_WEAROUT, ROAD_WEAROUT_POWER_CREEP, ROAD_DECAY_AMOUNT, ROAD_DECAY_TIME,
        LINK_HITS, LINK_HITS_MAX, LINK_CAPACITY, LINK_COOLDOWN, LINK_LOSS_RATIO, STORAGE_CAPACITY,
        STORAGE_HITS,
        STRUCTURE_SPAWN, STRUCTURE_EXTENSION, STRUCTURE_ROAD, STRUCTURE_WALL, STRUCTURE_RAMPART,
        STRUCTURE_KEEPER_LAIR, STRUCTURE_PORTAL, STRUCTURE_CONTROLLER, STRUCTURE_LINK,
        STRUCTURE_STORAGE, STRUCTURE_TOWER, STRUCTURE_OBSERVER, STRUCTURE_POWER_BANK,
        STRUCTURE_POWER_SPAWN, STRUCTURE_EXTRACTOR, STRUCTURE_LAB, STRUCTURE_TERMINAL,
        STRUCTURE_CONTAINER, STRUCTURE_NUKER, STRUCTURE_FACTORY, STRUCTURE_INVADER_CORE,
    );
    put_as!(CONSTRUCTION_COST, table(CONSTRUCTION_COST));
    put!(CONSTRUCTION_COST_ROAD_SWAMP_RATIO, CONSTRUCTION_COST_ROAD_WALL_RATIO);
    put_as!(CONTROLLER_LEVELS, levels(&CONTROLLER_LEVELS));
    put_as!(
        CONTROLLER_STRUCTURES,
        obj(CONTROLLER_STRUCTURES.iter().map(|(t, rcl)| (t, levels(rcl))))
    );
    put_as!(CONTROLLER_DOWNGRADE, levels(&CONTROLLER_DOWNGRADE));
    put!(
        CONTROLLER_DOWNGRADE_RESTORE, CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD,
        CONTROLLER_CLAIM_DOWNGRADE, CONTROLLER_RESERVE, CONTROLLER_RESERVE_MAX,
        CONTROLLER_MAX_UPGRADE_PER_TICK, CONTROLLER_ATTACK_BLOCKED_UPGRADE,
        CONTROLLER_NUKE_BLOCKED_UPGRADE,
        SAFE_MODE_DURATION, SAFE_MODE_COOLDOWN, SAFE_MODE_COST,
        TOWER_HITS, TOWER_CAPACITY, TOWER_ENERGY_COST, TOWER_POWER_ATTACK, TOWER_POWER_HEAL,
        TOWER_POWER_REPAIR, TOWER_OPTIMAL_RANGE, TOWER_FALLOFF_RANGE, TOWER_FALLOFF,
        OBSERVER_HITS, OBSERVER_RANGE,
        POWER_BANK_HITS, POWER_BANK_CAPACITY_MAX, POWER_BANK_CAPACITY_MIN, POWER_BANK_CAPACITY_CRIT,
        POWER_BANK_DECAY, POWER_BANK_HIT_BACK,
        POWER_SPAWN_HITS, POWER_SPAWN_ENERGY_CAPACITY, POWER_SPAWN_POWER_CAPACITY,
        POWER_SPAWN_ENERGY_RATIO,
        EXTRACTOR_HITS, EXTRACTOR_COOLDOWN,
        LAB_HITS, LAB_MINERAL_CAPACITY, LAB_ENERGY_CAPACITY, LAB_BOOST_ENERGY, LAB_BOOST_MINERAL,
        LAB_COOLDOWN, LAB_REACTION_AMOUNT, LAB_UNBOOST_ENERGY, LAB_UNBOOST_MINERAL,
        GCL_POW, GCL_MULTIPLY, GCL_NOVICE,
        MODE_SIMULATION, MODE_WORLD,
        TERRAIN_MASK_WALL, TERRAIN_MASK_SWAMP, TERRAIN_MASK_LAVA,
        MAX_CONSTRUCTION_SITES, MAX_CREEP_SIZE, MINERAL_REGEN_TIME,
    );
    put_as!(MINERAL_MIN_AMOUNT, table(MINERAL_MIN_AMOUNT));
    put!(MINERAL_RANDOM_FACTOR);
    put_as!(MINERAL_DENSITY, levels(&MINERAL_DENSITY));
    put_as!(MINERAL_DENSITY_PROBABILITY, levels(&MINERAL_DENSITY_PROBABILITY));
    put!(
        MINERAL_DENSITY_CHANGE,
        DENSITY_LOW, DENSITY_MODERATE, DENSITY_HIGH, DENSITY_ULTRA,
        DEPOSIT_EXHAUST_MULTIPLY, DEPOSIT_EXHAUST_POW, DEPOSIT_DECAY_TIME,
        TERMINAL_CAPACITY, TERMINAL_HITS, TERMINAL_SEND_COST, TERMINAL_MIN_SEND, TERMINAL_COOLDOWN,
        CONTAINER_HITS, CONTAINER_CAPACITY, CONTAINER_DECAY, CONTAINER_DECAY_TIME,
        CONTAINER_DECAY_TIME_OWNED,
        NUKER_HITS, NUKER_COOLDOWN, NUKER_ENERGY_CAPACITY, NUKER_GHODIUM_CAPACITY, NUKE_LAND_TIME,
        NUKE_RANGE,
    );
    put_as!(NUKE_DAMAGE, levels(&NUKE_DAMAGE));
    put!(
        FACTORY_HITS, FACTORY_CAPACITY, TOMBSTONE_DECAY_PER_PART, TOMBSTONE_DECAY_POWER_CREEP,
        RUIN_DECAY,
    );
    put_as!(RUIN_DECAY_STRUCTURES, table(RUIN_DECAY_STRUCTURES));
    put!(
        PORTAL_DECAY, ORDER_SELL, ORDER_BUY, MARKET_FEE, MARKET_MAX_ORDERS, MARKET_ORDER_LIFE_TIME,
        FLAGS_LIMIT, SUBSCRIPTION_TOKEN, CPU_UNLOCK, PIXEL, ACCESS_KEY, PIXEL_CPU_COST,
        RESOURCE_ENERGY, RESOURCE_POWER,
        RESOURCE_HYDROGEN, RESOURCE_OXYGEN, RESOURCE_UTRIUM, RESOURCE_LEMERGIUM, RESOURCE_KEANIUM,
        RESOURCE_ZYNTHIUM, RESOURCE_CATALYST, RESOURCE_GHODIUM,
        RESOURCE_SILICON, RESOURCE_METAL, RESOURCE_BIOMASS, RESOURCE_MIST,
        RESOURCE_HYDROXIDE, RESOURCE_ZYNTHIUM_KEANITE, RESOURCE_UTRIUM_LEMERGITE,
        RESOURCE_UTRIUM_HYDRIDE, RESOURCE_UTRIUM_OXIDE, RESOURCE_KEANIUM_HYDRIDE,
        RESOURCE_KEANIUM_OXIDE, RESOURCE_LEMERGIUM_HYDRIDE, RESOURCE_LEMERGIUM_OXIDE,
        RESOURCE_ZYNTHIUM_HYDRIDE, RESOURCE_ZYNTHIUM_OXIDE, RESOURCE_GHODIUM_HYDRIDE,
        RESOURCE_GHODIUM_OXIDE,
        RESOURCE_UTRIUM_ACID, RESOURCE_UTRIUM_ALKALIDE, RESOURCE_KEANIUM_ACID,
        RESOURCE_KEANIUM_ALKALIDE, RESOURCE_LEMERGIUM_ACID, RESOURCE_LEMERGIUM_ALKALIDE,
        RESOURCE_ZYNTHIUM_ACID, RESOURCE_ZYNTHIUM_ALKALIDE, RESOURCE_GHODIUM_ACID,
        RESOURCE_GHODIUM_ALKALIDE,
        RESOURCE_CATALYZED_UTRIUM_ACID, RESOURCE_CATALYZED_UTRIUM_ALKALIDE,
        RESOURCE_CATALYZED_KEANIUM_ACID, RESOURCE_CATALYZED_KEANIUM_ALKALIDE,
        RESOURCE_CATALYZED_LEMERGIUM_ACID, RESOURCE_CATALYZED_LEMERGIUM_ALKALIDE,
        RESOURCE_CATALYZED_ZYNTHIUM_ACID, RESOURCE_CATALYZED_ZYNTHIUM_ALKALIDE,
        RESOURCE_CATALYZED_GHODIUM_ACID, RESOURCE_CATALYZED_GHODIUM_ALKALIDE,
        RESOURCE_OPS,
        RESOURCE_UTRIUM_BAR, RESOURCE_LEMERGIUM_BAR, RESOURCE_ZYNTHIUM_BAR, RESOURCE_KEANIUM_BAR,
        RESOURCE_GHODIUM_MELT, RESOURCE_OXIDANT, RESOURCE_REDUCTANT, RESOURCE_PURIFIER,
        RESOURCE_BATTERY,
        RESOURCE_COMPOSITE, RESOURCE_CRYSTAL, RESOURCE_LIQUID,
        RESOURCE_WIRE, RESOURCE_SWITCH, RESOURCE_TRANSISTOR, RESOURCE_MICROCHIP, RESOURCE_CIRCUIT,
        RESOURCE_DEVICE,
        RESOURCE_CELL, RESOURCE_PHLEGM, RESOURCE_TISSUE, RESOURCE_MUSCLE, RESOURCE_ORGANOID,
        RESOURCE_ORGANISM,
        RESOURCE_ALLOY, RESOURCE_TUBE, RESOURCE_FIXTURES, RESOURCE_FRAME, RESOURCE_HYDRAULICS,
        RESOURCE_MACHINE,
        RESOURCE_CONDENSATE, RESOURCE_CONCENTRATE, RESOURCE_EXTRACT, RESOURCE_SPIRIT,
        RESOURCE_EMANATION, RESOURCE_ESSENCE,
    );
    put_as!(REACTIONS, obj(REACTIONS.iter().map(|(a, row)| (a, table(row)))));
    put_as!(
        BOOSTS,
        obj(BOOSTS.iter().map(|(part, row)| {
            (part, obj(row.iter().map(|(r, effects)| (r, table(effects)))))
        }))
    );
    put_as!(REACTION_TIME, table(REACTION_TIME));
    put!(
        PORTAL_UNSTABLE, PORTAL_MIN_TIMEOUT, PORTAL_MAX_TIMEOUT, POWER_BANK_RESPAWN_TIME,
        INVADERS_ENERGY_GOAL, SYSTEM_USERNAME, SIGN_NOVICE_AREA, SIGN_RESPAWN_AREA,
        SIGN_PLANNED_AREA,
        EVENT_ATTACK, EVENT_OBJECT_DESTROYED, EVENT_ATTACK_CONTROLLER, EVENT_BUILD, EVENT_HARVEST,
        EVENT_HEAL, EVENT_REPAIR, EVENT_RESERVE_CONTROLLER, EVENT_UPGRADE_CONTROLLER, EVENT_EXIT,
        EVENT_POWER, EVENT_TRANSFER,
        EVENT_ATTACK_TYPE_MELEE, EVENT_ATTACK_TYPE_RANGED, EVENT_ATTACK_TYPE_RANGED_MASS,
        EVENT_ATTACK_TYPE_DISMANTLE, EVENT_ATTACK_TYPE_HIT_BACK, EVENT_ATTACK_TYPE_NUKE,
        EVENT_HEAL_TYPE_MELEE, EVENT_HEAL_TYPE_RANGED,
        POWER_LEVEL_MULTIPLY, POWER_LEVEL_POW, POWER_CREEP_SPAWN_COOLDOWN,
        POWER_CREEP_DELETE_COOLDOWN, POWER_CREEP_MAX_LEVEL, POWER_CREEP_LIFE_TIME,
    );
    o.push(("POWER_CLASS", obj([("OPERATOR", POWER_CLASS_OPERATOR.json())])));
    put!(
        PWR_GENERATE_OPS, PWR_OPERATE_SPAWN, PWR_OPERATE_TOWER, PWR_OPERATE_STORAGE,
        PWR_OPERATE_LAB, PWR_OPERATE_EXTENSION, PWR_OPERATE_OBSERVER, PWR_OPERATE_TERMINAL,
        PWR_DISRUPT_SPAWN, PWR_DISRUPT_TOWER, PWR_DISRUPT_SOURCE, PWR_SHIELD, PWR_REGEN_SOURCE,
        PWR_REGEN_MINERAL, PWR_DISRUPT_TERMINAL, PWR_OPERATE_POWER, PWR_FORTIFY,
        PWR_OPERATE_CONTROLLER, PWR_OPERATE_FACTORY,
        EFFECT_INVULNERABILITY, EFFECT_COLLAPSE_TIMER,
        INVADER_CORE_HITS,
    );
    put_as!(INVADER_CORE_CREEP_SPAWN_TIME, levels(&INVADER_CORE_CREEP_SPAWN_TIME.map(Some)));
    put_as!(INVADER_CORE_EXPAND_TIME, levels(&INVADER_CORE_EXPAND_TIME));
    put!(INVADER_CORE_CONTROLLER_POWER, INVADER_CORE_CONTROLLER_DOWNGRADE);
    put_as!(STRONGHOLD_RAMPART_HITS, levels(&STRONGHOLD_RAMPART_HITS.map(Some)));
    put!(STRONGHOLD_DECAY_TICKS);

    // Second Object.assign in constants.js. POWER_INFO's keys are integers, which JS orders
    // ascending regardless of source order (PWR_FORTIFY is declared before PWR_OPERATE_POWER).
    put_as!(POWER_INFO, obj(POWER_INFO.iter().map(|p| (p.power, power_info_json(p)))));
    put!(BODYPARTS_ALL, RESOURCES_ALL, COLORS_ALL, INTERSHARD_RESOURCES);
    put_as!(COMMODITIES, obj(COMMODITIES.iter().map(|c| (c.product, commodity_json(c)))));

    obj(o)
}
