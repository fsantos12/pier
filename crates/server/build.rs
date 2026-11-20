//! Points `rust-embed` at the built web UI (`crates/web/dist`), or at a placeholder page
//! when the UI has not been built yet, so `cargo build` always works.
//!
//! After rebuilding the UI with `trunk build --release`, run
//! `touch crates/server/src/assets.rs` (or `cargo clean -p pier-server`) so the server
//! picks up the new files.

use std::env;
use std::fs;
use std::path::PathBuf;

const PLACEHOLDER: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Pier</title></head>\
<body><h1>Pier</h1><p>The web UI has not been built into this binary. Run \
<code>cd crates/web &amp;&amp; trunk build --release</code>, then rebuild the server.</p></body></html>\n";

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let dist = manifest.join("../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    println!("cargo:rerun-if-changed=build.rs");

    let chosen = if dist.join("index.html").is_file() {
        dist
    } else {
        let out = PathBuf::from(env::var("OUT_DIR").unwrap_or_default()).join("web-placeholder");
        if fs::create_dir_all(&out).is_ok() {
            let _ = fs::write(out.join("index.html"), PLACEHOLDER);
        }
        println!("cargo:warning=crates/web/dist not found; embedding a placeholder page");
        out
    };
    println!("cargo:rustc-env=PIER_WEB_DIST={}", chosen.display());
}
