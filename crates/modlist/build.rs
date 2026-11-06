//! Finds the mod crates in `<workspace>/mods/*/` and makes them this crate's dependencies and its `MODS` list.
//!
//! Cargo reads manifests before any build script runs, so a dependency can only be added for the next build: when
//! the crates under mods/ differ from the generated block in this crate's Cargo.toml, the block is rewritten and the
//! build stops with a message to run it again. `MODS` names only crates the block already had, so the stopped
//! build never refers to one Cargo did not resolve.
//!
//! The block is empty in the repository: a mod is a git submodule, and a clone without submodules has empty folders
//! here, which a listed path dependency would fail on before this script could run. So the core names no mod, and
//! `run.bat` marks this file skip-worktree where mods are present.

use std::path::{Path, PathBuf};

const BEGIN: &str = "# BEGIN MODS: written by build.rs from mods/*/Cargo.toml, do not edit";
const END: &str = "# END MODS";

/// `name = "…"` of the `[package]` table.
fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package {
            if let Some(v) = line.strip_prefix("name").map(str::trim).and_then(|v| v.strip_prefix('=')) {
                return Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

fn main() {
    let here = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mods_dir = here.join("../../mods");
    println!("cargo:rerun-if-changed={}", mods_dir.display());
    println!("cargo:rerun-if-changed=Cargo.toml");

    // (crate name, directory name), by directory name
    let mut found: Vec<(String, String)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&mods_dir) {
        for e in entries.flatten() {
            let manifest = e.path().join("Cargo.toml");
            let Ok(text) = std::fs::read_to_string(&manifest) else { continue };
            let name = package_name(&text).unwrap_or_else(|| panic!("{}: no [package] name", manifest.display()));
            found.push((name, e.file_name().to_string_lossy().into_owned()));
        }
    }
    found.sort_by(|a, b| a.1.cmp(&b.1));

    let manifest_path = here.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    let (start, end) = match (manifest.find(BEGIN), manifest.find(END)) {
        (Some(s), Some(e)) if s < e => (s + BEGIN.len(), e),
        _ => panic!("{}: the {BEGIN:?} … {END:?} block is missing", manifest_path.display()),
    };
    let have = &manifest[start..end];
    let want: String = found.iter().map(|(name, dir)| format!("\n{name} = {{ path = \"../../mods/{dir}\" }}")).collect::<String>() + "\n";

    // the crates Cargo resolved for this build
    let linked: Vec<&str> = have.lines().filter_map(|l| l.split_once(" = ").map(|(n, _)| n.trim())).collect();
    let list: String = linked.iter().map(|n| format!("    &::{}::MOD,\n", n.replace('-', "_"))).collect();
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("mods.rs");
    std::fs::write(out, format!("/// Every mod crate under mods/, by directory name.\npub static MODS: &[&dyn backend::mods::Mod] = &[\n{list}];\n")).unwrap();

    if have != want {
        std::fs::write(&manifest_path, format!("{}{want}{}", &manifest[..start], &manifest[end..])).unwrap();
        let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
        let names = if names.is_empty() { "no mods".to_string() } else { names.join(", ") };
        eprintln!("\nThe mods folder changed, so crates/modlist/Cargo.toml now lists: {names}.");
        eprintln!("This stop is expected: Cargo has to read that file again. Run the same build once more. run.bat does this by itself.\n");
        std::process::exit(1);
    }
}
