//! Differential test against node zlib (tools/util-gen-deflate-fixture.js).

use std::collections::HashMap;
use util::deflate::*;

/// Reads "<name> <len>\n<bytes>\n" records.
fn records() -> Vec<(String, Vec<u8>)> {
    let path = format!("{}/tests/fixtures/deflate_cases.bin", env!("CARGO_MANIFEST_DIR"));
    let data = std::fs::read(&path).unwrap();
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let nl = pos + data[pos..].iter().position(|&c| c == b'\n').unwrap();
        let header = std::str::from_utf8(&data[pos..nl]).unwrap();
        let (name, len) = header.rsplit_once(' ').unwrap();
        let len: usize = len.parse().unwrap();
        out.push((name.to_string(), data[nl + 1..nl + 1 + len].to_vec()));
        pos = nl + 1 + len + 1;
    }
    out
}

#[test]
fn inflates_node_output() {
    let recs = records();
    let originals: HashMap<&str, &[u8]> = recs.iter().filter_map(|(n, d)| Some((n.strip_prefix("orig:")?, d.as_slice()))).collect();
    let mut checked = 0;
    for (name, data) in &recs {
        if name.starts_with("orig:") {
            continue;
        }
        let (case, variant) = name.split_once(':').unwrap();
        let expected = originals[case];
        let got = match variant.split('-').next().unwrap() {
            "raw" => inflate(data, usize::MAX),
            "zlib" => zlib_decompress(data, usize::MAX),
            "gzip" => gzip_decompress(data, usize::MAX),
            k => panic!("unknown kind {k}"),
        };
        assert_eq!(got.as_deref(), Ok(expected), "{name}");
        checked += 1;
    }
    assert!(checked > 100);
}

#[test]
fn round_trips_fixture_inputs() {
    for (name, data) in records() {
        if name.starts_with("orig:") {
            assert_eq!(inflate(&deflate(&data), usize::MAX).unwrap(), data, "{name}");
        }
    }
}

/// Dev check that node can inflate our output:
///   cargo test -p util --test deflate -- --ignored export_for_node
///   node tools/util-check-deflate.js
#[test]
#[ignore]
fn export_for_node() {
    let dir = format!("{}/../../target/util-deflate-check", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, data) in records() {
        if let Some(case) = name.strip_prefix("orig:") {
            std::fs::write(format!("{dir}/{case}.orig"), &data).unwrap();
            std::fs::write(format!("{dir}/{case}.raw"), deflate(&data)).unwrap();
            std::fs::write(format!("{dir}/{case}.zlib"), zlib_compress(&data)).unwrap();
            std::fs::write(format!("{dir}/{case}.gz"), gzip_compress(&data)).unwrap();
        }
    }
}
