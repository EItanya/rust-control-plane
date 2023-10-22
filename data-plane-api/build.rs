
#[cfg(feature = "serde")]
use prost_wkt_build::*;

use glob::glob;
use std::io;
use std::path::PathBuf;

#[cfg(not(feature = "serde"))]
fn main() -> io::Result<()> {
    let protos: Vec<PathBuf> = glob("data-plane-api/envoy/**/v3/*.proto")
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    let config = prost_build::Config::new();

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_well_known_types(true)
        .include_file("mod.rs")
        .compile_with_config(
            config,
            &protos,
            &[
                "data-plane-api",
                "googleapis",
                "protoc-gen-validate",
                "xds",
                "opencensus-proto/src",
                "opentelemetry-proto",
                "client_model",
            ],
        )?;
    Ok(())
}

#[cfg(feature = "serde")]
fn main() -> io::Result<()> {
  let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
  let descriptor_file = out.join("descriptors.bin");
  let protos: Vec<PathBuf> = glob("data-plane-api/envoy/**/v3/*.proto")
      .unwrap()
      .filter_map(Result::ok)
      .collect();
  let config = prost_build::Config::new();

  tonic_build::configure()
    .build_server(true)
    .build_client(true)
    .compile_well_known_types(false)
    .include_file("mod.rs")
    .file_descriptor_set_path(&descriptor_file)
    .type_attribute(".", "#[derive(serde::Serialize,serde::Deserialize)]")
    .extern_path(".google.protobuf.Any", "::prost_wkt_types::Any")
    .extern_path(".google.protobuf.Timestamp", "::prost_wkt_types::Timestamp")
    .extern_path(".google.protobuf.Value", "::prost_wkt_types::Value")
    .extern_path(".google.protobuf.Struct", "::prost_wkt_types::Struct")
    .extern_path(".google.protobuf.Duration", "::prost_wkt_types::Duration")
    // .extern_path(".google.protobuf", "::prost_wkt_types")
    .compile_with_config(
        config,
        &protos,
        &[
            "data-plane-api",
            "googleapis",
            "protoc-gen-validate",
            "xds",
            "opencensus-proto/src",
            "opentelemetry-proto",
            "client_model",
        ],
    )?;

  let descriptor_bytes = std::fs::read(descriptor_file).unwrap();

  let descriptor: FileDescriptorSet = FileDescriptorSet::decode(&descriptor_bytes[..]).unwrap();
  
  prost_wkt_build::add_serde(out, descriptor);
  Ok(())
}