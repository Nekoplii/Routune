fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../proto/routune.proto");
    tonic_build::configure()
        .compile_protos(&["../proto/routune.proto"], &["../proto"])?;
    Ok(())
}
