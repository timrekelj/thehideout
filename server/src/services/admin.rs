//! Site-admin dashboard: overview, activity, guild/user/file listings, file
//! deletion, admin rights and the audit log. Every call requires a site admin.
//!
//! Listings of guilds and users scan their table and filter, sort and page in
//! memory: fine for the scale this server targets (thousands of rows), and it
//! keeps the query substring match case-insensitive without an index. Pages
//! are cursors on the id (ascending), so they stay stable while rows are added.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use chrono::{DateTime, Duration, Utc};
use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use futures::{StreamExt, TryStreamExt};

use crate::AppState;
use crate::admin::{self, require_admin};
use crate::auth::internal;
use crate::convert::{datetime_to_proto, id_from_db, id_to_db};
use crate::media::{self, KindTotals, files};
use crate::proto::thehideout::v1::*;
use crate::services::emoji::{EmojiSvc, list_emojis};
use crate::services::guild::load_guild;
use crate::services::media::MediaSvc;
use crate::services::message::publish_message_updated;
use crate::snowflake::bucket_of;
use crate::stats;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const DEFAULT_DAYS: u32 = 30;
const MAX_DAYS: u32 = 90;
/// Rows of a page loaded at the same time.
const CONCURRENCY: usize = 16;

pub struct AdminSvc {
    state: AppState,
}

impl AdminSvc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

fn page_limit(limit: u32) -> usize {
    match limit as usize {
        0 => DEFAULT_LIMIT,
        n => n.min(MAX_LIMIT),
    }
}

fn bad_token() -> ConnectError {
    ConnectError::invalid_argument("invalid page_token")
}

fn breakdown(kinds: &[KindTotals]) -> StorageBreakdown {
    let of = |name: &str| kinds.iter().find(|k| k.kind == name).map_or(0, |k| k.bytes.max(0) as u64);
    StorageBreakdown {
        attachments: of("attachment"),
        emojis: of("emoji"),
        guild_icons: of("guild_icon"),
        avatars: of("avatar"),
        embeds: of("embed"),
        ..Default::default()
    }
}

fn file_kind(label: &str) -> AdminFileKind {
    match label {
        "attachment" => AdminFileKind::ADMIN_FILE_KIND_ATTACHMENT,
        "emoji" => AdminFileKind::ADMIN_FILE_KIND_EMOJI,
        "guild_icon" => AdminFileKind::ADMIN_FILE_KIND_GUILD_ICON,
        "avatar" => AdminFileKind::ADMIN_FILE_KIND_AVATAR,
        "embed" => AdminFileKind::ADMIN_FILE_KIND_EMBED_THUMBNAIL,
        _ => AdminFileKind::ADMIN_FILE_KIND_UNSPECIFIED,
    }
}

fn kind_label(kind: AdminFileKind) -> Option<&'static str> {
    match kind {
        AdminFileKind::ADMIN_FILE_KIND_ATTACHMENT => Some("attachment"),
        AdminFileKind::ADMIN_FILE_KIND_EMOJI => Some("emoji"),
        AdminFileKind::ADMIN_FILE_KIND_GUILD_ICON => Some("guild_icon"),
        AdminFileKind::ADMIN_FILE_KIND_AVATAR => Some("avatar"),
        AdminFileKind::ADMIN_FILE_KIND_EMBED_THUMBNAIL => Some("embed"),
        AdminFileKind::ADMIN_FILE_KIND_UNSPECIFIED => None,
    }
}

/// `(id, rest)` rows of a full-table scan.
async fn scan_rows<T: for<'f, 'm> scylla::deserialize::row::DeserializeRow<'f, 'm>>(
    state: &AppState,
    cql: &'static str,
) -> Result<Vec<T>, ConnectError> {
    let result = state.session.execute_unpaged(cql, ()).await.map_err(internal)?.into_rows_result().map_err(internal)?;
    let rows = result.rows::<T>().map_err(internal)?.collect::<Result<Vec<T>, _>>().map_err(internal)?;
    Ok(rows)
}

impl AdminSvc {
    /// Fills the suspension and deletion fields of a user row.
    async fn standing_into(&self, id: u64, row: &mut AdminUserRow) -> Result<(), ConnectError> {
        if let Some(account) = crate::accounts::load_any(&self.state, id).await? {
            row.deleted = account.deletion_state.is_some();
            if let Some((reason, until)) = account.active_suspension() {
                row.suspended = true;
                row.suspended_reason = reason;
                row.suspended_until = until.map(datetime_to_proto).into();
            }
        }
        Ok(())
    }

    /// The full admin row of one user (without the usage numbers of listings).
    async fn user_row(&self, id: u64) -> Result<AdminUserRow, ConnectError> {
        let user = self.user(id).await?.ok_or_else(|| ConnectError::not_found("user not found"))?;
        let mut row = AdminUserRow { user: user.into(), ..Default::default() };
        self.standing_into(id, &mut row).await?;
        Ok(row)
    }

    /// The account a destructive admin call targets: it must exist and not be deleted.
    async fn target(&self, id: u64) -> Result<crate::accounts::Account, ConnectError> {
        crate::accounts::load(&self.state, id).await?.ok_or_else(|| ConnectError::not_found("user not found"))
    }

    async fn user(&self, id: u64) -> Result<Option<User>, ConnectError> {
        admin::load_user(&self.state, id).await
    }

    /// Removes the objects, accounting and owner reference of `key`.
    async fn remove_file(&self, entry: &files::Entry) -> Result<(), ConnectError> {
        let state = &self.state;
        let key = entry.key.as_str();
        // Other objects that go with this one.
        let mut also: Vec<String> = Vec::new();
        let mut updated_message = None;

        match entry.label {
            "attachment" => {
                if let (Some(channel_id), Some(message_id)) = (entry.channel_id, entry.message_id) {
                    let rows = state
                        .session
                        .execute_unpaged(
                            "SELECT position, file, thumbnail FROM message_attachments WHERE channel_id = ? AND bucket = ? AND message_id = ?",
                            (id_to_db(channel_id), bucket_of(message_id), id_to_db(message_id)),
                        )
                        .await
                        .map_err(internal)?
                        .into_rows_result()
                        .map_err(internal)?
                        .rows::<(i32, Option<String>, Option<String>)>()
                        .map_err(internal)?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(internal)?;
                    for (position, file, thumbnail) in rows {
                        if file.as_deref() != Some(key) && thumbnail.as_deref() != Some(key) {
                            continue;
                        }
                        also.extend(file.into_iter().chain(thumbnail).filter(|k| k != key && !k.is_empty()));
                        state
                            .session
                            .execute_unpaged(
                                "UPDATE message_attachments SET removed = true, file = '', thumbnail = null \
                                 WHERE channel_id = ? AND bucket = ? AND message_id = ? AND position = ?",
                                (id_to_db(channel_id), bucket_of(message_id), id_to_db(message_id), position),
                            )
                            .await
                            .map_err(internal)?;
                        updated_message = Some((channel_id, message_id));
                    }
                }
            }
            "embed" => {
                if let (Some(channel_id), Some(message_id)) = (entry.channel_id, entry.message_id) {
                    let (channel, bucket, message) = (id_to_db(channel_id), bucket_of(message_id), id_to_db(message_id));
                    let embeds = state
                        .session
                        .execute_unpaged(
                            "SELECT position, thumbnail, author_icon FROM message_embeds WHERE channel_id = ? AND bucket = ? AND message_id = ?",
                            (channel, bucket, message),
                        )
                        .await
                        .map_err(internal)?
                        .into_rows_result()
                        .map_err(internal)?
                        .rows::<(i32, Option<String>, Option<String>)>()
                        .map_err(internal)?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(internal)?;
                    for (position, thumbnail, author_icon) in embeds {
                        let cql = if thumbnail.as_deref() == Some(key) {
                            "UPDATE message_embeds SET thumbnail = null, thumbnail_width = null, thumbnail_height = null \
                             WHERE channel_id = ? AND bucket = ? AND message_id = ? AND position = ?"
                        } else if author_icon.as_deref() == Some(key) {
                            "UPDATE message_embeds SET author_icon = null WHERE channel_id = ? AND bucket = ? AND message_id = ? AND position = ?"
                        } else {
                            continue;
                        };
                        state.session.execute_unpaged(cql, (channel, bucket, message, position)).await.map_err(internal)?;
                        updated_message = Some((channel_id, message_id));
                    }
                    let media_rows = state
                        .session
                        .execute_unpaged(
                            "SELECT position, idx, file, poster FROM message_embed_media WHERE channel_id = ? AND bucket = ? AND message_id = ?",
                            (channel, bucket, message),
                        )
                        .await
                        .map_err(internal)?
                        .into_rows_result()
                        .map_err(internal)?
                        .rows::<(i32, i32, Option<String>, Option<String>)>()
                        .map_err(internal)?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(internal)?;
                    for (position, idx, file, poster) in media_rows {
                        let cql = if file.as_deref() == Some(key) {
                            // The media item goes, and with it its poster.
                            also.extend(poster.filter(|p| !p.is_empty()));
                            "DELETE FROM message_embed_media WHERE channel_id = ? AND bucket = ? AND message_id = ? AND position = ? AND idx = ?"
                        } else if poster.as_deref() == Some(key) {
                            "UPDATE message_embed_media SET poster = null WHERE channel_id = ? AND bucket = ? AND message_id = ? AND position = ? AND idx = ?"
                        } else {
                            continue;
                        };
                        state.session.execute_unpaged(cql, (channel, bucket, message, position, idx)).await.map_err(internal)?;
                        updated_message = Some((channel_id, message_id));
                    }
                }
            }
            _ => {}
        }

        // Objects first: if the store is down nothing has changed yet.
        if let Some(storage) = &state.storage {
            for k in std::iter::once(&key.to_owned()).chain(&also) {
                storage.delete(k).await.map_err(|e| {
                    tracing::warn!(key = %k, error = %format!("{e:#}"), "deleting object failed");
                    ConnectError::unavailable("media storage is unavailable")
                })?;
            }
        }

        match entry.label {
            "emoji" => {
                let guild_id = entry.guild_id.unwrap_or(entry.owner_id);
                let emojis = list_emojis(state, guild_id).await?;
                if let Some(emoji) = emojis.iter().find(|e| e.image == key) {
                    EmojiSvc::new(state.clone()).remove_emoji(guild_id, emoji).await?;
                }
            }
            "guild_icon" => {
                let guild_id = entry.owner_id;
                if let Some(guild) = load_guild(state, id_to_db(guild_id)).await?
                    && guild.icon == key
                {
                    MediaSvc::new(state.clone()).set_icon(guild_id, None).await?;
                }
            }
            "avatar" => {
                let user_id = entry.owner_id;
                if self.user(user_id).await?.is_some_and(|u| u.avatar == key) {
                    MediaSvc::new(state.clone()).set_avatar(user_id, None).await?;
                }
            }
            _ => {}
        }
        media::delete_record(state, key).await;
        for k in &also {
            media::delete_record(state, k).await;
        }
        if let Some((channel_id, message_id)) = updated_message
            && let Err(e) = publish_message_updated(state, channel_id, message_id).await
        {
            tracing::warn!(channel_id, message_id, error = %e.message.as_deref().unwrap_or("?"), "publishing message_updated failed");
        }
        Ok(())
    }

    async fn file_row(&self, e: files::Entry) -> Result<AdminFileRow, ConnectError> {
        let guild = match e.guild_id {
            Some(id) => load_guild(&self.state, id_to_db(id)).await?,
            None => None,
        };
        let owner = match e.user_id {
            Some(id) => self.user(id).await?,
            None => None,
        };
        Ok(AdminFileRow {
            key: e.key,
            kind: file_kind(e.label).into(),
            filename: e.filename,
            bytes: e.bytes.max(0) as u64,
            content_type: e.content_type,
            guild: guild.into(),
            owner: owner.into(),
            channel_id: e.channel_id,
            message_id: e.message_id,
            created_at: datetime_to_proto(e.created_at).into(),
            ..Default::default()
        })
    }

    async fn file_rows(&self, entries: Vec<files::Entry>) -> Result<Vec<AdminFileRow>, ConnectError> {
        let mut rows = Vec::with_capacity(entries.len());
        for chunk in entries.chunks(CONCURRENCY) {
            rows.extend(futures::future::try_join_all(chunk.iter().cloned().map(|e| self.file_row(e))).await?);
        }
        Ok(rows)
    }
}

/// Cursor page over `rows` sorted by id: those after `token`, `limit` of them,
/// and the next token.
fn paginate<T>(mut rows: Vec<(u64, T)>, token: &str, limit: usize) -> Result<(Vec<(u64, T)>, String), ConnectError> {
    rows.sort_by_key(|(id, _)| *id);
    let after: u64 = if token.is_empty() { 0 } else { token.parse().map_err(|_| bad_token())? };
    let mut rest: Vec<(u64, T)> = rows.into_iter().filter(|(id, _)| token.is_empty() || *id > after).collect();
    let more = rest.len() > limit;
    rest.truncate(limit);
    let next = if more { rest.last().map(|(id, _)| id.to_string()).unwrap_or_default() } else { String::new() };
    Ok((rest, next))
}

impl AdminService for AdminSvc {
    async fn get_overview(
        &self,
        ctx: RequestContext,
        _req: ServiceRequest<'_, GetOverviewRequest>,
    ) -> ServiceResult<GetOverviewResponse> {
        require_admin(&self.state, &ctx).await?;
        let site = stats::scope(&self.state, "site").await?;
        let today = stats::scope(&self.state, &format!("day:{}", stats::today())).await?;
        let kinds = media::totals_by_kind(&self.state, None).await?;
        let (total, _) = media::totals(&self.state, "global").await?;
        let storage = breakdown(&kinds);
        let n = |m: &HashMap<String, i64>, k: &str| m.get(k).copied().unwrap_or(0) as u64;
        Response::ok(GetOverviewResponse {
            users: n(&site, "users"),
            guilds: n(&site, "guilds"),
            channels: n(&site, "channels"),
            messages: n(&site, "messages"),
            storage_bytes_total: total.max(0) as u64,
            storage_bytes_avatars: storage.avatars,
            storage_bytes_guild_icons: storage.guild_icons,
            live_connections: self.state.live.connections.load(Ordering::Relaxed),
            messages_today: n(&today, "messages"),
            signups_today: n(&today, "signups"),
            active_users_today: n(&today, "active_users"),
            storage: storage.into(),
            ..Default::default()
        })
    }

    async fn get_activity(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, GetActivityRequest>,
    ) -> ServiceResult<GetActivityResponse> {
        require_admin(&self.state, &ctx).await?;
        let days = match req.days {
            0 => DEFAULT_DAYS,
            n if n <= MAX_DAYS => n,
            _ => return Err(ConnectError::invalid_argument("days must be at most 90")),
        };
        let today = Utc::now();
        let dates: Vec<String> = (0..days).rev().map(|back| stats::day_of(today - Duration::days(back as i64))).collect();
        let scopes: Vec<HashMap<String, i64>> = futures::stream::iter(dates.iter().cloned())
            .map(|d| async move { stats::scope(&self.state, &format!("day:{d}")).await })
            .buffered(CONCURRENCY)
            .try_collect()
            .await?;
        let points = dates
            .into_iter()
            .zip(scopes)
            .map(|(date, c)| ActivityPoint {
                date,
                messages: c.get("messages").copied().unwrap_or(0) as u64,
                signups: c.get("signups").copied().unwrap_or(0) as u64,
                active_users: c.get("active_users").copied().unwrap_or(0) as u64,
                ..Default::default()
            })
            .collect();
        Response::ok(GetActivityResponse { points, ..Default::default() })
    }

    async fn list_guilds(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ListGuildsRequest>,
    ) -> ServiceResult<ListGuildsResponse> {
        require_admin(&self.state, &ctx).await?;
        let query = req.query.trim().to_lowercase();
        let rows = scan_rows::<(i64, Option<String>)>(&self.state, "SELECT id, name FROM guilds").await?;
        let rows: Vec<(u64, ())> = rows
            .into_iter()
            .filter(|(_, name)| query.is_empty() || name.as_deref().unwrap_or("").to_lowercase().contains(&query))
            .map(|(id, _)| (id_from_db(id), ()))
            .collect();
        let (page, next_page_token) = paginate(rows, req.page_token, page_limit(req.limit))?;
        let guilds = futures::stream::iter(page)
            .map(|(id, _)| async move {
                let Some(guild) = load_guild(&self.state, id_to_db(id)).await? else {
                    return Ok(None);
                };
                let c = stats::scope(&self.state, &format!("guild:{id}")).await?;
                let (bytes, _) = media::totals(&self.state, &format!("guild:{id}")).await?;
                let kinds = media::totals_by_kind(&self.state, Some(id)).await?;
                let mut storage = breakdown(&kinds);
                storage.avatars = 0;
                Ok::<_, ConnectError>(Some(AdminGuildRow {
                    guild: guild.into(),
                    member_count: c.get("members").copied().unwrap_or(0) as u64,
                    channel_count: c.get("channels").copied().unwrap_or(0) as u64,
                    message_count: c.get("messages").copied().unwrap_or(0) as u64,
                    storage_bytes: bytes.max(0) as u64,
                    storage: storage.into(),
                    ..Default::default()
                }))
            })
            .buffered(CONCURRENCY)
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .flatten()
            .collect();
        Response::ok(ListGuildsResponse { guilds, next_page_token, ..Default::default() })
    }

    async fn list_users(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ListUsersRequest>,
    ) -> ServiceResult<ListUsersResponse> {
        require_admin(&self.state, &ctx).await?;
        let query = req.query.trim().to_lowercase();
        let rows = scan_rows::<(i64, Option<String>)>(&self.state, "SELECT id, username FROM users").await?;
        let rows: Vec<(u64, ())> = rows
            .into_iter()
            .filter(|(_, name)| query.is_empty() || name.as_deref().unwrap_or("").to_lowercase().contains(&query))
            .map(|(id, _)| (id_from_db(id), ()))
            .collect();
        let (page, next_page_token) = paginate(rows, req.page_token, page_limit(req.limit))?;
        let users = futures::stream::iter(page)
            .map(|(id, _)| async move {
                let Some(user) = self.user(id).await? else {
                    return Ok(None);
                };
                let messages = stats::scope(&self.state, &format!("user:{id}")).await?.get("messages").copied().unwrap_or(0);
                let guild_count = self
                    .state
                    .session
                    .execute_unpaged("SELECT guild_id FROM guilds_by_user WHERE user_id = ?", (id_to_db(id),))
                    .await
                    .map_err(internal)?
                    .into_rows_result()
                    .map_err(internal)?
                    .rows_num();
                let storage_bytes = if user.avatar.is_empty() {
                    0
                } else {
                    files::entry(&self.state, &user.avatar).await?.map_or(0, |e| e.bytes.max(0) as u64)
                };
                let last_active_at = self
                    .state
                    .session
                    .execute_unpaged("SELECT last_active_at FROM user_activity WHERE user_id = ?", (id_to_db(id),))
                    .await
                    .map_err(internal)?
                    .into_rows_result()
                    .map_err(internal)?
                    .maybe_first_row::<(Option<DateTime<Utc>>,)>()
                    .map_err(internal)?
                    .and_then(|(t,)| t);
                let mut row = AdminUserRow {
                    user: user.into(),
                    guild_count: guild_count as u64,
                    message_count: messages as u64,
                    storage_bytes,
                    last_active_at: last_active_at.map(datetime_to_proto).into(),
                    ..Default::default()
                };
                self.standing_into(id, &mut row).await?;
                Ok::<_, ConnectError>(Some(row))
            })
            .buffered(CONCURRENCY)
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .flatten()
            .collect();
        Response::ok(ListUsersResponse { users, next_page_token, ..Default::default() })
    }

    async fn set_admin(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, SetAdminRequest>,
    ) -> ServiceResult<SetAdminResponse> {
        let actor = require_admin(&self.state, &ctx).await?;
        let _guard = self.state.admin_lock.lock().await;
        let target = self.user(req.user_id).await?.ok_or_else(|| ConnectError::not_found("user not found"))?;
        if target.is_admin != req.is_admin {
            if !req.is_admin {
                if req.user_id == actor {
                    return Err(ConnectError::failed_precondition("admins cannot demote themselves"));
                }
                if admin::admin_ids(&self.state).await?.iter().all(|id| *id == req.user_id) {
                    return Err(ConnectError::failed_precondition("cannot demote the last admin"));
                }
            }
            admin::set_flag(&self.state, req.user_id, req.is_admin).await?;
            let action = if req.is_admin { "admin.grant" } else { "admin.revoke" };
            admin::audit(&self.state, actor, action, &req.user_id.to_string(), "").await?;
        } else if !req.is_admin && req.user_id == actor {
            return Err(ConnectError::failed_precondition("admins cannot demote themselves"));
        }
        let user = self.user(req.user_id).await?.ok_or_else(|| ConnectError::not_found("user not found"))?;
        // The user's own streams learn about the change.
        self.state.events.notify_user_profile_updated(req.user_id, user.clone());
        Response::ok(SetAdminResponse { user: user.into(), ..Default::default() })
    }

    async fn list_files(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ListFilesRequest>,
    ) -> ServiceResult<ListFilesResponse> {
        require_admin(&self.state, &ctx).await?;
        let limit = page_limit(req.limit);
        let label = match req.kind {
            Some(kind) => match kind.as_known() {
                Some(known) => kind_label(known),
                None => return Err(ConnectError::invalid_argument("unknown file kind")),
            },
            None => None,
        };
        let scope = match (req.guild_id, label) {
            (Some(g), Some(l)) => format!("guild:{g}:{l}"),
            (Some(g), None) => format!("guild:{g}"),
            (None, Some(l)) => format!("global:{l}"),
            (None, None) => "global".to_owned(),
        };
        let after = if req.page_token.is_empty() {
            None
        } else {
            let (bytes, key) = req.page_token.split_once(':').ok_or_else(bad_token)?;
            Some((bytes.parse::<i64>().map_err(|_| bad_token())?, key.to_owned()))
        };
        let mut found = files::page(&self.state, &scope, after.as_ref(), limit + 1).await?;
        let more = found.len() > limit;
        found.truncate(limit);
        let next_page_token = match (more, found.last()) {
            (true, Some((bytes, key))) => format!("{bytes}:{key}"),
            _ => String::new(),
        };
        let keys: Vec<String> = found.into_iter().map(|(_, key)| key).collect();
        let entries = files::entries(&self.state, &keys).await?;
        let files = self.file_rows(entries).await?;
        Response::ok(ListFilesResponse { files, next_page_token, ..Default::default() })
    }

    async fn delete_file(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, DeleteFileRequest>,
    ) -> ServiceResult<DeleteFileResponse> {
        let actor = require_admin(&self.state, &ctx).await?;
        let entry = files::entry(&self.state, req.key)
            .await?
            .ok_or_else(|| ConnectError::not_found("file not found"))?;
        self.remove_file(&entry).await?;
        admin::audit(&self.state, actor, "file.delete", req.key, req.reason).await?;
        Response::ok(DeleteFileResponse::default())
    }

    async fn suspend_user(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, SuspendUserRequest>,
    ) -> ServiceResult<SuspendUserResponse> {
        let actor = require_admin(&self.state, &ctx).await?;
        let target = self.target(req.user_id).await?;
        if target.is_admin || target.id == actor {
            return Err(ConnectError::failed_precondition("site admins cannot be suspended: remove their admin rights first"));
        }
        if req.reason.chars().count() > crate::moderation::MAX_REASON_CHARS {
            return Err(ConnectError::invalid_argument("reason is too long"));
        }
        let until = match req.until.as_option() {
            Some(ts) => {
                let until = DateTime::from_timestamp(ts.seconds, ts.nanos.max(0) as u32).ok_or_else(|| ConnectError::invalid_argument("invalid until"))?;
                if until <= Utc::now() {
                    return Err(ConnectError::invalid_argument("until must be in the future"));
                }
                Some(until)
            }
            None => None,
        };
        crate::lifecycle::suspend(&self.state, actor, target.id, req.reason.trim(), until).await?;
        Response::ok(SuspendUserResponse { user: self.user_row(target.id).await?.into(), ..Default::default() })
    }

    async fn unsuspend_user(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, UnsuspendUserRequest>,
    ) -> ServiceResult<UnsuspendUserResponse> {
        let actor = require_admin(&self.state, &ctx).await?;
        let target = self.target(req.user_id).await?;
        crate::lifecycle::unsuspend(&self.state, actor, target.id).await?;
        Response::ok(UnsuspendUserResponse { user: self.user_row(target.id).await?.into(), ..Default::default() })
    }

    async fn admin_delete_user(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, AdminDeleteUserRequest>,
    ) -> ServiceResult<AdminDeleteUserResponse> {
        let actor = require_admin(&self.state, &ctx).await?;
        let target = self.target(req.user_id).await?;
        let bootstrap = self.state.admins.contains(&target.username)
            || (self.state.policy.bootstrap_admin_email.is_some() && target.email == self.state.policy.bootstrap_admin_email);
        if target.is_admin || bootstrap {
            return Err(ConnectError::failed_precondition("site admins cannot be deleted: remove their admin rights first"));
        }
        if req.reason.chars().count() > crate::moderation::MAX_REASON_CHARS {
            return Err(ConnectError::invalid_argument("reason is too long"));
        }
        let owned = crate::lifecycle::owned_guilds(&self.state, target.id).await?;
        if !owned.is_empty() {
            if !req.force_transfer {
                return Err(crate::lifecycle::owned_guilds_error(owned));
            }
            crate::lifecycle::hand_over_guilds(&self.state, actor, target.id).await?;
        }
        crate::lifecycle::delete_account(&self.state, &target, actor, req.reason.trim()).await?;
        Response::ok(AdminDeleteUserResponse::default())
    }

    async fn list_audit_log(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ListAuditLogRequest>,
    ) -> ServiceResult<ListAuditLogResponse> {
        require_admin(&self.state, &ctx).await?;
        let limit = page_limit(req.limit);
        let before = if req.page_token.is_empty() { None } else { Some(req.page_token.parse::<u64>().map_err(|_| bad_token())?) };
        let mut rows = admin::audit_page(&self.state, before, limit + 1).await?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let next_page_token = if more { rows.last().map(|r| r.id.to_string()).unwrap_or_default() } else { String::new() };
        let mut actors: HashMap<u64, Option<User>> = HashMap::new();
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            if let std::collections::hash_map::Entry::Vacant(slot) = actors.entry(row.actor_id) {
                slot.insert(self.user(row.actor_id).await?);
            }
            entries.push(AuditEntry {
                id: row.id,
                actor: actors[&row.actor_id].clone().into(),
                action: row.action,
                target: row.target,
                reason: row.reason,
                created_at: datetime_to_proto(crate::convert::snowflake_datetime(row.id)).into(),
                ..Default::default()
            });
        }
        Response::ok(ListAuditLogResponse { entries, next_page_token, ..Default::default() })
    }
}
