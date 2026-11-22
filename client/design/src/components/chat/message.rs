//! MessageGroup: an avatar, the author and time, and one or more bodies. Consecutive messages
//! by one author within [`GROUP_WINDOW_SECS`] share a group (see [`MessageGroup::runs`]);
//! follow-ups show their time in the gutter on hover.
//!
//! Delivery: `Pending` is dimmed (no spinner), `Sent` is plain, `Failed` adds a danger line with
//! a Retry link. A message that mentions the current user takes a faint accent wash and a 2pt
//! accent bar on its left edge. Hovering a message shows a floating bar (reply, copy, more).
//!
//! Motion: a newly mounted message row fades in and rises 6pt on `motion::CONTROL`; pending to
//! sent only animates opacity; existing rows only move to make room. Keep a message's `id`
//! stable across pending -> sent (use a client nonce) or the row remounts and replays the entrance.
//!
//! Bodies are markup (see `body`); the `id` also scopes which spoilers are revealed.

use std::rc::Rc;

use rune::{col, el, row, text, El, SpringPair, Transition};
use rune_kit::{Kit, RollingText};

use super::body;
use super::emoji::EmojiSet;
use super::emoji_picker::picture;
use crate::components::{Avatar, IconButton, Size, Variant};
use crate::icons::{COPY, DOTS_THREE, PENCIL, PLUS, REPLY, SMILEY, TRASH};
use crate::theme_of;
use crate::tokens::{motion, radius, type_};

/// Messages by one author this close together (seconds) collapse into one group.
pub const GROUP_WINDOW_SECS: i64 = 5 * 60;
/// Opacity of a message awaiting its ack.
const PENDING_OPACITY: f32 = 0.55;
// Space above a group (`Metrics::group_gap`, 12 / 7) plus the row padding is ~16 / ~10 between
// groups, while rows inside a group sit 2 / 1pt apart, so a run reads as one block.
/// The hover bar: a 24pt IconButton, 2pt padding and a 1pt border around it.
const ACTION_BAR_H: f32 = 24.0 + 2.0 * 2.0 + 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DeliveryState {
    #[default]
    Sent,
    /// Sent, awaiting the server's ack.
    Pending,
    Failed,
}

/// Distinct emojis a message may carry reactions for; past it the "+" chip is disabled.
pub const MAX_REACTIONS: usize = 20;
/// Side of a custom emoji in a reaction chip.
const CHIP_EMOJI: f32 = 18.0;

/// The emoji of a reaction chip.
#[derive(Clone, Debug, PartialEq)]
pub enum ReactionGlyph {
    Unicode(String),
    /// A custom emoji: its id (to find the picture) and its name (the fallback label).
    Custom { id: String, name: String },
}

/// One chip under a message: an emoji, how many reacted, whether you did.
#[derive(Clone, Debug, PartialEq)]
pub struct ReactionChip {
    /// The reaction's key (`u:…` / `c:…`), handed back when the chip is pressed.
    pub key: String,
    pub glyph: ReactionGlyph,
    pub count: u32,
    pub me: bool,
    /// The hover text: who reacted, or at least the emoji's name.
    pub tip: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ChatMessage {
    pub id: String,
    pub author_id: String,
    pub author_name: String,
    /// Unix seconds, for grouping.
    pub at: i64,
    /// Display time ("14:03"), shown in the header and the follow-up gutter.
    pub time_label: String,
    pub body: String,
    pub delivery: DeliveryState,
    pub mentions_me: bool,
    pub reactions: Vec<ReactionChip>,
    /// Set once the message was edited: the hover text of its faint "(edited)" mark ("Edited
    /// today at 14:05").
    pub edited: Option<String>,
    /// The hover bar offers Edit (your own message).
    pub can_edit: bool,
    /// The hover bar offers Delete (your own, or anyone's with the permission).
    pub can_delete: bool,
}

/// Whether `next` joins the group `prev` ends: same author, at most five minutes later.
pub fn collapses_into(prev: &ChatMessage, next: &ChatMessage) -> bool {
    prev.author_id == next.author_id && (0..=GROUP_WINDOW_SECS).contains(&(next.at - prev.at))
}

type Hook = Box<dyn Fn(El, &str) -> El>;
/// Hooks the chips: the message id and the reaction key.
type ChipHook = Box<dyn Fn(El, &str, &str) -> El>;
/// Hooks the Delete button: the message id and whether Shift was held.
type DeleteHook = Box<dyn Fn(El, &str) -> El>;
/// Wraps the More button (the app hangs its menu on it), by message id.
type Wrap = Rc<dyn Fn(El, &str) -> El>;
/// Builds the editor that replaces a message's body while it is being edited, by message id.
type Editor = Rc<dyn Fn(&str) -> Option<El>>;
/// Looks something up by author id: their picture, or their name's colour.
/// Builds what hangs under a message's body (its attachments and embeds), by message id.
type Extras = Rc<dyn Fn(&str) -> Vec<El>>;
type ById<T> = Rc<dyn Fn(&str) -> Option<T>>;

pub struct MessageGroup {
    messages: Vec<ChatMessage>,
    show_actions: Option<String>,
    reply: Option<Hook>,
    copy: Option<Hook>,
    more: Option<Hook>,
    more_wrap: Option<Wrap>,
    edit: Option<Hook>,
    delete: Option<DeleteHook>,
    context: Option<Hook>,
    editor: Option<Editor>,
    retry: Option<Hook>,
    react: Option<ChipHook>,
    add_reaction: Option<ChipHook>,
    emojis: Option<EmojiSet>,
    wire: Option<body::Wire>,
    avatar: Option<ById<rune::Pixels>>,
    name_color: Option<ById<rune::Color>>,
    mention: Option<body::MentionInk>,
    extras: Option<Extras>,
}

fn hook<S: 'static>(f: impl Fn(&mut S, &str) + 'static) -> Hook {
    let f = Rc::new(f);
    Box::new(move |e, id| {
        let (f, id) = (f.clone(), id.to_owned());
        e.on_press(move |s: &mut S| f(s, &id))
    })
}

impl MessageGroup {
    /// `messages` must be one run (see [`MessageGroup::runs`]); the first sets the header.
    pub fn new(messages: &[ChatMessage]) -> MessageGroup {
        MessageGroup {
            messages: messages.to_vec(),
            show_actions: None,
            reply: None,
            copy: None,
            more: None,
            more_wrap: None,
            edit: None,
            delete: None,
            context: None,
            editor: None,
            retry: None,
            react: None,
            add_reaction: None,
            emojis: None,
            wire: None,
            avatar: None,
            name_color: None,
            mention: None,
            extras: None,
        }
    }

    /// Content under a message's body, built by the app from the message id: attachments, then
    /// embeds. Empty means nothing.
    pub fn extras(mut self, f: impl Fn(&str) -> Vec<El> + 'static) -> Self {
        self.extras = Some(Rc::new(f));
        self
    }

    /// How custom emojis in bodies and reaction chips find their pictures.
    pub fn emojis(mut self, set: EmojiSet) -> Self {
        self.emojis = Some(set);
        self
    }

    /// A chip was pressed: toggles your reaction. Called with the message id and the reaction key.
    pub fn on_react<S: 'static>(mut self, f: impl Fn(&mut S, &str, &str) + 'static) -> Self {
        let f = Rc::new(f);
        self.react = Some(Box::new(move |e, id, key| {
            let (f, id, key) = (f.clone(), id.to_owned(), key.to_owned());
            e.on_press(move |s: &mut S| f(s, &id, &key))
        }));
        self
    }

    /// The "+" chip or the hover bar's React button was pressed: the app opens its picker, hung
    /// from the element keyed by the second argument (`react-add-<id>` or `react-bar-<id>`).
    /// Called with the message id and that key.
    pub fn on_add_reaction<S: 'static>(mut self, f: impl Fn(&mut S, &str, &str) + 'static) -> Self {
        let f = Rc::new(f);
        self.add_reaction = Some(Box::new(move |e, id, anchor| {
            let (f, id, anchor) = (f.clone(), id.to_owned(), anchor.to_owned());
            e.on_press(move |s: &mut S| f(s, &id, &anchor))
        }));
        self
    }

    /// Each author's picture (by `ChatMessage::author_id`), drawn over their initials.
    pub fn avatars(mut self, f: impl Fn(&str) -> Option<rune::Pixels> + 'static) -> Self {
        self.avatar = Some(Rc::new(f));
        self
    }

    /// Each author's name colour (by `ChatMessage::author_id`): the text tone of their role.
    pub fn name_colors(mut self, f: impl Fn(&str) -> Option<rune::Color> + 'static) -> Self {
        self.name_color = Some(Rc::new(f));
        self
    }

    /// The colours of an `@name` chip for a member with a coloured role (see `body::MentionInk`).
    pub fn mention_ink(
        mut self,
        f: impl Fn(&str) -> Option<(rune::Color, rune::Color)> + 'static,
    ) -> Self {
        self.mention = Some(Rc::new(f));
        self
    }

    /// Makes links and spoilers in the bodies pressable in an app whose state is `S`: a link
    /// opens in the default browser (http, https, mailto only), a spoiler reveals. Without it
    /// they are drawn but inert.
    pub fn links<S: 'static>(mut self) -> Self {
        self.wire = Some(body::wire::<S>());
        self
    }

    /// Splits a transcript into runs that each become one group.
    pub fn runs(messages: &[ChatMessage]) -> Vec<&[ChatMessage]> {
        let mut out = Vec::new();
        let mut start = 0;
        for i in 1..=messages.len() {
            if i == messages.len() || !collapses_into(&messages[i - 1], &messages[i]) {
                out.push(&messages[start..i]);
                start = i;
            }
        }
        out.retain(|r| !r.is_empty());
        out
    }

    /// Forces the hover state (highlight, action bar, gutter time) on message `id`.
    pub fn force_hover(mut self, id: impl Into<String>) -> Self {
        self.show_actions = Some(id.into());
        self
    }
    pub fn on_reply<S: 'static>(mut self, f: impl Fn(&mut S, &str) + 'static) -> Self {
        self.reply = Some(hook(f));
        self
    }
    pub fn on_copy<S: 'static>(mut self, f: impl Fn(&mut S, &str) + 'static) -> Self {
        self.copy = Some(hook(f));
        self
    }
    pub fn on_more<S: 'static>(mut self, f: impl Fn(&mut S, &str) + 'static) -> Self {
        self.more = Some(hook(f));
        self
    }
    /// The Edit button (on messages with `can_edit`).
    pub fn on_edit<S: 'static>(mut self, f: impl Fn(&mut S, &str) + 'static) -> Self {
        self.edit = Some(hook(f));
        self
    }
    /// The Delete button (on messages with `can_delete`); the flag is whether Shift was held
    /// (the app may skip its confirmation).
    pub fn on_delete<S: 'static>(mut self, f: impl Fn(&mut S, &str, bool) + 'static) -> Self {
        let f = Rc::new(f);
        self.delete = Some(Box::new(move |e, id| {
            let (f, id) = (f.clone(), id.to_owned());
            e.on_press_tx(move |s: &mut S, tx| f(s, &id, tx.mods().shift))
        }));
        self
    }
    /// A secondary press on a message: the app opens its menu at the point.
    pub fn on_context<S: 'static>(mut self, f: impl Fn(&mut S, &str, rune::PointerPress) + 'static) -> Self {
        let f = Rc::new(f);
        self.context = Some(Box::new(move |e, id| {
            let (f, id) = (f.clone(), id.to_owned());
            e.on_secondary_press(move |s: &mut S, p, _tx| f(s, &id, p))
        }));
        self
    }
    /// Lets the app wrap the More button, to hang a menu on it (the button's press stays `on_more`).
    pub fn wrap_more(mut self, f: impl Fn(El, &str) -> El + 'static) -> Self {
        self.more_wrap = Some(Rc::new(f));
        self
    }
    /// The editor shown in place of the body of the message the app is editing: the app returns
    /// `Some` for that message's id and `None` for the rest.
    pub fn editor(mut self, f: impl Fn(&str) -> Option<El> + 'static) -> Self {
        self.editor = Some(Rc::new(f));
        self
    }
    pub fn on_retry<S: 'static>(mut self, f: impl Fn(&mut S, &str) + 'static) -> Self {
        self.retry = Some(hook(f));
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let Some(first) = self.messages.first() else {
            return col();
        };
        let mut g = col()
            .key(format!("group-{}", first.id))
            .w_full()
            .mt(crate::metrics_of(k).group_gap())
            .transition(Transition::none());
        for (i, m) in self.messages.iter().enumerate() {
            g = g.child(self.row(k, m, i == 0));
        }
        g
    }

    fn row(&self, k: &Kit, m: &ChatMessage, head: bool) -> El {
        let t = theme_of(k);
        let mt = crate::metrics_of(k);
        let forced = self.show_actions.as_deref() == Some(m.id.as_str());
        let gutter = if head {
            Avatar::new(m.author_id.clone(), m.author_name.clone())
                .size(mt.msg_avatar())
                .image(self.avatar.as_ref().and_then(|f| f(&m.author_id)))
                .build(k)
        } else {
            let stamp = text(m.time_label.clone())
                .font(type_::at(k, type_::CAPTION))
                .color(t.text_muted.color());
            let stamp = if forced {
                stamp
            } else {
                stamp.group_opacity(0.0, 1.0)
            };
            row()
                .w(mt.msg_avatar() as f32)
                .h(mt.font(type_::BODY).line_px())
                .center()
                .child(stamp)
        };

        let mut content = col().grow(1.0).shrink().min_w(0.0).gap(2.0);
        if head {
            content = content.child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(
                        text(m.author_name.clone()).font(type_::at(k, type_::BODY_STRONG)).color(
                            self.name_color
                                .as_ref()
                                .and_then(|f| f(&m.author_id))
                                .unwrap_or(t.text.color()),
                        ),
                    )
                    .child(
                        text(m.time_label.clone())
                            .font(type_::at(k, type_::CAPTION))
                            .color(t.text_muted.color()),
                    ),
            );
        }
        let dim = if m.delivery == DeliveryState::Pending {
            PENDING_OPACITY
        } else {
            1.0
        };
        let editing = self.editor.as_ref().and_then(|f| f(&m.id));
        let is_editing = editing.is_some();
        content = content.child(match editing {
            Some(editor) => editor,
            None => body::render_full(k, &m.body, &m.id, self.wire.as_ref(), self.mention.as_ref(), self.emojis.as_ref())
                .key("body")
                .opacity(dim)
                .opacity_motion(motion::CONTROL),
        });
        if let (Some(tip), false) = (&m.edited, is_editing) {
            content = content.child(
                text("(edited)")
                    .key("edited")
                    .font(type_::at(k, type_::CAPTION))
                    .color(t.text_faint.color())
                    .opacity(dim)
                    .help(tip.clone()),
            );
        }
        if let Some(f) = &self.extras {
            let under = f(&m.id);
            if !under.is_empty() {
                content = content.child(col().key("extras").w_full().gap(6.0).pt(4.0).opacity(dim).children(under));
            }
        }
        if !m.reactions.is_empty() {
            content = content.child(self.chips(k, m));
        }
        if m.delivery == DeliveryState::Failed {
            let mut retry = text("Retry")
                .font(type_::at(k, type_::CONTROL_STRONG))
                .color(t.danger.color())
                .role("button")
                .label("Retry sending")
                .focusable(true)
                .hover(t.hover.color());
            if let Some(h) = &self.retry {
                retry = h(retry, &m.id);
            }
            content = content.child(
                row()
                    .key("failed")
                    .gap(4.0)
                    .center_y()
                    .child(
                        text("Failed to send \u{00b7}")
                            .font(type_::at(k, type_::CONTROL))
                            .color(t.danger.color()),
                    )
                    .child(retry),
            );
        }

        let pad = mt.msg_pad(head);
        let enter = Transition::offset(0.0, 6.0)
            .combined(Transition::opacity())
            .springs(SpringPair::new(motion::CONTROL, motion::CONTROL));
        let mut r = row()
            .key(format!("msg-{}", m.id))
            .w_full()
            .px(16.0)
            .pt(pad.0)
            .pb(pad.1)
            .gap(12.0)
            .items_start()
            .hover_group()
            .transition(Transition::asymmetric(enter, Transition::none()))
            .token_motion(motion::CONTROL);
        if m.mentions_me {
            r = r.fill(t.accent_wash.color()).child(
                el().key("mention-bar")
                    .absolute(0.0, 0.0)
                    .w(2.0)
                    .h_full()
                    .fill(t.accent.color())
                    .a11y_hidden(),
            );
        }
        r = if forced {
            r.fill(crate::components::util::over(
                t.hover.color(),
                if m.mentions_me {
                    t.accent_wash.color()
                } else {
                    rune::Color::TRANSPARENT
                },
            ))
        } else {
            r.hover(t.hover.color())
        };
        r = r.child(gutter).child(content);
        if let Some(h) = &self.context {
            // A row with a menu is named for assistive technology: who said it, and when.
            r = h(r, &m.id).role("article").label(format!("{}, {}", m.author_name, m.time_label));
        }
        if is_editing {
            // The editor has its own keys; no bar floats over it.
            return r;
        }
        r.child(self.actions(k, m, forced))
    }

    /// The chips under a body, then "+".
    fn chips(&self, k: &Kit, m: &ChatMessage) -> El {
        let t = theme_of(k);
        let mt = crate::metrics_of(k);
        let none = EmojiSet::none();
        let set = self.emojis.as_ref().unwrap_or(&none);
        // A chip pops in when its emoji is first reacted with; a count change rolls.
        let pop = Transition::scale(0.8)
            .combined(Transition::opacity())
            .springs(SpringPair::new(motion::POP, motion::CONTROL));
        let mut line = row().key("reactions").w_full().flex_wrap().gap(4.0).pt(2.0).items_center();
        for c in &m.reactions {
            let art = match &c.glyph {
                ReactionGlyph::Unicode(g) => text(g.clone()).font(mt.font(rune::TextStyle::new(15.0, 400.0).lh(20.0))).a11y_hidden(),
                ReactionGlyph::Custom { id, name } => match set.get(id) {
                    Some(img) => picture(k, set, &img, mt.em(CHIP_EMOJI), false),
                    None => text(format!(":{name}:")).font(type_::at(k, type_::CAPTION)).color(t.text_muted.color()).a11y_hidden(),
                },
            };
            let ink = if c.me { t.accent_text } else { t.text_muted };
            let count = RollingText::new(format!("rxc-{}-{}", m.id, c.key), c.count.to_string())
                .value(f64::from(c.count))
                .font(type_::at(k, type_::CONTROL_STRONG))
                .color(ink.color())
                .build(k);
            let mut chip = row()
                .key(format!("rx-{}-{}", m.id, c.key))
                .role("button")
                .label(format!("{}, {} {}{}", c.tip, c.count, if c.count == 1 { "reaction" } else { "reactions" }, if c.me { ", you reacted" } else { "" }))
                .focus_selected(c.me)
                .min_h(mt.em(24.0))
                .px(6.0)
                .gap(5.0)
                .center_y()
                .radius(radius::MD)
                .focusable(true)
                .focus_ring(2.0, 1.0, k.c.border_focus)
                .help(c.tip.clone())
                .transition(pop)
                .token_motion(motion::CONTROL)
                .child(art)
                .child(count);
            chip = if c.me {
                chip.fill(t.accent_subtle.color()).border(1.0, t.accent.color())
            } else {
                chip.fill(t.hover.color()).hover(t.pressed.color()).press(t.pressed.color())
            };
            if let Some(h) = &self.react {
                chip = h(chip, &m.id, &c.key);
            }
            line = line.child(chip);
        }
        let full = m.reactions.len() >= MAX_REACTIONS;
        let mut add = row()
            .key(format!("react-add-{}", m.id))
            .role("button")
            .label(if full { "Add reaction (limit reached)" } else { "Add reaction" })
            .size(28.0, mt.em(24.0))
            .center()
            .radius(radius::MD)
            .fill(t.hover.color())
            .focusable(!full)
            .focus_ring(2.0, 1.0, k.c.border_focus)
            .help(if full { format!("A message can have {MAX_REACTIONS} different reactions") } else { "Add reaction".to_owned() })
            .child(rune::icon(&PLUS, mt.icon(crate::tokens::icon::XS)).color(t.text_muted.color()).a11y_hidden());
        add = if full {
            add.layer_opacity(0.5).disabled(true)
        } else {
            let a = add.hover(t.pressed.color()).press(t.pressed.color());
            match &self.add_reaction {
                Some(h) => h(a, &m.id, &format!("react-add-{}", m.id)),
                None => a,
            }
        };
        line.child(add)
    }

    fn actions(&self, k: &Kit, m: &ChatMessage, forced: bool) -> El {
        let t = theme_of(k);
        let id = m.id.as_str();
        let btn = |g: &'static rune::Icon, label: &str, h: &Option<Hook>| {
            let b = IconButton::new(g, label.to_owned()).size(Size::Sm);
            let b = b.build(k);
            match h {
                Some(h) => h(b, id),
                None => b,
            }
        };
        let mut bar = row()
            .key("actions")
            .padding(2.0)
            .gap(2.0)
            .radius(radius::MD)
            .fill(t.surface_overlay.color())
            .border(1.0, t.border_subtle.color())
            .child({
                let b = IconButton::new(&SMILEY, "Add reaction").size(Size::Sm).build(k).key(format!("react-bar-{id}"));
                match &self.add_reaction {
                    Some(h) => h(b, id, &format!("react-bar-{id}")),
                    None => b,
                }
            })
            .child(btn(&REPLY, "Reply", &self.reply))
            .child(btn(&COPY, "Copy text", &self.copy));
        if let (true, Some(_)) = (m.can_edit, &self.edit) {
            bar = bar.child(btn(&PENCIL, "Edit message", &self.edit).key(format!("edit-bar-{id}")));
        }
        if let (true, Some(h)) = (m.can_delete, &self.delete) {
            let b = IconButton::new(&TRASH, "Delete message")
                .size(Size::Sm)
                .variant(Variant::Ghost)
                .ink(t.danger.color())
                .hover_fill(t.danger_bg.color())
                .build(k)
                .key(format!("delete-bar-{id}"));
            bar = bar.child(h(b, id));
        }
        let more = btn(&DOTS_THREE, "More", &self.more).key(format!("more-bar-{id}"));
        bar = bar.child(match &self.more_wrap {
            Some(w) => w(more, id),
            None => more,
        });
        bar = k.elevate(bar, &k.elevation.floating);
        if !forced {
            bar = bar.group_opacity(0.0, 1.0);
        }
        // Its bottom edge sits 3pt inside the row's top: over the row above's padding (as Discord's
        // bar does) and clear of this row's own text.
        row()
            .absolute(0.0, -ACTION_BAR_H + 3.0)
            .w_full()
            .pr(16.0)
            .justify_end()
            .child(bar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, who: &str, at: i64) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            author_id: who.into(),
            author_name: who.into(),
            at,
            time_label: String::new(),
            body: String::new(),
            delivery: DeliveryState::Sent,
            mentions_me: false,
            reactions: Vec::new(),
            edited: None,
            can_edit: false,
            can_delete: false,
        }
    }

    #[test]
    fn same_author_within_five_minutes_collapses() {
        assert!(collapses_into(
            &m("1", "a", 0),
            &m("2", "a", GROUP_WINDOW_SECS)
        ));
        assert!(!collapses_into(
            &m("1", "a", 0),
            &m("2", "a", GROUP_WINDOW_SECS + 1)
        ));
        assert!(!collapses_into(&m("1", "a", 0), &m("2", "b", 10)));
        assert!(
            !collapses_into(&m("1", "a", 10), &m("2", "a", 0)),
            "out of order never joins"
        );
    }

    #[test]
    fn runs_split_on_author_and_on_gaps() {
        let msgs = [
            m("1", "a", 0),
            m("2", "a", 60),
            m("3", "b", 90),
            m("4", "a", 100),
            m("5", "a", 1000),
        ];
        let runs: Vec<Vec<&str>> = MessageGroup::runs(&msgs)
            .iter()
            .map(|r| r.iter().map(|m| m.id.as_str()).collect())
            .collect();
        assert_eq!(runs, vec![vec!["1", "2"], vec!["3"], vec!["4"], vec!["5"]]);
        assert!(MessageGroup::runs(&[]).is_empty());
    }
}
