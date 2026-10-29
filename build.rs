use std::io::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    prost_build::compile_protos(&["proto/scpf.proto"], &["proto/"])?;

    let include_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("include");
    std::fs::create_dir_all(include_dir.join("scpf"))?;
    std::fs::copy("proto/scpf.proto", include_dir.join("scpf/scpf.proto"))?;
    println!("cargo::metadata=proto_dir={}", include_dir.display());

    Ok(())
}
