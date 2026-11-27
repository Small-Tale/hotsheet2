fn main() {
    // A system protoc is not assumed (HS2-XE0ZVB): use the vendored binary.
    std::env::set_var(
        "PROTOC",
        protoc_bin_vendored::protoc_bin_path().expect("vendored protoc"),
    );
    tonic_build::compile_protos("proto/bench.proto").expect("compile bench.proto");
}
