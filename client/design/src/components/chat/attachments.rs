//! Attachments under a message and in the composer's tray: an image grid, a video card with its
//! controls, an audio row, a file card and the "removed by an admin" placeholder, plus the
//! tray's chips and the lightbox.
//!
//! Data in, handlers out: every pressable part takes a [`Wire`] (a function that attaches the
//! app's handler to an element), so nothing here knows the app's state. Cards are filled, not
//! outlined (the README's "borders separate structure, not content"). Nothing moves on the first
//! frame; the only motion is the lightbox's fade (`motion::OVERLAY`), a progress bar's width and
//! the controls' fill (`motion::CONTROL`).

use std::rc::Rc;

use rune::{col, el, icon, row, text, El, Icon, Pixels, Player, SurfaceFit};
use rune_kit::Kit;

use crate::components::{Button, IconButton, Size, Variant};
use crate::icons::{
    ARROW_SQUARE_OUT, CHEVRON_LEFT, CHEVRON_RIGHT, DOWNLOAD, EXPAND, FILE, FILE_AUDIO, FILE_CODE,
    FILE_DOC, FILE_IMAGE, FILE_PDF, FILE_TEXT, FILE_VIDEO, FILE_ZIP, PAUSE, PLAY, RETRY, SPEAKER,
    SPEAKER_SLASH, WARNING, X,
};
use crate::theme_of;
use crate::tokens::color::Tone;
use crate::tokens::{icon as isize, motion, radius, type_};

/// Attaches the app's handler to an element (a press, a drag).
pub type Wire = Rc<dyn Fn(El) -> El>;

/// A wire that adds nothing: for previews and tests.
pub fn inert() -> Wire {
    Rc::new(|e| e)
}

/// Widest and tallest an inline picture or video grows.
pub const MEDIA_MAX_W: f32 = 400.0;
pub const MEDIA_MAX_H: f32 = 300.0;
/// The gap between grid tiles.
pub const GRID_GAP: f32 = 4.0;
/// Height of a two-column grid.
pub const GRID_H: f32 = 240.0;
/// The widest a file card or audio row grows.
pub const CARD_W: f32 = 340.0;

/// Ink over pictures and video (the same in both appearances, like the avatar's).
pub const MEDIA_INK: Tone = Tone::new(0.985, 0.006, 75.0);
/// The dim the controls sit on, over a picture.
const MEDIA_DIM: Tone = Tone::new(0.12, 0.01, 55.0).alpha(0.66);
/// What shows behind a picture that has not decoded and a video that has not started.
pub const MEDIA_BACK: Tone = Tone::new(0.2, 0.01, 55.0);

// ---- what a file is --------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Image,
    Video,
    Audio,
    Pdf,
    Archive,
    Text,
    Code,
    Doc,
    Other,
}

/// The kind of a file by its content type, then its extension.
pub fn file_kind(content_type: &str, filename: &str) -> FileKind {
    let ct = content_type.to_ascii_lowercase();
    let ext = filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ct.starts_with("image/") {
        FileKind::Image
    } else if ct.starts_with("video/") || matches!(ext.as_str(), "mp4" | "mov" | "webm" | "m4v") {
        FileKind::Video
    } else if ct.starts_with("audio/")
        || matches!(ext.as_str(), "mp3" | "m4a" | "wav" | "flac" | "ogg" | "aac")
    {
        FileKind::Audio
    } else if ct == "application/pdf" || ext == "pdf" {
        FileKind::Pdf
    } else if ct.contains("zip")
        || ct.contains("tar")
        || ct.contains("compressed")
        || ct.contains("7z")
        || matches!(
            ext.as_str(),
            "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "bz2" | "xz"
        )
    {
        FileKind::Archive
    } else if ct.contains("json")
        || ct.contains("xml")
        || ct.contains("javascript")
        || matches!(
            ext.as_str(),
            "rs" | "js"
                | "ts"
                | "py"
                | "json"
                | "toml"
                | "yaml"
                | "yml"
                | "html"
                | "css"
                | "sh"
                | "c"
                | "cpp"
                | "go"
        )
    {
        FileKind::Code
    } else if ct.starts_with("text/") || matches!(ext.as_str(), "txt" | "md" | "csv" | "log") {
        FileKind::Text
    } else if ct.contains("word")
        || ct.contains("officedocument")
        || ct.contains("opendocument")
        || matches!(
            ext.as_str(),
            "doc" | "docx" | "odt" | "xls" | "xlsx" | "ppt" | "pptx"
        )
    {
        FileKind::Doc
    } else {
        FileKind::Other
    }
}

impl FileKind {
    pub fn icon(self) -> &'static Icon {
        match self {
            FileKind::Image => &FILE_IMAGE,
            FileKind::Video => &FILE_VIDEO,
            FileKind::Audio => &FILE_AUDIO,
            FileKind::Pdf => &FILE_PDF,
            FileKind::Archive => &FILE_ZIP,
            FileKind::Text => &FILE_TEXT,
            FileKind::Code => &FILE_CODE,
            FileKind::Doc => &FILE_DOC,
            FileKind::Other => &FILE,
        }
    }

    /// Whether the server serves it inline, so "Open" in the browser shows it rather than saving it.
    pub fn opens_inline(self) -> bool {
        matches!(
            self,
            FileKind::Image | FileKind::Video | FileKind::Audio | FileKind::Pdf
        )
    }
}

/// "12 B", "3.4 KB", "5.0 MB", "1.2 GB" (powers of 1024, one decimal from KB up).
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    format!("{v:.1} {}", UNITS[unit])
}

/// "0:07", "12:03", "1:02:03".
pub fn clock(secs: f64) -> String {
    let s = if secs.is_finite() {
        secs.max(0.0) as u64
    } else {
        0
    };
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `(w, h)` scaled down (never up past `natural`) to fit `max_w` x `max_h`, keeping its aspect.
pub fn fit_size(natural: Option<(u32, u32)>, max_w: f32, max_h: f32) -> (f32, f32) {
    let (nw, nh) = match natural {
        Some((w, h)) if w > 0 && h > 0 => (w as f32, h as f32),
        _ => (max_w, max_w * 9.0 / 16.0),
    };
    let scale = (max_w / nw).min(max_h / nh).min(1.0);
    // A thumbnail is never a speck: a tiny picture still gets a legible box.
    let scale = if nw * scale < 96.0 && nh * scale < 96.0 {
        (96.0 / nw).max(96.0 / nh).min(max_w / nw).min(max_h / nh)
    } else {
        scale
    };
    ((nw * scale).round(), (nh * scale).round())
}

// ---- image grid ------------------------------------------------------------------------------

/// A tile's place in a grid, in points from its top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where `n` pictures sit: one alone keeps its aspect (within [`MEDIA_MAX_W`] x [`MEDIA_MAX_H`]);
/// two sit side by side; three put the first tall on the left; four or more are a 2x2 (a fifth and
/// beyond are reached through the lightbox, and the fourth tile says how many more there are).
/// Returns the grid's size and its cells.
pub fn image_grid(n: usize, single: Option<(u32, u32)>) -> ((f32, f32), Vec<Cell>) {
    let cell = |x, y, w, h| Cell { x, y, w, h };
    match n {
        0 => ((0.0, 0.0), Vec::new()),
        1 => {
            let (w, h) = fit_size(single, MEDIA_MAX_W, MEDIA_MAX_H);
            ((w, h), vec![cell(0.0, 0.0, w, h)])
        }
        _ => {
            let w = MEDIA_MAX_W;
            let half = (w - GRID_GAP) / 2.0;
            let cells = match n {
                2 => vec![
                    cell(0.0, 0.0, half, GRID_H),
                    cell(half + GRID_GAP, 0.0, half, GRID_H),
                ],
                3 => {
                    let small = (GRID_H - GRID_GAP) / 2.0;
                    vec![
                        cell(0.0, 0.0, half, GRID_H),
                        cell(half + GRID_GAP, 0.0, half, small),
                        cell(half + GRID_GAP, small + GRID_GAP, half, small),
                    ]
                }
                _ => {
                    let small = (GRID_H - GRID_GAP) / 2.0;
                    let right = half + GRID_GAP;
                    let low = small + GRID_GAP;
                    vec![
                        cell(0.0, 0.0, half, small),
                        cell(right, 0.0, half, small),
                        cell(0.0, low, half, small),
                        cell(right, low, half, small),
                    ]
                }
            };
            ((w, GRID_H), cells)
        }
    }
}

/// One picture of a grid.
pub struct GridImage {
    /// A stable key (the attachment id or the embed's media index).
    pub key: String,
    pub pixels: Option<Pixels>,
    pub label: String,
    pub open: Wire,
}

/// The pictures of one message or embed, as a grid; `total` is how many there are in all (the
/// tiles drawn are at most four).
pub fn image_grid_el(
    k: &Kit,
    key: &str,
    images: &[GridImage],
    single: Option<(u32, u32)>,
    total: usize,
) -> El {
    let t = theme_of(k);
    let shown = images.len().min(4);
    let ((w, h), cells) = image_grid(shown, single);
    let mut grid = el()
        .key(key.to_owned())
        .size(w, h)
        .role("group")
        .label("Pictures");
    for (i, (img, c)) in images.iter().zip(&cells).enumerate() {
        let mut tile = el()
            .key(format!("img-{}", img.key))
            .absolute(c.x, c.y)
            .size(c.w, c.h)
            .radius(radius::LG)
            .fill(MEDIA_BACK.color())
            .role("button")
            .label(format!("View {}", img.label))
            .focusable(true)
            .focus_ring(2.0, 1.0, k.c.border_focus)
            .hover_group();
        if let Some(p) = &img.pixels {
            tile = tile.pixels_fit(p, SurfaceFit::Fill);
        }
        tile = tile.child(
            el().absolute(0.0, 0.0)
                .size(c.w, c.h)
                .radius(radius::LG)
                .fill(t.hover.color())
                .group_opacity(0.0, 1.0)
                .a11y_hidden(),
        );
        if i + 1 == shown && total > shown {
            tile = tile.child(
                col()
                    .absolute(0.0, 0.0)
                    .size(c.w, c.h)
                    .center()
                    .radius(radius::LG)
                    .fill(MEDIA_DIM.color())
                    .child(
                        text(format!("+{}", total - shown + 1))
                            .font(type_::at(k, type_::HEADING))
                            .color(MEDIA_INK.color()),
                    ),
            );
        }
        grid = grid.child((img.open)(tile));
    }
    grid
}

// ---- video -----------------------------------------------------------------------------------

/// What a video card shows of its player.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VideoUi {
    pub playing: bool,
    pub time: f64,
    pub duration: f64,
    pub muted: bool,
    /// The player is opening the clip.
    pub loading: bool,
    /// The platform can't play it (or the file is bad): the card offers the browser.
    pub failed: bool,
}

pub struct VideoCard {
    key: String,
    size: (f32, f32),
    poster: Option<Pixels>,
    player: Option<Player>,
    ui: VideoUi,
    controls: bool,
    play: Wire,
    toggle: Wire,
    mute: Wire,
    seek: Wire,
    expand: Option<Wire>,
    browser: Wire,
    /// Shown instead of play when the server kept no file for it (too large): poster and a link.
    poster_only: bool,
    badge: Option<String>,
    started: bool,
}

impl VideoCard {
    /// `size` is the box in points (see [`fit_size`]).
    pub fn new(key: impl Into<String>, size: (f32, f32)) -> VideoCard {
        VideoCard {
            key: key.into(),
            size,
            poster: None,
            player: None,
            ui: VideoUi::default(),
            controls: true,
            play: inert(),
            toggle: inert(),
            mute: inert(),
            seek: inert(),
            expand: None,
            browser: inert(),
            poster_only: false,
            badge: None,
            started: false,
        }
    }
    pub fn poster(mut self, p: Option<Pixels>) -> Self {
        self.poster = p;
        self
    }
    /// The player, once the video has been started; until then the poster shows.
    pub fn player(mut self, p: Option<Player>, ui: VideoUi) -> Self {
        self.player = p;
        self.ui = ui;
        self
    }
    /// Draws the controls as if a player were running (scenes and previews that have none).
    pub fn started(mut self, on: bool) -> Self {
        self.started = on;
        self
    }
    /// No bar (a looping clip that plays by itself).
    pub fn controls(mut self, on: bool) -> Self {
        self.controls = on;
        self
    }
    pub fn on_play(mut self, w: Wire) -> Self {
        self.play = w;
        self
    }
    /// A press on the picture itself while a player exists: pauses or resumes.
    pub fn on_toggle(mut self, w: Wire) -> Self {
        self.toggle = w;
        self
    }
    pub fn on_mute(mut self, w: Wire) -> Self {
        self.mute = w;
        self
    }
    /// Attaches the drag that scrubs to the bar.
    pub fn on_seek(mut self, w: Wire) -> Self {
        self.seek = w;
        self
    }
    pub fn on_expand(mut self, w: Wire) -> Self {
        self.expand = Some(w);
        self
    }
    /// "Open in browser" (the platform can't play it) or "Open original" (nothing stored).
    pub fn on_browser(mut self, w: Wire) -> Self {
        self.browser = w;
        self
    }
    pub fn poster_only(mut self, on: bool) -> Self {
        self.poster_only = on;
        self
    }
    /// A small label in the corner ("GIF", "0:14").
    pub fn badge(mut self, label: impl Into<String>) -> Self {
        self.badge = Some(label.into());
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let (w, h) = self.size;
        let ink = MEDIA_INK.color();
        let mut card = el()
            .key(self.key.clone())
            .size(w, h)
            .radius(radius::LG)
            .fill(MEDIA_BACK.color())
            .role("group")
            .label("Video");
        if let Some(p) = &self.poster {
            card = card.child(
                el().key("poster")
                    .absolute(0.0, 0.0)
                    .size(w, h)
                    .radius(radius::LG)
                    .pixels_fit(p, SurfaceFit::Fit)
                    .a11y_hidden(),
            );
        }
        let started = (self.player.is_some() || self.started) && !self.ui.failed;
        if let (Some(p), true) = (&self.player, started) {
            // Frames paint over the poster once the first one is decoded.
            let surface = (self.toggle)(
                el().key("surface")
                    .absolute(0.0, 0.0)
                    .size(w, h)
                    .radius(radius::LG)
                    .video(p, SurfaceFit::Fit)
                    .role("button")
                    .label(if self.ui.playing { "Pause" } else { "Play" }),
            );
            card = card.child(surface);
        }
        if self.ui.failed || self.poster_only {
            let label = if self.poster_only {
                "Open original"
            } else {
                "Open in browser"
            };
            let why = if self.poster_only {
                "Too large to keep here"
            } else {
                "This video can't play in the app"
            };
            card = card.child(
                col()
                    .key("fallback")
                    .absolute(0.0, 0.0)
                    .size(w, h)
                    .radius(radius::LG)
                    .fill(MEDIA_DIM.alpha(0.45).color())
                    .center()
                    .gap(8.0)
                    .child(text(why).font(type_::at(k, type_::CONTROL)).color(ink))
                    .child((self.browser)(
                        Button::new(label)
                            .icon(&ARROW_SQUARE_OUT)
                            .variant(Variant::Secondary)
                            .size(Size::Sm)
                            .build(k)
                            .key("browser"),
                    )),
            );
        } else if !started || self.ui.loading && !self.ui.playing && self.player.is_none() {
            let big = (self.play)(
                row()
                    .key("play")
                    .size(48.0, 48.0)
                    .center()
                    .radius(radius::FULL)
                    .fill(MEDIA_DIM.color())
                    .role("button")
                    .label("Play video")
                    .focusable(true)
                    .focus_ring(crate::space::RING_WIDTH, crate::space::RING_OFFSET, k.c.border_focus)
                    .hover(MEDIA_BACK.color())
                    .child(icon(&PLAY, isize::MD).color(ink).a11y_hidden()),
            );
            card = card.child(
                row()
                    .key("play-wrap")
                    .absolute(0.0, 0.0)
                    .size(w, h)
                    .center()
                    .child(big),
            );
        } else if self.ui.loading && !self.ui.playing {
            card = card.child(
                row()
                    .key("loading")
                    .absolute(0.0, 0.0)
                    .size(w, h)
                    .center()
                    .child(text("Loading\u{2026}").font(type_::at(k, type_::CONTROL)).color(ink)),
            );
        }
        if let Some(b) = &self.badge {
            card = card.child(
                row()
                    .key("badge")
                    .absolute(8.0, 8.0)
                    .px(6.0)
                    .h(18.0)
                    .center()
                    .radius(radius::SM)
                    .fill(MEDIA_DIM.color())
                    .child(text(b.clone()).font(type_::at(k, type_::CAPTION)).color(ink)),
            );
        }
        if self.controls && started && !self.ui.failed {
            card = card.child(self.bar(k));
        }
        card
    }

    fn bar(&self, k: &Kit) -> El {
        let ink = MEDIA_INK.color();
        let btn = |g: &'static Icon, label: &str, w: &Wire| {
            (w)(row()
                .size(28.0, 28.0)
                .center()
                .radius(radius::MD)
                .role("button")
                .label(label.to_owned())
                .focusable(true)
                .focus_ring(2.0, 0.0, k.c.border_focus)
                .hover(MEDIA_BACK.color())
                .child(icon(g, isize::XS).color(ink).a11y_hidden()))
        };
        let frac = if self.ui.duration > 0.0 {
            (self.ui.time / self.ui.duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let track = (self.seek)(
            row()
                .key("scrub")
                .role("slider")
                .label("Seek")
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .h(28.0)
                .center_y()
                .child(
                    row()
                        .w_full()
                        .h(4.0)
                        .radius(radius::FULL)
                        .fill(MEDIA_INK.alpha(0.3).color())
                        .child(
                            el().key("scrub-fill")
                                .w_frac(frac)
                                .h(4.0)
                                .radius(radius::FULL)
                                .fill(ink),
                        ),
                ),
        );
        let mut bar = row()
            .key("controls")
            .pin(Some(0.0), None, Some(0.0), Some(0.0))
            .h(36.0)
            .px(6.0)
            .gap(4.0)
            .center_y()
            .fill(MEDIA_DIM.color())
            .child(btn(
                if self.ui.playing { &PAUSE } else { &PLAY },
                if self.ui.playing { "Pause" } else { "Play" },
                &self.toggle,
            ))
            .child(track)
            .child(
                text(format!(
                    "{} / {}",
                    clock(self.ui.time),
                    clock(self.ui.duration)
                ))
                .key("time")
                .font(type_::at(k, type_::CAPTION))
                .color(ink),
            )
            .child(btn(
                if self.ui.muted {
                    &SPEAKER_SLASH
                } else {
                    &SPEAKER
                },
                if self.ui.muted { "Unmute" } else { "Mute" },
                &self.mute,
            ));
        if let Some(e) = &self.expand {
            bar = bar.child(btn(&EXPAND, "Expand", e));
        }
        bar
    }
}

// ---- audio -----------------------------------------------------------------------------------

/// A compact player row for an audio attachment.
pub struct AudioRow {
    pub key: String,
    pub name: String,
    pub size: u64,
    pub ui: VideoUi,
    pub toggle: Wire,
    pub seek: Wire,
    pub download: Wire,
}

impl AudioRow {
    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let frac = if self.ui.duration > 0.0 {
            (self.ui.time / self.ui.duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let play = (self.toggle)(
            IconButton::new(
                if self.ui.playing { &PAUSE } else { &PLAY },
                if self.ui.playing { "Pause" } else { "Play" },
            )
            .variant(Variant::Primary)
            .size(Size::Md)
            .build(k)
            .key("toggle"),
        );
        let track = (self.seek)(
            row()
                .key("scrub")
                .role("slider")
                .label("Seek")
                .w_full()
                .h(16.0)
                .center_y()
                .child(
                    row()
                        .w_full()
                        .h(4.0)
                        .radius(radius::FULL)
                        .fill(t.pressed.color())
                        .child(
                            el().key("scrub-fill")
                                .w_frac(frac)
                                .h(4.0)
                                .radius(radius::FULL)
                                .fill(t.accent.color()),
                        ),
                ),
        );
        let time = if self.ui.duration > 0.0 {
            format!("{} / {}", clock(self.ui.time), clock(self.ui.duration))
        } else {
            human_size(self.size)
        };
        row()
            .key(self.key)
            .w_full()
            .max_w(CARD_W)
            .px(8.0)
            .py(8.0)
            .gap(10.0)
            .center_y()
            .radius(radius::LG)
            .fill(t.hover.color())
            .child(play)
            .child(
                col()
                    .grow(1.0)
                    .shrink()
                    .min_w(0.0)
                    .gap(2.0)
                    .child(
                        text(self.name)
                            .font(type_::at(k, type_::CONTROL_STRONG))
                            .color(t.text.color())
                            .truncate()
                            .w_full(),
                    )
                    .child(track)
                    .child(
                        text(time)
                            .key("time")
                            .font(type_::at(k, type_::CAPTION))
                            .color(t.text_muted.color()),
                    ),
            )
            .child((self.download)(
                IconButton::new(&DOWNLOAD, "Download")
                    .size(Size::Sm)
                    .build(k)
                    .key("download"),
            ))
    }
}

// ---- file card -------------------------------------------------------------------------------

/// A downloadable file: its type icon, name and size, "Open" for what a browser shows, "Download".
pub struct FileCard {
    pub key: String,
    pub name: String,
    pub size: u64,
    pub kind: FileKind,
    /// Download progress (0 to 1) while one runs.
    pub progress: Option<f32>,
    pub download: Wire,
    pub open: Option<Wire>,
}

impl FileCard {
    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let art = row()
            .size(36.0, 36.0)
            .center()
            .radius(radius::MD)
            .fill(t.accent_subtle.color())
            .child(
                icon(self.kind.icon(), isize::MD)
                    .color(t.accent_text.color())
                    .a11y_hidden(),
            );
        let sub = match self.progress {
            Some(p) => format!("Downloading\u{2026} {}%", (p * 100.0).round() as u32),
            None => human_size(self.size),
        };
        let mut info = col()
            .grow(1.0)
            .shrink()
            .min_w(0.0)
            .gap(1.0)
            .child(
                text(self.name.clone())
                    .font(type_::at(k, type_::CONTROL_STRONG))
                    .color(t.text.color())
                    .truncate()
                    .w_full(),
            )
            .child(
                text(sub)
                    .key("size")
                    .font(type_::at(k, type_::CAPTION))
                    .color(t.text_muted.color()),
            );
        if let Some(p) = self.progress {
            info = info.child(
                row()
                    .key("progress")
                    .w_full()
                    .h(3.0)
                    .radius(radius::FULL)
                    .fill(t.pressed.color())
                    .child(
                        el().w_frac(p.clamp(0.0, 1.0))
                            .h(3.0)
                            .radius(radius::FULL)
                            .fill(t.accent.color())
                            .token_motion(motion::CONTROL),
                    ),
            );
        }
        let mut actions = row().gap(2.0).center_y();
        if let Some(o) = &self.open {
            actions = actions.child((o)(IconButton::new(&ARROW_SQUARE_OUT, "Open")
                .size(Size::Sm)
                .build(k)
                .key("open")));
        }
        actions = actions.child((self.download)(
            IconButton::new(&DOWNLOAD, format!("Download {}", self.name))
                .size(Size::Sm)
                .disabled(self.progress.is_some())
                .build(k)
                .key("download"),
        ));
        row()
            .key(self.key)
            .role("group")
            .label(format!("File {}", self.name))
            .w_full()
            .max_w(CARD_W)
            .padding(10.0)
            .gap(10.0)
            .center_y()
            .radius(radius::LG)
            .fill(t.hover.color())
            .child(art)
            .child(info)
            .child(actions)
    }
}

/// "File removed by an admin", where the file was.
pub fn removed(k: &Kit, key: &str) -> El {
    let t = theme_of(k);
    row()
        .key(key.to_owned())
        .role("group")
        .label("File removed by an admin")
        .w_full()
        .max_w(CARD_W)
        .px(10.0)
        .h(36.0)
        .gap(8.0)
        .center_y()
        .radius(radius::LG)
        .fill(t.hover.color())
        .child(
            icon(&FILE, isize::XS)
                .color(t.text_faint.color())
                .a11y_hidden(),
        )
        .child(
            text("File removed by an admin")
                .font(type_::at(k, type_::CONTROL))
                .color(t.text_muted.color()),
        )
}

// ---- composer tray ---------------------------------------------------------------------------

/// What a tray chip shows of its upload.
#[derive(Clone, Debug, PartialEq)]
pub enum ChipState {
    /// `sent` of `total` bytes are on their way.
    Uploading {
        sent: u64,
        total: u64,
    },
    Ready,
    Failed(String),
}

pub struct Chip {
    pub key: String,
    pub name: String,
    pub size: u64,
    pub kind: FileKind,
    pub thumb: Option<Pixels>,
    pub state: ChipState,
    pub remove: Wire,
    pub retry: Wire,
}

/// The chips above the composer's input.
pub fn tray(k: &Kit, chips: Vec<Chip>) -> El {
    let t = theme_of(k);
    let mut line = row()
        .key("tray")
        .w_full()
        .flex_wrap()
        .gap(8.0)
        .px(4.0)
        .pt(4.0)
        .pb(4.0);
    for c in chips {
        let art = match &c.thumb {
            Some(p) => el()
                .size(32.0, 32.0)
                .radius(radius::MD)
                .fill(MEDIA_BACK.color())
                .pixels_fit(p, SurfaceFit::Fill)
                .a11y_hidden(),
            None => row()
                .size(32.0, 32.0)
                .center()
                .radius(radius::MD)
                .fill(t.accent_subtle.color())
                .child(
                    icon(c.kind.icon(), isize::SM)
                        .color(t.accent_text.color())
                        .a11y_hidden(),
                ),
        };
        let failed = matches!(c.state, ChipState::Failed(_));
        let sub = match &c.state {
            ChipState::Uploading { sent, total } => {
                format!("{} / {}", human_size(*sent), human_size(*total))
            }
            ChipState::Ready => human_size(c.size),
            ChipState::Failed(why) => why.clone(),
        };
        let mut info = col()
            .grow(1.0)
            .shrink()
            .min_w(0.0)
            .gap(1.0)
            .child(
                text(c.name.clone())
                    .font(type_::at(k, type_::CONTROL_STRONG))
                    .color(t.text.color())
                    .truncate()
                    .w_full(),
            )
            .child(
                text(sub)
                    .key("sub")
                    .font(type_::at(k, type_::CAPTION))
                    .color(if failed {
                        t.danger.color()
                    } else {
                        t.text_muted.color()
                    })
                    .truncate()
                    .w_full(),
            );
        if let ChipState::Uploading { sent, total } = &c.state {
            let frac = if *total == 0 {
                0.0
            } else {
                (*sent as f32 / *total as f32).clamp(0.0, 1.0)
            };
            info = info.child(
                row()
                    .key("progress")
                    .w_full()
                    .h(3.0)
                    .radius(radius::FULL)
                    .fill(t.pressed.color())
                    .child(
                        el().key("bar")
                            .w_frac(frac)
                            .h(3.0)
                            .radius(radius::FULL)
                            .fill(t.accent.color())
                            .token_motion(motion::CONTROL),
                    ),
            );
        }
        let mut chip = row()
            .key(format!("chip-{}", c.key))
            .role("group")
            .label(format!("Attachment {}", c.name))
            .w(220.0)
            .padding(6.0)
            .gap(8.0)
            .center_y()
            .radius(radius::LG)
            .fill(if failed {
                t.danger_bg.color()
            } else {
                t.layers[1].color()
            })
            .child(art)
            .child(info);
        if failed {
            chip = chip.child((c.retry)(
                IconButton::new(&RETRY, "Try again")
                    .size(Size::Sm)
                    .build(k)
                    .key("retry"),
            ));
        }
        chip = chip.child((c.remove)(
            IconButton::new(&X, format!("Remove {}", c.name))
                .size(Size::Sm)
                .build(k)
                .key("remove"),
        ));
        line = line.child(chip);
    }
    line
}

/// The note under the tray: too many files, too large.
pub fn tray_note(k: &Kit, message: &str) -> El {
    let t = theme_of(k);
    row()
        .key("tray-note")
        .w_full()
        .px(8.0)
        .py(2.0)
        .gap(6.0)
        .center_y()
        .child(
            icon(&WARNING, isize::XS)
                .color(t.danger.color())
                .a11y_hidden(),
        )
        .child(
            text(message.to_owned())
                .font(type_::at(k, type_::CAPTION))
                .color(t.danger.color())
                .wrap(),
        )
}

// ---- lightbox --------------------------------------------------------------------------------

/// What the lightbox shows now.
pub struct LightboxView {
    pub name: String,
    pub pixels: Option<Pixels>,
    pub video: Option<El>,
    /// 1-based position and count when there is more than one.
    pub position: Option<(usize, usize)>,
    pub size: (f32, f32),
    pub on_close: Wire,
    pub on_prev: Option<Wire>,
    pub on_next: Option<Wire>,
    pub on_download: Wire,
}

/// Pushes the scrim and the picture onto `layer`. `close` is the modal's close handler (Escape,
/// a press outside the picture); it is attached with `modal`.
pub fn lightbox(k: &Kit, layer: &mut Vec<El>, v: LightboxView, close: impl FnOnce(El) -> El) {
    let t = theme_of(k);
    let fade = rune::Transition::opacity()
        .springs(rune::SpringPair::new(motion::OVERLAY, motion::OVERLAY));
    layer.push(
        el().key("lightbox-scrim")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(MEDIA_DIM.alpha(0.9).color())
            .transition(fade)
            .a11y_hidden(),
    );
    let ink = MEDIA_INK.color();
    let (w, h) = v.size;
    let mut stage = el()
        .key("lightbox-media")
        .size(w, h)
        .radius(radius::LG)
        .fill(MEDIA_BACK.color());
    if let Some(p) = &v.pixels {
        stage = stage.pixels_fit(p, SurfaceFit::Fit);
    }
    if let Some(video) = v.video {
        stage = stage.child(video);
    }
    let label = match v.position {
        Some((i, n)) => format!("{} \u{b7} {i} of {n}", v.name),
        None => v.name.clone(),
    };
    let bar = row()
        .key("lightbox-bar")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            text(label)
                .font(type_::at(k, type_::CONTROL))
                .color(ink)
                .truncate()
                .grow(1.0)
                .shrink()
                .min_w(0.0),
        )
        .child((v.on_download)(
            IconButton::new(&DOWNLOAD, "Download")
                .build(k)
                .key("lightbox-download"),
        ))
        .child((v.on_close)(
            IconButton::new(&X, "Close").build(k).key("lightbox-close"),
        ));
    let arrow = |g: &'static Icon, label: &str, key: &str, w: &Option<Wire>| -> El {
        match w {
            Some(w) => w(row()
                .key(key.to_owned())
                .size(40.0, 40.0)
                .center()
                .radius(radius::FULL)
                .fill(MEDIA_DIM.color())
                .role("button")
                .label(label.to_owned())
                .focusable(true)
                .focus_ring(crate::space::RING_WIDTH, crate::space::RING_OFFSET, k.c.border_focus)
                .hover(t.hover.color())
                .child(icon(g, isize::MD).color(ink).a11y_hidden())),
            None => el().size(40.0, 40.0),
        }
    };
    let body = row()
        .key("lightbox-body")
        .gap(12.0)
        .center_y()
        .child(arrow(
            &CHEVRON_LEFT,
            "Previous",
            "lightbox-prev",
            &v.on_prev,
        ))
        .child(stage)
        .child(arrow(&CHEVRON_RIGHT, "Next", "lightbox-next", &v.on_next));
    // The modal is the picture and its bar, not the whole layer, so a press anywhere around them
    // (the layer itself) is "outside" and closes it.
    let content = close(
        col()
            .key("lightbox-content")
            .role("dialog")
            .label(format!("Viewing {}", v.name))
            .gap(12.0)
            .center()
            .child(body)
            .child(row().w(w + 104.0).child(bar)),
    )
    .modal_click_outside(true);
    layer.push(
        el().key("lightbox-layer")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(24.0)
            .center()
            .transition(
                rune::Transition::opacity()
                    .springs(rune::SpringPair::new(motion::OVERLAY, motion::OVERLAY)),
            )
            .child(content),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_in_binary_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(human_size(50 * 1024 * 1024 * 1024), "50.0 GB");
    }

    #[test]
    fn clocks() {
        assert_eq!(clock(7.4), "0:07");
        assert_eq!(clock(723.0), "12:03");
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(clock(f64::NAN), "0:00");
    }

    #[test]
    fn kinds_come_from_the_type_then_the_name() {
        assert_eq!(file_kind("image/png", "a"), FileKind::Image);
        assert_eq!(
            file_kind("application/octet-stream", "clip.MP4"),
            FileKind::Video
        );
        assert_eq!(file_kind("application/pdf", "x"), FileKind::Pdf);
        assert_eq!(file_kind("application/zip", "x"), FileKind::Archive);
        assert_eq!(file_kind("", "notes.txt"), FileKind::Text);
        assert_eq!(file_kind("", "blob"), FileKind::Other);
        assert!(FileKind::Pdf.opens_inline() && !FileKind::Archive.opens_inline());
    }

    #[test]
    fn a_single_picture_keeps_its_aspect_within_the_limits() {
        let ((w, h), cells) = image_grid(1, Some((1600, 900)));
        assert_eq!((w, h), (400.0, 225.0));
        assert_eq!(cells.len(), 1);
        let ((w, h), _) = image_grid(1, Some((600, 1200)));
        assert_eq!((w, h), (150.0, 300.0), "tall pictures are capped by height");
        let ((w, h), _) = image_grid(1, Some((200, 100)));
        assert_eq!((w, h), (200.0, 100.0), "small pictures are not blown up");
        let ((w, h), _) = image_grid(1, None);
        assert_eq!((w, h), (400.0, 225.0), "unknown sizes get 16:9");
    }

    #[test]
    fn grids_follow_the_two_up_three_and_two_by_two_rules() {
        let ((w, h), two) = image_grid(2, None);
        assert_eq!((w, h, two.len()), (400.0, GRID_H, 2));
        assert_eq!(two[0].h, GRID_H);
        assert_eq!(two[1].x, two[0].w + GRID_GAP);
        let (_, three) = image_grid(3, None);
        assert_eq!(three[0].h, GRID_H, "the first is tall");
        assert_eq!(
            three[1].h + GRID_GAP + three[2].h,
            GRID_H,
            "two stack at the right"
        );
        for n in [4, 7] {
            let (_, cells) = image_grid(n, None);
            assert_eq!(cells.len(), 4, "never more than four tiles");
            assert_eq!(cells[0].w, cells[3].w);
            assert_eq!(cells[2].y, cells[0].h + GRID_GAP);
        }
        assert!(image_grid(0, None).1.is_empty());
    }
}
