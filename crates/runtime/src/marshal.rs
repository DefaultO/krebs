//! RunInput → V8 (external one-byte/two-byte strings, zero copy), outMessage → ValueSerializer → system
//! isolate `__store` → RunOutput (ARCHITECTURE.md §4.2, §6.6). Owner: WP08.

use crate::natives::{error_text, js_str};
use crate::natives_world::set;
use engine::doc::{Obj, Val};
use engine::runner::{Module, RunInput, RunOutput, RunStatus};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Arc;
use util::json::Json;
use util::jsstr::JsStr;

/// Bytes of Rust buffers currently referenced by V8 external strings (governor input, §6.11).
pub(crate) static LIVE_EXTERNAL_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Non-vanilla safety caps (§4.5), reported as RUN-TOTAL-ERROR.
const MAX_OUT_MESSAGE: usize = 32 << 20;
const MAX_CONSOLE: usize = 1 << 20;
pub(crate) const TOO_LARGE: &str = "intents too large";

unsafe extern "C" fn drop_onebyte(p: *mut std::ffi::c_char, len: usize) {
    LIVE_EXTERNAL_BYTES.fetch_sub(len, Relaxed);
    // SAFETY: `p` came from `Arc::<[u8]>::into_raw` (or an `Arc<str>`, same layout) of length `len`.
    drop(unsafe { Arc::<[u8]>::from_raw(std::ptr::slice_from_raw_parts(p as *const u8, len)) });
}

unsafe extern "C" fn drop_twobyte(p: *mut u16, len: usize) {
    LIVE_EXTERNAL_BYTES.fetch_sub(len * 2, Relaxed);
    // SAFETY: `p` came from `Arc::<[u16]>::into_raw` of length `len`.
    drop(unsafe { Arc::<[u16]>::from_raw(std::ptr::slice_from_raw_parts(p as *const u16, len)) });
}

/// A one-byte string over the Arc's bytes (Latin-1); V8 holds one strong count until its GC frees it.
pub(crate) fn external_latin1<'s>(scope: &mut v8::PinScope<'s, '_>, bytes: &Arc<[u8]>) -> v8::Local<'s, v8::String> {
    let len = bytes.len();
    let p = Arc::into_raw(bytes.clone()) as *const u8 as *mut std::ffi::c_char;
    LIVE_EXTERNAL_BYTES.fetch_add(len, Relaxed);
    // SAFETY: the buffer stays valid until `drop_onebyte` releases the count taken above.
    unsafe { v8::String::new_external_onebyte_raw(scope, p, len, drop_onebyte) }.expect("external string")
}

/// UTF-8 text: shared when ASCII (the blob/event-log contract), copied otherwise.
pub(crate) fn text<'s>(scope: &mut v8::PinScope<'s, '_>, s: &Arc<str>) -> v8::Local<'s, v8::String> {
    if s.is_ascii() {
        // SAFETY: str and [u8] share layout; `drop_onebyte` rebuilds it as Arc<[u8]>.
        let bytes: Arc<[u8]> = unsafe { Arc::from_raw(Arc::into_raw(s.clone()) as *const [u8]) };
        let out = external_latin1(scope, &bytes);
        drop(bytes);
        out
    } else {
        v8::String::new(scope, s).expect("string")
    }
}

pub(crate) fn js_string<'s>(scope: &mut v8::PinScope<'s, '_>, s: &JsStr) -> v8::Local<'s, v8::String> {
    match s {
        JsStr::Latin1(b) => external_latin1(scope, b),
        JsStr::Utf16(u) => {
            let len = u.len();
            let p = Arc::into_raw(u.clone()) as *const u16 as *mut u16;
            LIVE_EXTERNAL_BYTES.fetch_add(len * 2, Relaxed);
            // SAFETY: as in `external_latin1`.
            unsafe { v8::String::new_external_twobyte_raw(scope, p, len, drop_twobyte) }.expect("external string")
        }
    }
}

/// Owned text without a copy when ASCII (the per-user meta JSON).
fn owned<'s>(scope: &mut v8::PinScope<'s, '_>, s: String) -> v8::Local<'s, v8::String> {
    if s.is_ascii() {
        v8::String::new_external_onebyte(scope, s.into_bytes().into_boxed_slice()).expect("string")
    } else {
        v8::String::new(scope, &s).expect("string")
    }
}

/// `__tick(meta, blobs, eventLogs, memory|undefined, segments, foreignSegment|undefined,
/// modules|undefined, plans|undefined)` arguments (§6.7; plans: doc_reuse.rs).
pub(crate) fn tick_args<'s>(scope: &mut v8::PinScope<'s, '_>, input: RunInput, plans: Option<String>, pass_memory: bool, pass_code: bool) -> [v8::Local<'s, v8::Value>; 8] {
    let undefined: v8::Local<v8::Value> = v8::undefined(scope).into();
    let meta = owned(scope, input.meta).into();
    let blobs = v8::Array::new(scope, input.blobs.len() as i32);
    for (i, b) in input.blobs.iter().enumerate() {
        let v: v8::Local<v8::Value> = match b {
            Some(b) => {
                debug_assert!(b.json.is_ascii(), "room blobs are ASCII (§3.4)");
                external_latin1(scope, &b.json).into()
            }
            None => v8::null(scope).into(),
        };
        blobs.set_index(scope, i as u32, v);
    }
    let logs = v8::Array::new(scope, input.event_logs.len() as i32);
    for (i, l) in input.event_logs.iter().enumerate() {
        let s = text(scope, l);
        logs.set_index(scope, i as u32, s.into());
    }
    let memory = if pass_memory { js_string(scope, &input.memory).into() } else { undefined };
    let segments = v8::Object::new(scope);
    for (id, data) in &input.segments {
        let s = js_string(scope, data);
        let k = v8::Integer::new_from_unsigned(scope, *id as u32);
        segments.set(scope, k.into(), s.into());
    }
    let foreign = match &input.foreign_segment {
        Some((username, id, data)) => {
            let o = v8::Object::new(scope);
            let u = v8::String::new(scope, username).unwrap();
            set(scope, o, "username", u.into());
            let n = v8::Number::new(scope, *id as f64);
            set(scope, o, "id", n.into());
            let d = js_string(scope, data);
            set(scope, o, "data", d.into());
            o.into()
        }
        None => undefined,
    };
    let modules = if pass_code {
        let o = v8::Object::new(scope);
        for (name, m) in &input.code.modules {
            let v: v8::Local<v8::Value> = match m {
                Module::Text(t) => text(scope, t).into(),
                // Copied: player code may write to its buffer.
                Module::Binary(b) => crate::natives::uint8_array(scope, b.to_vec()).expect("Uint8Array").into(),
            };
            // Own data properties, as vanilla's ExternalCopy of `userCode` builds them: a module named
            // `__proto__` stays a module instead of hitting the prototype setter.
            let k = v8::String::new(scope, name).unwrap();
            o.create_data_property(scope, k.into(), v);
        }
        o.into()
    } else {
        undefined
    };
    let plans = plans.map_or(undefined, |p| owned(scope, p).into());
    [meta, blobs.into(), logs.into(), memory, segments.into(), foreign, modules, plans]
}

/// What leaves the player isolate.
pub(crate) struct Captured {
    /// The ValueSerializer bytes of outMessage (minus the strings read directly).
    pub bytes: Vec<u8>,
    /// `outMessage.memory` when the run produced a new Memory string.
    pub memory: Option<JsStr>,
    pub segments: Vec<(u8, JsStr)>,
}

struct Serializer;

impl v8::ValueSerializerImpl for Serializer {
    /// isolated-vm's ExternalCopy throws a TypeError (`… could not be cloned.`).
    fn throw_data_clone_error<'s>(&self, scope: &mut v8::PinScope<'s, '_>, message: v8::Local<'s, v8::String>) {
        let e = v8::Exception::type_error(scope, message);
        scope.throw_exception(e);
    }
    fn get_wasm_module_transfer_id(&self, _scope: &mut v8::PinScope<'_, '_>, _module: v8::Local<v8::WasmModuleObject>) -> Option<u32> {
        None
    }
}

struct Deserializer;
impl v8::ValueDeserializerImpl for Deserializer {}

fn get<'s>(scope: &mut v8::PinScope<'s, '_>, o: v8::Local<v8::Object>, key: &str) -> Option<v8::Local<'s, v8::Value>> {
    let k = v8::String::new(scope, key).unwrap();
    o.get(scope, k.into())
}

/// §4.2 steps 1-2: Memory and segments are read as strings (cloning a string cannot fail, so skipping
/// them in the copy is equivalent), then outMessage is serialized. `Err` = RUN-TOTAL-ERROR text.
pub(crate) fn capture(scope: &mut v8::PinScope, out: v8::Local<v8::Value>) -> Result<Captured, String> {
    v8::tc_scope!(let tc, scope);
    let Ok(o) = v8::Local::<v8::Object>::try_from(out) else { return Err("invalid outMessage".into()) };
    let mut memory = None;
    if let Some(m) = get(tc, o, "memory") {
        // A string (§6.8), or vanilla's `{data, userId}`.
        let m = match v8::Local::<v8::Object>::try_from(m) {
            Ok(mo) if !m.is_string() => get(tc, mo, "data"),
            _ => Some(m),
        };
        if let Some(s) = m.and_then(|m| v8::Local::<v8::String>::try_from(m).ok()) {
            memory = Some(js_str(tc, s));
        }
    }
    let mut segments = Vec::new();
    if let Some(segs) = get(tc, o, "memorySegments").and_then(|s| v8::Local::<v8::Object>::try_from(s).ok()) {
        if let Some(keys) = segs.get_own_property_names(tc, Default::default()) {
            for i in 0..keys.length() {
                let Some(k) = keys.get_index(tc, i) else { continue };
                let id = k.to_rust_string_lossy(tc).parse::<u8>();
                let v = segs.get(tc, k).and_then(|v| v8::Local::<v8::String>::try_from(v).ok());
                if let (Ok(id), Some(v)) = (id, v) {
                    segments.push((id, js_str(tc, v)));
                }
            }
        }
    }
    let undefined = v8::undefined(tc).into();
    for k in ["memory", "memorySegments"] {
        set(tc, o, k, undefined);
    }
    if let Some(e) = tc.exception() {
        return Err(error_text(tc, e));
    }
    let ser = v8::ValueSerializer::new(tc, Box::new(Serializer));
    use v8::ValueSerializerHelper;
    ser.write_header();
    let context = tc.get_current_context();
    if ser.write_value(context, out).is_none() {
        let e = tc.exception().map(|e| error_text(tc, e)).unwrap_or_else(|| "could not be cloned".into());
        return Err(e);
    }
    let bytes = ser.release();
    if bytes.len() > MAX_OUT_MESSAGE {
        return Err(TOO_LARGE.into());
    }
    Ok(Captured { bytes, memory, segments })
}

/// §4.2 step 4: deserialize in the system isolate and call `__store(out, userId)`.
pub(crate) fn store(scope: &mut v8::PinScope, store_fn: v8::Local<v8::Function>, bytes: &[u8], user_id: &str) -> Result<String, String> {
    v8::tc_scope!(let tc, scope);
    let de = v8::ValueDeserializer::new(tc, Box::new(Deserializer), bytes);
    let context = tc.get_current_context();
    use v8::ValueDeserializerHelper;
    let out = de.read_header(context).and_then(|_| de.read_value(context));
    drop(de);
    let Some(out) = out else {
        return Err(tc.exception().map(|e| error_text(tc, e)).unwrap_or_else(|| "outMessage copy failed".into()));
    };
    let uid = v8::String::new(tc, user_id).unwrap();
    let recv = v8::undefined(tc).into();
    match store_fn.call(tc, recv, &[out, uid.into()]) {
        Some(r) if r.is_string() => Ok(r.to_rust_string_lossy(tc)),
        Some(r) => Err(format!("__store returned {}", r.to_rust_string_lossy(tc))),
        None => Err(tc.exception().map(|e| error_text(tc, e)).unwrap_or_else(|| "__store failed".into())),
    }
}

/// A run whose output is discarded (RUN-TOTAL-ERROR, resets).
pub(crate) fn failed(user: engine::world::UserIdx, status: RunStatus, error: String) -> RunOutput {
    RunOutput {
        user,
        status,
        error: Some(error),
        intents: None,
        memory: None,
        segments: Vec::new(),
        seg_settings: None,
        inter_shard: None,
        console_log: Vec::new(),
        console_results: Vec::new(),
        visuals: Vec::new(),
        used_time: 0,
        used_clean_ms: 0.0,
        used_dirty_ms: 0,
    }
}

/// The bytes capture() makes of a value (store.rs tests).
#[cfg(test)]
pub(crate) fn serialize_for_tests(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Vec<u8> {
    let ser = v8::ValueSerializer::new(scope, Box::new(Serializer));
    use v8::ValueSerializerHelper;
    ser.write_header();
    let context = scope.get_current_context();
    ser.write_value(context, value).unwrap();
    ser.release()
}

fn segment_id(v: &Json) -> bool {
    v.as_f64().is_some_and(|n| n.fract() == 0.0 && (0.0..=99.0).contains(&n))
}

/// What a run may leave of a segment setting, null included; None leaves the setting as it is. runtime.js checks ids
/// with the bot's own `parseInt` and `_`, and `setActiveForeignSegment` not the name at all, and this goes into the user
/// doc after the CPU meter has stopped, so any other shape is a size or a set of keys the bot picked.
fn segment_setting(key: &str, v: &Json) -> Option<Val> {
    const MAX_USERNAME: usize = 256; // above valid_username's: a world may hold older names
    const MAX_PUBLIC: usize = 1024; // 100 ids of two digits and a comma, listed a few times over
    if matches!(v, Json::Null) {
        return Some(Val::Null);
    }
    match key {
        "activeSegments" => v.as_array().filter(|a| a.len() <= 10 && a.iter().all(segment_id)).map(|_| Val::from_json(v)),
        "defaultPublicSegment" => segment_id(v).then(|| Val::from_json(v)),
        "publicSegments" => v.as_str().filter(|s| s.len() <= MAX_PUBLIC && s.bytes().all(|c| c.is_ascii_digit() || c == b',')).map(|_| Val::from_json(v)),
        "activeForeignSegment" => {
            let username = v.get("username");
            let id = v.get("id");
            let good = v.as_object().is_some()
                && username.is_none_or(|u| u.as_str().is_some_and(|u| u.len() <= MAX_USERNAME))
                && id.is_none_or(segment_id);
            if !good {
                return Some(Val::Null);
            }
            let mut o = Obj::default();
            for (k, v) in [("username", username), ("id", id)] {
                if let Some(v) = v {
                    o.set(engine::doc::Atom::intern(k), Val::from_json(v));
                }
            }
            Some(Val::Obj(o))
        }
        _ => None,
    }
}

fn segment_settings(j: &Json) -> Obj {
    let mut settings = Obj::default();
    for k in ["activeSegments", "activeForeignSegment", "defaultPublicSegment", "publicSegments"] {
        if let Some(v) = j.get(k).and_then(|v| segment_setting(k, v)) {
            settings.set(engine::doc::Atom::intern(k), v);
        }
    }
    settings
}

/// §4.2 steps 5-6: the `__store` JSON plus the strings read directly → RunOutput.
pub(crate) fn output(
    world: &dyn crate::WorldAccess,
    user: engine::world::UserIdx,
    j: Json,
    cap: Captured,
    meter: crate::Meter,
) -> RunOutput {
    let strings = |k: &str| -> Vec<String> {
        let list = j.get("console").and_then(|c| c.get(k)).and_then(Json::as_array);
        list.map(|l| {
            l.iter()
                .filter_map(|m| m.as_str().or_else(|| m.get("message").and_then(Json::as_str)).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
    };
    let (console_log, console_results) = (strings("log"), strings("results"));
    if console_log.iter().chain(&console_results).map(String::len).sum::<usize>() > MAX_CONSOLE {
        return failed(user, RunStatus::TotalError, TOO_LARGE.into());
    }
    let num = |k: &str| j.get(k).and_then(Json::as_f64).unwrap_or(0.0);
    let intents = world.parse_intents(&j);
    let (mut used_time, mut used_clean_ms, mut used_dirty_ms) = (num("usedTime") as u32, num("usedCleanTime"), num("usedDirtyTime") as u32);
    if let crate::Meter::Fixed { ms_per_tick } = meter {
        // Reproducible billing (§6.10): ms_per_tick + 0.2 per intent.
        used_clean_ms = ms_per_tick;
        used_time = (ms_per_tick + 0.2 * intents.count as f64).ceil() as u32;
        used_dirty_ms = ms_per_tick.ceil() as u32;
    }
    let settings = segment_settings(&j);
    let visuals = j
        .get("visual")
        .and_then(Json::as_object)
        .map(|v| v.iter().filter_map(|(room, s)| Some((room.as_str().into(), Arc::from(s.as_str()?)))).collect())
        .unwrap_or_default();
    RunOutput {
        user,
        status: if j.get("type").and_then(Json::as_str) == Some("error") { RunStatus::PartialError } else { RunStatus::Done },
        error: j.get("error").and_then(Json::as_str).map(str::to_string),
        intents: Some(intents),
        memory: cap.memory,
        segments: cap.segments,
        seg_settings: (!settings.is_empty()).then_some(Val::Obj(settings)),
        inter_shard: None,
        console_log,
        console_results,
        visuals,
        used_time,
        used_clean_ms,
        used_dirty_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(json: &str) -> String {
        Val::Obj(segment_settings(&Json::parse(json).unwrap())).to_json().stringify()
    }

    /// What a legal bot sends passes as it was, null and missing keys included.
    #[test]
    fn legal_segment_settings_pass() {
        for s in [
            r#"{"activeSegments":[1,2,99,0],"activeForeignSegment":{"username":"Other","id":3},"defaultPublicSegment":5,"publicSegments":"1,2"}"#,
            r#"{"activeSegments":[],"activeForeignSegment":null,"defaultPublicSegment":null,"publicSegments":""}"#,
            r#"{"activeForeignSegment":{"username":"Other"}}"#,
            r#"{"activeForeignSegment":{"id":4}}"#,
            r#"{"activeForeignSegment":{}}"#,
            r#"{"activeSegments":null}"#,
        ] {
            assert_eq!(settings(s), s);
        }
    }

    /// `setActiveForeignSegment(obj)` put any object in the user doc, and runtime.js checks the rest with functions
    /// the bot can replace.
    #[test]
    fn other_segment_settings_do_not_pass() {
        let junk = |k: &str, v: &str| settings(&format!(r#"{{"{k}":{v}}}"#));
        for v in [r#"{"username":{"a":1,"b":2}}"#, r#"{"username":5}"#, r#"{"username":"x","id":100}"#, r#"{"username":"x","id":-1}"#, r#"{"username":"x","id":1.5}"#, r#"{"username":"x","id":"3"}"#, "[1]", r#""x""#] {
            assert_eq!(junk("activeForeignSegment", v), r#"{"activeForeignSegment":null}"#, "{v}");
        }
        let long = format!(r#"{{"username":"{}"}}"#, "n".repeat(257));
        assert_eq!(junk("activeForeignSegment", &long), r#"{"activeForeignSegment":null}"#);
        assert_eq!(junk("activeForeignSegment", r#"{"username":"x","id":3,"more":{"k":1}}"#), r#"{"activeForeignSegment":{"username":"x","id":3}}"#);
        for v in ["5", "{}", r#"[1,null]"#, r#"[100]"#, r#"[[1]]"#, r#"[1,2,3,4,5,6,7,8,9,10,11]"#] {
            assert_eq!(junk("activeSegments", v), "{}", "{v}");
        }
        for v in ["100", "-1", "1.5", r#""1""#, "{}"] {
            assert_eq!(junk("defaultPublicSegment", v), "{}", "{v}");
        }
        for v in ["5", "{}", r#""1,x""#] {
            assert_eq!(junk("publicSegments", v), "{}", "{v}");
        }
        assert_eq!(junk("publicSegments", &format!(r#""{}""#, "1,".repeat(600))), "{}");
    }

    /// Two isolates read the very bytes of one Arc (no copy), and disposal gives the counts back.
    #[test]
    fn external_strings_share_the_arc() {
        crate::init_v8();
        let bytes: Arc<[u8]> = Arc::from(&b"{\"room\":{},\"seq\":[],\"objects\":[]}"[..]);
        let utf16: Arc<[u16]> = Arc::from(&[0x61u16, 0xd800][..]);
        let pointers = |iso: &mut v8::OwnedIsolate| {
            v8::scope!(let scope, iso);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            let latin1 = external_latin1(scope, &bytes);
            let two = js_string(scope, &JsStr::Utf16(utf16.clone()));
            let data = latin1.get_external_onebyte_string_resource().map(|r| unsafe { r.as_ref() }.data() as *const u8);
            (data, latin1.is_external_onebyte(), two.is_external_twobyte(), two.length())
        };
        // Owned isolates nest: each is used while it is the entered one.
        let mut a = v8::Isolate::new(Default::default());
        let pa = pointers(&mut a);
        let mut b = v8::Isolate::new(Default::default());
        let pb = pointers(&mut b);
        assert_eq!(pa, (Some(bytes.as_ptr()), true, true, 2));
        assert_eq!(pb, pa);
        assert_eq!((Arc::strong_count(&bytes), Arc::strong_count(&utf16)), (3, 3));
        drop(b);
        drop(a);
        assert_eq!((Arc::strong_count(&bytes), Arc::strong_count(&utf16)), (1, 1));
    }
}
