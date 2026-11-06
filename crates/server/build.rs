//! Embeds chitin.manifest into the Windows binary: the process heap becomes the Segment Heap (§2.9), which the
//! runner and room threads share without the NT heap's lock contention. Older Windows ignores the setting.

fn main() {
    println!("cargo:rerun-if-changed=chitin.manifest");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("chitin.manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    }
}
