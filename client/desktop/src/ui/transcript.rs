//! The transcript: grouped messages with date and "New" dividers, opening at the bottom and
//! sticking to it, a "New messages" pill when the reader is scrolled away, older pages loading
//! as the top comes into view, and the empty, loading and failed states.
//!
//! Motion: switching channel crossfades the whole stage (`motion-control`, nothing slides); the
//! viewport of a new channel opens at its end on its first frame. A page that loads lands at once.
//! The pill rises and fades in on `motion-control`.
//!
//! Every message row is a plain element (no virtual list): 150 messages is a few thousand
//! elements and a page adds at most fifty more. Growing histories would want `virtualize`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rune::{Color, El, Pixels, ScrollSpec, SpringPair, Transition, Tx, col, el, row, text};
use rune_kit::Kit;
use thehideout_api::{Message, ReactionEmoji};
use thehideout_design::components::chat::emoji::glyph_label;
use thehideout_design::components::{Button, ChatMessage, DateDivider, DeliveryState, EmptyState, MessageGroup, NewMessagesDivider, ReactionChip, ReactionGlyph, Size, Variant};
use thehideout_design::icons::{self, CHEVRON_DOWN, CHAT, PLUS};
use thehideout_design::{metrics_of, motion, palette_text, palette_wash, radius, theme_of, type_};

use crate::app::App;
use crate::format::{clock_label, day_label, unix};
use crate::emoji::PickerTarget;
use crate::state::{AppState, Delivery, DialogKind, Fetch, Marker, Menu as MenuState, Place, chat_id_of};

/// A row of the transcript: the view model plus the server id (outgoing messages have none).
struct Item {
    msg: ChatMessage,
    id: Option<u64>,
}

fn items(s: &AppState, channel: u64) -> Vec<Item> {
    let me = s.my_id();
    let mut out = Vec::new();
    if let Some(log) = s.log(channel) {
        for m in &log.messages {
            let at = unix(m.created_at);
            out.push(Item {
                id: Some(m.id),
                msg: ChatMessage {
                    id: chat_id_of(m, me),
                    author_id: m.author_id.to_string(),
                    author_name: s.user_name(m.author_id),
                    at,
                    time_label: clock_label(s.tz, at),
                    body: m.content.clone(),
                    delivery: DeliveryState::Sent,
                    mentions_me: m.author_id != me && s.mentions_me(&m.content),
                    reactions: chips(s, m),
                    edited: m.edited_at.map(|t| s.edited_tip(t)),
                    can_edit: s.can_edit_message(m),
                    can_delete: s.can_delete_message(m),
                },
            });
        }
    }
    for o in s.data.outgoing.iter().filter(|o| o.channel == channel) {
        let at = unix(o.at);
        out.push(Item {
            id: None,
            msg: ChatMessage {
                id: format!("n:{}", o.nonce),
                author_id: me.to_string(),
                author_name: s.user_name(me),
                at,
                time_label: clock_label(s.tz, at),
                body: o.content.clone(),
                delivery: if o.state == Delivery::Failed { DeliveryState::Failed } else { DeliveryState::Pending },
                mentions_me: false,
                reactions: Vec::new(),
                edited: None,
                can_edit: false,
                can_delete: false,
            },
        });
    }
    out
}

/// The chips under a message: each reaction's emoji, count, whether you are among them, and what
/// hovering says (the server lists counts, not names, so the tip names the emoji and your part).
fn chips(s: &AppState, m: &Message) -> Vec<ReactionChip> {
    m.reactions
        .iter()
        .map(|r| {
            let (glyph, label) = match &r.emoji {
                ReactionEmoji::Unicode(g) => (ReactionGlyph::Unicode(g.clone()), glyph_label(g)),
                ReactionEmoji::Custom(id) => {
                    let name = m.guild_id.and_then(|g| s.emoji(g, *id)).map_or_else(|| "emoji".to_owned(), |e| e.name.clone());
                    (ReactionGlyph::Custom { id: id.to_string(), name: name.clone() }, format!(":{name}:"))
                }
            };
            let who = match (r.me, r.count) {
                (true, 1) => "you".to_owned(),
                (true, n) => format!("you and {} other{}", n - 1, if n == 2 { "" } else { "s" }),
                (false, 1) => "1 person".to_owned(),
                (false, n) => format!("{n} people"),
            };
            ReactionChip { key: r.emoji.key(), glyph, count: r.count, me: r.me, tip: format!("{label} \u{b7} reacted by {who}") }
        })
        .collect()
}

/// What the transcript draws from people rather than messages: their pictures, the colour of
/// their names, and the chip colours of an `@mention` of someone with a coloured role.
struct Looks {
    avatars: Rc<HashMap<String, Pixels>>,
    names: Rc<HashMap<String, Color>>,
    mentions: Rc<HashMap<String, (Color, Color)>>,
}

impl Looks {
    fn of(a: &App, k: &Kit, list: &[Item]) -> Looks {
        let t = theme_of(k);
        let s = &a.state;
        let guild = match s.ui.place {
            Place::Guild(g) => Some(g),
            Place::Home => None,
        };
        let mut avatars = HashMap::new();
        let mut names = HashMap::new();
        for it in list {
            let id = &it.msg.author_id;
            if avatars.contains_key(id) || names.contains_key(id) {
                continue;
            }
            let Ok(user) = id.parse::<u64>() else { continue };
            if let Some(p) = s.avatar_pixels(user) {
                avatars.insert(id.clone(), p);
            }
            if let Some(tone) = guild.and_then(|g| palette_text(s.name_color(g, user).as_key(), t)) {
                names.insert(id.clone(), tone.color());
            }
        }
        let mut mentions = HashMap::new();
        if let Some(g) = guild {
            for id in s.data.members.get(&g).into_iter().flatten() {
                let key = s.name_color(g, *id);
                if let (Some(ink), Some(wash)) = (palette_text(key.as_key(), t), palette_wash(key.as_key(), t)) {
                    mentions.insert(s.user_name(*id).to_lowercase(), (ink.color(), wash.color()));
                }
            }
        }
        Looks { avatars: Rc::new(avatars), names: Rc::new(names), mentions: Rc::new(mentions) }
    }

    fn apply(&self, g: MessageGroup) -> MessageGroup {
        let (avatars, names, mentions) = (self.avatars.clone(), self.names.clone(), self.mentions.clone());
        g.avatars(move |id| avatars.get(id).cloned())
            .name_colors(move |id| names.get(id).copied())
            .mention_ink(move |name| mentions.get(&name.to_lowercase()).copied())
    }
}

/// What hangs under each message with attachments or embeds, by row id, and under each pending
/// message's files.
fn extras(a: &App, k: &Kit, channel: u64) -> HashMap<String, Vec<El>> {
    let s = &a.state;
    let me = s.my_id();
    let mut out = HashMap::new();
    if let Some(log) = s.log(channel) {
        for m in log.messages.iter().filter(|m| !m.attachments.is_empty() || !m.embeds.is_empty()) {
            out.insert(chat_id_of(m, me), super::attach::extras(a, k, m));
        }
    }
    for o in s.data.outgoing.iter().filter(|o| o.channel == channel && !o.files.is_empty()) {
        out.insert(format!("n:{}", o.nonce), super::attach::outgoing_extras(a, k, o));
    }
    out
}

/// Index of the first message the "New" divider sits above.
fn marker_index(items: &[Item], marker: Marker, channel: u64) -> Option<usize> {
    let Marker::After { channel: c, id } = marker else { return None };
    if c != channel {
        return None;
    }
    items.iter().position(|i| matches!(i.id, Some(x) if id.is_none_or(|m| x > m)))
}

pub fn build(a: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = theme_of(k);
    let s = &a.state;
    let stage = match s.current_channel() {
        Some(c) => channel_stage(a, k, layer, c),
        None => placeholder_stage(a, k),
    };
    let mut area = col().key("transcript-area").grow(1.0).min_h(0.0).w_full().fill(t.surface.color()).child(stage);
    area = drop_zone(area);
    let pill_shown = !s.ui.at_bottom && s.ui.unseen_below > 0;
    if pill_shown
        && let Some(c) = s.current_channel()
    {
        area = area.child(pill(k, c));
    }
    if let Some(hint) = super::attach::drop_hint(a, k) {
        area = area.child(hint);
    }
    if let Some(toast) = super::toasts::over_transcript(a, k, pill_shown) {
        area = area.child(toast);
    }
    area
}

/// Files dropped anywhere on the transcript go to the composer's tray.
pub fn drop_zone(e: El) -> El {
    e.on_drop_files(|a: &mut App, files, _tx| a.act(|s| s.attach_paths(files)))
        .on_drop_hover(|a: &mut App, over, _tx| a.state.ui.attach.drop_over = over)
}

fn fade() -> Transition {
    Transition::opacity().springs(SpringPair::new(motion::CONTROL, motion::CONTROL))
}

fn centered(e: El) -> El {
    col().w_full().h_full().center().child(e)
}

/// Nothing is open: Home with no conversation chosen, or a server with no channels.
fn placeholder_stage(a: &App, k: &Kit) -> El {
    let s = &a.state;
    let body = match s.ui.place {
        Place::Home => EmptyState::new(&CHAT, "Your conversations", "Pick one from the list, or start a new message.")
            .action(Button::new("New message").icon(&PLUS).on_press(|a: &mut App| a.act(|s| s.open_dialog(DialogKind::NewDm))).build(k))
            .build(k),
        Place::Guild(g) => {
            let mut e = EmptyState::new(&icons::HASH, "No channels yet", "Channels keep conversations in one place.");
            if s.guild(g).is_some_and(|x| x.owner_id == s.my_id()) {
                e = e.action(Button::new("Create channel").icon(&PLUS).on_press(move |a: &mut App| a.act(|s| s.open_dialog(DialogKind::CreateChannel { guild: g }))).build(k));
            }
            e.build(k)
        }
    };
    el().key("stage-none").absolute(0.0, 0.0).w_full().h_full().transition(fade()).child(centered(body))
}

fn channel_stage(a: &App, k: &Kit, layer: &mut Vec<El>, channel: u64) -> El {
    let s = &a.state;
    let stage = el().key(format!("stage-{channel}")).absolute(0.0, 0.0).w_full().h_full().transition(fade());
    let Some(log) = s.log(channel) else { return stage };
    if !log.loaded {
        return stage.child(match log.fetch {
            Fetch::Initial { shown: true, .. } => skeleton(k),
            Fetch::Idle => centered(
                EmptyState::new(&icons::WARNING, "Couldn't load messages", "Check your connection and try again.")
                    .action(Button::new("Try again").variant(Variant::Secondary).on_press(|a: &mut App| a.act(|s| s.reload_current())).build(k))
                    .build(k),
            ),
            _ => el(),
        });
    }
    let list = items(s, channel);
    if list.is_empty() {
        let name = s.current_channel_name().unwrap_or_default();
        let (title, body) = match s.ui.place {
            Place::Home => (format!("This is the start of your chat with {name}"), "Say hello.".to_owned()),
            Place::Guild(_) => ("No messages yet".to_owned(), format!("Be the first to say something in #{name}.")),
        };
        return stage.child(centered(EmptyState::new(&CHAT, title, body).build(k)));
    }
    stage.child(viewport(a, k, layer, channel, &list))
}

fn viewport(a: &App, k: &Kit, layer: &mut Vec<El>, channel: u64, list: &[Item]) -> El {
    let t = theme_of(k);
    let s = &a.state;
    let log = s.log(channel);
    let mut c = col().key("messages").w_full().pb(8.0);

    if let Some(log) = log {
        if log.has_more {
            // Keyed by the oldest id: after a page lands this is a new element, so it reports
            // again if it is still on screen and the next page is wanted.
            let loading = log.fetch == Fetch::Older;
            let mut top = row().key(format!("older-{}", log.oldest_id.unwrap_or(0))).w_full().h(48.0).center().on_visible(move |a: &mut App, on, _tx| {
                if on {
                    a.act(|s| s.load_older(channel));
                }
            });
            if loading {
                top = top.child(text("Loading earlier messages\u{2026}").font(type_::at(k, type_::CAPTION)).color(t.text_muted.color()));
            }
            c = c.child(top);
        } else {
            let name = s.current_channel_name().unwrap_or_default();
            let title = match s.ui.place {
                Place::Home => format!("This is the start of your chat with {name}"),
                Place::Guild(_) => format!("Welcome to #{name}"),
            };
            c = c.child(col().key("start").w_full().px(16.0).pt(24.0).pb(4.0).gap(2.0).child(text(title).font(type_::at(k, type_::TITLE)).color(t.text.color())).child(text("This is where it begins.").font(type_::at(k, type_::CONTROL)).color(t.text_muted.color())));
        }
    }

    let looks = Looks::of(a, k, list);
    let menus = MessageMenus::of(a, k, channel);
    let editing = editing_chat(a, channel);
    let under = Rc::new(RefCell::new(extras(a, k, channel)));
    let emojis = s.emoji_set();
    let marker_at = marker_index(list, s.ui.marker, channel);
    let mut start = 0;
    let mut prev_day: Option<i64> = None;
    for i in 1..=list.len() {
        let day_here = list.get(i).map(|it| s.day_of(it.msg.at));
        let page_edge = i < list.len() && list[i].id.is_some_and(|id| log.is_some_and(|l| l.breaks.contains(&id)));
        let boundary = i == list.len() || day_here != Some(s.day_of(list[i - 1].msg.at)) || Some(i) == marker_at || page_edge;
        if !boundary {
            continue;
        }
        let first = &list[start];
        let day = s.day_of(first.msg.at);
        if prev_day != Some(day) {
            c = c.child(DateDivider::new(day_label(day, s.today)).build(k));
            prev_day = Some(day);
        }
        if Some(start) == marker_at {
            c = c.child(NewMessagesDivider::build(k));
        }
        let msgs: Vec<ChatMessage> = list[start..i].iter().map(|it| it.msg.clone()).collect();
        for run in MessageGroup::runs(&msgs) {
            c = c.child(
                looks
                    .apply(MessageGroup::new(run))
                    .links::<App>()
                    .emojis(emojis.clone())
                    .extras({
                        let under = under.clone();
                        move |id| under.borrow_mut().remove(id).unwrap_or_default()
                    })
                    .on_react(|a: &mut App, id: &str, key: &str| {
                        if let Some(emoji) = ReactionEmoji::from_key(key) {
                            a.act(|s| {
                                if let Some((channel, message)) = s.message_ref(id) {
                                    s.toggle_reaction(channel, message, emoji);
                                }
                            });
                        }
                    })
                    .on_add_reaction(|a: &mut App, id: &str, anchor: &str| {
                        a.act(|s| {
                            if let Some((channel, message)) = s.message_ref(id) {
                                s.open_picker(PickerTarget::Reaction { channel, message }, anchor);
                            }
                        })
                    })
                    .on_retry(|a: &mut App, id: &str| {
                        if let Some(nonce) = id.strip_prefix("n:") {
                            let nonce = nonce.to_owned();
                            a.act(|s| s.retry(&nonce));
                        }
                    })
                    .on_copy(|a: &mut App, id: &str| a.act(|s| s.copy_message(id)))
                    .on_reply(|a: &mut App, id: &str| a.act(|s| s.reply_to(id)))
                    .on_edit(|a: &mut App, id: &str| a.act(|s| {
                        s.begin_edit_chat(id);
                    }))
                    .on_delete(|a: &mut App, id: &str, shift: bool| a.act(|s| s.delete_pressed_chat(id, shift)))
                    .on_context(|a: &mut App, id: &str, p| {
                        if let Some((channel, message)) = a.state.message_ref(id) {
                            a.state.ui.menu = MenuState::Message { channel, message, at: Some((p.x as i32, p.y as i32)) };
                        }
                    })
                    .on_more(|a: &mut App, id: &str| {
                        if let Some((channel, message)) = a.state.message_ref(id) {
                            let open = matches!(a.state.ui.menu, MenuState::Message { message: m, at: None, .. } if m == message);
                            a.state.ui.menu = if open { MenuState::None } else { MenuState::Message { channel, message, at: None } };
                        }
                    })
                    .wrap_more(menus.wrap())
                    .force_hover(menus.open_chat.clone().unwrap_or_default())
                    .editor(editor(a, k, editing.clone()))
                    .build(k),
            );
        }
        start = i;
    }

    let scroller = col()
        .key(format!("tx-{channel}"))
        .scroll(ScrollSpec { stick_bottom: true })
        .scroll_anchor()
        .on_at_bottom_if(|a: &mut App, at| {
            let ui = &mut a.state.ui;
            if ui.at_bottom == at {
                return false;
            }
            ui.at_bottom = at;
            if at {
                ui.unseen_below = 0;
            }
            true
        })
        .child(c);
    menus.finish(a, k, layer);
    // Hairlines under the header and over the composer, only while messages are cut off there.
    col().w_full().h_full().child(s.ui.edges.transcript.wrap(k, scroller))
}

/// Grey bars where messages will be. Shown only for loads slower than the placeholder delay.
fn skeleton(k: &Kit) -> El {
    let t = theme_of(k);
    let bar = |w: f32| el().w(w).h(10.0).radius(radius::SM).fill(t.surface_raised.color());
    let mut c = col().key("loading").w_full().h_full().justify_end().px(16.0).pb(16.0).gap(20.0);
    for (i, (a, b)) in [(120.0, 340.0), (90.0, 260.0), (140.0, 400.0), (100.0, 220.0), (110.0, 300.0)].into_iter().enumerate() {
        c = c.child(
            row()
                .key(format!("skeleton-{i}"))
                .gap(12.0)
                .items_start()
                .child(el().size(32.0, 32.0).radius(radius::MD).fill(t.surface_raised.color()))
                .child(col().gap(8.0).pt(4.0).child(bar(a)).child(bar(b))),
        );
    }
    c.a11y_hidden()
}

/// "New messages" at the bottom centre; pressing it glides to the end.
fn pill(k: &Kit, channel: u64) -> El {
    let enter = Transition::offset(0.0, 6.0).combined(Transition::opacity()).springs(SpringPair::new(motion::CONTROL, motion::CONTROL));
    let button = Button::new("New messages").icon(&CHEVRON_DOWN).size(Size::Sm).build(k).on_press_tx(move |_: &mut App, tx| tx.scroll_to_bottom(format!("tx-{channel}")));
    row().key("new-pill").pin(Some(0.0), None, Some(0.0), Some(12.0)).justify_center().transition(enter).child(k.elevate(button, &k.elevation.floating))
}

// ---- editing and the message menu ----------------------------------------------------------------

/// The transcript id of the message being edited in `channel`.
fn editing_chat(a: &App, channel: u64) -> Option<String> {
    let s = &a.state;
    let e = s.editing_here().filter(|e| e.channel == channel)?;
    let me = s.my_id();
    s.log(channel)?.messages.iter().find(|m| m.id == e.message).map(|m| chat_id_of(m, me))
}

fn edit_input(a: &mut App) -> &mut rune::TextEdit {
    match a.state.ui.editing.as_mut() {
        Some(e) => &mut e.input,
        None => &mut a.state.ui.idle_composer,
    }
}

/// The editor that replaces the body of the message being edited: the text in a field, and what
/// the keys do. It shows only for that message's transcript id.
fn editor(a: &App, k: &Kit, chat_id: Option<String>) -> impl Fn(&str) -> Option<El> + 'static {
    let t = theme_of(k);
    let built = chat_id.zip(a.state.editing_here()).map(|(id, e)| {
        let style = metrics_of(k).font(type_::BODY);
        let field = rune::text_input(&e.input, style)
            .key("edit-input")
            .w_full()
            .color(t.text.color())
            .caret(t.accent_text.color(), t.accent_subtle.color())
            .label("Edit message")
            .on_edit(edit_input)
            .on_submit(|a: &mut App, _tx| a.act(|s| s.save_edit()))
            .on_input_key(|a: &mut App, ev: &rune::KeyEvent, _tx: &mut Tx| {
                if ev.key == rune::Key::Escape {
                    a.act(|s| s.cancel_edit());
                    return true;
                }
                false
            });
        let m = metrics_of(k);
        let pad = (m.field_h() - style.line_px()) / 2.0;
        let card = col()
            .key("edit-card")
            .w_full()
            .px(12.0)
            .py(pad)
            .radius(radius::MD)
            .fill(t.surface_raised.color())
            .border(1.0, t.control_border.color())
            .outline(1.0, -1.0, t.focus.color())
            .child(rune::auto_grow(field, 1, 8).role("none"));
        let hint = text("Esc to cancel \u{b7} Enter to save")
            .key("edit-hint")
            .font(type_::at(k, type_::CAPTION))
            .color(t.text_faint.color());
        (id, std::cell::RefCell::new(Some(col().key("editor").w_full().gap(4.0).child(card).child(hint))))
    });
    move |id: &str| match &built {
        Some((want, cell)) if want == id => cell.borrow_mut().take(),
        _ => None,
    }
}

/// The menu of one message, when it is open: a context menu where the click landed, or one hung
/// on the hover bar's More button.
struct MessageMenus {
    /// The transcript id of the message whose menu is open.
    open_chat: Option<String>,
    more: Rc<RefCell<Option<rune_kit::Menu>>>,
    point: Option<rune_kit::Menu>,
    layer: Rc<RefCell<Vec<El>>>,
    kit: Kit,
    open_message: Option<u64>,
}

impl MessageMenus {
    fn of(a: &App, k: &Kit, channel: u64) -> MessageMenus {
        let s = &a.state;
        let me = s.my_id();
        let (mut open_chat, mut open_message, mut more, mut point) = (None, None, None, None);
        if let MenuState::Message { channel: c, message, at } = s.ui.menu
            && c == channel
            && let Some(m) = s.log(channel).and_then(|l| l.messages.iter().find(|m| m.id == message))
        {
            let chat = chat_id_of(m, me);
            open_chat = Some(chat.clone());
            open_message = Some(message);
            let menu = message_items(a, rune_kit::Menu::new("menu-message"), channel, message, chat);
            match at {
                Some((x, y)) => point = Some(menu.at(x as f32, y as f32).open(true)),
                None => more = Some(menu),
            }
        }
        MessageMenus {
            open_chat,
            more: Rc::new(RefCell::new(more)),
            point,
            layer: Rc::new(RefCell::new(Vec::new())),
            kit: k.clone(),
            open_message,
        }
    }

    /// Hangs the open menu on the More button of its message.
    fn wrap(&self) -> impl Fn(El, &str) -> El + 'static {
        let (open, more, layer, kit) = (self.open_chat.clone(), self.more.clone(), self.layer.clone(), self.kit.clone());
        move |button: El, id: &str| {
            if open.as_deref() != Some(id) {
                return button;
            }
            match more.borrow_mut().take() {
                Some(menu) => menu
                    .trigger(button)
                    .open(true)
                    .on_close(|a: &mut App| a.state.ui.menu = MenuState::None)
                    .min_width(180.0)
                    .build(&kit, &mut layer.borrow_mut())
                    .role("none"),
                None => button,
            }
        }
    }

    fn finish(self, _a: &App, k: &Kit, layer: &mut Vec<El>) {
        if let Some(menu) = self.point {
            // A context menu has no trigger of its own; the point is its anchor.
            let _ = self.open_message;
            menu.trigger(el())
                .on_close(|a: &mut App| a.state.ui.menu = MenuState::None)
                .min_width(180.0)
                .build(k, layer);
        }
        layer.append(&mut self.layer.borrow_mut());
    }
}

/// Reply, Copy text, Edit, Delete.
fn message_items(a: &App, menu: rune_kit::Menu, channel: u64, message: u64, chat: String) -> rune_kit::Menu {
    use rune_kit::MenuItem;
    let s = &a.state;
    let Some(m) = s.log(channel).and_then(|l| l.messages.iter().find(|m| m.id == message)) else { return menu };
    let (id_reply, id_copy) = (chat.clone(), chat);
    let mut menu = menu
        .item(MenuItem::new("Reply").icon(&icons::REPLY).on_select(move |a: &mut App| {
            a.act(|s| {
                s.ui.menu = MenuState::None;
                s.reply_to(&id_reply)
            })
        }))
        .item(MenuItem::new("Copy text").icon(&icons::COPY).on_select(move |a: &mut App| {
            a.act(|s| {
                s.ui.menu = MenuState::None;
                s.copy_message(&id_copy)
            })
        }));
    let (can_edit, can_delete) = (s.can_edit_message(m), s.can_delete_message(m));
    if can_edit {
        menu = menu.item(MenuItem::new("Edit message").icon(&icons::PENCIL).on_select(move |a: &mut App| {
            a.act(|s| {
                s.begin_edit(channel, message);
            })
        }));
    }
    if can_delete {
        menu = menu.separator().item(
            MenuItem::new("Delete message")
                .icon(&icons::TRASH)
                .danger(true)
                .on_select_tx(move |a: &mut App, tx: &mut Tx| {
                    let shift = tx.mods().shift;
                    a.act(|s| s.delete_pressed(channel, message, shift))
                }),
        );
    }
    menu
}
