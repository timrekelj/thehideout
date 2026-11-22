//! libwebrtc's Objective-C categories (used by its video encoder factory) are only linked in
//! with `-ObjC`; without it the first peer connection factory aborts with
//! `+[NSString stringForStdString:]: unrecognized selector`. `rustc-link-arg` only reaches this
//! package's own tests/examples, so a binary that depends on this crate (the desktop app) has
//! to pass the same flag itself, see the README.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-ObjC");
    }
}
