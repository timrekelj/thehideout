use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};
use futures::future::try_join_all;
use rand::distr::{Alphanumeric, SampleString};
use scylla::statement::batch::{Batch, BatchType};

use crate::AppState;
use crate::access::{find_user, is_member, require_guild_member};
use crate::auth::{authenticate, internal, peer_key};
use crate::convert::{datetime_to_proto, id_from_db, id_to_db, snowflake_datetime};
use crate::events::EventPayload;
use crate::layout::{self, IconRef, MAX_SECTIONS_PER_GUILD};
use crate::moderation;
use crate::permissions::{BAN_MEMBERS, KICK_MEMBERS, MANAGE_CHANNELS, MANAGE_SERVER, require_permission, standing};
use crate::proto::thehideout::v1::*;
use crate::services::emoji::emoji_exists;
use crate::validation::{display_name, validate_color, validate_icon, validate_topic, validate_unicode_emoji};
use crate::versions;

const GUILD_NAME_LEN: std::ops::RangeInclusive<usize> = 1..=100;
const CHANNEL_NAME_LEN: std::ops::RangeInclusive<usize> = 1..=100;
const INVITE_CODE_LEN: usize = 8;
const INVITE_CODE_ATTEMPTS: usize = 5;
const DEFAULT_CHANNEL: &str = "general";

/// Guilds one user may be a member of via creation.
pub const MAX_GUILDS_PER_USER: i64 = 100;
/// Guild creations allowed per user within [`GUILD_CREATE_WINDOW`].
pub const GUILD_CREATE_MAX: usize = 10;
const GUILD_CREATE_WINDOW: Duration = Duration::from_secs(60 * 60);

/// Channels one guild may have.
pub const MAX_CHANNELS_PER_GUILD: i64 = 500;

/// Layout/settings edits (guild, channel, section, layout RPCs) allowed per user within [`GUILD_EDIT_WINDOW`].
pub const GUILD_EDIT_MAX: usize = 60;
const GUILD_EDIT_WINDOW: Duration = Duration::from_secs(60);

/// Guild joins (invite-code lookups) allowed per peer IP within [`JOIN_WINDOW`].
pub const JOIN_IP_MAX: usize = 30;
const JOIN_WINDOW: Duration = Duration::from_secs(15 * 60);

pub struct GuildSvc {
    state: AppState,
}

impl GuildSvc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

/// Trims, lowercases and joins whitespace runs with '-'; `None` unless the result matches `^[a-z0-9_-]{1,100}$`.
fn normalize_channel_name(raw: &str) -> Option<String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join("-").to_lowercase();
    let valid = CHANNEL_NAME_LEN.contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    valid.then_some(name)
}

pub(crate) fn guild_to_proto(
    id: i64,
    name: String,
    owner_id: i64,
    invite_code: String,
    created_at: DateTime<Utc>,
    icon: Option<String>,
) -> Guild {
    Guild {
        id: id_from_db(id),
        name,
        owner_id: id_from_db(owner_id),
        invite_code,
        created_at: datetime_to_proto(created_at).into(),
        icon: icon.unwrap_or_default(),
        ..Default::default()
    }
}

/// Loads a guild row by database ID for proto conversion.
pub async fn load_guild(state: &AppState, db_id: i64) -> Result<Option<Guild>, ConnectError> {
    Ok(state
    .session
        .execute_unpaged(
            "SELECT name, owner_id, invite_code, created_at, icon FROM guilds WHERE id = ?",
            (db_id,),
        )
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(String, i64, String, DateTime<Utc>, Option<String>)>()
        .map_err(internal)?
        .map(|(name, owner_id, invite_code, created_at, icon)| {
            guild_to_proto(db_id, name, owner_id, invite_code, created_at, icon)
        }))
}

impl GuildService for GuildSvc {
    async fn create_guild(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, CreateGuildRequest>,
    ) -> ServiceResult<CreateGuildResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let name = req.name.trim();
        if !GUILD_NAME_LEN.contains(&name.chars().count()) {
            return Err(ConnectError::invalid_argument("guild name must be 1-100 characters"));
        }
        self.state
            .limiter
            .check(&format!("create_guild:{user_id}"), GUILD_CREATE_MAX, GUILD_CREATE_WINDOW)?;
        self.reserve_guild_slot(user_id).await?;

        let id = self.state.snowflake.next_id();
        let db_id = id_to_db(id);
        let created_at = snowflake_datetime(id);
        let invite_code = self.claim_invite_code(db_id).await?;

        self.state
            .session
            .execute_unpaged(
                "INSERT INTO guilds (id, name, owner_id, invite_code, created_at) VALUES (?, ?, ?, ?, ?)",
                (db_id, name, id_to_db(user_id), &invite_code, created_at),
            )
            .await
            .map_err(internal)?;
        crate::stats::guild_created(&self.state).await;
        self.add_member(id, user_id).await?;
        let channel = self.insert_channel(id, DEFAULT_CHANNEL, None, 0, None, "", ChannelKind::CHANNEL_KIND_TEXT).await?;
        self.state.events.publish_to_guild(id, EventPayload::ChannelCreated(channel));
        let guild = guild_to_proto(db_id, name.to_owned(), id_to_db(user_id), invite_code, created_at, None);
        self.state.events.notify_user_joined_guild(user_id, guild.clone());

        tracing::info!(guild_id = id, user_id, "guild created");
        Response::ok(CreateGuildResponse {
            guild: guild.into(),
            ..Default::default()
        })
    }

    async fn list_my_guilds(
        &self,
        ctx: RequestContext,
        _req: ServiceRequest<'_, ListMyGuildsRequest>,
    ) -> ServiceResult<ListMyGuildsResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_ids = self
            .state
            .session
            .execute_unpaged("SELECT guild_id FROM guilds_by_user WHERE user_id = ?", (id_to_db(user_id),))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .rows::<(i64,)>()
            .map_err(internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(internal)?;

        let mut guilds = try_join_all(guild_ids.into_iter().map(|(id,)| self.find_guild(id)))
            .await?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        guilds.sort_by_key(|g| g.id);
        Response::ok(ListMyGuildsResponse {
            guilds,
            ..Default::default()
        })
    }

    async fn join_guild(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, JoinGuildRequest>,
    ) -> ServiceResult<JoinGuildResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        self.state
            .limiter
            .check(&format!("join:ip:{}", peer_key(&ctx)), JOIN_IP_MAX, JOIN_WINDOW)?;
        let invalid = || ConnectError::not_found("invalid invite");
        let (guild_id,) = self
            .state
            .session
            .execute_unpaged(
                "SELECT guild_id FROM guilds_by_invite WHERE invite_code = ?",
                (req.invite_code.trim(),),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .maybe_first_row::<(i64,)>()
            .map_err(internal)?
            .ok_or_else(invalid)?;
        let guild = self.find_guild(guild_id).await?.ok_or_else(invalid)?;
        self.admit(&guild, user_id).await?;
        Response::ok(JoinGuildResponse {
            guild: guild.into(),
            ..Default::default()
        })
    }

    async fn list_guild_members(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ListGuildMembersRequest>,
    ) -> ServiceResult<ListGuildMembersResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        require_guild_member(&self.state, user_id, req.guild_id).await?;

        let joined = self
            .state
            .session
            .execute_unpaged(
                "SELECT user_id, joined_at FROM guild_members WHERE guild_id = ?",
                (id_to_db(req.guild_id),),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .rows::<(i64, Option<DateTime<Utc>>)>()
            .map_err(internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(internal)?;
        let mut roles: std::collections::HashMap<i64, Vec<u64>> = self
            .state
            .session
            .execute_unpaged(
                "SELECT user_id, role_ids FROM member_roles WHERE guild_id = ?",
                (id_to_db(req.guild_id),),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .rows::<(i64, Option<Vec<i64>>)>()
            .map_err(internal)?
            .map(|r| r.map(|(uid, ids)| (uid, ids.unwrap_or_default().into_iter().map(id_from_db).collect())))
            .collect::<Result<_, _>>()
            .map_err(internal)?;

        let mut members = try_join_all(joined.into_iter().map(|(uid, joined_at)| async move {
            Ok::<_, ConnectError>(find_user(&self.state, uid).await?.map(|user| (uid, user, joined_at)))
        }))
        .await?
        .into_iter()
        .flatten()
        .map(|(uid, user, joined_at)| {
            let mut role_ids = roles.remove(&uid).unwrap_or_default();
            role_ids.sort_unstable();
            Member {
                user: user.into(),
                role_ids,
                joined_at: datetime_to_proto(joined_at.unwrap_or_default()).into(),
                ..Default::default()
            }
        })
        .collect::<Vec<_>>();
        members.sort_by(|a, b| a.user.username.cmp(&b.user.username));
        let users = members.iter().filter_map(|m| m.user.as_option().cloned()).collect();
        Response::ok(ListGuildMembersResponse {
            users,
            members,
            ..Default::default()
        })
    }

    async fn create_channel(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, CreateChannelRequest>,
    ) -> ServiceResult<CreateChannelResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        require_permission(&self.state, user_id, guild_id, MANAGE_CHANNELS).await?;
        self.throttle_layout(user_id)?;
        let name = normalize_channel_name(req.name).ok_or_else(|| {
            ConnectError::invalid_argument(
                "channel name must be 1-100 characters of a-z, 0-9, '_' or '-'",
            )
        })?;
        // `icon_ref` wins over the legacy `icon`.
        let icon = if req.icon_ref.is_set() {
            let kind = req.icon_ref.as_option().and_then(|i| i.kind.clone());
            self.parse_icon(guild_id, kind).await?
        } else {
            self.parse_legacy_icon(req.icon)?
        };
        validate_color(req.color)?;
        let kind = match req.kind.as_known() {
            Some(ChannelKind::CHANNEL_KIND_VOICE) => ChannelKind::CHANNEL_KIND_VOICE,
            Some(ChannelKind::CHANNEL_KIND_UNSPECIFIED | ChannelKind::CHANNEL_KIND_TEXT) => ChannelKind::CHANNEL_KIND_TEXT,
            None => return Err(ConnectError::invalid_argument("unknown channel kind")),
        };

        let data = layout::load(&self.state, guild_id).await?;
        let built = layout::build(guild_id, &data, 0);
        let section = req.section_id;
        if let Some(section) = section
            && !data.sections.iter().any(|s| s.id == section)
        {
            return Err(ConnectError::not_found("section not found"));
        }
        let position = built
            .layout
            .entries
            .iter()
            .find(|e| e.section_id == section.unwrap_or(0))
            .map_or(0, |e| e.channel_ids.len() as i32);

        self.reserve_channel_slot(guild_id).await?;
        let channel = self
            .insert_channel(guild_id, &name, section, position, icon.as_ref(), req.color, kind)
            .await?;
        versions::bump(&self.state, guild_id, versions::LAYOUT, None).await?;
        self.state
            .events
            .publish_to_guild(guild_id, EventPayload::ChannelCreated(channel.clone()));
        self.publish_layout(guild_id).await?;
        Response::ok(CreateChannelResponse {
            channel: channel.into(),
            ..Default::default()
        })
    }

    async fn list_channels(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ListChannelsRequest>,
    ) -> ServiceResult<ListChannelsResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        require_guild_member(&self.state, user_id, req.guild_id).await?;
        let built = layout::load_built(&self.state, req.guild_id).await?;
        Response::ok(ListChannelsResponse {
            channels: built.channels,
            layout: built.layout.into(),
            ..Default::default()
        })
    }

    async fn update_guild(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, UpdateGuildRequest>,
    ) -> ServiceResult<UpdateGuildResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        require_permission(&self.state, user_id, guild_id, MANAGE_SERVER).await?;
        self.throttle_layout(user_id)?;
        let name = req.name.map(|n| display_name(n, "guild")).transpose()?;
        if let Some(name) = &name {
            self.state
                .session
                .execute_unpaged("UPDATE guilds SET name = ? WHERE id = ?", (name, id_to_db(guild_id)))
                .await
                .map_err(internal)?;
        }
        let guild = self
            .find_guild(id_to_db(guild_id))
            .await?
            .ok_or_else(|| ConnectError::not_found("guild not found"))?;
        if name.is_some() {
            self.state
                .events
                .publish_to_guild(guild_id, EventPayload::GuildUpdated(guild.clone()));
        }
        Response::ok(UpdateGuildResponse {
            guild: guild.into(),
            ..Default::default()
        })
    }

    async fn update_channel(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, UpdateChannelRequest>,
    ) -> ServiceResult<UpdateChannelResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        require_permission(&self.state, user_id, guild_id, MANAGE_CHANNELS).await?;
        self.throttle_layout(user_id)?;
        let name = req
            .name
            .map(|n| {
                normalize_channel_name(n).ok_or_else(|| {
                    ConnectError::invalid_argument(
                        "channel name must be 1-100 characters of a-z, 0-9, '_' or '-'",
                    )
                })
            })
            .transpose()?;
        // `Some(None)` resets to the default icon, `None` keeps the current one.
        let new_icon: Option<Option<IconRef>> = if req.icon_ref.is_set() {
            let kind = req.icon_ref.as_option().and_then(|i| i.kind.clone());
            Some(self.parse_icon(guild_id, kind).await?)
        } else if let Some(icon) = req.icon {
            Some(self.parse_legacy_icon(icon)?)
        } else {
            None
        };
        if let Some(color) = req.color {
            validate_color(color)?;
        }
        if let Some(topic) = req.topic {
            validate_topic(topic)?;
        }

        let data = layout::load(&self.state, guild_id).await?;
        let current = data
            .channels
            .iter()
            .find(|c| c.id == req.channel_id)
            .ok_or_else(|| ConnectError::not_found("channel not found"))?;
        let name = name.unwrap_or_else(|| current.name.clone());
        let icon = new_icon.unwrap_or_else(|| current.icon.clone());
        let color = req.color.map_or_else(|| current.color.clone(), str::to_owned);
        let topic = req.topic.map_or_else(|| current.topic.clone(), str::to_owned);
        let (icon_kind, icon_value) = IconRef::columns_of(icon.as_ref());
        self.state
            .session
            .execute_unpaged(
                "UPDATE channels SET name = ?, icon = ?, icon_kind = ?, icon_value = ?, color = ?, topic = ? \
                 WHERE guild_id = ? AND id = ?",
                (
                    &name,
                    IconRef::legacy_mirror(icon.as_ref()),
                    icon_kind,
                    &icon_value,
                    &color,
                    &topic,
                    id_to_db(guild_id),
                    id_to_db(req.channel_id),
                ),
            )
            .await
            .map_err(internal)?;

        let data = layout::load(&self.state, guild_id).await?;
        let channel = layout::build(guild_id, &data, 0)
            .channels
            .into_iter()
            .find(|c| c.id == req.channel_id)
            .ok_or_else(|| ConnectError::not_found("channel not found"))?;
        self.state
            .events
            .publish_to_guild(guild_id, EventPayload::ChannelUpdated(channel.clone()));
        Response::ok(UpdateChannelResponse {
            channel: channel.into(),
            ..Default::default()
        })
    }

    async fn create_section(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, CreateSectionRequest>,
    ) -> ServiceResult<CreateSectionResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        require_permission(&self.state, user_id, guild_id, MANAGE_CHANNELS).await?;
        self.throttle_layout(user_id)?;
        let name = display_name(req.name, "section")?;
        let data = layout::load(&self.state, guild_id).await?;
        if data.sections.len() >= MAX_SECTIONS_PER_GUILD {
            return Err(ConnectError::resource_exhausted("section limit reached"));
        }
        let id = self.state.snowflake.next_id();
        self.state
            .session
            .execute_unpaged(
                "INSERT INTO sections (guild_id, id, name, position) VALUES (?, ?, ?, ?)",
                (id_to_db(guild_id), id_to_db(id), &name, data.sections.len() as i32),
            )
            .await
            .map_err(internal)?;
        versions::bump(&self.state, guild_id, versions::LAYOUT, None).await?;
        let layout = self.publish_layout(guild_id).await?;
        let section = layout.sections.iter().find(|s| s.id == id).cloned();
        Response::ok(CreateSectionResponse {
            section: section.into(),
            layout: layout.into(),
            ..Default::default()
        })
    }

    async fn update_section(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, UpdateSectionRequest>,
    ) -> ServiceResult<UpdateSectionResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        require_permission(&self.state, user_id, guild_id, MANAGE_CHANNELS).await?;
        self.throttle_layout(user_id)?;
        let name = display_name(req.name, "section")?;
        let data = layout::load(&self.state, guild_id).await?;
        if !data.sections.iter().any(|s| s.id == req.section_id) {
            return Err(ConnectError::not_found("section not found"));
        }
        self.state
            .session
            .execute_unpaged(
                "UPDATE sections SET name = ? WHERE guild_id = ? AND id = ?",
                (&name, id_to_db(guild_id), id_to_db(req.section_id)),
            )
            .await
            .map_err(internal)?;
        versions::bump(&self.state, guild_id, versions::LAYOUT, None).await?;
        let layout = self.publish_layout(guild_id).await?;
        let section = layout.sections.iter().find(|s| s.id == req.section_id).cloned();
        Response::ok(UpdateSectionResponse {
            section: section.into(),
            layout: layout.into(),
            ..Default::default()
        })
    }

    async fn delete_section(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, DeleteSectionRequest>,
    ) -> ServiceResult<DeleteSectionResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        require_permission(&self.state, user_id, guild_id, MANAGE_CHANNELS).await?;
        self.throttle_layout(user_id)?;
        let data = layout::load(&self.state, guild_id).await?;
        if !data.sections.iter().any(|s| s.id == req.section_id) {
            return Err(ConnectError::not_found("section not found"));
        }
        let built = layout::build(guild_id, &data, 0);
        let entry = |section_id: u64| {
            built
                .layout
                .entries
                .iter()
                .find(|e| e.section_id == section_id)
                .map(|e| e.channel_ids.clone())
                .unwrap_or_default()
        };
        // Channels of the deleted section are appended after the top-level ones.
        let start = entry(0).len() as i32;
        let moved: Vec<(u64, Option<u64>, i32)> = entry(req.section_id)
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, None, start + i as i32))
            .collect();
        layout::write_channel_positions(&self.state, guild_id, &moved).await?;
        self.state
            .session
            .execute_unpaged(
                "DELETE FROM sections WHERE guild_id = ? AND id = ?",
                (id_to_db(guild_id), id_to_db(req.section_id)),
            )
            .await
            .map_err(internal)?;
        let remaining: Vec<(u64, i32)> = data
            .sections
            .iter()
            .filter(|s| s.id != req.section_id)
            .enumerate()
            .map(|(i, s)| (s.id, i as i32))
            .collect();
        layout::write_section_positions(&self.state, guild_id, &remaining).await?;
        versions::bump(&self.state, guild_id, versions::LAYOUT, None).await?;
        let layout = self.publish_layout(guild_id).await?;
        Response::ok(DeleteSectionResponse {
            layout: layout.into(),
            ..Default::default()
        })
    }

    async fn update_layout(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, UpdateLayoutRequest>,
    ) -> ServiceResult<UpdateLayoutResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        require_permission(&self.state, user_id, guild_id, MANAGE_CHANNELS).await?;
        self.throttle_layout(user_id)?;
        if versions::get(&self.state, guild_id, versions::LAYOUT).await? != req.expected_version {
            return Err(ConnectError::aborted("layout changed: refetch and retry"));
        }
        let data = layout::load(&self.state, guild_id).await?;
        let invalid = |m: &str| ConnectError::invalid_argument(format!("layout is not a permutation of the current one: {m}"));

        let section_ids: Vec<u64> = req.section_ids.iter().copied().collect();
        let known_sections: HashSet<u64> = data.sections.iter().map(|s| s.id).collect();
        let distinct_sections: HashSet<u64> = section_ids.iter().copied().collect();
        if section_ids.len() != data.sections.len()
            || distinct_sections.len() != section_ids.len()
            || distinct_sections != known_sections
        {
            return Err(invalid("section_ids"));
        }

        let known_channels: HashSet<u64> = data.channels.iter().map(|c| c.id).collect();
        let mut seen_channels = HashSet::new();
        let mut seen_entries = HashSet::new();
        let mut channel_assignments = Vec::with_capacity(data.channels.len());
        for entry in req.entries.iter() {
            let container = entry.section_id;
            if (container != 0 && !known_sections.contains(&container)) || !seen_entries.insert(container) {
                return Err(invalid("entries"));
            }
            for (i, channel_id) in entry.channel_ids.iter().enumerate() {
                if !known_channels.contains(channel_id) || !seen_channels.insert(*channel_id) {
                    return Err(invalid("channel_ids"));
                }
                channel_assignments.push((*channel_id, (container != 0).then_some(container), i as i32));
            }
        }
        if seen_channels.len() != known_channels.len() {
            return Err(invalid("channel_ids"));
        }

        versions::bump(&self.state, guild_id, versions::LAYOUT, Some(req.expected_version)).await?;
        let section_positions: Vec<(u64, i32)> =
            section_ids.iter().enumerate().map(|(i, id)| (*id, i as i32)).collect();
        layout::write_section_positions(&self.state, guild_id, &section_positions).await?;
        layout::write_channel_positions(&self.state, guild_id, &channel_assignments).await?;
        let layout = self.publish_layout(guild_id).await?;
        Response::ok(UpdateLayoutResponse {
            layout: layout.into(),
            ..Default::default()
        })
    }
    async fn kick_member(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, KickMemberRequest>,
    ) -> ServiceResult<KickMemberResponse> {
        let actor = authenticate(&self.state, &ctx).await?;
        self.authorize_moderation(actor, req.guild_id, req.user_id, KICK_MEMBERS, true).await?;
        moderation::remove_member(&self.state, req.guild_id, req.user_id, MemberRemovalReason::MEMBER_REMOVAL_REASON_KICKED).await?;
        moderation::audit(&self.state, actor, "guild.kick", &member_target(req.guild_id, req.user_id), "").await;
        Response::ok(KickMemberResponse::default())
    }

    async fn ban_member(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, BanMemberRequest>,
    ) -> ServiceResult<BanMemberResponse> {
        let actor = authenticate(&self.state, &ctx).await?;
        let (guild_id, target) = (req.guild_id, req.user_id);
        let reason = req.reason.trim();
        if reason.chars().count() > moderation::MAX_REASON_CHARS {
            return Err(ConnectError::invalid_argument("reason must be at most 512 characters"));
        }
        let purge_seconds = req.delete_message_seconds.unwrap_or(0);
        if purge_seconds > moderation::MAX_DELETE_MESSAGE_SECONDS {
            return Err(ConnectError::invalid_argument("delete_message_seconds must be at most 604800 (7 days)"));
        }
        self.authorize_moderation(actor, guild_id, target, BAN_MEMBERS, false).await?;
        if find_user(&self.state, id_to_db(target)).await?.is_none() {
            return Err(ConnectError::not_found("user not found"));
        }
        // The ban first, so a rejoin racing the removal is refused.
        self.state
            .session
            .execute_unpaged(
                "INSERT INTO guild_bans (guild_id, user_id, banned_by, reason, created_at) VALUES (?, ?, ?, ?, ?)",
                (id_to_db(guild_id), id_to_db(target), id_to_db(actor), reason, Utc::now()),
            )
            .await
            .map_err(internal)?;
        moderation::remove_member(&self.state, guild_id, target, MemberRemovalReason::MEMBER_REMOVAL_REASON_BANNED).await?;
        moderation::audit(&self.state, actor, "guild.ban", &member_target(guild_id, target), reason).await;
        if purge_seconds > 0 {
            let state = self.state.clone();
            let channel_ids: Vec<u64> = layout::load(&state, guild_id).await?.channels.iter().map(|c| c.id).collect();
            tokio::spawn(async move {
                moderation::purge_author_messages(&state, guild_id, &channel_ids, target, purge_seconds).await;
            });
        }
        Response::ok(BanMemberResponse::default())
    }

    async fn unban_member(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, UnbanMemberRequest>,
    ) -> ServiceResult<UnbanMemberResponse> {
        let actor = authenticate(&self.state, &ctx).await?;
        require_permission(&self.state, actor, req.guild_id, BAN_MEMBERS).await?;
        if !moderation::is_banned(&self.state, req.guild_id, req.user_id).await? {
            return Err(ConnectError::not_found("user is not banned"));
        }
        self.state
            .session
            .execute_unpaged(
                "DELETE FROM guild_bans WHERE guild_id = ? AND user_id = ?",
                (id_to_db(req.guild_id), id_to_db(req.user_id)),
            )
            .await
            .map_err(internal)?;
        moderation::audit(&self.state, actor, "guild.unban", &member_target(req.guild_id, req.user_id), "").await;
        Response::ok(UnbanMemberResponse::default())
    }

    async fn list_bans(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, ListBansRequest>,
    ) -> ServiceResult<ListBansResponse> {
        let actor = authenticate(&self.state, &ctx).await?;
        require_permission(&self.state, actor, req.guild_id, BAN_MEMBERS).await?;
        let rows = self
            .state
            .session
            .execute_unpaged(
                "SELECT user_id, banned_by, reason, created_at FROM guild_bans WHERE guild_id = ?",
                (id_to_db(req.guild_id),),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .rows::<(i64, Option<i64>, Option<String>, Option<DateTime<Utc>>)>()
            .map_err(internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(internal)?;
        let mut bans = Vec::with_capacity(rows.len());
        for (user_id, banned_by, reason, created_at) in rows {
            let created_at = created_at.unwrap_or_default();
            let user = match find_user(&self.state, user_id).await? {
                Some(user) => user,
                // The account is gone; keep the ban visible by id.
                None => User { id: id_from_db(user_id), ..Default::default() },
            };
            bans.push((created_at, Ban {
                user: user.into(),
                banned_by: banned_by.map_or(0, id_from_db),
                reason: reason.unwrap_or_default(),
                created_at: datetime_to_proto(created_at).into(),
                ..Default::default()
            }));
        }
        bans.sort_by_key(|b| std::cmp::Reverse(b.0));
        Response::ok(ListBansResponse { bans: bans.into_iter().map(|(_, b)| b).collect(), ..Default::default() })
    }

    async fn leave_guild(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, LeaveGuildRequest>,
    ) -> ServiceResult<LeaveGuildResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        if standing(&self.state, user_id, req.guild_id).await?.is_owner {
            return Err(ConnectError::failed_precondition(
                "the owner cannot leave: transfer ownership or delete the guild first",
            ));
        }
        moderation::remove_member(&self.state, req.guild_id, user_id, MemberRemovalReason::MEMBER_REMOVAL_REASON_LEFT).await?;
        Response::ok(LeaveGuildResponse::default())
    }

    async fn delete_channel(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, DeleteChannelRequest>,
    ) -> ServiceResult<DeleteChannelResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let (guild_id, channel_id) = (req.guild_id, req.channel_id);
        require_permission(&self.state, user_id, guild_id, MANAGE_CHANNELS).await?;
        self.throttle_layout(user_id)?;
        let data = layout::load(&self.state, guild_id).await?;
        let channel = data
            .channels
            .iter()
            .find(|c| c.id == channel_id)
            .ok_or_else(|| ConnectError::not_found("channel not found"))?;
        let name = channel.name.clone();
        // Unreachable from here on: reads and sends resolve channels through these rows.
        let mut batch = Batch::new(BatchType::Logged);
        batch.append_statement("DELETE FROM channels_by_id WHERE id = ?");
        batch.append_statement("DELETE FROM channels WHERE guild_id = ? AND id = ?");
        self.state
            .session
            .batch(&batch, ((id_to_db(channel_id),), (id_to_db(guild_id), id_to_db(channel_id))))
            .await
            .map_err(internal)?;
        let _ = self
            .state
            .session
            .execute_unpaged(
                "UPDATE guild_channel_quota SET created = created - 1 WHERE guild_id = ?",
                (id_to_db(guild_id),),
            )
            .await;
        crate::stats::channel_deleted(&self.state, guild_id).await;
        versions::bump(&self.state, guild_id, versions::LAYOUT, None).await?;
        crate::voice::on_channel_deleted(&self.state, channel_id).await;
        moderation::publish_channel_deleted(&self.state, guild_id, channel_id);
        let layout = self.publish_layout(guild_id).await?;
        moderation::audit(&self.state, user_id, "channel.delete", &format!("guild:{guild_id}/channel:{channel_id}"), &name).await;
        let state = self.state.clone();
        tokio::spawn(async move { moderation::purge_channel(&state, channel_id).await });
        Response::ok(DeleteChannelResponse { layout: layout.into(), ..Default::default() })
    }

    async fn delete_guild(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, DeleteGuildRequest>,
    ) -> ServiceResult<DeleteGuildResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        let guild = self.owned_guild(user_id, guild_id, req.confirm_name).await?;
        let channel_ids: Vec<u64> = layout::load(&self.state, guild_id).await?.channels.iter().map(|c| c.id).collect();
        let members: Vec<i64> = self
            .state
            .session
            .execute_unpaged("SELECT user_id FROM guild_members WHERE guild_id = ?", (id_to_db(guild_id),))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .rows::<(i64,)>()
            .map_err(internal)?
            .map(|r| r.map(|(u,)| u))
            .collect::<Result<_, _>>()
            .map_err(internal)?;

        // Unreachable first: the guild, its invite code, memberships and channel lookups.
        let db_guild = id_to_db(guild_id);
        self.state.session.execute_unpaged("DELETE FROM guilds WHERE id = ?", (db_guild,)).await.map_err(internal)?;
        self.state
            .session
            .execute_unpaged("DELETE FROM guilds_by_invite WHERE invite_code = ?", (&guild.invite_code,))
            .await
            .map_err(internal)?;
        for chunk in members.chunks(50) {
            let mut batch = Batch::new(BatchType::Unlogged);
            let mut values = Vec::with_capacity(chunk.len() * 2);
            for user in chunk {
                batch.append_statement("DELETE FROM guild_members WHERE guild_id = ? AND user_id = ?");
                values.push((db_guild, *user));
                batch.append_statement("DELETE FROM guilds_by_user WHERE user_id = ? AND guild_id = ?");
                values.push((*user, db_guild));
            }
            self.state.session.batch(&batch, values).await.map_err(internal)?;
        }
        for chunk in channel_ids.chunks(50) {
            let mut batch = Batch::new(BatchType::Unlogged);
            let mut values = Vec::with_capacity(chunk.len());
            for id in chunk {
                batch.append_statement("DELETE FROM channels_by_id WHERE id = ?");
                values.push((id_to_db(*id),));
            }
            self.state.session.batch(&batch, values).await.map_err(internal)?;
        }
        for channel_id in &channel_ids {
            crate::voice::on_channel_deleted(&self.state, *channel_id).await;
        }
        for user in &members {
            self.state.events.notify_user_removed_from_guild(id_from_db(*user), guild_id);
        }
        crate::stats::guild_deleted(&self.state, channel_ids.len()).await;
        moderation::audit(&self.state, user_id, "guild.delete", &format!("guild:{guild_id}"), &guild.name).await;
        let state = self.state.clone();
        tokio::spawn(async move { moderation::purge_deleted_guild(&state, guild_id, channel_ids).await });
        tracing::info!(guild_id, user_id, "guild deleted");
        Response::ok(DeleteGuildResponse::default())
    }

    async fn rotate_invite_code(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, RotateInviteCodeRequest>,
    ) -> ServiceResult<RotateInviteCodeResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        require_permission(&self.state, user_id, guild_id, MANAGE_SERVER).await?;
        self.throttle_layout(user_id)?;
        let old = self
            .find_guild(id_to_db(guild_id))
            .await?
            .ok_or_else(|| ConnectError::not_found("guild not found"))?;
        let code = self.claim_invite_code(id_to_db(guild_id)).await?;
        self.state
            .session
            .execute_unpaged("UPDATE guilds SET invite_code = ? WHERE id = ?", (&code, id_to_db(guild_id)))
            .await
            .map_err(internal)?;
        self.state
            .session
            .execute_unpaged("DELETE FROM guilds_by_invite WHERE invite_code = ?", (&old.invite_code,))
            .await
            .map_err(internal)?;
        if let Some(guild) = self.find_guild(id_to_db(guild_id)).await? {
            self.state.events.publish_to_guild(guild_id, EventPayload::GuildUpdated(guild));
        }
        moderation::audit(&self.state, user_id, "guild.rotate_invite", &format!("guild:{guild_id}"), "").await;
        Response::ok(RotateInviteCodeResponse { invite_code: code, ..Default::default() })
    }

    async fn transfer_ownership(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, TransferOwnershipRequest>,
    ) -> ServiceResult<TransferOwnershipResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let guild_id = req.guild_id;
        self.owned_guild(user_id, guild_id, req.confirm_name).await?;
        if req.user_id == user_id {
            return Err(ConnectError::invalid_argument("you already own this guild"));
        }
        if !is_member(&self.state, req.user_id, guild_id).await? {
            return Err(ConnectError::not_found("the new owner must be a member of the guild"));
        }
        // Only the current owner's transfer wins if two race.
        let (applied, _) = self
            .state
            .session
            .execute_unpaged(
                "UPDATE guilds SET owner_id = ? WHERE id = ? IF owner_id = ?",
                (id_to_db(req.user_id), id_to_db(guild_id), id_to_db(user_id)),
            )
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .first_row::<(bool, Option<i64>)>()
            .map_err(internal)?;
        if !applied {
            return Err(ConnectError::aborted("ownership changed: refetch and retry"));
        }
        let guild = self
            .find_guild(id_to_db(guild_id))
            .await?
            .ok_or_else(|| ConnectError::not_found("guild not found"))?;
        self.state.events.publish_to_guild(guild_id, EventPayload::GuildUpdated(guild.clone()));
        moderation::audit(&self.state, user_id, "guild.transfer", &member_target(guild_id, req.user_id), "").await;
        Response::ok(TransferOwnershipResponse { guild: guild.into(), ..Default::default() })
    }
}

/// Audit target of an action on a member.
fn member_target(guild_id: u64, user_id: u64) -> String {
    format!("guild:{guild_id}/user:{user_id}")
}

impl GuildSvc {
    /// The guild if `user_id` owns it and `confirm_name` matches its name:
    /// `NotFound` for non-members, `PermissionDenied` for other members,
    /// `InvalidArgument` for a wrong name.
    async fn owned_guild(&self, user_id: u64, guild_id: u64, confirm_name: &str) -> Result<Guild, ConnectError> {
        if !standing(&self.state, user_id, guild_id).await?.is_owner {
            return Err(ConnectError::permission_denied("only the owner can do this"));
        }
        let guild = self
            .find_guild(id_to_db(guild_id))
            .await?
            .ok_or_else(|| ConnectError::not_found("guild not found"))?;
        if confirm_name.trim() != guild.name {
            return Err(ConnectError::invalid_argument("confirm_name does not match the guild's name"));
        }
        Ok(guild)
    }

    /// Checks `actor` may kick / ban `target`: they hold `permission` and
    /// strictly outrank the target (nobody but the owner can act on the owner;
    /// nobody acts on themself). Banning does not need the target to be a
    /// member (`must_be_member` false), but still protects the owner.
    async fn authorize_moderation(
        &self,
        actor: u64,
        guild_id: u64,
        target: u64,
        permission: u64,
        must_be_member: bool,
    ) -> Result<(), ConnectError> {
        let actor_standing = require_permission(&self.state, actor, guild_id, permission).await?;
        if actor == target {
            return Err(ConnectError::invalid_argument("you cannot do this to yourself"));
        }
        if !is_member(&self.state, target, guild_id).await? {
            if must_be_member {
                return Err(ConnectError::not_found("member not found"));
            }
            return Ok(());
        }
        let target_standing = standing(&self.state, target, guild_id).await?;
        if !actor_standing.outranks_member(&target_standing) {
            return Err(ConnectError::permission_denied("that member's top role is not below yours"));
        }
        Ok(())
    }
}

impl GuildSvc {
    /// Validates the legacy `icon` string ("" = default).
    fn parse_legacy_icon(&self, icon: &str) -> Result<Option<IconRef>, ConnectError> {
        validate_icon(icon)?;
        Ok((!icon.is_empty()).then(|| IconRef::Phosphor(icon.to_owned())))
    }

    /// Validates an `icon_ref` kind (`None` = reset to default). A custom emoji
    /// must exist in `guild_id`; every failure is INVALID_ARGUMENT.
    async fn parse_icon(
        &self,
        guild_id: u64,
        kind: Option<channel_icon::KindView<'_>>,
    ) -> Result<Option<IconRef>, ConnectError> {
        match kind {
            None => Ok(None),
            Some(channel_icon::KindView::Phosphor(name)) => self.parse_legacy_icon(name),
            Some(channel_icon::KindView::Unicode(emoji)) => {
                validate_unicode_emoji(emoji)?;
                Ok(Some(IconRef::Unicode(emoji.to_owned())))
            }
            Some(channel_icon::KindView::CustomEmojiId(id)) => {
                if !emoji_exists(&self.state, guild_id, id).await? {
                    return Err(ConnectError::invalid_argument("custom emoji does not exist in this guild"));
                }
                Ok(Some(IconRef::Custom(id)))
            }
        }
    }

    fn throttle_layout(&self, user_id: u64) -> Result<(), ConnectError> {
        self.state
            .limiter
            .check(&format!("guild_edit:{user_id}"), GUILD_EDIT_MAX, GUILD_EDIT_WINDOW)
    }

    /// Reloads the layout, publishes it to the guild and returns it.
    async fn publish_layout(&self, guild_id: u64) -> Result<GuildLayout, ConnectError> {
        let layout = layout::load_built(&self.state, guild_id).await?.layout;
        self.state.events.publish_to_guild(
            guild_id,
            EventPayload::LayoutUpdated(LayoutUpdated {
                guild_id,
                layout: layout.clone().into(),
                ..Default::default()
            }),
        );
        Ok(layout)
    }

    /// Reserves an unused invite code for `guild_db_id`, retrying on collisions.
    async fn claim_invite_code(&self, guild_db_id: i64) -> Result<String, ConnectError> {
        for _ in 0..INVITE_CODE_ATTEMPTS {
            let code = Alphanumeric.sample_string(&mut rand::rng(), INVITE_CODE_LEN);
            // An LWT returns `[applied]` plus the existing row's columns when not applied.
            let (claimed, _, _) = self
                .state
                .session
                .execute_unpaged(
                    "INSERT INTO guilds_by_invite (invite_code, guild_id) VALUES (?, ?) IF NOT EXISTS",
                    (&code, guild_db_id),
                )
                .await
                .map_err(internal)?
                .into_rows_result()
                .map_err(internal)?
                .first_row::<(bool, Option<String>, Option<i64>)>()
                .map_err(internal)?;
            if claimed {
                return Ok(code);
            }
        }
        Err(internal("could not allocate a unique invite code"))
    }

    /// Writes both membership tables in one logged batch so they stay consistent.
    /// Makes `user_id` a member of `guild` (no-op for members) and tells everyone.
    pub async fn admit(&self, guild: &Guild, user_id: u64) -> Result<(), ConnectError> {
        if !is_member(&self.state, user_id, guild.id).await? {
            if moderation::is_banned(&self.state, guild.id, user_id).await? {
                return Err(ConnectError::permission_denied("you are banned from this guild"));
            }
            self.add_member(guild.id, user_id).await?;
            if let Some(user) = find_user(&self.state, id_to_db(user_id)).await? {
                self.state.events.publish_to_guild(
                    guild.id,
                    EventPayload::GuildMemberJoined(GuildMemberJoined {
                        guild_id: guild.id,
                        user: user.into(),
                        ..Default::default()
                    }),
                );
            }
            self.state.events.notify_user_joined_guild(user_id, guild.clone());
            tracing::info!(guild_id = guild.id, user_id, "user joined guild");
        }
        Ok(())
    }

    async fn add_member(&self, guild_id: u64, user_id: u64) -> Result<(), ConnectError> {
        let (guild_id, user_id) = (id_to_db(guild_id), id_to_db(user_id));
        let joined_at = Utc::now();
        let mut batch = Batch::new(BatchType::Logged);
        batch.append_statement("INSERT INTO guild_members (guild_id, user_id, joined_at) VALUES (?, ?, ?)");
        batch.append_statement("INSERT INTO guilds_by_user (user_id, guild_id, joined_at) VALUES (?, ?, ?)");
        self.state
            .session
            .batch(
                &batch,
                ((guild_id, user_id, joined_at), (user_id, guild_id, joined_at)),
            )
            .await
            .map_err(internal)?;
        crate::stats::member_added(&self.state, id_from_db(guild_id)).await;
        Ok(())
    }

    /// Writes `channels` and `channels_by_id` in one logged batch.
    #[allow(clippy::too_many_arguments)]
    async fn insert_channel(
        &self,
        guild_id: u64,
        name: &str,
        section_id: Option<u64>,
        position: i32,
        icon: Option<&IconRef>,
        color: &str,
        kind: ChannelKind,
    ) -> Result<Channel, ConnectError> {
        let mirror = IconRef::legacy_mirror(icon);
        let (icon_kind, icon_value) = IconRef::columns_of(icon);
        let id = self.state.snowflake.next_id();
        let created_at = snowflake_datetime(id);
        let (db_guild_id, db_id) = (id_to_db(guild_id), id_to_db(id));
        let mut batch = Batch::new(BatchType::Logged);
        batch.append_statement(
            "INSERT INTO channels (guild_id, id, name, created_at, section_id, position, icon, color, topic, \
             icon_kind, icon_value, kind) VALUES (?, ?, ?, ?, ?, ?, ?, ?, '', ?, ?, ?)",
        );
        batch.append_statement("INSERT INTO channels_by_id (id, guild_id) VALUES (?, ?)");
        self.state
            .session
            .batch(
                &batch,
                (
                    (db_guild_id, db_id, name, created_at, section_id.map(id_to_db), position, mirror, color, icon_kind, &icon_value, layout::kind_column(kind)),
                    (db_id, db_guild_id),
                ),
            )
            .await
            .map_err(internal)?;
        crate::stats::channel_created(&self.state, guild_id).await;
        Ok(Channel {
            id,
            guild_id,
            name: name.to_owned(),
            created_at: datetime_to_proto(created_at).into(),
            section_id,
            position,
            icon: mirror.to_owned(),
            icon_ref: icon.map(IconRef::to_proto).into(),
            color: color.to_owned(),
            kind: kind.into(),
            ..Default::default()
        })
    }

    /// Atomically reserves one guild-creation slot for `user_id` via the
    /// `user_guild_quota` counter and rejects past [`MAX_GUILDS_PER_USER`]. The
    /// increment is atomic, so concurrent creates cannot all pass the cap; a
    /// rejected create compensates its increment.
    async fn reserve_guild_slot(&self, user_id: u64) -> Result<(), ConnectError> {
        let key = id_to_db(user_id);
        self.state
            .session
            .execute_unpaged(
                "UPDATE user_guild_quota SET created = created + 1 WHERE user_id = ?",
                (key,),
            )
            .await
            .map_err(internal)?;
        let (created,): (scylla::value::Counter,) = self
            .state
            .session
            .execute_unpaged("SELECT created FROM user_guild_quota WHERE user_id = ?", (key,))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .first_row::<(scylla::value::Counter,)>()
            .map_err(internal)?;
        if created.0 > MAX_GUILDS_PER_USER {
            let _ = self
                .state
                .session
                .execute_unpaged(
                    "UPDATE user_guild_quota SET created = created - 1 WHERE user_id = ?",
                    (key,),
                )
                .await;
            return Err(ConnectError::resource_exhausted("guild limit reached"));
        }
        Ok(())
    }

    /// Atomically reserves one channel-creation slot for `guild_id` via the
    /// `guild_channel_quota` counter and rejects past [`MAX_CHANNELS_PER_GUILD`].
    async fn reserve_channel_slot(&self, guild_id: u64) -> Result<(), ConnectError> {
        let key = id_to_db(guild_id);
        self.state
            .session
            .execute_unpaged(
                "UPDATE guild_channel_quota SET created = created + 1 WHERE guild_id = ?",
                (key,),
            )
            .await
            .map_err(internal)?;
        let (created,): (scylla::value::Counter,) = self
            .state
            .session
            .execute_unpaged("SELECT created FROM guild_channel_quota WHERE guild_id = ?", (key,))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?
            .first_row::<(scylla::value::Counter,)>()
            .map_err(internal)?;
        if created.0 > MAX_CHANNELS_PER_GUILD {
            let _ = self
                .state
                .session
                .execute_unpaged(
                    "UPDATE guild_channel_quota SET created = created - 1 WHERE guild_id = ?",
                    (key,),
                )
                .await;
            return Err(ConnectError::resource_exhausted("channel limit reached"));
        }
        Ok(())
    }

    async fn find_guild(&self, db_id: i64) -> Result<Option<Guild>, ConnectError> {
        load_guild(&self.state, db_id).await
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names_are_normalized() {
        assert_eq!(normalize_channel_name("  My   Channel ").as_deref(), Some("my-channel"));
        assert_eq!(normalize_channel_name("a_b-1").as_deref(), Some("a_b-1"));
        assert_eq!(normalize_channel_name(""), None);
        assert_eq!(normalize_channel_name("   "), None);
        assert_eq!(normalize_channel_name("bad!"), None);
        assert_eq!(normalize_channel_name("émile"), None);
        assert_eq!(normalize_channel_name(&"a".repeat(101)), None);
        assert!(normalize_channel_name(&"a".repeat(100)).is_some());
    }
}
