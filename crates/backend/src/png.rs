//! mirror: reference/backend/lib/utils.js
//! parity: DIVERGENT(FMT-MAP-PNG): tiles are rendered on demand from terrain and cached instead of being written by
//! the CLI; pixels are identical (createTerrainColorsMap, map.js updateRoomImageAssets layout), the PNG bytes are
//! not (pngjs filters/compresses differently)
//!
//! `/assets/map/*.png` from terrain (writeTerrainToPng, FMT-MAP-PNG, util::deflate), cached. Owner: WP04.

use crate::http::{Request, Response};
use crate::{room_idx, ApiCtx};
use engine::room_name::RoomXY;
use engine::terrain::RoomTerrain;
use std::collections::HashMap;
use engine::world::TerrainStore;
use std::sync::{Arc, Mutex, OnceLock, Weak};

/// utils.createTerrainColorsMap for one tile (alpha 255).
pub fn tile_color(t: &RoomTerrain, x: u8, y: u8) -> [u8; 4] {
    let code = t.raw(x, y);
    if code & 1 != 0 {
        [0, 0, 0, 255]
    } else if code & 2 != 0 {
        [35, 37, 19, 255]
    } else if x == 0 || y == 0 || x == 49 || y == 49 {
        [50, 50, 50, 255]
    } else {
        [43, 43, 43, 255]
    }
}

/// An 8-bit RGBA PNG (utils.writePng), scanlines unfiltered.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(ty);
        out.extend_from_slice(data);
        let crc = util::deflate::crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let row = width as usize * 4;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgba.chunks(row) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &util::deflate::zlib_compress(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// `map/<room>.png`: 150×150, 3×3 pixels per tile (writeTerrainToPng zoomIn).
pub fn room_png(t: &RoomTerrain) -> Vec<u8> {
    let mut px = vec![0u8; 150 * 150 * 4];
    for y in 0..150usize {
        for x in 0..150usize {
            px[(y * 150 + x) * 4..][..4].copy_from_slice(&tile_color(t, (x / 3) as u8, (y / 3) as u8));
        }
    }
    encode_png(150, 150, &px)
}

/// `map/zoom2/<R>.png`: 200×200, the 4×4 rooms block starting at R, 1 pixel per tile, missing rooms transparent.
pub fn zoom2_png(block: &[[Option<&RoomTerrain>; 4]; 4]) -> Vec<u8> {
    let mut px = vec![0u8; 200 * 200 * 4];
    for (xx, col) in block.iter().enumerate() {
        for (yy, t) in col.iter().enumerate() {
            let Some(t) = t else { continue };
            for cy in 0..50usize {
                for cx in 0..50usize {
                    let (x, y) = (cx + xx * 50, cy + yy * 50);
                    px[(y * 200 + x) * 4..][..4].copy_from_slice(&tile_color(t, cx as u8, cy as u8));
                }
            }
        }
    }
    encode_png(200, 200, &px)
}

/// Rendered tiles of one terrain store, by path. The Weak pins the store's allocation, so a new store can never
/// reuse its address and inherit stale tiles.
// ponytail: cleared wholesale past 20 000 tiles; an LRU only if map browsing at scale shows churn.
type Tiles = (Weak<TerrainStore>, HashMap<String, Arc<Vec<u8>>>);

fn cache() -> &'static Mutex<Tiles> {
    static C: OnceLock<Mutex<Tiles>> = OnceLock::new();
    C.get_or_init(|| Mutex::new((Weak::new(), HashMap::new())))
}

/// The cached tile for this terrain store, if any (a different store empties the cache).
fn cached(terrain: &Arc<TerrainStore>, path: &str) -> Option<Arc<Vec<u8>>> {
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if !c.0.ptr_eq(&Arc::downgrade(terrain)) {
        *c = (Arc::downgrade(terrain), HashMap::new());
    }
    c.1.get(path).cloned()
}

/// `GET /assets/map/<room>.png`
pub fn map_png(req: &Request, ctx: &ApiCtx) -> Response {
    let view = ctx.published.get();
    let rest = req.path.strip_prefix("/assets/map/").unwrap_or("");
    let (zoom2, file) = match rest.strip_prefix("zoom2/") {
        Some(f) => (true, f),
        None => (false, rest),
    };
    if let Some(png) = cached(&view.terrain, rest) {
        return png_response(&png);
    }
    let Some(name) = file.strip_suffix(".png") else { return Response::not_found(&req.method, &req.path) };
    let png = if zoom2 {
        // only block-aligned names exist (map.js names the file after floor(x/4)*4, floor(y/4)*4)
        let xy = name.parse::<RoomXY>().ok().filter(|xy| xy.to_string() == name && xy.x.rem_euclid(4) == 0 && xy.y.rem_euclid(4) == 0);
        let Some(xy) = xy else { return Response::not_found(&req.method, &req.path) };
        let rooms: Vec<Vec<Option<u32>>> = (0..4).map(|xx| (0..4).map(|yy| room_idx(&view, &RoomXY::new(xy.x + xx, xy.y + yy).to_string())).collect()).collect();
        if rooms.iter().flatten().all(Option::is_none) {
            return Response::not_found(&req.method, &req.path);
        }
        let terrains: Vec<Vec<Option<RoomTerrain>>> = rooms.iter().map(|c| c.iter().map(|r| r.map(|r| crate::terrain_of(&view, r))).collect()).collect();
        let block: [[Option<&RoomTerrain>; 4]; 4] = std::array::from_fn(|xx| std::array::from_fn(|yy| terrains[xx][yy].as_ref()));
        zoom2_png(&block)
    } else {
        match room_idx(&view, name) {
            Some(r) => room_png(&crate::terrain_of(&view, r)),
            None => return Response::not_found(&req.method, &req.path),
        }
    };
    let png = Arc::new(png);
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if c.0.ptr_eq(&Arc::downgrade(&view.terrain)) {
        if c.1.len() > 20_000 {
            c.1.clear();
        }
        c.1.insert(rest.to_string(), png.clone());
    }
    png_response(&png)
}

fn png_response(png: &[u8]) -> Response {
    Response { status: 200, headers: vec![("Content-Type".into(), "image/png".into()), ("Cache-Control".into(), "public, max-age=0".into())], body: png.to_vec() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use util::json::Json;

    /// Decodes our own PNG: checks the chunk CRCs and returns (width, height, RGBA pixels).
    fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let (mut i, mut w, mut h, mut idat) = (8, 0, 0, Vec::new());
        while i < png.len() {
            let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
            let ty = &png[i + 4..i + 8];
            let data = &png[i + 8..i + 8 + len];
            let crc = u32::from_be_bytes(png[i + 8 + len..i + 12 + len].try_into().unwrap());
            assert_eq!(crc, util::deflate::crc32(&png[i + 4..i + 8 + len]));
            match ty {
                b"IHDR" => {
                    w = u32::from_be_bytes(data[..4].try_into().unwrap());
                    h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                    assert_eq!(&data[8..], &[8, 6, 0, 0, 0]);
                }
                b"IDAT" => idat.extend_from_slice(data),
                _ => {}
            }
            i += 12 + len;
        }
        let raw = util::deflate::zlib_decompress(&idat, 1 << 24).unwrap();
        let mut px = Vec::new();
        for line in raw.chunks(w as usize * 4 + 1) {
            assert_eq!(line[0], 0);
            px.extend_from_slice(&line[1..]);
        }
        (w, h, px)
    }

    /// Pixels against the official utils.createTerrainColorsMap (tests/fixtures/png/colors.json).
    #[test]
    fn pixels_match_node() {
        let fx = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/png/colors.json")).unwrap();
        let fx = Json::parse(&fx).unwrap();
        let t = RoomTerrain::from_db_str(fx.get("terrain").unwrap().as_str().unwrap()).unwrap();
        let colors = fx.get("colors").unwrap();
        let want = |x: usize, y: usize| -> [u8; 4] {
            let c = colors.at(y).unwrap().at(x).unwrap();
            [0, 1, 2].map(|i| c.at(i).unwrap().as_f64().unwrap() as u8).iter().chain([255].iter()).copied().collect::<Vec<_>>().try_into().unwrap()
        };
        let (w, h, px) = decode(&room_png(&t));
        assert_eq!((w, h), (150, 150));
        for y in 0..150 {
            for x in 0..150 {
                assert_eq!(&px[(y * 150 + x) * 4..][..4], &want(x / 3, y / 3), "room.png {x},{y}");
            }
        }
        let mut block: [[Option<&RoomTerrain>; 4]; 4] = [[None; 4]; 4];
        block[1][2] = Some(&t);
        let (w, h, px) = decode(&zoom2_png(&block));
        assert_eq!((w, h), (200, 200));
        for y in 0..200 {
            for x in 0..200 {
                let got = &px[(y * 200 + x) * 4..][..4];
                if (50..100).contains(&x) && (100..150).contains(&y) {
                    assert_eq!(got, &want(x - 50, y - 100));
                } else {
                    assert_eq!(got, &[0, 0, 0, 0]);
                }
            }
        }
    }
}
