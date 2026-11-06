//! JS number <-> string conversions with exact ECMAScript semantics.
//!
//! Digit generation leans on std's float formatting, which is exact: `{:e}`
//! yields the shortest round-trip digits, and `{:.N}` / `{:.Ne}` round the
//! exact binary value correctly. The one difference from ECMA-262 is the tie
//! rule: std rounds exact ties to even, while toFixed/toExponential/toPrecision
//! "pick the larger n". Exact ties are detected from the bits and fixed up.

use std::fmt::{self, Write};

/// Number::toString(x) (radix 10), ECMA-262 6.1.6.1.20.
pub fn to_js_string(x: f64) -> String {
    let mut s = String::new();
    write_js_string(&mut s, x);
    s
}

/// Appends Number::toString(x) to `out` (no allocation besides `out`).
pub fn write_js_string(out: &mut String, x: f64) {
    if x.is_nan() {
        out.push_str("NaN");
        return;
    }
    if x == 0.0 {
        out.push('0'); // also -0
        return;
    }
    if x.is_infinite() {
        out.push_str(if x < 0.0 { "-Infinity" } else { "Infinity" });
        return;
    }
    // Below 2^53 an integer's own digits are its shortest round-trip form.
    if x.fract() == 0.0 && x.abs() < 9007199254740992.0 {
        let (mut n, mut buf, mut i) = (x.abs() as u64, [0u8; 17], 17);
        loop {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        if x < 0.0 {
            out.push('-');
        }
        push_ascii(out, &buf[i..]);
        return;
    }
    if x < 0.0 {
        out.push('-');
    }
    let s = Sci::shortest(x.abs());
    let d = s.digits();
    let k = d.len() as i32;
    let n = s.exp + 1; // ECMA-262's n: value = 0.d1d2.. * 10^n
    if k <= n && n <= 21 {
        push_ascii(out, d);
        push_zeros(out, (n - k) as usize);
    } else if 0 < n && n <= 21 {
        push_ascii(out, &d[..n as usize]);
        out.push('.');
        push_ascii(out, &d[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        push_zeros(out, (-n) as usize);
        push_ascii(out, d);
    } else {
        push_exponential(out, d, n - 1);
    }
}

/// Number.prototype.toFixed(digits), ECMA-262 21.1.3.3.
/// `digits` must be 0..=100; the caller throws the RangeError.
pub fn to_fixed(x: f64, digits: usize) -> String {
    assert!(digits <= 100, "toFixed digits out of range");
    if !x.is_finite() || x.abs() >= 1e21 {
        return to_js_string(x);
    }
    let a = x.abs();
    let mut s = if fixed_tie(a, digits) {
        // `a` has exactly digits+1 fractional digits, so this is exact and ends in 5.
        let mut s = format!("{:.*}", digits + 1, a);
        s.pop();
        if s.ends_with('.') {
            s.pop();
        }
        round_up_decimal(&mut s);
        s
    } else {
        format!("{:.*}", digits, a)
    };
    if x < 0.0 {
        s.insert(0, '-'); // -0 is not < 0, so (-0).toFixed(2) is "0.00" like JS
    }
    s
}

/// Number.prototype.toExponential(fractionDigits), ECMA-262 21.1.3.2.
/// `None` is `undefined` (as many digits as needed); `Some(f)` needs 0..=100.
pub fn to_exponential(x: f64, digits: Option<usize>) -> String {
    assert!(digits.is_none_or(|d| d <= 100), "toExponential digits out of range");
    if !x.is_finite() {
        return to_js_string(x);
    }
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let a = x.abs();
    if a == 0.0 {
        let f = digits.unwrap_or(0);
        out.push('0');
        if f > 0 {
            out.push('.');
            push_zeros(&mut out, f);
        }
        out.push_str("e+0");
        return out;
    }
    let s = match digits {
        None => Sci::shortest(a),
        Some(f) => Sci::round(a, f + 1),
    };
    push_exponential(&mut out, s.digits(), s.exp);
    out
}

/// Number.prototype.toPrecision(precision), ECMA-262 21.1.3.5.
/// `precision` must be 1..=100; the caller throws the RangeError.
pub fn to_precision(x: f64, precision: usize) -> String {
    assert!((1..=100).contains(&precision), "toPrecision precision out of range");
    if !x.is_finite() {
        return to_js_string(x);
    }
    let p = precision as i32;
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let a = x.abs();
    let (s, e) = if a == 0.0 {
        (Sci::zeros(precision), 0)
    } else {
        let s = Sci::round(a, precision);
        let e = s.exp;
        (s, e)
    };
    let d = s.digits();
    if e < -6 || e >= p {
        push_exponential(&mut out, d, e);
    } else if e == p - 1 {
        push_ascii(&mut out, d);
    } else if e >= 0 {
        push_ascii(&mut out, &d[..e as usize + 1]);
        out.push('.');
        push_ascii(&mut out, &d[e as usize + 1..]);
    } else {
        out.push_str("0.");
        push_zeros(&mut out, (-(e + 1)) as usize);
        push_ascii(&mut out, d);
    }
    out
}

/// Number.prototype.toString(radix) for radix 2..=36, matching V8 output
/// (including the digit-limited fraction for non-integers).
pub fn to_radix_string(x: f64, radix: u32) -> String {
    assert!((2..=36).contains(&radix), "radix out of range");
    if radix == 10 || !x.is_finite() {
        return to_js_string(x);
    }
    // Port of V8 DoubleToRadixCString (src/numbers/conversions.cc).
    const CHARS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let r = radix as f64;
    let negative = x < 0.0;
    let value = x.abs();
    let mut integer = value.floor();
    let mut fraction = value - integer;
    // Only emit fractional digits up to the input's precision.
    let mut delta = (0.5 * (value.next_up() - value)).max(f64::from_bits(1));
    let mut frac: Vec<u8> = Vec::new();
    if fraction >= delta {
        loop {
            fraction *= r;
            delta *= r;
            let digit = fraction as usize;
            frac.push(CHARS[digit]);
            fraction -= digit as f64;
            // Round to even.
            if (fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1)) && fraction + delta > 1.0 {
                // Round up, propagating the carry through written digits.
                loop {
                    match frac.pop() {
                        None => {
                            integer += 1.0;
                            break;
                        }
                        Some(c) => {
                            let d = (c as char).to_digit(36).unwrap();
                            if d + 1 < radix {
                                frac.push(CHARS[d as usize + 1]);
                                break;
                            }
                        }
                    }
                }
                break;
            }
            if fraction < delta {
                break;
            }
        }
    }
    // Integer digits, least significant first. Digits below the double's
    // precision are filled with zeros, as V8 does.
    let mut int: Vec<u8> = Vec::new();
    while double_exponent(integer / r) > 0 {
        integer /= r;
        int.push(b'0');
    }
    loop {
        let rem = integer % r;
        int.push(CHARS[rem as usize]);
        integer = (integer - rem) / r;
        if integer <= 0.0 {
            break;
        }
    }
    let mut out = String::with_capacity(int.len() + frac.len() + 2);
    if negative {
        out.push('-');
    }
    int.reverse();
    push_ascii(&mut out, &int);
    if !frac.is_empty() {
        out.push('.');
        push_ascii(&mut out, &frac);
    }
    out
}

/// ECMAScript StringToNumber (7.1.4.1.1): what `Number(s)` / unary `+` do.
pub fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(is_js_whitespace);
    let b = t.as_bytes();
    if b.is_empty() {
        return 0.0;
    }
    if b.len() >= 2 && b[0] == b'0' {
        let radix = match b[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 0,
        };
        if radix != 0 {
            return parse_pow2_radix(&b[2..], radix);
        }
    }
    let (neg, rest) = match b[0] {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    let v = if rest == "Infinity" {
        f64::INFINITY
    } else if is_str_unsigned_decimal(rest.as_bytes()) {
        // std's parser is correctly rounded and accepts a superset of this grammar.
        rest.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        return f64::NAN;
    };
    if neg {
        -v
    } else {
        v
    }
}

/// WhiteSpace and LineTerminator code points (ECMA-262 12.2, 12.3).
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{9}'
            | '\u{a}'
            | '\u{b}'
            | '\u{c}'
            | '\u{d}'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// StrUnsignedDecimalLiteral minus "Infinity":
/// digits [. digits] | . digits, then an optional exponent. No separators.
fn is_str_unsigned_decimal(b: &[u8]) -> bool {
    let mut i = 0;
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - int_start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - frac_start;
    }
    if digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return false;
        }
    }
    i == b.len()
}

/// 0x / 0o / 0b literal digits, correctly rounded (round half to even) like
/// V8's InternalStringToIntDouble.
fn parse_pow2_radix(b: &[u8], radix: u32) -> f64 {
    if b.is_empty() {
        return f64::NAN;
    }
    let bits = radix.trailing_zeros();
    let mut m: u128 = 0;
    let mut dropped_bits: i32 = 0;
    let mut sticky = false;
    for &c in b {
        let Some(d) = (c as char).to_digit(radix) else {
            return f64::NAN;
        };
        if m >> (128 - bits) == 0 {
            m = (m << bits) | d as u128;
        } else {
            dropped_bits = dropped_bits.saturating_add(bits as i32);
            sticky |= d != 0;
        }
    }
    // Once digits are dropped m has >= 124 bits, so bit 0 sits far below the
    // 53-bit rounding point and works as a sticky bit for `as f64`.
    if sticky {
        m |= 1;
    }
    let v = m as f64;
    if dropped_bits > 1023 {
        return if v == 0.0 { 0.0 } else { f64::INFINITY };
    }
    v * f64::from_bits(((dropped_bits + 1023) as u64) << 52) // exact power-of-two scale
}

/// V8's Double::Exponent(): e such that value = significand * 2^e with a
/// 53-bit integer significand.
fn double_exponent(x: f64) -> i32 {
    let biased = ((x.to_bits() >> 52) & 0x7ff) as i32;
    if biased == 0 {
        -1074
    } else {
        biased - 1075
    }
}

/// Splits finite `a > 0` into (m, e) with a = m * 2^e and m odd.
fn odd_decompose(a: f64) -> (u64, i32) {
    let bits = a.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1 << 52) - 1);
    let (m, e) = if biased == 0 { (frac, -1074) } else { (frac | 1 << 52, biased - 1075) };
    let tz = m.trailing_zeros();
    (m >> tz, e + tz as i32)
}

/// If the exact decimal expansion of finite `a > 0` ends in the digit 5,
/// the decimal position (power of ten) of that 5. A value rounded at the
/// position just before it is an exact tie.
fn tie_pos(a: f64) -> Option<i32> {
    let (mut m, e) = odd_decompose(a);
    if e < 0 {
        // a = m * 5^k / 10^k with m*5^k odd, so it ends in a 5 at 10^-k.
        return Some(e);
    }
    // Integer m * 2^e: the last nonzero digit is a 5 (at 10^e) only when m
    // has more factors of 5 than the 2^e can pair into tens.
    let mut v5 = 0;
    while m % 5 == 0 {
        m /= 5;
        v5 += 1;
    }
    (v5 > e).then_some(e)
}

/// Is `a` exactly halfway between two `digits`-fraction-digit decimals?
fn fixed_tie(a: f64, digits: usize) -> bool {
    if a == 0.0 {
        return false;
    }
    let (_, e) = odd_decompose(a);
    // Only non-integers can tie, and their last digit (at 10^e) is always 5.
    e < 0 && -(e as i64) == digits as i64 + 1
}

fn round_up_decimal(s: &mut String) {
    let mut b = std::mem::take(s).into_bytes();
    let mut i = b.len();
    loop {
        if i == 0 {
            b.insert(0, b'1');
            break;
        }
        i -= 1;
        match b[i] {
            b'.' => continue,
            b'9' => b[i] = b'0',
            _ => {
                b[i] += 1;
                break;
            }
        }
    }
    *s = String::from_utf8(b).unwrap();
}

/// A decimal in scientific form: d.ddd * 10^exp, digits kept on the stack.
struct Sci {
    buf: [u8; 128],
    len: usize,
    exp: i32,
}

impl Sci {
    fn digits(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    fn zeros(n: usize) -> Sci {
        Sci { buf: [b'0'; 128], len: n, exp: 0 }
    }

    /// Shortest round-trip digits of finite `a > 0`. When two shortest
    /// candidates are equally close, ECMA-262 picks the even one; std picks
    /// the larger, so that case is redone from the exact digits.
    fn shortest(a: f64) -> Sci {
        let mut w = Buf::new();
        write!(w, "{:e}", a).unwrap();
        let s = Sci::from_std(w.as_bytes());
        let k = s.len;
        if tie_pos(a) != Some(s.exp - k as i32) {
            return s;
        }
        let mut w = Buf::new();
        write!(w, "{:.*e}", k, a).unwrap(); // exact: k digits plus the final 5
        let mut c = Sci::from_std(w.as_bytes());
        c.len = k;
        if c.buf[k - 1] & 1 == 1 {
            c.increment();
        }
        while c.len > 1 && c.buf[c.len - 1] == b'0' {
            c.len -= 1;
        }
        let mut w = Buf::new();
        write!(w, "{}e{}", std::str::from_utf8(c.digits()).unwrap(), c.exp - c.len as i32 + 1).unwrap();
        // The even candidate only counts if it still round-trips.
        let round_trips = std::str::from_utf8(w.as_bytes()).unwrap().parse::<f64>() == Ok(a);
        if round_trips {
            c
        } else {
            s
        }
    }

    /// Adds one unit in the last digit; 99.9 carries to 100 with exp + 1.
    fn increment(&mut self) {
        let mut i = self.len;
        loop {
            if i == 0 {
                self.buf[0] = b'1';
                self.exp += 1;
                return;
            }
            i -= 1;
            if self.buf[i] == b'9' {
                self.buf[i] = b'0';
            } else {
                self.buf[i] += 1;
                return;
            }
        }
    }

    /// `a` rounded to `n` significant digits (1..=101), exact ties rounded up.
    fn round(a: f64, n: usize) -> Sci {
        let mut w = Buf::new();
        // One extra digit: an exact tie shows as a trailing 5 that is the
        // last digit of the exact expansion.
        write!(w, "{:.*e}", n, a).unwrap();
        let mut t = Sci::from_std(w.as_bytes());
        if tie_pos(a) == Some(t.exp - n as i32) {
            t.len = n;
            t.increment();
            return t;
        }
        let mut w = Buf::new();
        write!(w, "{:.*e}", n - 1, a).unwrap();
        Sci::from_std(w.as_bytes())
    }

    /// Parses std's `{:e}` output such as "1.25e-7".
    fn from_std(s: &[u8]) -> Sci {
        let mut r = Sci { buf: [0; 128], len: 0, exp: 0 };
        let e = s.iter().position(|&c| c == b'e').unwrap();
        for &c in &s[..e] {
            if c != b'.' {
                r.buf[r.len] = c;
                r.len += 1;
            }
        }
        r.exp = std::str::from_utf8(&s[e + 1..]).unwrap().parse().unwrap();
        r
    }
}

/// Fixed-size stack buffer for `write!`; 160 bytes fits any `{:.101e}` of an f64.
struct Buf {
    b: [u8; 160],
    len: usize,
}

impl Buf {
    fn new() -> Buf {
        Buf { b: [0; 160], len: 0 }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.b[..self.len]
    }
}

impl Write for Buf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        self.b.get_mut(self.len..end).ok_or(fmt::Error)?.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// d[.ddd]e(+|-)exp
fn push_exponential(out: &mut String, d: &[u8], exp: i32) {
    out.push(d[0] as char);
    if d.len() > 1 {
        out.push('.');
        push_ascii(out, &d[1..]);
    }
    out.push('e');
    out.push(if exp < 0 { '-' } else { '+' });
    write!(out, "{}", exp.unsigned_abs()).unwrap();
}

fn push_ascii(out: &mut String, d: &[u8]) {
    out.push_str(std::str::from_utf8(d).unwrap());
}

fn push_zeros(out: &mut String, n: usize) {
    out.extend(std::iter::repeat_n('0', n));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_string_layout() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "0"),
            (f64::NAN, "NaN"),
            (f64::NEG_INFINITY, "-Infinity"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123e-20, "1.23e-18"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-1.5, "-1.5"),
            (7.0, "7"),
            (-49.0, "-49"),
            (9007199254740991.0, "9007199254740991"),
            (-9007199254740991.0, "-9007199254740991"),
            (9007199254740992.0, "9007199254740992"),
            (2f64.powi(60), "1152921504606847000"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
        ];
        for &(x, s) in cases {
            assert_eq!(to_js_string(x), s, "{x:e}");
        }
    }

    #[test]
    fn formatting_ties_round_up() {
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(-2.5, 0), "-3");
        assert_eq!(to_fixed(1.005, 2), "1.00"); // 1.00499999...
        assert_eq!(to_fixed(1.125, 2), "1.13");
        assert_eq!(to_fixed(9.995, 2), "9.99"); // below the tie in binary
        assert_eq!(to_fixed(-0.0000001, 2), "-0.00");
        assert_eq!(to_exponential(2.5, Some(0)), "3e+0");
        assert_eq!(to_exponential(250.0, Some(0)), "3e+2");
        assert_eq!(to_exponential(0.0, None), "0e+0");
        assert_eq!(to_precision(1.25, 2), "1.3");
        assert_eq!(to_precision(99.5, 2), "1.0e+2");
        assert_eq!(to_precision(0.0, 3), "0.00");
        assert_eq!(to_precision(123456.0, 2), "1.2e+5");
        assert_eq!(to_precision(0.000001234, 2), "0.0000012");
        assert_eq!(to_precision(0.0000001234, 2), "1.2e-7");
    }

    #[test]
    fn radix() {
        assert_eq!(to_radix_string(255.0, 16), "ff");
        assert_eq!(to_radix_string(-0.5, 2), "-0.1");
        assert_eq!(to_radix_string(0.1, 3), "0.0022002200220022002200220022002201");
        assert_eq!(to_radix_string(-0.0, 2), "0");
    }

    #[test]
    fn parse() {
        assert_eq!(string_to_number(""), 0.0);
        assert_eq!(string_to_number(" \n\u{feff}12\u{3000}"), 12.0);
        assert!(string_to_number("12\u{85}").is_nan());
        assert_eq!(string_to_number("0x10"), 16.0);
        assert!(string_to_number("-0x10").is_nan());
        assert!(string_to_number("0x").is_nan());
        assert_eq!(string_to_number("0b101"), 5.0);
        assert_eq!(string_to_number("0O17"), 15.0);
        assert_eq!(string_to_number("-Infinity"), f64::NEG_INFINITY);
        assert!(string_to_number("infinity").is_nan());
        assert!(string_to_number("inf").is_nan());
        assert!(string_to_number("NaN").is_nan());
        assert_eq!(string_to_number("1."), 1.0);
        assert_eq!(string_to_number("+.5"), 0.5);
        assert!(string_to_number(".").is_nan());
        assert!(string_to_number("1e").is_nan());
        assert!(string_to_number("1_0").is_nan());
        assert!(string_to_number("-0").is_sign_negative());
        assert_eq!(string_to_number("1e400"), f64::INFINITY);
        // 2^53 + 1 rounds to even, 2^53 + 3 rounds up
        assert_eq!(string_to_number("0x20000000000001"), 9007199254740992.0);
        assert_eq!(string_to_number("0x20000000000003"), 9007199254740996.0);
    }
}

/// `new Date(ms).toISOString()` (UTC; ms since the epoch).
pub fn iso_date(ms: i64) -> String {
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    // civil_from_days (H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000)
}

#[cfg(test)]
mod iso_date_tests {
    use super::iso_date;

    #[test]
    fn iso_dates() {
        assert_eq!(iso_date(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_date(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
        assert_eq!(iso_date(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(iso_date(-1), "1969-12-31T23:59:59.999Z");
    }
}
