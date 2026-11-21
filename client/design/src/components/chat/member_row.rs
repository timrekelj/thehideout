//! MemberRow: avatar with presence, the name, and a crown for the owner. 32pt tall; offline
//! members read fainter. Hover paints the quiet overlay; nothing animates.

use rune::{icon, row, text, El};
use rune_kit::{Kit, State};

use crate::components::util::{over, Acts};
use crate::components::{Avatar, Presence};
use crate::icons::CROWN;
use crate::theme_of;
use crate::tokens::{icon as size, radius, type_};

pub struct MemberRow {
    id: String,
    name: String,
    presence: Presence,
    owner: bool,
    state: State,
    backdrop: Option<rune::Color>,
    acts: Acts,
    image: Option<rune::Pixels>,
    name_color: Option<rune::Color>,
}

impl MemberRow {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> MemberRow {
        MemberRow {
            id: id.into(),
            name: name.into(),
            presence: Presence::Online,
            owner: false,
            state: State::Rest,
            backdrop: None,
            acts: Acts::default(),
            image: None,
            name_color: None,
        }
    }
    /// The member's picture, over the initials.
    pub fn image(mut self, image: Option<rune::Pixels>) -> Self {
        self.image = image;
        self
    }
    /// The name's colour (their highest coloured role's text tone, see `palette_text`) instead of `text`.
    pub fn name_color(mut self, c: Option<rune::Color>) -> Self {
        self.name_color = c;
        self
    }
    pub fn presence(mut self, p: Presence) -> Self {
        self.presence = p;
        self
    }
    pub fn owner(mut self, on: bool) -> Self {
        self.owner = on;
        self
    }
    /// The surface the row sits on, for the presence dot's cut-out. Defaults to `bg`.
    pub fn backdrop(mut self, c: rune::Color) -> Self {
        self.backdrop = Some(c);
        self
    }
    pub fn state(mut self, s: State) -> Self {
        self.state = s;
        self
    }
    pub fn on_press<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.acts.press(f);
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let t = theme_of(k);
        let faded = matches!(self.presence, Presence::Offline | Presence::None);
        let ink = match (faded, self.name_color) {
            (true, _) => t.text_faint.color(),
            (false, Some(c)) => c,
            (false, None) => t.text.color(),
        };
        let mut avatar = Avatar::new(self.id, self.name.clone())
            .size(24)
            .image(self.image)
            .presence(self.presence);
        if let Some(b) = self.backdrop {
            avatar = avatar.backdrop(b);
        }
        let mut e = row()
            .role("button")
            .label(self.name.clone())
            .w_full()
            .h(crate::metrics_of(k).row_h())
            .px(8.0)
            .gap(10.0)
            .center_y()
            .radius(radius::MD)
            .focusable(true)
            .focus_ring(crate::space::RING_WIDTH, crate::space::RING_INSET, k.c.border_focus);
        e = if self.state == State::Hover {
            e.fill(over(t.hover.color(), rune::Color::TRANSPARENT))
        } else {
            e.hover(t.hover.color()).press(t.pressed.color())
        };
        e = e.child(avatar.build(k)).child(
            text(self.name)
                .font(type_::at(k, type_::CONTROL))
                .color(ink)
                .max_lines(1)
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        );
        if self.owner {
            e = e.child(
                icon(&CROWN, crate::metrics_of(k).icon(size::SM))
                    .key("owner")
                    .label("Owner")
                    .color(t.accent_text.color()),
            );
        }
        self.acts.apply(e, true)
    }
}
