//! DEFLATE (RFC 1951) compressor and decompressor, zlib (RFC 1950) and gzip
//! (RFC 1952) framing, CRC-32 and Adler-32.
//!
//! Decompression takes an output limit: inflating untrusted input is a trust
//! boundary and a few KB can otherwise expand to gigabytes.

use std::collections::BinaryHeap;
use std::cmp::Reverse;
use std::fmt;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InflateError(pub &'static str);

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "inflate: {}", self.0)
    }
}

impl std::error::Error for InflateError {}

// ---------------------------------------------------------------- checksums

const CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xedb88320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

/// CRC-32 (IEEE, as used by gzip and zip).
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c = CRC_TABLE[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

/// Adler-32 (as used by zlib).
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 is the most bytes before b can overflow u32 (zlib's NMAX).
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

// ------------------------------------------------------------------ framing

pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x9c];
    out.extend_from_slice(&deflate(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

pub fn zlib_decompress(data: &[u8], max_out: usize) -> Result<Vec<u8>, InflateError> {
    if data.len() < 6 {
        return Err(InflateError("truncated zlib stream"));
    }
    let (cmf, flg) = (data[0], data[1]);
    if cmf & 0x0f != 8 || cmf >> 4 > 7 || !(cmf as u16 * 256 + flg as u16).is_multiple_of(31) {
        return Err(InflateError("bad zlib header"));
    }
    if flg & 0x20 != 0 {
        return Err(InflateError("zlib preset dictionary not supported"));
    }
    let (out, used) = inflate_raw(&data[2..], max_out)?;
    let rest = &data[2 + used..];
    if rest.len() < 4 || u32::from_be_bytes(rest[..4].try_into().unwrap()) != adler32(&out) {
        return Err(InflateError("bad zlib checksum"));
    }
    Ok(out)
}

pub fn gzip_compress(data: &[u8]) -> Vec<u8> {
    // magic, CM=8, no flags, no mtime, XFL=0, OS=255 (unknown)
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
    out.extend_from_slice(&deflate(data));
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

/// Decompresses a gzip file; concatenated members are joined like gunzip does.
pub fn gzip_decompress(data: &[u8], max_out: usize) -> Result<Vec<u8>, InflateError> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let (member, used) = gzip_member(&data[pos..], max_out - out.len())?;
        out.extend_from_slice(&member);
        pos += used;
        if pos == data.len() {
            return Ok(out);
        }
        if !data[pos..].starts_with(&[0x1f, 0x8b]) {
            return Err(InflateError("trailing data after gzip member"));
        }
    }
}

fn gzip_member(data: &[u8], max_out: usize) -> Result<(Vec<u8>, usize), InflateError> {
    const EOF: InflateError = InflateError("truncated gzip header");
    if data.len() < 10 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 8 {
        return Err(InflateError("bad gzip header"));
    }
    let flags = data[3];
    if flags & 0xe0 != 0 {
        return Err(InflateError("reserved gzip flags set"));
    }
    let mut pos = 10;
    if flags & 4 != 0 {
        // FEXTRA
        let xlen = data.get(pos..pos + 2).ok_or(EOF)?;
        pos += 2 + u16::from_le_bytes([xlen[0], xlen[1]]) as usize;
    }
    for flag in [8, 16] {
        // FNAME, FCOMMENT: zero-terminated
        if flags & flag != 0 {
            pos += 1 + data.get(pos..).ok_or(EOF)?.iter().position(|&c| c == 0).ok_or(EOF)?;
        }
    }
    if flags & 2 != 0 {
        pos += 2; // FHCRC
    }
    let body = data.get(pos..).ok_or(EOF)?;
    let (out, used) = inflate_raw(body, max_out)?;
    pos += used;
    let trailer = data.get(pos..pos + 8).ok_or(InflateError("truncated gzip trailer"))?;
    if u32::from_le_bytes(trailer[..4].try_into().unwrap()) != crc32(&out) {
        return Err(InflateError("bad gzip crc"));
    }
    if u32::from_le_bytes(trailer[4..].try_into().unwrap()) != out.len() as u32 {
        return Err(InflateError("bad gzip length"));
    }
    Ok((out, pos + 8))
}

// ------------------------------------------------------------------ inflate

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// Order of code length code lengths in a dynamic block header.
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

const EOF: InflateError = InflateError("unexpected end of data");

/// Decompresses a raw DEFLATE stream (no zlib/gzip framing).
pub fn inflate(data: &[u8], max_out: usize) -> Result<Vec<u8>, InflateError> {
    inflate_raw(data, max_out).map(|(out, _)| out)
}

/// LSB-first bit reader. Bits of `buf` above `cnt` are either zero or equal
/// to the stream bits that will be loaded there, so a refill may OR in a
/// whole word without masking.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    cnt: u32,
}

impl Bits<'_> {
    fn refill(&mut self) {
        if let Some(word) = self.data.get(self.pos..self.pos + 8) {
            self.buf |= u64::from_le_bytes(word.try_into().unwrap()) << self.cnt;
            let n = (63 - self.cnt) / 8;
            self.pos += n as usize;
            self.cnt += n * 8;
        } else {
            while self.cnt <= 56 && self.pos < self.data.len() {
                self.buf |= (self.data[self.pos] as u64) << self.cnt;
                self.pos += 1;
                self.cnt += 8;
            }
        }
    }

    fn bits(&mut self, n: u32) -> Result<u32, InflateError> {
        if self.cnt < n {
            self.refill();
            if self.cnt < n {
                return Err(EOF);
            }
        }
        let v = (self.buf & ((1u64 << n) - 1)) as u32;
        self.buf >>= n;
        self.cnt -= n;
        Ok(v)
    }

    /// Bytes of input consumed so far (whole bytes still buffered are not).
    fn consumed(&self) -> usize {
        self.pos - (self.cnt / 8) as usize
    }
}

const FAST_BITS: u32 = 10;

/// Canonical Huffman decoder: a table for codes up to FAST_BITS long and a
/// bit-by-bit walk (as in zlib's puff.c) for longer ones.
struct Huffman {
    /// Indexed by the next FAST_BITS input bits: symbol << 4 | length, 0 if the code is longer.
    fast: [u16; 1 << FAST_BITS],
    count: [u16; 16],
    symbol: [u16; 288],
}

impl Huffman {
    /// Returns the decoder and whether the code is incomplete; errors when
    /// over-subscribed.
    fn new(lengths: &[u8]) -> Result<(Huffman, bool), InflateError> {
        let mut h = Huffman { fast: [0; 1 << FAST_BITS], count: [0; 16], symbol: [0; 288] };
        for &l in lengths {
            h.count[l as usize] += 1;
        }
        let mut left: i32 = 1;
        for len in 1..16 {
            left = (left << 1) - h.count[len] as i32;
            if left < 0 {
                return Err(InflateError("over-subscribed huffman code"));
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + h.count[len];
        }
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                h.symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        let mut code: u32 = 0;
        let mut idx = 0;
        for len in 1..=FAST_BITS {
            for _ in 0..h.count[len as usize] {
                let entry = h.symbol[idx] << 4 | len as u16;
                let rev = code.reverse_bits() >> (32 - len);
                for hi in 0..1u32 << (FAST_BITS - len) {
                    h.fast[(rev | hi << len) as usize] = entry;
                }
                idx += 1;
                code += 1;
            }
            code <<= 1;
        }
        let complete = left == 0 || h.count[0] as usize == lengths.len();
        Ok((h, !complete))
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16, InflateError> {
        if bits.cnt < 15 {
            bits.refill();
        }
        let e = self.fast[(bits.buf & ((1 << FAST_BITS) - 1)) as usize];
        if e != 0 {
            let len = (e & 15) as u32;
            if len > bits.cnt {
                return Err(EOF);
            }
            bits.buf >>= len;
            bits.cnt -= len;
            return Ok(e >> 4);
        }
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.bits(1)? as i32;
            let count = self.count[len] as i32;
            if code - count < first {
                return Ok(self.symbol[(index + code - first) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(InflateError("invalid huffman code"))
    }
}

fn fixed_tables() -> &'static (Huffman, Huffman) {
    static FIXED: OnceLock<(Huffman, Huffman)> = OnceLock::new();
    FIXED.get_or_init(|| {
        let mut l = [0u8; 288];
        l[..144].fill(8);
        l[144..256].fill(9);
        l[256..280].fill(7);
        l[280..].fill(8);
        (Huffman::new(&l).unwrap().0, Huffman::new(&[5; 30]).unwrap().0)
    })
}

/// Inflates a raw stream; returns the output and the input bytes consumed.
fn inflate_raw(data: &[u8], max_out: usize) -> Result<(Vec<u8>, usize), InflateError> {
    let mut bits = Bits { data, pos: 0, buf: 0, cnt: 0 };
    let mut out = Vec::new();
    loop {
        let last = bits.bits(1)?;
        match bits.bits(2)? {
            0 => stored(&mut bits, &mut out, max_out)?,
            1 => {
                let (lit, dist) = fixed_tables();
                codes(&mut bits, &mut out, lit, dist, max_out)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut bits)?;
                codes(&mut bits, &mut out, &lit, &dist, max_out)?;
            }
            _ => return Err(InflateError("invalid block type")),
        }
        if last == 1 {
            return Ok((out, bits.consumed()));
        }
    }
}

fn stored(bits: &mut Bits, out: &mut Vec<u8>, max_out: usize) -> Result<(), InflateError> {
    bits.bits(bits.cnt % 8)?; // skip to a byte boundary
    let len = bits.bits(16)? as usize;
    if bits.bits(16)? as usize != !len & 0xffff {
        return Err(InflateError("stored block length mismatch"));
    }
    // Restart byte-wise reading where the bit buffer left off.
    let pos = bits.consumed();
    let block = bits.data.get(pos..pos + len).ok_or(EOF)?;
    if out.len() + len > max_out {
        return Err(InflateError("output limit exceeded"));
    }
    out.extend_from_slice(block);
    bits.pos = pos + len;
    bits.buf = 0;
    bits.cnt = 0;
    Ok(())
}

fn dynamic_tables(bits: &mut Bits) -> Result<(Huffman, Huffman), InflateError> {
    let nlen = bits.bits(5)? as usize + 257;
    let ndist = bits.bits(5)? as usize + 1;
    let ncode = bits.bits(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(InflateError("bad dynamic block counts"));
    }
    let mut lengths = [0u8; 286 + 30];
    for &i in &CL_ORDER[..ncode] {
        lengths[i] = bits.bits(3)? as u8;
    }
    let (lencode, incomplete) = Huffman::new(&lengths[..19])?;
    if incomplete {
        return Err(InflateError("incomplete code length code"));
    }
    lengths[..19].fill(0);
    let mut i = 0;
    while i < nlen + ndist {
        let sym = lencode.decode(bits)?;
        if sym < 16 {
            lengths[i] = sym as u8;
            i += 1;
            continue;
        }
        let (len, rep) = match sym {
            16 => {
                if i == 0 {
                    return Err(InflateError("repeat with no previous length"));
                }
                (lengths[i - 1], 3 + bits.bits(2)?)
            }
            17 => (0, 3 + bits.bits(3)?),
            _ => (0, 11 + bits.bits(7)?),
        };
        let rep = rep as usize;
        if i + rep > nlen + ndist {
            return Err(InflateError("too many code lengths"));
        }
        lengths[i..i + rep].fill(len);
        i += rep;
    }
    if lengths[256] == 0 {
        return Err(InflateError("missing end-of-block code"));
    }
    // Incomplete codes are only allowed for a single code of length 1 (puff.c).
    let only_len1 = |l: &[u8]| l.iter().all(|&x| x <= 1);
    let (lit, incomplete) = Huffman::new(&lengths[..nlen])?;
    if incomplete && !only_len1(&lengths[..nlen]) {
        return Err(InflateError("incomplete literal/length code"));
    }
    let (dist, incomplete) = Huffman::new(&lengths[nlen..nlen + ndist])?;
    if incomplete && !only_len1(&lengths[nlen..nlen + ndist]) {
        return Err(InflateError("incomplete distance code"));
    }
    Ok((lit, dist))
}

fn codes(bits: &mut Bits, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman, max_out: usize) -> Result<(), InflateError> {
    loop {
        let sym = lit.decode(bits)? as usize;
        if sym < 256 {
            if out.len() >= max_out {
                return Err(InflateError("output limit exceeded"));
            }
            out.push(sym as u8);
            continue;
        }
        if sym == 256 {
            return Ok(());
        }
        let li = sym - 257;
        if li >= 29 {
            return Err(InflateError("invalid length symbol"));
        }
        let len = LBASE[li] as usize + bits.bits(LEXT[li] as u32)? as usize;
        let di = dist.decode(bits)? as usize;
        if di >= 30 {
            return Err(InflateError("invalid distance symbol"));
        }
        let d = DBASE[di] as usize + bits.bits(DEXT[di] as u32)? as usize;
        if d > out.len() {
            return Err(InflateError("distance too far back"));
        }
        if out.len() + len > max_out {
            return Err(InflateError("output limit exceeded"));
        }
        let start = out.len() - d;
        if d >= len {
            out.extend_from_within(start..start + len);
        } else {
            for i in 0..len {
                out.push(out[start + i]); // overlapping copy repeats the pattern
            }
        }
    }
}

// ------------------------------------------------------------------ deflate

const WINDOW: usize = 32768;
const WMASK: usize = WINDOW - 1;
const HASH_BITS: u32 = 15;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
// zlib level 6 parameters (deflate.c configuration_table).
const MAX_CHAIN: usize = 128;
const GOOD_LEN: usize = 8;
const MAX_LAZY: usize = 16;
const NICE_LEN: usize = 128;
/// A 3-byte match farther than this costs more than three literals (zlib TOO_FAR).
const TOO_FAR: usize = 4096;
const BLOCK_TOKENS: usize = 16384;
const NIL: u32 = u32::MAX;

/// Compresses to a raw DEFLATE stream: LZ77 with hash chains and lazy
/// matching (like zlib level 6), each block written as dynamic Huffman,
/// fixed Huffman or stored, whichever is smallest.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut c = Compressor {
        data,
        head: vec![NIL; 1 << HASH_BITS],
        prev: vec![NIL; WINDOW],
        next_insert: 0,
        tokens: Vec::with_capacity(BLOCK_TOKENS),
        block_start: 0,
        w: BitWriter { out: Vec::with_capacity(data.len() / 3 + 64), buf: 0, cnt: 0 },
    };
    let n = data.len();
    let mut pos = 0;
    // Lazy matching: a match found at pos - 1 is held back until we know
    // pos does not start a longer one.
    let mut pending = false;
    let (mut prev_len, mut prev_dist) = (0, 0);
    while pos < n {
        let (mut len, mut dist) = if prev_len < MAX_LAZY { c.longest_match(pos, prev_len) } else { (0, 0) };
        if len == MIN_MATCH && dist > TOO_FAR {
            (len, dist) = (0, 0);
        }
        if pending && prev_len >= MIN_MATCH && len <= prev_len {
            c.emit(Token { len: prev_len as u16, dist: prev_dist as u16 }, pos - 1 + prev_len);
            pos = pos - 1 + prev_len;
            pending = false;
            prev_len = 0;
        } else {
            if pending {
                c.emit(Token { len: 0, dist: data[pos - 1] as u16 }, pos);
            }
            pending = true;
            (prev_len, prev_dist) = (len, dist);
            pos += 1;
        }
    }
    if pending {
        c.emit(Token { len: 0, dist: data[n - 1] as u16 }, n);
    }
    c.flush_block(true);
    c.w.align();
    c.w.out
}

/// A literal (len 0, byte in `dist`) or a match.
#[derive(Clone, Copy)]
struct Token {
    len: u16,
    dist: u16,
}

struct Compressor<'a> {
    data: &'a [u8],
    /// Most recent position for each hash of 3 bytes.
    head: Vec<u32>,
    /// Previous position with the same hash, indexed by position & WMASK.
    prev: Vec<u32>,
    next_insert: usize,
    tokens: Vec<Token>,
    block_start: usize,
    w: BitWriter,
}

impl Compressor<'_> {
    fn hash(&self, i: usize) -> usize {
        let d = self.data;
        let v = d[i] as u32 | (d[i + 1] as u32) << 8 | (d[i + 2] as u32) << 16;
        (v.wrapping_mul(0x9e3779b1) >> (32 - HASH_BITS)) as usize
    }

    /// Adds positions up to `end` (exclusive) to the hash chains, each once.
    fn insert_until(&mut self, end: usize) {
        let limit = end.min(self.data.len().saturating_sub(MIN_MATCH - 1));
        while self.next_insert < limit {
            let i = self.next_insert;
            let h = self.hash(i);
            self.prev[i & WMASK] = self.head[h];
            self.head[h] = i as u32;
            self.next_insert += 1;
        }
        self.next_insert = self.next_insert.max(end);
    }

    /// Longest match at `pos` that beats `prev_len`, as (len, dist), or (0, 0).
    fn longest_match(&mut self, pos: usize, prev_len: usize) -> (usize, usize) {
        let d = self.data;
        let max_len = MAX_MATCH.min(d.len() - pos);
        self.insert_until(pos);
        if max_len < MIN_MATCH || prev_len >= max_len {
            return (0, 0);
        }
        let mut cand = self.head[self.hash(pos)] as usize;
        self.insert_until(pos + 1);
        // Positions at distance WINDOW share a `prev` slot with `pos`, so stop short of it.
        let min_pos = pos.saturating_sub(WINDOW - 1);
        let mut chain = if prev_len >= GOOD_LEN { MAX_CHAIN / 4 } else { MAX_CHAIN };
        let (mut best_len, mut best_dist) = (prev_len.max(MIN_MATCH - 1), 0);
        while cand >= min_pos && cand < pos && chain > 0 {
            if d[cand + best_len] == d[pos + best_len] {
                let l = match_len(d, cand, pos, max_len);
                if l > best_len {
                    (best_len, best_dist) = (l, pos - cand);
                    if l >= NICE_LEN || l == max_len {
                        break;
                    }
                }
            }
            let next = self.prev[cand & WMASK] as usize;
            if next >= cand {
                break; // NIL (or a stale slot); chains only go backwards
            }
            cand = next;
            chain -= 1;
        }
        if best_dist == 0 {
            (0, 0)
        } else {
            (best_len, best_dist)
        }
    }

    /// Queues a token covering input up to `end`.
    fn emit(&mut self, t: Token, end: usize) {
        self.tokens.push(t);
        if self.tokens.len() >= BLOCK_TOKENS {
            self.flush_block_until(end, false);
        }
    }

    fn flush_block(&mut self, last: bool) {
        self.flush_block_until(self.data.len(), last);
    }

    fn flush_block_until(&mut self, end: usize, last: bool) {
        write_block(&mut self.w, &self.tokens, &self.data[self.block_start..end], last);
        self.tokens.clear();
        self.block_start = end;
    }
}

/// Length of the common prefix of data[a..] and data[b..], up to `max`
/// (a < b and b + max <= data.len()).
fn match_len(data: &[u8], a: usize, b: usize, max: usize) -> usize {
    let mut n = 0;
    while n + 8 <= max {
        let x = u64::from_le_bytes(data[a + n..a + n + 8].try_into().unwrap());
        let y = u64::from_le_bytes(data[b + n..b + n + 8].try_into().unwrap());
        if x != y {
            return n + ((x ^ y).trailing_zeros() / 8) as usize;
        }
        n += 8;
    }
    while n < max && data[a + n] == data[b + n] {
        n += 1;
    }
    n
}

fn len_code(len: usize) -> usize {
    let x = len - 3;
    if x < 8 {
        x
    } else if x == 255 {
        28
    } else {
        let nb = x.ilog2() as usize;
        4 * (nb - 1) + ((x >> (nb - 2)) & 3)
    }
}

fn dist_code(dist: usize) -> usize {
    let x = dist - 1;
    if x < 4 {
        x
    } else {
        let nb = x.ilog2() as usize;
        2 * nb + ((x >> (nb - 1)) & 1)
    }
}

struct BitWriter {
    out: Vec<u8>,
    buf: u64,
    cnt: u32,
}

impl BitWriter {
    /// Writes the low `n` (<= 16) bits of `v`, LSB first.
    fn put(&mut self, v: u32, n: u32) {
        self.buf |= (v as u64) << self.cnt;
        self.cnt += n;
        if self.cnt >= 32 {
            self.out.extend_from_slice(&(self.buf as u32).to_le_bytes());
            self.buf >>= 32;
            self.cnt -= 32;
        }
    }

    /// Pads to a byte boundary and flushes all buffered bytes.
    fn align(&mut self) {
        self.cnt = self.cnt.next_multiple_of(8);
        while self.cnt > 0 {
            self.out.push(self.buf as u8);
            self.buf >>= 8;
            self.cnt -= 8;
        }
    }
}

fn write_block(w: &mut BitWriter, tokens: &[Token], raw: &[u8], last: bool) {
    let mut lit_freq = [0u32; 286];
    let mut dist_freq = [0u32; 30];
    for t in tokens {
        if t.len == 0 {
            lit_freq[t.dist as usize] += 1;
        } else {
            lit_freq[257 + len_code(t.len as usize)] += 1;
            dist_freq[dist_code(t.dist as usize)] += 1;
        }
    }
    lit_freq[256] = 1;

    let lit_len = huffman_lengths(&lit_freq, 15);
    let dist_len = huffman_lengths(&dist_freq, 15);
    let nlit = 257.max(lit_len.iter().rposition(|&l| l != 0).unwrap() + 1);
    let ndist = 1.max(dist_len.iter().rposition(|&l| l != 0).map_or(0, |i| i + 1));
    let mut all = lit_len[..nlit].to_vec();
    all.extend_from_slice(&dist_len[..ndist]);
    let rle = rle_lengths(&all);
    let mut cl_freq = [0u32; 19];
    for &(sym, _) in &rle {
        cl_freq[sym as usize] += 1;
    }
    let cl_len = huffman_lengths(&cl_freq, 7);
    let ncl = 4.max(CL_ORDER.iter().rposition(|&i| cl_len[i] != 0).unwrap() + 1);

    let data_bits = |ll: &[u8], dl: &[u8]| -> u64 {
        let mut bits = 0u64;
        for (s, &f) in lit_freq.iter().enumerate() {
            let extra = if s > 256 { LEXT[s - 257] as u64 } else { 0 };
            bits += f as u64 * (ll[s] as u64 + extra);
        }
        for (s, &f) in dist_freq.iter().enumerate() {
            bits += f as u64 * (dl[s] as u64 + DEXT[s] as u64);
        }
        bits
    };
    let (fixed_lit, fixed_dist) = fixed_lengths();
    let mut dynamic_bits = 14 + 3 * ncl as u64 + data_bits(&lit_len, &dist_len);
    for &(sym, _) in &rle {
        dynamic_bits += cl_len[sym as usize] as u64 + [2, 3, 7].get((sym as usize).wrapping_sub(16)).copied().unwrap_or(0);
    }
    let fixed_bits = data_bits(&fixed_lit, &fixed_dist);
    // Stored: per 65535-byte chunk a 3-bit header, up to 7 bits of padding and LEN/NLEN.
    let stored_bits = raw.len().div_ceil(65535).max(1) as u64 * 42 + 8 * raw.len() as u64;

    if stored_bits <= dynamic_bits.min(fixed_bits) {
        let mut chunks = raw.chunks(65535).peekable();
        if raw.is_empty() {
            write_stored(w, &[], last);
        }
        while let Some(chunk) = chunks.next() {
            write_stored(w, chunk, last && chunks.peek().is_none());
        }
        return;
    }
    w.put(last as u32, 1);
    if fixed_bits <= dynamic_bits {
        w.put(1, 2);
        write_tokens(w, tokens, &fixed_lit, &fixed_dist);
    } else {
        w.put(2, 2);
        w.put((nlit - 257) as u32, 5);
        w.put((ndist - 1) as u32, 5);
        w.put((ncl - 4) as u32, 4);
        for &i in &CL_ORDER[..ncl] {
            w.put(cl_len[i] as u32, 3);
        }
        let cl_code = canonical_codes(&cl_len);
        for &(sym, extra) in &rle {
            w.put(cl_code[sym as usize] as u32, cl_len[sym as usize] as u32);
            match sym {
                16 => w.put(extra as u32, 2),
                17 => w.put(extra as u32, 3),
                18 => w.put(extra as u32, 7),
                _ => {}
            }
        }
        write_tokens(w, tokens, &lit_len, &dist_len);
    }
}

fn write_stored(w: &mut BitWriter, chunk: &[u8], last: bool) {
    w.put(last as u32, 1);
    w.put(0, 2);
    w.align();
    w.put(chunk.len() as u32, 16);
    w.put(!chunk.len() as u32 & 0xffff, 16);
    w.align();
    w.out.extend_from_slice(chunk);
}

fn write_tokens(w: &mut BitWriter, tokens: &[Token], lit_len: &[u8], dist_len: &[u8]) {
    let lit_code = canonical_codes(lit_len);
    let dist_code_ = canonical_codes(dist_len);
    for t in tokens {
        if t.len == 0 {
            let b = t.dist as usize;
            w.put(lit_code[b] as u32, lit_len[b] as u32);
        } else {
            let (len, dist) = (t.len as usize, t.dist as usize);
            let lc = len_code(len);
            w.put(lit_code[257 + lc] as u32, lit_len[257 + lc] as u32);
            w.put((len - LBASE[lc] as usize) as u32, LEXT[lc] as u32);
            let dc = dist_code(dist);
            w.put(dist_code_[dc] as u32, dist_len[dc] as u32);
            w.put((dist - DBASE[dc] as usize) as u32, DEXT[dc] as u32);
        }
    }
    w.put(lit_code[256] as u32, lit_len[256] as u32);
}

fn fixed_lengths() -> ([u8; 288], [u8; 30]) {
    let mut l = [0u8; 288];
    l[..144].fill(8);
    l[144..256].fill(9);
    l[256..280].fill(7);
    l[280..].fill(8);
    (l, [5; 30])
}

/// Run-length codes 16/17/18 for a code length sequence, as (symbol, extra bits value).
fn rle_lengths(lens: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lens.len() {
        let l = lens[i];
        let run = lens[i..].iter().take_while(|&&x| x == l).count();
        let mut r = run;
        if l == 0 {
            while r >= 11 {
                let k = r.min(138);
                out.push((18, (k - 11) as u8));
                r -= k;
            }
            if r >= 3 {
                out.push((17, (r - 3) as u8));
                r = 0;
            }
        } else {
            out.push((l, 0));
            r -= 1;
            while r >= 3 {
                let k = r.min(6);
                out.push((16, (k - 3) as u8));
                r -= k;
            }
        }
        out.extend(std::iter::repeat_n((l, 0), r));
        i += run;
    }
    out
}

/// Huffman code lengths for `freq`, limited to `max_bits`. At least two
/// symbols get a code so every tree is complete (zlib's trees.c does the same).
fn huffman_lengths(freq: &[u32], max_bits: usize) -> Vec<u8> {
    let mut syms: Vec<usize> = (0..freq.len()).filter(|&s| freq[s] > 0).collect();
    for s in 0..2 {
        if syms.len() < 2 && !syms.contains(&s) {
            syms.push(s);
        }
    }
    let weight = |s: usize| freq[s].max(1) as u64;
    let m = syms.len();
    // Build the tree; leaves are 0..m, internal nodes m..2m-1.
    let mut parent = vec![0usize; 2 * m - 1];
    let mut heap: BinaryHeap<Reverse<(u64, usize)>> = syms.iter().enumerate().map(|(i, &s)| Reverse((weight(s), i))).collect();
    let mut next = m;
    while heap.len() > 1 {
        let Reverse((w1, a)) = heap.pop().unwrap();
        let Reverse((w2, b)) = heap.pop().unwrap();
        parent[a] = next;
        parent[b] = next;
        heap.push(Reverse((w1 + w2, next)));
        next += 1;
    }
    let root = next - 1;
    let mut count = vec![0u32; 2 * m + max_bits];
    for leaf in 0..m {
        let mut depth = 0;
        let mut x = leaf;
        while x != root {
            x = parent[x];
            depth += 1;
        }
        count[depth.min(max_bits)] += 1;
    }
    // Lengths clamped to max_bits oversubscribe the code; lengthen shorter
    // codes until the Kraft sum is exactly 1 (miniz's enforce_max_code_size).
    let mut total: u64 = (1..=max_bits).map(|i| (count[i] as u64) << (max_bits - i)).sum();
    while total > 1 << max_bits {
        count[max_bits] -= 1;
        for i in (1..max_bits).rev() {
            if count[i] != 0 {
                count[i] -= 1;
                count[i + 1] += 2;
                break;
            }
        }
        total -= 1;
    }
    // Most frequent symbols get the shortest codes.
    syms.sort_by_key(|&s| (Reverse(weight(s)), s));
    let mut lens = vec![0u8; freq.len()];
    let mut it = syms.iter();
    for (len, &c) in count.iter().enumerate().take(max_bits + 1).skip(1) {
        for _ in 0..c {
            lens[*it.next().unwrap()] = len as u8;
        }
    }
    lens
}

/// Canonical codes (RFC 1951 3.2.2), bit-reversed for LSB-first output.
fn canonical_codes(lens: &[u8]) -> Vec<u16> {
    let mut bl_count = [0u16; 16];
    for &l in lens {
        bl_count[l as usize] += 1;
    }
    bl_count[0] = 0;
    let mut next = [0u16; 16];
    let mut code = 0u16;
    for bits in 1..16 {
        code = (code + bl_count[bits - 1]) << 1;
        next[bits] = code;
    }
    lens.iter()
        .map(|&l| {
            if l == 0 {
                return 0;
            }
            let c = next[l as usize];
            next[l as usize] += 1;
            c.reverse_bits() >> (16 - l)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    fn samples() -> Vec<Vec<u8>> {
        let mut rng = Rng::new(7);
        let mut v: Vec<Vec<u8>> = vec![vec![], vec![0], b"a".to_vec(), b"hello hello hello hello".to_vec(), vec![7; 100_000]];
        v.push((0..200_000).map(|_| rng.next_u64() as u8).collect());
        v.push((0..300_000).map(|i| (i % 251) as u8 ^ (i / 997) as u8).collect());
        let words = ["creep", "room", "W12N34", "\"harvest\":", "{\"x\":", "25,", "energy", " "];
        v.push((0..200_000).flat_map(|_| words[rng.below(words.len() as u64) as usize].bytes()).collect());
        // Long-distance repeats near the 32 KiB window edge.
        let block: Vec<u8> = (0..32_770).map(|_| rng.next_u64() as u8).collect();
        v.push(block.iter().chain(block.iter()).copied().collect());
        v
    }

    #[test]
    fn round_trip() {
        for data in samples() {
            let c = deflate(&data);
            assert_eq!(inflate(&c, usize::MAX).unwrap(), data, "len {}", data.len());
            assert_eq!(zlib_decompress(&zlib_compress(&data), usize::MAX).unwrap(), data);
            assert_eq!(gzip_decompress(&gzip_compress(&data), usize::MAX).unwrap(), data);
        }
        // Concatenated gzip members.
        let mut two = gzip_compress(b"abc");
        two.extend(gzip_compress(b"def"));
        assert_eq!(gzip_decompress(&two, usize::MAX).unwrap(), b"abcdef");
    }

    #[test]
    fn compresses() {
        let s = samples();
        assert!(deflate(&s[4]).len() < 400); // 100 KB of one byte
        assert!(deflate(&s[5]).len() < s[5].len() + 100); // random: stored
        assert!(deflate(&s[7]).len() < s[7].len() / 4); // word soup
    }

    #[test]
    fn checksums() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e60398);
        assert_eq!(crc32(b""), 0);
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(&vec![0xff; 100_000]), 0x149a_302c);
    }

    #[test]
    fn output_limit() {
        let c = deflate(&[0; 10_000]);
        assert_eq!(inflate(&c, 10_000).unwrap().len(), 10_000);
        assert!(inflate(&c, 9_999).is_err());
    }

    #[test]
    fn malformed_input_never_panics() {
        let mut rng = Rng::new(99);
        let good: Vec<Vec<u8>> = samples().iter().map(|d| deflate(&d[..d.len().min(5000)])).collect();
        for _ in 0..20_000 {
            let mut c = good[rng.below(good.len() as u64) as usize].clone();
            match rng.below(3) {
                0 if !c.is_empty() => {
                    let n = rng.below(c.len() as u64) as usize;
                    c.truncate(n);
                }
                1 if !c.is_empty() => {
                    for _ in 0..1 + rng.below(4) {
                        let i = rng.below(c.len() as u64) as usize;
                        c[i] ^= 1 << rng.below(8);
                    }
                }
                _ => c = (0..rng.below(300)).map(|_| rng.next_u64() as u8).collect(),
            }
            let _ = inflate(&c, 1 << 20);
            let _ = zlib_decompress(&c, 1 << 20);
            let _ = gzip_decompress(&c, 1 << 20);
        }
    }

    #[test]
    fn codes_are_valid_and_limited() {
        // Fibonacci frequencies force very deep trees.
        let mut f = vec![1u32, 1];
        while f.len() < 40 {
            f.push(f[f.len() - 1] + f[f.len() - 2]);
        }
        let lens = huffman_lengths(&f, 15);
        assert!(lens.iter().all(|&l| (1..=15).contains(&l)));
        let kraft: f64 = lens.iter().map(|&l| 0.5f64.powi(l as i32)).sum();
        assert_eq!(kraft, 1.0);
        for len in 3..=258 {
            assert!(LBASE[len_code(len)] as usize <= len && len < LBASE[len_code(len)] as usize + (1 << LEXT[len_code(len)]) + (len == 258) as usize);
        }
        for d in 1..=32768 {
            assert!(DBASE[dist_code(d)] as usize <= d && d < DBASE[dist_code(d)] as usize + (1 << DEXT[dist_code(d)]));
        }
    }
}
