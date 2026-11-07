//! JSON values with JS semantics: `Json::parse` behaves like `JSON.parse` and
//! `stringify` produces exactly what `JSON.stringify` produces for the same
//! JS value.
//!
//! Objects keep JS property order. The parser applies it (a repeated key keeps
//! its first position and takes the last value; array-index keys come first in
//! ascending order); the serializer writes members in `Vec` order, so code that
//! builds objects by hand is responsible for that order.
//!
//! Known difference: Rust strings cannot hold lone surrogates, so a `\uD800`
//! escape without its pair parses to U+FFFD.

use crate::num;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

/// Nesting deeper than this is rejected (parse) or written as `null`
/// (stringify of hand-built values) so recursion cannot overflow the stack.
pub const MAX_DEPTH: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset into the input where parsing failed.
    pub offset: usize,
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

impl Json {
    pub fn parse(text: &str) -> Result<Json, ParseError> {
        let mut p = Parser { s: text, b: text.as_bytes(), pos: 0, depth: 0, items: Vec::new(), members: Vec::new(), check_only: false };
        let v = p.value()?;
        p.skip_ws();
        if p.pos != p.b.len() {
            return Err(p.err("unexpected data after JSON value"));
        }
        Ok(v)
    }

    /// Whether `Json::parse(text)` would give an object or an array. It builds nothing, so checking a large body costs
    /// next to no memory, and what handles the body parses it once.
    pub fn is_container(text: &str) -> bool {
        let mut p = Parser { s: text, b: text.as_bytes(), pos: 0, depth: 0, items: Vec::new(), members: Vec::new(), check_only: true };
        p.skip_ws();
        if !matches!(p.peek(), Some(b'{' | b'[')) || p.value().is_err() {
            return false;
        }
        p.skip_ws();
        p.pos == p.b.len()
    }

    /// `JSON.stringify(value)`.
    pub fn stringify(&self) -> String {
        let mut out = String::new();
        write_value(&mut out, self, "", 0);
        out
    }

    /// `JSON.stringify(value, null, indent)`; like JS, indent is capped at 10
    /// and 0 means compact.
    pub fn stringify_pretty(&self, indent: usize) -> String {
        let gap = " ".repeat(indent.min(10));
        let mut out = String::new();
        write_value(&mut out, self, &gap, 0);
        out
    }

    /// Object member by key.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        match self {
            Json::Obj(m) => m.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Array element by index.
    pub fn at(&self, i: usize) -> Option<&Json> {
        match self {
            Json::Arr(a) => a.get(i),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Json::Bool(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Json::Num(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Vec<(String, Json)>> {
        match self {
            Json::Obj(m) => Some(m),
            _ => None,
        }
    }
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.stringify())
    }
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    pos: usize,
    depth: usize,
    /// Scratch stacks: elements collect here and move into an exactly sized
    /// Vec when their container closes, instead of regrowing a Vec per container.
    items: Vec<Json>,
    members: Vec<(String, Json)>,
    /// `is_container`: the text is walked the same way, but nothing is kept.
    check_only: bool,
}

impl Parser<'_> {
    fn err(&self, message: &'static str) -> ParseError {
        ParseError { offset: self.pos, message }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Result<Json, ParseError> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("unexpected character")),
            None => Err(self.err("unexpected end of input")),
        }
    }

    fn literal(&mut self, word: &str, v: Json) -> Result<Json, ParseError> {
        if self.b[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(v)
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        self.pos += 1;
        Ok(())
    }

    fn array(&mut self) -> Result<Json, ParseError> {
        self.enter()?;
        let base = self.items.len();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
        } else {
            loop {
                let v = self.value()?;
                if !self.check_only {
                    self.items.push(v);
                }
                self.skip_ws();
                match self.peek() {
                    Some(b',') => self.pos += 1,
                    Some(b']') => {
                        self.pos += 1;
                        break;
                    }
                    _ => return Err(self.err("expected ',' or ']'")),
                }
            }
        }
        self.depth -= 1;
        Ok(Json::Arr(self.items.drain(base..).collect()))
    }

    fn object(&mut self) -> Result<Json, ParseError> {
        self.enter()?;
        let base = self.members.len();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
        } else {
            loop {
                self.skip_ws();
                if self.peek() != Some(b'"') {
                    return Err(self.err("expected string key"));
                }
                let key = self.string()?;
                self.skip_ws();
                if self.peek() != Some(b':') {
                    return Err(self.err("expected ':'"));
                }
                self.pos += 1;
                let v = self.value()?;
                if !self.check_only {
                    self.members.push((key, v));
                }
                self.skip_ws();
                match self.peek() {
                    Some(b',') => self.pos += 1,
                    Some(b'}') => {
                        self.pos += 1;
                        break;
                    }
                    _ => return Err(self.err("expected ',' or '}'")),
                }
            }
        }
        self.depth -= 1;
        Ok(Json::Obj(js_property_order(self.members.drain(base..).collect())))
    }

    fn number(&mut self) -> Result<Json, ParseError> {
        let start = self.pos;
        let neg = self.peek() == Some(b'-');
        if neg {
            self.pos += 1;
        }
        let mut mant: u64 = 0;
        let mut ndigits = 0;
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                while let Some(c @ b'0'..=b'9') = self.peek() {
                    mant = mant.wrapping_mul(10).wrapping_add((c - b'0') as u64);
                    ndigits += 1;
                    self.pos += 1;
                }
            }
            _ => return Err(self.err("invalid number")),
        }
        let mut int = true;
        if self.peek() == Some(b'.') {
            self.pos += 1;
            self.digits()?;
            int = false;
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.pos += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
            }
            self.digits()?;
            int = false;
        }
        // Up to 19 digits fit a u64 exactly and `as f64` rounds correctly.
        let v = if int && ndigits <= 19 {
            mant as f64
        } else {
            // std's parser is correctly rounded; the grammar was checked above.
            let v: f64 = self.s[start..self.pos].parse().map_err(|_| self.err("invalid number"))?;
            return Ok(Json::Num(v));
        };
        Ok(Json::Num(if neg { -v } else { v }))
    }

    fn digits(&mut self) -> Result<(), ParseError> {
        let start = self.pos;
        while let Some(b'0'..=b'9') = self.peek() {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(self.err("invalid number"));
        }
        Ok(())
    }

    /// Parses a string literal starting at the opening quote.
    fn string(&mut self) -> Result<String, ParseError> {
        self.pos += 1;
        let start = self.pos;
        // Fast path: no escapes. Stopping only at ASCII bytes keeps slices on
        // char boundaries.
        let b = self.b;
        let mut i = start;
        while i < b.len() && !STRING_STOP[b[i] as usize] {
            i += 1;
        }
        self.pos = i;
        match b.get(i) {
            Some(b'"') => {
                self.pos += 1;
                return Ok(if self.check_only { String::new() } else { self.s[start..i].to_owned() });
            }
            Some(b'\\') => {}
            Some(_) => return Err(self.err("control character in string")),
            None => return Err(self.err("unterminated string")),
        }
        let mut out = String::with_capacity(self.pos - start + 16);
        out.push_str(&self.s[start..self.pos]);
        loop {
            let run = self.pos;
            loop {
                match self.peek() {
                    Some(b'"' | b'\\') => break,
                    Some(0..=0x1f) => return Err(self.err("control character in string")),
                    Some(_) => self.pos += 1,
                    None => return Err(self.err("unterminated string")),
                }
            }
            out.push_str(&self.s[run..self.pos]);
            if self.b[self.pos] == b'"' {
                self.pos += 1;
                return Ok(out);
            }
            self.pos += 1; // backslash
            let c = match self.peek() {
                Some(b'"') => '"',
                Some(b'\\') => '\\',
                Some(b'/') => '/',
                Some(b'b') => '\u{8}',
                Some(b'f') => '\u{c}',
                Some(b'n') => '\n',
                Some(b'r') => '\r',
                Some(b't') => '\t',
                Some(b'u') => {
                    self.pos += 1;
                    let hi = self.hex4()?;
                    let c = if (0xd800..0xdc00).contains(&hi) && self.b[self.pos..].starts_with(b"\\u") {
                        let save = self.pos;
                        self.pos += 2;
                        let lo = self.hex4()?;
                        if (0xdc00..0xe000).contains(&lo) {
                            char::from_u32(0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00))
                        } else {
                            self.pos = save; // the next escape stands on its own
                            None
                        }
                    } else {
                        char::from_u32(hi)
                    };
                    out.push(c.unwrap_or('\u{fffd}')); // lone surrogate
                    continue;
                }
                _ => return Err(self.err("invalid escape")),
            };
            self.pos += 1;
            out.push(c);
        }
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let mut v = 0;
        for _ in 0..4 {
            let d = self.peek().and_then(|c| (c as char).to_digit(16)).ok_or_else(|| self.err("invalid \\u escape"))?;
            v = v * 16 + d;
            self.pos += 1;
        }
        Ok(v)
    }
}

/// Bytes that end the fast scan of a string literal: '"', '\\' and controls.
const STRING_STOP: [bool; 256] = {
    let mut t = [false; 256];
    let mut i = 0;
    while i < 0x20 {
        t[i] = true;
        i += 1;
    }
    t[b'"' as usize] = true;
    t[b'\\' as usize] = true;
    t
};

/// JS property order for an object built by JSON.parse: CreateDataProperty
/// keeps a repeated key at its first position with the last value, and
/// OrdinaryOwnPropertyKeys lists array-index keys first, ascending.
fn js_property_order(mut m: Vec<(String, Json)>) -> Vec<(String, Json)> {
    if m.len() < 2 {
        return m;
    }
    if has_duplicate_keys(&m) {
        let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut out: Vec<(String, Json)> = Vec::with_capacity(m.len());
        for (k, v) in m {
            match index.get(&k) {
                Some(&i) => out[i].1 = v,
                None => {
                    index.insert(k.clone(), out.len());
                    out.push((k, v));
                }
            }
        }
        m = out;
    }
    // Usually already in order (no index keys, or ascending ones up front).
    let mut in_order = true;
    let mut last: Option<u32> = None;
    let mut named = false;
    for (k, _) in &m {
        match array_index(k) {
            Some(i) if named || last.is_some_and(|l| l > i) => {
                in_order = false;
                break;
            }
            Some(i) => last = Some(i),
            None => named = true,
        }
    }
    if !in_order {
        // Stable: non-index keys (all u64::MAX) keep insertion order.
        m.sort_by_cached_key(|(k, _)| array_index(k).map_or(u64::MAX, u64::from));
    }
    m
}

fn has_duplicate_keys(m: &[(String, Json)]) -> bool {
    if m.len() <= 16 {
        // Length and first byte reject almost every pair without a memcmp call.
        let same = |a: &str, b: &str| a.len() == b.len() && a.as_bytes().first() == b.as_bytes().first() && a == b;
        return m.iter().enumerate().any(|(i, (k, _))| m[..i].iter().any(|(k2, _)| same(k, k2)));
    }
    let mut seen = std::collections::HashSet::with_capacity(m.len());
    !m.iter().all(|(k, _)| seen.insert(k.as_str()))
}

/// Canonical array index ("0".."4294967294"), ECMA-262 6.1.7.
fn array_index(k: &str) -> Option<u32> {
    let b = k.as_bytes();
    if b.is_empty() || b.len() > 10 || !b.iter().all(u8::is_ascii_digit) || (b[0] == b'0' && b.len() > 1) {
        return None;
    }
    let v: u64 = k.parse().ok()?;
    (v < u32::MAX as u64).then_some(v as u32)
}

fn write_value(out: &mut String, v: &Json, gap: &str, depth: usize) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) if n.is_finite() => num::write_js_string(out, *n),
        Json::Num(_) => out.push_str("null"),
        Json::Str(s) => write_quoted(out, s),
        _ if depth >= MAX_DEPTH => out.push_str("null"),
        Json::Arr(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, gap, depth + 1);
                write_value(out, item, gap, depth + 1);
            }
            newline(out, gap, depth);
            out.push(']');
        }
        Json::Obj(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, item)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, gap, depth + 1);
                write_quoted(out, k);
                out.push(':');
                if !gap.is_empty() {
                    out.push(' ');
                }
                write_value(out, item, gap, depth + 1);
            }
            newline(out, gap, depth);
            out.push('}');
        }
    }
}

fn newline(out: &mut String, gap: &str, depth: usize) {
    if !gap.is_empty() {
        out.push('\n');
        for _ in 0..depth {
            out.push_str(gap);
        }
    }
}

/// QuoteJSONString (ECMA-262 25.5.2.3).
pub fn write_quoted(out: &mut String, s: &str) {
    out.push('"');
    let b = s.as_bytes();
    let mut run = 0;
    for (i, &c) in b.iter().enumerate() {
        let esc = match c {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            0x08 => "\\b",
            b'\t' => "\\t",
            b'\n' => "\\n",
            0x0c => "\\f",
            b'\r' => "\\r",
            0..=0x1f => "",
            _ => continue,
        };
        out.push_str(&s[run..i]);
        run = i + 1;
        if esc.is_empty() {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            out.push_str("\\u00");
            out.push(HEX[(c >> 4) as usize] as char);
            out.push(HEX[(c & 15) as usize] as char);
        } else {
            out.push_str(esc);
        }
    }
    out.push_str(&s[run..]);
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(s: &str) -> String {
        Json::parse(s).unwrap().stringify()
    }

    #[test]
    fn round_trips() {
        assert_eq!(rt(r#" {"a" : [1, 2.5, -0, 1e21, "x\u0001\n\"\\\/"], "b": null, "c": true} "#), r#"{"a":[1,2.5,0,1e+21,"x\u0001\n\"\\/"],"b":null,"c":true}"#);
        assert_eq!(rt(r#"{"b":1,"2":2,"a":3,"1":4,"b":5}"#), r#"{"1":4,"2":2,"b":5,"a":3}"#);
        assert_eq!(rt(r#""\ud83d\ude00 \ud800 \udc00x""#), "\"\u{1f600} \u{fffd} \u{fffd}x\"");
        assert_eq!(rt("123456789012345678901234567890"), "1.2345678901234568e+29");
    }

    #[test]
    fn pretty() {
        let v = Json::parse(r#"{"a":[1,{}],"b":[],"c":{"d":"e"}}"#).unwrap();
        assert_eq!(v.stringify_pretty(2), "{\n  \"a\": [\n    1,\n    {}\n  ],\n  \"b\": [],\n  \"c\": {\n    \"d\": \"e\"\n  }\n}");
        assert_eq!(v.stringify_pretty(0), v.stringify());
    }

    #[test]
    fn errors() {
        for bad in ["", " ", "01", "1.", ".5", "+1", "[1,]", "{,}", "{\"a\"}", "{\"a\":}", "1 2", "\"\t\"", "\"\\x\"", "\"\\u12\"", "tru", "nul", "[", "{\"a\":1", "\"abc", "-", "1e", "NaN"] {
            assert!(Json::parse(bad).is_err(), "{bad:?} should fail");
        }
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert!(Json::parse(&deep).is_err());
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert_eq!(Json::parse(&ok).unwrap().stringify(), ok);
        assert_eq!(Json::parse("[1,x]").unwrap_err().offset, 3);
    }

    #[test]
    fn is_container_agrees_with_parse() {
        for bad in ["", " ", "01", "[1,]", "{,}", "{\"a\"}", "{\"a\":}", "[1] 2", "[\"\t\"]", "[\"\\x\"]", "[", "{\"a\":1", "[\"abc"] {
            assert!(Json::parse(bad).is_err() && !Json::is_container(bad), "{bad:?}");
        }
        for scalar in ["1", "\"x\"", "true", "null"] {
            assert!(Json::parse(scalar).is_ok() && !Json::is_container(scalar), "{scalar:?}");
        }
        for good in ["{}", "[]", " [1, {\"a\":\"x\\ny\", \"b\":[null]}] ", r#"{"a":{"b":[1,"s",true]}}"#] {
            assert!(Json::parse(good).is_ok() && Json::is_container(good), "{good:?}");
        }
        let at = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(Json::is_container(&at));
        assert!(!Json::is_container(&format!("[{at}]")), "past the depth limit");
    }

    /// The http threads of crates/backend have this stack, and handlers run on top of what this uses. A dev build needs
    /// about 768 KB here, so `cargo test -p util` in either profile fails if the margin goes.
    #[test]
    fn the_depth_limit_fits_an_http_thread() {
        let run = || {
            let text = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
            assert!(Json::is_container(&text));
            let v = Json::parse(&text).unwrap();
            assert_eq!(v.stringify(), text);
            drop(v); // dropping recurses as well
        };
        std::thread::Builder::new().stack_size(1 << 20).spawn(run).unwrap().join().unwrap();
    }

    #[test]
    fn accessors() {
        let v = Json::parse(r#"{"a":{"b":[1,"s",true,null]}}"#).unwrap();
        let arr = v.get("a").and_then(|a| a.get("b")).unwrap();
        assert_eq!(arr.at(0).and_then(Json::as_f64), Some(1.0));
        assert_eq!(arr.at(1).and_then(Json::as_str), Some("s"));
        assert_eq!(arr.at(2).and_then(Json::as_bool), Some(true));
        assert!(arr.at(3).unwrap().is_null());
        assert!(v.get("zz").is_none());
    }
}
