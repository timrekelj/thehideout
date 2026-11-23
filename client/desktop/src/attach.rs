//! Attachments, embeds, the lightbox, video players and downloads: their state and transitions.
//! Plain data and pure steps on `AppState` (the handlers in `ui::attach` call them): uploads
//! start the moment a file is chosen and run off the UI thread, a message sends the ones that
//! finished, downloads stream to a path the user chose, and a player exists only once a video has
//! been started (until then its poster is a picture and costs nothing).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use rune::{Player, PlayerState, Waker};
use thehideout_api::{ApiError, DEFAULT_MAX_UPLOAD_BYTES, EmbedMediaKind, Message, PendingAttachment};
use thehideout_design::components::chat::attachments::{VideoUi, file_kind, human_size};
use thehideout_design::components::{FileKind, ToastKind};

use crate::backend::Command;
use crate::state::AppState;

/// Files one message may carry.
pub const MAX_FILES: usize = 10;
/// Collapsed embeds the prefs remember (oldest forgotten first).
pub const EMBED_PREFS_CAP: usize = 400;
/// Players kept alive at once (a paused one beyond this is dropped).
const PLAYER_CAP: usize = 6;
/// How many of the newest messages of a channel have their pictures fetched.
pub const MEDIA_WINDOW: usize = 100;
/// A picture without a thumbnail is fetched whole only up to this size.
pub const INLINE_MAX_BYTES: u64 = 4 * 1024 * 1024;

// ---- the composer's tray -----------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Path(PathBuf),
    /// A pasted picture.
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadState {
    Uploading { sent: u64 },
    /// Uploaded; `pending` is what `SendWith` links.
    Ready { pending: u64 },
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftFile {
    pub id: u64,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub origin: Origin,
    pub state: UploadState,
    /// The media-cache key of its thumbnail (a picture).
    pub thumb: Option<String>,
}

/// One channel's files waiting to be sent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    pub files: Vec<DraftFile>,
    /// What the last refused file said ("up to 10 files", "over 50 MiB").
    pub note: Option<String>,
}

impl Draft {
    pub fn uploading(&self) -> bool {
        self.files.iter().any(|f| matches!(f.state, UploadState::Uploading { .. }))
    }
    pub fn ready(&self) -> usize {
        self.files.iter().filter(|f| matches!(f.state, UploadState::Ready { .. })).count()
    }
}

/// A file of a message that is on its way (kept for display until the server confirms it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutFile {
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub thumb: Option<String>,
}

// ---- the lightbox ----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LbItem {
    pub name: String,
    /// The full file's key (a video's is streamed, not fetched).
    pub key: String,
    /// A smaller picture to show until the full one arrives.
    pub thumb: Option<String>,
    pub poster: Option<String>,
    pub size: (u32, u32),
    pub video: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lightbox {
    pub items: Vec<LbItem>,
    pub at: usize,
}

impl Lightbox {
    pub fn current(&self) -> &LbItem {
        &self.items[self.at]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Download {
    pub received: u64,
    pub total: Option<u64>,
}

impl Download {
    pub fn fraction(&self) -> f32 {
        match self.total {
            Some(t) if t > 0 => (self.received as f32 / t as f32).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

// ---- players ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Video,
    /// A looping, silent clip that plays by itself while it is on screen.
    Gifv,
    Audio,
}

pub struct Clip {
    pub player: Player,
    pub kind: ClipKind,
}

impl Clip {
    pub fn gifv(&self) -> bool {
        self.kind == ClipKind::Gifv
    }
}

/// Everything this module adds to the UI state.
pub struct AttachUi {
    pub drafts: HashMap<u64, Draft>,
    upload_seq: u64,
    pub max_upload: u64,
    /// Files are being dragged over the window.
    pub drop_over: bool,
    pub lightbox: Option<Lightbox>,
    pub downloads: HashMap<String, Download>,
    pub clips: HashMap<String, Clip>,
    clip_order: Vec<String>,
    /// `(message id, embed index)` of the folded embeds, oldest first; saved to the prefs.
    pub embeds_collapsed: VecDeque<(u64, u32)>,
    pub embeds_dirty: bool,
    pub waker: Option<Waker>,
    /// A clip's controls pinned to a state, with no player behind them (scenes).
    pub pinned: HashMap<String, VideoUi>,
    /// Pretends the platform can't play video (tests; Linux is that for real).
    pub video_unsupported: bool,
}

impl Default for AttachUi {
    fn default() -> Self {
        AttachUi {
            drafts: HashMap::new(),
            upload_seq: 0,
            max_upload: DEFAULT_MAX_UPLOAD_BYTES,
            drop_over: false,
            lightbox: None,
            downloads: HashMap::new(),
            clips: HashMap::new(),
            clip_order: Vec::new(),
            embeds_collapsed: VecDeque::new(),
            embeds_dirty: false,
            waker: None,
            pinned: HashMap::new(),
            video_unsupported: !cfg!(any(target_os = "macos", target_os = "windows")),
        }
    }
}

/// The content type a file of this name is uploaded as.
pub fn content_type_of(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "json" => "application/json",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        _ => "application/octet-stream",
    }
}

/// What a refused upload says.
pub fn upload_error(e: &ApiError) -> String {
    match e {
        ApiError::TooLarge => "Too large".into(),
        ApiError::Network(_) => "Upload failed".into(),
        ApiError::PermissionDenied => "You can't attach files here".into(),
        ApiError::RateLimited => "Slow down, try again".into(),
        other => crate::state::sentence(&other.to_string()).trim_end_matches('.').to_owned(),
    }
}


/// The size of the file at `path`; `Ok(None)` for a folder. On the web a path is a picked `File`.
#[cfg(not(target_arch = "wasm32"))]
fn file_size(path: &Path) -> std::io::Result<Option<u64>> {
    let m = std::fs::metadata(path)?;
    Ok(m.is_file().then(|| m.len()))
}

#[cfg(target_arch = "wasm32")]
fn file_size(path: &Path) -> std::io::Result<Option<u64>> {
    crate::web::file_size(path).map(Some).ok_or_else(|| std::io::ErrorKind::NotFound.into())
}

impl AppState {
    // ---- uploads -----------------------------------------------------------------------------

    pub fn draft(&self, channel: u64) -> Option<&Draft> {
        self.ui.attach.drafts.get(&channel)
    }

    fn set_note(&mut self, channel: u64, note: String) {
        self.ui.attach.drafts.entry(channel).or_default().note = Some(note);
    }

    /// Checks `size` and the count against the limits; the note says which was broken.
    fn admit(&mut self, channel: u64, name: &str, size: u64) -> bool {
        let n = self.draft(channel).map_or(0, |d| d.files.len());
        if n >= MAX_FILES {
            self.set_note(channel, format!("You can attach up to {MAX_FILES} files to a message"));
            return false;
        }
        if size > self.ui.attach.max_upload {
            let limit = human_size(self.ui.attach.max_upload);
            self.set_note(channel, format!("{name} is {}, over the {limit} limit", human_size(size)));
            return false;
        }
        true
    }

    /// Chosen or dropped files start uploading at once (streamed from disk off the UI thread).
    pub fn attach_paths(&mut self, paths: &[PathBuf]) {
        for p in paths {
            self.attach_path(p);
        }
    }

    fn attach_path(&mut self, path: &Path) {
        let Some(channel) = self.current_channel() else { return };
        let name = path.file_name().map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());
        // One `stat`: the size is needed to refuse a file that is too large before it is read.
        let size = match file_size(path) {
            Ok(Some(len)) => len,
            Ok(None) => return self.set_note(channel, format!("{name} is a folder; attach its files instead")),
            Err(_) => return self.set_note(channel, format!("Couldn't read {name}")),
        };
        if !self.admit(channel, &name, size) {
            return;
        }
        let content_type = content_type_of(&name).to_owned();
        let id = self.next_upload();
        let draft = self.ui.attach.drafts.entry(channel).or_default();
        draft.note = None;
        draft.files.push(DraftFile {
            id,
            name: name.clone(),
            content_type: content_type.clone(),
            size,
            origin: Origin::Path(path.to_path_buf()),
            state: UploadState::Uploading { sent: 0 },
            thumb: None,
        });
        self.outbox.push(Command::UploadFile { channel, id, path: path.to_path_buf(), filename: name, content_type, size });
    }

    /// A picture pasted from the clipboard (PNG bytes).
    pub fn attach_pasted(&mut self, png: Vec<u8>) {
        let Some(channel) = self.current_channel() else { return };
        let id_hint = self.ui.attach.upload_seq + 1;
        let name = format!("pasted-image-{id_hint}.png");
        if !self.admit(channel, &name, png.len() as u64) {
            return;
        }
        let id = self.next_upload();
        let thumb = format!("local-{id}");
        self.data.media.insert(&thumb, png.clone());
        let size = png.len() as u64;
        let draft = self.ui.attach.drafts.entry(channel).or_default();
        draft.note = None;
        draft.files.push(DraftFile {
            id,
            name: name.clone(),
            content_type: "image/png".into(),
            size,
            origin: Origin::Bytes(png.clone()),
            state: UploadState::Uploading { sent: 0 },
            thumb: Some(thumb),
        });
        self.outbox.push(Command::UploadBytes { channel, id, filename: name, content_type: "image/png".into(), bytes: png });
    }

    fn next_upload(&mut self) -> u64 {
        self.ui.attach.upload_seq += 1;
        self.ui.attach.upload_seq
    }

    fn file_mut(&mut self, id: u64) -> Option<&mut DraftFile> {
        self.ui.attach.drafts.values_mut().flat_map(|d| d.files.iter_mut()).find(|f| f.id == id)
    }

    /// The tray's ✕: forgets the file, and stops it if it is still going up.
    pub fn remove_attachment(&mut self, id: u64) {
        for d in self.ui.attach.drafts.values_mut() {
            if let Some(at) = d.files.iter().position(|f| f.id == id) {
                let f = d.files.remove(at);
                d.note = None;
                if matches!(f.state, UploadState::Uploading { .. }) {
                    self.outbox.push(Command::CancelUpload { id });
                }
                return;
            }
        }
    }

    /// "Try again" on a file that failed.
    pub fn retry_attachment(&mut self, id: u64) {
        let Some(channel) = self.ui.attach.drafts.iter().find(|(_, d)| d.files.iter().any(|f| f.id == id)).map(|(c, _)| *c) else { return };
        let Some(f) = self.file_mut(id).filter(|f| matches!(f.state, UploadState::Failed(_))) else { return };
        f.state = UploadState::Uploading { sent: 0 };
        let (name, content_type, size, origin) = (f.name.clone(), f.content_type.clone(), f.size, f.origin.clone());
        self.outbox.push(match origin {
            Origin::Path(path) => Command::UploadFile { channel, id, path, filename: name, content_type, size },
            Origin::Bytes(bytes) => Command::UploadBytes { channel, id, filename: name, content_type, bytes },
        });
    }

    pub fn upload_progress(&mut self, id: u64, sent: u64, total: u64) {
        if let Some(f) = self.file_mut(id)
            && let UploadState::Uploading { sent: s } = &mut f.state
        {
            f.size = f.size.max(total);
            *s = sent.min(total.max(sent));
        }
    }

    pub fn upload_preview(&mut self, id: u64, bytes: Vec<u8>) {
        let key = format!("local-{id}");
        if self.file_mut(id).is_some() {
            self.data.media.insert(&key, bytes);
            if let Some(f) = self.file_mut(id) {
                f.thumb = Some(key);
            }
        }
    }

    pub fn uploaded(&mut self, id: u64, result: Result<PendingAttachment, ApiError>) {
        // A file removed meanwhile answers nothing.
        let Some(f) = self.file_mut(id) else { return };
        f.state = match result {
            Ok(p) => {
                f.size = p.size;
                UploadState::Ready { pending: p.id }
            }
            Err(e) => UploadState::Failed(upload_error(&e)),
        };
    }

    // ---- downloads ---------------------------------------------------------------------------

    /// Streams `key` to `path` (the user chose it in the save dialog).
    pub fn start_download(&mut self, key: &str, path: PathBuf) {
        if self.ui.attach.downloads.contains_key(key) {
            return;
        }
        self.ui.attach.downloads.insert(key.to_owned(), Download::default());
        self.outbox.push(Command::Download { key: key.to_owned(), path });
    }

    pub fn download_progress(&mut self, key: &str, received: u64, total: Option<u64>) {
        if let Some(d) = self.ui.attach.downloads.get_mut(key) {
            *d = Download { received, total };
        }
    }

    pub fn downloaded(&mut self, key: &str, path: &Path, result: Result<u64, ApiError>) {
        if self.ui.attach.downloads.remove(key).is_none() {
            return;
        }
        let name = path.file_name().map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());
        match result {
            Ok(_) => self.toast(ToastKind::Success, format!("Saved {name}")),
            Err(e) => self.toast(ToastKind::Danger, format!("Couldn't save {name}. {}", crate::state::sentence(&e.to_string()))),
        }
    }

    // ---- lightbox ----------------------------------------------------------------------------

    fn message_in_view(&self, id: u64) -> Option<&Message> {
        let c = self.current_channel()?;
        self.log(c)?.messages.iter().find(|m| m.id == id)
    }

    /// Opens the pictures of message `message` on attachment `attachment`.
    pub fn open_lightbox_attachment(&mut self, message: u64, attachment: u64) {
        let Some(m) = self.message_in_view(message) else { return };
        // A video opens alone, large.
        if let Some(v) = m.attachments.iter().find(|a| a.id == attachment && !a.removed && file_kind(&a.content_type, &a.filename) == FileKind::Video)
            && let Some(key) = v.file.clone()
        {
            let item = LbItem { name: v.filename.clone(), key, thumb: None, poster: None, size: (v.width.unwrap_or(0), v.height.unwrap_or(0)), video: true };
            return self.show_lightbox(vec![item], 0);
        }
        let items: Vec<(u64, LbItem)> = m
            .attachments
            .iter()
            .filter(|a| !a.removed && file_kind(&a.content_type, &a.filename) == FileKind::Image)
            .filter_map(|a| {
                Some((
                    a.id,
                    LbItem { name: a.filename.clone(), key: a.file.clone()?, thumb: a.thumbnail.clone(), poster: None, size: (a.width.unwrap_or(0), a.height.unwrap_or(0)), video: false },
                ))
            })
            .collect();
        let at = items.iter().position(|(id, _)| *id == attachment).unwrap_or(0);
        self.show_lightbox(items.into_iter().map(|(_, i)| i).collect(), at);
    }

    /// Opens the media of embed `embed` of message `message` on item `item`.
    pub fn open_lightbox_embed(&mut self, message: u64, embed: usize, item: usize) {
        let Some(e) = self.message_in_view(message).and_then(|m| m.embeds.get(embed)) else { return };
        let name = if e.title.is_empty() { e.author_name.clone() } else { e.title.clone() };
        let items: Vec<LbItem> = e
            .media
            .iter()
            .map(|m| LbItem {
                name: if name.is_empty() { "Media".into() } else { name.clone() },
                key: m.file.clone(),
                thumb: m.poster.clone(),
                poster: m.poster.clone(),
                size: (m.width, m.height),
                video: m.kind != EmbedMediaKind::Image,
            })
            .collect();
        self.show_lightbox(items, item);
    }

    fn show_lightbox(&mut self, items: Vec<LbItem>, at: usize) {
        if items.is_empty() {
            return;
        }
        let at = at.min(items.len() - 1);
        self.ui.attach.lightbox = Some(Lightbox { items, at });
        self.want_media();
    }

    pub fn close_lightbox(&mut self) {
        // A clip shown there goes quiet with it.
        if let Some(lb) = self.ui.attach.lightbox.take() {
            for i in lb.items.iter().filter(|i| i.video) {
                if let Some(c) = self.ui.attach.clips.get(&i.key).filter(|c| !c.gifv()) {
                    c.player.pause();
                }
            }
        }
    }

    /// Arrow keys and the buttons: the next (`1`) or previous (`-1`) picture of the message.
    pub fn lightbox_step(&mut self, delta: i32) {
        if let Some(lb) = &mut self.ui.attach.lightbox {
            let n = lb.items.len() as i32;
            let next = lb.at as i32 + delta;
            if (0..n).contains(&next) {
                let was = lb.items[lb.at].clone();
                lb.at = next as usize;
                if was.video
                    && let Some(c) = self.ui.attach.clips.get(&was.key).filter(|c| !c.gifv())
                {
                    c.player.pause();
                }
            }
        }
        self.want_media();
    }

    // ---- embeds ------------------------------------------------------------------------------

    pub fn embed_collapsed(&self, message: u64, embed: usize) -> bool {
        self.ui.attach.embeds_collapsed.contains(&(message, embed as u32))
    }

    /// The card's header: folds an open embed, opens a folded one; remembered across launches.
    pub fn toggle_embed(&mut self, message: u64, embed: usize) {
        let key = (message, embed as u32);
        let list = &mut self.ui.attach.embeds_collapsed;
        match list.iter().position(|k| *k == key) {
            Some(at) => {
                list.remove(at);
            }
            None => {
                list.push_back(key);
                while list.len() > EMBED_PREFS_CAP {
                    list.pop_front();
                }
            }
        }
        self.ui.attach.embeds_dirty = true;
    }

    // ---- players -----------------------------------------------------------------------------

    pub fn video_supported(&self) -> bool {
        !self.ui.attach.video_unsupported
    }

    pub fn clip(&self, key: &str) -> Option<&Clip> {
        self.ui.attach.clips.get(key)
    }

    /// Whether `key` can't be played: the platform can't, or its player failed.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn clip_failed(&self, key: &str) -> bool {
        !self.video_supported() || self.clip(key).is_some_and(|c| matches!(c.player.state(), PlayerState::Failed(_)))
    }

    /// What a card shows of `key`'s player.
    pub fn clip_ui(&self, key: &str) -> VideoUi {
        if let Some(ui) = self.ui.attach.pinned.get(key) {
            return *ui;
        }
        let Some(c) = self.clip(key) else {
            return VideoUi { failed: !self.video_supported(), muted: true, ..VideoUi::default() };
        };
        let state = c.player.state();
        VideoUi {
            playing: c.player.is_playing(),
            time: c.player.time(),
            duration: c.player.duration().unwrap_or(0.0),
            muted: c.player.muted(),
            loading: matches!(state, PlayerState::Loading),
            failed: !self.video_supported() || matches!(state, PlayerState::Failed(_)),
        }
    }

    /// Starts `key` (made on first use, muted). Another video that was playing is paused.
    pub fn play_clip(&mut self, key: &str, source: &str, kind: ClipKind) {
        if kind != ClipKind::Audio && !self.video_supported() {
            return;
        }
        if kind != ClipKind::Gifv {
            for (k, c) in &self.ui.attach.clips {
                if k != key && !c.gifv() {
                    c.player.pause();
                }
            }
        }
        if !self.ui.attach.clips.contains_key(key) {
            let p = if kind == ClipKind::Audio { Player::audio(source.to_owned()) } else { Player::url(source.to_owned()) };
            if kind != ClipKind::Audio {
                p.set_muted(true);
            }
            p.set_looping(kind == ClipKind::Gifv);
            if let Some(w) = &self.ui.attach.waker {
                p.set_waker(w.clone());
            }
            self.ui.attach.clips.insert(key.to_owned(), Clip { player: p, kind });
            self.ui.attach.clip_order.push(key.to_owned());
            self.trim_clips(key);
        }
        if let Some(c) = self.ui.attach.clips.get(key) {
            c.player.play();
        }
    }

    fn trim_clips(&mut self, keep: &str) {
        while self.ui.attach.clips.len() > PLAYER_CAP {
            let Some(at) = self.ui.attach.clip_order.iter().position(|k| k != keep && !self.ui.attach.clips.get(k).is_some_and(|c| c.player.is_playing())) else { break };
            let k = self.ui.attach.clip_order.remove(at);
            self.ui.attach.clips.remove(&k);
        }
    }

    /// Pause when playing, play when not (the first press of a card goes through `play_clip`).
    pub fn toggle_clip(&mut self, key: &str, source: &str, kind: ClipKind) {
        match self.clip(key).map(|c| c.player.is_playing()) {
            Some(true) => {
                if let Some(c) = self.clip(key) {
                    c.player.pause();
                }
            }
            _ => self.play_clip(key, source, kind),
        }
    }

    pub fn mute_clip(&mut self, key: &str) {
        if let Some(c) = self.clip(key) {
            c.player.set_muted(!c.player.muted());
        }
    }

    /// The scrub bar: jump to `fraction` of the clip.
    pub fn seek_clip(&mut self, key: &str, fraction: f32) {
        if let Some(c) = self.clip(key)
            && let Some(d) = c.player.duration()
        {
            c.player.seek(d * f64::from(fraction.clamp(0.0, 1.0)));
        }
    }

    /// A looping clip came into view (or left it): it plays while visible, unless motion is
    /// reduced, when it waits for a press like any video.
    pub fn gifv_visible(&mut self, key: &str, source: &str, on: bool) {
        if on {
            if !self.ui.reduced.get() {
                self.play_clip(key, source, ClipKind::Gifv);
            }
        } else if let Some(c) = self.clip(key) {
            c.player.pause();
        }
    }

    /// Whether any player is running: the window ticks to move its time while one is.
    pub fn any_clip_playing(&self) -> bool {
        self.ui.attach.clips.values().any(|c| c.player.is_playing() && !c.gifv())
    }

    // ---- what the transcript needs fetched ---------------------------------------------------

    /// The picture keys of the newest messages of the open channel, and of the open lightbox.
    pub fn attachment_media_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        let in_view = self.current_channel().and_then(|c| self.log(c)).map(|l| l.messages.iter().rev().take(MEDIA_WINDOW)).into_iter().flatten();
        for m in in_view {
            for a in &m.attachments {
                if a.removed || file_kind(&a.content_type, &a.filename) != FileKind::Image {
                    continue;
                }
                match (&a.thumbnail, &a.file) {
                    (Some(t), _) => keys.push(t.clone()),
                    (None, Some(f)) if a.size <= INLINE_MAX_BYTES => keys.push(f.clone()),
                    _ => {}
                }
            }
            for e in &m.embeds {
                keys.extend(e.thumbnail.clone());
                keys.extend(e.author_icon.clone());
                for md in &e.media {
                    keys.extend(md.poster.clone());
                    if md.kind == EmbedMediaKind::Image {
                        keys.push(md.file.clone());
                    }
                }
            }
        }
        if let Some(lb) = &self.ui.attach.lightbox {
            let from = lb.at.saturating_sub(1);
            for i in lb.items.iter().skip(from).take(3) {
                keys.extend(i.thumb.clone());
                keys.extend(i.poster.clone());
                if !i.video {
                    keys.push(i.key.clone());
                }
            }
        }
        keys
    }
}
