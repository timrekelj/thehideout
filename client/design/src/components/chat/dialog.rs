//! Dialog: a modal for "Create server", "Join server", "Create channel". A scrim fades in and
//! the panel fades from scale 0.98 on `motion::OVERLAY`. Focus trap, Escape, scrim click and
//! restoring focus to the opener are Rune's modal stack (`El::modal`), as rune-kit's own Dialog
//! uses. The panel sizes to its content; the footer holds the buttons, right aligned.
//!
//! Rhythm: 24 padding, title to body 16, label to field 6, field to field 16, body to footer 24.
//! Fields and buttons are the same 32pt control height.

use rune::{El, ScrollSpec, SpringPair, Transition, col, el, row, text};
use rune_kit::Kit;

use crate::components::{IconButton, Size, TextInput};
use crate::icons::X;
use crate::theme_of;
use crate::space::RING;
use crate::tokens::{motion, radius, type_};

pub struct Dialog {
    key: String,
    title: String,
    description: Option<String>,
    body: Vec<El>,
    footer: Vec<El>,
    footer_start: Vec<El>,
    open: bool,
    width: f32,
    body_max_h: Option<f32>,
    close: Option<Box<dyn FnOnce(El) -> El>>,
    button: Option<Box<dyn FnOnce(El) -> El>>,
}

impl Dialog {
    pub fn new(key: impl Into<String>, title: impl Into<String>) -> Dialog {
        Dialog {
            key: key.into(),
            title: title.into(),
            description: None,
            body: Vec::new(),
            footer: Vec::new(),
            footer_start: Vec::new(),
            open: false,
            width: 440.0,
            body_max_h: None,
            close: None,
            button: None,
        }
    }
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = Some(d.into());
        self
    }
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
    /// Caps the body's height: past it the body scrolls and the header and footer stay put (a
    /// window too short for the whole dialog). The app derives it from the window's height.
    pub fn body_max_h(mut self, h: f32) -> Self {
        self.body_max_h = Some(h.max(120.0));
        self
    }
    pub fn child(mut self, e: El) -> Self {
        self.body.push(e);
        self
    }
    /// A footer button at the left edge, away from the rest (a quiet destructive action that is
    /// not the dialog's answer, such as "Delete channel" on a settings dialog).
    pub fn footer_start(mut self, button: El) -> Self {
        self.footer_start.push(button);
        self
    }
    /// A footer button (built `Button`), in order; the primary goes last.
    pub fn footer(mut self, button: El) -> Self {
        self.footer.push(button);
        self
    }
    /// Runs on Escape, scrim click and the close button.
    pub fn on_close<S: 'static>(mut self, f: impl Fn(&mut S) + Clone + 'static) -> Self {
        let b = f.clone();
        self.button = Some(Box::new(move |e| e.on_press(b)));
        self.close = Some(Box::new(move |e| e.modal(true, f)));
        self
    }

    /// The panel as plain content (no modal, no scrim, no transition), elevated like the real one:
    /// for showing a dialog's anatomy in the gallery or a settings preview.
    pub fn preview(mut self, k: &Kit) -> El {
        let panel = self.panel(k);
        k.elevate(panel, &k.elevation.overlay)
    }

    fn panel(&mut self, k: &Kit) -> El {
        let t = theme_of(k);
        let mut x = IconButton::new(&X, "Close")
            .size(Size::Sm)
            .build(k)
            .key("close");
        if let Some(b) = self.button.take() {
            x = b(x);
        }
        let mut head = col().grow(1.0).gap(2.0).child(
            text(self.title.clone())
                .font(type_::at(k, type_::TITLE))
                .color(t.text.color()),
        );
        if let Some(d) = self.description.clone() {
            head = head.child(
                text(d)
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.text_muted.color())
                    .wrap(),
            );
        }
        let mut panel = col()
            .key(format!("{}-dialog", self.key))
            .role("dialog")
            .label(self.title.clone())
            .w_full()
            .max_w(self.width)
            .padding(24.0)
            .gap(16.0)
            .radius(radius::LG)
            .fill(t.surface_overlay.color())
            .border(1.0, t.border_subtle.color())
            .child(
                row()
                    .key("header")
                    .w_full()
                    .gap(12.0)
                    .items_start()
                    .child(head)
                    .child(x),
            );
        let body = std::mem::take(&mut self.body);
        if !body.is_empty() {
            let body = col().key("body").w_full().gap(16.0).children(body);
            panel = panel.child(match self.body_max_h {
                // The scroll view clips, so it carries `RING` of room round the body for its
                // controls' focus rings; the negative margin takes it back, so nothing moves.
                Some(h) => col()
                    .key("body-scroll")
                    .mx(-RING)
                    .my(-RING)
                    .padding(RING)
                    .max_h(h + 2.0 * RING)
                    .scroll(ScrollSpec { stick_bottom: false })
                    .child(body),
                None => body,
            });
        }
        let footer = std::mem::take(&mut self.footer);
        let start = std::mem::take(&mut self.footer_start);
        if !footer.is_empty() || !start.is_empty() {
            let spaced = !start.is_empty();
            let mut line = row().key("footer").w_full().mt(8.0).gap(8.0).justify_end().children(start);
            if spaced {
                line = line.child(el().key("footer-gap").grow(1.0));
            }
            line = line.children(footer);
            panel = panel.child(line);
        }
        panel
    }

    /// Pushes the scrim and panel onto `layer` (the list the view gives `rune_kit::root`) when
    /// open. Without `on_close` nothing is shown: a modal must be dismissable.
    pub fn build(mut self, k: &Kit, layer: &mut Vec<El>) {
        if !self.open || self.close.is_none() {
            return;
        }
        let close = self.close.take().unwrap();
        let panel = self.panel(k);
        let panel = k.elevate(panel.token_motion(motion::OVERLAY), &k.elevation.overlay);
        let panel = close(panel).modal_click_outside(true);
        let fade = Transition::opacity().springs(SpringPair::new(motion::OVERLAY, motion::OVERLAY));
        layer.push(
            el().key(format!("{}-scrim", self.key))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .fill(k.c.scrim)
                .transition(fade)
                .a11y_hidden(),
        );
        // The transition sits on the outermost mounting element: a child that mounts inside a
        // parent that is also entering rides the parent's entrance and plays none of its own.
        // The layer is the window's size and the panel is centred in it, so scaling the layer
        // about its centre scales the panel about its own.
        let enter =
            Transition::scale(0.98).springs(SpringPair::new(motion::OVERLAY, motion::OVERLAY));
        layer.push(
            el().key(format!("{}-layer", self.key))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .padding(16.0)
                .center()
                .transition(enter)
                .child(panel),
        );
    }
}

/// A labelled text field that spans the dialog's body, with an inline error under it. `TextInput`
/// is 280pt wide unless told otherwise, which leaves a gap at the right of a 440pt dialog; this
/// fills the body instead.
pub fn dialog_field(k: &Kit, label: &str, input: TextInput, error: Option<&str>) -> El {
    let t = theme_of(k);
    let mut c = col().w_full().gap(6.0).child(
        text(label.to_owned())
            .font(type_::at(k, type_::LABEL))
            .color(t.text_muted.color()),
    );
    c = c.child(
        input
            .width(None)
            .large()
            .invalid(error.is_some())
            .label(label.to_owned())
            .build(k),
    );
    match error {
        Some(e) => c.child(
            text(e.to_owned())
                .key("error")
                .font(type_::at(k, type_::CONTROL))
                .color(t.danger.color())
                .wrap()
                .w_full(),
        ),
        None => c,
    }
}
