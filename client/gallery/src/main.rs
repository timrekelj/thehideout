//! thehideout design gallery: one scene per page, in dark and light. Renders headless:
//!
//! ```sh
//! thehideout-gallery --scene foundations-dark --snap foundations-dark.png --size 1200x1500
//! ```
//! Scenes: `foundations-{dark,light}`, `controls-{dark,light}`, `icons-{dark,light}`, `chat-{dark,light}`,
//! `themes-{dark,light}` (accent x background matrix),
//! `chat-dialog-{dark,light}` (the live Create server dialog open).

#![forbid(unsafe_code)]

mod chat;
mod chat_app;
mod controls;
mod foundations;
mod icons;
#[cfg(test)]
mod motion_tests;
mod themes;
mod ui;

use rune::{AppSpec, Cx, El, ScrollSpec, TextEdit, col, row, text};
use rune_kit::Kit;
use thehideout_design::components::{Button, Size, Variant};
use thehideout_design::{Contrast, FAMILIES, FONTS, ThemeSpec, kit_for, theme_of, type_};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Foundations,
    Controls,
    Icons,
    Chat,
    Themes,
}

pub struct G {
    pub page: Page,
    pub dark: bool,
    /// Accent, background, tint and contrast of every page (`appearance` is `dark` above).
    pub spec: ThemeSpec,
    pub edits: [TextEdit; 6],
    /// The invite code boxes on the Controls page: empty, partial, full, error.
    pub codes: [TextEdit; 4],
    pub tooltip_open: bool,
    pub chat: chat_app::ChatDemo,
    /// The emoji picker on the Chat page's sheet.
    pub emoji: thehideout_design::components::EmojiPickerState,
}

fn init() -> G {
    G {
        page: Page::Foundations,
        dark: true,
        spec: ThemeSpec::default(),
        edits: [
            TextEdit::new(""),
            TextEdit::new("ana@thehideout.chat"),
            TextEdit::new(""),
            TextEdit::new(""),
            TextEdit::new("not-a-code"),
            TextEdit::new(""),
        ],
        codes: [
            TextEdit::new(""),
            TextEdit::new("N0X6"),
            TextEdit::new("N0X6PFNB"),
            TextEdit::new("N0X6PFNB"),
        ],
        tooltip_open: false,
        chat: chat_app::ChatDemo::new(),
        emoji: thehideout_design::components::EmojiPickerState::new(),
    }
}

macro_rules! scene {
    ($page:expr, $dark:expr, $tip:expr) => {
        |s: &mut G| {
            s.page = $page;
            s.dark = $dark;
            s.tooltip_open = $tip;
        }
    };
}

type Scene = (&'static str, fn(&mut G));

const SCENES: &[Scene] = &[
    ("foundations-dark", scene!(Page::Foundations, true, false)),
    ("foundations-light", scene!(Page::Foundations, false, false)),
    ("controls-dark", scene!(Page::Controls, true, true)),
    ("controls-light", scene!(Page::Controls, false, true)),
    ("icons-dark", scene!(Page::Icons, true, false)),
    ("icons-light", scene!(Page::Icons, false, false)),
    ("chat-dark", scene!(Page::Chat, true, false)),
    ("chat-light", scene!(Page::Chat, false, false)),
    ("themes-dark", scene!(Page::Themes, true, false)),
    ("themes-light", scene!(Page::Themes, false, false)),
    ("chat-dialog-dark", |s| {
        s.page = Page::Chat;
        s.dark = true;
        s.chat.dialog = true;
    }),
    ("chat-dialog-light", |s| {
        s.page = Page::Chat;
        s.dark = false;
        s.chat.dialog = true;
    }),
];

fn header(s: &G, k: &Kit) -> El {
    let t = theme_of(k);
    let tab = |label: &str, page: Page| {
        let v = if s.page == page {
            Variant::Secondary
        } else {
            Variant::Ghost
        };
        Button::new(label.to_owned())
            .variant(v)
            .size(Size::Sm)
            .on_press(move |g: &mut G| g.page = page)
            .build(k)
    };
    row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(
            text("thehideout".to_owned())
                .font(type_::TITLE)
                .color(t.accent_text.color()),
        )
        .child(
            text("design language".to_owned())
                .font(type_::BODY)
                .color(t.text_muted.color()),
        )
        .child(row().grow(1.0))
        .child(tab("Foundations", Page::Foundations))
        .child(tab("Controls", Page::Controls))
        .child(tab("Icons", Page::Icons))
        .child(tab("Chat", Page::Chat))
        .child(tab("Themes", Page::Themes))
        .child(
            Button::new(if s.spec.tint { "Tint: on" } else { "Tint: off" })
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .on_press(|g: &mut G| g.spec.tint = !g.spec.tint)
                .build(k),
        )
        .child(
            Button::new(if s.spec.contrast == Contrast::High { "High contrast" } else { "Standard contrast" })
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .on_press(|g: &mut G| {
                    g.spec.contrast = match g.spec.contrast {
                        Contrast::Standard => Contrast::High,
                        Contrast::High => Contrast::Standard,
                    }
                })
                .build(k),
        )
        .child(
            Button::new(if s.dark { "Light" } else { "Dark" })
                .variant(Variant::Secondary)
                .size(Size::Sm)
                .on_press(|g: &mut G| g.dark = !g.dark)
                .build(k),
        )
}

fn view(s: &G, _cx: &mut Cx<G>) -> El {
    let k = kit_for(&s.spec, s.dark);
    let mut layer = Vec::new();
    let body = match s.page {
        Page::Foundations => foundations::page(k),
        Page::Controls => controls::page(s, k, &mut layer),
        Page::Icons => icons::page(k),
        Page::Chat => chat::page(s, k, &mut layer),
        Page::Themes => themes::page(s, k),
    };
    let page = col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .fill(theme_of(k).bg.color())
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    col()
                        .gap(24.0)
                        .padding(32.0)
                        .child(header(s, k))
                        .child(body),
                ),
        );
    rune_kit::root(page, layer)
}

fn main() {
    rune::run(AppSpec {
        scenes: SCENES,
        fonts: FONTS,
        families: FAMILIES,
        ..AppSpec::new("thehideout design gallery", (1200.0, 900.0), init, view)
    });
}
