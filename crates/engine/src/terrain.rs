//! Static room terrain.
//!
//! Every format upstream uses is the same raw per-tile code, row-major (index y * 50 + x):
//! bit 1 (`TERRAIN_MASK_WALL`) and bit 2 (`TERRAIN_MASK_SWAMP`).
//! - Database `rooms.terrain.terrain`: a 2500-char string of '0'..'3'. `/api/game/room-terrain`
//!   with `encoded` returns these documents unchanged (backend/lib/game/api/game.js:263).
//! - `Room.Terrain.getRawBuffer()`: a 2500-byte Uint8Array of the same codes
//!   (driver/lib/runtime/make.js:44, engine/src/game/rooms.js:1235).
//! - The env key `terrainData` holds base64(zlib(JSON of all terrain documents))
//!   (backend/lib/cli/map.js:691); that wrapping belongs to storage, not here.
//!
//! Code 3 (both bits) occurs in real maps (4749 tiles in the default world). It stays 3 in the
//! raw buffer and reads as wall in every player-facing API (rooms.js:1232, 669, 1605), but the
//! processor tests each bit on its own (`utils.checkTerrain`), so there it is wall AND swamp:
//! use `is_wall` / `is_swamp` in processor code, `get` for player-facing reads.

use crate::constants::{BOTTOM, LEFT, RIGHT, TERRAIN_MASK_SWAMP, TERRAIN_MASK_WALL, TOP};

/// Tiles per room side.
pub const ROOM_SIZE: u8 = 50;
/// Tiles per room.
pub const ROOM_AREA: usize = 2500;

/// One room's terrain: raw codes at 2 bits per tile, 625 bytes.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct RoomTerrain {
    bits: [u8; ROOM_AREA / 4],
}

impl Default for RoomTerrain {
    /// All plain.
    fn default() -> Self {
        RoomTerrain { bits: [0; ROOM_AREA / 4] }
    }
}

fn index(x: u8, y: u8) -> usize {
    debug_assert!(x < ROOM_SIZE && y < ROOM_SIZE, "tile ({x}, {y}) is outside the room");
    y as usize * ROOM_SIZE as usize + x as usize
}

impl RoomTerrain {
    /// The raw code 0..=3, as `getRawBuffer` exposes it.
    pub fn raw(&self, x: u8, y: u8) -> u8 {
        let i = index(x, y);
        (self.bits[i / 4] >> (i % 4 * 2)) & 3
    }

    /// Sets the raw code; only the low two bits of `code` are kept.
    pub fn set_raw(&mut self, x: u8, y: u8, code: u8) {
        let i = index(x, y);
        let shift = i % 4 * 2;
        self.bits[i / 4] = self.bits[i / 4] & !(3 << shift) | (code & 3) << shift;
    }

    /// `Room.Terrain.get`: 0 plain, `TERRAIN_MASK_WALL` or `TERRAIN_MASK_SWAMP`; wall wins for
    /// code 3 (engine/src/game/rooms.js:1232). The player-facing wrapper must also reproduce
    /// the JS index aliasing for out-of-range x (`get(50, 0)` reads tile (0, 1)).
    pub fn get(&self, x: u8, y: u8) -> u8 {
        let v = self.raw(x, y);
        if v & TERRAIN_MASK_WALL != 0 {
            TERRAIN_MASK_WALL
        } else {
            v & TERRAIN_MASK_SWAMP
        }
    }

    /// `utils.checkTerrain(terrain, x, y, TERRAIN_MASK_WALL)` (engine/src/utils.js:333).
    pub fn is_wall(&self, x: u8, y: u8) -> bool {
        self.raw(x, y) & TERRAIN_MASK_WALL != 0
    }

    /// `utils.checkTerrain(terrain, x, y, TERRAIN_MASK_SWAMP)` (engine/src/utils.js:333). True
    /// for code 3 as well, unlike `get`: a road there costs and decays at both the swamp and the
    /// wall ratio (processor/intents/room/create-construction-site.js:37-45, roads/tick.js:11-19).
    pub fn is_swamp(&self, x: u8, y: u8) -> bool {
        self.raw(x, y) & TERRAIN_MASK_SWAMP != 0
    }

    /// Parses the 2500-char database string. `None` for any other length or for codes above 3
    /// (upstream's generator only writes 0..=3, common/index.js:25).
    pub fn from_db_str(s: &str) -> Option<Self> {
        let s = s.as_bytes();
        if s.len() != ROOM_AREA {
            return None;
        }
        let mut t = RoomTerrain::default();
        for (byte, chars) in t.bits.iter_mut().zip(s.chunks_exact(4)) {
            for (k, &c) in chars.iter().enumerate() {
                let code = c.wrapping_sub(b'0');
                if code > 3 {
                    return None;
                }
                *byte |= code << (k * 2);
            }
        }
        Some(t)
    }

    /// The 625-byte packed form (4 tiles per byte, low bits first; the native store).
    pub fn packed(&self) -> &[u8; ROOM_AREA / 4] {
        &self.bits
    }

    pub fn from_packed(bits: [u8; ROOM_AREA / 4]) -> Self {
        RoomTerrain { bits }
    }

    /// The 2500-char database string.
    pub fn to_db_string(&self) -> String {
        self.codes().map(|c| (b'0' + c) as char).collect()
    }

    /// From a 2500-byte raw buffer (`getRawBuffer` layout). `None` for any other length or for
    /// codes above 3.
    pub fn from_raw_buffer(buf: &[u8]) -> Option<Self> {
        if buf.len() != ROOM_AREA || buf.iter().any(|&c| c > 3) {
            return None;
        }
        let mut t = RoomTerrain::default();
        for (byte, codes) in t.bits.iter_mut().zip(buf.chunks_exact(4)) {
            *byte = codes[0] | codes[1] << 2 | codes[2] << 4 | codes[3] << 6;
        }
        Some(t)
    }

    /// `Room.Terrain.getRawBuffer()`.
    pub fn raw_buffer(&self) -> [u8; ROOM_AREA] {
        let mut out = [0; ROOM_AREA];
        for (o, c) in out.iter_mut().zip(self.codes()) {
            *o = c;
        }
        out
    }

    /// Raw codes in tile order.
    fn codes(&self) -> impl Iterator<Item = u8> + '_ {
        self.bits.iter().flat_map(|&b| [b & 3, b >> 2 & 3, b >> 4 & 3, b >> 6])
    }

    /// Non-wall tiles on one edge, in the order `room.find(FIND_EXIT_*)` returns them
    /// (engine/src/game/rooms.js:606): ascending x for TOP/BOTTOM, ascending y for LEFT/RIGHT.
    /// `side` is `TOP`, `RIGHT`, `BOTTOM` or `LEFT`, which equal the `FIND_EXIT_*` values.
    /// `FIND_EXIT` concatenates TOP, BOTTOM, RIGHT, LEFT, not numeric order (rooms.js:592-596).
    pub fn exits(&self, side: u32) -> impl Iterator<Item = (u8, u8)> + '_ {
        assert!(
            matches!(side, TOP | RIGHT | BOTTOM | LEFT),
            "exits: side must be TOP, RIGHT, BOTTOM or LEFT, got {side}"
        );
        let edge = ROOM_SIZE - 1;
        (0..ROOM_SIZE)
            .map(move |i| match side {
                TOP => (i, 0),
                RIGHT => (edge, i),
                BOTTOM => (i, edge),
                _ => (0, i),
            })
            .filter(move |&(x, y)| !self.is_wall(x, y))
    }

    /// The object list `/api/game/room-terrain` returns without `encoded`
    /// (common/index.js:43 decodeTerrain): y-major, and a code-3 tile yields both entries.
    pub fn decoded(&self) -> impl Iterator<Item = (u8, u8, &'static str)> + '_ {
        (0..ROOM_SIZE).flat_map(move |y| {
            (0..ROOM_SIZE).flat_map(move |x| {
                let v = self.raw(x, y);
                let wall = (v & TERRAIN_MASK_WALL != 0).then_some((x, y, "wall"));
                let swamp = (v & TERRAIN_MASK_SWAMP != 0).then_some((x, y, "swamp"));
                wall.into_iter().chain(swamp)
            })
        })
    }
}
