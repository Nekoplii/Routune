fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());
    }
    println!("cargo:rerun-if-changed=../proto/routune.proto");
    tonic_build::configure()
        .compile_protos(&["../proto/routune.proto"], &["../proto"])?;
    Ok(())
}
