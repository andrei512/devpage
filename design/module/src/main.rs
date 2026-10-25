//! `cargo run -p devpage-design-module`: builds the design engine for
//! WebAssembly, generates its JavaScript bindings with wasm-bindgen used as a
//! library, and writes both into `assets/vendor/` with the fingerprint of the
//! sources they were built from. `devpage` embeds those files; building
//! devpage itself never builds WebAssembly.

use std::fs;
use std::process::Command;

use devpage_design_module::{BINDINGS, FINGERPRINT, MODULE, fingerprint, record, repository};

fn main() {
    let root = repository();
    let sources = fingerprint(&root);
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let target_dir = root.join("target/design-module");

    let status = Command::new(&cargo)
        .current_dir(&root)
        .args([
            "build",
            "--profile",
            "wasm",
            "--locked",
            "-p",
            "devpage-design-wasm",
        ])
        .args(["--target", "wasm32-unknown-unknown", "--target-dir"])
        .arg(&target_dir)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .status()
        .expect("running cargo to build the design engine for WebAssembly");
    assert!(
        status.success(),
        "building the design engine for WebAssembly failed — is the target installed? \
         rustup target add wasm32-unknown-unknown"
    );

    let out = target_dir.join("bindgen");
    fs::create_dir_all(&out).expect("creating the bindings folder");
    wasm_bindgen_cli_support::Bindgen::new()
        .input_path(target_dir.join("wasm32-unknown-unknown/wasm/devpage_design_wasm.wasm"))
        .out_name("design-engine")
        .web(true)
        .expect("the web target")
        .typescript(false)
        .generate(&out)
        .expect("running wasm-bindgen on the design engine");
    fs::copy(out.join("design-engine_bg.wasm"), root.join(MODULE)).expect("vendoring the module");
    fs::copy(out.join("design-engine.js"), root.join(BINDINGS)).expect("vendoring the bindings");

    let rustc = Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    fs::write(root.join(FINGERPRINT), record(&sources, &rustc)).expect("writing the fingerprint");
    for file in [MODULE, BINDINGS, FINGERPRINT] {
        let size = fs::metadata(root.join(file)).map(|m| m.len()).unwrap_or(0);
        println!("{file} ({size} bytes)");
    }
}
