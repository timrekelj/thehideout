//! The `chat` page: the chat building blocks as a component sheet (forced states, one card each),
//! then the assembled preview in `chat_app`.

use rune::{El, TextEdit, col, row, text};
use rune_kit::{Kit, State};
use thehideout_design::components::chat::{
    ChannelHeader, ChannelRow, ChatMessage, Composer, ConnectionPill, ConnectionStatus,
    DateDivider, DeliveryState, Dialog, EmptyState, GuildBadge, MemberRow, MessageGroup, NavRow,
    NewMessagesDivider, ReactionChip, ReactionGlyph, SidebarSection, Toast, ToastKind,
};
use thehideout_design::components::{EmojiPicker, EmojiScope, EmojiSet, ServerEmoji};
use thehideout_design::components::{Button, IconButton, Presence, Size, TextInput, Variant};
use thehideout_design::icons::{self, CHAT, GEAR, HASH, SEARCH, USERS};
use thehideout_design::{radius, theme_of, type_};

use crate::G;
use crate::ui::*;

pub fn page(s: &G, k: &Kit, layer: &mut Vec<El>) -> El {
    let intro = page_title(
        k,
        "Chat",
        "Building blocks for the chat client. Hover, selected, pending and failed are forced for review; the live preview at the bottom is interactive.",
    );
    let left = col()
        .gap(20.0)
        .w(560.0)
        .child(section(k, "GuildBadge", "Rail on the left: dot = unread, short bar = hover, tall accent bar = selected. Count sits on the corner.", guild_badges(k)))
        .child(section(k, "NavRow, ChannelRow", "Selected moves between rows as one highlight (matched, motion-reflow). Every icon is fill; selected is the highlight, strong text and an accent icon.", rows(k)))
        .child(section(k, "SidebarSection", "Chevron turns on motion-disclosure; rows fade on the same spring. The + shows on hover.", sections(k)))
        .child(section(k, "ChannelHeader", "Title and topic; the breadcrumb variant for narrow layouts.", headers(k)))
        .child(section(k, "Composer", "Grows to 8 lines, then scrolls. Send is primary once there is text.", composers(k)))
        .child(section(k, "EmojiPicker", "Reused by the composer, reactions and the channel icon picker. Server emojis play on hover; Unicode draws in the system's colour emoji font.", emoji_picker(s, k)))
        .child(section(k, "MemberRow", "Presence dot, owner crown. Offline reads fainter.", members(k)));
    let right = col()
        .gap(20.0)
        .w(556.0)
        .child(section(k, "MessageGroup", "Hover shows the action bar and, on follow-ups, the time. A mention of you takes a faint wash and an accent bar.", transcript(k)))
        .child(section(k, "Markdown", "Message markup: bold, italic, strike, underline, code, fences, quotes, lists, headings, spoilers (click to reveal), links and mentions. Links open in the browser.", markdown(k)))
        .child(section(k, "Dividers", "Date and new-messages.", dividers(k)))
        .child(section(k, "ConnectionPill", "One element, colours move between states.", pills(k)))
        .child(section(k, "Toast", "Slides up 8pt and fades. The caller dismisses.", toasts(k)))
        .child(section(k, "EmptyState", "Icon tile, title, body, optional action.", empties(k)))
        .child(section(k, "Dialog", "Scrim fades, panel fades from 0.98 on motion-overlay. Esc and scrim click close it.", dialogs(s, k, layer)));
    col()
        .gap(24.0)
        .child(intro)
        .child(row().gap(20.0).items_start().child(left).child(right))
        .child(section(k, "Assembled preview", "Everything above, composed. Click channels, type and press Enter, collapse a section, open a dialog.", crate::chat_app::preview(s, k, layer)))
}

fn guild_badges(k: &Kit) -> El {
    let sizes = [24u32, 32, 40];
    let cols = ["Rest", "Hover", "Selected", "Unread", "Mentions"];
    let names = ["24", "32", "40"];
    grid(k, 36.0, 92.0, &names, &cols, |r, c| {
        let b = GuildBadge::new("guild-hideout", "The Hideout").size(sizes[r]);
        match c {
            1 => b.state(State::Hover),
            2 => b.selected(true),
            3 => b.unread(true),
            4 => b.mentions(if r == 2 { 128 } else { 4 }),
            _ => b,
        }
        .build(k)
    })
}

fn rows(k: &Kit) -> El {
    let t = theme_of(k);
    let nav = col()
        .gap(2.0)
        .w(220.0)
        .child(muted(k, "Nav rows"))
        .child(
            NavRow::new("Home", &icons::HOUSE)
                .highlight_group("sheet-nav")
                .selected(true)
                .build(k),
        )
        .child(
            NavRow::new("Mentions", &icons::BELL)
                .highlight_group("sheet-nav")
                .mentions(3)
                .build(k),
        )
        .child(
            NavRow::new("Friends", &icons::USERS)
                .highlight_group("sheet-nav")
                .state(State::Hover)
                .build(k),
        )
        .child(
            NavRow::new("Settings", &GEAR)
                .highlight_group("sheet-nav")
                .muted(true)
                .build(k),
        );
    let chans = col()
        .gap(2.0)
        .w(220.0)
        .child(muted(k, "Channel rows"))
        .child(
            ChannelRow::new("general")
                .highlight_group("sheet-ch")
                .selected(true)
                .build(k),
        )
        .child(
            ChannelRow::new("random")
                .highlight_group("sheet-ch")
                .unread(true)
                .build(k),
        )
        .child(
            ChannelRow::new("dev")
                .highlight_group("sheet-ch")
                .unread(true)
                .mentions(12)
                .build(k),
        )
        .child(
            ChannelRow::new("memes")
                .highlight_group("sheet-ch")
                .state(State::Hover)
                .build(k),
        )
        .child(
            ChannelRow::new("archive")
                .highlight_group("sheet-ch")
                .muted(true)
                .build(k),
        );
    row()
        .gap(24.0)
        .padding(8.0)
        .radius(radius::MD)
        .fill(t.bg.color())
        .child(nav)
        .child(chans)
}

fn sections(k: &Kit) -> El {
    let t = theme_of(k);
    let open = SidebarSection::new("Text channels", true)
        .add_label("Create channel")
        .show_add(true)
        .child(
            ChannelRow::new("general")
                .highlight_group("sheet-sec")
                .selected(true)
                .build(k),
        )
        .child(
            ChannelRow::new("random")
                .highlight_group("sheet-sec")
                .unread(true)
                .build(k),
        )
        .build(k);
    let closed = SidebarSection::new("Voice", false)
        .add_label("Create channel")
        .build(k);
    row()
        .gap(24.0)
        .items_start()
        .padding(8.0)
        .radius(radius::MD)
        .fill(t.bg.color())
        .child(col().w(220.0).child(open))
        .child(col().w(220.0).child(closed))
}

fn headers(k: &Kit) -> El {
    let actions = |k: &Kit| {
        (
            IconButton::new(&USERS, "Show members").build(k),
            IconButton::new(&SEARCH, "Search").build(k),
        )
    };
    let (a, b) = actions(k);
    let full = ChannelHeader::new("general")
        .topic("Announcements, plans and the occasional gif")
        .action(a)
        .action(b)
        .build(k);
    let (a, b) = actions(k);
    let crumb = ChannelHeader::breadcrumb("The Hideout", "general")
        .action(a)
        .action(b)
        .build(k);
    col().gap(8.0).child(pane(k, full)).child(pane(k, crumb))
}

fn pane(k: &Kit, e: El) -> El {
    col()
        .w_full()
        .radius(radius::MD)
        .fill(theme_of(k).bg.color())
        .border(1.0, theme_of(k).border_subtle.color())
        .child(e)
}

fn composers(k: &Kit) -> El {
    let t = theme_of(k);
    let long = TextEdit::new(
        "Line one of a longer draft\nLine two, with `code` and an @ana mention\nLine three",
    );
    let n = std::cell::Cell::new(0);
    let c = |e: &TextEdit, f: &dyn Fn(Composer) -> Composer| {
        n.set(n.get() + 1);
        f(Composer::new(e)
            .key(format!("sheet-composer-{}", n.get()))
            .placeholder("Message #general"))
        .build(k)
    };
    col()
        .gap(10.0)
        .padding(12.0)
        .radius(radius::MD)
        .fill(t.bg.color())
        .child(muted(k, "Empty: send is ghost and disabled"))
        .child(c(&TextEdit::new(""), &|c| c))
        .child(muted(k, "With text: send is primary"))
        .child(c(
            &TextEdit::new("Shipping the reconnect fix today"),
            &|c| c,
        ))
        .child(muted(k, "Multi-line"))
        .child(c(&long, &|c| c))
        .child(muted(k, "Disabled with a reason"))
        .child(c(&TextEdit::new(""), &|c| {
            c.disabled_reason("Reconnecting\u{2026}")
        }))
}

fn members(k: &Kit) -> El {
    let t = theme_of(k);
    col()
        .w(260.0)
        .gap(2.0)
        .padding(8.0)
        .radius(radius::MD)
        .fill(t.bg.color())
        .child(
            MemberRow::new("user-tim", "Tim Rekelj")
                .owner(true)
                .build(k),
        )
        .child(MemberRow::new("user-ana", "Ana Kovac").build(k))
        .child(
            MemberRow::new("user-bruno", "Bruno")
                .presence(Presence::Idle)
                .state(State::Hover)
                .build(k),
        )
        .child(
            MemberRow::new("user-cvetka", "Cvetka Z")
                .presence(Presence::Offline)
                .build(k),
        )
}

pub fn sample(id: &str, who: (&str, &str), at: i64, label: &str, body: &str) -> ChatMessage {
    ChatMessage {
        id: id.to_owned(),
        author_id: who.0.to_owned(),
        author_name: who.1.to_owned(),
        at,
        time_label: label.to_owned(),
        body: body.to_owned(),
        delivery: DeliveryState::Sent,
        mentions_me: false,
        reactions: Vec::new(),
        edited: None,
        can_edit: false,
        can_delete: false,
    }
}

/// A reaction chip of a Unicode emoji, for the sheet and the demo.
pub fn chip(glyph: &str, count: u32, me: bool) -> ReactionChip {
    ReactionChip {
        key: format!("u:{glyph}"),
        glyph: ReactionGlyph::Unicode(glyph.to_owned()),
        count,
        me,
        tip: format!("{glyph} \u{b7} reacted by {count}"),
    }
}

/// The emoji picker as a sheet: this server's emojis are named placeholders here (pictures come
/// from the app's image cache), the Unicode set is the real one.
fn emoji_picker(s: &G, k: &Kit) -> El {
    let t = theme_of(k);
    let server = vec![
        ServerEmoji { id: 1, name: "smug_cat".into(), animated: false },
        ServerEmoji { id: 2, name: "party_parrot".into(), animated: true },
        ServerEmoji { id: 3, name: "shipit".into(), animated: false },
    ];
    let recents = vec!["u:\u{1f44d}".to_owned(), "c:2".to_owned(), "u:\u{1f389}".to_owned()];
    let body = EmojiPicker::new("sheet-emoji", &s.emoji, EmojiScope::All, &server, &recents, EmojiSet::none()).build::<G>(
        k,
        |g: &mut G| &mut g.emoji,
        |g: &mut G, pick, _tx| {
            g.chat.draft.insert(&pick.text());
        },
        |_: &mut G| {},
    );
    col()
        .gap(10.0)
        .padding(12.0)
        .radius(radius::MD)
        .fill(t.bg.color())
        .child(muted(k, "Search, sections, recents, server and Unicode. Arrows move, Enter picks."))
        .child(row().justify_center().child(rune_kit::popover::raised(k, body, k.radius.md)))
}

fn transcript(k: &Kit) -> El {
    let t = theme_of(k);
    const ANA: (&str, &str) = ("user-ana", "Ana Kovac");
    const BRUNO: (&str, &str) = ("user-bruno", "Bruno");
    const TIM: (&str, &str) = ("user-tim", "Tim Rekelj");
    let mut g1 = vec![
        sample(
            "s1",
            ANA,
            0,
            "09:03",
            "Gateway looks fine. It backs off to `5s`, then `30s`, and resets on a clean connect.",
        ),
        sample(
            "s2",
            ANA,
            30,
            "09:03",
            "Here is the loop, for reference:\n```rust\nlet delay = (base << attempt).min(30_000);\nsleep(delay);\n```",
        ),
        sample(
            "s3",
            ANA,
            60,
            "09:04",
            "Hovered follow-up: its time shows in the gutter and the action bar floats above it.",
        ),
    ];
    g1.truncate(3);
    g1[0].reactions = vec![chip("\u{1f44d}", 3, true), chip("\u{1f389}", 2, false), chip("\u{1f440}", 1, false)];
    g1[2].body = "Hovered follow-up: its time shows in the gutter and the action bar floats above it. \u{1f642}".into();
    let jumbo = sample("s7", BRUNO, 2100, "09:42", "\u{1f389}\u{1f680}\u{2728}");
    let mut mention = sample(
        "s4",
        BRUNO,
        1000,
        "09:20",
        "@Tim can you check the gateway side before lunch?",
    );
    mention.mentions_me = true;
    let mut pending = sample("s5", TIM, 2000, "09:40", "Merging now, will report back.");
    pending.delivery = DeliveryState::Pending;
    let mut failed = sample("s6", TIM, 2030, "09:41", "Also bumping the client crate.");
    failed.delivery = DeliveryState::Failed;
    let g3 = vec![pending, failed];
    col()
        .w_full()
        .py(8.0)
        .radius(radius::MD)
        .fill(t.bg.color())
        .border(1.0, t.border_subtle.color())
        .child(MessageGroup::new(&g1).force_hover("s3").links::<G>().build(k))
        .child(MessageGroup::new(&[mention]).links::<G>().build(k))
        .child(MessageGroup::new(&g3).links::<G>().build(k))
        .child(MessageGroup::new(&[jumbo]).links::<G>().build(k))
}

/// Every construct of the message markup, as messages by one author.
fn markdown(k: &Kit) -> El {
    let t = theme_of(k);
    const ANA: (&str, &str) = ("user-ana", "Ana Kovac");
    const BRUNO: (&str, &str) = ("user-bruno", "Bruno");
    let bodies = [
        "**bold**, *italic*, _italic_, ***both***, ~~strike~~, __underline__, `inline code` and ||a hidden spoiler||. Escape with \\*stars\\*.",
        "# Heading 1\n## Heading 2\n### Heading 3\nHeadings only at the start of a line.",
        "> a block quote with **bold** and a mention for @Tim\n> that continues here",
        "- first item\n- second with `code`\n  - nested item\n  - another\n1. numbered\n2. in order",
        "```rust\nfn backoff(attempt: u32) -> u64 {\n    (5_000u64 << attempt).min(30_000)\n}\n```",
        "Read [the docs](https://docs.rs/rune) or https://www.rust-lang.org/learn. Unsafe [links](javascript:alert(1)) stay text. Custom emoji <:blob:123456> render as text for now.",
    ];
    let mut msgs: Vec<ChatMessage> = bodies
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let who = if i == 2 { BRUNO } else { ANA };
            sample(&format!("md{i}"), who, 10_000 + i as i64 * 20, "10:12", b)
        })
        .collect();
    msgs[2].mentions_me = true;
    let mut c = col()
        .w_full()
        .py(8.0)
        .radius(radius::MD)
        .fill(t.bg.color())
        .border(1.0, t.border_subtle.color());
    for run in MessageGroup::runs(&msgs) {
        c = c.child(MessageGroup::new(run).links::<G>().build(k));
    }
    c
}

fn dividers(k: &Kit) -> El {
    let t = theme_of(k);
    col()
        .w_full()
        .radius(radius::MD)
        .fill(t.bg.color())
        .border(1.0, t.border_subtle.color())
        .child(DateDivider::new("Today").build(k))
        .child(DateDivider::new("Yesterday").build(k))
        .child(DateDivider::new("Mon, Nov 17").build(k))
        .child(NewMessagesDivider::build(k))
        .child(row().h(8.0))
}

fn pills(k: &Kit) -> El {
    row()
        .gap(10.0)
        .flex_wrap()
        .child(ConnectionPill::new(ConnectionStatus::Connected).build(k))
        .child(ConnectionPill::new(ConnectionStatus::Connecting).build(k))
        .child(ConnectionPill::new(ConnectionStatus::Reconnecting(5)).build(k))
        .child(ConnectionPill::new(ConnectionStatus::Offline).build(k))
}

fn toasts(k: &Kit) -> El {
    col()
        .gap(10.0)
        .items_start()
        .child(Toast::new(ToastKind::Info, "Invite link copied").build(k))
        .child(Toast::new(ToastKind::Success, "Server created").build(k))
        .child(Toast::new(ToastKind::Danger, "Couldn't join: the invite has expired").build(k))
}

fn empties(k: &Kit) -> El {
    let t = theme_of(k);
    let card = |e: El| {
        col()
            .w_full()
            .py(24.0)
            .items_center()
            .radius(radius::MD)
            .fill(t.bg.color())
            .border(1.0, t.border_subtle.color())
            .child(e)
    };
    col()
        .gap(12.0)
        .child(card(EmptyState::new(&CHAT, "No messages yet", "Say hello in #general. Messages you send here are visible to everyone in the channel.").build(k)))
        .child(card(
            EmptyState::new(&HASH, "Join or create a server", "Servers are where your conversations live. Join one with an invite, or start your own.")
                .action(Button::new("Create a server").build(k))
                .build(k),
        ))
}

fn dialogs(s: &G, k: &Kit, layer: &mut Vec<El>) -> El {
    let t = theme_of(k);
    let body = col()
        .w_full()
        .gap(6.0)
        .child(muted(k, "Server name"))
        .child(
            TextInput::new(&s.edits[5])
                .placeholder("The Hideout")
                .label("Server name")
                .state(State::Rest)
                .build(k),
        );
    let preview = Dialog::new("preview", "Create server")
        .description("Give your server a name. You can change it later.")
        .child(body)
        .footer(Button::new("Cancel").variant(Variant::Secondary).build(k))
        .footer(Button::new("Create").build(k))
        .on_close(|_: &mut G| {})
        .preview(k);
    let open = Button::new("Open live dialog")
        .variant(Variant::Secondary)
        .size(Size::Sm)
        .on_press(|g: &mut G| g.chat.dialog = true)
        .build(k)
        .key("open-dialog");
    s.chat.dialog_layer(k, layer);
    col()
        .gap(12.0)
        .child(
            col()
                .w_full()
                .padding(16.0)
                .radius(radius::MD)
                .fill(t.bg.color())
                .border(1.0, t.border_subtle.color())
                .child(preview),
        )
        .child(open)
        .child(
            text("The live dialog is modal: Tab stays inside, Esc closes.".to_owned())
                .font(type_::CAPTION)
                .color(t.text_muted.color()),
        )
}
