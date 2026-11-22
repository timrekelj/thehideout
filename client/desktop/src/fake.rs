//! A deterministic backend with seeded data: no network, no randomness, no wall clock. It
//! answers each command with the reply the real server would give (including its errors),
//! queued until the next poll. Scenes and tests also drive it directly: incoming events,
//! connection changes, failing sends.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, UNIX_EPOCH};

use thehideout_api::{
    ApiError, Channel, ChannelIcon, ChannelIconRef, ChannelKind, DmChannel, Emoji, Event, GatewayUpdate, Guild,
    GuildLayout, LayoutEntry, Member, Message, Page, PaletteColor, Permissions, Reaction,
    ReactionEmoji, Role, RoleList, Section, User, VoiceState,
};

use crate::backend::{Backend, Command, GuildAccess, Initial, Reply, Session};
use crate::fake_admin::AdminWorld;
use crate::fake_lines::{DEV_LINES, DM_ANA_LINES, DM_BRUNO_LINES, general_line};
use crate::format::{DAY, utc};
use crate::layout;

#[path = "fake_files.rs"]
mod files;

#[path = "fake_account.rs"]
pub mod account;

#[path = "fake_voice.rs"]
mod voice;

#[path = "fake_moderation.rs"]
mod moderation;
pub use voice::{FAKE_MIC_LEVEL, FakeAudio, fake_devices_event};

/// The bytes of a generated file (tests compare downloads with them).
#[cfg(test)]
pub fn files_fixture_bytes(key: &str) -> Vec<u8> {
    files::fixture_bytes(key).expect("a generated file")
}

const PAGE: usize = 50;
const MAX_ROLE_NAME: usize = crate::server_settings::MAX_ROLE_NAME;
const MAX_CHARS: usize = thehideout_api::MAX_MESSAGE_CHARS;

/// The instant the fake world calls "now": Thursday 19 Nov 2026, 11:30 UTC.
pub fn now_unix() -> i64 {
    utc(2026, 11, 19, 11, 30)
}

pub const HIDEOUT: u64 = 1;
pub const GENERAL: u64 = 10;
pub const DEV: u64 = 11;
pub const RANDOM: u64 = 12;
/// Always empty.
pub const PLANS: u64 = 13;
pub const ANNOUNCE: u64 = 14;
pub const DESIGN: u64 = 15;
pub const MUSIC: u64 = 16;
pub const GAMING: u64 = 17;
pub const MEMES: u64 = 18;
pub const BOOKS: u64 = 19;
/// The Hideout's voice channels: Ana and Bruno are in the first, the second is empty.
pub const LOUNGE: u64 = 22;
pub const GAME_NIGHT: u64 = 23;
pub const BUILDING: u64 = 101;
pub const HANGOUT: u64 = 102;
pub const ARCHIVE: u64 = 103;
/// The last section of The Hideout: its voice channels.
pub const VOICE: u64 = 104;
/// Tim holds this in Cooking Club (which Bruno owns): a non-owner who may manage channels.
pub const COOK_MODS: u64 = 3001;
/// Bruno holds this in The Hideout.
pub const HIDEOUT_MODS: u64 = 1001;
/// The Hideout's ADMINISTRATOR role (Cvetka), its pink Designer (Ana) and the one nobody has.
pub const HIDEOUT_ADMINS: u64 = 1002;
pub const HIDEOUT_DESIGNERS: u64 = 1003;
pub const HIDEOUT_MEMBERS: u64 = 1004;
/// Garage Band (Ana owns it): Tim is a Roadie there, who may manage roles below his own and nothing else.
pub const BAND_DRUMMER: u64 = 2003;
pub const BAND_ROADIE: u64 = 2001;
pub const BAND_GUEST: u64 = 2002;
pub const BAND_FAN: u64 = 2004;
/// The ids of The Hideout's custom emojis (the animated ones are `PARROT`, `DANCE_CAT` and `WOBBLE`).
pub const SMUG_CAT: u64 = 7001;
pub const PARROT: u64 = 7002;
pub const SHIPIT: u64 = 7003;
pub const BLOB_LOVE: u64 = 7004;
pub const DANCE_CAT: u64 = 7005;
pub const WOBBLE: u64 = 7006;
/// Garage Band's two (one animated).
pub const DRUM_SOLO: u64 = 7101;
pub const DM_ANA: u64 = 50;
pub const DM_BRUNO: u64 = 51;
pub const TIM: u64 = 1;
pub const ANA: u64 = 2;
pub const BRUNO: u64 = 3;
pub const CVETKA: u64 = 4;

fn time(unix: i64) -> std::time::SystemTime {
    UNIX_EPOCH + Duration::from_secs(unix.max(0) as u64)
}

fn user(id: u64, name: &str) -> User {
    User {
        deleted: false,
        id,
        username: name.into(),
        created_at: time(utc(2026, 1, 5, 12, 0)),
        avatar: None,
        is_admin: false,
        email: None,
    }
}

struct World {
    me: User,
    users: Vec<User>,
    guilds: Vec<Guild>,
    channels: Vec<Channel>,
    dms: Vec<DmChannel>,
    members: HashMap<u64, Vec<u64>>,
    messages: HashMap<u64, Vec<Message>>,
    next_id: u64,
    sent_at: i64,
    queue: Vec<(f64, Reply)>,
    now: f64,
    latency: f64,
    waker: Option<rune::Waker>,
    log: Vec<Command>,
    fail_sends: u32,
    layouts: HashMap<u64, GuildLayout>,
    roles: HashMap<u64, Vec<Role>>,
    /// Role ids per (guild, user).
    member_roles: HashMap<(u64, u64), Vec<u64>>,
    next_section: u64,
    /// The next `UpdateLayout` is refused as stale, and the server's version moves on (another
    /// client got there first).
    conflict_next: bool,
    /// Like `conflict_next`, for `ReorderRoles`.
    conflict_next_roles: bool,
    /// Each guild's roles version.
    role_versions: HashMap<u64, u64>,
    next_role: u64,
    /// Stored pictures by media key (uploads); `gen-N` keys are generated on demand.
    media: HashMap<String, Vec<u8>>,
    next_media: u64,
    /// Each guild's custom emojis; their pictures are `emoji-<id>` keys generated on demand.
    emojis: HashMap<u64, Vec<Emoji>>,
    next_emoji: u64,
    /// The next `n` reaction calls fail with a network error.
    fail_reactions: u32,
    /// Uploads, downloads and generated files.
    files: files::Files,
    /// The site-admin side (users, servers, files, activity, audit).
    admin: AdminWorld,
    /// Sign-in methods, invites, sessions and the Security page.
    account: account::Accounts,
    /// Voice channels' occupants.
    voice: voice::Voice,
    /// Bans and the other moderation state.
    moderation: moderation::Moderation,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

/// A tiny deterministic generator for message gaps.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below
    }
}

impl World {
    fn new() -> World {
        let mut users = vec![
            user(TIM, "tim"),
            user(ANA, "ana"),
            user(BRUNO, "bruno"),
            user(CVETKA, "cvetka"),
        ];
        // The one site admin among the four (sign in as cvetka to see the dashboard).
        users[CVETKA as usize - 1].is_admin = true;
        // Bruno's account predates emails (a legacy account); the others have one.
        for u in users.iter_mut().filter(|u| u.id != BRUNO) {
            u.email = Some(format!("{}@example.com", u.username));
        }
        let ch = |id: u64, guild: u64, position: i32, name: &str| Channel {
            id,
            guild_id: guild,
            name: name.into(),
            created_at: time(utc(2026, 1, 6, 9, 0)),
            section_id: None,
            position,
            icon: ChannelIcon::default(),
            icon_ref: ChannelIconRef::Default,
            color: PaletteColor::None,
            topic: String::new(),
            kind: ChannelKind::Text,
        };
        let look = |mut c: Channel, icon: &str, color: PaletteColor, topic: &str| {
            c.icon = ChannelIcon::new(icon);
            c.icon_ref = ChannelIconRef::from(&c.icon);
            c.color = color;
            c.topic = topic.into();
            c
        };
        let guild = |id: u64, name: &str, owner: u64, code: &str| Guild {
            id,
            name: name.into(),
            owner_id: owner,
            invite_code: code.into(),
            created_at: time(utc(2026, 1, 6, 9, 0)),
            icon: None,
        };
        let dm = |id: u64, a: u64, b: u64| DmChannel {
            id,
            participants: vec![users[a as usize - 1].clone(), users[b as usize - 1].clone()],
            created_at: time(utc(2026, 3, 2, 18, 0)),
        };
        let mut f = World {
            me: users[0].clone(),
            guilds: vec![
                guild(HIDEOUT, "The Hideout", TIM, "HIDEOUT1"),
                guild(2, "Garage Band", ANA, "GARAGE22"),
                guild(3, "Cooking Club", BRUNO, "COOK333"),
            ],
            channels: vec![
                look(
                    ch(GENERAL, 1, 0, "general"),
                    "chat-circle-dots",
                    PaletteColor::Teal,
                    "Say hi, share links, plan the weekend",
                ),
                look(
                    ch(DEV, 1, 1, "dev"),
                    "code",
                    PaletteColor::Blue,
                    "Builds, reviews and the odd rant about lifetimes",
                ),
                look(
                    ch(RANDOM, 1, 2, "random"),
                    "dice-five",
                    PaletteColor::Amber,
                    "",
                ),
                look(
                    ch(PLANS, 1, 3, "plans"),
                    "calendar-check",
                    PaletteColor::Green,
                    "",
                ),
                look(
                    ch(ANNOUNCE, 1, 4, "announcements"),
                    "megaphone",
                    PaletteColor::Orange,
                    "Read-only news from the mods",
                ),
                look(
                    ch(DESIGN, 1, 5, "design"),
                    "paint-brush",
                    PaletteColor::Pink,
                    "",
                ),
                look(
                    ch(MUSIC, 1, 6, "music"),
                    "music-notes",
                    PaletteColor::Indigo,
                    "",
                ),
                look(
                    ch(GAMING, 1, 7, "gaming"),
                    "game-controller",
                    PaletteColor::Red,
                    "",
                ),
                look(ch(MEMES, 1, 8, "memes"), "smiley", PaletteColor::Yellow, ""),
                look(ch(BOOKS, 1, 9, "books"), "books", PaletteColor::Slate, ""),
                ch(20, 2, 0, "general"),
                ch(21, 2, 1, "gigs"),
                ch(30, 3, 0, "recipes"),
                ch(31, 3, 1, "shopping"),
            ],
            dms: vec![dm(DM_ANA, TIM, ANA), dm(DM_BRUNO, TIM, BRUNO)],
            members: HashMap::from([
                (1, vec![TIM, ANA, BRUNO, CVETKA]),
                (2, vec![TIM, ANA, CVETKA]),
                (3, vec![TIM, BRUNO]),
            ]),
            users,
            messages: HashMap::new(),
            next_id: 10_000,
            sent_at: now_unix(),
            queue: Vec::new(),
            now: 0.0,
            latency: 0.0,
            waker: None,
            log: Vec::new(),
            fail_sends: 0,
            layouts: HashMap::new(),
            roles: HashMap::new(),
            member_roles: HashMap::new(),
            next_section: 200,
            conflict_next: false,
            conflict_next_roles: false,
            role_versions: HashMap::from([(1, 4), (2, 2), (3, 1)]),
            next_role: 5000,
            media: HashMap::new(),
            next_media: 0,
            emojis: HashMap::new(),
            next_emoji: 7200,
            fail_reactions: 0,
            files: files::Files::default(),
            admin: AdminWorld::empty(),
            account: account::Accounts::new(),
            voice: voice::Voice::default(),
            moderation: moderation::Moderation::default(),
        };
        f.admin = AdminWorld::new(&f.users, &f.guilds);
        f.seed_structure();
        f.seed();
        f.seed_emojis();
        f.seed_voice();
        f
    }

    /// Sections, layouts and roles: Tim owns The Hideout, is a plain member of Garage Band and a
    /// moderator (not owner) of Cooking Club.
    fn seed_structure(&mut self) {
        let section = |id: u64, name: &str, position: i32| Section {
            id,
            guild_id: 1,
            name: name.into(),
            position,
        };
        self.layouts.insert(
            1,
            GuildLayout {
                version: 3,
                sections: vec![
                    section(BUILDING, "Building", 0),
                    section(HANGOUT, "Hangout", 1),
                    section(ARCHIVE, "Archive", 2),
                ],
                entries: vec![
                    LayoutEntry {
                        section_id: None,
                        channel_ids: vec![GENERAL, DEV, ANNOUNCE],
                    },
                    LayoutEntry {
                        section_id: Some(BUILDING),
                        channel_ids: vec![DESIGN, PLANS],
                    },
                    LayoutEntry {
                        section_id: Some(HANGOUT),
                        channel_ids: vec![RANDOM, MUSIC, GAMING, MEMES],
                    },
                    LayoutEntry {
                        section_id: Some(ARCHIVE),
                        channel_ids: vec![BOOKS],
                    },
                ],
            },
        );
        for (g, ids) in [(2, vec![20, 21]), (3, vec![30, 31])] {
            self.layouts.insert(
                g,
                GuildLayout {
                    version: 1,
                    sections: vec![],
                    entries: vec![LayoutEntry {
                        section_id: None,
                        channel_ids: ids,
                    }],
                },
            );
        }
        let role = |id: u64,
                    guild: u64,
                    name: &str,
                    color: PaletteColor,
                    position: i32,
                    permissions: Permissions| Role {
            id,
            guild_id: guild,
            name: name.into(),
            color,
            position,
            permissions,
            created_at: time(utc(2026, 1, 6, 9, 0)),
        };
        use PaletteColor as C;
        self.roles.insert(
            1,
            vec![
                role(
                    HIDEOUT_ADMINS,
                    1,
                    "Admin",
                    C::Red,
                    3,
                    Permissions::ADMINISTRATOR,
                ),
                role(
                    HIDEOUT_MODS,
                    1,
                    "Moderator",
                    C::Teal,
                    2,
                    Permissions::MANAGE_CHANNELS,
                ),
                role(
                    HIDEOUT_DESIGNERS,
                    1,
                    "Designer",
                    C::Pink,
                    1,
                    Permissions::NONE,
                ),
                role(HIDEOUT_MEMBERS, 1, "Regular", C::None, 0, Permissions::NONE),
            ],
        );
        self.roles.insert(
            2,
            vec![
                role(BAND_DRUMMER, 2, "Drummer", C::Orange, 3, Permissions::NONE),
                role(
                    BAND_ROADIE,
                    2,
                    "Roadie",
                    C::Blue,
                    2,
                    Permissions::MANAGE_ROLES | Permissions::MANAGE_EMOJIS,
                ),
                role(BAND_GUEST, 2, "Guest", C::Slate, 1, Permissions::NONE),
                role(BAND_FAN, 2, "Fan", C::None, 0, Permissions::NONE),
            ],
        );
        self.roles.insert(
            3,
            vec![role(
                COOK_MODS,
                3,
                "Moderator",
                C::Teal,
                0,
                Permissions::MANAGE_CHANNELS,
            )],
        );
        self.member_roles.insert((1, BRUNO), vec![HIDEOUT_MODS]);
        self.member_roles.insert((1, ANA), vec![HIDEOUT_DESIGNERS]);
        self.member_roles.insert((1, CVETKA), vec![HIDEOUT_ADMINS]);
        self.member_roles.insert((2, TIM), vec![BAND_ROADIE]);
        self.member_roles.insert((2, CVETKA), vec![BAND_DRUMMER]);
        self.member_roles.insert((3, TIM), vec![COOK_MODS]);
        for g in [1, 2, 3] {
            self.sync_channels(g);
        }
    }

    /// Custom emojis, messages that use them and reactions: #memes shows them all, and the last
    /// messages of #general carry a few reactions.
    fn seed_emojis(&mut self) {
        let emoji = |id: u64, guild: u64, name: &str, animated: bool, by: u64| Emoji {
            id,
            guild_id: guild,
            name: name.into(),
            animated,
            image: format!("emoji-{id}"),
            creator_id: by,
            created_at: time(utc(2026, 2, 3, 10, 0) + id as i64 * 60),
        };
        self.emojis.insert(
            HIDEOUT,
            vec![
                emoji(BLOB_LOVE, HIDEOUT, "blob_love", false, ANA),
                emoji(DANCE_CAT, HIDEOUT, "dance_cat", true, BRUNO),
                emoji(PARROT, HIDEOUT, "party_parrot", true, TIM),
                emoji(SHIPIT, HIDEOUT, "shipit", false, TIM),
                emoji(SMUG_CAT, HIDEOUT, "smug_cat", false, CVETKA),
                emoji(WOBBLE, HIDEOUT, "wobble", true, ANA),
            ],
        );
        self.emojis
            .insert(2, vec![emoji(DRUM_SOLO, 2, "drum_solo", false, ANA)]);
        let mut at = now_unix() - 3 * 3600;
        let mut say = |f: &mut World, who: u64, text: &str| {
            at += 40;
            f.push(MEMES, who, at, text);
        };
        say(self, ANA, "okay but the new emoji pack <:blob_love:7004>");
        say(
            self,
            BRUNO,
            "look at this guy <a:dance_cat:7005> and this one <:smug_cat:7001>",
        );
        say(self, TIM, "shipping the release tonight <:shipit:7003>");
        say(self, CVETKA, "<a:party_parrot:7002>");
        say(
            self,
            ANA,
            "<:smug_cat:7001> <:blob_love:7004> <:shipit:7003>",
        );
        say(self, BRUNO, "\u{1f389}\u{1f389}\u{1f389}");
        say(
            self,
            TIM,
            "unknown ones stay text: <:gone:99999> and a flag \u{1f1f8}\u{1f1ee}",
        );
        say(
            self,
            CVETKA,
            "**bold** with <:smug_cat:7001> inline, and a long enough line to wrap over to the next row so you can see how pictures sit in the text flow",
        );
        let unicode = |e: &str| ReactionEmoji::Unicode(e.into());
        let r = |emoji: ReactionEmoji, count: u32, me: bool| Reaction { emoji, count, me };
        let memes = self.messages.entry(MEMES).or_default();
        let n = memes.len();
        memes[n - 8].reactions = vec![
            r(unicode("\u{2764}\u{fe0f}"), 3, true),
            r(ReactionEmoji::Custom(BLOB_LOVE), 2, false),
            r(unicode("\u{1f44d}"), 1, false),
        ];
        memes[n - 7].reactions = vec![
            r(ReactionEmoji::Custom(DANCE_CAT), 4, true),
            r(ReactionEmoji::Custom(SMUG_CAT), 1, false),
            r(unicode("\u{1f602}"), 6, false),
        ];
        memes[n - 6].reactions = vec![
            r(unicode("\u{1f680}"), 2, false),
            r(ReactionEmoji::Custom(SHIPIT), 5, true),
        ];
        memes[n - 5].reactions = vec![
            r(ReactionEmoji::Custom(PARROT), 12, false),
            r(unicode("\u{1f389}"), 7, true),
        ];
        let general = self.messages.entry(GENERAL).or_default();
        let n = general.len();
        general[n - 3].reactions = vec![
            r(unicode("\u{1f44d}"), 2, false),
            r(unicode("\u{1f440}"), 1, true),
        ];
        general[n - 5].reactions = vec![r(unicode("\u{1f602}"), 3, false)];
    }

    fn emojis_sorted(&self, guild: u64) -> Vec<Emoji> {
        let mut list = self.emojis.get(&guild).cloned().unwrap_or_default();
        list.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        list
    }

    fn emojis_event(&self, guild: u64) -> Reply {
        Reply::Gateway(GatewayUpdate::Event(Event::EmojisUpdated {
            guild_id: guild,
            emojis: self.emojis_sorted(guild),
        }))
    }

    /// What the server checks when an emoji is added: permission, name, the 200 cap, the size, the format.
    fn create_emoji(
        &mut self,
        guild: u64,
        me: u64,
        name: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<Emoji, ApiError> {
        if !self.has_perm(guild, me, Permissions::MANAGE_EMOJIS) {
            return Err(ApiError::PermissionDenied);
        }
        if !crate::emoji::valid_name(name) {
            return Err(ApiError::InvalidArgument(
                "emoji name must match [a-z0-9_]{2,32}".into(),
            ));
        }
        if bytes.len() > thehideout_api::MAX_EMOJI_BYTES {
            return Err(ApiError::TooLarge);
        }
        check_image(&bytes, content_type)?;
        let list = self.emojis.entry(guild).or_default();
        if list.len() >= crate::emoji::MAX_EMOJIS {
            return Err(ApiError::Precondition(
                "a server can have at most 200 emojis".into(),
            ));
        }
        if list.iter().any(|e| e.name == name) {
            return Err(ApiError::AlreadyExists);
        }
        self.next_emoji += 1;
        self.next_media += 1;
        let (id, key) = (self.next_emoji, format!("up-{}", self.next_media));
        let animated = bytes.starts_with(b"GIF8");
        self.media.insert(key.clone(), bytes);
        let e = Emoji {
            id,
            guild_id: guild,
            name: name.into(),
            animated,
            image: key,
            creator_id: me,
            created_at: time(now_unix()),
        };
        self.emojis.entry(guild).or_default().push(e.clone());
        Ok(e)
    }

    fn rename_emoji(
        &mut self,
        guild: u64,
        me: u64,
        emoji: u64,
        name: &str,
    ) -> Result<Emoji, ApiError> {
        if !self.has_perm(guild, me, Permissions::MANAGE_EMOJIS) {
            return Err(ApiError::PermissionDenied);
        }
        if !crate::emoji::valid_name(name) {
            return Err(ApiError::InvalidArgument(
                "emoji name must match [a-z0-9_]{2,32}".into(),
            ));
        }
        let list = self.emojis.entry(guild).or_default();
        if list.iter().any(|e| e.id != emoji && e.name == name) {
            return Err(ApiError::AlreadyExists);
        }
        let e = list
            .iter_mut()
            .find(|e| e.id == emoji)
            .ok_or(ApiError::NotFound)?;
        e.name = name.into();
        Ok(e.clone())
    }

    /// Adds or removes `user`'s reaction like the server: idempotent, at most 20 distinct emojis
    /// a message, a custom emoji must belong to the channel's guild (and none in a DM).
    fn react(
        &mut self,
        channel: u64,
        message: u64,
        user: u64,
        emoji: &ReactionEmoji,
        add: bool,
    ) -> Result<(), ApiError> {
        let guild = self
            .channels
            .iter()
            .find(|c| c.id == channel)
            .map(|c| c.guild_id);
        if let ReactionEmoji::Custom(id) = emoji
            && !guild.is_some_and(|g| {
                self.emojis
                    .get(&g)
                    .is_some_and(|l| l.iter().any(|e| e.id == *id))
            })
        {
            return Err(ApiError::InvalidArgument(
                "that emoji can't be used in this channel".into(),
            ));
        }
        let m = self
            .messages
            .get_mut(&channel)
            .and_then(|v| v.iter_mut().find(|m| m.id == message))
            .ok_or(ApiError::NotFound)?;
        let mine = user == self.me.id;
        let at = m.reactions.iter().position(|r| &r.emoji == emoji);
        match (at, add) {
            (Some(i), true) => {
                if !(mine && m.reactions[i].me) {
                    m.reactions[i].count += 1;
                    m.reactions[i].me |= mine;
                }
            }
            (None, true) => {
                if m.reactions.len() >= thehideout_design::components::MAX_REACTIONS {
                    return Err(ApiError::Precondition(
                        "a message can have at most 20 different reactions".into(),
                    ));
                }
                m.reactions.push(Reaction {
                    emoji: emoji.clone(),
                    count: 1,
                    me: mine,
                });
            }
            (Some(i), false) => {
                if !mine || m.reactions[i].me {
                    m.reactions[i].count = m.reactions[i].count.saturating_sub(1);
                    m.reactions[i].me &= !mine;
                    if m.reactions[i].count == 0 {
                        m.reactions.remove(i);
                    }
                }
            }
            (None, false) => {}
        }
        Ok(())
    }

    /// Channels carry their place too; keep it in step with the layout.
    fn sync_channels(&mut self, guild: u64) {
        let Some(l) = self.layouts.get(&guild) else {
            return;
        };
        for e in &l.entries {
            for (i, id) in e.channel_ids.iter().enumerate() {
                if let Some(c) = self.channels.iter_mut().find(|c| c.id == *id) {
                    c.section_id = e.section_id;
                    c.position = i as i32;
                }
            }
        }
    }

    /// The server's `Standing`: (owner, effective permissions, position of the highest role).
    fn standing(&self, guild: u64, user: u64) -> (bool, Permissions, Option<i32>) {
        let owner = self
            .guilds
            .iter()
            .find(|g| g.id == guild)
            .map(|g| g.owner_id)
            == Some(user);
        let mine = self
            .member_roles
            .get(&(guild, user))
            .map_or(&[][..], Vec::as_slice);
        let held: Vec<&Role> = self.roles.get(&guild).map_or(Vec::new(), |rs| {
            rs.iter().filter(|r| mine.contains(&r.id)).collect()
        });
        let perms = held
            .iter()
            .fold(Permissions::NONE, |p, r| p | r.permissions);
        let perms = if owner || perms.contains(Permissions::ADMINISTRATOR) {
            crate::perms::ALL
        } else {
            perms | crate::perms::BASELINE
        };
        (owner, perms, held.iter().map(|r| r.position).max())
    }

    fn has_perm(&self, guild: u64, user: u64, bit: Permissions) -> bool {
        self.standing(guild, user).1.contains(bit)
    }

    fn can_manage(&self, guild: u64, user: u64) -> bool {
        self.has_perm(guild, user, Permissions::MANAGE_CHANNELS)
    }

    fn outranks(&self, guild: u64, user: u64, position: i32) -> bool {
        let (owner, _, top) = self.standing(guild, user);
        owner || top.is_some_and(|t| position < t)
    }

    fn holds_all(&self, guild: u64, user: u64, bits: u64) -> bool {
        let (owner, perms, _) = self.standing(guild, user);
        owner || perms.bits() & bits == bits
    }

    fn roles_event(&self, guild: u64) -> Reply {
        let roles = self.roles.get(&guild).cloned().unwrap_or_default();
        let version = self.role_versions.get(&guild).copied().unwrap_or(1);
        Reply::Gateway(GatewayUpdate::Event(Event::RolesUpdated {
            guild_id: guild,
            roles,
            version,
        }))
    }

    fn member_of(&self, guild: u64, user: u64) -> Option<Member> {
        let u = self.users.iter().find(|u| u.id == user)?.clone();
        let role_ids = self
            .member_roles
            .get(&(guild, user))
            .cloned()
            .unwrap_or_default();
        Some(Member {
            user: u,
            role_ids,
            joined_at: time(utc(2026, 1, 6, 9, 0)),
        })
    }

    fn member_event(&self, guild: u64, user: u64) -> Option<Reply> {
        self.member_of(guild, user).map(|member| {
            Reply::Gateway(GatewayUpdate::Event(Event::MemberUpdated {
                guild_id: guild,
                member,
            }))
        })
    }

    /// A guild's roles most senior first.
    fn roles_sorted(&self, guild: u64) -> Vec<Role> {
        crate::server_settings::sorted(self.roles.get(&guild).cloned().unwrap_or_default())
    }

    fn set_user_avatar(&mut self, id: u64, key: Option<String>) -> Option<User> {
        let u = self.users.iter_mut().find(|u| u.id == id)?;
        u.avatar = key;
        let u = u.clone();
        if self.me.id == id {
            self.me = u.clone();
        }
        for dm in &mut self.dms {
            for p in dm.participants.iter_mut().filter(|p| p.id == id) {
                *p = u.clone();
            }
        }
        Some(u)
    }

    /// Bumps the layout version, as the server's `publish_layout` does. The caller queues the
    /// `LayoutUpdated` event (`layout_event`) after its own reply.
    fn publish_layout(&mut self, guild: u64) -> GuildLayout {
        self.sync_channels(guild);
        let l = self.layouts.entry(guild).or_default();
        l.version += 1;
        l.clone()
    }

    fn layout_event(&self, guild: u64) -> Reply {
        Reply::Gateway(GatewayUpdate::Event(Event::LayoutUpdated {
            guild_id: guild,
            layout: self.layouts.get(&guild).cloned().unwrap_or_default(),
        }))
    }

    fn push(&mut self, channel: u64, author: u64, at: i64, content: &str) {
        self.next_id += 1;
        let guild_id = self
            .channels
            .iter()
            .find(|c| c.id == channel)
            .map(|c| c.guild_id);
        let m = Message {
            id: self.next_id,
            channel_id: channel,
            guild_id,
            author_id: author,
            content: content.into(),
            created_at: time(at),
            edited_at: None,
            nonce: String::new(),
            reactions: vec![],
            attachments: vec![],
            embeds: vec![],
        };
        self.messages.entry(channel).or_default().push(m);
    }

    fn seed(&mut self) {
        // #general: 150 messages over four days, authors in runs so groups form.
        const WHO: [u64; 12] = [
            ANA, ANA, BRUNO, TIM, TIM, TIM, CVETKA, ANA, BRUNO, BRUNO, TIM, CVETKA,
        ];
        let mut rng = Lcg(7);
        // Gaps first, so the last day can be laid out backwards and end just before "now".
        let day_of = |i: usize| match i {
            0..40 => -3,
            40..82 => -2,
            82..120 => -1,
            _ => 0,
        };
        let authors: Vec<u64> = (0..150).map(|i| WHO[i % WHO.len()]).collect();
        let gaps: Vec<i64> = (0..150)
            .map(|i| {
                if i % 17 == 0 {
                    5400
                } else if i > 0 && authors[i] == authors[i - 1] {
                    20 + rng.next(130) as i64
                } else {
                    70 + rng.next(800) as i64
                }
            })
            .collect();
        let mut t = 0;
        for i in 0..150usize {
            let d = day_of(i);
            if i == 0 || day_of(i - 1) != d {
                let span: i64 = (i + 1..150)
                    .take_while(|&j| day_of(j) == d)
                    .map(|j| gaps[j])
                    .sum();
                t = if d == 0 {
                    now_unix() - 18 * 60 - span
                } else {
                    utc(2026, 11, 19, 8, 40) + d * DAY + (i as i64 % 7) * 180
                };
            } else {
                t += gaps[i];
            }
            self.push(GENERAL, authors[i], t, general_line(i));
        }
        for (i, line) in DEV_LINES.iter().enumerate() {
            let author = [BRUNO, TIM, ANA][i % 3];
            self.push(DEV, author, utc(2026, 11, 18, 14, 0) + i as i64 * 400, line);
        }
        self.push(
            RANDOM,
            CVETKA,
            utc(2026, 11, 17, 20, 12),
            "found this at the flea market, 5 euros",
        );
        for (i, (who, line)) in DM_ANA_LINES.iter().enumerate() {
            self.push(DM_ANA, *who, utc(2026, 11, 18, 21, 0) + i as i64 * 90, line);
        }
        for (i, (who, line)) in DM_BRUNO_LINES.iter().enumerate() {
            self.push(
                DM_BRUNO,
                *who,
                utc(2026, 11, 17, 19, 30) + i as i64 * 120,
                line,
            );
        }
        self.push(
            20,
            ANA,
            utc(2026, 11, 16, 18, 0),
            "rehearsal moved to Thursday",
        );
        self.push(
            30,
            BRUNO,
            utc(2026, 11, 15, 10, 0),
            "sourdough starter is alive",
        );
    }

    // ---- knobs for scenes and tests ----------------------------------------------------------

    /// Seconds each reply waits before it is due (default 0: due at the next poll).
    fn set_latency(&mut self, secs: f64) {
        self.latency = secs;
    }

    /// The next `n` sends are refused with a network error.
    #[cfg_attr(not(test), allow(dead_code))]
    fn set_fail_sends(&mut self, n: u32) {
        self.fail_sends = n;
    }

    /// The next `UpdateLayout` is refused as stale (another client edited first).
    #[cfg_attr(not(test), allow(dead_code))]
    fn set_conflict_next(&mut self) {
        self.conflict_next = true;
    }

    /// `user` gains (or loses) the moderator role of `guild`, announced as the gateway would.
    fn set_moderator(&mut self, guild: u64, user: u64, on: bool) {
        let role = self
            .roles
            .get(&guild)
            .and_then(|r| r.iter().find(|r| r.name == "Moderator"))
            .map(|r| r.id);
        let Some(role) = role else { return };
        let ids = self.member_roles.entry((guild, user)).or_default();
        ids.retain(|r| *r != role);
        if on {
            ids.push(role);
        }
        if let Some(e) = self.member_event(guild, user) {
            self.enqueue(e);
        }
    }

    /// The next `ReorderRoles` is refused as stale (another client reordered first).
    #[cfg_attr(not(test), allow(dead_code))]
    fn set_conflict_next_roles(&mut self) {
        self.conflict_next_roles = true;
    }

    /// Every fake user gets a generated picture, announced as the gateway would.
    fn give_avatars(&mut self) {
        for id in [TIM, ANA, BRUNO, CVETKA] {
            if let Some(u) = self.set_user_avatar(id, Some(format!("gen-{id}"))) {
                self.enqueue(Reply::Gateway(GatewayUpdate::Event(Event::UserUpdated(u))));
            }
        }
    }

    /// `guild` gets a generated icon, announced as the gateway would.
    fn give_icon(&mut self, guild: u64) {
        if let Some(g) = self.guilds.iter_mut().find(|g| g.id == guild) {
            g.icon = Some(format!("gen-{}", 900 + guild));
            let g = g.clone();
            self.enqueue(Reply::Gateway(GatewayUpdate::Event(Event::GuildUpdated(g))));
        }
    }

    /// `author` posts `content` in `channel`, as the gateway would deliver it.
    fn incoming(&mut self, channel: u64, author: u64, content: &str) {
        self.sent_at += 20;
        self.push(channel, author, self.sent_at, content);
        let m = self.messages[&channel].last().cloned().unwrap();
        self.enqueue(Reply::Gateway(GatewayUpdate::Event(Event::MessageCreated(
            m,
        ))));
    }

    /// Like `incoming`, but the gateway never delivers it: the client is away and has to fetch it.
    fn missed(&mut self, channel: u64, author: u64, content: &str) {
        self.sent_at += 20;
        self.push(channel, author, self.sent_at, content);
    }

    fn gateway(&mut self, update: GatewayUpdate) {
        self.enqueue(Reply::Gateway(update));
    }

    fn disconnect(&mut self, retry_secs: u64) {
        self.gateway(GatewayUpdate::Disconnected {
            retry_in: Duration::from_secs(retry_secs),
            reason: "simulated".into(),
        });
    }

    /// One retry that fails: the real gateway announces `Connecting` before each attempt.
    fn failed_retry(&mut self, retry_secs: u64) {
        self.gateway(GatewayUpdate::Connecting);
        self.disconnect(retry_secs);
    }

    /// One retry that succeeds.
    fn reconnect(&mut self) {
        self.gateway(GatewayUpdate::Connecting);
        self.gateway(GatewayUpdate::Connected);
        if let Some(snapshot) = self.voice_snapshot() {
            self.enqueue(snapshot);
        }
    }

    fn initial(&self) -> Initial {
        let mine: Vec<Guild> = self
            .guilds
            .iter()
            .filter(|g| {
                self.members
                    .get(&g.id)
                    .is_some_and(|m| m.contains(&self.me.id))
            })
            .cloned()
            .collect();
        let channels = self
            .channels
            .iter()
            .filter(|c| mine.iter().any(|g| g.id == c.guild_id))
            .cloned()
            .collect();
        let dms = self
            .dms
            .iter()
            .filter(|d| d.participants.iter().any(|p| p.id == self.me.id))
            .cloned()
            .collect();
        let layouts = mine
            .iter()
            .filter_map(|g| self.layouts.get(&g.id).map(|l| (g.id, l.clone())))
            .collect();
        Initial {
            me: self.me.clone(),
            guilds: mine,
            channels,
            layouts,
            dms,
        }
    }

    fn enqueue(&mut self, reply: Reply) {
        self.queue.push((self.now + self.latency, reply));
        if let Some(w) = &self.waker {
            w();
        }
    }

    // ---- commands ----------------------------------------------------------------------------

    fn find_user(&self, name: &str) -> Option<&User> {
        self.users
            .iter()
            .find(|u| u.username.eq_ignore_ascii_case(name))
    }

    fn session_for(&mut self, username: &str) -> Session {
        let me = match self.find_user(username) {
            Some(u) => u.clone(),
            None => {
                let u = user(100 + self.users.len() as u64, username);
                self.users.push(u.clone());
                u
            }
        };
        self.me = me.clone();
        Session {
            token: format!("fake-token-{}", me.id),
            me,
        }
    }

    fn page(&self, channel: u64, before: Option<u64>) -> Page<Message> {
        let all = self.messages.get(&channel).map_or(&[][..], Vec::as_slice);
        let end = before.map_or(all.len(), |b| all.partition_point(|m| m.id < b));
        let start = end.saturating_sub(PAGE);
        let items: Vec<Message> = all[start..end].iter().rev().cloned().collect();
        let has_more = start > 0;
        Page {
            next_before: has_more.then(|| items.last().map(|m| m.id)).flatten(),
            items,
            has_more,
        }
    }

    fn handle(&mut self, cmd: Command) -> Vec<Reply> {
        let cmd = match self.files_command(cmd) {
            Ok(replies) => return replies,
            Err(cmd) => cmd,
        };
        let cmd = match self.account_command(cmd) {
            Ok(replies) => return replies,
            Err(cmd) => cmd,
        };
        let cmd = match self.voice_command(cmd) {
            Ok(replies) => return replies,
            Err(cmd) => cmd,
        };
        let cmd = match self.moderation_command(cmd) {
            Ok(replies) => return replies,
            Err(cmd) => cmd,
        };
        let me = self.me.id;
        match cmd {
            Command::Resume => vec![Reply::Resume(Ok(None))],
            Command::LoadInitial => {
                let mut out = vec![
                    Reply::Initial(Ok(self.initial())),
                    Reply::Gateway(GatewayUpdate::Connected),
                ];
                // A new stream starts with the voice snapshot, when anyone is in voice.
                out.extend(self.voice_snapshot());
                out
            }
            Command::Refresh => vec![Reply::Refreshed(Ok(self.initial()))],
            Command::LoadChannels { guild } => {
                let channels = self
                    .channels
                    .iter()
                    .filter(|c| c.guild_id == guild)
                    .cloned()
                    .collect();
                let layout = self.layouts.get(&guild).cloned().unwrap_or_default();
                vec![Reply::Channels {
                    guild,
                    result: Ok((channels, layout)),
                }]
            }
            Command::LoadAccess { guild } => {
                let roles = self.roles_sorted(guild);
                let version = self.role_versions.get(&guild).copied().unwrap_or(1);
                let member_roles = self
                    .member_roles
                    .iter()
                    .filter(|((g, _), _)| *g == guild)
                    .map(|((_, u), r)| (*u, r.clone()))
                    .collect();
                vec![Reply::Access {
                    guild,
                    result: Ok(GuildAccess {
                        roles,
                        version,
                        member_roles,
                    }),
                }]
            }
            Command::CreateSection { guild, name } => {
                let r = if !self.can_manage(guild, me) {
                    Err(ApiError::PermissionDenied)
                } else {
                    self.next_section += 1;
                    let section = Section {
                        id: self.next_section,
                        guild_id: guild,
                        name,
                        position: 0,
                    };
                    let l = self.layouts.entry(guild).or_default();
                    l.sections.push(section.clone());
                    l.entries.push(LayoutEntry {
                        section_id: Some(section.id),
                        channel_ids: vec![],
                    });
                    let l = self.publish_layout(guild);
                    Ok((section, l))
                };
                let echo = r.is_ok().then(|| self.layout_event(guild));
                std::iter::once(Reply::SectionSaved { guild, result: r })
                    .chain(echo)
                    .collect()
            }
            Command::RenameSection {
                guild,
                section,
                name,
            } => {
                let r = if !self.can_manage(guild, me) {
                    Err(ApiError::PermissionDenied)
                } else {
                    match self
                        .layouts
                        .get_mut(&guild)
                        .and_then(|l| l.sections.iter_mut().find(|s| s.id == section))
                    {
                        None => Err(ApiError::NotFound),
                        Some(s) => {
                            s.name = name;
                            let s = s.clone();
                            Ok((s, self.publish_layout(guild)))
                        }
                    }
                };
                let echo = r.is_ok().then(|| self.layout_event(guild));
                std::iter::once(Reply::SectionSaved { guild, result: r })
                    .chain(echo)
                    .collect()
            }
            Command::DeleteSection { guild, section } => {
                let r = if !self.can_manage(guild, me) {
                    Err(ApiError::PermissionDenied)
                } else if !self
                    .layouts
                    .get(&guild)
                    .is_some_and(|l| l.sections.iter().any(|s| s.id == section))
                {
                    Err(ApiError::NotFound)
                } else {
                    let l = self.layouts.get_mut(&guild).unwrap();
                    l.sections.retain(|s| s.id != section);
                    let moved = l
                        .entries
                        .iter()
                        .find(|e| e.section_id == Some(section))
                        .map(|e| e.channel_ids.clone())
                        .unwrap_or_default();
                    l.entries.retain(|e| e.section_id != Some(section));
                    if let Some(top) = l.entries.iter_mut().find(|e| e.section_id.is_none()) {
                        top.channel_ids.extend(moved);
                    }
                    Ok(self.publish_layout(guild))
                };
                let echo = r.is_ok().then(|| self.layout_event(guild));
                std::iter::once(Reply::SectionDeleted { guild, result: r })
                    .chain(echo)
                    .collect()
            }
            Command::UpdateLayout {
                guild,
                layout: want,
            } => {
                let result = if !self.can_manage(guild, me) {
                    Err(ApiError::PermissionDenied)
                } else if std::mem::take(&mut self.conflict_next) {
                    // Someone else's edit lands first.
                    self.publish_layout(guild);
                    Err(ApiError::Conflict)
                } else {
                    let cur = self.layouts.get(&guild).cloned().unwrap_or_default();
                    let sorted = |v: Vec<u64>| {
                        let mut v = v;
                        v.sort_unstable();
                        v
                    };
                    let (a, b) = (layout::normalized(&cur), layout::normalized(&want));
                    if want.version != cur.version {
                        Err(ApiError::Conflict)
                    } else if sorted(a.sections.iter().map(|s| s.id).collect())
                        != sorted(b.sections.iter().map(|s| s.id).collect())
                        || sorted(layout::channel_order(&a)) != sorted(layout::channel_order(&b))
                    {
                        Err(ApiError::InvalidArgument(
                            "layout is not a permutation of the current one".into(),
                        ))
                    } else {
                        let l = self.layouts.get_mut(&guild).unwrap();
                        l.sections = b.sections.clone();
                        l.entries = b.entries;
                        Ok(self.publish_layout(guild))
                    }
                };
                // A refusal for a stale version still means the server's layout moved on.
                let echo = (result.is_ok() || result == Err(ApiError::Conflict))
                    .then(|| self.layout_event(guild));
                std::iter::once(Reply::LayoutSaved { guild, result })
                    .chain(echo)
                    .collect()
            }
            Command::UpdateChannel {
                guild,
                channel,
                name,
                icon,
                color,
                topic,
            } => {
                let r = if !self.can_manage(guild, me) {
                    Err(ApiError::PermissionDenied)
                } else if name.as_ref().is_some_and(|n| {
                    self.channels
                        .iter()
                        .any(|c| c.guild_id == guild && c.id != channel && &c.name == n)
                }) {
                    Err(ApiError::AlreadyExists)
                } else {
                    match self
                        .channels
                        .iter_mut()
                        .find(|c| c.guild_id == guild && c.id == channel)
                    {
                        None => Err(ApiError::NotFound),
                        Some(c) => {
                            if let Some(n) = name {
                                c.name = n;
                            }
                            if let Some(i) = icon {
                                c.icon = ChannelIcon::new(i.phosphor_name());
                                c.icon_ref = i;
                            }
                            if let Some(col) = color {
                                c.color = col;
                            }
                            if let Some(t) = topic {
                                c.topic = t;
                            }
                            Ok(c.clone())
                        }
                    }
                };
                let echo = r
                    .clone()
                    .ok()
                    .map(|c| Reply::Gateway(GatewayUpdate::Event(Event::ChannelUpdated(c))));
                std::iter::once(Reply::ChannelSaved { guild, result: r })
                    .chain(echo)
                    .collect()
            }
            Command::LoadMessages { channel, before } => vec![Reply::Messages {
                channel,
                before,
                result: Ok(self.page(channel, before)),
            }],
            Command::Send {
                channel,
                content,
                nonce,
            } => {
                let result = if self.fail_sends > 0 {
                    self.fail_sends -= 1;
                    Err(ApiError::Network("simulated outage".into()))
                } else if self
                    .channels
                    .iter()
                    .any(|c| c.id == channel && c.kind == ChannelKind::Voice)
                {
                    Err(ApiError::Precondition(
                        "voice channels have no messages".into(),
                    ))
                } else if content.trim().is_empty() || content.chars().count() > MAX_CHARS {
                    Err(ApiError::InvalidArgument(
                        "message must be 1 to 4000 characters".into(),
                    ))
                } else {
                    self.sent_at += 15;
                    self.push(channel, me, self.sent_at, &content);
                    let m = self
                        .messages
                        .get_mut(&channel)
                        .and_then(|v| v.last_mut())
                        .unwrap();
                    m.nonce = nonce.clone();
                    Ok(m.clone())
                };
                // The server answers the sender and broadcasts to everyone, the sender included.
                let echo = result
                    .clone()
                    .ok()
                    .map(|m| Reply::Gateway(GatewayUpdate::Event(Event::MessageCreated(m))));
                std::iter::once(Reply::Sent {
                    channel,
                    nonce,
                    result,
                })
                .chain(echo)
                .collect()
            }
            // Answered by `files_command` before this match.
            Command::SendWith { .. }
            | Command::UploadFile { .. }
            | Command::UploadBytes { .. }
            | Command::CancelUpload { .. }
            | Command::Download { .. } => Vec::new(),
            Command::CreateGuild { name } => {
                let n = name.chars().count();
                if !(2..=32).contains(&n) {
                    return vec![Reply::GuildCreated(Err(ApiError::InvalidArgument(
                        "name must be 2 to 32 characters".into(),
                    )))];
                }
                let id = 100 + self.guilds.len() as u64;
                let g = Guild {
                    id,
                    name,
                    owner_id: me,
                    invite_code: format!("INVITE{id}"),
                    created_at: time(now_unix()),
                    icon: None,
                };
                self.guilds.push(g.clone());
                self.channels.push(Channel {
                    id: id * 10,
                    guild_id: id,
                    name: "general".into(),
                    created_at: time(now_unix()),
                    section_id: None,
                    position: 0,
                    icon: ChannelIcon::default(),
                    icon_ref: ChannelIconRef::Default,
                    color: PaletteColor::None,
                    topic: String::new(),
                    kind: ChannelKind::Text,
                });
                self.members.insert(id, vec![me]);
                self.layouts.insert(
                    id,
                    GuildLayout {
                        version: 1,
                        sections: vec![],
                        entries: vec![LayoutEntry {
                            section_id: None,
                            channel_ids: vec![id * 10],
                        }],
                    },
                );
                vec![Reply::GuildCreated(Ok(g))]
            }
            Command::JoinGuild { code } => {
                let found = self
                    .guilds
                    .iter()
                    .find(|g| g.invite_code.eq_ignore_ascii_case(code.trim()))
                    .cloned();
                let r = match found {
                    None => Err(ApiError::NotFound),
                    Some(g) if self.members.get(&g.id).is_some_and(|m| m.contains(&me)) => {
                        Err(ApiError::AlreadyExists)
                    }
                    Some(g) if self.is_banned(g.id, me) => Err(ApiError::PermissionDenied),
                    Some(g) => {
                        self.members.entry(g.id).or_default().push(me);
                        Ok(g)
                    }
                };
                vec![Reply::GuildJoined(r)]
            }
            Command::CreateChannel {
                guild,
                name,
                kind,
                section,
                icon,
                color,
            } => {
                let known = self.guilds.iter().any(|g| g.id == guild);
                let r = if !known {
                    Err(ApiError::NotFound)
                } else if !self.can_manage(guild, me) {
                    Err(ApiError::PermissionDenied)
                } else if self
                    .channels
                    .iter()
                    .any(|c| c.guild_id == guild && c.name == name)
                {
                    Err(ApiError::AlreadyExists)
                } else {
                    let c = Channel {
                        id: 1000 + self.channels.len() as u64,
                        guild_id: guild,
                        name,
                        created_at: time(now_unix()),
                        section_id: section,
                        position: 0,
                        icon: ChannelIcon::new(icon.phosphor_name()),
                        icon_ref: icon,
                        color,
                        topic: String::new(),
                        kind,
                    };
                    self.channels.push(c.clone());
                    let l = self.layouts.entry(guild).or_default();
                    *l = layout::with_channel(l, c.id, section);
                    Ok(c)
                };
                let mut out = vec![Reply::ChannelCreated(r.clone())];
                if let Ok(c) = r {
                    // The server announces the channel, then the new layout.
                    out.push(Reply::Gateway(GatewayUpdate::Event(Event::ChannelCreated(
                        c,
                    ))));
                    self.publish_layout(guild);
                    out.push(self.layout_event(guild));
                }
                out
            }
            Command::LoadMembers { guild } => {
                let ids = self.members.get(&guild).cloned().unwrap_or_default();
                let users = ids
                    .iter()
                    .filter_map(|i| self.users.iter().find(|u| u.id == *i).cloned())
                    .collect();
                vec![Reply::Members {
                    guild,
                    result: Ok(users),
                }]
            }
            Command::OpenDm { user_id } => {
                let existing = self
                    .dms
                    .iter()
                    .find(|d| {
                        d.participants.iter().any(|p| p.id == user_id)
                            && d.participants.iter().any(|p| p.id == me)
                    })
                    .cloned();
                let r = match (
                    existing,
                    self.users.iter().find(|u| u.id == user_id).cloned(),
                ) {
                    (Some(d), _) => Ok(d),
                    (None, None) => Err(ApiError::NotFound),
                    (None, Some(other)) => {
                        let mut participants = vec![self.me.clone(), other];
                        participants.sort_by_key(|u| u.id);
                        let d = DmChannel {
                            id: 500 + self.dms.len() as u64,
                            participants,
                            created_at: time(now_unix()),
                        };
                        self.dms.push(d.clone());
                        Ok(d)
                    }
                };
                vec![Reply::DmOpened(r)]
            }
            Command::LoadEmojis { guild } => {
                let result = if self.members.get(&guild).is_some_and(|m| m.contains(&me)) {
                    Ok(self.emojis_sorted(guild))
                } else {
                    Err(ApiError::NotFound)
                };
                vec![Reply::Emojis { guild, result }]
            }
            Command::CreateEmoji {
                guild,
                name,
                bytes,
                content_type,
            } => {
                let r = self.create_emoji(guild, me, &name, bytes, &content_type);
                let echo = r.is_ok().then(|| self.emojis_event(guild));
                std::iter::once(Reply::EmojiSaved { guild, result: r })
                    .chain(echo)
                    .collect()
            }
            Command::RenameEmoji { guild, emoji, name } => {
                let r = self.rename_emoji(guild, me, emoji, &name);
                let echo = r.is_ok().then(|| self.emojis_event(guild));
                std::iter::once(Reply::EmojiSaved { guild, result: r })
                    .chain(echo)
                    .collect()
            }
            Command::DeleteEmoji { guild, emoji } => {
                let r = if !self.has_perm(guild, me, Permissions::MANAGE_EMOJIS) {
                    Err(ApiError::PermissionDenied)
                } else {
                    let list = self.emojis.entry(guild).or_default();
                    match list.iter().position(|e| e.id == emoji) {
                        Some(i) => {
                            list.remove(i);
                            Ok(())
                        }
                        None => Err(ApiError::NotFound),
                    }
                };
                let echo = r.is_ok().then(|| self.emojis_event(guild));
                std::iter::once(Reply::EmojiDeleted {
                    guild,
                    emoji,
                    result: r,
                })
                .chain(echo)
                .collect()
            }
            Command::AddReaction {
                channel,
                message,
                emoji,
            } => {
                let result = if self.fail_reactions > 0 {
                    self.fail_reactions -= 1;
                    Err(ApiError::Network("simulated outage".into()))
                } else {
                    self.react(channel, message, me, &emoji, true)
                };
                let echo = result.is_ok().then(|| {
                    Reply::Gateway(GatewayUpdate::Event(Event::ReactionAdded {
                        channel_id: channel,
                        message_id: message,
                        emoji: emoji.clone(),
                        user_id: me,
                    }))
                });
                std::iter::once(Reply::ReactionSaved {
                    channel,
                    message,
                    emoji,
                    add: true,
                    result,
                })
                .chain(echo)
                .collect()
            }
            Command::RemoveReaction {
                channel,
                message,
                emoji,
            } => {
                let result = if self.fail_reactions > 0 {
                    self.fail_reactions -= 1;
                    Err(ApiError::Network("simulated outage".into()))
                } else {
                    self.react(channel, message, me, &emoji, false)
                };
                let echo = result.is_ok().then(|| {
                    Reply::Gateway(GatewayUpdate::Event(Event::ReactionRemoved {
                        channel_id: channel,
                        message_id: message,
                        emoji: emoji.clone(),
                        user_id: me,
                    }))
                });
                std::iter::once(Reply::ReactionSaved {
                    channel,
                    message,
                    emoji,
                    add: false,
                    result,
                })
                .chain(echo)
                .collect()
            }
            cmd @ (Command::AdminOverview
            | Command::AdminActivity { .. }
            | Command::AdminGuilds { .. }
            | Command::AdminUsers { .. }
            | Command::AdminFiles { .. }
            | Command::AdminAudit { .. }
            | Command::AdminSetAdmin { .. }
            | Command::AdminSuspendUser { .. }
            | Command::AdminUnsuspendUser { .. }
            | Command::AdminDeleteUser { .. }
            | Command::AdminDeleteFile { .. }) => {
                let me = self.me.clone();
                let mut replies = self.admin.handle(&me, cmd);
                // A deleted account shows as "Deleted user" everywhere, and everyone hears of it.
                for id in self.admin.deleted_ids() {
                    if let Some(w) = self.users.iter_mut().find(|w| w.id == id && !w.deleted) {
                        w.username = "Deleted user".to_owned();
                        w.avatar = None;
                        w.email = None;
                        w.is_admin = false;
                        w.deleted = true;
                        let gone = w.clone();
                        for dm in &mut self.dms {
                            for p in dm.participants.iter_mut().filter(|p| p.id == id) {
                                *p = gone.clone();
                            }
                        }
                        replies.push(Reply::Gateway(GatewayUpdate::Event(Event::UserUpdated(gone))));
                    }
                }
                // Rights granted or taken away show in the rest of the app too.
                for u in &self.admin.users {
                    if let Some(w) = self.users.iter_mut().find(|w| w.id == u.id) {
                        w.is_admin = u.is_admin;
                    }
                }
                replies
            }
            Command::FetchMedia { key } => {
                let result = match self.media.get(&key) {
                    Some(bytes) => Ok(bytes.clone()),
                    None if files::is_fixture(&key) => files::fixture_bytes(&key),
                    None if key.starts_with("adm-") => Ok(crate::media::generated_png(
                        key.bytes()
                            .fold(7u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b))),
                        48,
                    )),
                    None => match key.strip_prefix("gen-").and_then(|n| n.parse::<u64>().ok()) {
                        Some(seed) => Ok(crate::media::generated_png(seed, 64)),
                        None => match key
                            .strip_prefix("emoji-")
                            .and_then(|n| n.parse::<u64>().ok())
                        {
                            Some(id) => match self.emojis.values().flatten().find(|e| e.id == id) {
                                Some(e) if e.animated => {
                                    Ok(crate::media::generated_animated_emoji(id, 64, 8))
                                }
                                Some(_) => Ok(crate::media::generated_emoji(id, 64)),
                                None => Err(ApiError::NotFound),
                            },
                            None => Err(ApiError::NotFound),
                        },
                    },
                };
                vec![Reply::Media { key, result }]
            }
            Command::ReadImage { path, purpose } => vec![Reply::ImageRead {
                purpose,
                result: crate::media::read_image(&path),
            }],
            Command::UploadAvatar {
                bytes,
                content_type,
            } => {
                let r = check_image(&bytes, &content_type).map(|()| {
                    self.next_media += 1;
                    let key = format!("up-{}", self.next_media);
                    self.media.insert(key.clone(), bytes);
                    self.set_user_avatar(me, Some(key))
                        .expect("the signed-in user exists")
                });
                let echo = r
                    .clone()
                    .ok()
                    .map(|u| Reply::Gateway(GatewayUpdate::Event(Event::UserUpdated(u))));
                std::iter::once(Reply::AvatarSaved(r)).chain(echo).collect()
            }
            Command::RemoveAvatar => {
                let u = self.set_user_avatar(me, None).ok_or(ApiError::NotFound);
                let echo = u
                    .clone()
                    .ok()
                    .map(|u| Reply::Gateway(GatewayUpdate::Event(Event::UserUpdated(u))));
                std::iter::once(Reply::AvatarSaved(u)).chain(echo).collect()
            }
            Command::UploadGuildIcon {
                guild,
                bytes,
                content_type,
            } => {
                let r = self.guild_edit(guild, me, |w, g| {
                    check_image(&bytes, &content_type)?;
                    w.next_media += 1;
                    let key = format!("icon-{}", w.next_media);
                    w.media.insert(key.clone(), bytes);
                    g.icon = Some(key);
                    Ok(())
                });
                self.guild_replies(guild, r)
            }
            Command::RemoveGuildIcon { guild } => {
                let r = self.guild_edit(guild, me, |_, g| {
                    g.icon = None;
                    Ok(())
                });
                self.guild_replies(guild, r)
            }
            Command::RenameGuild { guild, name } => {
                let r = self.guild_edit(guild, me, |_, g| {
                    let n = name.trim().chars().count();
                    if !(2..=32).contains(&n) {
                        return Err(ApiError::InvalidArgument(
                            "name must be 2 to 32 characters".into(),
                        ));
                    }
                    g.name = name.trim().to_owned();
                    Ok(())
                });
                self.guild_replies(guild, r)
            }
            Command::CreateRole {
                guild,
                name,
                color,
                permissions,
            } => {
                let r = if !self.has_perm(guild, me, Permissions::MANAGE_ROLES) {
                    Err(ApiError::PermissionDenied)
                } else if !(1..=MAX_ROLE_NAME).contains(&name.trim().chars().count()) {
                    Err(ApiError::InvalidArgument(format!(
                        "role name must be 1 to {MAX_ROLE_NAME} characters"
                    )))
                } else if !self.holds_all(guild, me, permissions.bits()) {
                    Err(ApiError::PermissionDenied)
                } else {
                    let version = self.role_versions.entry(guild).or_insert(1);
                    *version += 1;
                    let version = *version;
                    let list = self.roles.entry(guild).or_default();
                    list.iter_mut().for_each(|r| r.position += 1);
                    self.next_role += 1;
                    let role = Role {
                        id: self.next_role,
                        guild_id: guild,
                        name: name.trim().to_owned(),
                        color,
                        position: 0,
                        permissions,
                        created_at: time(now_unix()),
                    };
                    list.push(role.clone());
                    Ok((role, version))
                };
                let echo = r.is_ok().then(|| self.roles_event(guild));
                std::iter::once(Reply::RoleCreated { guild, result: r })
                    .chain(echo)
                    .collect()
            }
            Command::UpdateRole {
                guild,
                role,
                name,
                color,
                permissions,
            } => {
                let r = self.update_role(guild, me, role, name, color, permissions);
                let echo = r.is_ok().then(|| self.roles_event(guild));
                std::iter::once(Reply::RoleUpdated { guild, result: r })
                    .chain(echo)
                    .collect()
            }
            Command::DeleteRole { guild, role } => {
                let found = self
                    .roles
                    .get(&guild)
                    .and_then(|rs| rs.iter().find(|r| r.id == role))
                    .cloned();
                let r = match found {
                    None => Err(ApiError::NotFound),
                    Some(_) if !self.has_perm(guild, me, Permissions::MANAGE_ROLES) => {
                        Err(ApiError::PermissionDenied)
                    }
                    Some(f) if !self.outranks(guild, me, f.position) => {
                        Err(ApiError::PermissionDenied)
                    }
                    Some(_) => {
                        let version = self.role_versions.entry(guild).or_insert(1);
                        *version += 1;
                        let version = *version;
                        let sorted = {
                            let mut v = self.roles_sorted(guild);
                            v.retain(|r| r.id != role);
                            v
                        };
                        let n = sorted.len() as i32;
                        self.roles.insert(
                            guild,
                            sorted
                                .into_iter()
                                .enumerate()
                                .map(|(i, mut r)| {
                                    r.position = n - 1 - i as i32;
                                    r
                                })
                                .collect(),
                        );
                        let mut affected = Vec::new();
                        for ((g, u), ids) in self.member_roles.iter_mut() {
                            if *g == guild && ids.contains(&role) {
                                ids.retain(|r| *r != role);
                                affected.push(*u);
                            }
                        }
                        Ok((version, affected))
                    }
                };
                match r {
                    Ok((version, mut affected)) => {
                        affected.sort_unstable();
                        let mut out = vec![
                            Reply::RoleDeleted {
                                guild,
                                role,
                                result: Ok(version),
                            },
                            self.roles_event(guild),
                        ];
                        out.extend(
                            affected
                                .into_iter()
                                .filter_map(|u| self.member_event(guild, u)),
                        );
                        out
                    }
                    Err(e) => vec![Reply::RoleDeleted {
                        guild,
                        role,
                        result: Err(e),
                    }],
                }
            }
            Command::ReorderRoles {
                guild,
                expected_version,
                role_ids,
            } => {
                let r = self.reorder_roles(guild, me, expected_version, &role_ids);
                // A refusal for a stale version still means the server's roles moved on.
                let echo =
                    (r.is_ok() || r == Err(ApiError::Conflict)).then(|| self.roles_event(guild));
                std::iter::once(Reply::RolesReordered { guild, result: r })
                    .chain(echo)
                    .collect()
            }
            Command::SetMemberRoles {
                guild,
                user,
                role_ids,
            } => {
                let r = self.set_member_roles(guild, me, user, &role_ids);
                let echo = r.is_ok().then(|| self.member_event(guild, user)).flatten();
                std::iter::once(Reply::MemberRolesSet {
                    guild,
                    user,
                    result: r,
                })
                .chain(echo)
                .collect()
            }
            Command::SignOut => vec![Reply::SignedOut],
            // Sign-in, security and invites: `account_command` answered them above.
            other => unreachable!("answered by fake_account: {other:?}"),
        }
    }
}

impl World {
    /// Runs `edit` on a guild the caller may manage (MANAGE_SERVER): the new guild or why not.
    fn guild_edit(
        &mut self,
        guild: u64,
        me: u64,
        edit: impl FnOnce(&mut World, &mut Guild) -> Result<(), ApiError>,
    ) -> Result<Guild, ApiError> {
        let at = self
            .guilds
            .iter()
            .position(|g| g.id == guild)
            .ok_or(ApiError::NotFound)?;
        if !self.has_perm(guild, me, Permissions::MANAGE_SERVER) {
            return Err(ApiError::PermissionDenied);
        }
        let mut g = self.guilds[at].clone();
        edit(self, &mut g)?;
        self.guilds[at] = g.clone();
        Ok(g)
    }

    fn guild_replies(&self, guild: u64, r: Result<Guild, ApiError>) -> Vec<Reply> {
        let echo = r
            .clone()
            .ok()
            .map(|g| Reply::Gateway(GatewayUpdate::Event(Event::GuildUpdated(g))));
        std::iter::once(Reply::GuildSaved { guild, result: r })
            .chain(echo)
            .collect()
    }

    fn update_role(
        &mut self,
        guild: u64,
        me: u64,
        role: u64,
        name: Option<String>,
        color: Option<PaletteColor>,
        permissions: Option<Permissions>,
    ) -> Result<Role, ApiError> {
        if !self.has_perm(guild, me, Permissions::MANAGE_ROLES) {
            return Err(ApiError::PermissionDenied);
        }
        let old = self
            .roles
            .get(&guild)
            .and_then(|rs| rs.iter().find(|r| r.id == role))
            .cloned()
            .ok_or(ApiError::NotFound)?;
        if !self.outranks(guild, me, old.position) {
            return Err(ApiError::PermissionDenied);
        }
        if let Some(n) = &name
            && !(1..=MAX_ROLE_NAME).contains(&n.trim().chars().count())
        {
            return Err(ApiError::InvalidArgument(format!(
                "role name must be 1 to {MAX_ROLE_NAME} characters"
            )));
        }
        if let Some(p) = permissions
            && !(self.holds_all(guild, me, p.bits())
                && self.holds_all(guild, me, p.bits() ^ old.permissions.bits()))
        {
            return Err(ApiError::PermissionDenied);
        }
        let slot = self
            .roles
            .get_mut(&guild)
            .and_then(|rs| rs.iter_mut().find(|r| r.id == role))
            .ok_or(ApiError::NotFound)?;
        if let Some(n) = name {
            slot.name = n.trim().to_owned();
        }
        if let Some(c) = color {
            slot.color = c;
        }
        if let Some(p) = permissions {
            slot.permissions = p;
        }
        Ok(slot.clone())
    }

    fn reorder_roles(
        &mut self,
        guild: u64,
        me: u64,
        expected: u64,
        ids: &[u64],
    ) -> Result<RoleList, ApiError> {
        if !self.has_perm(guild, me, Permissions::MANAGE_ROLES) {
            return Err(ApiError::PermissionDenied);
        }
        if std::mem::take(&mut self.conflict_next_roles) {
            // Someone else's reorder lands first.
            *self.role_versions.entry(guild).or_insert(1) += 1;
            return Err(ApiError::Conflict);
        }
        if self.role_versions.get(&guild).copied().unwrap_or(1) != expected {
            return Err(ApiError::Conflict);
        }
        let current = self.roles_sorted(guild);
        let mut want: Vec<u64> = ids.to_vec();
        let mut have: Vec<u64> = current.iter().map(|r| r.id).collect();
        want.sort_unstable();
        have.sort_unstable();
        if want != have || ids.len() != current.len() {
            return Err(ApiError::InvalidArgument(
                "role_ids must contain every role of the guild exactly once".into(),
            ));
        }
        let n = ids.len() as i32;
        let mut next = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            let mut r = current
                .iter()
                .find(|r| r.id == *id)
                .cloned()
                .expect("validated above");
            let position = n - 1 - i as i32;
            if r.position != position
                && !(self.outranks(guild, me, r.position) && self.outranks(guild, me, position))
            {
                return Err(ApiError::PermissionDenied);
            }
            r.position = position;
            next.push(r);
        }
        self.roles.insert(guild, next.clone());
        let version = self.role_versions.entry(guild).or_insert(1);
        *version += 1;
        Ok(RoleList {
            roles: next,
            version: *version,
        })
    }

    fn set_member_roles(
        &mut self,
        guild: u64,
        me: u64,
        user: u64,
        ids: &[u64],
    ) -> Result<Member, ApiError> {
        if !self.has_perm(guild, me, Permissions::MANAGE_ROLES) {
            return Err(ApiError::PermissionDenied);
        }
        if !self.members.get(&guild).is_some_and(|m| m.contains(&user)) {
            return Err(ApiError::NotFound);
        }
        let roles = self.roles.get(&guild).cloned().unwrap_or_default();
        if ids.iter().any(|id| !roles.iter().any(|r| r.id == *id)) {
            return Err(ApiError::NotFound);
        }
        let current = self
            .member_roles
            .get(&(guild, user))
            .cloned()
            .unwrap_or_default();
        for r in roles
            .iter()
            .filter(|r| ids.contains(&r.id) != current.contains(&r.id))
        {
            if !self.outranks(guild, me, r.position) {
                return Err(ApiError::PermissionDenied);
            }
        }
        self.member_roles.insert((guild, user), ids.to_vec());
        self.member_of(guild, user).ok_or(ApiError::NotFound)
    }
}

/// What the server checks before it stores a picture: a known format's signature, and the size.
fn check_image(bytes: &[u8], content_type: &str) -> Result<(), ApiError> {
    if bytes.len() > thehideout_api::MAX_MEDIA_BYTES {
        return Err(ApiError::InvalidArgument("image is over 8 MiB".into()));
    }
    let known = bytes.starts_with(b"\x89PNG")
        || bytes.starts_with(&[0xff, 0xd8])
        || bytes.starts_with(b"GIF8")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"));
    if !known || !content_type.starts_with("image/") {
        return Err(ApiError::InvalidArgument("not a supported image".into()));
    }
    Ok(())
}

impl World {
    fn submit(&mut self, cmd: Command) {
        self.log.push(cmd.clone());
        for r in self.handle(cmd) {
            self.enqueue(r);
        }
    }

    fn poll_due(&mut self, now: f64) -> Vec<Reply> {
        self.now = now;
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.queue)
            .into_iter()
            .partition(|(t, _)| *t <= now);
        self.queue = later;
        let mut out: Vec<Reply> = due.into_iter().map(|(_, r)| r).collect();
        // The passkey ceremony "finishes in the browser" a moment after it starts.
        if self.account.ceremony.as_ref().is_some_and(|(t, _)| *t <= now)
            && let Some((_, reply)) = self.account.ceremony.take()
        {
            out.push(reply);
        }
        out
    }

    fn due(&self) -> Option<f64> {
        self.queue
            .iter()
            .map(|(t, _)| *t)
            .chain(self.account.ceremony.as_ref().map(|(t, _)| *t))
            .reduce(f64::min)
    }
}

/// A handle to the fake world. Clones share it, so a scene or test keeps one to drive incoming
/// events while the app owns the boxed [`Backend`].
#[derive(Clone, Default)]
pub struct FakeBackend(Rc<RefCell<World>>);

impl FakeBackend {
    pub fn new() -> FakeBackend {
        FakeBackend::default()
    }

    /// Seconds each reply waits before it is due (default 0: due at the next poll).
    pub fn with_latency(self, secs: f64) -> Self {
        self.0.borrow_mut().set_latency(secs);
        self
    }

    /// Changes the reply latency of a shared handle.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_latency(&self, secs: f64) {
        self.0.borrow_mut().set_latency(secs);
    }

    /// The signed-in user has not accepted the current terms (the next `CheckTerms` says so).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn outdate_terms(&self) {
        self.0.borrow_mut().account.terms_outdated = true;
    }

    /// Turns an authenticator app on for `username` (recovery codes are `fake::account::RECOVERY`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn enable_totp(&self, username: &str) {
        self.0.borrow_mut().enable_totp(username);
    }

    /// Every admin command answers `Unimplemented`, as a server without the service does.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn admin_unavailable(&self, on: bool) {
        self.0.borrow_mut().admin.unavailable = on;
    }

    /// The next `n` admin file deletions fail with a network error.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn admin_fail_deletes(&self, n: u32) {
        self.0.borrow_mut().admin.fail_deletes = n;
    }

    /// The next demotion is refused whoever it is for (another admin got there first).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn admin_refuse_next_set(&self) {
        self.0.borrow_mut().admin.refuse_next_set = true;
    }

    /// What the admin side holds: `(files, has this key, audit as (action, target, reason) newest first)`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn admin_has_file(&self, key: &str) -> bool {
        self.0.borrow().admin.has_file(key)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn admin_file_count(&self) -> usize {
        self.0.borrow().admin.file_count()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn admin_audit(&self) -> Vec<(String, String, String)> {
        self.0.borrow().admin.audit_actions()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn admin_is_admin(&self, id: u64) -> bool {
        self.0.borrow().admin.is_admin(id)
    }

    /// The next `n` sends are refused with a network error.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn fail_sends(&self, n: u32) {
        self.0.borrow_mut().set_fail_sends(n);
    }

    /// The next `UpdateLayout` is refused as stale (another client edited first).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn conflict_next_layout(&self) {
        self.0.borrow_mut().set_conflict_next();
    }

    /// The next `ReorderRoles` is refused as stale (another client reordered first).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn conflict_next_roles(&self) {
        self.0.borrow_mut().set_conflict_next_roles();
    }

    /// Every user gets a generated avatar (announced on the gateway).
    pub fn give_avatars(&self) {
        self.0.borrow_mut().give_avatars();
    }

    /// `guild` gets a generated icon (announced on the gateway).
    pub fn give_icon(&self, guild: u64) {
        self.0.borrow_mut().give_icon(guild);
    }

    /// The server's roles of `guild`, most senior first (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn roles(&self, guild: u64) -> Vec<Role> {
        self.0.borrow().roles_sorted(guild)
    }

    /// The role ids the server has for `user` in `guild` (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn member_roles(&self, guild: u64, user: u64) -> Vec<u64> {
        self.0
            .borrow()
            .member_roles
            .get(&(guild, user))
            .cloned()
            .unwrap_or_default()
    }

    /// The server's copy of a user (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn user(&self, id: u64) -> Option<User> {
        self.0.borrow().users.iter().find(|u| u.id == id).cloned()
    }

    /// The server's copy of a guild (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn guild(&self, id: u64) -> Option<Guild> {
        self.0.borrow().guilds.iter().find(|g| g.id == id).cloned()
    }

    /// `user` gains or loses the Moderator role (MANAGE_CHANNELS) of `guild`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_moderator(&self, guild: u64, user: u64, on: bool) {
        self.0.borrow_mut().set_moderator(guild, user, on);
    }

    /// The server's layout of `guild` (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn layout(&self, guild: u64) -> GuildLayout {
        self.0
            .borrow()
            .layouts
            .get(&guild)
            .cloned()
            .unwrap_or_default()
    }

    /// The server's copy of a channel (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn channel(&self, id: u64) -> Option<Channel> {
        self.0
            .borrow()
            .channels
            .iter()
            .find(|c| c.id == id)
            .cloned()
    }

    /// Voice is not configured on the server: joining fails like it does without LiveKit.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn voice_unavailable(&self, on: bool) {
        self.0.borrow_mut().voice.unavailable = on;
    }

    /// The server's voice states of `guild`, in join order (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn voice_states(&self, guild: u64) -> Vec<VoiceState> {
        self.0.borrow().voice_states_of(guild)
    }

    /// `user` joins (or switches to) a voice channel, announced on the gateway (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn voice_join_as(&self, user: u64, channel: u64) {
        self.0.borrow_mut().voice_join_as(user, channel);
    }

    /// `user` leaves voice, announced on the gateway (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn voice_leave_as(&self, user: u64) {
        self.0.borrow_mut().voice_leave_as(user);
    }

    /// A moderator moves `user` (the gateway also tells them to rejoin when it is us), no
    /// permission checks (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn voice_move_as_moderator(&self, user: u64, channel: u64) {
        self.0.borrow_mut().voice_move_as_moderator(user, channel);
    }

    /// Changes what a role grants, announced on the gateway (tests of the moderation controls).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_role_permissions(&self, guild: u64, role: u64, permissions: Permissions) {
        self.0.borrow_mut().set_role_permissions(guild, role, permissions);
    }

    /// The server's custom emojis of `guild`, by name (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn emojis(&self, guild: u64) -> Vec<Emoji> {
        self.0.borrow().emojis_sorted(guild)
    }

    /// Fills `guild` with generated emojis up to `n` in all (tests of the 200 cap).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn fill_emojis(&self, guild: u64, n: usize) {
        let mut w = self.0.borrow_mut();
        let have = w.emojis.get(&guild).map_or(0, Vec::len);
        for i in have..n {
            w.next_emoji += 1;
            let id = w.next_emoji;
            let e = Emoji {
                id,
                guild_id: guild,
                name: format!("filler_{i}"),
                animated: false,
                image: format!("emoji-{id}"),
                creator_id: TIM,
                created_at: time(now_unix()),
            };
            w.emojis.entry(guild).or_default().push(e);
        }
    }

    /// `user` reacts (or takes the reaction back) on a message, announced on the gateway (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn react_as(&self, channel: u64, message: u64, emoji: ReactionEmoji, user: u64, add: bool) {
        let mut w = self.0.borrow_mut();
        let _ = w.react(channel, message, user, &emoji, add);
        let event = if add {
            Event::ReactionAdded {
                channel_id: channel,
                message_id: message,
                emoji,
                user_id: user,
            }
        } else {
            Event::ReactionRemoved {
                channel_id: channel,
                message_id: message,
                emoji,
                user_id: user,
            }
        };
        w.enqueue(Reply::Gateway(GatewayUpdate::Event(event)));
    }

    /// `user` reacts, but the gateway never says so: the client is away and only a fetch finds it (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn react_missed(
        &self,
        channel: u64,
        message: u64,
        emoji: ReactionEmoji,
        user: u64,
        add: bool,
    ) {
        let _ = self
            .0
            .borrow_mut()
            .react(channel, message, user, &emoji, add);
    }

    /// Makes the next `n` reaction calls fail, as a network error (tests of the rollback).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn fail_reactions(&self, n: u32) {
        self.0.borrow_mut().fail_reactions = n;
    }

    /// The server's copy of a message (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn message(&self, channel: u64, id: u64) -> Option<Message> {
        self.0
            .borrow()
            .messages
            .get(&channel)?
            .iter()
            .find(|m| m.id == id)
            .cloned()
    }

    /// A moderator removes `user` from `guild` (`reason` as the server reports it); when it is us
    /// the gateway also says we are out (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn remove_member_as_moderator(&self, guild: u64, user: u64, reason: thehideout_api::MemberRemovalReason) {
        self.0.borrow_mut().remove_member_as_moderator(guild, user, reason);
    }

    /// A moderator deletes a message, announced on the gateway (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn delete_message_as_moderator(&self, channel: u64, message: u64) {
        self.0.borrow_mut().delete_message_as_moderator(channel, message);
    }

    /// Someone else deletes `channel` of `guild`, announced on the gateway (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn delete_channel_as_moderator(&self, guild: u64, channel: u64) {
        self.0.borrow_mut().delete_channel_as_moderator(guild, channel);
    }

    /// The owner deletes `guild`, announced on the gateway (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn delete_guild_as_owner(&self, guild: u64) {
        self.0.borrow_mut().delete_guild_as_owner(guild);
    }

    /// The members of `guild` (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn member_ids(&self, guild: u64) -> Vec<u64> {
        self.0.borrow().members.get(&guild).cloned().unwrap_or_default()
    }

    /// `user` becomes a member of `guild` without any announcement (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn add_member(&self, guild: u64, user: u64) {
        self.0.borrow_mut().members.entry(guild).or_default().push(user);
    }

    /// Whether `user` is banned from `guild` (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_banned(&self, guild: u64, user: u64) -> bool {
        self.0.borrow().is_banned(guild, user)
    }

    /// The bans of `guild`, newest first (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bans(&self, guild: u64) -> Vec<thehideout_api::Ban> {
        self.0.borrow().moderation.bans_of(guild)
    }

    /// `author` posts `content` in `channel`, as the gateway would deliver it.
    pub fn incoming(&self, channel: u64, author: u64, content: &str) {
        self.0.borrow_mut().incoming(channel, author, content);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn gateway(&self, update: GatewayUpdate) {
        self.0.borrow_mut().gateway(update);
    }

    /// The gateway drops; it will retry in `retry_secs`.
    pub fn disconnect(&self, retry_secs: u64) {
        self.0.borrow_mut().disconnect(retry_secs);
    }

    /// `author` posts in `channel` while the client is away: only a fetch finds it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn missed(&self, channel: u64, author: u64, content: &str) {
        self.0.borrow_mut().missed(channel, author, content);
    }

    /// A retry that fails: `Connecting`, then `Disconnected` again.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn failed_retry(&self, retry_secs: u64) {
        self.0.borrow_mut().failed_retry(retry_secs);
    }

    /// A retry that works: `Connecting`, then `Connected`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn reconnect(&self) {
        self.0.borrow_mut().reconnect();
    }
}

impl Backend for FakeBackend {
    fn submit(&mut self, cmd: Command) {
        self.0.borrow_mut().submit(cmd);
    }

    fn poll(&mut self, now: f64) -> Vec<Reply> {
        self.0.borrow_mut().poll_due(now)
    }

    fn next_due(&self) -> Option<f64> {
        self.0.borrow().due()
    }

    fn media_path(&self, key: &str) -> Option<std::path::PathBuf> {
        self.0.borrow_mut().local_path(key)
    }

    fn set_waker(&mut self, waker: rune::Waker) {
        self.0.borrow_mut().waker = Some(waker);
    }

    fn sent(&self) -> Vec<Command> {
        self.0.borrow().log.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: &mut World, cmd: Command) -> Vec<Reply> {
        f.submit(cmd);
        f.poll_due(1.0)
    }

    #[test]
    fn seed_is_deterministic_and_busy() {
        let (a, b) = (World::new(), World::new());
        assert_eq!(a.messages[&GENERAL], b.messages[&GENERAL]);
        assert_eq!(a.messages[&GENERAL].len(), 150);
        assert!(
            a.messages[&GENERAL]
                .windows(2)
                .all(|w| w[0].id < w[1].id && w[0].created_at <= w[1].created_at)
        );
        assert!(!a.messages.contains_key(&PLANS));
    }

    #[test]
    fn pages_walk_back_to_the_start() {
        let f = World::new();
        let first = f.page(GENERAL, None);
        assert_eq!((first.items.len(), first.has_more), (PAGE, true));
        let mut before = first.next_before;
        let mut total = first.items.len();
        while let Some(b) = before {
            let p = f.page(GENERAL, Some(b));
            total += p.items.len();
            before = p.next_before;
        }
        assert_eq!(total, 150);
    }

    #[test]
    fn auth_rules() {
        let mut f = World::new();
        let login = |username: &str, password: &str| Command::Login { username: username.into(), password: password.into() };
        let r = run(&mut f, login("tim", "wrong"));
        assert_eq!(r, vec![Reply::Auth(Err(ApiError::Unauthenticated))]);
        let r = run(&mut f, login("nobody@example.com", "x"));
        assert_eq!(r, vec![Reply::Auth(Err(ApiError::Unauthenticated))], "an unknown email looks like a wrong password");
        let r = run(&mut f, login("tim", "anything"));
        assert!(matches!(&r[0], Reply::Auth(Ok(s)) if s.me.id == TIM));
        let r = run(&mut f, login("tim@example.com", "anything"));
        assert!(matches!(&r[0], Reply::Auth(Ok(s)) if s.me.id == TIM), "an email signs in too");
        let r = run(&mut f, login(account::DORA, "anything"));
        assert!(matches!(&r[0], Reply::SecondFactor { .. }), "dora has an authenticator app");
    }

    #[test]
    fn a_send_answers_with_the_reply_and_an_echo_carrying_the_nonce() {
        let mut f = World::new();
        let r = run(
            &mut f,
            Command::Send {
                channel: GENERAL,
                content: "hi".into(),
                nonce: "n1".into(),
            },
        );
        assert_eq!(r.len(), 2);
        assert!(matches!(&r[0], Reply::Sent { result: Ok(m), .. } if m.nonce == "n1"));
        assert!(
            matches!(&r[1], Reply::Gateway(GatewayUpdate::Event(Event::MessageCreated(m))) if m.nonce == "n1")
        );
    }

    #[test]
    fn latency_defers_replies() {
        let mut f = World::new();
        f.set_latency(0.5);
        f.submit(Command::SignOut);
        assert!(f.poll_due(0.2).is_empty());
        assert_eq!(f.due(), Some(0.5));
        assert_eq!(f.poll_due(0.6).len(), 1);
    }
}
