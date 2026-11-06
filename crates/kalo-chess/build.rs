//! Embeds the network `KALO_CHESS_EVALFILE` names into the `kalo-chess` binary (`uci::EMBEDDED_NETWORK`), so
//! that one file carries engine and network: what a release will ship. Unset, the embedded file is
//! empty and the binary is as it always was.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=KALO_CHESS_EVALFILE");
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR for a build script");
    let target = Path::new(&out_dir).join("embedded.nnue");
    match std::env::var("KALO_CHESS_EVALFILE") {
        Ok(path) if !path.is_empty() => {
            println!("cargo:rerun-if-changed={path}");
            std::fs::copy(&path, &target).unwrap_or_else(|error| panic!("failed to embed KALO_CHESS_EVALFILE {path}: {error}"));
        }
        _ => std::fs::write(&target, []).expect("failed to write the empty embedded network"),
    }
}
