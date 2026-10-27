//! Core's gRPC protocol (proto/, a copy of Core/Protocol/src/main/proto): compiled with protox, so
//! building needs no protoc.
fn main() {
    let root = "proto";
    let dir = format!("{root}/minecomplex/core/v1");
    println!("cargo:rerun-if-changed={root}");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("Core's .proto files")
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".proto"))
        .map(|name| format!("minecomplex/core/v1/{name}"))
        .collect();
    files.sort();
    let descriptors = protox::compile(&files, [root]).expect("Core's .proto files compile");
    tonic_prost_build::configure()
        .build_server(false)
        // SessionService and MessagingService have a `Connect` call: no `connect` constructor.
        .build_transport(false)
        .compile_fds(descriptors)
        .expect("generate Core's client");
}
