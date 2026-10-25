//! Compiles the watch stream's messages from `proto/design.proto` with protox,
//! a protobuf compiler in Rust, so building needs no `protoc`.

fn main() {
    println!("cargo:rerun-if-changed=../proto/design.proto");
    let descriptors =
        protox::compile(["design.proto"], ["../proto"]).expect("compiling design.proto");
    prost_build::Config::new()
        .compile_fds(descriptors)
        .expect("generating the design.proto messages");
}
