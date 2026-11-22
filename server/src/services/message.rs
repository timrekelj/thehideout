use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Utc};
use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use futures::future::try_join_all;
use scylla::value::{CqlValue, Row};

use crate::AppState;
use crate::access::{ChannelAccess, require_channel_access};
use crate::auth::{authenticate, internal};
use crate::embeds::{self, EMBED_COLUMNS, EmbedRow, MEDIA_COLUMNS, MediaRow, row_to_embed, row_to_media};
use crate::convert::{datetime_to_proto, id_from_db, id_to_db, snowflake_datetime};
use crate::events::EventPayload;
use crate::media::{self, Owner, pending};
use crate::moderation;
use crate::permissions::{MANAGE_MESSAGES, require_permission};
use crate::proto::thehideout::v1::*;
use crate::services::emoji::emoji_exists;
use crate::snowflake::{bucket_of, current_bucket};
use crate::validation::validate_unicode_emoji;

const MAX_CONTENT_CHARS: usize = 4000;
const MAX_NONCE_CHARS: usize = 64;
/// Attachments one message may carry.
pub const MAX_ATTACHMENTS: usize = 10;
const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 100;

/// Distinct emojis one message may carry.
pub const MAX_REACTION_EMOJIS: usize = 20;
/// Reaction adds/removes allowed per user within [`REACTION_WINDOW`].
pub const REACTION_MAX: usize = 120;
const REACTION_WINDOW: Duration = Duration::from_secs(60);

pub struct MessageSvc {
    state: AppState,
}

impl MessageSvc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

type MessageRow = (i64, i64, i64, String, Option<String>, Option<DateTime<Utc>>);

fn row_to_message(channel_id: u64, row: MessageRow) -> Message {
    let (id, guild_id, author_id, content, nonce, edited_at) = row;
    let id = id_from_db(id);
    Message {
        id,
        channel_id,
        guild_id: id_from_db(guild_id),
        author_id: id_from_db(author_id),
        content,
        created_at: datetime_to_proto(snowflake_datetime(id)).into(),
        edited_at: edited_at.map(datetime_to_proto).into(),
        nonce: nonce.unwrap_or_default(),
        ..Default::default()
    }
}

impl MessageService for MessageSvc {
    async fn send_message(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, SendMessageRequest>,
    ) -> ServiceResult<SendMessageResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let access = require_channel_access(&self.state, user_id, req.channel_id).await?;
        // DM messages carry guild_id = 0; guild channels carry their guild's ID.
        let guild_id = match &access {
            ChannelAccess::Guild(guild_id) => *guild_id,
            ChannelAccess::Dm => 0,
        };

        if let ChannelAccess::Guild(guild_id) = &access
            && crate::access::channel_kind(&self.state, *guild_id, req.channel_id).await?
                == ChannelKind::CHANNEL_KIND_VOICE
        {
            return Err(ConnectError::failed_precondition("voice channels have no messages"));
        }

        let attachment_ids: Vec<u64> = req.attachment_ids.iter().copied().collect();
        if attachment_ids.len() > MAX_ATTACHMENTS {
            return Err(ConnectError::invalid_argument("a message can have at most 10 attachments"));
        }
        if req.content.trim().is_empty() && attachment_ids.is_empty() {
            return Err(ConnectError::invalid_argument("message content must not be empty"));
        }
        if req.content.chars().count() > MAX_CONTENT_CHARS {
            return Err(ConnectError::invalid_argument(
                "message content must be at most 4000 characters",
            ));
        }
        let nonce = req.nonce.unwrap_or_default();
        if nonce.chars().count() > MAX_NONCE_CHARS {
            return Err(ConnectError::invalid_argument("nonce must be at most 64 characters"));
        }

        // A retried send (same user, channel and nonce within a minute) returns
        // the message that was stored the first time. Checked before the rate
        // limit so a retry costs nothing.
        if !nonce.is_empty()
            && let Some(message) = self.stored_by_nonce(user_id, req.channel_id, nonce).await?
        {
            return Response::ok(SendMessageResponse { message: message.into(), ..Default::default() });
        }
        self.state.limiter.check_message(user_id)?;

        let id = self.state.snowflake.next_id();
        if !nonce.is_empty() && !self.claim_nonce(user_id, req.channel_id, nonce, id).await? {
            // Lost a race with a retry of the same send.
            return match self.stored_by_nonce(user_id, req.channel_id, nonce).await? {
                Some(message) => Response::ok(SendMessageResponse { message: message.into(), ..Default::default() }),
                None => Err(ConnectError::aborted("a send with this nonce is in progress")),
            };
        }
        let claimed = if attachment_ids.is_empty() {
            None
        } else {
            match self.claim_attachments(user_id, &access, req.channel_id, id, &attachment_ids).await {
                Ok(claimed) => Some(claimed),
                Err(e) => {
                    self.release_nonce(user_id, req.channel_id, nonce).await;
                    return Err(e);
                }
            }
        };
        let inserted = self
            .state
            .session
            .execute_unpaged(
                "INSERT INTO messages (channel_id, bucket, id, guild_id, author_id, content, nonce) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                (
                    id_to_db(req.channel_id),
                    bucket_of(id),
                    id_to_db(id),
                    id_to_db(guild_id),
                    id_to_db(user_id),
                    req.content,
                    nonce,
                ),
            )
            .await;
        if let Err(e) = inserted {
            if let Some(claimed) = &claimed {
                self.release_attachments(req.channel_id, id, claimed).await;
            }
            self.release_nonce(user_id, req.channel_id, nonce).await;
            return Err(internal(e));
        }
        if let Some(claimed) = &claimed {
            for p in &claimed.pendings {
                if let Some(hour) = p.sweep_hour {
                    pending::forget_sweep(&self.state, hour, p.id).await;
                }
            }
        }

        crate::stats::message_sent(&self.state, guild_id, user_id).await;
        let mut message = row_to_message(
            req.channel_id,
            (
                id_to_db(id),
                id_to_db(guild_id),
                id_to_db(user_id),
                req.content.to_owned(),
                Some(nonce.to_owned()),
                None,
            ),
        );
        message.attachments = claimed.map(|c| c.attachments).unwrap_or_default();
        let event = EventPayload::MessageCreated(message.clone());
        match &access {
            ChannelAccess::Guild(guild_id) => self.state.events.publish_to_guild(*guild_id, event),
            ChannelAccess::Dm => self.state.events.publish_to_dm(req.channel_id, event),
        }
        embeds::spawn(
            &self.state,
            embeds::Job {
                channel_id: req.channel_id,
                guild_id,
                message_id: id,
                author_id: user_id,
                content: req.content.to_owned(),
            },
        );
        Response::ok(SendMessageResponse {
            message: message.into(),
            ..Default::default()
        })
    }

    async fn list_messages(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ListMessagesRequest>,
    ) -> ServiceResult<ListMessagesResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        require_channel_access(&self.state, user_id, req.channel_id).await?;

        let limit = match req.limit as usize {
            0 => DEFAULT_PAGE_SIZE,
            n => n.min(MAX_PAGE_SIZE),
        };
        if req.before == Some(0) {
            return Response::ok(ListMessagesResponse::default());
        }

        let mut rows = self.fetch_older(req.channel_id, req.before, limit + 1).await?;
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let mut messages: Vec<Message> = rows
            .into_iter()
            .map(|row| row_to_message(req.channel_id, row))
            .collect();
        let mut reactions = self.load_reactions(req.channel_id, user_id, &messages).await?;
        let mut attachments = self.load_attachments(req.channel_id, &messages).await?;
        let mut embeds = self.load_embeds(req.channel_id, &messages).await?;
        for message in &mut messages {
            message.reactions = reactions.remove(&message.id).unwrap_or_default();
            message.attachments = attachments.remove(&message.id).unwrap_or_default();
            message.embeds = embeds.remove(&message.id).unwrap_or_default();
        }
        let next_before = has_more.then(|| messages.last().map(|m| m.id)).flatten();
        Response::ok(ListMessagesResponse {
            messages,
            next_before,
            has_more,
            ..Default::default()
        })
    }

    async fn add_reaction(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, AddReactionRequest>,
    ) -> ServiceResult<AddReactionResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        self.throttle_reaction(user_id)?;
        let access = require_channel_access(&self.state, user_id, req.channel_id).await?;
        let kind = req.emoji.as_option().and_then(|e| e.kind.clone());
        let key = self.reaction_key(&access, kind, true).await?;
        self.require_message(req.channel_id, req.message_id).await?;
        let (channel, bucket, message) = (id_to_db(req.channel_id), bucket_of(req.message_id), id_to_db(req.message_id));

        // A new distinct emoji counts against the cap; adding to an emoji the
        // message already has never does. Racing adds of different new emojis
        // can overshoot the cap by a few (see migration 0006); the cap bounds
        // growth rather than being a hard invariant.
        // Every row of an emoji carries the emoji's first-use stamp (`at`), so
        // the order of emojis survives individual users removing theirs.
        let first_use = self
            .state
            .session
            .execute_unpaged(
                "SELECT at FROM reactions WHERE channel_id = ? AND bucket = ? AND message_id = ? \
                 AND emoji_key = ? LIMIT 1",
                (channel, bucket, message, &key),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .maybe_first_row::<(Option<i64>,)>()
            .map_err(internal)?;
        let present = first_use.is_some();
        let at = first_use.and_then(|(at,)| at).unwrap_or_else(|| id_to_db(self.state.snowflake.next_id()));
        if !present {
            let distinct: HashSet<String> = self
                .state
                .session
                .execute_unpaged(
                    "SELECT emoji_key FROM reactions WHERE channel_id = ? AND bucket = ? AND message_id = ?",
                    (channel, bucket, message),
                )
                .await
                .map_err(internal)?
                .into_rows_result()
                .map_err(internal)?
                .rows::<(String,)>()
                .map_err(internal)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(internal)?
                .into_iter()
                .map(|(k,)| k)
                .collect();
            if distinct.len() >= MAX_REACTION_EMOJIS {
                return Err(ConnectError::resource_exhausted("a message can have at most 20 distinct reactions"));
            }
        }

        let result = self
            .state
            .session
            .execute_unpaged(
                "INSERT INTO reactions (channel_id, bucket, message_id, emoji_key, user_id, at) \
                 VALUES (?, ?, ?, ?, ?, ?) IF NOT EXISTS",
                (channel, bucket, message, &key, id_to_db(user_id), at),
            )
            .await
            .map_err(internal)?;
        if applied(result)? {
            self.publish_reaction(&access, &req.emoji.as_option().and_then(|e| e.kind.clone()), req.channel_id, req.message_id, user_id, true);
        }
        Response::ok(AddReactionResponse::default())
    }

    async fn remove_reaction(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, RemoveReactionRequest>,
    ) -> ServiceResult<RemoveReactionResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        self.throttle_reaction(user_id)?;
        let access = require_channel_access(&self.state, user_id, req.channel_id).await?;
        let kind = req.emoji.as_option().and_then(|e| e.kind.clone());
        // The emoji may have been deleted since: only its shape is checked so
        // users can still remove their own reaction.
        let key = self.reaction_key(&access, kind.clone(), false).await?;
        self.require_message(req.channel_id, req.message_id).await?;
        let result = self
            .state
            .session
            .execute_unpaged(
                "DELETE FROM reactions WHERE channel_id = ? AND bucket = ? AND message_id = ? \
                 AND emoji_key = ? AND user_id = ? IF EXISTS",
                (id_to_db(req.channel_id), bucket_of(req.message_id), id_to_db(req.message_id), &key, id_to_db(user_id)),
            )
            .await
            .map_err(internal)?;
        if applied(result)? {
            self.publish_reaction(&access, &kind, req.channel_id, req.message_id, user_id, false);
        }
        Response::ok(RemoveReactionResponse::default())
    }
    async fn edit_message(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, EditMessageRequest>,
    ) -> ServiceResult<EditMessageResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let access = require_channel_access(&self.state, user_id, req.channel_id).await?;
        let guild_id = match &access {
            ChannelAccess::Guild(guild_id) => *guild_id,
            ChannelAccess::Dm => 0,
        };
        let Some(current) = self.load_message(req.channel_id, req.message_id, user_id).await? else {
            return Err(ConnectError::not_found("message not found"));
        };
        if current.author_id != user_id {
            return Err(ConnectError::permission_denied("only the author can edit a message"));
        }
        if req.content.trim().is_empty() && current.attachments.is_empty() {
            return Err(ConnectError::invalid_argument("message content must not be empty"));
        }
        if req.content.chars().count() > MAX_CONTENT_CHARS {
            return Err(ConnectError::invalid_argument("message content must be at most 4000 characters"));
        }
        self.state.limiter.check_message(user_id)?;
        if current.content == req.content {
            return Response::ok(EditMessageResponse { message: current.into(), ..Default::default() });
        }

        // IF EXISTS: an UPDATE would otherwise bring back a message deleted meanwhile.
        let result = self
            .state
            .session
            .execute_unpaged(
                "UPDATE messages SET content = ?, edited_at = ? WHERE channel_id = ? AND bucket = ? AND id = ? IF EXISTS",
                (
                    req.content,
                    Utc::now(),
                    id_to_db(req.channel_id),
                    bucket_of(req.message_id),
                    id_to_db(req.message_id),
                ),
            )
            .await
            .map_err(internal)?;
        if !applied(result)? {
            return Err(ConnectError::not_found("message not found"));
        }
        // The old embeds belong to the old text; the new ones are resolved in the background.
        moderation::purge_embeds(&self.state, req.channel_id, req.message_id).await?;
        publish_message_updated(&self.state, req.channel_id, req.message_id).await?;
        embeds::spawn(
            &self.state,
            embeds::Job {
                channel_id: req.channel_id,
                guild_id,
                message_id: req.message_id,
                author_id: user_id,
                content: req.content.to_owned(),
            },
        );
        let message = self
            .load_message(req.channel_id, req.message_id, user_id)
            .await?
            .ok_or_else(|| ConnectError::not_found("message not found"))?;
        Response::ok(EditMessageResponse { message: message.into(), ..Default::default() })
    }

    async fn delete_message(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, DeleteMessageRequest>,
    ) -> ServiceResult<DeleteMessageResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let access = require_channel_access(&self.state, user_id, req.channel_id).await?;
        let author = self
            .state
            .session
            .execute_unpaged(
                "SELECT author_id FROM messages WHERE channel_id = ? AND bucket = ? AND id = ?",
                (id_to_db(req.channel_id), bucket_of(req.message_id), id_to_db(req.message_id)),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .maybe_first_row::<(i64,)>()
            .map_err(internal)?
            .map(|(a,)| id_from_db(a))
            .ok_or_else(|| ConnectError::not_found("message not found"))?;
        let guild_id = match &access {
            ChannelAccess::Guild(guild_id) => *guild_id,
            ChannelAccess::Dm => 0,
        };
        let by_moderator = author != user_id;
        if by_moderator {
            match &access {
                ChannelAccess::Guild(guild_id) => {
                    require_permission(&self.state, user_id, *guild_id, MANAGE_MESSAGES).await?;
                }
                ChannelAccess::Dm => {
                    return Err(ConnectError::permission_denied("only the author can delete a direct message"));
                }
            }
        }
        moderation::purge_message(&self.state, req.channel_id, req.message_id).await?;
        moderation::publish_message_deleted(&self.state, guild_id, req.channel_id, req.message_id);
        if by_moderator {
            moderation::audit(
                &self.state,
                user_id,
                "message.delete",
                &format!("guild:{guild_id}/channel:{}/message:{}", req.channel_id, req.message_id),
                &format!("author:{author}"),
            )
            .await;
        }
        Response::ok(DeleteMessageResponse::default())
    }
}

/// Attachment row as read back by `load_attachments`.
type AttachmentRow = (
    i64,
    i32,
    i64,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<i32>,
    Option<i32>,
    Option<String>,
    Option<bool>,
);

/// Pending uploads turned into a message's attachments.
struct Claimed {
    pendings: Vec<pending::Pending>,
    attachments: Vec<Attachment>,
    /// Keys whose bytes were counted, to uncount on failure.
    accounted: Vec<String>,
}

fn pending_to_attachment(p: &pending::Pending) -> Attachment {
    Attachment {
        id: p.id,
        filename: p.filename.clone(),
        content_type: p.content_type.clone(),
        size: p.size,
        file: p.key.clone(),
        width: p.width,
        height: p.height,
        thumbnail: p.thumbnail.clone(),
        removed: false,
        ..Default::default()
    }
}

/// Reloads a message (reactions, attachments and embeds included) and publishes it as
/// `message_updated` to the audience of its channel. Used whenever a message
/// changes after creation (attachment removed, embeds resolved).
pub async fn publish_message_updated(state: &AppState, channel_id: u64, message_id: u64) -> Result<(), ConnectError> {
    let Some(message) = MessageSvc::new(state.clone()).load_message(channel_id, message_id, 0).await? else {
        return Err(ConnectError::not_found("message not found"));
    };
    let guild_id = message.guild_id;
    let event = EventPayload::MessageUpdated(message);
    if guild_id == 0 {
        state.events.publish_to_dm(channel_id, event);
    } else {
        state.events.publish_to_guild(guild_id, event);
    }
    Ok(())
}

/// Whether an LWT was applied (`[applied]` is its first column).
fn applied(result: scylla::response::query_result::QueryResult) -> Result<bool, ConnectError> {
    let row = result
        .into_rows_result()
        .map_err(internal)?
        .first_row::<Row>()
        .map_err(internal)?;
    Ok(matches!(row.columns.first(), Some(Some(CqlValue::Boolean(true)))))
}

/// `"u:<unicode>"` / `"c:<id>"` back to the proto emoji.
pub(crate) fn key_to_emoji(key: &str) -> Option<ReactionEmoji> {
    let kind = match key.split_at_checked(2)? {
        ("u:", unicode) => reaction_emoji::Kind::Unicode(unicode.to_owned()),
        ("c:", id) => reaction_emoji::Kind::CustomEmojiId(id.parse().ok()?),
        _ => return None,
    };
    Some(ReactionEmoji {
        kind: Some(kind),
        ..Default::default()
    })
}

impl MessageSvc {
    /// The message a send with this `nonce` already stored, if any.
    async fn stored_by_nonce(&self, user_id: u64, channel_id: u64, nonce: &str) -> Result<Option<Message>, ConnectError> {
        let Some((message_id,)) = self
            .state
            .session
            .execute_unpaged(
                "SELECT message_id FROM message_nonces WHERE user_id = ? AND channel_id = ? AND nonce = ?",
                (id_to_db(user_id), id_to_db(channel_id), nonce),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .maybe_first_row::<(i64,)>()
            .map_err(internal)?
        else {
            return Ok(None);
        };
        match self.load_message(channel_id, id_from_db(message_id), user_id).await? {
            Some(message) => Ok(Some(message)),
            // Claimed but not stored (yet, or the message was deleted since).
            None => Err(ConnectError::aborted("a send with this nonce is in progress or was deleted")),
        }
    }

    /// Remembers `nonce` for a minute; `false` if someone already holds it.
    async fn claim_nonce(&self, user_id: u64, channel_id: u64, nonce: &str, message_id: u64) -> Result<bool, ConnectError> {
        let result = self
            .state
            .session
            .execute_unpaged(
                "INSERT INTO message_nonces (user_id, channel_id, nonce, message_id) VALUES (?, ?, ?, ?) \
                 IF NOT EXISTS USING TTL 60",
                (id_to_db(user_id), id_to_db(channel_id), nonce, id_to_db(message_id)),
            )
            .await
            .map_err(internal)?;
        applied(result)
    }

    /// Forgets a nonce after a failed send, best effort, so the retry is not mistaken for a duplicate.
    async fn release_nonce(&self, user_id: u64, channel_id: u64, nonce: &str) {
        if nonce.is_empty() {
            return;
        }
        if let Err(e) = self
            .state
            .session
            .execute_unpaged(
                "DELETE FROM message_nonces WHERE user_id = ? AND channel_id = ? AND nonce = ?",
                (id_to_db(user_id), id_to_db(channel_id), nonce),
            )
            .await
        {
            tracing::warn!(error = %e, "releasing a message nonce failed");
        }
    }

    /// Validates the caller's pending uploads for a new message, consumes them,
    /// counts their bytes and writes the message's attachment rows (in the
    /// message's partition, before the message row itself exists, so nobody
    /// ever lists a message without its files).
    async fn claim_attachments(
        &self,
        user_id: u64,
        access: &ChannelAccess,
        channel_id: u64,
        message_id: u64,
        ids: &[u64],
    ) -> Result<Claimed, ConnectError> {
        let bad = |what: &str| ConnectError::invalid_argument(format!("attachment {what}"));
        let mut unique = HashSet::new();
        if ids.iter().any(|id| *id == 0 || !unique.insert(*id)) {
            return Err(ConnectError::invalid_argument("attachment_ids must be non-zero and distinct"));
        }
        let mut loaded: HashMap<u64, pending::Pending> =
            pending::load(&self.state, user_id, ids).await?.into_iter().map(|p| (p.id, p)).collect();
        let now = chrono::Utc::now();
        let mut pendings = Vec::with_capacity(ids.len());
        for id in ids {
            match loaded.remove(id) {
                Some(p) if p.channel_id == channel_id && p.expires_at > now => pendings.push(p),
                _ => return Err(bad(&format!("{id} is not a pending upload of yours for this channel (or it expired)"))),
            }
        }
        if !pending::consume(&self.state, user_id, ids).await? {
            return Err(bad("was already used or expired"));
        }

        // Storage accounting happens here, not at upload: pending uploads are
        // never counted, so expiry sweeps need no counter changes. Guild
        // channels count against the guild; DM files only against the global
        // totals, with the uploader as owner on the object row.
        let owner = match access {
            ChannelAccess::Guild(guild_id) => Owner { kind: "guild", id: *guild_id, guild_id: Some(*guild_id), label: "attachment" },
            ChannelAccess::Dm => Owner { kind: "user", id: user_id, guild_id: None, label: "attachment" },
        };
        let mut claimed = Claimed {
            pendings,
            attachments: Vec::new(),
            accounted: Vec::new(),
        };
        let result = async {
            for p in &claimed.pendings {
                let meta = media::Meta {
                    filename: Some(&p.filename),
                    content_type: Some(&p.content_type),
                    channel_id: Some(channel_id),
                    message_id: Some(message_id),
                    user_id: Some(user_id),
                };
                media::record_with(&self.state, &p.key, owner, p.size, meta).await?;
                claimed.accounted.push(p.key.clone());
                if let Some(thumbnail) = &p.thumbnail {
                    let meta = media::Meta { content_type: Some("image/webp"), ..meta };
                    media::record_with(&self.state, thumbnail, owner, p.thumbnail_bytes, meta).await?;
                    claimed.accounted.push(thumbnail.clone());
                }
            }
            let mut batch = scylla::statement::batch::Batch::new(scylla::statement::batch::BatchType::Logged);
            let mut values = Vec::with_capacity(claimed.pendings.len());
            for (position, p) in claimed.pendings.iter().enumerate() {
                batch.append_statement(
                    "INSERT INTO message_attachments (channel_id, bucket, message_id, position, id, filename, \
                     content_type, size, file, width, height, thumbnail, sha256, removed) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, false)",
                );
                values.push((
                    id_to_db(channel_id),
                    bucket_of(message_id),
                    id_to_db(message_id),
                    position as i32,
                    id_to_db(p.id),
                    p.filename.as_str(),
                    p.content_type.as_str(),
                    p.size as i64,
                    p.key.as_str(),
                    p.width.map(|v| v as i32),
                    p.height.map(|v| v as i32),
                    p.thumbnail.as_deref(),
                    p.sha256.as_str(),
                ));
            }
            self.state.session.batch(&batch, values).await.map_err(internal)?;
            Ok::<_, ConnectError>(())
        }
        .await;
        if let Err(e) = result {
            self.release_attachments(channel_id, message_id, &claimed).await;
            return Err(e);
        }
        claimed.attachments = claimed.pendings.iter().map(pending_to_attachment).collect();
        Ok(claimed)
    }

    /// Undoes [`Self::claim_attachments`] after a failed send, best effort: the
    /// attachment rows and byte counters go; the objects are left to the
    /// sweeper (their sweep rows are still registered and no longer accounted).
    async fn release_attachments(&self, channel_id: u64, message_id: u64, claimed: &Claimed) {
        for key in &claimed.accounted {
            media::delete_record(&self.state, key).await;
        }
        for position in 0..claimed.pendings.len() {
            if let Err(e) = self
                .state
                .session
                .execute_unpaged(
                    "DELETE FROM message_attachments WHERE channel_id = ? AND bucket = ? AND message_id = ? AND position = ?",
                    (id_to_db(channel_id), bucket_of(message_id), id_to_db(message_id), position as i32),
                )
                .await
            {
                tracing::warn!(error = %e, "releasing attachment rows failed");
            }
        }
    }

    /// The attachments of every message of a page: one query per bucket the
    /// page touches, like reactions.
    async fn load_attachments(&self, channel_id: u64, messages: &[Message]) -> Result<HashMap<u64, Vec<Attachment>>, ConnectError> {
        let mut per_bucket: BTreeMap<i32, Vec<i64>> = BTreeMap::new();
        for m in messages {
            per_bucket.entry(bucket_of(m.id)).or_default().push(id_to_db(m.id));
        }
        let fetched = try_join_all(per_bucket.into_iter().map(|(bucket, ids)| async move {
            self.state
                .session
                .execute_unpaged(
                    "SELECT message_id, position, id, filename, content_type, size, file, width, height, thumbnail, removed \
                     FROM message_attachments WHERE channel_id = ? AND bucket = ? AND message_id IN ?",
                    (id_to_db(channel_id), bucket, ids),
                )
                .await
                .map_err(internal)?
                .into_rows_result()
                .map_err(internal)?
                .rows::<AttachmentRow>()
                .map_err(internal)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(internal)
        }))
        .await?;
        let mut rows: Vec<AttachmentRow> = fetched.into_iter().flatten().collect();
        rows.sort_by_key(|r| (r.0, r.1));
        let mut out: HashMap<u64, Vec<Attachment>> = HashMap::new();
        for (message_id, _, id, filename, content_type, size, file, width, height, thumbnail, removed) in rows {
            out.entry(id_from_db(message_id)).or_default().push(Attachment {
                id: id_from_db(id),
                filename: filename.unwrap_or_default(),
                content_type: content_type.unwrap_or_default(),
                size: size.unwrap_or(0).max(0) as u64,
                file: file.unwrap_or_default(),
                width: width.map(|v| v as u32),
                height: height.map(|v| v as u32),
                thumbnail: thumbnail.filter(|t| !t.is_empty()),
                removed: removed.unwrap_or(false),
                ..Default::default()
            });
        }
        Ok(out)
    }

    /// The embeds of every message of a page: one query per bucket the page
    /// touches (for the embeds and for their media), like attachments.
    async fn load_embeds(&self, channel_id: u64, messages: &[Message]) -> Result<HashMap<u64, Vec<Embed>>, ConnectError> {
        let mut per_bucket: BTreeMap<i32, Vec<i64>> = BTreeMap::new();
        for m in messages {
            per_bucket.entry(bucket_of(m.id)).or_default().push(id_to_db(m.id));
        }
        let embed_query = format!("SELECT {EMBED_COLUMNS} FROM message_embeds WHERE channel_id = ? AND bucket = ? AND message_id IN ?");
        let media_query = format!("SELECT {MEDIA_COLUMNS} FROM message_embed_media WHERE channel_id = ? AND bucket = ? AND message_id IN ?");
        let (embed_query, media_query) = (embed_query.as_str(), media_query.as_str());
        let fetched = try_join_all(per_bucket.into_iter().map(|(bucket, ids)| async move {
            let embeds = self
                .state
                .session
                .execute_unpaged(embed_query, (id_to_db(channel_id), bucket, ids.clone()))
                .await
                .map_err(internal)?
                .into_rows_result()
                .map_err(internal)?
                .rows::<EmbedRow>()
                .map_err(internal)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(internal)?;
            // Only provider embeds (column 10, `provider`) have media rows; skip the query for plain links.
            let media = if embeds.iter().any(|r| r.10.is_some()) {
                self.state
                    .session
                    .execute_unpaged(media_query, (id_to_db(channel_id), bucket, ids))
                    .await
                    .map_err(internal)?
                    .into_rows_result()
                    .map_err(internal)?
                    .rows::<MediaRow>()
                    .map_err(internal)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(internal)?
            } else {
                Vec::new()
            };
            Ok::<_, ConnectError>((embeds, media))
        }))
        .await?;
        let mut rows: Vec<EmbedRow> = Vec::new();
        let mut media_rows: Vec<MediaRow> = Vec::new();
        for (embeds, media) in fetched {
            rows.extend(embeds);
            media_rows.extend(media);
        }
        rows.sort_by_key(|r| (r.0, r.1));
        let mut media: HashMap<(u64, i32), Vec<(i32, EmbedMedia)>> = HashMap::new();
        for (message_id, position, idx, m) in media_rows.into_iter().filter_map(row_to_media) {
            media.entry((message_id, position)).or_default().push((idx, m));
        }
        let mut out: HashMap<u64, Vec<Embed>> = HashMap::new();
        for row in rows {
            let (message_id, position, mut embed) = row_to_embed(row);
            if let Some(mut items) = media.remove(&(message_id, position)) {
                items.sort_by_key(|(idx, _)| *idx);
                embed.media = items.into_iter().map(|(_, m)| m).collect();
            }
            out.entry(message_id).or_default().push(embed);
        }
        Ok(out)
    }

    /// One message with its reactions, attachments and embeds, as seen by `viewer`
    /// (`0` = nobody: `me` is false everywhere).
    async fn load_message(&self, channel_id: u64, message_id: u64, viewer: u64) -> Result<Option<Message>, ConnectError> {
        let Some(row) = self
            .state
            .session
            .execute_unpaged(
                "SELECT id, guild_id, author_id, content, nonce, edited_at FROM messages \
                 WHERE channel_id = ? AND bucket = ? AND id = ?",
                (id_to_db(channel_id), bucket_of(message_id), id_to_db(message_id)),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .maybe_first_row::<MessageRow>()
            .map_err(internal)?
        else {
            return Ok(None);
        };
        let mut messages = vec![row_to_message(channel_id, row)];
        let mut reactions = self.load_reactions(channel_id, viewer, &messages).await?;
        let mut attachments = self.load_attachments(channel_id, &messages).await?;
        let mut embeds = self.load_embeds(channel_id, &messages).await?;
        let mut message = messages.remove(0);
        message.reactions = reactions.remove(&message.id).unwrap_or_default();
        message.attachments = attachments.remove(&message.id).unwrap_or_default();
        message.embeds = embeds.remove(&message.id).unwrap_or_default();
        Ok(Some(message))
    }

    fn throttle_reaction(&self, user_id: u64) -> Result<(), ConnectError> {
        self.state
            .limiter
            .check(&format!("reaction:{user_id}"), REACTION_MAX, REACTION_WINDOW)
    }

    /// Validates the emoji for the channel and returns its storage key.
    /// `must_exist` additionally requires a custom emoji to exist in the guild.
    async fn reaction_key(
        &self,
        access: &ChannelAccess,
        kind: Option<reaction_emoji::KindView<'_>>,
        must_exist: bool,
    ) -> Result<String, ConnectError> {
        match kind {
            None => Err(ConnectError::invalid_argument("emoji is required")),
            Some(reaction_emoji::KindView::Unicode(unicode)) => {
                validate_unicode_emoji(unicode)?;
                Ok(format!("u:{unicode}"))
            }
            Some(reaction_emoji::KindView::CustomEmojiId(id)) => match access {
                ChannelAccess::Dm => Err(ConnectError::invalid_argument("custom emojis cannot be used in direct messages")),
                ChannelAccess::Guild(guild_id) => {
                    if id == 0 || (must_exist && !emoji_exists(&self.state, *guild_id, id).await?) {
                        return Err(ConnectError::invalid_argument("custom emoji does not exist in this guild"));
                    }
                    Ok(format!("c:{id}"))
                }
            },
        }
    }

    async fn require_message(&self, channel_id: u64, message_id: u64) -> Result<(), ConnectError> {
        let found = self
            .state
            .session
            .execute_unpaged(
                "SELECT id FROM messages WHERE channel_id = ? AND bucket = ? AND id = ?",
                (id_to_db(channel_id), bucket_of(message_id), id_to_db(message_id)),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .rows_num()
            > 0;
        if found { Ok(()) } else { Err(ConnectError::not_found("message not found")) }
    }

    /// Publishes to the same audience as `message_created`.
    fn publish_reaction(
        &self,
        access: &ChannelAccess,
        kind: &Option<reaction_emoji::KindView<'_>>,
        channel_id: u64,
        message_id: u64,
        user_id: u64,
        added: bool,
    ) {
        let kind = kind.as_ref().map(|k| match k {
            reaction_emoji::KindView::Unicode(u) => reaction_emoji::Kind::Unicode((*u).to_owned()),
            reaction_emoji::KindView::CustomEmojiId(id) => reaction_emoji::Kind::CustomEmojiId(*id),
        });
        let change = ReactionChange {
            channel_id,
            message_id,
            emoji: ReactionEmoji { kind, ..Default::default() }.into(),
            user_id,
            ..Default::default()
        };
        let event = if added { EventPayload::ReactionAdded(change) } else { EventPayload::ReactionRemoved(change) };
        match access {
            ChannelAccess::Guild(guild_id) => self.state.events.publish_to_guild(*guild_id, event),
            ChannelAccess::Dm => self.state.events.publish_to_dm(channel_id, event),
        }
    }

    /// The reactions of every message of a page, with `me` for `user_id`:
    /// one query per bucket the page touches. Counts come from the rows
    /// themselves, so the cost is proportional to the reactions on the page;
    /// emojis are ordered by first use (`at`, see `add_reaction`).
    async fn load_reactions(
        &self,
        channel_id: u64,
        user_id: u64,
        messages: &[Message],
    ) -> Result<HashMap<u64, Vec<Reaction>>, ConnectError> {
        let mut per_bucket: BTreeMap<i32, Vec<i64>> = BTreeMap::new();
        for m in messages {
            per_bucket.entry(bucket_of(m.id)).or_default().push(id_to_db(m.id));
        }
        let me = id_to_db(user_id);
        let fetched = try_join_all(per_bucket.into_iter().map(|(bucket, ids)| async move {
            self.state
                .session
                .execute_unpaged(
                    "SELECT message_id, emoji_key, user_id, at FROM reactions \
                     WHERE channel_id = ? AND bucket = ? AND message_id IN ?",
                    (id_to_db(channel_id), bucket, ids),
                )
                .await
                .map_err(internal)?
                .into_rows_result()
                .map_err(internal)?
                .rows::<(i64, String, i64, Option<i64>)>()
                .map_err(internal)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(internal)
        }))
        .await?;

        // message -> emoji key -> (count, me, first use)
        let mut agg: HashMap<u64, HashMap<String, (u32, bool, i64)>> = HashMap::new();
        for (message_id, key, reactor, at) in fetched.into_iter().flatten() {
            let entry = agg
                .entry(id_from_db(message_id))
                .or_default()
                .entry(key)
                .or_insert((0, false, i64::MAX));
            entry.0 += 1;
            entry.1 |= reactor == me;
            entry.2 = entry.2.min(at.unwrap_or(0));
        }
        Ok(agg
            .into_iter()
            .map(|(message_id, emojis)| {
                let mut emojis: Vec<_> = emojis.into_iter().collect();
                emojis.sort_by(|a, b| a.1.2.cmp(&b.1.2).then_with(|| a.0.cmp(&b.0)));
                let reactions = emojis
                    .into_iter()
                    .filter_map(|(key, (count, me, _))| {
                        Some(Reaction {
                            emoji: key_to_emoji(&key)?.into(),
                            count,
                            me,
                            ..Default::default()
                        })
                    })
                    .collect();
                (message_id, reactions)
            })
            .collect())
    }

    /// Returns up to `want` messages of `channel_id` with an id below `before` (or the newest
    /// ones when `before` is `None`), newest first.
    ///
    /// Messages are partitioned by `(channel_id, bucket)`, so a page cannot be read with one
    /// query. Starting at the bucket of `before` (clamped to the current bucket), this walks
    /// buckets downward, asking each for the rows still needed, until `want` rows are collected
    /// or it has passed `bucket_of(channel_id)`: a message can't predate its channel, so no
    /// lower bucket can hold anything.
    ///
    /// Cost: one query per bucket visited, so a channel idle for many buckets scans that many
    /// empty partitions. Idea for later: remember each channel's newest non-empty bucket.
    async fn fetch_older(
        &self,
        channel_id: u64,
        before: Option<u64>,
        want: usize,
    ) -> Result<Vec<MessageRow>, ConnectError> {
        // IDs are stored as positive i64, so any larger bound is equivalent to i64::MAX.
        let bound = before.unwrap_or(i64::MAX as u64).min(i64::MAX as u64);
        let current_bucket = current_bucket();
        let lowest_bucket = bucket_of(channel_id);

        let mut rows = Vec::with_capacity(want);
        let mut bucket = bucket_of(bound).min(current_bucket);
        while bucket >= lowest_bucket && rows.len() < want {
            let remaining = (want - rows.len()) as i32;
            let found = self
                .state
                .session
                .execute_unpaged(
                    "SELECT id, guild_id, author_id, content, nonce, edited_at FROM messages \
                     WHERE channel_id = ? AND bucket = ? AND id < ? LIMIT ?",
                    (id_to_db(channel_id), bucket, id_to_db(bound), remaining),
                )
                .await
                .map_err(internal)?
                .into_rows_result()
                .map_err(internal)?
                .rows::<MessageRow>()
                .map_err(internal)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(internal)?;
            rows.extend(found);
            bucket -= 1;
        }
        Ok(rows)
    }
}
