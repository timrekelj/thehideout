//! `AppState`: everything the UI draws and every mode it can be in. Plain data and pure
//! transitions: handlers mutate it and push [`Command`]s on `outbox`; replies come back through
//! `apply::apply`. Nothing here touches Rune's runtime, so it is unit-testable.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::SystemTime;

use rune::TextEdit;
use thehideout_api::{
    Channel, ChannelIconRef, ChannelKind, DmChannel, Emoji, Guild, GuildLayout, MAX_MESSAGE_CHARS, Message,
    PaletteColor, Permissions, Role, User,
};
use thehideout_design::components::{ConnectionStatus, EmojiPickerState, ScrollEdges, ToastKind};

use crate::attach::{AttachUi, OutFile, UploadState};
use crate::backend::{Command, Session};
use crate::emoji::{PickerUi, Reduced, SuggestUi};
use crate::format::{Tz, unix};
pub use crate::forms::{
    Auth, AuthError, AuthMode, ChannelForm, DialogKind, DialogState, Phase, auth_error, check_name,
    sentence,
};
use crate::layout::{self, Item};
use crate::look::Look;
use crate::media::MediaCache;
use crate::server_settings::ServerSettings;

pub const DEFAULT_SERVER: &str = "http://127.0.0.1:8080";
/// How long a loading placeholder waits before it shows, so a fast load never flashes it.
pub const PLACEHOLDER_DELAY: f64 = 0.15;
const TOAST_SECS: f64 = 3.5;
const TOAST_QUEUE: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Launch with a saved session to check: neither the form nor the shell yet.
    Loading,
    Auth,
    Main,
}

// ---- data ------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fetch {
    Idle,
    /// The first page. `since` is stamped by the next poll (the clock a handler sees can be stale)
    /// and `shown` flips once the load has outlasted [`PLACEHOLDER_DELAY`].
    Initial {
        since: Option<f64>,
        shown: bool,
    },
    Older,
}

/// One channel's loaded history, oldest first.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelLog {
    pub messages: Vec<Message>,
    pub oldest_id: Option<u64>,
    pub has_more: bool,
    pub loaded: bool,
    pub fetch: Fetch,
    /// Ids of messages that start a new group no matter who wrote the one before: the oldest
    /// message of each page that older pages were later loaded above. A row that merged into an
    /// older group when the page arrived would change its place in the tree, and Rune holds the
    /// reader's row by that place.
    pub breaks: Vec<u64>,
    /// Loaded before the gateway dropped, and not refetched since: the next visit asks for the
    /// newest page again and merges it.
    pub stale: bool,
}

impl Default for ChannelLog {
    fn default() -> Self {
        ChannelLog {
            messages: Vec::new(),
            oldest_id: None,
            has_more: false,
            loaded: false,
            fetch: Fetch::Idle,
            breaks: Vec::new(),
            stale: false,
        }
    }
}

impl ChannelLog {
    pub fn newest_id(&self) -> Option<u64> {
        self.messages.last().map(|m| m.id)
    }

    /// Adds `m` in id order unless it is already here (the RPC reply and the gateway echo both
    /// bring a sent message). Returns whether the log changed.
    pub fn insert(&mut self, m: Message) -> bool {
        match self.messages.binary_search_by_key(&m.id, |x| x.id) {
            Ok(_) => false,
            Err(at) => {
                self.messages.insert(at, m);
                self.oldest_id = self.messages.first().map(|m| m.id);
                true
            }
        }
    }

    /// Merges a page (newest first, as the API returns it). A message already here takes the
    /// page's reactions: the page is what the server says now, and events may have been missed.
    pub fn insert_page(&mut self, mut items: Vec<Message>) {
        items.sort_by_key(|m| m.id);
        for m in items {
            if let Ok(at) = self.messages.binary_search_by_key(&m.id, |x| x.id) {
                self.messages[at].reactions = m.reactions;
            } else {
                self.insert(m);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Pending,
    Failed,
}

/// A message this client sent that the server hasn't confirmed yet (or refused).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub nonce: String,
    pub channel: u64,
    pub content: String,
    pub at: SystemTime,
    pub state: Delivery,
    /// The files sent with it, for display.
    pub files: Vec<OutFile>,
    /// Their `PendingAttachment` ids, for the send and its retry.
    pub ids: Vec<u64>,
}

#[derive(Default)]
pub struct Data {
    pub guilds: Vec<Guild>,
    pub channels: HashMap<u64, Vec<Channel>>,
    pub dms: Vec<DmChannel>,
    pub users: HashMap<u64, User>,
    /// Member ids per guild, once loaded.
    pub members: HashMap<u64, Vec<u64>>,
    /// Guilds whose members were asked for and not yet refused, so one question is ever in flight.
    pub members_asked: HashSet<u64>,
    pub logs: HashMap<u64, ChannelLog>,
    pub outgoing: Vec<Outgoing>,
    /// Each guild's sections and channel order.
    pub layouts: HashMap<u64, GuildLayout>,
    pub roles: HashMap<u64, Vec<Role>>,
    /// Role ids per member per guild.
    pub member_roles: HashMap<u64, HashMap<u64, Vec<u64>>>,
    /// Guilds whose access was asked for and not yet refused.
    pub access_asked: HashSet<u64>,
    /// The layout as it was before an optimistic edit that the server hasn't answered. While one
    /// is out, further structure edits wait (they would be based on a version the server is about to retire).
    pub layout_undo: HashMap<u64, GuildLayout>,
    /// Each guild's roles version, for `ReorderRoles`.
    pub role_versions: HashMap<u64, u64>,
    /// The roles as they were before an optimistic reorder the server hasn't answered. While one
    /// is out, further reorders wait.
    pub role_undo: HashMap<u64, Vec<Role>>,
    /// Each loaded guild's custom emojis, sorted by name.
    pub emojis: HashMap<u64, Vec<Emoji>>,
    /// Guilds whose emojis were asked for and not yet refused.
    pub emojis_asked: HashSet<u64>,
    /// Who is in voice, per guild per user (the server keeps one state per user).
    pub voice: HashMap<u64, HashMap<u64, thehideout_api::VoiceState>>,
    /// Each guild's bans, once loaded (needs BAN_MEMBERS).
    pub bans: HashMap<u64, Vec<thehideout_api::Ban>>,
    /// Guilds we are leaving or deleting ourselves, so their `guild_removed` isn't announced as news.
    pub exiting: HashSet<u64>,
    /// Decoded avatars, server icons and emojis by media key.
    pub media: MediaCache,
}

// ---- ui --------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Home: the direct-message list.
    Home,
    Guild(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Unread {
    pub count: u32,
    pub mentions: u32,
}

/// Where the "New" divider sits in the channel being read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    None,
    /// Entered with `unread` new messages before the history arrived.
    Pending {
        channel: u64,
        unread: u32,
    },
    /// The divider goes after message `id` (`None`: before the first).
    After {
        channel: u64,
        id: Option<u64>,
    },
}

/// A page of the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Appearance,
    /// Microphone and speakers, how the mic opens, the voice hotkeys and the sounds.
    Voice,
    /// Email, password, two-factor, passkeys and signed-in devices.
    Security,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    None,
    /// The rail's "+": create or join a server.
    Add,
    /// The sidebar header's menu.
    Guild,
    /// The user strip's gear: profile, settings, (admin), security and sign out.
    User,
    /// A channel's menu (the row's "..." button, or a right-click at `at`).
    Channel {
        id: u64,
        at: Option<(i32, i32)>,
    },
    /// A section header's menu.
    Section {
        id: u64,
        at: Option<(i32, i32)>,
    },
    /// The channel menu's "Move to section" submenu.
    MoveTo {
        id: u64,
        at: Option<(i32, i32)>,
    },
    /// A message's menu (the hover bar's "More", or a right-click at `at`).
    Message {
        channel: u64,
        message: u64,
        at: Option<(i32, i32)>,
    },
    /// A member's menu in the member panel (a press on the row, or a right-click at `at`).
    Member {
        user: u64,
        at: Option<(i32, i32)>,
    },
    /// A voice participant's menu (a press on the row, or a right-click at `at`).
    Participant {
        user: u64,
        at: Option<(i32, i32)>,
    },
    /// The participant menu's "Move to" submenu (the other voice channels).
    ParticipantMove {
        user: u64,
        at: Option<(i32, i32)>,
    },
}

/// A sidebar row held by the pointer: dragged `dy` points from where it was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub guild: u64,
    pub item: Item,
    pub dy: f32,
}


/// Whether the event stream has been whole since the last `Connected`. The gateway doesn't
/// replay what it missed, and `ConnectionStatus` can't say: `Connecting` arrives before every
/// retry and overwrites `Reconnecting`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Live,
    /// A `Disconnected` came after the last `Connected`: events may have been missed.
    Interrupted,
}

pub struct ToastEntry {
    pub id: u64,
    pub kind: ToastKind,
    pub message: String,
    /// Stamped by the poll that first shows it.
    pub until: Option<f64>,
}

pub struct Ui {
    pub place: Place,
    pub guild_channel: HashMap<u64, u64>,
    pub home_dm: Option<u64>,
    pub unread: HashMap<u64, Unread>,
    pub marker: Marker,
    pub dialog: Option<DialogState>,
    pub menu: Menu,
    pub toasts: VecDeque<ToastEntry>,
    pub members_open: bool,
    pub composers: HashMap<u64, TextEdit>,
    /// The field shown while no channel is open.
    pub idle_composer: TextEdit,
    pub connection: ConnectionStatus,
    pub stream: Stream,
    pub at_bottom: bool,
    /// Messages that arrived below the fold while the reader was scrolled up.
    pub unseen_below: u32,
    pub dms_open: bool,
    /// Sections the user folded, `(guild, section)`; saved to the prefs (`collapsed_dirty`).
    pub collapsed: HashSet<(u64, u64)>,
    pub collapsed_dirty: bool,
    pub drag: Option<Drag>,
    /// The saved look (accent, background, appearance, tint, contrast, reduce motion). Scenes and
    /// tests force an appearance through it.
    pub look: Look,
    /// A change to `look` the saver has not been given yet (`App::flush` hands it over).
    pub look_dirty: bool,
    /// The settings page that replaces the shell, when open.
    pub settings: Option<Section>,
    /// The recorders of the voice hotkeys on the Voice & audio page.
    pub voice_keys: crate::voice::VoiceKeys,
    /// Which device list is open on that page.
    pub voice_menu: crate::voice::VoiceMenu,
    /// The server's settings page (roles, members, icon), which replaces the shell the same way.
    pub server_settings: Option<ServerSettings>,
    /// The site-admin dashboard (admins only), which replaces the shell the same way.
    pub admin: Option<crate::admin::AdminState>,
    /// Terms prompts, account deletion and data export (see `lifecycle.rs`).
    pub lifecycle: crate::lifecycle::Lifecycle,
    /// The delete-account dialog and the admin's user actions (see the end of this file).
    pub account: AccountUi,
    /// Text for the next poll to put on the clipboard (a press handler has no `Tx`).
    pub copy_request: Option<String>,
    /// The message being edited in the transcript.
    pub editing: Option<crate::moderation_ui::Editing>,
    pub toast_seq: u64,
    /// The emoji picker, when open.
    pub picker: Option<PickerUi>,
    /// The state handlers fall back on while no picker is open.
    pub idle_picker: EmojiPickerState,
    pub suggest: SuggestUi,
    /// Recently used emojis as reaction keys, newest first (saved to the prefs).
    pub recents: Vec<String>,
    pub recents_dirty: bool,
    /// Whether the window is drawing with reduced motion (set by the view each frame).
    pub reduced: Reduced,
    /// The scroll-edge hairlines of each scrolling region (see `ScrollEdges`).
    pub edges: Edges,
    /// Attachments, the lightbox, players, downloads and folded embeds.
    pub attach: AttachUi,
    /// The Security page, its dialogs and the add-email prompt.
    pub security: crate::account::SecurityUi,
    /// The server invite dialog's list.
    pub invites: crate::account::InvitesUi,
}

/// One `ScrollEdges` per scrolling region of the window; their values live here so a rebuilt view
/// binds the same lines again.
#[derive(Clone, Default)]
pub struct Edges {
    pub transcript: ScrollEdges,
    pub sidebar: ScrollEdges,
    pub members: ScrollEdges,
    /// The settings pages (user and server): only one is open at a time.
    pub page: ScrollEdges,
    /// The admin pages' scrolling body.
    pub admin: ScrollEdges,
}

impl Ui {
    fn new() -> Ui {
        Ui {
            place: Place::Home,
            guild_channel: HashMap::new(),
            home_dm: None,
            unread: HashMap::new(),
            marker: Marker::None,
            dialog: None,
            menu: Menu::None,
            toasts: VecDeque::new(),
            members_open: false,
            composers: HashMap::new(),
            idle_composer: TextEdit::new(""),
            connection: ConnectionStatus::Connecting,
            stream: Stream::Live,
            at_bottom: true,
            unseen_below: 0,
            dms_open: true,
            collapsed: HashSet::new(),
            collapsed_dirty: false,
            drag: None,
            look: Look::default(),
            look_dirty: false,
            settings: None,
            voice_keys: Default::default(),
            voice_menu: Default::default(),
            server_settings: None,
            admin: None,
            lifecycle: Default::default(),
            account: Default::default(),
            copy_request: None,
            editing: None,
            toast_seq: 0,
            picker: None,
            idle_picker: EmojiPickerState::new(),
            suggest: SuggestUi::default(),
            recents: Vec::new(),
            recents_dirty: false,
            reduced: Reduced::default(),
            edges: Edges::default(),
            attach: AttachUi::default(),
            security: Default::default(),
            invites: Default::default(),
        }
    }
}

pub struct AppState {
    pub screen: Screen,
    pub auth: Auth,
    pub session: Option<Session>,
    pub data: Data,
    pub ui: Ui,
    pub outbox: Vec<Command>,
    /// The app clock as of the last poll.
    pub clock: f64,
    /// Our own voice connection (see `voice.rs`).
    pub voice: crate::voice::VoiceLocal,
    /// The wall-clock day number "Today" means.
    pub today: i64,
    pub tz: Tz,
    /// The wall clock; scenes and tests swap in a fixed one.
    pub now: fn() -> SystemTime,
    nonce: u64,
}

impl AppState {
    pub fn new(now: fn() -> SystemTime, tz: Tz) -> AppState {
        AppState {
            screen: Screen::Auth,
            auth: Auth::new(DEFAULT_SERVER),
            session: None,
            data: Data::default(),
            ui: Ui::new(),
            outbox: Vec::new(),
            clock: 0.0,
            voice: Default::default(),
            today: crate::format::day_of(tz, unix(now())),
            tz,
            now,
            nonce: 0,
        }
    }

    /// The URL in the "Server" field.
    pub fn server_url(&self) -> String {
        self.auth.server.text.trim().to_owned()
    }

    /// The URL a stored file's bytes are served at on the configured server (what
    /// `Client::media_url` builds from the same base URL).
    pub fn media_url(&self, key: &str) -> String {
        format!("{}/media/{key}", self.server_url().trim_end_matches('/'))
    }

    /// Back to a fresh signed-out app, keeping the server URL and the look.
    pub fn reset(&mut self, error: Option<AuthError>) {
        let (server, look, today, tz, now) = (
            self.server_url(),
            self.ui.look,
            self.today,
            self.tz,
            self.now,
        );
        let collapsed = std::mem::take(&mut self.ui.collapsed);
        let recents = std::mem::take(&mut self.ui.recents);
        let folded = std::mem::take(&mut self.ui.attach.embeds_collapsed);
        let waker = self.ui.attach.waker.take();
        let was_in_voice = self.voice.session != crate::voice::VoiceSession::None;
        // The picture cache keeps its decoder (and the waker it was given); the pictures go.
        let mut media = std::mem::take(&mut self.data.media);
        media.clear();
        *self = AppState::new(now, tz);
        self.data.media = media;
        self.ui.collapsed = collapsed;
        self.ui.recents = recents;
        self.ui.attach.embeds_collapsed = folded;
        self.ui.attach.waker = waker;
        self.today = today;
        self.auth = Auth::new(&server);
        self.auth.error = error;
        self.ui.look = look;
        if was_in_voice {
            self.voice.intents.push(crate::voice::VoiceIntent::Disconnect);
        }
    }

    // ---- queries -----------------------------------------------------------------------------

    pub fn me(&self) -> Option<&User> {
        self.session.as_ref().map(|s| &s.me)
    }

    pub fn my_id(&self) -> u64 {
        self.me().map_or(0, |u| u.id)
    }

    pub fn user_name(&self, id: u64) -> String {
        self.data
            .users
            .get(&id)
            .map_or_else(|| format!("user {id}"), |u| if u.deleted { crate::lifecycle::DELETED_USER.to_owned() } else { u.username.clone() })
    }

    pub fn guild(&self, id: u64) -> Option<&Guild> {
        self.data.guilds.iter().find(|g| g.id == id)
    }

    pub fn channels_of(&self, guild: u64) -> &[Channel] {
        self.data.channels.get(&guild).map_or(&[], Vec::as_slice)
    }

    /// A guild's channels in sidebar order: the layout's, then any the layout doesn't list yet.
    pub fn ordered_channels(&self, guild: u64) -> Vec<&Channel> {
        let all = self.channels_of(guild);
        let Some(l) = self.data.layouts.get(&guild) else {
            return all.iter().collect();
        };
        let mut out: Vec<&Channel> = layout::channel_order(l)
            .into_iter()
            .filter_map(|id| all.iter().find(|c| c.id == id))
            .collect();
        let unlisted: Vec<&Channel> = all
            .iter()
            .filter(|c| !out.iter().any(|o| o.id == c.id))
            .collect();
        out.extend(unlisted);
        out
    }

    /// The first text channel in sidebar order: what a server opens on (voice channels are joined,
    /// not opened).
    pub fn first_text_channel(&self, guild: u64) -> Option<u64> {
        self.ordered_channels(guild)
            .iter()
            .find(|c| c.kind == ChannelKind::Text)
            .map(|c| c.id)
    }

    pub fn channel(&self, guild: u64, id: u64) -> Option<&Channel> {
        self.channels_of(guild).iter().find(|c| c.id == id)
    }

    /// What the signed-in user may do in `guild` (nothing until its roles have arrived, unless they own it).
    pub fn perms(&self, guild: u64) -> Permissions {
        let Some(g) = self.guild(guild) else {
            return Permissions::NONE;
        };
        let mine = self
            .data
            .member_roles
            .get(&guild)
            .and_then(|m| m.get(&self.my_id()))
            .map_or(&[][..], Vec::as_slice);
        crate::perms::effective(
            g,
            self.my_id(),
            mine,
            self.data.roles.get(&guild).map_or(&[][..], Vec::as_slice),
        )
    }

    /// Whether the structure controls (drag, menus, create section, settings) are offered.
    pub fn can_manage_channels(&self, guild: u64) -> bool {
        self.perms(guild).contains(Permissions::MANAGE_CHANNELS)
    }

    pub fn section_open(&self, guild: u64, section: u64) -> bool {
        !self.ui.collapsed.contains(&(guild, section))
    }

    pub fn dm(&self, id: u64) -> Option<&DmChannel> {
        self.data.dms.iter().find(|d| d.id == id)
    }

    /// The other participant of a DM.
    pub fn dm_peer<'a>(&self, dm: &'a DmChannel) -> Option<&'a User> {
        let me = self.my_id();
        dm.participants
            .iter()
            .find(|u| u.id != me)
            .or(dm.participants.first())
    }

    /// The channel (or DM) being read.
    pub fn current_channel(&self) -> Option<u64> {
        match self.ui.place {
            Place::Home => self.ui.home_dm,
            Place::Guild(g) => self.ui.guild_channel.get(&g).copied(),
        }
    }

    pub fn current_guild(&self) -> Option<&Guild> {
        match self.ui.place {
            Place::Guild(g) => self.guild(g),
            Place::Home => None,
        }
    }

    /// The open guild channel, whole (for its icon, colour and topic).
    pub fn current_guild_channel(&self) -> Option<&Channel> {
        match self.ui.place {
            Place::Guild(g) => self.current_channel().and_then(|c| self.channel(g, c)),
            Place::Home => None,
        }
    }

    pub fn current_channel_name(&self) -> Option<String> {
        let id = self.current_channel()?;
        match self.ui.place {
            Place::Home => self
                .dm(id)
                .and_then(|d| self.dm_peer(d))
                .map(|u| u.username.clone()),
            Place::Guild(g) => self
                .channels_of(g)
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone()),
        }
    }

    pub fn log(&self, channel: u64) -> Option<&ChannelLog> {
        self.data.logs.get(&channel)
    }

    pub fn unread_of(&self, channel: u64) -> Unread {
        self.ui.unread.get(&channel).copied().unwrap_or_default()
    }

    pub fn guild_unread(&self, guild: u64) -> Unread {
        self.channels_of(guild)
            .iter()
            .fold(Unread::default(), |a, c| {
                let u = self.unread_of(c.id);
                Unread {
                    count: a.count + u.count,
                    mentions: a.mentions + u.mentions,
                }
            })
    }

    pub fn home_unread(&self) -> Unread {
        self.data.dms.iter().fold(Unread::default(), |a, d| {
            let u = self.unread_of(d.id);
            Unread {
                count: a.count + u.count,
                mentions: a.mentions + u.mentions,
            }
        })
    }

    /// Everyone the user could start a DM with: members of their servers and DM partners.
    pub fn known_users(&self) -> Vec<&User> {
        let me = self.my_id();
        let mut v: Vec<&User> = self.data.users.values().filter(|u| u.id != me).collect();
        v.sort_by_key(|u| u.username.to_lowercase());
        v
    }

    /// Asks for every avatar and server icon that isn't cached, on its way or given up on.
    pub fn want_media(&mut self) {
        let mut keys: Vec<String> = self
            .data
            .users
            .values()
            .filter_map(|u| u.avatar.clone())
            .collect();
        keys.extend(self.data.guilds.iter().filter_map(|g| g.icon.clone()));
        // Only the open server's emojis are fetched; the cache is bounded.
        if let Some(g) = self.emoji_guild() {
            keys.extend(self.emojis_of(g).iter().map(|e| e.image.clone()));
        }
        keys.extend(self.admin_media_keys());
        keys.extend(self.attachment_media_keys());
        keys.sort();
        keys.dedup();
        for key in keys {
            if self.data.media.wants(&key) {
                self.data.media.ask(&key);
                self.outbox.push(Command::FetchMedia { key });
            }
        }
    }

    /// The decoded avatar of `user`, if they have one and it is cached.
    pub fn avatar_pixels(&self, user: u64) -> Option<rune::Pixels> {
        self.data
            .users
            .get(&user)?
            .avatar
            .as_deref()
            .and_then(|k| self.data.media.pixels(k))
    }

    /// The decoded icon of `guild`.
    pub fn icon_pixels(&self, guild: u64) -> Option<rune::Pixels> {
        self.guild(guild)?
            .icon
            .as_deref()
            .and_then(|k| self.data.media.pixels(k))
    }

    /// Whether the composer may send: the gateway is up (or still coming up).
    pub fn composer_block(&self) -> Option<String> {
        match self.ui.connection {
            ConnectionStatus::Reconnecting(_) => {
                Some("Reconnecting\u{2026} you can send again once you're back online".into())
            }
            ConnectionStatus::Offline => {
                Some("You're offline. Messages can't be sent right now".into())
            }
            ConnectionStatus::Connected | ConnectionStatus::Connecting => None,
        }
    }

    /// The composer's field for the open channel (drafts are kept per channel).
    pub fn composer_mut(&mut self) -> &mut TextEdit {
        match self.current_channel() {
            Some(c) => self
                .ui
                .composers
                .entry(c)
                .or_insert_with(|| TextEdit::new("")),
            None => &mut self.ui.idle_composer,
        }
    }

    pub fn composer(&self) -> &TextEdit {
        self.current_channel()
            .and_then(|c| self.ui.composers.get(&c))
            .unwrap_or(&self.ui.idle_composer)
    }

    // ---- settings ----------------------------------------------------------------------------

    pub fn open_settings(&mut self) {
        self.ui.menu = Menu::None;
        self.ui.server_settings = None;
        self.ui.settings = Some(Section::Appearance);
    }

    pub fn close_settings(&mut self) {
        self.ui.settings = None;
    }

    /// Changes the look; a real change is queued for the saver.
    pub fn edit_look(&mut self, f: impl FnOnce(&mut Look)) {
        let before = self.ui.look;
        f(&mut self.ui.look);
        if self.ui.look != before {
            self.ui.look_dirty = true;
        }
    }

    // ---- navigation --------------------------------------------------------------------------

    pub fn go_home(&mut self) {
        self.ui.menu = Menu::None;
        if self.ui.place == Place::Home {
            return;
        }
        self.ui.place = Place::Home;
        self.arrive();
    }

    pub fn go_guild(&mut self, id: u64) {
        self.ui.menu = Menu::None;
        if self.ui.place == Place::Guild(id) || self.guild(id).is_none() {
            return;
        }
        self.ui.place = Place::Guild(id);
        if !self.ui.guild_channel.contains_key(&id)
            && let Some(first) = self.first_text_channel(id)
        {
            self.ui.guild_channel.insert(id, first);
        }
        self.want_members(id);
        self.want_access(id);
        self.want_emojis(id);
        // Pictures that were pushed out to make room come back when their server is visited.
        self.data.media.forgive();
        self.want_media();
        if !self.data.channels.contains_key(&id) {
            self.outbox.push(Command::LoadChannels { guild: id });
        }
        self.arrive();
    }

    /// Asks for a guild's members unless they are loaded or already asked for.
    pub fn want_members(&mut self, guild: u64) {
        if !self.data.members.contains_key(&guild) && self.data.members_asked.insert(guild) {
            self.outbox.push(Command::LoadMembers { guild });
        }
    }

    /// Asks for a guild's roles and role assignments unless that is already done or asked.
    pub fn want_access(&mut self, guild: u64) {
        if !self.data.roles.contains_key(&guild) && self.data.access_asked.insert(guild) {
            self.outbox.push(Command::LoadAccess { guild });
        }
    }

    /// Asks for the members of the guild `m` was posted in when its author is a stranger.
    pub fn want_author(&mut self, m: &Message) {
        if let Some(guild) = m.guild_id
            && !self.data.users.contains_key(&m.author_id)
        {
            self.want_members(guild);
        }
    }

    /// Opens `channel` in the current place. A voice channel is not opened: it is joined, and the
    /// text channel that is open stays.
    pub fn select_channel(&mut self, channel: u64) {
        if let Place::Guild(g) = self.ui.place
            && self.channel(g, channel).is_some_and(|c| c.kind == ChannelKind::Voice)
        {
            // The channel we are in opens its stage; any other one is joined.
            if self.my_voice_channel() == Some(channel) {
                return self.open_stage();
            }
            return self.join_voice(channel);
        }
        // A text channel is on the main panel: the voice stage gives way to it, even when it is
        // the one that was open under the stage.
        self.voice.view = crate::voice::MainView::Chat;
        if self.current_channel() == Some(channel) {
            return;
        }
        match self.ui.place {
            Place::Home => self.ui.home_dm = Some(channel),
            Place::Guild(g) => {
                self.ui.guild_channel.insert(g, channel);
            }
        }
        self.arrive();
    }

    /// Alt+Up / Alt+Down: the previous or next channel (or DM) in the sidebar.
    pub fn step_channel(&mut self, delta: i32) {
        let ids: Vec<u64> = match self.ui.place {
            Place::Home => self.data.dms.iter().map(|d| d.id).collect(),
            Place::Guild(g) => self
                .ordered_channels(g)
                .iter()
                .filter(|c| c.kind == ChannelKind::Text)
                .map(|c| c.id)
                .collect(),
        };
        if ids.is_empty() {
            return;
        }
        let at = self
            .current_channel()
            .and_then(|c| ids.iter().position(|&i| i == c));
        let next = match at {
            Some(i) => (i as i32 + delta).clamp(0, ids.len() as i32 - 1) as usize,
            None => 0,
        };
        self.select_channel(ids[next]);
    }

    /// The reader just arrived at the current channel: fix the "New" divider, clear its unread
    /// count and fetch its history if it hasn't been.
    fn arrive(&mut self) {
        // Going anywhere returns the main panel to the transcript (the voice stage was on it).
        self.voice.view = crate::voice::MainView::Chat;
        self.ui.at_bottom = true;
        self.ui.unseen_below = 0;
        self.ui.marker = Marker::None;
        let Some(channel) = self.current_channel() else {
            return;
        };
        let unread = self.ui.unread.remove(&channel).filter(|u| u.count > 0);
        let log = self.data.logs.entry(channel).or_default();
        if let Some(u) = unread {
            self.ui.marker = if log.loaded {
                let at = log.messages.len().checked_sub(u.count as usize + 1);
                Marker::After {
                    channel,
                    id: at.map(|i| log.messages[i].id),
                }
            } else {
                Marker::Pending {
                    channel,
                    unread: u.count,
                }
            };
        }
        if !log.loaded && log.fetch == Fetch::Idle {
            log.fetch = Fetch::Initial {
                since: None,
                shown: false,
            };
            self.outbox.push(Command::LoadMessages {
                channel,
                before: None,
            });
        } else if log.loaded && log.stale {
            // Keeps what is shown; the newest page merges in when it lands.
            log.stale = false;
            self.outbox.push(Command::LoadMessages {
                channel,
                before: None,
            });
        }
    }

    /// The gateway is back after a gap and doesn't replay: fetch the lists again, the open
    /// channel's newest page now, and every other loaded channel's on its next visit. Nothing
    /// is cleared and the selection stays.
    pub fn resync(&mut self) {
        self.outbox.push(Command::Refresh);
        let current = self.current_channel();
        for (&id, log) in &mut self.data.logs {
            log.stale = log.loaded && Some(id) != current;
        }
        if let Some(channel) = current {
            self.outbox.push(Command::LoadMessages {
                channel,
                before: None,
            });
        }
    }

    /// Tries the open channel's first page again after a failed load.
    pub fn reload_current(&mut self) {
        let Some(channel) = self.current_channel() else {
            return;
        };
        let log = self.data.logs.entry(channel).or_default();
        if !log.loaded && log.fetch == Fetch::Idle {
            log.fetch = Fetch::Initial {
                since: None,
                shown: false,
            };
            self.outbox.push(Command::LoadMessages {
                channel,
                before: None,
            });
        }
    }

    /// The reply button: starts the draft with an @mention of the message's author.
    pub fn reply_to(&mut self, chat_id: &str) {
        let Some(channel) = self.current_channel() else {
            return;
        };
        let me = self.my_id();
        let author = self
            .log(channel)
            .and_then(|l| l.messages.iter().find(|m| chat_id_of(m, me) == chat_id))
            .map(|m| m.author_id);
        if let Some(author) = author {
            let name = self.user_name(author);
            let edit = self.composer_mut();
            let draft = format!("@{name} {}", edit.text);
            edit.replace_text(&draft);
            edit.focused = true;
        }
    }

    /// The copy button: the message text goes to the clipboard on the next poll.
    pub fn copy_message(&mut self, chat_id: &str) {
        let Some(channel) = self.current_channel() else {
            return;
        };
        let me = self.my_id();
        let found = self
            .log(channel)
            .and_then(|l| l.messages.iter().find(|m| chat_id_of(m, me) == chat_id))
            .map(|m| m.content.clone());
        let found = found.or_else(|| {
            self.data
                .outgoing
                .iter()
                .find(|o| format!("n:{}", o.nonce) == chat_id)
                .map(|o| o.content.clone())
        });
        self.ui.copy_request = found;
    }

    /// Asks for the page older than what's loaded, once.
    pub fn load_older(&mut self, channel: u64) {
        let Some(log) = self.data.logs.get_mut(&channel) else {
            return;
        };
        if !log.loaded || !log.has_more || log.fetch != Fetch::Idle {
            return;
        }
        log.fetch = Fetch::Older;
        self.outbox.push(Command::LoadMessages {
            channel,
            before: log.oldest_id,
        });
    }

    // ---- sending -----------------------------------------------------------------------------

    /// Queues `text` (with the channel's uploaded files) as a pending message in the open channel.
    /// False (and nothing queued) when there is no channel, the text is over the limit, a file is
    /// still going up, or there is neither text nor a finished file.
    pub fn send(&mut self, text: &str) -> bool {
        let content = text.trim();
        let Some(channel) = self.current_channel() else {
            return false;
        };
        if content.chars().count() > MAX_MESSAGE_CHARS {
            return false;
        }
        let draft = self.ui.attach.drafts.get(&channel);
        if draft.is_some_and(|d| d.uploading()) || (content.is_empty() && draft.is_none_or(|d| d.ready() == 0)) {
            return false;
        }
        let (mut ids, mut files) = (Vec::new(), Vec::new());
        if let Some(d) = self.ui.attach.drafts.get_mut(&channel) {
            d.files.retain(|f| match &f.state {
                UploadState::Ready { pending } => {
                    ids.push(*pending);
                    files.push(OutFile { name: f.name.clone(), content_type: f.content_type.clone(), size: f.size, thumb: f.thumb.clone() });
                    false
                }
                _ => true,
            });
            d.note = None;
        }
        self.nonce += 1;
        let nonce = format!("{}-{}", self.my_id(), self.nonce);
        self.data.outgoing.push(Outgoing {
            nonce: nonce.clone(),
            channel,
            content: content.to_owned(),
            at: (self.now)(),
            state: Delivery::Pending,
            files,
            ids: ids.clone(),
        });
        self.outbox.push(send_command(channel, content.to_owned(), nonce, ids));
        true
    }

    pub fn retry(&mut self, nonce: &str) {
        if let Some(o) = self
            .data
            .outgoing
            .iter_mut()
            .find(|o| o.nonce == nonce && o.state == Delivery::Failed)
        {
            o.state = Delivery::Pending;
            self.outbox.push(send_command(o.channel, o.content.clone(), o.nonce.clone(), o.ids.clone()));
        }
    }

    // ---- auth --------------------------------------------------------------------------------

    pub fn sign_out(&mut self) {
        // `reset` starts from a fresh state, outbox included, so the command goes in after it.
        self.reset(None);
        self.outbox.push(Command::SignOut);
    }

    // ---- dialogs -----------------------------------------------------------------------------

    pub fn open_dialog(&mut self, kind: DialogKind) {
        self.ui.menu = Menu::None;
        let (text, channel) = match kind {
            DialogKind::CreateChannel { .. } => (String::new(), Some(ChannelForm::new(None))),
            DialogKind::RenameSection { guild, section } => (
                self.data
                    .layouts
                    .get(&guild)
                    .and_then(|l| l.sections.iter().find(|s| s.id == section))
                    .map_or_else(String::new, |s| s.name.clone()),
                None,
            ),
            DialogKind::ChannelSettings { guild, channel } => match self.channel(guild, channel) {
                Some(c) => (c.name.clone(), Some(ChannelForm::of(c))),
                None => return,
            },
            DialogKind::InviteByEmail { guild } => {
                self.invite_dialog_opened(guild);
                (String::new(), None)
            }
            _ => (String::new(), None),
        };
        let mut input = TextEdit::new(&text);
        input.focused = true;
        self.ui.dialog = Some(DialogState {
            kind,
            input,
            channel,
            error: None,
            phase: Phase::Idle,
            pick: Default::default(),
            form: Default::default(),
        });
    }

    /// "Create channel here": the create dialog with the section already chosen.
    pub fn open_create_channel_in(&mut self, guild: u64, section: u64) {
        self.open_dialog(DialogKind::CreateChannel { guild });
        if let Some(f) = self.ui.dialog.as_mut().and_then(|d| d.channel.as_mut()) {
            f.section = Some(section);
        }
    }

    pub fn close_dialog(&mut self) {
        self.ui.dialog = None;
    }

    /// Enter or the primary button. Validates first; an invalid form shows its error and sends
    /// nothing.
    pub fn submit_dialog(&mut self) {
        if self.submit_moderation_dialog() {
            return;
        }
        if let Some((DialogKind::InviteByEmail { guild }, value)) = self
            .ui
            .dialog
            .as_ref()
            .filter(|d| d.phase != Phase::Busy)
            .map(|d| (d.kind, d.input.text.clone()))
        {
            let result = self.invite_command(guild, value);
            if let Some(d) = &mut self.ui.dialog {
                match result {
                    Ok(cmd) => {
                        d.error = None;
                        d.phase = Phase::Busy;
                        self.outbox.push(cmd);
                    }
                    Err(e) => fail(d, e),
                }
            }
            return;
        }
        let Some(d) = &mut self.ui.dialog else { return };
        if d.phase == Phase::Busy {
            return;
        }
        let value = d.input.text.trim().to_owned();
        let cmd = match d.kind {
            DialogKind::Invite { .. } => {
                self.ui.dialog = None;
                return;
            }
            DialogKind::InviteByEmail { .. }
            | DialogKind::DeleteMessage { .. }
            | DialogKind::KickMember { .. }
            | DialogKind::BanMember { .. }
            | DialogKind::LeaveGuild { .. }
            | DialogKind::DeleteChannel { .. }
            | DialogKind::DeleteGuild { .. }
            | DialogKind::TransferOwnership { .. } => return,
            DialogKind::CreateGuild => match check_name(&value, 2, 32, "Give the server a name") {
                Ok(()) => Command::CreateGuild { name: value },
                Err(e) => return fail(d, e),
            },
            DialogKind::JoinGuild => {
                if value.is_empty() {
                    return fail(d, "Paste an invite code".into());
                }
                Command::JoinGuild { code: value }
            }
            DialogKind::CreateChannel { guild } => {
                let name = value.to_lowercase().replace(' ', "-");
                let f = d.channel.as_ref();
                match check_name(&name, 1, 32, "Give the channel a name") {
                    Ok(()) => Command::CreateChannel {
                        guild,
                        name,
                        kind: f.map_or(ChannelKind::Text, |f| f.kind),
                        section: f.and_then(|f| f.section),
                        icon: f.map_or(ChannelIconRef::Default, ChannelForm::icon_ref),
                        color: PaletteColor::from_key(f.map_or("", |f| f.color)),
                    },
                    Err(e) => return fail(d, e),
                }
            }
            DialogKind::CreateSection { guild } => {
                match check_name(&value, 1, 32, "Give the section a name") {
                    Ok(()) => Command::CreateSection { guild, name: value },
                    Err(e) => return fail(d, e),
                }
            }
            DialogKind::RenameSection { guild, section } => {
                match check_name(&value, 1, 32, "Give the section a name") {
                    Ok(()) => Command::RenameSection {
                        guild,
                        section,
                        name: value,
                    },
                    Err(e) => return fail(d, e),
                }
            }
            DialogKind::DeleteSection { guild, section } => {
                Command::DeleteSection { guild, section }
            }
            DialogKind::DeleteRole { guild, role } => Command::DeleteRole { guild, role },
            DialogKind::DeleteEmoji { guild, emoji } => Command::DeleteEmoji { guild, emoji },
            DialogKind::Profile => match d.pick.draft.as_ref() {
                Some(draft) => Command::UploadAvatar {
                    bytes: draft.bytes.clone(),
                    content_type: draft.content_type.clone(),
                },
                None => {
                    self.ui.dialog = None;
                    return;
                }
            },
            DialogKind::ChannelSettings { guild, channel } => {
                let name = value.to_lowercase().replace(' ', "-");
                if let Err(e) = check_name(&name, 1, 32, "Give the channel a name") {
                    return fail(d, e);
                }
                let Some(f) = d.channel.as_ref() else { return };
                let topic = f.topic.text.trim().to_owned();
                if topic.chars().count() > 256 {
                    return fail(d, "Use up to 256 characters for the topic".into());
                }
                let old = f.original.as_ref();
                let cmd = Command::UpdateChannel {
                    guild,
                    channel,
                    name: (old.map(|c| c.name.as_str()) != Some(name.as_str())).then_some(name),
                    icon: (old.map(|c| &c.icon_ref) != Some(&f.icon_ref())).then(|| f.icon_ref()),
                    color: (old.map(|c| c.color.as_key()) != Some(f.color))
                        .then(|| PaletteColor::from_key(f.color)),
                    topic: (old.map(|c| c.topic.as_str()) != Some(topic.as_str())).then_some(topic),
                };
                if matches!(
                    &cmd,
                    Command::UpdateChannel {
                        name: None,
                        icon: None,
                        color: None,
                        topic: None,
                        ..
                    }
                ) {
                    self.ui.dialog = None;
                    return;
                }
                cmd
            }
            DialogKind::NewDm => {
                let wanted = value.trim_start_matches('@').to_lowercase();
                if wanted.is_empty() {
                    return fail(d, "Enter a username".into());
                }
                let me = self.session.as_ref().map_or(0, |s| s.me.id);
                match self
                    .data
                    .users
                    .values()
                    .find(|u| u.id != me && !u.deleted && u.username.to_lowercase() == wanted)
                {
                    Some(u) => Command::OpenDm { user_id: u.id },
                    None => return fail(d, format!("No one called \"{wanted}\" in your servers")),
                }
            }
        };
        d.error = None;
        d.phase = Phase::Busy;
        self.outbox.push(cmd);
    }

    // ---- toasts ------------------------------------------------------------------------------

    pub fn toast(&mut self, kind: ToastKind, message: impl Into<String>) {
        self.ui.toast_seq += 1;
        self.ui.toasts.push_back(ToastEntry {
            id: self.ui.toast_seq,
            kind,
            message: message.into(),
            until: None,
        });
        while self.ui.toasts.len() > TOAST_QUEUE {
            self.ui.toasts.pop_front();
        }
    }

    /// Time-driven state, called from `poll`: toast expiry and the loading placeholder's delay.
    /// Returns whether something visible changed and when to be woken next.
    pub fn tick(&mut self, clock: f64) -> (bool, Option<f64>) {
        self.clock = clock;
        let mut changed = false;
        let mut next: Option<f64> = None;
        let mut soon = |t: f64| next = Some(next.map_or(t, |n| n.min(t)));
        // "Today" follows the wall clock past local midnight, and wakes the app there.
        let now = unix((self.now)());
        let today = crate::format::day_of(self.tz, now);
        if today != self.today {
            self.today = today;
            changed = true;
        }
        soon(clock + crate::format::until_next_day(self.tz, now) as f64);
        let (resend, at) = self.tick_resend(clock);
        changed |= resend;
        if let Some(at) = at {
            soon(at);
        }
        let (retry, at) = self.voice_tick(clock);
        changed |= retry;
        if let Some(at) = at {
            soon(at);
        }
        while let Some(front) = self.ui.toasts.front_mut() {
            match front.until {
                None => front.until = Some(clock + TOAST_SECS),
                Some(t) if t <= clock => {
                    self.ui.toasts.pop_front();
                    changed = true;
                    continue;
                }
                Some(_) => {}
            }
            soon(front.until.unwrap_or(clock));
            break;
        }
        if let Some(log) = self
            .current_channel()
            .and_then(|c| self.data.logs.get_mut(&c))
            && let Fetch::Initial { since, shown } = &mut log.fetch
            && !*shown
        {
            let start = *since.get_or_insert(clock);
            if clock - start >= PLACEHOLDER_DELAY {
                *shown = true;
                changed = true;
            } else {
                soon(start + PLACEHOLDER_DELAY);
            }
        }
        (changed, next)
    }

    // ---- derived -----------------------------------------------------------------------------

    /// Whether `content` mentions the signed-in user.
    pub fn mentions_me(&self, content: &str) -> bool {
        self.me().is_some_and(|m| mentions(content, &m.username))
    }

    pub fn day_of(&self, unix: i64) -> i64 {
        crate::format::day_of(self.tz, unix)
    }
}

/// `Send` for a text message, `SendWith` when files go along.
fn send_command(channel: u64, content: String, nonce: String, attachments: Vec<u64>) -> Command {
    if attachments.is_empty() {
        Command::Send { channel, content, nonce }
    } else {
        Command::SendWith { channel, content, nonce, attachments }
    }
}

fn fail(d: &mut DialogState, message: String) {
    d.error = Some(message);
}

/// Whether `content` contains an `@username` mention (case-insensitive).
pub fn mentions(content: &str, username: &str) -> bool {
    use thehideout_design::components::chat::body::{Block, Inline, parse};
    let want = format!("@{}", username.to_lowercase());
    parse(content).iter().any(|b| match b {
        Block::Text { text, marks } => marks
            .iter()
            .any(|&(a, z, k)| k == Inline::Mention && text[a..z].to_lowercase() == want),
        Block::Code { .. } => false,
    })
}

/// The stable element id of a message row: a sent message keeps the id of its pending self (its
/// nonce) so the row doesn't remount when the server confirms it.
pub fn chat_id_of(m: &Message, me: u64) -> String {
    if m.author_id == me && !m.nonce.is_empty() {
        format!("n:{}", m.nonce)
    } else {
        format!("m:{}", m.id)
    }
}

// ---- the account screens' own state --------------------------------------------------------------

/// The suspend dialog's durations: the label and the days (`None`: until an admin lifts it).
pub const SUSPEND_DURATIONS: [(&str, Option<u64>); 4] =
    [("1 day", Some(1)), ("7 days", Some(7)), ("30 days", Some(30)), ("Indefinitely", None)];

/// When a suspension picked from [`SUSPEND_DURATIONS`] ends, counted from `now`.
pub fn suspend_until(now: SystemTime, choice: usize) -> Option<SystemTime> {
    let days = SUSPEND_DURATIONS.get(choice).and_then(|d| d.1)?;
    Some(now + std::time::Duration::from_secs(days * 86_400))
}

/// The fields of the "Delete account" dialog (what is asked depends on the account: a password,
/// or an emailed code when it has none, plus the authenticator code when that is on).
pub struct DeleteDialog {
    pub password: TextEdit,
    pub email_code: TextEdit,
    pub totp: TextEdit,
}

/// An admin's question about one user.
pub enum AdminUserDialog {
    Suspend { user: u64, reason: TextEdit, duration: usize, open: bool },
    Delete { user: u64, reason: TextEdit, force: bool },
}

impl AdminUserDialog {
    pub fn user(&self) -> u64 {
        match self {
            AdminUserDialog::Suspend { user, .. } | AdminUserDialog::Delete { user, .. } => *user,
        }
    }
}

/// UI-only state of the account screens (the lifecycle's own state is in `lifecycle.rs`).
#[derive(Default)]
pub struct AccountUi {
    pub delete: Option<DeleteDialog>,
    /// The Users page's open actions menu (the user it belongs to).
    pub admin_menu: Option<u64>,
    pub admin_dialog: Option<AdminUserDialog>,
    /// The servers that stopped an admin's deletion of a user (the server named them).
    pub admin_owned: HashMap<u64, Vec<thehideout_api::OwnedGuild>>,
}

impl AppState {
    /// Whether the account has no password (it signs in with codes or passkeys), once the
    /// Security info says so.
    pub fn passwordless(&self) -> bool {
        self.ui.security.info.as_ref().is_some_and(|i| !i.summary.has_password)
    }

    pub fn has_totp(&self) -> bool {
        self.ui.security.totp == Some(true)
    }

    pub fn open_delete_account(&mut self) {
        let lc = &mut self.ui.lifecycle;
        lc.delete_error = None;
        lc.owned_guilds.clear();
        lc.deletion_code_sent = false;
        let mut password = TextEdit::new("").secure();
        password.focused = !self.passwordless();
        self.ui.account.delete = Some(DeleteDialog { password, email_code: TextEdit::new(""), totp: TextEdit::new("") });
    }

    pub fn close_delete_account(&mut self) {
        // A deletion in flight cannot be taken back: the dialog stays until it answers.
        if !self.ui.lifecycle.delete_busy {
            self.ui.account.delete = None;
        }
    }

    /// Everything the account asks for is filled in.
    pub fn delete_ready(&self) -> bool {
        let Some(d) = &self.ui.account.delete else { return false };
        let code = |e: &TextEdit| e.text.trim().chars().filter(char::is_ascii_digit).count() == 6;
        let proof = if self.passwordless() { code(&d.email_code) } else { !d.password.text.is_empty() };
        proof && (!self.has_totp() || code(&d.totp))
    }

    /// "Delete account" in the dialog.
    pub fn submit_delete_account(&mut self) {
        if !self.delete_ready() {
            return;
        }
        let Some(d) = &self.ui.account.delete else { return };
        let (password, code, totp) = if self.passwordless() {
            (String::new(), d.email_code.text.clone(), d.totp.text.clone())
        } else {
            (d.password.text.clone(), String::new(), d.totp.text.clone())
        };
        let totp = if self.has_totp() { totp } else { String::new() };
        self.delete_account(&password, &code, &totp);
    }

    /// A server named in the "you still own" list: leave the dialog for its Overview page, where
    /// the owner transfers or deletes it.
    pub fn open_owned_server(&mut self, guild: u64) {
        self.ui.account.delete = None;
        self.ui.lifecycle.owned_guilds.clear();
        self.ui.lifecycle.delete_error = None;
        self.open_server_settings(guild);
    }

    /// The live session was refused as suspended: back to the sign-in screen's suspended card.
    pub fn session_suspended(&mut self, reason: String, until: Option<SystemTime>) {
        let error = crate::forms::auth_error(&thehideout_api::ApiError::Suspended { reason, until });
        self.reset(Some(error));
        self.outbox.push(Command::SignOut);
    }

    /// "Back to sign in" on the suspended card.
    pub fn dismiss_suspension(&mut self) {
        self.auth.error = None;
    }

    // ---- admin: actions on one user --------------------------------------------------------------

    /// Admins and yourself are out of reach: the server refuses suspending or deleting them.
    pub fn admin_can_act_on(&self, user: u64) -> bool {
        user != self.my_id()
            && self.ui.admin.as_ref().and_then(|ad| ad.users.items.iter().find(|r| r.user.id == user)).is_some_and(|r| !r.user.is_admin && !r.deleted)
    }

    pub fn toggle_admin_menu(&mut self, user: u64) {
        let a = &mut self.ui.account;
        a.admin_menu = if a.admin_menu == Some(user) { None } else { Some(user) };
    }

    pub fn close_admin_menu(&mut self) {
        self.ui.account.admin_menu = None;
    }

    pub fn ask_admin_suspend(&mut self, user: u64) {
        if !self.admin_can_act_on(user) {
            return;
        }
        let mut reason = TextEdit::new("");
        reason.focused = true;
        self.ui.account.admin_menu = None;
        self.ui.account.admin_dialog = Some(AdminUserDialog::Suspend { user, reason, duration: 1, open: false });
    }

    pub fn ask_admin_delete(&mut self, user: u64) {
        if !self.admin_can_act_on(user) {
            return;
        }
        self.ui.account.admin_menu = None;
        self.ui.account.admin_owned.remove(&user);
        if let Some(ad) = self.ui.admin.as_mut() {
            ad.user_errors.remove(&user);
        }
        let mut reason = TextEdit::new("");
        reason.focused = true;
        self.ui.account.admin_dialog = Some(AdminUserDialog::Delete { user, reason, force: false });
    }

    pub fn cancel_admin_user_dialog(&mut self) {
        let user = self.ui.account.admin_dialog.as_ref().map(AdminUserDialog::user);
        if user.is_some_and(|u| self.ui.admin.as_ref().is_some_and(|ad| ad.busy_users.contains(&u))) {
            return;
        }
        self.ui.account.admin_dialog = None;
    }

    /// "Suspend" / "Delete user" in the dialog.
    pub fn submit_admin_user_dialog(&mut self) {
        let now = (self.now)();
        match &self.ui.account.admin_dialog {
            Some(AdminUserDialog::Suspend { user, reason, duration, .. }) => {
                let (user, reason, until) = (*user, reason.text.clone(), suspend_until(now, *duration));
                self.admin_suspend(user, &reason, until);
            }
            Some(AdminUserDialog::Delete { user, reason, force }) => {
                let (user, reason, force) = (*user, reason.text.clone(), *force);
                self.admin_delete_user(user, force, &reason);
            }
            None => {}
        }
    }

    /// The reply to a suspend or delete came back: a success closes the dialog, a refusal keeps it
    /// open with the row's error.
    pub(crate) fn admin_user_dialog_settled(&mut self, user: u64, ok: bool) {
        if ok && self.ui.account.admin_dialog.as_ref().is_some_and(|d| d.user() == user) {
            self.ui.account.admin_dialog = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_ignore_case_and_code() {
        assert!(mentions("hi @Tim, ok?", "tim"));
        assert!(!mentions("hi @timothy", "tim"));
        assert!(!mentions("run `@tim`", "tim"));
        assert!(!mentions("mail me@tim.com", "tim"));
    }

    static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn clock() -> SystemTime {
        std::time::UNIX_EPOCH
            + std::time::Duration::from_secs(CLOCK.load(std::sync::atomic::Ordering::SeqCst))
    }

    #[test]
    fn today_follows_the_wall_clock_across_local_midnight() {
        use crate::format::utc;
        // 23:59:50 on the 19th in UTC+1 is 22:59:50 UTC.
        CLOCK.store(
            utc(2026, 11, 19, 22, 59) as u64 + 50,
            std::sync::atomic::Ordering::SeqCst,
        );
        let mut s = AppState::new(clock, Tz::Fixed(3600));
        let before = s.today;
        let (changed, wake) = s.tick(100.0);
        assert!(!changed);
        assert_eq!(wake, Some(110.0), "woken at the turn of the day");
        CLOCK.store(
            utc(2026, 11, 19, 23, 0) as u64,
            std::sync::atomic::Ordering::SeqCst,
        );
        let (changed, _) = s.tick(110.0);
        assert!(changed);
        assert_eq!(s.today, before + 1);
    }

    #[test]
    fn sentences() {
        assert_eq!(sentence("username too short"), "Username too short.");
        assert_eq!(sentence("done!"), "Done!");
    }
}
