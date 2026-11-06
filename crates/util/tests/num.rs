//! Differential tests against node's outputs (tools/util-gen-num-fixture.js).

use util::num::*;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn f64_from_hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn check(name: &str, mut f: impl FnMut(&str, &str) -> Option<String>) {
    let text = fixture(name);
    let mut failures = Vec::new();
    let mut count = 0;
    for line in text.lines() {
        let (input, expected) = line.split_once('\t').unwrap();
        count += 1;
        if let Some(got) = f(input, expected) {
            failures.push(format!("{input}: expected {expected:?}, got {got:?}"));
        }
    }
    assert!(count > 1000, "{name}: fixture too small");
    if !failures.is_empty() {
        panic!("{name}: {} of {count} failed:\n{}", failures.len(), failures[..failures.len().min(30)].join("\n"));
    }
}

#[test]
fn to_string_matches_node() {
    check("num_tostring.txt", |input, expected| {
        let got = to_js_string(f64_from_hex(input));
        (got != expected).then_some(got)
    });
}

#[test]
fn fixed_exponential_precision_match_node() {
    check("num_format.txt", |input, expected| {
        let mut parts = input.split(' ');
        let kind = parts.next().unwrap();
        let x = f64_from_hex(parts.next().unwrap());
        let d: i64 = parts.next().unwrap().parse().unwrap();
        let got = match kind {
            "f" => to_fixed(x, d as usize),
            "e" => to_exponential(x, (d >= 0).then_some(d as usize)),
            "p" => to_precision(x, d as usize),
            _ => unreachable!(),
        };
        (got != expected).then_some(got)
    });
}

#[test]
fn radix_matches_node() {
    check("num_radix.txt", |input, expected| {
        let (x, r) = input.split_once(' ').unwrap();
        let got = to_radix_string(f64_from_hex(x), r.parse().unwrap());
        (got != expected).then_some(got)
    });
}

#[test]
fn string_to_number_matches_node() {
    check("num_parse.txt", |input, expected| {
        let bytes: Vec<u8> = (0..input.len()).step_by(2).map(|i| u8::from_str_radix(&input[i..i + 2], 16).unwrap()).collect();
        let s = String::from_utf8(bytes).unwrap();
        let v = string_to_number(&s);
        let got = if v.is_nan() { "nan".to_string() } else { format!("{:016x}", v.to_bits()) };
        (got != expected).then(|| format!("{got} for {s:?}"))
    });
}
