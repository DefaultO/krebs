use engine::constants::{BOTTOM, LEFT, RIGHT, TERRAIN_MASK_SWAMP, TERRAIN_MASK_WALL, TOP};
use engine::terrain::{RoomTerrain, ROOM_AREA};

/// Deterministic pseudo-random codes 0..=3 (an LCG is plenty for coverage).
fn noise_codes() -> Vec<u8> {
    let mut s: u32 = 12345;
    (0..ROOM_AREA)
        .map(|_| {
            s = s.wrapping_mul(1103515245).wrapping_add(12345);
            (s >> 16) as u8 & 3
        })
        .collect()
}

#[test]
fn db_string_and_raw_buffer_round_trip() {
    let codes = noise_codes();
    let db: String = codes.iter().map(|&c| (b'0' + c) as char).collect();

    let t = RoomTerrain::from_db_str(&db).unwrap();
    assert_eq!(t.to_db_string(), db);
    assert_eq!(t.raw_buffer().as_slice(), codes.as_slice());
    assert_eq!(RoomTerrain::from_raw_buffer(&codes), Some(t.clone()));
    for (i, &c) in codes.iter().enumerate() {
        assert_eq!(t.raw((i % 50) as u8, (i / 50) as u8), c);
    }

    let mut built = RoomTerrain::default();
    for (i, &c) in codes.iter().enumerate() {
        built.set_raw((i % 50) as u8, (i / 50) as u8, c);
    }
    assert_eq!(built, t);
    built.set_raw(7, 3, 0);
    assert_eq!(built.raw(7, 3), 0);
    assert_eq!(built.raw(6, 3), t.raw(6, 3));
    assert_eq!(built.raw(8, 3), t.raw(8, 3));

    assert_eq!(std::mem::size_of::<RoomTerrain>(), 625);
}

#[test]
fn rejects_malformed_input() {
    let ok = "0".repeat(ROOM_AREA);
    assert!(RoomTerrain::from_db_str(&ok).is_some());
    assert!(RoomTerrain::from_db_str(&ok[1..]).is_none());
    assert!(RoomTerrain::from_db_str(&format!("{ok}0")).is_none());
    assert!(RoomTerrain::from_db_str(&format!("4{}", &ok[1..])).is_none());
    assert!(RoomTerrain::from_db_str(&format!("x{}", &ok[1..])).is_none());
    assert!(RoomTerrain::from_raw_buffer(&[0; 2499]).is_none());
    let mut lava = [0; ROOM_AREA];
    lava[100] = 4;
    assert!(RoomTerrain::from_raw_buffer(&lava).is_none());
}

#[test]
fn code_three_reads_as_wall_but_stays_raw() {
    let mut t = RoomTerrain::default();
    t.set_raw(1, 1, 3);
    t.set_raw(2, 1, TERRAIN_MASK_SWAMP);
    t.set_raw(3, 1, TERRAIN_MASK_WALL);
    assert_eq!(t.get(1, 1), TERRAIN_MASK_WALL);
    assert_eq!(t.raw(1, 1), 3);
    assert_eq!(t.raw_buffer()[51], 3);
    assert_eq!(t.get(2, 1), TERRAIN_MASK_SWAMP);
    assert_eq!(t.get(3, 1), TERRAIN_MASK_WALL);
    assert_eq!(t.get(4, 1), 0);
    assert!(t.is_wall(1, 1) && !t.is_wall(2, 1));
    // The processor's checkTerrain sees both bits of code 3 (road cost/decay x5 x150 there).
    assert!(t.is_swamp(1, 1) && t.is_swamp(2, 1) && !t.is_swamp(3, 1) && !t.is_swamp(4, 1));
    assert_eq!(
        t.decoded().collect::<Vec<_>>(),
        [(1, 1, "wall"), (1, 1, "swamp"), (2, 1, "swamp"), (3, 1, "wall")]
    );
}

#[test]
fn exits_follow_find_order() {
    let mut t = RoomTerrain::default();
    for i in 0..50 {
        for (x, y) in [(i, 0), (49, i), (i, 49), (0, i)] {
            t.set_raw(x, y, TERRAIN_MASK_WALL);
        }
    }
    for (x, y) in [(10, 0), (11, 0), (49, 20), (5, 49), (0, 30), (0, 31)] {
        t.set_raw(x, y, 0);
    }
    t.set_raw(12, 0, 3); // still a wall
    t.set_raw(49, 21, TERRAIN_MASK_SWAMP); // swamp exits are exits
    assert_eq!(t.exits(TOP).collect::<Vec<_>>(), [(10, 0), (11, 0)]);
    assert_eq!(t.exits(RIGHT).collect::<Vec<_>>(), [(49, 20), (49, 21)]);
    assert_eq!(t.exits(BOTTOM).collect::<Vec<_>>(), [(5, 49)]);
    assert_eq!(t.exits(LEFT).collect::<Vec<_>>(), [(0, 30), (0, 31)]);
}
