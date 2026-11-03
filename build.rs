fn main() {
    println!(
        "cargo:rustc-env=NATIVE_EXTENSION_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
}
