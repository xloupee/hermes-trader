use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=PROTOC");
    println!("cargo:rerun-if-changed=proto/jito-shredstream.proto");
    println!("cargo:rerun-if-changed=proto/shared.proto");

    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    let include_path = protoc_bin_vendored::include_path()?;

    env::set_var("PROTOC", protoc);

    let include_path = include_path.to_string_lossy().into_owned();
    let includes = ["proto", include_path.as_str()];
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    tonic_prost_build::configure()
        .file_descriptor_set_path(out_dir.join("jito_shredstream_descriptor.bin"))
        .compile_protos(&["proto/jito-shredstream.proto"], &includes)?;

    Ok(())
}
