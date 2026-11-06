//! Differential test against node's JSON.parse / JSON.stringify
//! (tools/util-gen-json-fixture.js).

use util::json::Json;

/// Reads "<tag> <len>\n<bytes>\n" records.
fn records(data: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let nl = pos + data[pos..].iter().position(|&c| c == b'\n').unwrap();
        let header = std::str::from_utf8(&data[pos..nl]).unwrap();
        let (tag, len) = header.split_once(' ').unwrap();
        let len: usize = len.parse().unwrap();
        let body = String::from_utf8(data[nl + 1..nl + 1 + len].to_vec()).unwrap();
        out.push((tag.to_string(), body));
        pos = nl + 1 + len + 1;
    }
    out
}

#[test]
fn matches_node() {
    let path = format!("{}/tests/fixtures/json_cases.txt", env!("CARGO_MANIFEST_DIR"));
    let data = std::fs::read(&path).unwrap();
    let recs = records(&data);
    let mut failures = Vec::new();
    let mut cases = 0;
    let mut parsed: Option<Result<Json, util::json::ParseError>> = None;
    let mut input = String::new();
    for (tag, body) in recs {
        let short = |s: &str| s.chars().take(200).collect::<String>();
        match tag.as_str() {
            "in" => {
                cases += 1;
                parsed = Some(Json::parse(&body));
                input = body;
            }
            "err" => {
                if let Some(Ok(v)) = &parsed {
                    failures.push(format!("{:?}: node rejects, we parsed {}", short(&input), short(&v.stringify())));
                }
            }
            _ => {
                let got = match &parsed {
                    Some(Ok(v)) if tag == "c" => v.stringify(),
                    Some(Ok(v)) => v.stringify_pretty(tag[1..].parse().unwrap()),
                    Some(Err(e)) => format!("error: {e}"),
                    None => unreachable!(),
                };
                if got != body {
                    failures.push(format!("{:?} [{tag}]:\n  node: {:?}\n  ours: {:?}", short(&input), short(&body), short(&got)));
                }
            }
        }
    }
    assert!(cases > 1000);
    if !failures.is_empty() {
        panic!("{} of {cases} cases failed:\n{}", failures.len(), failures[..failures.len().min(20)].join("\n"));
    }
}
