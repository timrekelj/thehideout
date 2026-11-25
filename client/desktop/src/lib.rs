//! thehideout in the browser: the same app as the desktop binary (`main.rs`), built for
//! wasm32 and started by the `wasm_bindgen(start)` entry below. Empty on native targets.

#![cfg(target_arch = "wasm32")]
#![forbid(unsafe_code)]
// The module list is shared with the desktop binary, which uses items the browser build doesn't
// (and the tests use more): don't warn about the difference.
#![allow(dead_code, unused_imports)]

include!("modules.rs");

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    rune::prewarm_gpu();
    app::spec().run();
}
