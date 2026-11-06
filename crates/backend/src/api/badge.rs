//! mirror: reference/backend/lib/game/api/badge.js
//! parity: DIVERGENT(FMT-BADGE): `Math.sin/cos` come from the platform libm, not V8's fdlibm, so a coordinate can
//! differ in its last digits (tests/fixtures/badge compares numbers to 1e-12); everything else is exact
//!
//! `getBadgeSvg(badge, border)` with the 24 path generators and the 80-colour palette (FMT-BADGE). Owner: WP04.

use std::f64::consts::PI;
use util::json::Json;

fn n(x: f64) -> String {
    util::num::to_js_string(x)
}

/// A template literal's line break followed by `k` spaces of indentation.
fn nl(k: usize) -> String {
    format!("\n{}", " ".repeat(k))
}

/// `BadgePaths[type].calc(param)` → (path1, path2, flip).
fn paths(ty: u32, param: f64) -> Option<(String, String, Option<f64>)> {
    let p = param;
    let (cos, sin) = (f64::cos, f64::sin);
    Some(match ty {
        1 => {
            let (mut vert, mut hor) = (0.0, 0.0);
            if p > 0.0 {
                vert = p * 30.0 / 100.0;
            }
            if p < 0.0 {
                hor = -p * 30.0 / 100.0;
            }
            (format!("M 50 {} L {} 50 H {} Z", n(100.0 - vert), n(hor), n(100.0 - hor)), format!("M {} 50 H {} L 50 {} Z", n(hor), n(100.0 - hor), n(vert)), None)
        }
        2 => {
            let (mut x, mut y) = (0.0, 0.0);
            if p > 0.0 {
                x = p * 30.0 / 100.0;
            }
            if p < 0.0 {
                y = -p * 30.0 / 100.0;
            }
            (
                format!("M {} {} L 50 50 L {} {} V -1 H -1 Z", n(x), n(y), n(100.0 - x), n(y)),
                format!("M {} {} L 50 50 L {} {} V 101 H -1 Z", n(x), n(100.0 - y), n(100.0 - x), n(100.0 - y)),
                None,
            )
        }
        3 => {
            let angle = PI / 4.0 + PI / 4.0 * (p + 100.0) / 200.0;
            let (a1, a2, a3) = (-PI / 2.0, PI / 2.0 + PI / 3.0, PI / 2.0 - PI / 3.0);
            let pt = |a: f64| format!("{} {}", n(50.0 + 100.0 * cos(a)), n(50.0 + 100.0 * sin(a)));
            (
                format!("M 50 50 L {} L {} Z", pt(a1 - angle / 2.0), pt(a1 + angle / 2.0)),
                format!("M 50 50 L {} L {} Z{}M 50 50 L {} L {}", pt(a2 - angle / 2.0), pt(a2 + angle / 2.0), nl(26), pt(a3 - angle / 2.0), pt(a3 + angle / 2.0)),
                Some(180.0),
            )
        }
        4 => {
            let p = p + 100.0;
            let (y1, y2) = (50.0 - p * 30.0 / 200.0, 50.0 + p * 30.0 / 200.0);
            (format!("M 0 {} H 100 V 100 H 0 Z", n(y2)), if p > 0.0 { format!("M 0 {} H 100 V {} H 0 Z", n(y1), n(y2)) } else { String::new() }, Some(90.0))
        }
        5 => {
            let p = p + 100.0;
            let (x1, x2) = (50.0 - p * 10.0 / 200.0 - 10.0, 50.0 + p * 10.0 / 200.0 + 10.0);
            (format!("M {} 0 H {} V 100 H {} Z", n(x1), n(x2), n(x1)), format!("M 0 {} H 100 V {} H 0 Z", n(x1), n(x2)), Some(45.0))
        }
        6 => {
            let width = 5.0 + (p + 100.0) * 8.0 / 200.0;
            let (x1, x2, x3) = (50.0, 20.0, 80.0);
            (
                format!("M {} 0 H {} V 100 H {}", n(x1 - width), n(x1 + width), n(x1 - width)),
                format!("M {} 0 H {} V 100 H {} Z{}M {} 0 H {} V 100 H {} Z", n(x2 - width), n(x2 + width), n(x2 - width), nl(26), n(x3 - width), n(x3 + width), n(x3 - width)),
                Some(90.0),
            )
        }
        7 => {
            let w = 20.0 + p * 10.0 / 100.0;
            (
                "M 0 50 Q 25 30 50 50 T 100 50 V 100 H 0 Z".into(),
                format!("M 0 {} Q 25 {} 50 {} T 100 {}{}V {} Q 75 {} 50 {} T 0 {} Z", n(50.0 - w), n(30.0 - w), n(50.0 - w), n(50.0 - w), nl(28), n(50.0 + w), n(70.0 + w), n(50.0 + w), n(50.0 + w)),
                Some(90.0),
            )
        }
        8 => {
            let y = p * 20.0 / 100.0;
            ("M 0 50 H 100 V 100 H 0 Z".into(), format!("M 0 50 Q 50 {} 100 50 Q 50 {} 0 50 Z", n(y), n(100.0 - y)), Some(90.0))
        }
        9 => {
            let (mut y1, mut y2, h) = (0.0, 50.0, 70.0);
            if p > 0.0 {
                y1 += p / 100.0 * 20.0;
            }
            if p < 0.0 {
                y2 += p / 100.0 * 30.0;
            }
            (
                format!("M 50 {} L 100 {} V 101 H 0 V {} Z", n(y1), n(y1 + h), n(y1 + h)),
                format!("M 50 {} L 100 {} V 101 H 0 V {} Z", n(y1 + y2), n(y1 + y2 + h), n(y1 + y2 + h)),
                Some(180.0),
            )
        }
        10 => {
            let (mut r, mut d) = (30.0, 7.0);
            if p > 0.0 {
                r += p * 50.0 / 100.0;
            }
            if p < 0.0 {
                d -= p * 20.0 / 100.0;
            }
            (
                format!("M {} {} A {} {} 0 0 0 {} {} H 101 V {} Z", n(50.0 + d + r), n(50.0 - r), n(r), n(r), n(50.0 + d + r), n(50.0 + r), n(50.0 - r)),
                format!("M {} {} A {} {} 0 0 1 {} {} H -1 V {} Z", n(50.0 - d - r), n(50.0 - r), n(r), n(r), n(50.0 - d - r), n(50.0 + r), n(50.0 - r)),
                Some(90.0),
            )
        }
        11 => {
            let (mut a1, mut a2) = (30.0, 30.0);
            let (x, y) = (50.0 - 50.0 * cos(PI / 4.0), 50.0 - 50.0 * sin(PI / 4.0));
            if p > 0.0 {
                a1 += p * 25.0 / 100.0;
                a2 += p * 25.0 / 100.0;
            }
            if p < 0.0 {
                a2 -= p * 50.0 / 100.0;
            }
            (
                format!(
                    "M {} {} Q {} 50 {} {} H 0 V {} Z{}M {} {} Q {} 50 {} {} H 100 V {} Z",
                    n(x), n(y), n(a1), n(x), n(100.0 - y), n(y), nl(26), n(100.0 - x), n(y), n(100.0 - a1), n(100.0 - x), n(100.0 - y), n(y)
                ),
                format!(
                    "M {} {} Q 50 {} {} {} V 0 H {} Z{}M {} {} Q 50 {} {} {} V 100 H {} Z",
                    n(x), n(y), n(a2), n(100.0 - x), n(y), n(x), nl(26), n(x), n(100.0 - y), n(100.0 - a2), n(100.0 - x), n(100.0 - y), n(x)
                ),
                Some(90.0),
            )
        }
        12 => {
            let (mut a1, mut a2) = (30.0, 35.0);
            if p > 0.0 {
                a1 += p * 30.0 / 100.0;
            }
            if p < 0.0 {
                a2 += p * 15.0 / 100.0;
            }
            (
                format!("M 0 {} H 100 V 100 H 0 Z", n(a1)),
                format!("M 0 {} H {} V 100 H 0 Z{}M 100 {} H {} V 100 H 100 Z", n(a1), n(a2), nl(26), n(a1), n(100.0 - a2)),
                Some(180.0),
            )
        }
        13 => {
            let (mut r, mut d) = (30.0, 0.0);
            if p > 0.0 {
                r += p * 50.0 / 100.0;
            }
            if p < 0.0 {
                d -= p * 20.0 / 100.0;
            }
            ("M 0 0 H 50 V 100 H 0 Z".into(), format!("M {} {} A {} {} 0 0 0 {} {} V 0 H {} Z", n(50.0 - r), n(50.0 - d - r), n(r), n(r), n(50.0 + r), n(50.0 - r - d), n(50.0 - r)), Some(180.0))
        }
        14 => {
            let d = 0.0;
            let a = PI / 4.0 + p * PI / 4.0 / 100.0;
            (
                format!("M 50 0 Q 50 {} {} {} H 100 V 0 H 50 Z", n(50.0 + d), n(50.0 + 50.0 * cos(a)), n(50.0 + 50.0 * sin(a))),
                format!("M 50 0 Q 50 {} {} {} H 0 V 0 H 50 Z", n(50.0 + d), n(50.0 - 50.0 * cos(a)), n(50.0 + 50.0 * sin(a))),
                Some(180.0),
            )
        }
        15 => {
            let w = 13.0 + p * 6.0 / 100.0;
            let (r1, r2, d) = (80.0, 45.0, 10.0);
            let arc = |r: f64| {
                format!(
                    "M {} {} A {} {} 0 0 1 {} {}{}H {} A {} {} 0 1 0 {} {}",
                    n(50.0 - r - w), n(100.0 + d), n(r + w), n(r + w), n(50.0 + r + w), n(100.0 + d), nl(35), n(50.0 + r - w), n(r - w), n(r - w), n(50.0 - r + w), n(100.0 + d)
                )
            };
            (arc(r1), arc(r2), Some(180.0))
        }
        16 => {
            let (mut a, mut d) = (30.0 * PI / 180.0, 25.0);
            if p > 0.0 {
                a += 30.0 * PI / 180.0 * p / 100.0;
            }
            if p < 0.0 {
                d += p * 25.0 / 100.0;
            }
            // path1 loops with `- Math.PI/2`, path2 with `+ Math.PI/2` (x + (-y) is exactly x - y)
            let (mut p1, mut p2) = (String::new(), String::new());
            for (out, sign) in [(&mut p1, -1.0), (&mut p2, 1.0)] {
                for i in 0..3 {
                    let i = i as f64;
                    let (angle1, angle2) = (i * PI * 2.0 / 3.0 + a / 2.0 + sign * PI / 2.0, i * PI * 2.0 / 3.0 - a / 2.0 + sign * PI / 2.0);
                    *out += &format!(
                        "M {} {}{}L {} {}{}L {} {}{}A {} {} 0 0 1 {} {} Z",
                        n(50.0 + 100.0 * cos(angle1)), n(50.0 + 100.0 * sin(angle1)), nl(31),
                        n(50.0 + 100.0 * cos(angle2)), n(50.0 + 100.0 * sin(angle2)), nl(31),
                        n(50.0 + d * cos(angle2)), n(50.0 + d * sin(angle2)), nl(31),
                        n(d), n(d), n(50.0 + d * cos(angle1)), n(50.0 + d * sin(angle1))
                    );
                }
            }
            (p1, p2, None)
        }
        17 => {
            let (mut w, mut h) = (35.0, 45.0);
            if p > 0.0 {
                w += p * 20.0 / 100.0;
            }
            if p < 0.0 {
                h -= p * 30.0 / 100.0;
            }
            (format!("M 50 45 L {} {} H {} Z", n(50.0 - w), n(h + 45.0), n(50.0 + w)), format!("M 50 0 L {} {} H {} Z", n(50.0 - w), n(h), n(50.0 + w)), None)
        }
        18 => {
            let (mut a, mut d) = (90.0 * PI / 180.0, 10.0);
            if p > 0.0 {
                a -= 60.0 / 180.0 * PI * p / 100.0;
            }
            if p < 0.0 {
                d -= p * 15.0 / 100.0;
            }
            let (mut p1, mut p2) = (String::new(), String::new());
            for i in 0..3 {
                let fi = i as f64;
                let (angle1, angle2) = (PI * 2.0 / 3.0 * fi + a / 2.0 - PI / 2.0, PI * 2.0 / 3.0 * fi - a / 2.0 - PI / 2.0);
                let path = format!(
                    "M {} {}{}L {} {}{}L {} {} Z",
                    n(50.0 + 100.0 * cos(angle1)), n(50.0 + 100.0 * sin(angle1)), nl(28),
                    n(50.0 + 100.0 * cos(angle2)), n(50.0 + 100.0 * sin(angle2)), nl(28),
                    n(50.0 + d * cos((angle1 + angle2) / 2.0)), n(50.0 + d * sin((angle1 + angle2) / 2.0))
                );
                if i == 0 {
                    p1 += &path;
                } else {
                    p2 += &path;
                }
            }
            (p1, p2, Some(180.0))
        }
        19 => {
            let (w2, w1) = (20.0 + p * 20.0 / 100.0, 60.0 + p * 20.0 / 100.0);
            (format!("M 50 -10 L {} 100 H {} Z", n(50.0 - w1), n(50.0 + w1)), if w2 > 0.0 { format!("M 50 0 L {} 100 H {} Z", n(50.0 - w2), n(50.0 + w2)) } else { String::new() }, Some(180.0))
        }
        20 => {
            let (mut w, mut h) = (10.0, 20.0);
            if p > 0.0 {
                w += p * 20.0 / 100.0;
            }
            if p < 0.0 {
                h += p * 40.0 / 100.0;
            }
            (format!("M 0 {} H {} V 100 H 0 Z", n(50.0 - h), n(50.0 - w)), format!("M {} 0 V {} H 100 V 0 Z", n(50.0 + w), n(50.0 + h)), Some(90.0))
        }
        21 => {
            let (mut w, mut h) = (40.0, 50.0);
            if p > 0.0 {
                w -= p * 20.0 / 100.0;
            }
            if p < 0.0 {
                h += p * 20.0 / 100.0;
            }
            (
                format!("M 50 {} Q {} 0 50 0 T 50 {} Z{}M 50 {} Q {} 100 50 100 T 50 {} Z", n(h), n(50.0 + w), n(h), nl(26), n(100.0 - h), n(50.0 + w), n(100.0 - h)),
                format!("M {} 50 Q 0 {} 0 50 T {} 50 Z{}M {} 50 Q 100 {} 100 50 T {} 50 Z", n(h), n(50.0 + w), n(h), nl(26), n(100.0 - h), n(50.0 + w), n(100.0 - h)),
                Some(45.0),
            )
        }
        22 => {
            let w = 20.0 + p * 10.0 / 100.0;
            let mut p2 = String::new();
            for i in -4i32..4 {
                for j in -4i32..4 {
                    let a = ((i + j) % 2) as f64; // JS remainder keeps the sign
                    let (fi, fj) = (i as f64, j as f64);
                    p2 += &format!("M {} {} h {} v {} h {} Z", n(50.0 - w - w * 2.0 * fi), n(50.0 - w - w * 2.0 * (fj + a)), n(-w * 2.0), n(w * 2.0), n(w * 2.0));
                }
            }
            (format!("M {} {} H {} V {} H {} Z", n(50.0 - w), n(50.0 - w), n(50.0 + w), n(50.0 + w), n(50.0 - w)), p2, Some(45.0))
        }
        23 => {
            let (mut w, mut h) = (17.0, 25.0);
            if p > 0.0 {
                w += p * 35.0 / 100.0;
            }
            if p < 0.0 {
                h -= p * 23.0 / 100.0;
            }
            let row = |yy: f64| (-4..=4).map(|i| format!("M {} {} l {} {} l {} {} l {} {} Z", n(50.0 - w * i as f64 * 2.0), n(yy), n(-w), n(-h), n(-w), n(h), n(w), n(h))).collect::<String>();
            (row(50.0 - h), row(50.0 + h), Some(90.0))
        }
        24 => {
            let (mut w, mut h) = (50.0, 45.0);
            if p > 0.0 {
                w += p * 60.0 / 100.0;
            }
            if p < 0.0 {
                h += p * 30.0 / 100.0;
            }
            (format!("M 0 {} L 50 70 L 100 {} V 100 H 0 Z", n(h), n(h)), format!("M 50 0 L {} 100 H 100 V {} L 50 70 L 0 {} V 100 H {} Z", n(50.0 + w), n(h), n(h), n(50.0 - w)), Some(180.0))
        }
        _ => return None,
    })
}

/// badge.js `hsl2rgb(H, S, L)` → `rrggbb`.
fn hsl2rgb(h: f64, s: f64, l: f64) -> String {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_ = h / 60.0;
    let x = c * (1.0 - ((h_ % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match h_ {
        h if (0.0..1.0).contains(&h) => (c, x, 0.0),
        h if (1.0..2.0).contains(&h) => (x, c, 0.0),
        h if (2.0..3.0).contains(&h) => (0.0, c, x),
        h if (3.0..4.0).contains(&h) => (0.0, x, c),
        h if (4.0..5.0).contains(&h) => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    // Math.round: floor(x + 0.5)
    let round = |v: f64| ((v + m) * 255.0 + 0.5).floor() as i64;
    format!("{:02x}{:02x}{:02x}", round(r1), round(g1), round(b1))
}

/// The 80-colour badge palette (palette index → `#rrggbb`).
pub fn palette() -> Vec<String> {
    let mut colors = Vec::with_capacity(80);
    for (s, l) in [(0.6, 0.8), (0.7, 0.5), (0.4, 0.3), (0.5, 0.1)] {
        colors.push(format!("#{}", hsl2rgb(0.0, 0.0, l)));
        for i in 0..19 {
            colors.push(format!("#{}", hsl2rgb(i as f64 * 360.0 / 19.0, s, l)));
        }
    }
    colors
}

/// `getBadgeSvg(badgeInfo, border)`; None where the original throws (unknown type, bad palette index).
pub fn get_badge_svg(badge: &Json, border: bool) -> Option<String> {
    let pal = palette();
    let color = |k: &str| -> Option<String> {
        match badge.get(k) {
            Some(Json::Str(s)) => Some(s.clone()),
            v => {
                // colors[v].rgb: property lookup by ToString(v)
                let key = crate::socket::user::js_to_string(v);
                engine::doc::array_index(&key).and_then(|i| pal.get(i as usize)).cloned()
            }
        }
    };
    let (c1, c2, c3) = (color("color1")?, color("color2")?, color("color3")?);
    let param = match badge.get("param") {
        Some(Json::Num(p)) => *p,
        v => crate::api::user::to_number(v),
    };
    let (path1, path2, rotate) = match badge.get("type") {
        Some(Json::Num(t)) => {
            let ty = (t.fract() == 0.0 && *t >= 1.0).then_some(*t as u32)?;
            let (p1, p2, flip) = paths(ty, param)?;
            let flip_on = badge.get("flip").is_some_and(|f| crate::truthy(Some(&engine::doc::Val::from_json(f))));
            (p1, p2, if flip_on { flip.unwrap_or(0.0) } else { 0.0 })
        }
        Some(t @ Json::Obj(_)) => {
            let s = |k: &str| t.get(k).map(|v| crate::socket::user::js_to_string(Some(v)));
            let p2 = t.get("path2").filter(|v| crate::truthy(Some(&engine::doc::Val::from_json(v)))).map(|v| crate::socket::user::js_to_string(Some(v)));
            (s("path1").unwrap_or_else(|| "undefined".into()), p2.unwrap_or_default(), 0.0)
        }
        _ => return None,
    };
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"128\" height=\"128\" viewBox=\"0 0 100 100\" shape-rendering=\"geometricPrecision\">{}<defs>{}<clipPath id=\"clip\">{}<circle cx=\"50\" cy=\"50\" r=\"{}\" />{}<!--<rect x=\"0\" y=\"0\" width=\"100\" height=\"100\"/>-->{}</clipPath>{}</defs>{}<g transform=\"rotate({} 50 50)\">{}<rect x=\"0\" y=\"0\" width=\"100\" height=\"100\" fill=\"{}\" clip-path=\"url(#clip)\"/>",
        nl(28), nl(32), nl(36), if border { 48 } else { 52 }, nl(36), nl(32), nl(28), nl(28), n(rotate), nl(28), c1
    );
    svg += &format!("<path d=\"{path1}\" fill=\"{c2}\" clip-path=\"url(#clip)\"/>");
    if !path2.is_empty() {
        svg += &format!("<path d=\"{path2}\" fill=\"{c3}\" clip-path=\"url(#clip)\"/>");
    }
    if border {
        svg += "<circle cx=\"50\" cy=\"50\" r=\"47.5\" fill=\"transparent\" stroke=\"#000\" stroke-width=\"5\"></circle>";
    }
    svg += "</g></svg>";
    Some(svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Splits into number and non-number tokens.
    fn tokens(s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut num = false;
        for c in s.chars() {
            let is_num = c.is_ascii_digit() || c == '.' || c == 'e' && num || (c == '-' && !num);
            if is_num != num && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            num = is_num;
            cur.push(c);
        }
        out.push(cur);
        out
    }

    fn close(a: &str, b: &str) -> bool {
        let (ta, tb) = (tokens(a), tokens(b));
        ta.len() == tb.len()
            && ta.iter().zip(&tb).all(|(x, y)| {
                x == y || match (x.parse::<f64>(), y.parse::<f64>()) {
                    (Ok(p), Ok(q)) => (p - q).abs() <= 1e-12 * p.abs().max(1.0),
                    _ => false,
                }
            })
    }

    /// node: badge.getBadgeSvg over every type, param, flip and border (tests/fixtures/badge/svg.json).
    #[test]
    fn svgs_match_node() {
        let fx = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/badge/svg.json")).unwrap();
        let fx = Json::parse(&fx).unwrap();
        assert_eq!(Json::Arr(palette().into_iter().map(Json::Str).collect()), *fx.get("palette").unwrap());
        let (mut exact, mut total) = (0, 0);
        for c in fx.get("cases").unwrap().as_array().unwrap() {
            let (badge, border, want) = (c.get("badge").unwrap(), c.get("border").and_then(Json::as_bool).unwrap(), c.get("svg").and_then(Json::as_str).unwrap());
            let got = get_badge_svg(badge, border).unwrap_or_else(|| panic!("no svg for {badge}"));
            assert!(close(&got, want), "badge {badge} border {border}\n ours   {got}\n theirs {want}");
            exact += usize::from(got == want);
            total += 1;
        }
        // the rest differ only in trig last digits (libm vs fdlibm)
        assert!(total >= 200 && exact * 10 >= total * 8, "{exact}/{total} byte-exact");
    }
}
