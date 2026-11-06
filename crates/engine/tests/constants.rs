use engine::constants::*;

#[test]
fn constants_json_matches_official_byte_for_byte() {
    // Fixture: node tools/engine-gen-constants-fixture.js
    let expected = include_str!("fixtures/constants.json");
    let actual = constants_json();
    if actual != expected {
        let at = actual.bytes().zip(expected.bytes()).position(|(a, b)| a != b).unwrap_or(0);
        let from = at.saturating_sub(80);
        panic!(
            "first difference at byte {at}\n  ours:     {}\n  official: {}",
            &actual[from..(at + 80).min(actual.len())],
            &expected[from..(at + 80).min(expected.len())],
        );
    }
}

#[test]
fn enums_round_trip_through_strings_and_indices() {
    assert_eq!(ResourceType::COUNT, 84);
    for (i, &r) in ResourceType::ALL.iter().enumerate() {
        assert_eq!(r.index(), i);
        assert_eq!(ResourceType::from_index(i), Some(r));
        assert_eq!(r.as_str().parse::<ResourceType>(), Ok(r));
    }
    for &s in StructureType::ALL {
        assert_eq!(s.as_str().parse::<StructureType>(), Ok(s));
    }
    for &p in BodyPart::ALL {
        assert_eq!(p.as_str().parse::<BodyPart>(), Ok(p));
    }
    assert_eq!("token".parse::<ResourceType>(), Err(()));
    assert_eq!(ResourceType::from_index(ResourceType::COUNT), None);
}

#[test]
fn lookups_agree_with_tables() {
    for &(p, cost) in BODYPART_COST {
        assert_eq!(p.cost(), cost);
    }
    assert_eq!(construction_cost(StructureType::Spawn), Some(15000));
    assert_eq!(construction_cost(StructureType::Controller), None);
    assert_eq!(controller_structures(StructureType::Extension, 8), Some(60));
    assert_eq!(controller_structures(StructureType::Link, 0), None);
    assert_eq!(controller_structures(StructureType::Spawn, 0), Some(0));
    assert_eq!(controller_structures(StructureType::Portal, 8), None);
    assert_eq!(controller_structures(StructureType::Spawn, 9), None);

    for &(a, row) in REACTIONS {
        for &(b, product) in row {
            assert_eq!(reaction(a, b), Some(product));
            // Every reaction is listed from both reagents.
            assert_eq!(reaction(b, a), Some(product));
        }
    }
    assert_eq!(reaction(ResourceType::H, ResourceType::H), None);
    assert_eq!(reaction_time(ResourceType::XGHO2), Some(150));
    assert_eq!(reaction_time(ResourceType::H), None);

    assert_eq!(boost(BodyPart::Work, ResourceType::XGH2O, "upgradeController"), Some(2.0));
    assert_eq!(boost(BodyPart::Tough, ResourceType::GO, "damage"), Some(0.7));
    assert_eq!(boost(BodyPart::Move, ResourceType::GO, "damage"), None);
    assert_eq!(boost(BodyPart::Work, ResourceType::LH, "harvest"), None);

    for c in COMMODITIES {
        assert_eq!(commodity(c.product), Some(c));
    }
    assert_eq!(commodity(ResourceType::Power), None);

    for (i, p) in POWER_INFO.iter().enumerate() {
        assert_eq!(p.power as usize, i + 1);
        assert_eq!(power_info(p.power), Some(p));
    }
    assert_eq!(power_info(0), None);
    assert_eq!(power_info(PWR_DISRUPT_TERMINAL).unwrap().ops.unwrap().at(4), 10);
}
