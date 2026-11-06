//! Embeds what a snapshot or a release plays into the `kalo-chess` binary (`uci::EMBEDDED_*`), so that
//! one file carries the engine, its network, its tuned parameters and what it is:
//!
//! - `KALO_CHESS_EVALFILE`: the `.nnue` it evaluates with;
//! - `KALO_CHESS_PARAMETERS`: the `parameters.txt` it searches with;
//! - `KALO_CHESS_DESCRIPTION`: a line of text saying what it is, not a path;
//! - `KALO_CHESS_NOTICES`: the third-party notices a release carries (`cargo-about`'s), a path.
//!
//! Each unset (or empty) embeds nothing: the PeSTO evaluation, the untuned parameters, no
//! description, no notices, the binary every development build is.

use std::path::Path;

fn main() {
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR for a build script");
    embed_file("KALO_CHESS_EVALFILE", &Path::new(&out_dir).join("embedded.nnue"));
    embed_file("KALO_CHESS_PARAMETERS", &Path::new(&out_dir).join("embedded-parameters.txt"));
    embed_file("KALO_CHESS_NOTICES", &Path::new(&out_dir).join("embedded-notices.txt"));
    println!("cargo:rerun-if-env-changed=KALO_CHESS_DESCRIPTION");
    let description = std::env::var("KALO_CHESS_DESCRIPTION").unwrap_or_default();
    std::fs::write(Path::new(&out_dir).join("embedded-description.txt"), description.trim())
        .expect("failed to write the embedded description");
}

/// Copies the file the variable `name` names to `target`, or writes `target` empty when it is unset.
fn embed_file(name: &str, target: &Path) {
    println!("cargo:rerun-if-env-changed={name}");
    match std::env::var(name) {
        Ok(path) if !path.is_empty() => {
            println!("cargo:rerun-if-changed={path}");
            std::fs::copy(&path, target).unwrap_or_else(|error| panic!("failed to embed {name} {path}: {error}"));
        }
        _ => std::fs::write(target, []).unwrap_or_else(|error| panic!("failed to write the empty {name}: {error}")),
    }
}
