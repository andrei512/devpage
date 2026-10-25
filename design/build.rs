//! Compiles the watch stream's messages from `proto/design.proto` with protox,
//! a protobuf compiler in Rust, so building needs no `protoc`. With the
//! `server` feature it also generates the server side of the `Designs`
//! service, which `devpage` serves; the WebAssembly module leaves it out.

fn main() {
    println!("cargo:rerun-if-changed=proto/design.proto");
    let descriptors = protox::compile(["design.proto"], ["proto"]).expect("compiling design.proto");
    tonic_prost_build::configure()
        .build_client(false)
        .build_server(std::env::var_os("CARGO_FEATURE_SERVER").is_some())
        .compile_fds(descriptors)
        .expect("generating the design.proto messages");
}
