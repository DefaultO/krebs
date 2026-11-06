//! The Rust mods in `mods/` (ARCHITECTURE.md §10.1): every crate there is a dependency of this one and an entry of
//! `MODS`, both kept by build.rs. The server enables them by name (`mods` in the config, `--mods`).

include!(concat!(env!("OUT_DIR"), "/mods.rs"));
