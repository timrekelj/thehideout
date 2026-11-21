//! The site-admin dashboard: a full-window page like the settings (nav on the left: Overview,
//! Servers, Files, Users, Invites, Audit log), with the page on the inset panel. Every page is a header,
//! optional controls and a body that scrolls under the scroll-edge hairlines; the lists are
//! virtual rows that ask for their next page as the end comes into view. The overview is stat
//! cards, a storage bar split by kind and the activity chart. Cards and rows are told apart by
//! fills and spacing, not borders.

use rune::{El, GrowthSpec, ScrollSpec, TextEdit, TextStyle, col, el, row, text};
use rune_kit::{Kit, Select};
use thehideout_api::{
    AdminFileKind, AdminFileRow, AdminGuildRow, AdminUserRow, AuditEntry, InviteStatus,
    StorageBreakdown,
};
use thehideout_design::metrics_of;
use thehideout_design::components::avatar::picture;
use thehideout_design::components::util::frame_over;
use thehideout_design::components::{
    ActivityChart, Avatar, Badge, BadgeVariant, BarSegment, Button, Checkbox, Dialog, EmptyState,
    IconButton, Segmented, Size, StackedBar, StatCard, TextInput, Variant, dialog_field,
    group_digits,
};
use thehideout_design::icons::{self, SEARCH, SHIELD, TRASH, USERS};
use thehideout_design::{palette_color, radius, space, theme_of, type_};

use super::server_settings::tile;
use super::settings::{header, nav_shell};
use crate::account::invite_status_label;
use crate::admin::{
    AdminPage, AdminState, Confirm, FILE_KINDS, INVITE_STATUSES, RANGES, file_title, has_thumbnail,
    kind_label, storage_parts,
};
use crate::app::App;
use crate::format::{bytes, clock_label, day_label, day_of, unix};
use crate::state::AppState;

/// Space between the window edge and the panel, as on the other full-window pages.
const INSET: f32 = 8.0;
/// Side padding of the page's header, controls and rows.
fn pad(k: &Kit) -> f32 {
    metrics_of(k).pad(32.0)
}
const MAX_W: f32 = 1040.0;
/// Extra room before a left-aligned column that follows a right-aligned one (a figure's right edge
/// would otherwise sit one 12pt gap from the next column's text).
const COL_GAP: f32 = 16.0;
fn row_h(k: &Kit) -> f32 {
    metrics_of(k).table_row(52.0)
}
fn file_row_h(k: &Kit) -> f32 {
    metrics_of(k).table_row(56.0)
}

const FIGURES: TextStyle = TextStyle {
    features: "'tnum' 1",
    ..type_::CONTROL
};
const FIGURES_STRONG: TextStyle = TextStyle {
    features: "'tnum' 1",
    ..type_::CONTROL_STRONG
};

fn close(a: &mut App) {
    a.act(|s| s.close_admin());
}

/// `compact`: a narrow window drops the secondary columns.
pub fn build(a: &App, k: &Kit, layer: &mut Vec<El>, bar: f32, compact: bool) -> El {
    let Some(ad) = a.state.ui.admin.as_ref() else {
        return el();
    };
    let t = theme_of(k);
    let page = ad.page;
    let title = match page {
        AdminPage::Overview => "Overview",
        AdminPage::Servers => "Servers",
        AdminPage::Files => "Files",
        AdminPage::Users => "Users",
        AdminPage::Invites => "Invites",
        AdminPage::Audit => "Audit log",
    };
    let parts = if ad.unavailable {
        Parts::scroll(unavailable(k))
    } else {
        match page {
            AdminPage::Overview => Parts::scroll(overview(a, k, ad)),
            AdminPage::Servers => servers(k, ad, compact),
            AdminPage::Files => files(k, layer, ad, compact),
            AdminPage::Users => users(k, ad, compact),
            AdminPage::Invites => invites(a, k, layer, ad, compact),
            AdminPage::Audit => audit(k, ad, compact),
        }
    };
    let mut top = col()
        .w_full()
        .px(pad(k))
        .child(header(k, title, close).key("admin-header"));
    if let Some(controls) = parts.controls {
        top = top.child(col().w_full().pb(12.0).child(controls));
    }
    if let Some(head) = parts.head {
        top = top.child(head);
    }
    let panel = col()
        .key("admin-panel")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .fill(t.surface.color())
        .radius(radius::LG)
        .clip_rounded(radius::LG)
        .child(top)
        .child(
            a.state.ui.edges.admin.wrap(
                k,
                col()
                    .key(format!("admin-scroll-{page:?}"))
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(parts.body),
            ),
        )
        .child(frame_over(1.0, t.border_subtle.color(), radius::LG));
    let body = row()
        .key("admin-body")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .pt(INSET)
        .pb(INSET)
        .child(nav(k, page))
        .child(col().grow(1.0).min_w(0.0).h_full().pr(INSET).child(panel));
    confirm(k, layer, ad);
    col()
        .key("admin")
        .w_full()
        .h_full()
        .fill(t.bg.color())
        .pt(bar)
        .child(body)
}

/// What a page hands the frame: controls above the table, the table head, the scrolling body.
struct Parts {
    controls: Option<El>,
    head: Option<El>,
    body: El,
}

impl Parts {
    fn scroll(body: El) -> Parts {
        Parts {
            controls: None,
            head: None,
            body,
        }
    }
}

fn nav(k: &Kit, page: AdminPage) -> El {
    let mut rows = Vec::new();
    for (p, label, glyph) in [
        (AdminPage::Overview, "Overview", &icons::CHART_BAR),
        (AdminPage::Servers, "Servers", &icons::HARD_DRIVES),
        (AdminPage::Files, "Files", &icons::FILES),
        (AdminPage::Users, "Users", &USERS),
        (AdminPage::Invites, "Invites", &icons::ENVELOPE_SIMPLE),
        (AdminPage::Audit, "Audit log", &icons::CLIPBOARD_TEXT),
    ] {
        rows.push(
            thehideout_design::components::NavRow::new(label, glyph)
                .selected(page == p)
                .highlight_group("admin-nav")
                .on_press(move |a: &mut App| a.act(|s| s.admin_page(p)))
                .build(k)
                .key(format!("admin-nav-{label}")),
        );
    }
    nav_shell(k, "Admin", rows)
}

// ---- shared pieces ---------------------------------------------------------------------------

fn card(k: &Kit) -> El {
    col()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .radius(radius::LG)
        .fill(theme_of(k).bg.color())
}

fn muted(k: &Kit, s: impl Into<String>) -> El {
    text(s.into())
        .font(type_::at(k, type_::CONTROL))
        .color(theme_of(k).text_muted.color())
}

/// A right-aligned figure in a column `w` wide.
fn num(k: &Kit, s: String, w: f32) -> El {
    row().w(w).shrink().justify_end().child(
        text(s)
            .font(FIGURES)
            .color(theme_of(k).text.color())
            .max_lines(1),
    )
}

fn head_cell(k: &Kit, label: &str, w: Option<f32>, right: bool) -> El {
    let t = theme_of(k);
    let cell = match w {
        Some(w) => row().w(w).shrink(),
        None => row().grow(1.0).shrink().min_w(0.0),
    };
    let cell = if right { cell.justify_end() } else { cell };
    cell.child(
        text(label.to_owned())
            .font(type_::at(k, type_::LABEL))
            .color(t.text_muted.color())
            .max_lines(1),
    )
}

fn table_head(cells: Vec<El>) -> El {
    row()
        .key("admin-table-head")
        .w_full()
        .h(32.0)
        .gap(12.0)
        .center_y()
        .children(cells)
}

/// A centred message in the empty body of a list.
fn blank(k: &Kit, glyph: &'static rune::Icon, title: &str, body: &str) -> El {
    col()
        .w_full()
        .py(48.0)
        .items_center()
        .child(EmptyState::new(glyph, title, body).build(k))
}

fn loading(k: &Kit) -> El {
    col()
        .key("admin-loading")
        .w_full()
        .py(48.0)
        .items_center()
        .child(muted(k, "Loading\u{2026}"))
}

/// The line under a list: loading more, or the failure with its retry.
fn list_foot(k: &Kit, pending: bool, error: &Option<String>) -> Option<El> {
    let t = theme_of(k);
    if let Some(e) = error {
        return Some(
            row()
                .key("admin-list-error")
                .w_full()
                .py(16.0)
                .gap(12.0)
                .center()
                .child(text(e.clone()).font(type_::at(k, type_::CONTROL)).color(t.danger.color()))
                .child(
                    Button::new("Try again")
                        .variant(Variant::Secondary)
                        .size(Size::Sm)
                        .on_press(|a: &mut App| a.act(|s| s.retry_admin()))
                        .build(k)
                        .key("admin-retry"),
                ),
        );
    }
    pending.then(|| {
        row()
            .key("admin-list-more")
            .w_full()
            .py(16.0)
            .center()
            .child(muted(k, "Loading more\u{2026}"))
    })
}

fn unavailable(k: &Kit) -> El {
    col().key("admin-unavailable").w_full().py(64.0).items_center().child(
        EmptyState::new(&icons::PLUGS, "Admin service not available", "This server doesn't offer the admin dashboard yet. Update the server, then try again.")
            .action(Button::new("Try again").variant(Variant::Secondary).on_press(|a: &mut App| a.act(|s| s.retry_admin())).build(k).key("admin-retry"))
            .build(k),
    )
}

/// The search field above a list: its text is the query, changes restart the list.
fn search_field(
    k: &Kit,
    page: AdminPage,
    edit_state: &TextEdit,
    placeholder: &str,
    edit: fn(&mut App) -> &mut TextEdit,
) -> El {
    TextInput::new(edit_state)
        .placeholder(placeholder)
        .label(placeholder)
        .leading_icon(&SEARCH)
        .width(Some(320.0))
        .on_edit(edit)
        .on_change(move |a: &mut App, _| a.act(|s| s.admin_search_changed(page)))
        .build(k)
        .key(format!("admin-search-{page:?}").to_lowercase())
}

fn server_search_edit(a: &mut App) -> &mut TextEdit {
    match a.state.ui.admin.as_mut() {
        Some(ad) => &mut ad.server_search,
        None => &mut a.state.ui.idle_composer,
    }
}

fn user_search_edit(a: &mut App) -> &mut TextEdit {
    match a.state.ui.admin.as_mut() {
        Some(ad) => &mut ad.user_search,
        None => &mut a.state.ui.idle_composer,
    }
}

/// A virtual list of `count` rows of about `est` points, each built from the state by index. Asks
/// for the next page when the last rows are close.
fn rows(
    key: &str,
    count: usize,
    est: f32,
    kit: &Kit,
    row_key: impl Fn(&AppState, usize) -> String + 'static,
    build: impl Fn(&App, &Kit, usize) -> El + 'static,
) -> El {
    let kit = kit.clone();
    col()
        .key(key.to_owned())
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            count,
            est,
            move |s: &App, i| row_key(&s.state, i).into(),
            move |s: &App, i| build(s, &kit, i),
        )
        .on_visible_rows(move |a: &mut App, range| {
            if range.end + 8 < count {
                return false;
            }
            let mut queued = false;
            a.act(|s| queued = s.admin_more());
            queued
        })
}

// ---- overview --------------------------------------------------------------------------------

fn overview(a: &App, k: &Kit, ad: &AdminState) -> El {
    let t = theme_of(k);
    let o = ad.overview.as_ref();
    let stat = |key: &str, label: &str, v: Option<u64>| {
        StatCard::new(format!("stat-{key}"), label, v)
            .build(k)
            .min_w(190.0)
    };
    // One wrapping row: four across when wide, two when narrow, never a clipped figure.
    let cards = row()
        .key("admin-stats")
        .w_full()
        .flex_wrap()
        .gap(12.0)
        .child(stat("users", "Users", o.map(|o| o.users)))
        .child(stat("servers", "Servers", o.map(|o| o.guilds)))
        .child(stat("channels", "Channels", o.map(|o| o.channels)))
        .child(stat("messages", "Messages", o.map(|o| o.messages)))
        .child(stat(
            "live",
            "Live connections",
            o.map(|o| o.live_connections),
        ))
        .child(stat(
            "messages-today",
            "Messages today",
            o.map(|o| o.messages_today),
        ))
        .child(stat(
            "signups-today",
            "Sign-ups today",
            o.map(|o| o.signups_today),
        ))
        .child(stat(
            "active-today",
            "Active users today",
            o.map(|o| o.active_users_today),
        ));

    let mut page = col()
        .key("page-admin-overview")
        .w_full()
        .max_w(MAX_W)
        .px(pad(k))
        .pb(pad(k))
        .gap(12.0)
        .child(cards)
        .child(storage_card(k, o.map(|o| &o.storage)))
        .child(activity_card(a, k, ad));
    if let Some(e) = &ad.overview_error {
        page = page.child(
            row()
                .key("overview-error")
                .gap(12.0)
                .center_y()
                .child(text(e.clone()).font(type_::at(k, type_::CONTROL)).color(t.danger.color()))
                .child(
                    Button::new("Try again")
                        .variant(Variant::Secondary)
                        .size(Size::Sm)
                        .on_press(|a: &mut App| a.act(|s| s.retry_admin()))
                        .build(k)
                        .key("admin-retry"),
                ),
        );
    }
    col().w_full().items_center().child(page)
}

/// The palette key each storage kind is drawn in.
fn kind_color(key: &str) -> &'static str {
    match key {
        "attachments" => "blue",
        "emojis" => "amber",
        "icons" => "teal",
        "avatars" => "pink",
        _ => "indigo",
    }
}

fn storage_card(k: &Kit, s: Option<&StorageBreakdown>) -> El {
    let t = theme_of(k);
    let parts = s.map(storage_parts);
    let total = s.map_or(0, StorageBreakdown::total);
    let segments: Vec<BarSegment> = parts
        .iter()
        .flatten()
        .map(|&(key, label, n)| BarSegment {
            key: key.to_owned(),
            label: format!("{label}: {}", bytes(n)),
            weight: n,
            color: palette_color(kind_color(key), t)
                .unwrap_or(t.text_muted)
                .color(),
        })
        .collect();
    let mut legend = row().key("storage-legend").w_full().flex_wrap().gap(20.0);
    for (key, label, n) in parts.into_iter().flatten() {
        let tone = palette_color(kind_color(key), t).unwrap_or(t.text_muted);
        legend = legend.child(
            row()
                .key(format!("legend-{key}"))
                .role("group")
                .label(format!("{label}, {}", bytes(n)))
                .gap(8.0)
                .center_y()
                .child(
                    el().size(8.0, 8.0)
                        .radius(radius::FULL)
                        .fill(tone.color())
                        .a11y_hidden(),
                )
                .child(
                    text(label.to_owned())
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text_muted.color())
                        .a11y_hidden(),
                )
                .child(
                    text(bytes(n))
                        .font(FIGURES_STRONG)
                        .color(t.text.color())
                        .a11y_hidden(),
                ),
        );
    }
    card(k)
        .key("storage-card")
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    text("Storage")
                        .font(type_::at(k, type_::TITLE))
                        .color(t.text.color())
                        .grow(1.0),
                )
                .child(
                    text(if s.is_some() {
                        bytes(total)
                    } else {
                        "\u{2013}".to_owned()
                    })
                    .key("storage-total")
                    .font(TextStyle {
                        size: 20.0,
                        line_height: 28.0,
                        weight: 650.0,
                        features: "'tnum' 1",
                        ..type_::HEADING
                    })
                    .color(t.text.color()),
                ),
        )
        .child(
            StackedBar::new("storage-bar", segments)
                .height(12.0)
                .build(k),
        )
        .child(legend)
}

fn activity_card(a: &App, k: &Kit, ad: &AdminState) -> El {
    let t = theme_of(k);
    let days = a.state.chart_days();
    let selected = RANGES.iter().position(|d| *d == ad.range).unwrap_or(1);
    let range = col().key("admin-range").w(240.0).child(
        Segmented::new(&RANGES.map(|d| format!("{d} days")), selected)
            .label("Range")
            .on_pick(|a: &mut App, i| a.act(|s| s.set_admin_range(RANGES[i])))
            .build(k),
    );
    let body = if days.is_empty() {
        let msg = if ad.activity_pending.is_some() || ad.activity.is_none() {
            "Loading\u{2026}"
        } else {
            "No activity in this range."
        };
        col()
            .key("activity-empty")
            .w_full()
            .h(200.0)
            .center()
            .child(muted(k, msg))
    } else {
        ActivityChart::new("activity", days)
            .hover(ad.hover_day)
            .on_hover(|a: &mut App, day: usize, over: bool| a.act(|s| s.hover_admin_day(day, over)))
            .build(k)
    };
    let legend = row()
        .gap(16.0)
        .center_y()
        .child(key_dot(k, "Messages", t.accent.color()))
        .child(key_dot(k, "Active users", t.info.color()));
    card(k)
        .key("activity-card")
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(12.0)
                .center_y()
                .child(text("Activity").font(type_::at(k, type_::TITLE)).color(t.text.color()))
                .child(legend)
                .child(el().grow(1.0))
                .child(range),
        )
        .child(body)
}

fn key_dot(k: &Kit, label: &str, color: rune::Color) -> El {
    row()
        .gap(6.0)
        .center_y()
        .child(
            el().size(8.0, 8.0)
                .radius(radius::FULL)
                .fill(color)
                .a11y_hidden(),
        )
        .child(muted(k, label.to_owned()))
}

// ---- servers ---------------------------------------------------------------------------------

const SERVER_NUM: f32 = 84.0;
const SERVER_STORAGE: f32 = 200.0;

fn servers(k: &Kit, ad: &AdminState, compact: bool) -> Parts {
    let feed = &ad.servers;
    let mut head = vec![
        head_cell(k, "Server", None, false),
        head_cell(k, "Members", Some(SERVER_NUM), true),
    ];
    if !compact {
        head.push(head_cell(k, "Channels", Some(SERVER_NUM), true));
        head.push(head_cell(k, "Messages", Some(96.0), true));
    }
    head.push(head_cell(k, "Storage", Some(SERVER_STORAGE), true));
    let body = if !feed.loaded && feed.error.is_none() {
        loading(k)
    } else if feed.items.is_empty() && feed.error.is_none() {
        blank(
            k,
            &SHIELD,
            "No servers found",
            "Nothing matches that search.",
        )
    } else {
        let list = rows(
            "admin-server-rows",
            feed.items.len(),
            row_h(k),
            k,
            |s, i| {
                s.admin().map_or_else(String::new, |ad| {
                    format!("server-row-{}", ad.servers.items[i].guild.id)
                })
            },
            move |a, k, i| server_row(a, k, i, compact),
        );
        col()
            .w_full()
            .max_w(MAX_W)
            .px(pad(k) - 8.0)
            .pb(pad(k))
            .child(list)
            .children(list_foot(k, feed.pending, &feed.error))
    };
    Parts {
        controls: Some(search_field(
            k,
            AdminPage::Servers,
            &ad.server_search,
            "Search servers",
            server_search_edit,
        )),
        head: Some(table_head(head)),
        body: col().w_full().items_center().child(body),
    }
}

fn server_row(a: &App, k: &Kit, i: usize, compact: bool) -> El {
    let t = theme_of(k);
    let Some(r) = a.state.admin().and_then(|ad| ad.servers.items.get(i)) else {
        return el().key(format!("server-gone-{i}")).h(row_h(k));
    };
    let max = a
        .state
        .admin()
        .map_or(1, |ad| {
            ad.servers
                .items
                .iter()
                .map(|r| r.storage_bytes)
                .max()
                .unwrap_or(1)
        })
        .max(1);
    let guild = r.guild.id;
    let pixels = r
        .guild
        .icon
        .as_deref()
        .and_then(|key| a.state.data.media.pixels(key));
    let name = col().grow(1.0).shrink().min_w(0.0).child(
        text(r.guild.name.clone())
            .font(type_::at(k, type_::CONTROL_STRONG))
            .color(t.text.color())
            .max_lines(1),
    );
    let mut line = row()
        .key(format!("server-row-{guild}"))
        .role("button")
        .label(format!(
            "{}, {} members, {} stored. Show its files",
            r.guild.name,
            r.member_count,
            bytes(r.storage_bytes)
        ))
        .w_full()
        .h(row_h(k))
        .px(8.0)
        .gap(12.0)
        .center_y()
        .radius(radius::MD)
        .focusable(true)
        .focus_ring(space::RING_WIDTH, space::RING_INSET, k.c.border_focus)
        .hover(t.hover.color())
        .press(t.pressed.color())
        .on_press(move |a: &mut App| a.act(|s| s.admin_files_of(guild)))
        .child(
            row()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(12.0)
                .center_y()
                .child(tile(
                    k,
                    &guild.to_string(),
                    &r.guild.name,
                    pixels,
                    32.0,
                    radius::MD,
                ))
                .child(name),
        )
        .child(num(k, r.member_count.to_string(), SERVER_NUM));
    if !compact {
        line = line
            .child(num(k, r.channel_count.to_string(), SERVER_NUM))
            .child(num(k, group_digits(r.message_count), 96.0));
    }
    line.child(storage_cell(k, r, max))
}

/// The bar (this server's share of the biggest one, split by kind) and its size; the tooltip
/// spells the kinds out.
fn storage_cell(k: &Kit, r: &AdminGuildRow, max: u64) -> El {
    let t = theme_of(k);
    let width = 96.0;
    let filled = (r.storage_bytes as f32 / max as f32 * width).max(if r.storage_bytes > 0 {
        2.0
    } else {
        0.0
    });
    let mut inner = row()
        .key(format!("server-bar-{}", r.guild.id))
        .w(filled)
        .h_full()
        .gap(1.0)
        .clip_rounded(radius::XS);
    for (key, _, n) in storage_parts(&r.storage) {
        if n > 0 {
            inner = inner.child(
                el().grow(n as f32).shrink().h_full().fill(
                    palette_color(kind_color(key), t)
                        .unwrap_or(t.text_muted)
                        .color(),
                ),
            );
        }
    }
    let tip = storage_parts(&r.storage)
        .iter()
        .filter(|p| p.2 > 0)
        .map(|(_, label, n)| format!("{label}  {}", bytes(*n)))
        .collect::<Vec<_>>()
        .join("\n");
    row()
        .key(format!("server-storage-{}", r.guild.id))
        .w(SERVER_STORAGE)
        .shrink()
        .gap(10.0)
        .center_y()
        .justify_end()
        .help(if tip.is_empty() {
            "Nothing stored".to_owned()
        } else {
            tip
        })
        .child(
            row()
                .w(width)
                .h(6.0)
                .radius(radius::XS)
                .fill(t.hover.color())
                .child(inner),
        )
        .child(
            row().w(68.0).justify_end().child(
                text(bytes(r.storage_bytes))
                    .font(FIGURES)
                    .color(t.text.color())
                    .max_lines(1),
            ),
        )
}

// ---- files -----------------------------------------------------------------------------------

const FILE_KIND_W: f32 = 104.0;
const FILE_SERVER_W: f32 = 140.0;
const FILE_OWNER_W: f32 = 100.0;
const FILE_SIZE_W: f32 = 76.0;
const FILE_DATE_W: f32 = 108.0;
const FILE_ACT_W: f32 = 64.0;
const CHECK_W: f32 = 16.0;

fn files(k: &Kit, layer: &mut Vec<El>, ad: &AdminState, compact: bool) -> Parts {
    let feed = &ad.files;

    let mut guild_labels = vec!["All servers".to_owned()];
    guild_labels.extend(ad.known_guilds.iter().map(|(_, n)| n.clone()));
    let current_guild = ad
        .file_guild
        .and_then(|g| ad.guild_name(g))
        .unwrap_or("All servers")
        .to_owned();
    let ids: Vec<(u64, String)> = ad.known_guilds.clone();
    let guild_select = Select::new("admin-guild-select", &guild_labels)
        .width(220.0)
        .label("Server")
        .current(Some(current_guild.as_str()))
        .open(ad.guild_menu)
        .on_toggle(|a: &mut App| {
            if let Some(ad) = a.state.ui.admin.as_mut() {
                ad.guild_menu = !ad.guild_menu;
                ad.kind_menu = false;
            }
        })
        .on_close(|a: &mut App| {
            if let Some(ad) = a.state.ui.admin.as_mut() {
                ad.guild_menu = false;
            }
        })
        .on_choose(move |a: &mut App, label: &str, _tx| {
            let chosen = ids.iter().find(|(_, n)| n == label).map(|(id, _)| *id);
            a.act(|s| s.filter_files_guild(chosen));
        })
        .build(k, layer);

    let mut kind_labels = vec!["All kinds".to_owned()];
    kind_labels.extend(FILE_KINDS.iter().map(|k| kind_label(*k).to_owned()));
    let current_kind = ad.file_kind.map_or("All kinds", kind_label).to_owned();
    let kind_select = Select::new("admin-kind-select", &kind_labels)
        .width(160.0)
        .label("Kind")
        .current(Some(current_kind.as_str()))
        .open(ad.kind_menu)
        .on_toggle(|a: &mut App| {
            if let Some(ad) = a.state.ui.admin.as_mut() {
                ad.kind_menu = !ad.kind_menu;
                ad.guild_menu = false;
            }
        })
        .on_close(|a: &mut App| {
            if let Some(ad) = a.state.ui.admin.as_mut() {
                ad.kind_menu = false;
            }
        })
        .on_choose(|a: &mut App, label: &str, _tx| {
            let chosen = FILE_KINDS.iter().copied().find(|k| kind_label(*k) == label);
            a.act(|s| s.filter_files_kind(chosen));
        })
        .build(k, layer);

    let mut controls = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(col().key("admin-guild-filter").child(guild_select))
        .child(col().key("admin-kind-filter").child(kind_select))
        .child(el().grow(1.0));
    if let Some((done, total)) = ad.progress() {
        controls = controls.child(
            muted(
                k,
                format!("Deleting {} of {total}\u{2026}", (done + 1).min(total)),
            )
            .key("admin-delete-progress"),
        );
    }
    if !ad.selected.is_empty() {
        let keys: Vec<String> = feed
            .items
            .iter()
            .filter(|r| ad.selected.contains(&r.key))
            .map(|r| r.key.clone())
            .collect();
        controls = controls.child(
            Button::new(format!("Delete {}", ad.selected.len()))
                .variant(Variant::Danger)
                .icon(&TRASH)
                .size(Size::Sm)
                .on_press(move |a: &mut App| {
                    let keys = keys.clone();
                    a.act(|s| s.ask_delete_files(keys))
                })
                .build(k)
                .key("admin-delete-selected"),
        );
    }

    let all = !feed.items.is_empty() && feed.items.iter().all(|r| ad.selected.contains(&r.key));
    let some = !ad.selected.is_empty() && !all;
    let mut head = vec![
        row().w(CHECK_W).child(
            Checkbox::new(all)
                .mixed(some)
                .label("Select all")
                .hide_label()
                .on_toggle(|a: &mut App| a.act(|s| s.toggle_all_files()))
                .build(k)
                .key("admin-select-all"),
        ),
        head_cell(k, "File", None, false),
        head_cell(k, "Kind", Some(FILE_KIND_W), false),
    ];
    if !compact {
        head.push(head_cell(k, "Server", Some(FILE_SERVER_W), false));
        head.push(head_cell(k, "Owner", Some(FILE_OWNER_W), false));
    }
    head.push(head_cell(k, "Size", Some(FILE_SIZE_W), true));
    if !compact {
        head.push(head_cell(k, "Added", Some(FILE_DATE_W), false).ml(COL_GAP));
    }
    head.push(row().w(FILE_ACT_W).shrink());

    let body = if !feed.loaded && feed.error.is_none() {
        loading(k)
    } else if feed.items.is_empty() && feed.error.is_none() {
        blank(
            k,
            &icons::FILES,
            "No files",
            if ad.file_guild.is_some() || ad.file_kind.is_some() {
                "Nothing stored matches these filters."
            } else {
                "Nothing is stored yet."
            },
        )
    } else {
        let list = rows(
            "admin-file-rows",
            feed.items.len(),
            file_row_h(k),
            k,
            |s, i| {
                s.admin().map_or_else(String::new, |ad| {
                    format!("file-row-{}", ad.files.items[i].key)
                })
            },
            move |a, k, i| file_row(a, k, i, compact),
        );
        col()
            .w_full()
            .max_w(MAX_W)
            .px(pad(k) - 8.0)
            .pb(pad(k))
            .child(list)
            .children(list_foot(k, feed.pending, &feed.error))
    };
    Parts {
        controls: Some(controls),
        head: Some(table_head(head)),
        body: col().w_full().items_center().child(body),
    }
}

/// The glyph of a file's type, for rows without a picture.
fn type_icon(r: &AdminFileRow) -> &'static rune::Icon {
    let name = match (r.kind, r.content_type.as_str()) {
        (AdminFileKind::Attachment, c) if c.starts_with("video/") => "file-video",
        (AdminFileKind::Attachment, c) if c.starts_with("audio/") => "file-audio",
        (AdminFileKind::Attachment, c) if c.starts_with("image/") => "image",
        (AdminFileKind::Attachment, "application/pdf") => "file-pdf",
        (AdminFileKind::Attachment, c)
            if c.contains("zip") || c.contains("gzip") || c.contains("7z") =>
        {
            "file-zip"
        }
        (AdminFileKind::Attachment, c) if c.starts_with("text/") => "file-text",
        (AdminFileKind::Attachment, _) => "file",
        (AdminFileKind::Emoji, _) => "smiley",
        (AdminFileKind::GuildIcon, _) => "hard-drives",
        (AdminFileKind::Avatar, _) => "user-circle",
        (AdminFileKind::EmbedThumbnail, _) => "link",
        (AdminFileKind::Unknown, _) => "file",
    };
    icons::ALL.iter().copied().find(|i| i.name == name).unwrap_or(&icons::IMAGE)
}

fn preview(a: &App, k: &Kit, r: &AdminFileRow) -> El {
    let t = theme_of(k);
    let mut tile = row()
        .key(format!("file-thumb-{}", r.key))
        .size(36.0, 36.0)
        .center()
        .radius(radius::MD)
        .fill(t.bg.color())
        .child(
            rune::icon(type_icon(r), 18.0)
                .color(t.text_muted.color())
                .a11y_hidden(),
        );
    if has_thumbnail(r)
        && let Some(p) = a.state.data.media.pixels(&r.key)
    {
        tile = tile.child(picture(&p, 36.0, radius::MD));
    }
    tile
}

fn file_row(a: &App, k: &Kit, i: usize, compact: bool) -> El {
    let t = theme_of(k);
    let Some(ad) = a.state.admin() else {
        return el().key(format!("file-gone-{i}")).h(file_row_h(k));
    };
    let Some(r) = ad.files.items.get(i) else {
        return el().key(format!("file-gone-{i}")).h(file_row_h(k));
    };
    let key = r.key.clone();
    let selected = ad.selected.contains(&key);
    let title = file_title(r);
    let when = day_label(day_of(a.state.tz, unix(r.created_at)), a.state.today);
    let owner = r
        .owner
        .as_ref()
        .map_or("\u{2013}".to_owned(), |u| u.username.clone());
    let server = r
        .guild
        .as_ref()
        .map_or("\u{2013}".to_owned(), |g| g.name.clone());
    let pick = key.clone();
    let open = key.clone();
    let del = key.clone();
    let mut line = row()
        .key(format!("file-row-{key}"))
        .role("row")
        .label(format!(
            "{title}, {}, {}",
            kind_label(r.kind),
            bytes(r.bytes)
        ))
        .focus_selected(selected)
        .w_full()
        .h(file_row_h(k))
        .px(8.0)
        .gap(12.0)
        .center_y()
        .radius(radius::MD)
        .child(
            row().w(CHECK_W).child(
                Checkbox::new(selected)
                    .label(format!("Select {title}"))
                    .hide_label()
                    .on_toggle(move |a: &mut App| a.act(|s| s.toggle_file_selected(&pick)))
                    .build(k)
                    .key(format!("file-check-{key}")),
            ),
        )
        .child(
            row()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(12.0)
                .center_y()
                .child(preview(a, k, r))
                .child(
                    col()
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0)
                        .child(
                            text(title.clone())
                                .font(type_::at(k, type_::CONTROL_STRONG))
                                .color(t.text.color())
                                .max_lines(1)
                                .help_when_truncated(title.clone()),
                        )
                        .child(
                            text(r.content_type.clone())
                                .font(type_::at(k, type_::CAPTION))
                                .color(t.text_faint.color())
                                .max_lines(1),
                        ),
                ),
        )
        .child(
            row().w(FILE_KIND_W).shrink().child(
                text(kind_label(r.kind).to_owned())
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.text_muted.color())
                    .max_lines(1),
            ),
        );
    line = if selected {
        line.fill(t.accent_subtle.color())
    } else {
        line.hover(t.hover.color())
    };
    if !compact {
        line = line
            .child(
                row().w(FILE_SERVER_W).shrink().child(
                    text(server)
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text_muted.color())
                        .max_lines(1),
                ),
            )
            .child(
                row().w(FILE_OWNER_W).shrink().child(
                    text(owner)
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text_muted.color())
                        .max_lines(1),
                ),
            );
    }
    line = line.child(num(k, bytes(r.bytes), FILE_SIZE_W));
    if !compact {
        line = line.child(
            row().w(FILE_DATE_W).shrink().ml(COL_GAP).child(
                text(when)
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.text_muted.color())
                    .max_lines(1),
            ),
        );
    }
    line.child(
        row()
            .w(FILE_ACT_W)
            .shrink()
            .gap(2.0)
            .justify_end()
            .child(
                IconButton::new(
                    &icons::ARROW_SQUARE_OUT,
                    format!("Open {title}"),
                )
                .size(Size::Sm)
                .build(k)
                .on_press_tx(move |a: &mut App, tx| {
                    let url = a.state.media_url(&open);
                    let _ = tx.open_url(&url);
                })
                .key(format!("file-open-{key}")),
            )
            .child(
                IconButton::new(&TRASH, format!("Delete {title}"))
                    .size(Size::Sm)
                    .on_press(move |a: &mut App| {
                        let keys = vec![del.clone()];
                        a.act(|s| s.ask_delete_files(keys))
                    })
                    .build(k)
                    .key(format!("file-delete-{key}")),
            ),
    )
}

// ---- users -----------------------------------------------------------------------------------

const USER_ADMIN_W: f32 = 64.0;
const USER_NUM_W: f32 = 80.0;
const USER_LAST_W: f32 = 120.0;

fn users(k: &Kit, ad: &AdminState, compact: bool) -> Parts {
    let feed = &ad.users;
    let mut head = vec![
        head_cell(k, "User", None, false),
        head_cell(k, "Admin", Some(USER_ADMIN_W), true),
        head_cell(k, "Servers", Some(USER_NUM_W), true),
    ];
    if !compact {
        head.push(head_cell(k, "Messages", Some(USER_NUM_W), true));
    }
    head.push(head_cell(k, "Storage", Some(USER_NUM_W), true));
    if !compact {
        head.push(head_cell(k, "Last active", Some(USER_LAST_W), false).ml(COL_GAP));
    }
    let body = if !feed.loaded && feed.error.is_none() {
        loading(k)
    } else if feed.items.is_empty() && feed.error.is_none() {
        blank(k, &USERS, "No users found", "Nothing matches that search.")
    } else {
        let list = rows(
            "admin-user-rows",
            feed.items.len(),
            row_h(k),
            k,
            |s, i| {
                s.admin().map_or_else(String::new, |ad| {
                    format!("user-row-{}", ad.users.items[i].user.id)
                })
            },
            move |a, k, i| user_row(a, k, i, compact),
        );
        col()
            .w_full()
            .max_w(MAX_W)
            .px(pad(k) - 8.0)
            .pb(pad(k))
            .child(list)
            .children(list_foot(k, feed.pending, &feed.error))
    };
    Parts {
        controls: Some(search_field(
            k,
            AdminPage::Users,
            &ad.user_search,
            "Search users",
            user_search_edit,
        )),
        head: Some(table_head(head)),
        body: col().w_full().items_center().child(body),
    }
}

fn last_active(a: &App, r: &AdminUserRow) -> String {
    match r.last_active_at {
        None => "Never".to_owned(),
        Some(at) => {
            let day = day_of(a.state.tz, unix(at));
            if day == a.state.today {
                format!("Today, {}", clock_label(a.state.tz, unix(at)))
            } else {
                day_label(day, a.state.today)
            }
        }
    }
}

fn user_row(a: &App, k: &Kit, i: usize, compact: bool) -> El {
    let t = theme_of(k);
    let Some(ad) = a.state.admin() else {
        return el().key(format!("user-gone-{i}")).h(row_h(k));
    };
    let Some(r) = ad.users.items.get(i) else {
        return el().key(format!("user-gone-{i}")).h(row_h(k));
    };
    let id = r.user.id;
    let name = r.user.username.clone();
    let busy = ad.busy_users.contains(&id);
    let on = r.user.is_admin;
    let mut who = row().gap(8.0).center_y().child(
        text(name.clone())
            .font(type_::at(k, type_::CONTROL_STRONG))
            .color(t.text.color())
            .max_lines(1),
    );
    if on {
        who = who.child(
            Badge::new("Admin")
                .variant(BadgeVariant::Accent)
                .build(k)
                .key(format!("user-badge-{id}")),
        );
    }
    let mut labels = col().grow(1.0).shrink().min_w(0.0).gap(2.0).child(who);
    if let Some(e) = ad.user_errors.get(&id) {
        labels = labels.child(
            text(e.clone())
                .key(format!("user-error-{id}"))
                .font(type_::at(k, type_::CAPTION))
                .color(t.danger.color())
                .wrap()
                .w_full(),
        );
    }
    let toggle = thehideout_design::components::switch(on)
        .label(format!("{name} is an admin"))
        .hide_label()
        .disabled(busy)
        .on_toggle(move |a: &mut App| a.act(|s| s.ask_set_admin(id, !on)))
        .build(k)
        .key(format!("user-admin-{id}"));
    let mut line = row()
        .key(format!("user-row-{id}"))
        .role("row")
        .label(format!(
            "{name}, {} servers, {} stored",
            r.guild_count,
            bytes(r.storage_bytes)
        ))
        .w_full()
        .min_h(row_h(k))
        .px(8.0)
        .py(6.0)
        .gap(12.0)
        .center_y()
        .radius(radius::MD)
        .hover(t.hover.color())
        .child(
            row()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(12.0)
                .center_y()
                .child(
                    Avatar::new(id.to_string(), name.clone())
                        .size(32)
                        .image(
                            r.user
                                .avatar
                                .as_deref()
                                .and_then(|k| a.state.data.media.pixels(k)),
                        )
                        .backdrop(t.surface.color())
                        .build(k),
                )
                .child(labels),
        )
        .child(row().w(USER_ADMIN_W).shrink().justify_end().child(toggle))
        .child(num(k, r.guild_count.to_string(), USER_NUM_W));
    if !compact {
        line = line.child(num(k, group_digits(r.message_count), USER_NUM_W));
    }
    line = line.child(num(k, bytes(r.storage_bytes), USER_NUM_W));
    if !compact {
        line = line.child(
            row().w(USER_LAST_W).shrink().ml(COL_GAP).child(
                text(last_active(a, r))
                    .font(type_::at(k, type_::CONTROL))
                    .color(t.text_muted.color())
                    .max_lines(1),
            ),
        );
    }
    line
}

// ---- invites ---------------------------------------------------------------------------------

const INVITE_SERVERS_W: f32 = 180.0;
const INVITE_STATUS_W: f32 = 96.0;
const INVITE_BY_W: f32 = 110.0;
const INVITE_EXPIRES_W: f32 = 110.0;
const INVITE_ACTIONS_W: f32 = 168.0;

fn invite_email_edit(a: &mut App) -> &mut TextEdit {
    match a.state.ui.admin.as_mut().and_then(|ad| ad.invite_form.as_mut()) {
        Some(f) => &mut f.email,
        None => &mut a.state.ui.idle_composer,
    }
}

fn invites(a: &App, k: &Kit, layer: &mut Vec<El>, ad: &AdminState, compact: bool) -> Parts {
    let t = theme_of(k);
    let feed = &ad.invites;
    let labels: Vec<&str> = std::iter::once("All").chain(INVITE_STATUSES.iter().map(|s| invite_status_label(*s))).collect();
    let picked = ad.invite_filter.and_then(|f| INVITE_STATUSES.iter().position(|s| *s == f)).map_or(0, |i| i + 1);
    let filter = Segmented::new(&labels, picked)
        .label("Show invites")
        .on_pick(|a: &mut App, i| a.act(|s| s.filter_invites(if i == 0 { None } else { Some(INVITE_STATUSES[i - 1]) })))
        .build(k);
    let mut controls = col().w_full().gap(12.0).child(
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(col().key("admin-invite-filter").w(420.0).shrink().child(filter))
            .child(el().grow(1.0))
            .child(
                Button::new("Create invite")
                    .icon(&thehideout_design::icons::PLUS)
                    .on_press(|a: &mut App| a.act(|s| s.open_invite_form()))
                    .build(k)
                    .key("admin-invite-create"),
            ),
    );
    if let Some(created) = &ad.invite_created {
        let code = created.code.clone().unwrap_or_default();
        let copy = code.clone();
        let link = created.link.clone();
        let mut line = format!("Invite sent to {}. Code: {code}", created.email);
        if let Some(link) = &link {
            line.push_str(&format!(" \u{00b7} Link: {link}"));
        }
        let copy_link = link.map(|link| {
            Button::new("Copy link")
                .icon(&icons::COPY)
                .variant(Variant::Secondary)
                .size(Size::Sm)
                .build(k)
                .key("admin-invite-copy-link")
                .on_press_tx(move |a: &mut App, tx| {
                    tx.copy(link.clone());
                    a.state.toast(thehideout_design::components::ToastKind::Success, "Invite link copied");
                })
        });
        controls = controls.child(
            row()
                .key("admin-invite-created")
                .w_full()
                .gap(12.0)
                .px(12.0)
                .py(8.0)
                .center_y()
                .radius(radius::MD)
                .fill(t.success_bg.color())
                .child(
                    text(line)
                        .key("admin-invite-created-text")
                        .font(type_::at(k, type_::CONTROL))
                        .color(t.text.color())
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0)
                        .wrap(),
                )
                .children(copy_link)
                .child(
                    Button::new("Copy code")
                        .icon(&icons::COPY)
                        .variant(Variant::Secondary)
                        .size(Size::Sm)
                        .build(k)
                        .key("admin-invite-copy")
                        .on_press_tx(move |a: &mut App, tx| {
                            tx.copy(copy.clone());
                            a.state.toast(thehideout_design::components::ToastKind::Success, "Invite code copied");
                        }),
                )
                .child(
                    IconButton::new(&icons::X, "Dismiss")
                        .size(Size::Sm)
                        .on_press(|a: &mut App| a.act(|s| s.dismiss_invite_created()))
                        .build(k),
                ),
        );
    }
    let mut head = vec![head_cell(k, "Email", None, false)];
    if !compact {
        head.push(head_cell(k, "Servers", Some(INVITE_SERVERS_W), false));
    }
    head.push(head_cell(k, "Status", Some(INVITE_STATUS_W), false));
    if !compact {
        head.push(head_cell(k, "Invited by", Some(INVITE_BY_W), false));
    }
    head.push(head_cell(k, "Expires", Some(INVITE_EXPIRES_W), false));
    head.push(head_cell(k, "", Some(INVITE_ACTIONS_W), true));
    let shown = a.state.admin_invite_rows().len();
    let body = if !feed.loaded && feed.error.is_none() {
        loading(k)
    } else if shown == 0 && feed.error.is_none() {
        blank(
            k,
            &icons::ENVELOPE_SIMPLE,
            if feed.items.is_empty() { "No invites yet" } else { "No invites like that" },
            "Invites you and server members send show up here.",
        )
    } else {
        let list = rows(
            "admin-invite-rows",
            shown,
            row_h(k),
            k,
            |s, i| s.admin_invite_rows().get(i).map_or_else(String::new, |r| format!("invite-row-{}", r.id)),
            move |a, k, i| invite_row(a, k, i, compact),
        );
        col().w_full().max_w(MAX_W).px(pad(k) - 8.0).pb(pad(k)).child(list).children(list_foot(k, feed.pending, &feed.error))
    };
    invite_form(k, layer, ad);
    Parts { controls: Some(controls), head: Some(table_head(head)), body: col().w_full().items_center().child(body) }
}

fn invite_row(a: &App, k: &Kit, i: usize, compact: bool) -> El {
    let t = theme_of(k);
    let rows = a.state.admin_invite_rows();
    let (Some(r), Some(ad)) = (rows.get(i), a.state.admin()) else {
        return el().key(format!("invite-gone-{i}")).h(row_h(k));
    };
    let id = r.id;
    let busy = ad.invite_busy.contains(&id);
    let mut who = row().gap(8.0).center_y().child(text(r.email.clone()).font(type_::at(k, type_::CONTROL_STRONG)).color(t.text.color()).max_lines(1).shrink());
    if r.make_admin {
        who = who.child(Badge::new("Admin").variant(BadgeVariant::Accent).build(k));
    }
    let servers = r.guild_ids.iter().map(|g| ad.guild_name(*g).map_or_else(|| format!("server {g}"), str::to_owned)).collect::<Vec<_>>().join(", ");
    let status = match r.status {
        InviteStatus::Pending => BadgeVariant::Info,
        InviteStatus::Accepted => BadgeVariant::Success,
        InviteStatus::Expired => BadgeVariant::Warning,
        _ => BadgeVariant::Neutral,
    };
    let cell = |w: f32, s: String| {
        row().w(w).shrink().child(text(s.clone()).font(type_::at(k, type_::CONTROL)).color(t.text_muted.color()).max_lines(1).help_when_truncated(s))
    };
    let expires = day_label(day_of(a.state.tz, unix(r.expires_at)), a.state.today);
    let mut line = row()
        .key(format!("invite-row-{id}"))
        .role("row")
        .label(format!("{}, {}", r.email, invite_status_label(r.status)))
        .w_full()
        .min_h(row_h(k))
        .px(8.0)
        .py(6.0)
        .gap(12.0)
        .center_y()
        .radius(radius::MD)
        .hover(t.hover.color())
        .child(row().grow(1.0).shrink().min_w(0.0).child(who));
    if !compact {
        line = line.child(cell(INVITE_SERVERS_W, if servers.is_empty() { "\u{2013}".into() } else { servers }));
    }
    line = line.child(row().w(INVITE_STATUS_W).shrink().child(Badge::new(invite_status_label(r.status)).variant(status).build(k)));
    if !compact {
        line = line.child(cell(INVITE_BY_W, r.inviter.as_ref().map_or_else(|| "Server".to_owned(), |u| u.username.clone())));
    }
    line = line.child(cell(INVITE_EXPIRES_W, expires));
    let mut actions = row().w(INVITE_ACTIONS_W).shrink().gap(4.0).justify_end();
    if matches!(r.status, InviteStatus::Pending | InviteStatus::Expired) {
        actions = actions.child(
            Button::new("Resend")
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .disabled(busy)
                .on_press(move |a: &mut App| a.act(|s| s.admin_resend_invite(id)))
                .build(k)
                .key(format!("admin-invite-resend-{id}")),
        );
    }
    if r.status == InviteStatus::Pending {
        actions = actions.child(
            Button::new("Withdraw")
                .variant(Variant::Ghost)
                .size(Size::Sm)
                .disabled(busy)
                .on_press(move |a: &mut App| a.act(|s| s.admin_revoke_invite(id)))
                .build(k)
                .key(format!("admin-invite-revoke-{id}")),
        );
    }
    line.child(actions)
}

/// "Create invite": an email, the servers to join (every server the dashboard knows) and whether
/// the new account is a site admin.
fn invite_form(k: &Kit, layer: &mut Vec<El>, ad: &AdminState) {
    let Some(f) = &ad.invite_form else { return };
    let t = theme_of(k);
    let input = TextInput::new(&f.email)
        .placeholder("friend@example.com")
        .large()
        .on_edit(invite_email_edit)
        .on_submit(|a: &mut App, _| a.act(|s| s.submit_invite_form()));
    let mut servers = row().key("admin-invite-servers").w_full().flex_wrap().gap(8.0);
    for (id, name) in &ad.known_guilds {
        let id = *id;
        let on = f.guilds.contains(&id);
        servers = servers.child(
            row()
                .key(format!("admin-invite-server-{id}"))
                .gap(8.0)
                .px(10.0)
                .h(32.0)
                .center_y()
                .radius(radius::MD)
                .fill(if on { t.accent_subtle.color() } else { t.hover.color() })
                .child(Checkbox::new(on).label(name.clone()).hide_label().on_toggle(move |a: &mut App| a.act(|s| s.toggle_invite_guild(id))).build(k))
                .child(text(name.clone()).font(type_::at(k, type_::CONTROL)).color(t.text.color()).a11y_hidden()),
        );
    }
    if ad.known_guilds.is_empty() {
        servers = servers.child(muted(k, "No servers yet."));
    }
    let admin = row()
        .key("admin-invite-admin")
        .w_full()
        .gap(12.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .shrink()
                .gap(2.0)
                .child(text("Make them a site admin").font(type_::at(k, type_::CONTROL_STRONG)).color(t.text.color()))
                .child(text("They will see this dashboard, delete anyone's files and manage admins.").font(type_::at(k, type_::CAPTION)).color(t.text_muted.color()).wrap()),
        )
        .child(
            thehideout_design::components::switch(f.make_admin)
                .label("Make them a site admin")
                .hide_label()
                .on_toggle(|a: &mut App| a.act(|s| s.toggle_invite_admin()))
                .build(k)
                .key("admin-invite-admin-switch"),
        );
    let body = col()
        .key("admin-invite-form")
        .w_full()
        .gap(16.0)
        .child(dialog_field(k, "Email", input, f.error.as_deref()).w_full())
        .child(col().w_full().gap(6.0).child(text("Servers to join").font(type_::at(k, type_::LABEL)).color(t.text_muted.color())).child(servers))
        .child(admin);
    let close = |a: &mut App| a.act(|s| s.close_invite_form());
    Dialog::new("admin-invite-dlg", "Create invite")
        .description("They get an email with a link and a code to make their account. Invites last 7 days.")
        .open(true)
        .child(body)
        .footer(Button::new("Cancel").variant(Variant::Secondary).on_press(close).build(k).key("admin-cancel"))
        .footer(
            Button::new(if f.busy { "Sending\u{2026}" } else { "Send invite" })
                .disabled(f.busy)
                .on_press(|a: &mut App| a.act(|s| s.submit_invite_form()))
                .build(k)
                .key("admin-submit"),
        )
        .on_close(close)
        .build(k, layer);
}

// ---- audit log -------------------------------------------------------------------------------

const AUDIT_ACTOR_W: f32 = 160.0;
const AUDIT_ACTION_W: f32 = 140.0;
const AUDIT_TIME_W: f32 = 150.0;

fn audit(k: &Kit, ad: &AdminState, compact: bool) -> Parts {
    let feed = &ad.audit;
    let mut head = vec![
        head_cell(k, "Actor", Some(AUDIT_ACTOR_W), false),
        head_cell(k, "Action", Some(AUDIT_ACTION_W), false),
        head_cell(k, "Target", None, false),
    ];
    if !compact {
        head.push(head_cell(k, "Reason", None, false));
    }
    head.push(head_cell(k, "When", Some(AUDIT_TIME_W), false));
    let body = if !feed.loaded && feed.error.is_none() {
        loading(k)
    } else if feed.items.is_empty() && feed.error.is_none() {
        blank(
            k,
            &icons::CLIPBOARD_TEXT,
            "Nothing logged yet",
            "Deleted files and changes to admin rights show up here.",
        )
    } else {
        let list = rows(
            "admin-audit-rows",
            feed.items.len(),
            44.0,
            k,
            |s, i| {
                s.admin().map_or_else(String::new, |ad| {
                    format!("audit-row-{}", ad.audit.items[i].id)
                })
            },
            move |a, k, i| audit_row(a, k, i, compact),
        );
        col()
            .w_full()
            .max_w(MAX_W)
            .px(pad(k) - 8.0)
            .pb(pad(k))
            .child(list)
            .children(list_foot(k, feed.pending, &feed.error))
    };
    Parts {
        controls: None,
        head: Some(table_head(head)),
        body: col().w_full().items_center().child(body),
    }
}

/// What an audit action says in a row.
pub fn action_label(action: &str) -> &str {
    match action {
        "file.delete" => "Deleted a file",
        "admin.grant" => "Made admin",
        "admin.revoke" => "Removed admin",
        "invite.create" => "Invited",
        "invite.resend" => "Resent an invite",
        "invite.revoke" => "Withdrew an invite",
        "invite.accept" => "Accepted an invite",
        other => other,
    }
}

fn audit_row(a: &App, k: &Kit, i: usize, compact: bool) -> El {
    let t = theme_of(k);
    let Some(e) = a.state.admin().and_then(|ad| ad.audit.items.get(i)) else {
        return el().key(format!("audit-gone-{i}")).h(44.0);
    };
    let e: &AuditEntry = e;
    let actor = e
        .actor
        .as_ref()
        .map_or("Unknown".to_owned(), |u| u.username.clone());
    let actor_id = e.actor.as_ref().map_or(0, |u| u.id);
    let ts = unix(e.created_at);
    let when = format!(
        "{}, {}",
        day_label(day_of(a.state.tz, ts), a.state.today),
        clock_label(a.state.tz, ts)
    );
    let cell = |w: Option<f32>, s: String, muted: bool| {
        let c = match w {
            Some(w) => row().w(w).shrink(),
            None => row().grow(1.0).shrink().min_w(0.0),
        };
        c.child(
            text(s.clone())
                .font(type_::at(k, type_::CONTROL))
                .color(if muted { t.text_muted } else { t.text }.color())
                .max_lines(1)
                .help_when_truncated(s),
        )
    };
    let mut line = row()
        .key(format!("audit-row-{}", e.id))
        .role("row")
        .label(format!("{actor}: {} {}", action_label(&e.action), e.target))
        .w_full()
        .h(44.0)
        .px(8.0)
        .gap(12.0)
        .center_y()
        .radius(radius::MD)
        .hover(t.hover.color())
        .child(
            row()
                .w(AUDIT_ACTOR_W)
                .shrink()
                .gap(8.0)
                .center_y()
                .child(
                    Avatar::new(actor_id.to_string(), actor.clone())
                        .size(24)
                        .image(
                            e.actor
                                .as_ref()
                                .and_then(|u| u.avatar.as_deref())
                                .and_then(|k| a.state.data.media.pixels(k)),
                        )
                        .backdrop(t.surface.color())
                        .build(k),
                )
                .child(
                    text(actor)
                        .font(type_::at(k, type_::CONTROL_STRONG))
                        .color(t.text.color())
                        .max_lines(1),
                ),
        )
        .child(
            cell(
                Some(AUDIT_ACTION_W),
                action_label(&e.action).to_owned(),
                false,
            )
            .help(e.action.clone()),
        )
        .child(cell(None, e.target.clone(), false));
    if !compact {
        line = line.child(cell(
            None,
            if e.reason.is_empty() {
                "\u{2013}".to_owned()
            } else {
                e.reason.clone()
            },
            true,
        ));
    }
    line.child(cell(Some(AUDIT_TIME_W), when, true))
}

// ---- confirmations ---------------------------------------------------------------------------

fn reason_edit(a: &mut App) -> &mut TextEdit {
    match a
        .state
        .ui
        .admin
        .as_mut()
        .and_then(|ad| match ad.confirm.as_mut() {
            Some(Confirm::DeleteFiles(d)) => Some(&mut d.reason),
            _ => None,
        }) {
        Some(r) => r,
        None => &mut a.state.ui.idle_composer,
    }
}

fn cancel(a: &mut App) {
    a.act(|s| s.cancel_admin_confirm());
}

fn confirm(k: &Kit, layer: &mut Vec<El>, ad: &AdminState) {
    let t = theme_of(k);
    match &ad.confirm {
        None => {}
        Some(Confirm::DeleteFiles(d)) => {
            let keys = &d.keys;
            let reason = &d.reason;
            let rows: Vec<&AdminFileRow> = keys
                .iter()
                .filter_map(|key| ad.files.items.iter().find(|r| &r.key == key))
                .collect();
            let total: u64 = rows.iter().map(|r| r.bytes).sum();
            let (title, button, description) = match rows.as_slice() {
                [one] => (
                    "Delete file".to_owned(),
                    "Delete file".to_owned(),
                    format!(
                        "\u{201c}{}\u{201d} ({}{}) will be removed from storage. Where it was shared it shows as removed. This can't be undone.",
                        file_title(one),
                        bytes(one.bytes),
                        one.guild
                            .as_ref()
                            .map_or(String::new(), |g| format!(", {}", g.name))
                    ),
                ),
                many => (
                    format!("Delete {} files", many.len()),
                    format!("Delete {} files", many.len()),
                    format!(
                        "{} files ({}) will be removed from storage. Where they were shared they show as removed. This can't be undone.",
                        many.len(),
                        bytes(total)
                    ),
                ),
            };
            let input = TextInput::new(reason)
                .placeholder("Why is it being removed?")
                .large()
                .on_edit(reason_edit)
                .on_submit(|a: &mut App, _| a.act(|s| s.confirm_delete_files()));
            let body = col()
                .key("admin-confirm")
                .w_full()
                .gap(8.0)
                .child(dialog_field(k, "Reason (optional)", input, None).w_full())
                .child(
                    text("Recorded in the audit log.")
                        .font(type_::at(k, type_::CAPTION))
                        .color(t.text_muted.color()),
                );
            Dialog::new("admin-dlg", title)
                .description(description)
                .open(true)
                .child(body)
                .footer(
                    Button::new("Cancel")
                        .variant(Variant::Secondary)
                        .on_press(cancel)
                        .build(k)
                        .key("admin-cancel"),
                )
                .footer(
                    Button::new(button)
                        .variant(Variant::Danger)
                        .on_press(|a: &mut App| a.act(|s| s.confirm_delete_files()))
                        .build(k)
                        .key("admin-submit"),
                )
                .on_close(cancel)
                .build(k, layer);
        }
        Some(Confirm::SetAdmin { user, make }) => {
            let name = ad
                .users
                .items
                .iter()
                .find(|r| r.user.id == *user)
                .map_or_else(|| format!("user {user}"), |r| r.user.username.clone());
            let (title, button, description) = if *make {
                (
                    "Make admin".to_owned(),
                    "Make admin",
                    format!(
                        "{name} will be able to see usage across the whole site, delete anyone's files and give or take away admin rights."
                    ),
                )
            } else {
                (
                    "Remove admin rights".to_owned(),
                    "Remove admin",
                    format!(
                        "{name} will lose access to this dashboard and can no longer delete other people's files."
                    ),
                )
            };
            Dialog::new("admin-dlg", title)
                .description(description)
                .open(true)
                .footer(
                    Button::new("Cancel")
                        .variant(Variant::Secondary)
                        .on_press(cancel)
                        .build(k)
                        .key("admin-cancel"),
                )
                .footer(
                    Button::new(button)
                        .variant(if *make {
                            Variant::Primary
                        } else {
                            Variant::Danger
                        })
                        .on_press(|a: &mut App| a.act(|s| s.confirm_set_admin()))
                        .build(k)
                        .key("admin-submit"),
                )
                .on_close(cancel)
                .build(k, layer);
        }
    }
}
