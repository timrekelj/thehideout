//! Pictures: avatars and server icons. Bytes come from `Backend` (`Command::FetchMedia`, fetched
//! on the network runtime, never in a frame); `MediaCache` hands them to Rune's `ImageCache`
//! (which decodes on its own worker and wakes the window when a picture is ready) and keeps the
//! decoded handles in a bounded LRU, so a key is fetched and decoded once however many rows draw
//! it. Also here: reading a picked file for upload (run off-thread by the backend) and a tiny PNG
//! writer the fake backend and the tests make pictures with.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use rune::{Image, ImageCache, Pixels, Waker};
use thehideout_api::{ApiError, MAX_MEDIA_BYTES};

/// What the cache may hold, in bytes of decoded pictures (an estimate, see [`COST_FLOOR`]).
pub const BUDGET_BYTES: usize = 64 * 1024 * 1024;
/// The least a cached picture is charged. The server stores 256x256 pictures, whose decoded
/// pixels and mip levels come to a little over 256 KiB; a bigger encoded file is charged its size.
pub const COST_FLOOR: usize = 256 * 1024;
/// The least a cached emoji is charged: a 128x128 picture with its mip levels is about 87 KiB, so
/// two hundred of them fit in a quarter of the budget.
pub const EMOJI_FLOOR: usize = 96 * 1024;
/// An animated emoji decodes to every frame at once: its encoded size understates it by about this much.
const ANIMATION_FACTOR: usize = 8;

/// Extensions the pickers and the drop zones accept.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

/// The content type of an image file by its extension, or `None` for anything the server won't take.
pub fn image_type(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// The content type of `path` if it names an image the server takes and `len` bytes fit.
fn check_image(path: &Path, len: u64) -> Result<&'static str, ApiError> {
    let kind = image_type(path)
        .ok_or_else(|| ApiError::InvalidArgument("choose a PNG, JPEG, WebP or GIF image".into()))?;
    if len > MAX_MEDIA_BYTES as u64 {
        return Err(ApiError::InvalidArgument("that image is over 8 MiB".into()));
    }
    Ok(kind)
}

/// The browser's [`read_image`]: the picked file behind `path`, read asynchronously.
#[cfg(target_arch = "wasm32")]
pub async fn read_web_image(path: &Path) -> Result<(Vec<u8>, String), ApiError> {
    let unreadable = || ApiError::InvalidArgument("can't read that file".into());
    let blob = crate::web::file_blob(path).ok_or_else(unreadable)?;
    let kind = check_image(path, crate::web::file_size(path).ok_or_else(unreadable)?)?;
    let bytes = crate::web::read_blob(&blob).await.map_err(|_| unreadable())?;
    Ok((bytes, kind.to_owned()))
}

/// Reads a picked image: its bytes and content type. Blocking, so the backend runs it off the
/// frame loop. Refuses other file types and anything over [`MAX_MEDIA_BYTES`] without reading it.
pub fn read_image(path: &Path) -> Result<(Vec<u8>, String), ApiError> {
    // The type is refused before the file is touched.
    check_image(path, 0)?;
    let meta = std::fs::metadata(path)
        .map_err(|e| ApiError::InvalidArgument(format!("can't read that file: {e}")))?;
    let kind = check_image(path, meta.len())?;
    let bytes = std::fs::read(path)
        .map_err(|e| ApiError::InvalidArgument(format!("can't read that file: {e}")))?;
    Ok((bytes, kind.to_owned()))
}

struct Entry {
    image: Image,
    /// The first frame of an animated picture, drawn when motion is reduced and while at rest in
    /// the picker. Freed with the entry.
    still: Option<Image>,
    cost: usize,
    used: Cell<u64>,
}

/// The first frame of a GIF as straight RGBA, decoded here (a GIF's first frame is cheap) so a
/// still can sit beside the animation. `None` for anything else, or a GIF that doesn't decode.
pub fn first_frame(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if !bytes.starts_with(b"GIF8") {
        return None;
    }
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Gif).ok()?.into_rgba8();
    Some((img.width(), img.height(), img.into_raw()))
}

/// Decoded pictures by media key. Views read it (`pixels`, which only stamps recency); the
/// reducers fill it (`insert`) and decide what to ask for (`want`).
pub struct MediaCache {
    images: ImageCache,
    entries: HashMap<String, Entry>,
    /// Keys asked for and not answered yet.
    pending: HashSet<String>,
    /// Keys that failed to download, or were pushed out to make room: not asked for again until
    /// `forgive` (a new visit) or until the owner's key changes.
    skipped: HashSet<String>,
    bytes: usize,
    budget: usize,
    clock: Cell<u64>,
}

impl Default for MediaCache {
    fn default() -> Self {
        MediaCache::with_waker(Arc::new(|| {}))
    }
}

impl MediaCache {
    pub fn with_waker(waker: Waker) -> MediaCache {
        MediaCache {
            images: ImageCache::new(waker),
            entries: HashMap::new(),
            pending: HashSet::new(),
            skipped: HashSet::new(),
            bytes: 0,
            budget: BUDGET_BYTES,
            clock: Cell::new(0),
        }
    }

    /// A cache that holds at most `budget` bytes (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_budget(mut self, budget: usize) -> MediaCache {
        self.budget = budget;
        self
    }

    /// Swaps in the shell's waker, which Rune hands over once at setup. Pictures decoded after
    /// that wake the window; anything cached before keeps working.
    pub fn set_waker(&mut self, waker: Waker) {
        self.images = ImageCache::new(waker);
    }

    /// Forgets everything (another account signs in).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.pending.clear();
        self.skipped.clear();
        self.bytes = 0;
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Bytes charged to the cached pictures.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Whether `key` should be fetched: not cached, not on its way, not given up on.
    pub fn wants(&self, key: &str) -> bool {
        !self.entries.contains_key(key)
            && !self.pending.contains(key)
            && !self.skipped.contains(key)
    }

    /// `key` is being fetched.
    pub fn ask(&mut self, key: &str) {
        self.pending.insert(key.to_owned());
    }

    /// The download of `key` failed: it stays plain initials.
    pub fn fail(&mut self, key: &str) {
        self.pending.remove(key);
        self.skipped.insert(key.to_owned());
    }

    /// Lets evicted pictures be fetched again (the user came back to a place that shows them).
    pub fn forgive(&mut self) {
        self.skipped.clear();
    }

    /// `key`'s downloaded bytes: handed to the decoder, and the oldest pictures pushed out
    /// while the cache is over budget (never the one just added).
    pub fn insert(&mut self, key: &str, bytes: Vec<u8>) {
        self.insert_charged(key, bytes, COST_FLOOR, false);
    }

    /// [`insert`](Self::insert) for a custom emoji: charged [`EMOJI_FLOOR`] (more when animated),
    /// and an animated one also gets its still.
    pub fn insert_emoji(&mut self, key: &str, bytes: Vec<u8>) {
        self.insert_charged(key, bytes, EMOJI_FLOOR, true);
    }

    fn insert_charged(&mut self, key: &str, bytes: Vec<u8>, floor: usize, with_still: bool) {
        self.pending.remove(key);
        let animated = bytes.starts_with(b"GIF8");
        let cost = if animated { bytes.len() * ANIMATION_FACTOR } else { bytes.len() }.max(floor);
        let still = with_still.then(|| first_frame(&bytes)).flatten().map(|(w, h, raw)| self.images.rgba(w, h, raw));
        let image = self.images.load(bytes);
        let stamp = self.touch();
        if let Some(old) = self.entries.insert(
            key.to_owned(),
            Entry {
                image,
                still,
                cost,
                used: Cell::new(stamp),
            },
        ) {
            self.bytes -= old.cost;
        }
        self.bytes += cost;
        while self.bytes > self.budget && self.entries.len() > 1 {
            let oldest = self
                .entries
                .iter()
                .filter(|(k, _)| k.as_str() != key)
                .min_by_key(|(_, e)| e.used.get())
                .map(|(k, _)| k.clone());
            let Some(oldest) = oldest else { break };
            if let Some(e) = self.entries.remove(&oldest) {
                self.bytes -= e.cost;
                self.skipped.insert(oldest);
            }
        }
    }

    /// The picture for `key` to draw: its pixels as soon as it is cached (they paint nothing until
    /// the decode lands, and the window wakes then), `None` if unknown or undecodable, so the
    /// caller's initials show. Counts as a use for the LRU.
    pub fn pixels(&self, key: &str) -> Option<Pixels> {
        let e = self.entries.get(key)?;
        if e.image.error().is_some() {
            return None;
        }
        e.used.set(self.touch());
        Some(e.image.pixels().clone())
    }

    /// The first frame of the animated picture `key`, when it has one (see [`first_frame`]).
    pub fn still(&self, key: &str) -> Option<Pixels> {
        let e = self.entries.get(key)?;
        e.still.as_ref().map(|s| s.pixels().clone())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// A one-off decode of picked bytes (the preview in a dialog); not cached, freed with the handle.
    pub fn preview(&self, bytes: Vec<u8>) -> Image {
        self.images.load(bytes)
    }

    /// Waits for every pending decode: for headless scenes and tests, which have no window to wake.
    pub fn settle(&self) {
        for e in self.entries.values() {
            let _ = e.image.wait();
            if let Some(s) = &e.still {
                let _ = s.wait();
            }
        }
    }

    fn touch(&self) -> u64 {
        self.clock.set(self.clock.get() + 1);
        self.clock.get()
    }
}

// ---- a tiny PNG writer -----------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// A `w` x `h` RGB PNG (stored, not compressed) filled by `pixel(x, y)`.
pub fn png(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((h * (w * 3 + 1)) as usize);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend_from_slice(&pixel(x, y));
        }
    }
    // zlib: a header, stored blocks of at most 65535 bytes, the Adler-32 of the raw data.
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A deterministic picture for `seed`: a diagonal gradient between two hues taken from it, with a
/// light disc, so scenes tell the fake users' avatars apart and see the crop.
pub fn generated_png(seed: u64, side: u32) -> Vec<u8> {
    let hue = |s: u64| -> [f32; 3] {
        let h = ((s.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40) % 360) as f32;
        // HSV with s .55, v .85.
        let (c, x) = (
            0.85 * 0.55,
            0.85 * 0.55 * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs()),
        );
        let m = 0.85 - c;
        let (r, g, b) = match (h / 60.0) as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        [r + m, g + m, b + m]
    };
    let (from, to) = (hue(seed), hue(seed ^ 0x5555));
    let half = side as f32 / 2.0;
    png(side, side, |x, y| {
        let t = (x + y) as f32 / (2 * side) as f32;
        let d = ((x as f32 - half).hypot(y as f32 - half * 0.9)) / half;
        let lift = if d < 0.42 { 0.28 } else { 0.0 };
        let mix = |a: f32, b: f32| ((a + (b - a) * t + lift).clamp(0.0, 1.0) * 255.0) as u8;
        [
            mix(from[0], to[0]),
            mix(from[1], to[1]),
            mix(from[2], to[2]),
        ]
    })
}

/// A deterministic `w` x `h` picture for `seed`: the avatar's gradient, stretched, so scenes
/// have photographs of any shape.
pub fn generated_picture(seed: u64, w: u32, h: u32) -> Vec<u8> {
    let hue = |s: u64| -> [f32; 3] {
        let a = ((s.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40) % 360) as f32;
        let (c, x) = (0.85 * 0.55, 0.85 * 0.55 * (1.0 - ((a / 60.0) % 2.0 - 1.0).abs()));
        let m = 0.85 - c;
        let (r, g, b) = match (a / 60.0) as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        [r + m, g + m, b + m]
    };
    let (from, to) = (hue(seed), hue(seed ^ 0x5555));
    png(w, h, |px, py| {
        let t = (px as f32 / w as f32 + py as f32 / h as f32) / 2.0;
        // A soft horizon and a disc, so crops and aspect show.
        let d = ((px as f32 / w as f32 - 0.7).hypot((py as f32 / h as f32 - 0.35) * h as f32 / w as f32)) / 0.18;
        let lift = if d < 1.0 { 0.25 } else if py as f32 > h as f32 * 0.75 { -0.12 } else { 0.0 };
        let mix = |a: f32, b: f32| ((a + (b - a) * t + lift).clamp(0.0, 1.0) * 255.0) as u8;
        [mix(from[0], to[0]), mix(from[1], to[1]), mix(from[2], to[2])]
    })
}

/// A deterministic static emoji for `seed`: a round face on a transparent square, so scenes see
/// how emojis sit in a line of text.
pub fn generated_emoji(seed: u64, side: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let frame = emoji_frame(seed, side, 0.0);
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    image::ImageEncoder::write_image(enc, frame.as_raw(), side, side, image::ExtendedColorType::Rgba8).expect("encodes in memory");
    out
}

/// A deterministic animated GIF emoji: the same face bobbing over `frames` frames.
pub fn generated_animated_emoji(seed: u64, side: u32, frames: u32) -> Vec<u8> {
    use image::{Delay, Frame, codecs::gif::{GifEncoder, Repeat}};
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite).expect("encodes in memory");
        for i in 0..frames {
            let phase = i as f32 / frames as f32 * std::f32::consts::TAU;
            let delay = Delay::from_numer_denom_ms(80, 1);
            enc.encode_frame(Frame::from_parts(emoji_frame(seed, side, phase), 0, 0, delay)).expect("encodes in memory");
        }
    }
    out
}

/// One frame of the generated emoji: a disc in the seed's hue with eyes and a mouth; `phase` moves
/// the face up and down and squashes the mouth.
fn emoji_frame(seed: u64, side: u32, phase: f32) -> image::RgbaImage {
    let hue = ((seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40) % 360) as f32;
    let (c, x) = (0.9 * 0.6, 0.9 * 0.6 * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs()));
    let m = 0.9 - c;
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let body = [((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8];
    let s = side as f32;
    let bob = phase.sin() * s * 0.04;
    image::RgbaImage::from_fn(side, side, |px, py| {
        let (fx, fy) = ((px as f32 + 0.5) / s - 0.5, (py as f32 + 0.5) / s - 0.5 - bob / s);
        let d = fx.hypot(fy);
        let eye = |ex: f32| (fx - ex).hypot(fy + 0.1) < 0.07;
        let mouth = fy > 0.08 && fy < 0.08 + 0.1 * (0.6 + 0.4 * phase.cos().abs()) && fx.abs() < 0.2 && (fx * fx / 0.04 + (fy - 0.08).powi(2) / 0.01) < 1.6;
        if d > 0.46 {
            image::Rgba([0, 0, 0, 0])
        } else if eye(-0.15) || eye(0.15) || mouth {
            image::Rgba([40, 30, 30, 255])
        } else {
            image::Rgba([body[0], body[1], body[2], 255])
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pic(seed: u64) -> Vec<u8> {
        generated_png(seed, 16)
    }

    #[test]
    fn the_generated_png_decodes() {
        let cache = MediaCache::default();
        let image = cache.preview(generated_png(7, 64));
        assert_eq!(image.wait().ok(), Some([64, 64]));
    }

    #[test]
    fn extensions_map_to_content_types() {
        assert_eq!(image_type(Path::new("/a/b.PNG")), Some("image/png"));
        assert_eq!(image_type(Path::new("x.jpeg")), Some("image/jpeg"));
        assert_eq!(image_type(Path::new("x.webp")), Some("image/webp"));
        assert_eq!(image_type(Path::new("x.gif")), Some("image/gif"));
        assert_eq!(image_type(Path::new("x.svg")), None);
        assert_eq!(image_type(Path::new("noext")), None);
    }

    #[test]
    fn a_fetched_picture_is_cached_and_never_asked_for_twice() {
        let mut c = MediaCache::default();
        assert!(c.wants("k"));
        c.ask("k");
        assert!(!c.wants("k"), "in flight");
        c.insert("k", pic(1));
        assert!(!c.wants("k") && c.contains("k"));
        assert!(c.pixels("k").is_some());
        assert!(c.pixels("other").is_none());
        c.fail("bad");
        assert!(!c.wants("bad"), "a failed key is left alone");
        c.forgive();
        assert!(c.wants("bad"));
    }

    #[test]
    fn the_cache_stays_within_its_byte_budget_and_drops_the_least_recently_used() {
        let mut c = MediaCache::default().with_budget(3 * COST_FLOOR);
        for i in 0..3 {
            c.insert(&format!("k{i}"), pic(i));
        }
        assert_eq!((c.len(), c.bytes()), (3, 3 * COST_FLOOR));
        // k0 is used again, so k1 is the oldest.
        assert!(c.pixels("k0").is_some());
        c.insert("k3", pic(3));
        assert_eq!(c.len(), 3, "one went out to make room");
        assert!(c.contains("k0") && c.contains("k3") && c.contains("k2"));
        assert!(!c.contains("k1"), "the least recently used was dropped");
        assert!(c.bytes() <= 3 * COST_FLOOR);
        assert!(
            !c.wants("k1"),
            "an evicted key is not fetched again on its own"
        );
        c.forgive();
        assert!(c.wants("k1"));
        // A flood never exceeds the budget.
        for i in 10..40 {
            c.insert(&format!("k{i}"), pic(i));
            assert!(c.bytes() <= 3 * COST_FLOOR && c.len() <= 3);
        }
    }

    #[test]
    fn a_big_file_is_charged_its_size() {
        let mut c = MediaCache::default().with_budget(2 * COST_FLOOR);
        c.insert("big", vec![0; 3 * COST_FLOOR]);
        assert_eq!(
            c.len(),
            1,
            "the newest stays even when it alone is over budget"
        );
        c.insert("small", pic(1));
        assert!(
            !c.contains("big"),
            "and goes as soon as something newer arrives"
        );
    }

    #[test]
    fn reading_a_picked_file_checks_type_and_size() {
        let dir = std::env::temp_dir().join(format!("thehideout-media-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("me.png");
        std::fs::write(&ok, pic(3)).unwrap();
        assert_eq!(read_image(&ok).unwrap(), (pic(3), "image/png".to_owned()));
        let text = dir.join("notes.txt");
        std::fs::write(&text, b"hi").unwrap();
        assert!(matches!(
            read_image(&text),
            Err(ApiError::InvalidArgument(_))
        ));
        let big = dir.join("huge.png");
        std::fs::write(&big, vec![0u8; MAX_MEDIA_BYTES + 1]).unwrap();
        assert!(
            matches!(read_image(&big), Err(ApiError::InvalidArgument(m)) if m.contains("8 MiB"))
        );
        let exact = dir.join("limit.gif");
        std::fs::write(&exact, vec![0u8; MAX_MEDIA_BYTES]).unwrap();
        assert!(read_image(&exact).is_ok(), "8 MiB itself is allowed");
        assert!(read_image(&dir.join("missing.png")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
