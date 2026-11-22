use std::path::PathBuf;
use std::process::Command;

// Same approach as the server: build the descriptor set with `buf`, then let
// `connectrpc-build` generate the messages and clients from it.
fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let descriptor = out_dir.join("thehideout.binpb");

    let status = Command::new("buf")
        .current_dir("../..")
        .args(["build", "--as-file-descriptor-set", "-o"])
        .arg(&descriptor)
        .status()
        .expect("failed to run `buf` (is buf installed?)");
    assert!(status.success(), "`buf build` failed");

    connectrpc_build::Config::new()
        .descriptor_set(&descriptor)
        .files(&[
            "thehideout/v1/auth_service.proto",
            "thehideout/v1/gateway_service.proto",
            "thehideout/v1/guild_service.proto",
            "thehideout/v1/message_service.proto",
            "thehideout/v1/dm_service.proto",
            "thehideout/v1/role_service.proto",
            "thehideout/v1/media_service.proto",
            "thehideout/v1/admin_service.proto",
            "thehideout/v1/emoji_service.proto",
            "thehideout/v1/invite_service.proto",
            "thehideout/v1/voice_service.proto",
            "thehideout/v1/attachment.proto",
            "thehideout/v1/types.proto",
        ])
        .include_file("_connectrpc.rs")
        .compile()
        .unwrap();

    println!("cargo:rerun-if-changed=../../proto");
    println!("cargo:rerun-if-changed=../../buf.yaml");
}
