//! Builds the design engine for WebAssembly, which the binary embeds as the
//! built-in asset `/__devpage/design.wasm`, and generates the server side of
//! the watch stream from `proto/design.proto`.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // The watch stream's server side; its messages are the design engine's.
    println!("cargo:rerun-if-changed=proto/design.proto");
    let descriptors = protox::compile(["design.proto"], ["proto"]).expect("compiling design.proto");
    tonic_prost_build::configure()
        .build_client(false)
        .extern_path(".devpage.design.v1", "::devpage_design::proto")
        .compile_fds(descriptors)
        .expect("generating the Designs service");

    println!("cargo:rerun-if-changed=design/src");
    println!("cargo:rerun-if-changed=design/wasm/src");
    println!("cargo:rerun-if-changed=design/Cargo.toml");
    println!("cargo:rerun-if-changed=design/wasm/Cargo.toml");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_dir = out.join("wasm");
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "devpage-design-wasm",
        ])
        .args(["--target", "wasm32-unknown-unknown", "--target-dir"])
        .arg(&target_dir)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .expect("running cargo to build the design engine for WebAssembly");
    assert!(
        status.success(),
        "building the design engine for WebAssembly failed — is the target installed? \
         rustup target add wasm32-unknown-unknown"
    );
    std::fs::copy(
        target_dir.join("wasm32-unknown-unknown/release/devpage_design_wasm.wasm"),
        out.join("design.wasm"),
    )
    .expect("copying design.wasm");
}
