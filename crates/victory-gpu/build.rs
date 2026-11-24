//! Fingerprints the shader translator's sources (`victory-shader`), so the
//! shader cache can outlive rebuilds that leave the translator unchanged
//! (see `shader_cache::build`).

use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for e in read.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let shader = manifest.join("../victory-shader");
    let mut list = Vec::new();
    files(&shader.join("src"), &mut list);
    list.push(shader.join("Cargo.toml"));
    // The workspace lock pins the translator's dependencies.
    list.push(manifest.join("../../Cargo.lock"));
    list.sort();
    // The cache's own entry encoding.
    list.push(manifest.join("src/shader_cache.rs"));
    // FNV-1a, 64-bit, over each file's path (relative) and bytes.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: &[u8]| {
        for &x in b {
            h ^= u64::from(x);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for p in &list {
        let rel = p.strip_prefix(&shader).unwrap_or(p);
        eat(rel.to_string_lossy().replace('\\', "/").as_bytes());
        eat(&std::fs::read(p).unwrap_or_default());
        println!("cargo:rerun-if-changed={}", p.display());
    }
    println!("cargo:rerun-if-changed={}", shader.join("src").display());
    println!("cargo:rustc-env=VICTORY_TRANSLATOR_FINGERPRINT={h:016x}");
}
