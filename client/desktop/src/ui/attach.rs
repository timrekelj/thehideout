//! Attachments, embeds, the composer's tray and the lightbox, wired to the app: the design
//! components get data and wires (press, drag, visibility handlers that call `AppState`), and
//! this module decides what each message shows. Pictures come from the media cache (fetched off
//! the UI thread); a player exists only once a video was started.

use std::rc::Rc;

use rune::{col, row, text, El, Phase, Tx};
use rune_kit::Kit;
use thehideout_api::{Attachment, Embed, EmbedKind, EmbedMedia, EmbedMediaKind, Message};
use thehideout_design::components::chat::attachments::{
    file_kind, fit_size, image_grid_el, lightbox, removed, tray, tray_note, AudioRow, Chip,
    ChipState, FileCard, FileKind, GridImage, LightboxView, VideoCard, Wire,
};
use thehideout_design::components::chat::body;
use thehideout_design::components::chat::embed::{EmbedCard, Provider};
use thehideout_design::components::ToastKind;
use thehideout_design::{theme_of, type_};

use crate::app::App;
use crate::attach::{ClipKind, Draft, LbItem, OutFile, UploadState, INLINE_MAX_BYTES};
use crate::format::{clock_label, short_date};
use crate::state::Outgoing;

// ---- wires -------------------------------------------------------------------------------------

/// What a card's expand button does.
type Expand = Rc<dyn Fn(&mut App)>;

fn wire(f: impl Fn(&mut App) + 'static) -> Wire {
    let f = Rc::new(f);
    Rc::new(move |e: El| {
        let f = f.clone();
        e.on_press(move |a: &mut App| f(a))
    })
}

fn wire_tx(f: impl Fn(&mut App, &mut Tx) + 'static) -> Wire {
    let f = Rc::new(f);
    Rc::new(move |e: El| {
        let f = f.clone();
        e.on_press_tx(move |a: &mut App, tx: &mut Tx| f(a, tx))
    })
}

/// Where a video is played from: its file on this machine (the fake world's clips) or the server.
fn source(a: &App, key: &str) -> String {
    a.backend.media_path(key).map_or_else(
        || a.state.media_url(key),
        |p| p.to_string_lossy().into_owned(),
    )
}

/// Opens `url` in the browser when it is a web address.
fn open_web(tx: &mut Tx, url: &str) {
    if url.starts_with("http://") || url.starts_with("https://") {
        tx.open_url(url).ok();
    }
}

/// The save dialog for `key`; the answer starts the download.
#[cfg(target_arch = "wasm32")]
pub fn save_as(a: &mut App, _tx: &mut Tx, key: String, name: String) {
    // No save dialog on the web: the browser downloads `/media/<key>` (the server names the file).
    crate::web::download_url(&a.state.media_url(&key), &name);
    a.state.toast(ToastKind::Success, format!("Downloading {name}"));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save_as(_a: &mut App, tx: &mut Tx, key: String, name: String) {
    let options = rune::FileDialogOptions {
        title: "Save file".into(),
        file_name: Some(name),
        ..Default::default()
    };
    tx.save_file(options, move |a: &mut App, result, _tx| match result {
        Ok(Some(path)) => a.act(|s| s.start_download(&key, path)),
        Ok(None) => {}
        Err(e) => a.act(|s| {
            s.toast(
                ToastKind::Danger,
                format!("Can't open the save dialog. {e}"),
            )
        }),
    });
}

/// The "+": the system's file chooser (one file at a time: Rune's dialog picks one).
#[cfg(target_arch = "wasm32")]
pub fn choose_files(_tx: &mut Tx) {
    crate::web::pick_files("", true, |a, paths| a.act(|s| s.attach_paths(&paths)));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn choose_files(tx: &mut Tx) {
    let options = rune::FileDialogOptions {
        title: "Attach a file".into(),
        ..Default::default()
    };
    tx.open_file(options, |a: &mut App, result, _tx| match result {
        Ok(Some(path)) => a.act(|s| s.attach_paths(&[path])),
        Ok(None) => {}
        Err(e) => a.act(|s| {
            s.toast(
                ToastKind::Danger,
                format!("Can't open the file chooser. {e}"),
            )
        }),
    });
}

/// A drag that scrubs: its position along the bar, as a fraction of the clip.
fn scrub(key: String) -> Wire {
    Rc::new(move |e: El| {
        let key = key.clone();
        e.on_drag(move |a: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
            if matches!(d.phase, Phase::Down | Phase::Move) && d.w > 0.0 {
                let f = d.x / d.w;
                a.act(|s| s.seek_clip(&key, f));
            }
        })
    })
}

// ---- messages ----------------------------------------------------------------------------------

/// What hangs under message `m`: its attachments, then its embeds.
pub fn extras(a: &App, k: &Kit, m: &Message) -> Vec<El> {
    let mut out = attachments(a, k, m);
    for (i, e) in m.embeds.iter().enumerate() {
        out.push(embed(a, k, m, i, e));
    }
    out
}

/// The key of the picture an inline image shows: its thumbnail, or the file when it is small.
fn inline_key(att: &Attachment) -> Option<&String> {
    att.thumbnail
        .as_ref()
        .or_else(|| att.file.as_ref().filter(|_| att.size <= INLINE_MAX_BYTES))
}

fn attachments(a: &App, k: &Kit, m: &Message) -> Vec<El> {
    let s = &a.state;
    let mut out = Vec::new();
    let images: Vec<&Attachment> = m
        .attachments
        .iter()
        .filter(|x| {
            !x.removed
                && x.file.is_some()
                && file_kind(&x.content_type, &x.filename) == FileKind::Image
        })
        .collect();
    if !images.is_empty() {
        let tiles: Vec<GridImage> = images
            .iter()
            .map(|x| {
                let (msg, id) = (m.id, x.id);
                GridImage {
                    key: format!("{msg}-{id}"),
                    pixels: inline_key(x).and_then(|key| s.data.media.pixels(key)),
                    label: x.filename.clone(),
                    open: wire(move |a| a.act(|s| s.open_lightbox_attachment(msg, id))),
                }
            })
            .collect();
        let single = images.first().and_then(|x| x.width.zip(x.height));
        out.push(image_grid_el(
            k,
            &format!("grid-{}", m.id),
            &tiles,
            single,
            tiles.len(),
        ));
    }
    for att in m
        .attachments
        .iter()
        .filter(|x| !images.iter().any(|i| i.id == x.id))
    {
        let key = format!("att-{}-{}", m.id, att.id);
        if att.removed || att.file.is_none() {
            out.push(removed(k, &key));
            continue;
        }
        let file = att.file.clone().unwrap_or_default();
        let kind = file_kind(&att.content_type, &att.filename);
        match kind {
            FileKind::Video => out.push(video_attachment(a, k, m.id, att, &file)),
            FileKind::Audio => out.push(audio_attachment(a, k, &key, att, &file)),
            _ => out.push(file_card(a, k, &key, att, &file, kind)),
        }
    }
    out
}

fn file_card(a: &App, k: &Kit, key: &str, att: &Attachment, file: &str, kind: FileKind) -> El {
    let (f1, n1, f2) = (file.to_owned(), att.filename.clone(), file.to_owned());
    FileCard {
        key: key.to_owned(),
        name: att.filename.clone(),
        size: att.size,
        kind,
        progress: a.state.ui.attach.downloads.get(file).map(|d| d.fraction()),
        download: wire_tx(move |a, tx| save_as(a, tx, f1.clone(), n1.clone())),
        open: kind
            .opens_inline()
            .then(|| wire_tx(move |a, tx| open_web(tx, &a.state.media_url(&f2)))),
    }
    .build(k)
}

fn audio_attachment(a: &App, k: &Kit, key: &str, att: &Attachment, file: &str) -> El {
    let (f1, f2, f3, n) = (
        file.to_owned(),
        file.to_owned(),
        file.to_owned(),
        att.filename.clone(),
    );
    AudioRow {
        key: key.to_owned(),
        name: att.filename.clone(),
        size: att.size,
        ui: a.state.clip_ui(file),
        toggle: wire(move |a| {
            let src = source(a, &f1);
            a.act(|s| s.toggle_clip(&f1, &src, ClipKind::Audio));
        }),
        seek: scrub(f2),
        download: wire_tx(move |a, tx| save_as(a, tx, f3.clone(), n.clone())),
    }
    .build(k)
}

fn video_attachment(a: &App, k: &Kit, msg: u64, att: &Attachment, file: &str) -> El {
    let size = fit_size(att.width.zip(att.height), 400.0, 300.0);
    let key = format!("vid-{msg}-{}", att.id);
    let att_id = att.id;
    clip_card(
        a,
        k,
        key,
        file,
        size,
        None,
        ClipKind::Video,
        Some(Rc::new(move |a: &mut App| a.act(|s| s.open_lightbox_attachment(msg, att_id)))),
        "",
    )
}

/// A video card for the clip stored as `file`; `original` is where "Open in browser" goes.
#[allow(clippy::too_many_arguments)]
fn clip_card(
    a: &App,
    k: &Kit,
    key: String,
    file: &str,
    size: (f32, f32),
    poster: Option<rune::Pixels>,
    kind: ClipKind,
    expand: Option<Expand>,
    original: &str,
) -> El {
    let s = &a.state;
    let ui = s.clip_ui(file);
    let player = s.clip(file).map(|c| c.player.clone());
    let (f1, f2, f3, f4) = (
        file.to_owned(),
        file.to_owned(),
        file.to_owned(),
        file.to_owned(),
    );
    let url = if original.is_empty() {
        a.state.media_url(file)
    } else {
        original.to_owned()
    };
    let mut card = VideoCard::new(key, size)
        .poster(poster)
        .started(s.ui.attach.pinned.contains_key(file))
        .player(player, ui)
        .controls(kind != ClipKind::Gifv)
        .on_play(wire(move |a| {
            let src = source(a, &f1);
            a.act(|s| s.play_clip(&f1, &src, kind));
        }))
        .on_toggle(wire(move |a| {
            let src = source(a, &f2);
            a.act(|s| s.toggle_clip(&f2, &src, kind));
        }))
        .on_mute(wire(move |a| a.act(|s| s.mute_clip(&f3))))
        .on_seek(scrub(f4))
        .on_browser(wire_tx(move |_, tx| open_web(tx, &url)));
    if kind == ClipKind::Gifv {
        card = card.badge("GIF");
    } else if let Some(expand) = expand {
        card = card.on_expand(wire(move |a| expand(a)));
    }
    if file.is_empty() {
        card = card.poster_only(true);
    }
    let built = card.build(k);
    if kind == ClipKind::Gifv && !file.is_empty() {
        let f = file.to_owned();
        return built.on_visible(move |a: &mut App, on, _tx| {
            let src = source(a, &f);
            a.act(|s| s.gifv_visible(&f, &src, on));
        });
    }
    built
}

// ---- embeds ------------------------------------------------------------------------------------

fn embed(a: &App, k: &Kit, m: &Message, i: usize, e: &Embed) -> El {
    let s = &a.state;
    let provider = Provider::from_key(&e.provider);
    let (msg, url) = (m.id, e.url.clone());
    let at = e
        .published_at
        .map(crate::format::unix)
        .map(|u| format!("{}, {}", short_date(s.day_of(u)), clock_label(s.tz, u)))
        .unwrap_or_default();
    let url2 = url.clone();
    let mut card = EmbedCard::new(format!("embed-{msg}-{i}"), provider)
        .site(e.site_name.clone())
        .collapsed(s.embed_collapsed(msg, i))
        .on_toggle(wire(move |a| a.act(|s| s.toggle_embed(msg, i))))
        .on_open_original(wire_tx(move |_, tx| open_web(tx, &url)))
        .on_open_link(wire_tx(move |_, tx| open_web(tx, &url2)));
    let title = if e.title.is_empty() && provider == Provider::Link {
        e.url.clone()
    } else {
        e.title.clone()
    };
    card = card.title(title.clone()).published(at);
    if provider == Provider::Link && !matches!(e.kind, EmbedKind::Rich) {
        card = card.description(e.description.clone());
        if let Some(p) = e.thumbnail.as_ref().and_then(|t| s.data.media.pixels(t)) {
            card = card.thumbnail(p, e.thumbnail_width.zip(e.thumbnail_height));
        }
        return card.build(k);
    }
    let icon = e.author_icon.as_ref().and_then(|t| s.data.media.pixels(t));
    card = card.author(e.author_name.clone(), icon);
    card = card.text(post_text(k, msg, i, &e.title, &e.description));
    card.media(post_media(a, k, m, i, e)).build(k)
}

/// The post's title (Reddit) and text, as the message markup: links work, nothing else is needed.
fn post_text(k: &Kit, msg: u64, i: usize, title: &str, description: &str) -> El {
    let t = theme_of(k);
    let wire = body::wire::<App>();
    let mut c = col().key("post-text").w_full().gap(2.0);
    if !title.is_empty() {
        c = c.child(
            text(title.to_owned())
                .font(type_::at(k, type_::BODY_STRONG))
                .color(t.text.color())
                .wrap()
                .w_full(),
        );
    }
    if !description.is_empty() {
        c = c.child(body::render_full(
            k,
            description,
            &format!("embed-{msg}-{i}"),
            Some(&wire),
            None,
            None,
        ));
    }
    c
}

fn post_media(a: &App, k: &Kit, m: &Message, i: usize, e: &Embed) -> Vec<El> {
    let s = &a.state;
    let mut out = Vec::new();
    let msg = m.id;
    let images: Vec<(usize, &EmbedMedia)> = e
        .media
        .iter()
        .enumerate()
        .filter(|(_, x)| x.kind == EmbedMediaKind::Image)
        .collect();
    if !images.is_empty() {
        let tiles: Vec<GridImage> = images
            .iter()
            .map(|(j, x)| {
                let j = *j;
                GridImage {
                    key: format!("{msg}-{i}-{j}"),
                    pixels: s.data.media.pixels(&x.file),
                    label: if e.title.is_empty() {
                        "picture".into()
                    } else {
                        e.title.clone()
                    },
                    open: wire(move |a| a.act(|s| s.open_lightbox_embed(msg, i, j))),
                }
            })
            .collect();
        let single = images.first().map(|(_, x)| (x.width, x.height));
        out.push(image_grid_el(
            k,
            &format!("egrid-{msg}-{i}"),
            &tiles,
            single,
            tiles.len(),
        ));
    }
    for (j, x) in e
        .media
        .iter()
        .enumerate()
        .filter(|(_, x)| x.kind != EmbedMediaKind::Image)
    {
        let size = fit_size(Some((x.width, x.height)), 400.0, 300.0);
        let poster = x.poster.as_ref().and_then(|p| s.data.media.pixels(p));
        let kind = if x.kind == EmbedMediaKind::Gifv {
            ClipKind::Gifv
        } else {
            ClipKind::Video
        };
        let expand: Option<Expand> = Some(Rc::new(move |a: &mut App| {
            a.act(|s| s.open_lightbox_embed(msg, i, j))
        }));
        out.push(clip_card(
            a,
            k,
            format!("evid-{msg}-{i}-{j}"),
            &x.file,
            size,
            poster,
            kind,
            expand,
            &e.url,
        ));
    }
    out
}

// ---- outgoing ----------------------------------------------------------------------------------

/// The files of a message that is still on its way: cards that can't be pressed yet.
pub fn outgoing_extras(a: &App, k: &Kit, o: &Outgoing) -> Vec<El> {
    let s = &a.state;
    let mut out = Vec::new();
    let thumbs: Vec<(&OutFile, rune::Pixels)> = o
        .files
        .iter()
        .filter_map(|f| Some((f, f.thumb.as_ref().and_then(|t| s.data.media.pixels(t))?)))
        .collect();
    if !thumbs.is_empty() {
        let tiles: Vec<GridImage> = thumbs
            .iter()
            .enumerate()
            .map(|(n, (f, p))| GridImage {
                key: format!("{}-{n}", o.nonce),
                pixels: Some(p.clone()),
                label: f.name.clone(),
                open: Rc::new(|e| e),
            })
            .collect();
        out.push(image_grid_el(
            k,
            &format!("ogrid-{}", o.nonce),
            &tiles,
            None,
            tiles.len(),
        ));
    }
    for (n, f) in o.files.iter().enumerate().filter(|(_, f)| {
        f.thumb
            .as_ref()
            .and_then(|t| s.data.media.pixels(t))
            .is_none()
    }) {
        out.push(
            FileCard {
                key: format!("ofile-{}-{n}", o.nonce),
                name: f.name.clone(),
                size: f.size,
                kind: file_kind(&f.content_type, &f.name),
                progress: None,
                download: Rc::new(|e| e),
                open: None,
            }
            .build(k),
        );
    }
    out
}

// ---- the composer's tray -----------------------------------------------------------------------

/// The chips above the input (and the note that says why a file was refused), if there is any.
pub fn composer_tray(a: &App, k: &Kit, channel: u64) -> Option<El> {
    let d: &Draft = a.state.draft(channel)?;
    if d.files.is_empty() && d.note.is_none() {
        return None;
    }
    let s = &a.state;
    let chips: Vec<Chip> = d
        .files
        .iter()
        .map(|f| {
            let id = f.id;
            Chip {
                key: id.to_string(),
                name: f.name.clone(),
                size: f.size,
                kind: file_kind(&f.content_type, &f.name),
                thumb: f.thumb.as_ref().and_then(|t| s.data.media.pixels(t)),
                state: match &f.state {
                    UploadState::Uploading { sent } => ChipState::Uploading {
                        sent: *sent,
                        total: f.size,
                    },
                    UploadState::Ready { .. } => ChipState::Ready,
                    UploadState::Failed(why) => ChipState::Failed(why.clone()),
                },
                remove: wire(move |a| a.act(|s| s.remove_attachment(id))),
                retry: wire(move |a| a.act(|s| s.retry_attachment(id))),
            }
        })
        .collect();
    let mut c = col().key("tray-area").w_full();
    if !chips.is_empty() {
        c = c.child(tray(k, chips));
    }
    if let Some(n) = &d.note {
        c = c.child(tray_note(k, n));
    }
    Some(c)
}

// ---- the lightbox ------------------------------------------------------------------------------

/// The lightbox over the window, when one is open. `window` is the window's size in points.
pub fn lightbox_layer(a: &App, k: &Kit, layer: &mut Vec<El>, window: (f32, f32)) {
    let s = &a.state;
    let Some(lb) = &s.ui.attach.lightbox else {
        return;
    };
    let item: &LbItem = lb.current();
    let (maxw, maxh) = (
        (window.0 - 208.0).clamp(240.0, 1600.0),
        (window.1 - 190.0).clamp(160.0, 1200.0),
    );
    let natural = (item.size.0 > 0 && item.size.1 > 0).then_some(item.size);
    // A picture is shown at its natural size when it fits, else scaled down; a video fills the room.
    let size = if item.video {
        let (nw, nh) = natural.map_or((16.0, 9.0), |(w, h)| (w as f32, h as f32));
        let scale = (maxw / nw).min(maxh / nh).min(2.0);
        ((nw * scale).round(), (nh * scale).round())
    } else {
        fit_size(natural.or(Some((4, 3))), maxw, maxh)
    };
    let pixels = s
        .data
        .media
        .pixels(&item.key)
        .or_else(|| item.thumb.as_ref().and_then(|t| s.data.media.pixels(t)));
    let video = item.video.then(|| {
        let poster = item.poster.as_ref().and_then(|t| s.data.media.pixels(t));
        clip_card(
            a,
            k,
            "lb-video".into(),
            &item.key,
            size,
            poster,
            ClipKind::Video,
            None,
            "",
        )
    });
    let n = lb.items.len();
    let key = item.key.clone();
    let name = item.name.clone();
    let view = LightboxView {
        name: item.name.clone(),
        pixels,
        video,
        position: (n > 1).then_some((lb.at + 1, n)),
        size,
        on_close: wire(|a| a.act(|s| s.close_lightbox())),
        on_prev: (lb.at > 0).then(|| wire(|a| a.act(|s| s.lightbox_step(-1)))),
        on_next: (lb.at + 1 < n).then(|| wire(|a| a.act(|s| s.lightbox_step(1)))),
        on_download: wire_tx(move |a, tx| save_as(a, tx, key.clone(), name.clone())),
    };
    lightbox(k, layer, view, |e| {
        e.modal(true, |a: &mut App| a.act(|s| s.close_lightbox()))
    });
}

/// A faint hint over the transcript while files are dragged over the window.
pub fn drop_hint(a: &App, k: &Kit) -> Option<El> {
    if !a.state.ui.attach.drop_over || a.state.current_channel().is_none() {
        return None;
    }
    let t = theme_of(k);
    Some(
        col()
            .key("drop-hint")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .center()
            .fill(t.accent_subtle.color())
            .inert()
            .child(
                row()
                    .px(16.0)
                    .h(40.0)
                    .center()
                    .radius(thehideout_design::radius::LG)
                    .fill(t.surface_overlay.color())
                    .child(
                        text("Drop files to attach")
                            .font(type_::at(k, type_::CONTROL_STRONG))
                            .color(t.accent_text.color()),
                    ),
            )
            .a11y_hidden(),
    )
}

/// Pasted pictures and files go to the tray: true when the clipboard held either.
pub fn paste(a: &mut App, content: &rune::ClipboardContent) -> bool {
    if let Some(img) = content.image() {
        let png = img.png().to_vec();
        a.act(|s| s.attach_pasted(png));
        return true;
    }
    if !content.files().is_empty() {
        let files = content.files().to_vec();
        a.act(|s| s.attach_paths(&files));
        return true;
    }
    false
}
