//! A JS string kept exactly (Memory, segments, InterShardMemory): Latin-1 when every
//! UTF-16 unit is below 0x100, else UTF-16, so lone surrogates survive.

use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub enum JsStr {
    Latin1(Arc<[u8]>),
    Utf16(Arc<[u16]>),
}

impl JsStr {
    #[allow(clippy::should_implement_trait)] // infallible, unlike FromStr
    pub fn from_str(s: &str) -> JsStr {
        if s.chars().all(|c| (c as u32) < 0x100) {
            JsStr::Latin1(s.chars().map(|c| c as u8).collect())
        } else {
            JsStr::Utf16(s.encode_utf16().collect())
        }
    }

    /// Length in UTF-16 units (what JS `.length` and the 2 MiB / 100 KiB limits count).
    pub fn utf16_len(&self) -> usize {
        match self {
            JsStr::Latin1(b) => b.len(),
            JsStr::Utf16(u) => u.len(),
        }
    }

    fn units(&self) -> Box<dyn Iterator<Item = u16> + '_> {
        match self {
            JsStr::Latin1(b) => Box::new(b.iter().map(|&b| b as u16)),
            JsStr::Utf16(u) => Box::new(u.iter().copied()),
        }
    }

    /// `JSON.stringify(s)`: QuoteJSONString with lone surrogates as `\udxxx`.
    pub fn write_json_quoted(&self, out: &mut String) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let esc = |out: &mut String, u: u32| {
            out.push_str("\\u");
            for shift in [12, 8, 4, 0] {
                out.push(HEX[(u >> shift) as usize & 15] as char);
            }
        };
        out.push('"');
        for r in char::decode_utf16(self.units()) {
            match r {
                Ok('"') => out.push_str("\\\""),
                Ok('\\') => out.push_str("\\\\"),
                Ok('\u{8}') => out.push_str("\\b"),
                Ok('\u{c}') => out.push_str("\\f"),
                Ok('\n') => out.push_str("\\n"),
                Ok('\r') => out.push_str("\\r"),
                Ok('\t') => out.push_str("\\t"),
                Ok(c) if (c as u32) < 0x20 => esc(out, c as u32),
                Ok(c) => out.push(c),
                Err(e) => esc(out, e.unpaired_surrogate() as u32),
            }
        }
        out.push('"');
    }

    /// Rust string; lone surrogates become U+FFFD.
    pub fn to_string_lossy(&self) -> String {
        match self {
            JsStr::Latin1(b) => b.iter().map(|&b| b as char).collect(),
            JsStr::Utf16(u) => String::from_utf16_lossy(u),
        }
    }
}

/// Equal when the UTF-16 contents are equal, whichever representation each side uses.
impl PartialEq for JsStr {
    fn eq(&self, other: &JsStr) -> bool {
        match (self, other) {
            (JsStr::Latin1(a), JsStr::Latin1(b)) => a == b,
            (JsStr::Utf16(a), JsStr::Utf16(b)) => a == b,
            _ => self.utf16_len() == other.utf16_len() && self.units().eq(other.units()),
        }
    }
}

impl Eq for JsStr {}

impl Default for JsStr {
    fn default() -> JsStr {
        JsStr::Latin1(Arc::from(&[][..]))
    }
}

impl fmt::Debug for JsStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = String::new();
        self.write_json_quoted(&mut s);
        f.write_str(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(s: &JsStr) -> String {
        let mut out = String::new();
        s.write_json_quoted(&mut out);
        out
    }

    #[test]
    fn representation_and_length() {
        assert!(matches!(JsStr::from_str("héllo\u{ff}"), JsStr::Latin1(_)));
        assert!(matches!(JsStr::from_str("€"), JsStr::Utf16(_)));
        assert_eq!(JsStr::from_str("a😀").utf16_len(), 3);
        assert_eq!(JsStr::from_str("é").utf16_len(), 1);
        assert_eq!(JsStr::from_str("é"), JsStr::Utf16(Arc::from(&[0xe9u16][..])));
        assert_eq!(JsStr::from_str("x€😀").to_string_lossy(), "x€😀");
        assert_eq!(JsStr::default().utf16_len(), 0);
    }

    #[test]
    fn json_matches_v8() {
        // Expected values from node: JSON.stringify(s).
        assert_eq!(json(&JsStr::from_str("a\"\\\u{8}\u{c}\n\r\t\u{1}\u{1f}\u{7f}é€😀/")), "\"a\\\"\\\\\\b\\f\\n\\r\\t\\u0001\\u001f\u{7f}é€😀/\"");
        let lone = JsStr::Utf16(Arc::from(&[0x61, 0xd800, 0x62, 0xdc00, 0xd83d, 0xde00, 0xdbff][..]));
        assert_eq!(json(&lone), "\"a\\ud800b\\udc00😀\\udbff\"");
        assert_eq!(lone.to_string_lossy(), "a\u{fffd}b\u{fffd}😀\u{fffd}");
    }
}
