//! Keeps `torch_cuda.dll` linked into `smithy`, when torch has one.
//!
//! Nothing in `smithy` calls a CUDA-only symbol directly, so the MSVC linker drops
//! `torch_cuda.dll` as unused; CUDA registers itself with libtorch only once that library is loaded,
//! so `Device::cuda_if_available()` then silently falls back to the CPU. Forcing one of its symbols
//! in keeps the library. `smithy` only: no other target links libtorch.
//!
//! A CPU-only torch wheel (a machine without a GPU, where `smithy` only runs jobs that need no training)
//! has no `torch_cuda.dll`, so nothing would define the forced symbol and the link would fail: the
//! symbol is forced in only when that file exists, the same test torch-sys uses to link
//! `torch_cuda` at all.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    if msvc {
        // `bin/smithy` only runs with this repository's own `.kalo-chess` venv active, so torch's
        // libraries are always here on Windows: the repository root is two levels above this crate.
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
        let torch_lib = std::path::Path::new(&manifest_dir).join("../../.kalo-chess/Lib/site-packages/torch/lib");
        // A directory's mtime changes when files are added or removed in it, so reinstalling torch
        // (CPU wheel to CUDA wheel, or back) reruns this script.
        println!("cargo:rerun-if-changed={}", torch_lib.display());
        if torch_lib.join("torch_cuda.dll").exists() {
            // `int at::cuda::warp_size()`, decorated.
            println!("cargo:rustc-link-arg-bin=smithy=/INCLUDE:?warp_size@cuda@at@@YAHXZ");
        }
    }
}
