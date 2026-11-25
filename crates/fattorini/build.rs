//! Embeds what a snapshot or a release plays into the `fattorini` binary (`uci::EMBEDDED_*`), so that
//! one file carries the engine, its network, its tuned parameters and what it is:
//!
//! - `FATTORINI_EVALFILE`: the `.nnue` it evaluates with;
//! - `FATTORINI_SEARCH_PARAMETERS`: the `search-parameters.txt` it searches with;
//! - `FATTORINI_TIME_PARAMETERS`: the `time-parameters.txt` it plans its clock with;
//! - `FATTORINI_DESCRIPTION`: a line of text saying what it is, not a path;
//! - `FATTORINI_NOTICES`: the third-party notices a release carries (`cargo-about`'s), a path;
//! - `FATTORINI_RELEASE`: `1` for a release alone (`arena release`), passed on as
//!   `FATTORINI_RELEASE_BUILD`: its UCI name has no `-dev`.
//!
//! Each unset (or empty) embeds nothing: the PeSTO evaluation, the untuned parameters of each kind, no
//! description, no notices, not a release, the binary every development build is.
//!
//! Every build, whatever is set, also carries the logo as the `.exe`'s icon (`docs/logo/fattorini.ico`,
//! rendered by `bin/logo.sh`).

use std::path::Path;

fn main() {
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR for a build script");
    embed_file("FATTORINI_EVALFILE", &Path::new(&out_dir).join("embedded.nnue"));
    embed_file("FATTORINI_SEARCH_PARAMETERS", &Path::new(&out_dir).join("embedded-search-parameters.txt"));
    embed_file("FATTORINI_TIME_PARAMETERS", &Path::new(&out_dir).join("embedded-time-parameters.txt"));
    embed_file("FATTORINI_NOTICES", &Path::new(&out_dir).join("embedded-notices.txt"));
    println!("cargo:rerun-if-env-changed=FATTORINI_DESCRIPTION");
    let description = std::env::var("FATTORINI_DESCRIPTION").unwrap_or_default();
    std::fs::write(Path::new(&out_dir).join("embedded-description.txt"), description.trim())
        .expect("failed to write the embedded description");
    println!("cargo:rerun-if-env-changed=FATTORINI_RELEASE");
    let release = std::env::var("FATTORINI_RELEASE").is_ok_and(|value| value == "1");
    println!("cargo:rustc-env=FATTORINI_RELEASE_BUILD={}", if release { "1" } else { "" });
    embed_icon(Path::new(&out_dir));
}

/// Compiles the icon into a resource the `.exe` links: a one-line `.rc` naming the `.ico` by its
/// absolute path (forward slashes, which `rc.exe` takes), so that where `rc.exe` looks for a
/// relative one does not matter.
fn embed_icon(out_dir: &Path) {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR for a build script");
    let icon = Path::new(&manifest_dir).join("../../docs/logo/fattorini.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let rc = out_dir.join("fattorini.rc");
    let icon_path = icon.display().to_string().replace('\\', "/");
    std::fs::write(&rc, format!("1 ICON \"{icon_path}\"\n")).expect("failed to write the icon's .rc");
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_optional()
        .unwrap_or_else(|error| panic!("failed to compile the icon's resource: {error}"));
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
