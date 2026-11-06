//! Room coordinates, ported from reference/engine/src/utils.js:358-392 (getRoomNameFromXY,
//! roomNameToXY) and the room classification in reference/backend/lib/utils.js:297-309.
//!
//! West and north are negative with no zero gap: W0 is x = -1, E0 is x = 0, N0 is y = -1,
//! S0 is y = 0. The simulation-mode room name "sim" is not a world room and is not handled here.

use std::fmt;
use std::str::FromStr;

/// A room's world coordinates. 8 bytes, `Copy`, ordered by x then y.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct RoomXY {
    pub x: i32,
    pub y: i32,
}

/// The number a name shows for one axis: E5 and W5 both show 5.
fn shown(c: i32) -> i32 {
    if c < 0 {
        -(c + 1)
    } else {
        c
    }
}

impl RoomXY {
    pub const fn new(x: i32, y: i32) -> Self {
        RoomXY { x, y }
    }

    /// Parses a name the way the engine's `utils.roomNameToXY` does (reference/engine/src/utils.js:374),
    /// which is what `new RoomPosition(x, y, name)` accepts: lowercase `w`/`n` work, any other
    /// direction letter means E/S, trailing junk is ignored. `None` where the engine yields NaN or
    /// the coordinate does not fit `i32`.
    ///
    /// Deliberate divergence: upstream finds the N/S letter at a fixed index chosen by
    /// `xx >= 10` / `xx >= 100`, so x numbers from 1000 up parse wrong (W1000N5 gives NaN). This
    /// continues the ladder by digit count, which is identical below 1000 and correct above.
    pub fn parse_engine(name: &str) -> Option<RoomXY> {
        // JS strings index by UTF-16 unit.
        let units = || name.encode_utf16();
        let xx = js_parse_int(units().skip(1))?;
        let vertical_pos = if xx < 10 { 2 } else { xx.ilog10() as usize + 2 };
        let yy = js_parse_int(units().skip(vertical_pos + 1))?;
        let horizontal = units().next();
        let vertical = units().nth(vertical_pos);
        let x = if horizontal == Some(b'W' as u16) || horizontal == Some(b'w' as u16) { -xx - 1 } else { xx };
        let y = if vertical == Some(b'N' as u16) || vertical == Some(b'n' as u16) { -yy - 1 } else { yy };
        Some(RoomXY { x: i32::try_from(x).ok()?, y: i32::try_from(y).ok()? })
    }

    /// True for rooms on a highway: either axis number ends in 0 (backend/lib/utils.js:297 isBus).
    pub fn is_highway(self) -> bool {
        shown(self.x) % 10 == 0 || shown(self.y) % 10 == 0
    }

    /// The 3x3 block at a sector's center, both axis numbers ending in 4..=6: the source keeper
    /// rooms plus the sector center room (backend/lib/utils.js:301 isCenter).
    pub fn is_center_room(self) -> bool {
        (4..=6).contains(&(shown(self.x) % 10)) && (4..=6).contains(&(shown(self.y) % 10))
    }

    /// The single room at a sector's center, both axis numbers ending in 5
    /// (backend/lib/utils.js:306 isVeryCenter).
    pub fn is_sector_center(self) -> bool {
        shown(self.x) % 10 == 5 && shown(self.y) % 10 == 5
    }

    /// Center rooms other than the sector center. This is the stronghold room rule
    /// (backend/lib/strongholds.js:143); whether keepers actually live there is map data
    /// (`rooms.sourceKeepers`), not geometry.
    pub fn is_source_keeper_room(self) -> bool {
        self.is_center_room() && !self.is_sector_center()
    }

    /// The center room of this room's sector. A sector is every room sharing the tens of both
    /// axis numbers in the same quadrant, its own `..0` highways included (strongholds.js:132
    /// selects a sector as `^W1\dN2\d$` for center W15N25).
    pub fn sector_center(self) -> RoomXY {
        fn axis(c: i32) -> i32 {
            let n = shown(c);
            let center = n - n % 10 + 5;
            if c < 0 {
                -center - 1
            } else {
                center
            }
        }
        RoomXY { x: axis(self.x), y: axis(self.y) }
    }
}

/// engine/src/utils.js:358 getRoomNameFromXY.
impl fmt::Display for RoomXY {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let h = if self.x < 0 { 'W' } else { 'E' };
        let v = if self.y < 0 { 'N' } else { 'S' };
        write!(f, "{h}{}{v}{}", shown(self.x), shown(self.y))
    }
}

/// Strict parse with the pathfinder's rule `^([WE])([0-9]+)([NS])([0-9]+)$`
/// (reference/driver/lib/path-finder.js:11), minus its world-size bound. Leading zeros are
/// accepted there, so W05N5 parses as W5N5. Use `parse_engine` for RoomPosition semantics.
impl FromStr for RoomXY {
    type Err = ();

    fn from_str(name: &str) -> Result<Self, ()> {
        fn number(digits: &[u8]) -> Result<i32, ()> {
            if digits.is_empty() {
                return Err(());
            }
            digits.iter().try_fold(0i32, |n, &d| {
                if !d.is_ascii_digit() {
                    return Err(());
                }
                n.checked_mul(10).and_then(|n| n.checked_add((d - b'0') as i32)).ok_or(())
            })
        }
        let b = name.as_bytes();
        let split = b.iter().skip(1).position(|c| !c.is_ascii_digit()).ok_or(())? + 1;
        let xn = number(&b[1..split])?;
        let yn = number(&b[split + 1..])?;
        let x = match b[0] {
            b'W' => -xn - 1,
            b'E' => xn,
            _ => return Err(()),
        };
        let y = match b[split] {
            b'N' => -yn - 1,
            b'S' => yn,
            _ => return Err(()),
        };
        Ok(RoomXY { x, y })
    }
}

/// JS `parseInt(s, 10)` over UTF-16 units; `None` for NaN. Saturates far outside the i32 range,
/// which callers reject anyway.
fn js_parse_int(mut units: impl Iterator<Item = u16>) -> Option<i64> {
    // StrWhiteSpaceChar: WhiteSpace and LineTerminator (ECMA-262 7.2, 7.3).
    fn is_space(u: u16) -> bool {
        matches!(u, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
    }
    let mut c = units.next();
    while c.is_some_and(is_space) {
        c = units.next();
    }
    let negative = c == Some(b'-' as u16);
    if negative || c == Some(b'+' as u16) {
        c = units.next();
    }
    let mut n: i64 = 0;
    let mut any = false;
    while let Some(d @ 0x30..=0x39) = c {
        n = n.saturating_mul(10).saturating_add((d - 0x30) as i64);
        any = true;
        c = units.next();
    }
    any.then_some(if negative { -n } else { n })
}
