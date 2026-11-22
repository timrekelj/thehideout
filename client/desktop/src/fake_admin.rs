//! The fake server's site-admin side: 44 users, 12 servers, 300 stored files from a few bytes to
//! over a gigabyte, 90 days of activity with a weekly rhythm and an audit trail, all derived from
//! one deterministic seed. Totals and per-server numbers are computed from the files, so deleting
//! a file moves them. `handle` answers the `Admin*` commands the way the real service would,
//! including its refusals.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use thehideout_api::{
    ActivityPoint, AdminFileKind, AdminFileRow, AdminGuildRow, AdminUserRow, ApiError, AuditEntry,
    Guild, Overview, Paged, StorageBreakdown, User,
};

use crate::backend::{Command, Reply};
use crate::format::{DAY, utc};

/// "Today" of the fake world (see `fake::now_unix`): the day of its last activity point.
const TODAY: (i64, u32, u32) = (2026, 11, 19);

const GIB: u64 = 1024 * 1024 * 1024;
const MIB: u64 = 1024 * 1024;
const KIB: u64 = 1024;

const NAMES: [&str; 40] = [
    "marta", "luka", "nina", "jure", "eva", "tomaz", "sara", "matej", "ajda", "gasper", "pia",
    "rok", "zala", "ziga", "mia", "anze", "lara", "jan", "neza", "vid", "maja", "peter", "kaja",
    "bor", "tina", "miha", "ula", "nik", "spela", "tim2", "lea", "klemen", "petra", "domen", "ema",
    "urban", "masa", "alen", "hana", "david",
];

const MORE_GUILDS: [&str; 9] = [
    "Retro Games",
    "Book Circle",
    "Night Owls",
    "Hiking Crew",
    "Pixel Lab",
    "Study Hall",
    "Film Club",
    "Board Games",
    "Plant Parents",
];

/// Users that hold site-admin rights at the start (besides whoever signs in as `cvetka`).
const EXTRA_ADMINS: [&str; 1] = ["marta"];

fn time(unix: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(unix.max(0) as u64)
}

fn today_unix() -> i64 {
    utc(TODAY.0, TODAY.1, TODAY.2, 11, 30)
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below.max(1)
    }

    fn between(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next(hi - lo + 1)
    }
}

/// Per-user numbers that are not derived from files.
struct UserStats {
    guilds: u64,
    messages: u64,
    last_active: Option<i64>,
}

/// Per-server numbers that are not derived from files.
struct GuildStats {
    members: u64,
    channels: u64,
    messages: u64,
}

pub struct AdminWorld {
    pub users: Vec<User>,
    user_stats: Vec<UserStats>,
    guilds: Vec<Guild>,
    guild_stats: Vec<GuildStats>,
    files: Vec<AdminFileRow>,
    activity: Vec<ActivityPoint>,
    audit: Vec<AuditEntry>,
    next_audit: u64,
    live: u64,
    /// Every admin command answers `Unimplemented` (a server without the service).
    pub unavailable: bool,
    /// The next `n` file deletions fail with a network error.
    pub fail_deletes: u32,
    /// The next `SetAdmin` is refused as if another admin had just changed the rules under it.
    pub refuse_next_set: bool,
    /// Suspended users: the reason and when it ends (`None` = until lifted).
    suspended: std::collections::HashMap<u64, (String, Option<SystemTime>)>,
    /// Users whose accounts were deleted (anonymized).
    deleted: std::collections::HashSet<u64>,
}

fn user(id: u64, name: &str, admin: bool, avatar: Option<String>) -> User {
    User {
        id,
        username: name.into(),
        created_at: time(utc(2026, 1, 5, 12, 0) + id as i64 * DAY),
        avatar,
        is_admin: admin,
        email: None,
        deleted: false,
    }
}

impl AdminWorld {
    /// Nothing yet: the world fills it in once its own users and servers exist.
    pub fn empty() -> AdminWorld {
        AdminWorld {
            users: Vec::new(),
            user_stats: Vec::new(),
            guilds: Vec::new(),
            guild_stats: Vec::new(),
            files: Vec::new(),
            activity: Vec::new(),
            audit: Vec::new(),
            next_audit: 1,
            live: 7,
            unavailable: false,
            fail_deletes: 0,
            refuse_next_set: false,
            suspended: Default::default(),
            deleted: Default::default(),
        }
    }

    /// `base_users` and `base_guilds` are the fake world's own (their ids stay), extended to 44 users and 12 servers.
    pub fn new(base_users: &[User], base_guilds: &[Guild]) -> AdminWorld {
        let mut rng = Lcg(0x5eed_ad31);
        let mut users: Vec<User> = base_users.to_vec();
        for (i, name) in NAMES.iter().enumerate() {
            let id = 10 + i as u64;
            let avatar = (i % 4 == 1).then(|| format!("gen-{}", 40 + i));
            users.push(user(id, name, EXTRA_ADMINS.contains(name), avatar));
        }
        let user_stats = users
            .iter()
            .enumerate()
            .map(|(i, _)| UserStats {
                guilds: 1 + rng.next(6),
                messages: if i % 17 == 16 {
                    0
                } else {
                    rng.between(12, 9_000)
                },
                // A couple of people never got going; the rest were seen within the quarter.
                last_active: if i % 17 == 16 {
                    None
                } else {
                    Some(
                        today_unix()
                            - rng.next(60 * 24 * 60 * 60 / (1 + (i as u64 % 5) * 6)) as i64,
                    )
                },
            })
            .collect();

        let mut guilds: Vec<Guild> = base_guilds.to_vec();
        for (i, name) in MORE_GUILDS.iter().enumerate() {
            let id = base_guilds.len() as u64 + 1 + i as u64;
            guilds.push(Guild {
                id,
                name: (*name).into(),
                owner_id: users[(i * 5 + 4) % users.len()].id,
                invite_code: format!("FAKE{id:04}"),
                created_at: time(utc(2026, 2, 1 + i as u32, 9, 0)),
                icon: (i % 3 == 0).then(|| format!("gen-{}", 70 + i)),
            });
        }
        let guild_stats = guilds
            .iter()
            .enumerate()
            .map(|(i, _)| GuildStats {
                members: if i == 0 { 31 } else { rng.between(3, 40) },
                channels: rng.between(2, 14),
                messages: if i == 0 {
                    48_210
                } else {
                    rng.between(120, 31_000)
                },
            })
            .collect();

        let mut world = AdminWorld {
            users,
            user_stats,
            guilds,
            guild_stats,
            files: Vec::new(),
            activity: Vec::new(),
            audit: Vec::new(),
            next_audit: 1,
            live: 7,
            unavailable: false,
            fail_deletes: 0,
            refuse_next_set: false,
            suspended: Default::default(),
            deleted: Default::default(),
        };
        world.seed_files(&mut rng);
        world.seed_activity(&mut rng);
        world.seed_audit(&mut rng);
        world
    }

    fn seed_files(&mut self, rng: &mut Lcg) {
        const IMAGES: [(&str, &str); 4] = [
            ("screenshot", "image/png"),
            ("IMG", "image/jpeg"),
            ("meme", "image/gif"),
            ("diagram", "image/webp"),
        ];
        const DOCS: [(&str, &str); 4] = [
            ("report.pdf", "application/pdf"),
            ("budget.xlsx", "application/vnd.ms-excel"),
            ("minutes.docx", "application/msword"),
            ("slides.pptx", "application/vnd.ms-powerpoint"),
        ];
        const VIDEOS: [(&str, &str); 3] = [
            ("holiday-recap.mp4", "video/mp4"),
            ("gameplay-session.mkv", "video/x-matroska"),
            ("demo-reel.mov", "video/quicktime"),
        ];
        const AUDIO: [(&str, &str); 3] = [
            ("mixdown.wav", "audio/wav"),
            ("voice-memo.m4a", "audio/mp4"),
            ("rehearsal.flac", "audio/flac"),
        ];
        const ARCHIVES: [(&str, &str); 3] = [
            ("backup.zip", "application/zip"),
            ("assets.tar.gz", "application/gzip"),
            ("photos.7z", "application/x-7z-compressed"),
        ];
        let now = today_unix();
        let (guilds, users) = (self.guilds.len() as u64, self.users.len() as u64);
        for i in 0..300u64 {
            let guild = self.guilds[rng.next(guilds) as usize].clone();
            let owner = self.users[rng.next(users) as usize].clone();
            let age = (rng.next(90 * 24) as i64) * 3600;
            let (kind, filename, content_type, bytes) = match i {
                0 => (
                    AdminFileKind::Attachment,
                    "wedding-video-raw.mov".to_owned(),
                    "video/quicktime".to_owned(),
                    GIB + 400 * MIB,
                ),
                1 => (
                    AdminFileKind::Attachment,
                    "note.txt".to_owned(),
                    "text/plain".to_owned(),
                    12,
                ),
                2 => (
                    AdminFileKind::Attachment,
                    "empty.log".to_owned(),
                    "text/plain".to_owned(),
                    1,
                ),
                3 => (
                    AdminFileKind::Attachment,
                    "session-recording.mkv".to_owned(),
                    "video/x-matroska".to_owned(),
                    640 * MIB,
                ),
                4..=209 => {
                    let bucket = rng.next(100);
                    match bucket {
                        0..=39 => {
                            let (stem, mime) = IMAGES[rng.next(IMAGES.len() as u64) as usize];
                            let ext = mime
                                .rsplit('/')
                                .next()
                                .unwrap_or("png")
                                .replace("jpeg", "jpg");
                            (
                                AdminFileKind::Attachment,
                                format!("{stem}-{}.{ext}", 100 + i),
                                mime.to_owned(),
                                rng.between(18 * KIB, 1900 * KIB),
                            )
                        }
                        40..=59 => {
                            let (name, mime) = DOCS[rng.next(DOCS.len() as u64) as usize];
                            (
                                AdminFileKind::Attachment,
                                format!("{}-{i}", name.replace('.', "-v.").replace("-v.", ".")),
                                mime.to_owned(),
                                rng.between(KIB, 30 * MIB),
                            )
                        }
                        60..=69 => {
                            let (name, mime) = VIDEOS[rng.next(VIDEOS.len() as u64) as usize];
                            (
                                AdminFileKind::Attachment,
                                format!("{i}-{name}"),
                                mime.to_owned(),
                                rng.between(24 * MIB, 900 * MIB),
                            )
                        }
                        70..=79 => {
                            let (name, mime) = AUDIO[rng.next(AUDIO.len() as u64) as usize];
                            (
                                AdminFileKind::Attachment,
                                format!("{i}-{name}"),
                                mime.to_owned(),
                                rng.between(MIB, 80 * MIB),
                            )
                        }
                        80..=89 => {
                            let (name, mime) = ARCHIVES[rng.next(ARCHIVES.len() as u64) as usize];
                            (
                                AdminFileKind::Attachment,
                                format!("{i}-{name}"),
                                mime.to_owned(),
                                rng.between(MIB, 400 * MIB),
                            )
                        }
                        _ => (
                            AdminFileKind::Attachment,
                            format!("notes-{i}.txt"),
                            "text/plain".to_owned(),
                            rng.between(13, 6000),
                        ),
                    }
                }
                210..=233 => (
                    AdminFileKind::Emoji,
                    String::new(),
                    "image/webp".to_owned(),
                    rng.between(2 * KIB, 120 * KIB),
                ),
                234..=248 => (
                    AdminFileKind::GuildIcon,
                    String::new(),
                    "image/webp".to_owned(),
                    rng.between(8 * KIB, 240 * KIB),
                ),
                249..=284 => (
                    AdminFileKind::Avatar,
                    String::new(),
                    "image/webp".to_owned(),
                    rng.between(8 * KIB, 240 * KIB),
                ),
                _ => (
                    AdminFileKind::EmbedThumbnail,
                    String::new(),
                    "image/webp".to_owned(),
                    rng.between(6 * KIB, 70 * KIB),
                ),
            };
            let in_guild = !matches!(kind, AdminFileKind::Avatar);
            self.files.push(AdminFileRow {
                key: format!("adm-{}", 1000 + i),
                kind,
                filename,
                bytes,
                content_type,
                guild: in_guild.then_some(guild),
                owner: Some(owner),
                channel_id: matches!(
                    kind,
                    AdminFileKind::Attachment | AdminFileKind::EmbedThumbnail
                )
                .then(|| 10 + rng.next(9)),
                message_id: matches!(
                    kind,
                    AdminFileKind::Attachment | AdminFileKind::EmbedThumbnail
                )
                .then(|| 5_000 + i * 7),
                created_at: time(now - age),
            });
        }
        self.files
            .sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.key.cmp(&b.key)));
    }

    fn seed_activity(&mut self, rng: &mut Lcg) {
        let today = utc(TODAY.0, TODAY.1, TODAY.2, 0, 0).div_euclid(DAY);
        for back in (0..90i64).rev() {
            let day = today - back;
            // 1970-01-01 was a Thursday: day % 7 == 2 is a Saturday, 3 a Sunday.
            let weekday = day.rem_euclid(7);
            let rhythm = match weekday {
                2 => 0.62,
                3 => 0.5,
                _ => 1.0,
            };
            let growth = 0.75 + 0.25 * (90 - back) as f64 / 90.0;
            let noise = 0.85 + rng.next(30) as f64 / 100.0;
            let messages = (1_800.0 * rhythm * growth * noise) as u64;
            let active = ((30.0 * rhythm * growth * noise) as u64).min(self.users.len() as u64);
            let signups = if rng.next(10) < if (2..=3).contains(&weekday) { 2 } else { 5 } {
                1 + rng.next(3)
            } else {
                0
            };
            let (y, m, d) = civil(day);
            self.activity.push(ActivityPoint {
                date: format!("{y:04}-{m:02}-{d:02}"),
                messages,
                signups,
                active_users: active,
            });
        }
    }

    fn seed_audit(&mut self, rng: &mut Lcg) {
        const REASONS: [&str; 6] = [
            "spam",
            "copyright complaint",
            "",
            "too large for the server's purpose",
            "contains personal data",
            "reported by a member",
        ];
        let now = today_unix();
        for i in 0..64i64 {
            let at = now - i * 11 * 3600 - rng.next(5 * 3600) as i64;
            let actor = if i % 3 == 0 {
                self.users.iter().find(|u| u.username == "marta")
            } else {
                self.users.iter().find(|u| u.username == "cvetka")
            }
            .cloned();
            let (action, target, reason) = match i % 9 {
                4 => ("admin.grant", "marta".to_owned(), String::new()),
                7 => (
                    "admin.revoke",
                    NAMES[rng.next(NAMES.len() as u64) as usize].to_owned(),
                    "left the team".to_owned(),
                ),
                _ => (
                    "file.delete",
                    format!("adm-{}", 2000 + i),
                    REASONS[rng.next(REASONS.len() as u64) as usize].to_owned(),
                ),
            };
            self.audit.push(AuditEntry {
                id: 0,
                actor,
                action: action.to_owned(),
                target,
                reason,
                created_at: time(at),
            });
        }
        // Newest first, ids ascending with age.
        self.audit.sort_by_key(|e| std::cmp::Reverse(e.created_at));
        let n = self.audit.len() as u64;
        for (i, e) in self.audit.iter_mut().enumerate() {
            e.id = n - i as u64;
        }
        self.next_audit = n + 1;
    }

    // ---- derived -----------------------------------------------------------------------------

    fn breakdown<'a>(files: impl Iterator<Item = &'a AdminFileRow>) -> StorageBreakdown {
        let mut s = StorageBreakdown::default();
        for f in files {
            match f.kind {
                AdminFileKind::Attachment => s.attachments += f.bytes,
                AdminFileKind::Emoji => s.emojis += f.bytes,
                AdminFileKind::GuildIcon => s.guild_icons += f.bytes,
                AdminFileKind::Avatar => s.avatars += f.bytes,
                AdminFileKind::EmbedThumbnail => s.embeds += f.bytes,
                AdminFileKind::Unknown => {}
            }
        }
        s
    }

    pub fn overview(&self) -> Overview {
        let storage = Self::breakdown(self.files.iter());
        let last = self.activity.last().cloned().unwrap_or(ActivityPoint {
            date: String::new(),
            messages: 0,
            signups: 0,
            active_users: 0,
        });
        Overview {
            users: self.users.len() as u64,
            guilds: self.guilds.len() as u64,
            channels: self.guild_stats.iter().map(|g| g.channels).sum(),
            messages: self.guild_stats.iter().map(|g| g.messages).sum(),
            storage_bytes_total: storage.total(),
            storage_bytes_avatars: storage.avatars,
            storage_bytes_guild_icons: storage.guild_icons,
            live_connections: self.live,
            messages_today: last.messages,
            signups_today: last.signups,
            active_users_today: last.active_users,
            storage,
        }
    }

    fn guild_rows(&self, query: &str) -> Vec<AdminGuildRow> {
        let q = query.trim().to_lowercase();
        let mut rows: Vec<AdminGuildRow> = self
            .guilds
            .iter()
            .zip(&self.guild_stats)
            .filter(|(g, _)| q.is_empty() || g.name.to_lowercase().contains(&q))
            .map(|(g, s)| {
                let storage = Self::breakdown(
                    self.files
                        .iter()
                        .filter(|f| f.guild.as_ref().is_some_and(|fg| fg.id == g.id)),
                );
                AdminGuildRow {
                    guild: g.clone(),
                    member_count: s.members,
                    channel_count: s.channels,
                    message_count: s.messages,
                    storage_bytes: storage.total(),
                    storage,
                }
            })
            .collect();
        rows.sort_by(|a, b| {
            b.storage_bytes
                .cmp(&a.storage_bytes)
                .then(a.guild.id.cmp(&b.guild.id))
        });
        rows
    }

    fn user_rows(&self, query: &str) -> Vec<AdminUserRow> {
        let q = query.trim().to_lowercase();
        let mut rows: Vec<AdminUserRow> = self
            .users
            .iter()
            .zip(&self.user_stats)
            .filter(|(u, _)| q.is_empty() || u.username.to_lowercase().contains(&q))
            .map(|(u, s)| AdminUserRow {
                user: u.clone(),
                guild_count: s.guilds,
                message_count: s.messages,
                storage_bytes: self
                    .files
                    .iter()
                    .filter(|f| f.owner.as_ref().is_some_and(|o| o.id == u.id))
                    .map(|f| f.bytes)
                    .sum(),
                last_active_at: s.last_active.map(time),
                suspended: self.suspended.contains_key(&u.id),
                suspended_until: self.suspended.get(&u.id).and_then(|s| s.1),
                suspended_reason: self.suspended.get(&u.id).map(|s| s.0.clone()).unwrap_or_default(),
                deleted: self.deleted.contains(&u.id),
            })
            .collect();
        rows.sort_by_key(|r| r.user.username.to_lowercase());
        rows
    }

    // ---- commands ----------------------------------------------------------------------------

    /// Answers one `Admin*` command for the signed-in `me`.
    pub fn handle(&mut self, me: &User, cmd: Command) -> Vec<Reply> {
        let refuse = if self.unavailable {
            Some(ApiError::Unimplemented)
        } else if !self.users.iter().any(|u| u.id == me.id && u.is_admin) {
            Some(ApiError::PermissionDenied)
        } else {
            None
        };
        let err = |e: &Option<ApiError>| e.clone().expect("checked");
        match cmd {
            Command::AdminOverview => vec![Reply::AdminOverview(
                refuse.map_or_else(|| Ok(self.overview()), Err),
            )],
            Command::AdminActivity { days } => {
                let days = if days == 0 { 30 } else { days.min(90) } as usize;
                let from = self.activity.len().saturating_sub(days);
                vec![Reply::AdminActivity {
                    days: days as u32,
                    result: refuse.map_or_else(|| Ok(self.activity[from..].to_vec()), Err),
                }]
            }
            Command::AdminGuilds {
                epoch,
                query,
                page_token,
                limit,
            } => {
                let result = if refuse.is_some() {
                    Err(err(&refuse))
                } else {
                    Ok(page(self.guild_rows(&query), page_token.as_deref(), limit))
                };
                vec![Reply::AdminGuilds { epoch, result }]
            }
            Command::AdminUsers {
                epoch,
                query,
                page_token,
                limit,
            } => {
                let result = if refuse.is_some() {
                    Err(err(&refuse))
                } else {
                    Ok(page(self.user_rows(&query), page_token.as_deref(), limit))
                };
                vec![Reply::AdminUsers { epoch, result }]
            }
            Command::AdminFiles {
                epoch,
                guild,
                kind,
                page_token,
                limit,
            } => {
                let result = if refuse.is_some() {
                    Err(err(&refuse))
                } else {
                    let rows: Vec<AdminFileRow> = self
                        .files
                        .iter()
                        .filter(|f| {
                            guild.is_none_or(|g| f.guild.as_ref().is_some_and(|fg| fg.id == g))
                                && kind.is_none_or(|k| f.kind == k)
                        })
                        .cloned()
                        .collect();
                    Ok(page(rows, page_token.as_deref(), limit))
                };
                vec![Reply::AdminFiles { epoch, result }]
            }
            Command::AdminAudit {
                epoch,
                page_token,
                limit,
            } => {
                let result = if refuse.is_some() {
                    Err(err(&refuse))
                } else {
                    Ok(page(self.audit.clone(), page_token.as_deref(), limit))
                };
                vec![Reply::AdminAudit { epoch, result }]
            }
            Command::AdminSetAdmin { user, is_admin } => {
                let result = match refuse {
                    Some(e) => Err(e),
                    None => self.set_admin(me, user, is_admin),
                };
                vec![Reply::AdminAdminSet {
                    user,
                    is_admin,
                    result,
                }]
            }
            Command::AdminSuspendUser { user, reason, until } => {
                let result = match refuse {
                    Some(e) => Err(e),
                    None => self.suspend(me, user, &reason, until),
                };
                vec![Reply::AdminUserSuspended { user, result }]
            }
            Command::AdminUnsuspendUser { user } => {
                let result = match refuse {
                    Some(e) => Err(e),
                    None => self.unsuspend(me, user),
                };
                vec![Reply::AdminUserUnsuspended { user, result }]
            }
            Command::AdminDeleteUser { user, force_transfer, reason } => {
                let result = match refuse {
                    Some(e) => Err(e),
                    None => self.delete_user(me, user, force_transfer, &reason),
                };
                vec![Reply::AdminUserDeleted { user, result }]
            }
            Command::AdminDeleteFile { key, reason } => {
                let result = match refuse {
                    Some(e) => Err(e),
                    None => self.delete_file(me, &key, &reason),
                };
                vec![Reply::AdminFileDeleted { key, result }]
            }
            _ => Vec::new(),
        }
    }

    /// The ids of deleted users.
    pub fn deleted_ids(&self) -> Vec<u64> {
        self.deleted.iter().copied().collect()
    }

    fn row_of(&self, id: u64) -> Result<AdminUserRow, ApiError> {
        self.user_rows("").into_iter().find(|r| r.user.id == id).ok_or(ApiError::NotFound)
    }

    /// The target of a suspension or deletion: it exists, is not deleted, and (the server's rule)
    /// is not a site admin.
    fn live_target(&self, id: u64) -> Result<&User, ApiError> {
        match self.users.iter().find(|u| u.id == id) {
            Some(u) if !self.deleted.contains(&id) => Ok(u),
            _ => Err(ApiError::NotFound),
        }
    }

    fn suspend(&mut self, me: &User, target: u64, reason: &str, until: Option<SystemTime>) -> Result<AdminUserRow, ApiError> {
        let t = self.live_target(target)?;
        if t.is_admin || target == me.id {
            return Err(ApiError::Precondition("site admins cannot be suspended: remove their admin rights first".into()));
        }
        if reason.chars().count() > 512 {
            return Err(ApiError::InvalidArgument("reason is too long".into()));
        }
        if until.is_some_and(|t| t <= SystemTime::now()) {
            return Err(ApiError::InvalidArgument("until must be in the future".into()));
        }
        let name = t.username.clone();
        self.suspended.insert(target, (reason.trim().to_owned(), until));
        self.log(me, "user.suspend", format!("user:{target}"), reason.trim().to_owned());
        let _ = name;
        self.row_of(target)
    }

    fn unsuspend(&mut self, me: &User, target: u64) -> Result<AdminUserRow, ApiError> {
        self.live_target(target)?;
        self.suspended.remove(&target);
        self.log(me, "user.unsuspend", format!("user:{target}"), String::new());
        self.row_of(target)
    }

    fn delete_user(&mut self, me: &User, target: u64, force: bool, reason: &str) -> Result<(), ApiError> {
        let t = self.live_target(target)?;
        if t.is_admin {
            return Err(ApiError::Precondition("site admins cannot be deleted: remove their admin rights first".into()));
        }
        let owned: Vec<thehideout_api::OwnedGuild> = self
            .guilds
            .iter()
            .filter(|g| g.owner_id == target)
            .map(|g| thehideout_api::OwnedGuild { id: g.id, name: g.name.clone(), member_count: 2 })
            .collect();
        if !owned.is_empty() {
            if !force {
                return Err(ApiError::OwnsGuilds(owned));
            }
            // Guilds pass to the first other user (the server picks the highest role).
            let heir = self.users.iter().map(|u| u.id).find(|id| *id != target && !self.deleted.contains(id));
            for g in self.guilds.iter_mut().filter(|g| g.owner_id == target) {
                if let Some(heir) = heir {
                    g.owner_id = heir;
                }
            }
        }
        self.deleted.insert(target);
        self.suspended.remove(&target);
        if let Some(u) = self.users.iter_mut().find(|u| u.id == target) {
            u.username = "Deleted user".to_owned();
            u.avatar = None;
            u.email = None;
            u.deleted = true;
        }
        self.log(me, "account.delete", format!("user:{target}"), reason.trim().to_owned());
        Ok(())
    }

    fn set_admin(&mut self, me: &User, target: u64, make: bool) -> Result<User, ApiError> {
        let refused = std::mem::take(&mut self.refuse_next_set);
        let admins = self.users.iter().filter(|u| u.is_admin).count();
        let current = self
            .users
            .iter()
            .find(|u| u.id == target)
            .ok_or(ApiError::NotFound)?
            .is_admin;
        if !make && (refused || target == me.id || (current && admins <= 1)) {
            return Err(ApiError::Precondition(if target == me.id {
                "cannot demote yourself".into()
            } else {
                "cannot demote the last admin".into()
            }));
        }
        let user = self
            .users
            .iter_mut()
            .find(|u| u.id == target)
            .ok_or(ApiError::NotFound)?;
        if user.is_admin != make {
            user.is_admin = make;
            let target_name = user.username.clone();
            self.log(
                me,
                if make { "admin.grant" } else { "admin.revoke" },
                target_name,
                String::new(),
            );
        }
        Ok(self
            .users
            .iter()
            .find(|u| u.id == target)
            .cloned()
            .expect("just found"))
    }

    fn delete_file(&mut self, me: &User, key: &str, reason: &str) -> Result<(), ApiError> {
        if self.fail_deletes > 0 {
            self.fail_deletes -= 1;
            return Err(ApiError::Network("connection reset".into()));
        }
        let at = self
            .files
            .iter()
            .position(|f| f.key == key)
            .ok_or(ApiError::NotFound)?;
        self.files.remove(at);
        self.log(me, "file.delete", key.to_owned(), reason.to_owned());
        Ok(())
    }

    fn log(&mut self, actor: &User, action: &str, target: String, reason: String) {
        let id = self.next_audit;
        self.next_audit += 1;
        self.audit.insert(
            0,
            AuditEntry {
                id,
                actor: Some(actor.clone()),
                action: action.to_owned(),
                target,
                reason,
                created_at: time(today_unix()),
            },
        );
    }

    // ---- for scenes and tests ----------------------------------------------------------------

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn has_file(&self, key: &str) -> bool {
        self.files.iter().any(|f| f.key == key)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn audit_actions(&self) -> Vec<(String, String, String)> {
        self.audit
            .iter()
            .map(|e| (e.action.clone(), e.target.clone(), e.reason.clone()))
            .collect()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_admin(&self, id: u64) -> bool {
        self.users.iter().any(|u| u.id == id && u.is_admin)
    }
}

/// `items[offset..offset+limit]` with the next offset as the token (`o:N`).
fn page<T: Clone>(items: Vec<T>, token: Option<&str>, limit: u32) -> Paged<T> {
    let start = token
        .and_then(|t| t.strip_prefix("o:"))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(0)
        .min(items.len());
    let limit = if limit == 0 { 50 } else { limit as usize };
    let end = (start + limit).min(items.len());
    Paged {
        items: items[start..end].to_vec(),
        next_page_token: (end < items.len()).then(|| format!("o:{end}")),
    }
}

/// Civil date of a day number (days since 1970-01-01).
fn civil(day: i64) -> (i64, u32, u32) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> (AdminWorld, User) {
        let base = vec![
            user(1, "tim", false, None),
            user(2, "ana", false, None),
            user(3, "bruno", false, None),
            user(4, "cvetka", true, None),
        ];
        let guilds: Vec<Guild> = (1..=3)
            .map(|id| Guild {
                id,
                name: format!("g{id}"),
                owner_id: 1,
                invite_code: String::new(),
                created_at: UNIX_EPOCH,
                icon: None,
            })
            .collect();
        let w = AdminWorld::new(&base, &guilds);
        (w, base[3].clone())
    }

    #[test]
    fn the_seed_is_big_deterministic_and_spans_bytes_to_gibibytes() {
        let (a, _) = world();
        let (b, _) = world();
        assert!(a.users.len() >= 40);
        assert_eq!(a.guilds.len(), 12);
        assert_eq!(a.files.len(), 300);
        assert_eq!(a.activity.len(), 90);
        assert_eq!(
            a.files.iter().map(|f| f.key.clone()).collect::<Vec<_>>(),
            b.files.iter().map(|f| f.key.clone()).collect::<Vec<_>>()
        );
        assert!(a.files.first().unwrap().bytes > GIB);
        assert!(a.files.last().unwrap().bytes < 100);
        assert!(
            a.files.windows(2).all(|w| w[0].bytes >= w[1].bytes),
            "sorted largest first"
        );
        assert!(a.audit.len() >= 30);
        assert_eq!(a.activity.last().unwrap().date, "2026-11-19");
    }

    #[test]
    fn weekdays_are_busier_than_weekends() {
        let (w, _) = world();
        let avg = |pred: fn(i64) -> bool| {
            let v: Vec<u64> = w
                .activity
                .iter()
                .filter(|p| pred(crate::format::parse_date(&p.date).unwrap().rem_euclid(7)))
                .map(|p| p.messages)
                .collect();
            v.iter().sum::<u64>() / v.len() as u64
        };
        assert!(avg(|d| d != 2 && d != 3) > avg(|d| d == 2 || d == 3) * 3 / 2);
    }

    #[test]
    fn totals_follow_the_files_and_guild_rows_sum_to_them() {
        let (mut w, me) = world();
        let before = w.overview();
        let guild_sum: u64 = w.guild_rows("").iter().map(|r| r.storage_bytes).sum();
        let avatars = before.storage.avatars;
        assert_eq!(
            guild_sum + avatars,
            before.storage_bytes_total,
            "avatars belong to no server"
        );
        let big = w.files[0].clone();
        w.delete_file(&me, &big.key, "test").unwrap();
        assert_eq!(
            w.overview().storage_bytes_total,
            before.storage_bytes_total - big.bytes
        );
    }

    #[test]
    fn set_admin_refuses_self_and_the_last_admin() {
        let (mut w, me) = world();
        assert!(matches!(
            w.set_admin(&me, me.id, false),
            Err(ApiError::Precondition(_))
        ));
        let ana = w.users.iter().find(|u| u.username == "ana").unwrap().id;
        assert!(w.set_admin(&me, ana, true).unwrap().is_admin);
        assert!(!w.set_admin(&me, ana, false).unwrap().is_admin);
        let acts = w.audit_actions();
        assert_eq!(acts[0].0, "admin.revoke");
        assert_eq!(acts[1].0, "admin.grant");
    }

    #[test]
    fn non_admins_are_refused_and_pages_chain() {
        let (mut w, _) = world();
        let tim = w.users[0].clone();
        let r = w.handle(&tim, Command::AdminOverview);
        assert!(matches!(
            &r[0],
            Reply::AdminOverview(Err(ApiError::PermissionDenied))
        ));
        let p = page((0..10).collect::<Vec<_>>(), None, 4);
        assert_eq!(
            (p.items.len(), p.next_page_token.as_deref()),
            (4, Some("o:4"))
        );
        let p = page((0..10).collect::<Vec<_>>(), Some("o:8"), 4);
        assert_eq!((p.items, p.next_page_token), (vec![8, 9], None));
    }
}
