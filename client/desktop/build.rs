//! libwebrtc's Objective-C categories (used by its video encoder factory) are only linked in
//! with `-ObjC`; without it the first peer connection factory aborts with
//! `+[NSString stringForStdString:]: unrecognized selector`. `rustc-link-arg` set by
//! `thehideout-voice`'s build script only reaches that package's own tests and examples, so the
//! desktop binary and its tests pass the flag themselves.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-ObjC");
    }
}
