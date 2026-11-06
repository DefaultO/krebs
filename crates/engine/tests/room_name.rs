use engine::room_name::RoomXY;

/// Fixture: node tools/engine-gen-room-name-fixture.js (column docs there).
const FIXTURE: &str = include_str!("fixtures/room_names.tsv");

/// A fixture coordinate pair; `None` for NaN, "-" or anything outside i32.
fn xy(x: &str, y: &str) -> Option<RoomXY> {
    let int = |s: &str| -> Option<i32> {
        let v: f64 = s.parse().ok()?;
        (v.fract() == 0.0 && v >= i32::MIN as f64 && v <= i32::MAX as f64).then_some(v as i32)
    };
    Some(RoomXY::new(int(x)?, int(y)?))
}

#[test]
fn matches_official_functions() {
    let mut rows = 0;
    for line in FIXTURE.lines() {
        let c: Vec<&str> = line.split('\t').collect();
        assert_eq!(c.len(), 13, "bad fixture row {line:?}");
        let name = c[2];
        let strict = xy(c[5], c[6]);
        // c[3..5] is the official engine result. c[7..9] equals it while the x number is below
        // 1000 (the generator checks), and continues its digit-position ladder above that, where
        // upstream breaks.
        let ours = xy(c[7], c[8]);
        assert_eq!(RoomXY::parse_engine(name), ours, "parse_engine({name:?})");
        assert_eq!(name.parse::<RoomXY>().ok(), strict, "from_str({name:?})");

        if c[0] != "-" {
            let room = RoomXY::new(c[0].parse().unwrap(), c[1].parse().unwrap());
            assert_eq!(room.to_string(), name);
            assert_eq!(room.is_highway(), c[9] == "1", "is_highway({name})");
            assert_eq!(room.is_center_room(), c[10] == "1", "is_center_room({name})");
            assert_eq!(room.is_sector_center(), c[11] == "1", "is_sector_center({name})");
            assert_eq!(room.is_source_keeper_room(), c[10] == "1" && c[11] == "0");
            assert_eq!(room.sector_center().to_string(), c[12], "sector_center({name})");
        }
        rows += 1;
    }
    assert!(rows > 7000);
}

#[test]
fn seams_and_extremes() {
    assert_eq!(RoomXY::new(-1, -1).to_string(), "W0N0");
    assert_eq!(RoomXY::new(0, 0).to_string(), "E0S0");
    assert_eq!("W500N500".parse(), Ok(RoomXY::new(-501, -501)));
    assert_eq!(RoomXY::parse_engine("W500N500"), Some(RoomXY::new(-501, -501)));
    let corner = RoomXY::new(i32::MIN, i32::MAX);
    assert_eq!(corner.to_string().parse(), Ok(corner));
    assert_eq!(std::mem::size_of::<RoomXY>(), 8);
}
