//! The cargo profile this binary is built with, for its fuzz failures' reproductions and the manifest
//! check (`zero_worker::fuzz::build_identity`): `release`, `fuzz` (release + debug assertions +
//! overflow checks, Cargo.toml's `[profile.fuzz]`) or `dev`.
fn main() {
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    let debug_assertions = std::env::var_os("CARGO_CFG_DEBUG_ASSERTIONS").is_some();
    let p = match (release, debug_assertions) {
        (true, false) => "release",
        (true, true) => "fuzz",
        _ => "dev",
    };
    println!("cargo:rustc-env=MTGZ_PROFILE={p}");
    println!("cargo:rerun-if-changed=build.rs");
}
