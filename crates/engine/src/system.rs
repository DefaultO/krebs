//! mirror: reference/common/lib/system.js
//! parity: FAITHFUL
//!
//! Intent sanitizers for JSON inputs: API intents (add-object-intent, add-global-intent; ARCHITECTURE.md §4.4).
//! `sanitizeUserRoomIntents` only serves the runner's storeIntents, which runs in the system isolate
//! (`js/common/system.js`, §4.2); nothing in Rust groups room intents, so it is not ported here.
//!
//! Results are what the Loki JSON transport delivers to the processor (the backend `$merge`s them through the
//! storage RPC): `undefined` fields are absent, NaN/±Infinity are `null`, `-0` is `0`.
//! Fixtures: tools/oracle/gen/system-fixture.js → tests/fixtures/system/sanitize.ndjson.

use crate::constants::BodyPart;
use crate::doc::{Atom, Obj, Val};
use crate::live::{self, V};
use util::json::Json;

/// A `transforms` key of system.js (also the value type of `customIntentTypes`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldType {
    /// 'string'
    String,
    /// 'number'
    Number,
    /// 'boolean'
    Boolean,
    /// 'price'
    Price,
    /// 'string[]'
    StringArr,
    /// 'number[]'
    NumberArr,
    /// 'bodypart[]'
    BodypartArr,
    /// 'userString'
    UserString,
    /// 'userText'
    UserText,
}

/// One `intentTypes` entry: field → transform, in output order.
pub type IntentType = &'static [(&'static str, FieldType)];

/// `customIntentTypes` (mods, `hooks::ProcessorHooks::intent_types`), in key order.
pub type CustomIntentTypes<'a> = &'a [(&'static str, IntentType)];

use FieldType::{
    Boolean as B, BodypartArr as BP, Number as N, NumberArr as NA, Price as P, String as S, StringArr as SA,
    UserString as US, UserText as UT,
};

// vanilla: system.js:16 (declaration order is the output order)
pub const INTENT_TYPES: &[(&str, IntentType)] = &[
    ("notify", &[("message", UT), ("groupInterval", N)]),
    ("createConstructionSite", &[("roomName", S), ("x", N), ("y", N), ("structureType", S), ("name", US)]),
    ("createFlag", &[("roomName", S), ("x", N), ("y", N), ("name", US), ("color", N), ("secondaryColor", N)]),
    ("destroyStructure", &[("roomName", S), ("id", S)]),
    ("removeConstructionSite", &[("roomName", S), ("id", S)]),
    ("removeFlag", &[("roomName", S), ("name", US)]),
    ("cancelOrder", &[("orderId", S)]),
    ("changeOrderPrice", &[("orderId", S), ("newPrice", P)]),
    ("createOrder", &[("type", S), ("resourceType", S), ("price", P), ("totalAmount", N), ("roomName", S)]),
    ("createPowerCreep", &[("name", US), ("className", S)]),
    ("deal", &[("orderId", S), ("amount", N), ("targetRoomName", S)]),
    ("deletePowerCreep", &[("id", S), ("cancel", B)]),
    ("extendOrder", &[("orderId", S), ("addAmount", N)]),
    ("renamePowerCreep", &[("id", S), ("name", US)]),
    ("spawnPowerCreep", &[("id", S), ("name", US)]),
    ("suicidePowerCreep", &[("id", S)]),
    ("upgradePowerCreep", &[("id", S), ("power", N)]),
    ("activateSafeMode", &[]),
    ("attack", &[("id", S), ("x", N), ("y", N)]),
    ("attackController", &[("id", S)]),
    ("boostCreep", &[("id", S), ("bodyPartsCount", N)]),
    ("build", &[("id", S), ("x", N), ("y", N)]),
    ("cancelSpawning", &[]),
    ("claimController", &[("id", S)]),
    ("createCreep", &[("name", US), ("body", BP), ("energyStructures", SA), ("directions", NA)]),
    ("destroy", &[]),
    ("dismantle", &[("id", S)]),
    ("drop", &[("amount", N), ("resourceType", S)]),
    ("enableRoom", &[("id", S)]),
    ("generateSafeMode", &[("id", S)]),
    ("harvest", &[("id", S)]),
    ("heal", &[("id", S), ("x", N), ("y", N)]),
    ("launchNuke", &[("x", N), ("y", N), ("roomName", S)]),
    ("move", &[("id", S), ("direction", N)]),
    ("notifyWhenAttacked", &[("enabled", B)]),
    ("observeRoom", &[("roomName", S)]),
    ("pickup", &[("id", S)]),
    ("processPower", &[]),
    ("produce", &[("resourceType", S), ("amount", N)]),
    ("pull", &[("id", S)]),
    ("rangedAttack", &[("id", S)]),
    ("rangedHeal", &[("id", S)]),
    ("rangedMassAttack", &[]),
    ("recycleCreep", &[("id", S)]),
    ("renew", &[("id", S)]),
    ("renewCreep", &[("id", S)]),
    ("reverseReaction", &[("lab1", S), ("lab2", S)]),
    ("runReaction", &[("lab1", S), ("lab2", S)]),
    ("remove", &[]),
    ("repair", &[("id", S), ("x", N), ("y", N)]),
    ("reserveController", &[("id", S)]),
    ("say", &[("message", US), ("isPublic", B)]),
    ("send", &[("targetRoomName", S), ("resourceType", S), ("amount", N), ("description", US)]),
    ("setColor", &[("color", N), ("secondaryColor", N)]),
    ("setPosition", &[("x", N), ("y", N), ("roomName", S)]),
    ("setPublic", &[("isPublic", B)]),
    ("setSpawnDirections", &[("directions", NA)]),
    ("signController", &[("id", S), ("sign", US)]),
    ("suicide", &[]),
    ("transfer", &[("id", S), ("amount", N), ("resourceType", S)]),
    ("unboostCreep", &[("id", S)]),
    ("unclaim", &[]),
    ("upgradeController", &[("id", S)]),
    ("usePower", &[("power", N), ("id", S)]),
    ("withdraw", &[("id", S), ("amount", N), ("resourceType", S)]),
];

/// `intentTypes[name] || customIntentTypes[name]`.
pub fn intent_type(name: &str, custom: CustomIntentTypes) -> Option<IntentType> {
    INTENT_TYPES.iter().chain(custom).find(|(n, _)| *n == name).map(|(_, t)| *t)
}

/// `sanitizeUserIntents(input, customIntentTypes)` (vanilla: system.js:95), e.g. `input = {[name]: intent}`.
/// `Err` carries the TypeError text vanilla throws (`input` or an array element that is `null`, an object with an
/// own `toString` key converted to a string or number).
// ponytail: custom names or fields that are Object.prototype members, array indices or `length` read inherited /
// array / string properties in JS; mods do not declare such names, so lookups are own keys of objects only.
pub fn sanitize_user_intents(input: &Json, custom: CustomIntentTypes) -> Result<Obj, String> {
    if matches!(input, Json::Null) {
        return Err(format!("TypeError: Cannot read properties of null (reading '{}')", INTENT_TYPES[0].0));
    }
    let mut intent_result = Obj::new();
    // Built-in names inside arrays are sanitized without customIntentTypes (system.js:100); the built-in schema
    // wins anyway, so both loops share one body.
    for &(name, _) in INTENT_TYPES.iter().chain(custom) {
        let Some(value) = get(input, name).filter(|v| truthy(v)) else { continue };
        let sanitized = match value {
            Json::Arr(items) => Val::Arr(
                items.iter().map(|i| sanitize_intent(name, i, custom).map(Val::Obj)).collect::<Result<_, _>>()?,
            ),
            _ => Val::Obj(sanitize_intent(name, value, custom)?),
        };
        intent_result.set(Atom::intern(name), sanitized);
    }
    Ok(intent_result)
}

// vanilla: system.js:84 — every declared field is emitted, present or not
pub fn sanitize_intent(name: &str, intent: &Json, custom: CustomIntentTypes) -> Result<Obj, String> {
    let mut result = Obj::new();
    for &(field, t) in intent_type(name, custom).unwrap_or(&[]) {
        if matches!(intent, Json::Null) {
            return Err(format!("TypeError: Cannot read properties of null (reading '{field}')"));
        }
        if let Some(v) = transform(t, get(intent, field))? {
            result.set(Atom::intern(field), v);
        }
    }
    Ok(result)
}

// vanilla: system.js:4. `None` in and out is `undefined`; `Err` is a TypeError of a string/number conversion.
fn transform(t: FieldType, value: Option<&Json>) -> Result<Option<Val>, String> {
    let user_string = |n| -> Result<Option<Val>, String> {
        let s = if truthy_opt(value) { js_string(value)? } else { String::new() };
        Ok(Some(Val::Str(substring(&s, n).into())))
    };
    Ok(Some(match t {
        FieldType::String => Val::Str(js_string(value)?.into()),
        FieldType::Number => number(parse_int(&js_string(value)?)),
        FieldType::Boolean => Val::Bool(truthy_opt(value)),
        FieldType::Price => number(parse_int(&util::num::to_fixed(1000.0 * to_number(value)?, 0))),
        FieldType::StringArr => return map_array(value, |i| Ok(Val::Str(js_string(Some(i))?.into()))),
        FieldType::NumberArr => return map_array(value, |i| Ok(number(parse_int(&js_string(Some(i))?)))),
        FieldType::BodypartArr => {
            let body_part = |i: &&Json| i.as_str().is_some_and(|s| s.parse::<BodyPart>().is_ok());
            Val::Arr(lodash_each(value).filter(body_part).map(Val::from_json).collect())
        }
        FieldType::UserString => return user_string(100),
        FieldType::UserText => return user_string(1000),
    }))
}

/// The values lodash 3.10.1 `_.filter` visits (baseEach): an array's elements; an object with a valid numeric
/// `length` its index keys below it (missing ones are undefined and never kept); any other object its own
/// values; strings only single UTF-16 units (never a body part), other primitives nothing.
fn lodash_each(value: Option<&Json>) -> Box<dyn Iterator<Item = &Json> + '_> {
    match value {
        Some(Json::Arr(items)) => Box::new(items.iter()),
        Some(o @ Json::Obj(members)) => match get(o, "length") {
            Some(&Json::Num(len)) if len > -1.0 && len % 1.0 == 0.0 && len <= 9007199254740991.0 => {
                let below = move |k: &str| crate::doc::array_index(k).is_some_and(|i| (i as f64) < len);
                Box::new(members.iter().filter(move |(k, _)| below(k)).map(|(_, v)| v))
            }
            _ => Box::new(members.iter().map(|(_, v)| v)),
        },
        _ => Box::new(std::iter::empty()),
    }
}

fn get<'a>(o: &'a Json, key: &str) -> Option<&'a Json> {
    match o {
        Json::Obj(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// `_.isArray(value) ? _.map(value, f) : undefined`.
fn map_array(value: Option<&Json>, f: impl Fn(&Json) -> Result<Val, String>) -> Result<Option<Val>, String> {
    match value {
        Some(Json::Arr(a)) => Ok(Some(Val::Arr(a.iter().map(f).collect::<Result<_, _>>()?))),
        _ => Ok(None),
    }
}

/// Through the JSON transport: NaN/±Infinity become null, -0 becomes 0.
fn number(n: f64) -> Val {
    if n.is_finite() {
        Val::Num(if n == 0.0 { 0.0 } else { n })
    } else {
        Val::Null
    }
}

fn parse_int(s: &str) -> f64 {
    live::parse_int(&V::str(s))
}

fn truthy(v: &Json) -> bool {
    match v {
        Json::Null => false,
        Json::Bool(b) => *b,
        Json::Num(n) => *n != 0.0 && !n.is_nan(),
        Json::Str(s) => !s.is_empty(),
        Json::Arr(_) | Json::Obj(_) => true,
    }
}

fn truthy_opt(v: Option<&Json>) -> bool {
    v.is_some_and(truthy)
}

/// ToPrimitive of a JSON object with an own `toString` key: neither method is callable.
const NO_PRIMITIVE: &str = "TypeError: Cannot convert object to primitive value";

/// `"" + v`.
fn js_string(v: Option<&Json>) -> Result<String, String> {
    let mut out = String::new();
    match v {
        None => out.push_str("undefined"),
        Some(v) => write_js_string(&mut out, v)?,
    }
    Ok(out)
}

fn write_js_string(out: &mut String, v: &Json) -> Result<(), String> {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => util::num::write_js_string(out, *n),
        Json::Str(s) => out.push_str(s),
        Json::Obj(_) if get(v, "toString").is_some() => return Err(NO_PRIMITIVE.into()),
        Json::Obj(_) => out.push_str("[object Object]"),
        // Array.prototype.join: null elements are empty
        Json::Arr(items) => {
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if !matches!(x, Json::Null) {
                    write_js_string(out, x)?;
                }
            }
        }
    }
    Ok(())
}

/// JS ToNumber.
fn to_number(v: Option<&Json>) -> Result<f64, String> {
    Ok(match v {
        None => f64::NAN,
        Some(Json::Null) => 0.0,
        Some(Json::Bool(b)) => *b as u8 as f64,
        Some(Json::Num(n)) => *n,
        Some(Json::Str(s)) => util::num::string_to_number(s),
        Some(v) => util::num::string_to_number(&js_string(Some(v))?),
    })
}

/// `s.substring(0, n)` in UTF-16 units; a pair cut in half leaves a lone surrogate, which a Rust string holds
/// as U+FFFD (ARCHITECTURE.md §7.2 J6).
fn substring(s: &str, n: usize) -> String {
    let mut units = 0;
    let mut out = String::new();
    for c in s.chars() {
        let w = c.len_utf16();
        if units + w > n {
            if units < n {
                out.push('\u{FFFD}');
            }
            break;
        }
        units += w;
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MY_INTENT: IntentType = &[
        ("a", S), ("b", P), ("c", NA), ("d", BP), ("e", UT), ("f", B), ("g", US), ("h", SA), ("i", N),
    ];
    // Must match CUSTOM_SRC in tools/oracle/gen/system-fixture.js.
    const CUSTOM: CustomIntentTypes = &[("myIntent", MY_INTENT), ("move", &[("q", S)])];

    /// Every line of the official sanitizeUserIntents fixture (node tools/oracle/gen/system-fixture.js).
    #[test]
    fn fixtures() {
        let text = include_str!("../tests/fixtures/system/sanitize.ndjson");
        let mut n = 0;
        for (line_no, line) in text.lines().enumerate().skip(1) {
            let case = Json::parse(line).unwrap();
            let input = Json::parse(case.get("in").unwrap().as_str().unwrap()).unwrap();
            let custom = if case.get("custom").unwrap().as_bool().unwrap() { CUSTOM } else { &[] };
            let got = sanitize_user_intents(&input, custom).map(|o| Val::Obj(o).to_json().stringify());
            let want = match case.get("out") {
                Some(out) => Ok(Json::parse(out.as_str().unwrap()).unwrap().stringify()),
                None => Err(case.get("err").unwrap().as_str().unwrap().to_string()),
            };
            assert_eq!(got, want, "line {}: {}", line_no + 1, line);
            n += 1;
        }
        assert!(n > 1000, "{n} cases");
    }

    #[test]
    fn transforms() {
        let s = |t, j: &str| transform(t, Some(&Json::parse(j).unwrap())).unwrap().map(|v| v.to_json().stringify());
        assert_eq!(s(N, "1e21").as_deref(), Some("1"));
        assert_eq!(s(N, "5e-7").as_deref(), Some("5"));
        assert_eq!(s(N, "-0.5").as_deref(), Some("0"));
        assert_eq!(s(P, "0.0004").as_deref(), Some("0"));
        assert_eq!(s(P, "1.0005").as_deref(), Some("1001"));
        assert_eq!(s(S, "[1,[2,null],{}]").as_deref(), Some("\"1,2,,[object Object]\""));
        assert_eq!(s(BP, r#"{"length":2,"0":"move","1":"x","2":"work"}"#).as_deref(), Some(r#"["move"]"#));
        assert_eq!(s(SA, "{}"), None);
        assert_eq!(transform(S, None), Ok(Some(Val::Str("undefined".into()))));
        assert_eq!(transform(N, None), Ok(Some(Val::Null)));
        assert_eq!(transform(P, Some(&Json::parse(r#"[{"toString":1}]"#).unwrap())), Err(NO_PRIMITIVE.into()));
        assert_eq!(substring("ab\u{1F600}", 3), "ab\u{FFFD}");
    }
}
