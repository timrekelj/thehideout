package app.thehideout.core.model

import com.google.protobuf.Timestamp
import java.time.Instant

private typealias PbUser = com.thehideout.v1.User
private typealias PbGuild = com.thehideout.v1.Guild
private typealias PbChannel = com.thehideout.v1.Channel
private typealias PbMessage = com.thehideout.v1.Message
private typealias PbMember = com.thehideout.v1.Member
private typealias PbRole = com.thehideout.v1.Role
private typealias PbDm = com.thehideout.v1.DmChannel
private typealias PbLayout = com.thehideout.v1.GuildLayout
private typealias PbEmoji = com.thehideout.v1.Emoji
private typealias PbVoice = com.thehideout.v1.VoiceState
private typealias PbReactionEmoji = com.thehideout.v1.ReactionEmoji
private typealias PbInvite = com.thehideout.v1.Invite

/** Mapping from the generated protobuf-lite messages to the domain models (and back where the app sends). */

fun Timestamp.toInstant(): Instant = Instant.ofEpochSecond(seconds, nanos.toLong())

fun Instant.toTimestamp(): Timestamp = Timestamp.newBuilder().setSeconds(epochSecond).setNanos(nano).build()

private fun Long.orNullIfZero(): Long? = if (this == 0L) null else this

fun PbUser.toModel() = User(
    id = id,
    username = username,
    createdAt = createdAt.toInstant(),
    avatar = avatar,
    isAdmin = isAdmin,
    email = if (hasEmail()) email else null,
    deleted = deleted,
)

fun PbGuild.toModel() = Guild(id, name, ownerId, inviteCode, createdAt.toInstant(), icon)

fun PbChannel.toModel(): Channel {
    val ref: ChannelIconRef = if (hasIconRef()) {
        when (iconRef.kindCase) {
            com.thehideout.v1.ChannelIcon.KindCase.PHOSPHOR -> ChannelIconRef.Phosphor(iconRef.phosphor)
            com.thehideout.v1.ChannelIcon.KindCase.UNICODE -> ChannelIconRef.Unicode(iconRef.unicode)
            com.thehideout.v1.ChannelIcon.KindCase.CUSTOM_EMOJI_ID -> ChannelIconRef.CustomEmoji(iconRef.customEmojiId)
            else -> ChannelIconRef.Default
        }
    } else if (icon.isNotEmpty()) {
        ChannelIconRef.Phosphor(icon)
    } else {
        ChannelIconRef.Default
    }
    return Channel(
        id = id,
        guildId = guildId,
        name = name,
        createdAt = createdAt.toInstant(),
        sectionId = if (hasSectionId()) sectionId else null,
        position = position,
        icon = ref,
        color = color,
        topic = topic,
        // CHANNEL_KIND_UNSPECIFIED (rows that predate voice) means text.
        kind = if (kind == com.thehideout.v1.ChannelKind.CHANNEL_KIND_VOICE) ChannelKind.Voice else ChannelKind.Text,
    )
}

fun com.thehideout.v1.Section.toModel() = Section(id, guildId, name, position)

fun PbLayout.toModel() = GuildLayout(
    version = version,
    sections = sectionsList.map { it.toModel() },
    entries = entriesList.map { LayoutEntry(it.sectionId, it.channelIdsList.toList()) },
)

fun PbRole.toModel() = Role(id, guildId, name, color, position, permissions, createdAt.toInstant())

fun PbMember.toModel() = Member(user.toModel(), roleIdsList.toList(), joinedAt.toInstant())

fun PbEmoji.toModel() = Emoji(id, guildId, name, animated, image, creatorId, createdAt.toInstant())

fun com.thehideout.v1.Ban.toModel() = Ban(user.toModel(), bannedBy, reason, createdAt.toInstant())

fun PbDm.toModel() = DmChannel(id, participantsList.map { it.toModel() }, createdAt.toInstant())

fun PbReactionEmoji.toModel(): ReactionEmoji = when (kindCase) {
    com.thehideout.v1.ReactionEmoji.KindCase.CUSTOM_EMOJI_ID -> ReactionEmoji.Custom(customEmojiId)
    else -> ReactionEmoji.Unicode(unicode)
}

fun ReactionEmoji.toProto(): PbReactionEmoji {
    val b = PbReactionEmoji.newBuilder()
    when (this) {
        is ReactionEmoji.Unicode -> b.unicode = emoji
        is ReactionEmoji.Custom -> b.customEmojiId = id
    }
    return b.build()
}

fun com.thehideout.v1.Attachment.toModel() = Attachment(
    id = id,
    filename = filename,
    contentType = contentType,
    size = size,
    file = file,
    width = if (hasWidth()) width else null,
    height = if (hasHeight()) height else null,
    thumbnail = if (hasThumbnail()) thumbnail else null,
    removed = removed,
)

fun com.thehideout.v1.Embed.toModel() = Embed(
    url = url,
    title = title,
    description = description,
    siteName = siteName,
    thumbnail = if (hasThumbnail()) thumbnail else null,
    thumbnailWidth = if (hasThumbnailWidth()) thumbnailWidth else null,
    thumbnailHeight = if (hasThumbnailHeight()) thumbnailHeight else null,
    kind = when (kind) {
        com.thehideout.v1.EmbedKind.EMBED_KIND_IMAGE -> EmbedKind.Image
        com.thehideout.v1.EmbedKind.EMBED_KIND_VIDEO -> EmbedKind.Video
        com.thehideout.v1.EmbedKind.EMBED_KIND_RICH -> EmbedKind.Rich
        else -> EmbedKind.Link
    },
    provider = provider,
    authorName = authorName,
    authorUrl = if (hasAuthorUrl()) authorUrl else null,
    authorIcon = if (hasAuthorIcon()) authorIcon else null,
    media = mediaList.map {
        EmbedMedia(
            kind = when (it.kind) {
                com.thehideout.v1.EmbedMediaKind.EMBED_MEDIA_KIND_VIDEO -> EmbedMediaKind.Video
                com.thehideout.v1.EmbedMediaKind.EMBED_MEDIA_KIND_GIFV -> EmbedMediaKind.Gifv
                else -> EmbedMediaKind.Image
            },
            file = it.file,
            poster = if (it.hasPoster()) it.poster else null,
            width = it.width,
            height = it.height,
            durationMs = if (it.hasDurationMs()) it.durationMs else null,
            size = it.size,
        )
    },
    publishedAt = if (hasPublishedAt()) publishedAt.toInstant() else null,
)

fun PbMessage.toModel() = Message(
    id = id,
    channelId = channelId,
    guildId = guildId.orNullIfZero(),
    authorId = authorId,
    content = content,
    createdAt = createdAt.toInstant(),
    editedAt = if (hasEditedAt()) editedAt.toInstant() else null,
    nonce = nonce,
    reactions = reactionsList.map { Reaction(it.emoji.toModel(), it.count, it.me) },
    attachments = attachmentsList.map { it.toModel() },
    embeds = embedsList.map { it.toModel() },
)

fun PbVoice.toModel() = VoiceState(guildId, channelId, userId, selfMute, selfDeaf, serverMute, selfVideo, selfStream, joinedAt.toInstant())

fun PbInvite.toModel() = Invite(
    id = id,
    email = email,
    code = if (hasCode()) code else null,
    status = when (status) {
        com.thehideout.v1.InviteStatus.INVITE_STATUS_PENDING -> InviteStatus.Pending
        com.thehideout.v1.InviteStatus.INVITE_STATUS_ACCEPTED -> InviteStatus.Accepted
        com.thehideout.v1.InviteStatus.INVITE_STATUS_EXPIRED -> InviteStatus.Expired
        com.thehideout.v1.InviteStatus.INVITE_STATUS_REVOKED -> InviteStatus.Revoked
        else -> InviteStatus.Unspecified
    },
    inviter = if (hasInviter()) inviter.toModel() else null,
    guildIds = guildIdsList.toList(),
    createdAt = createdAt.toInstant(),
    expiresAt = expiresAt.toInstant(),
    makeAdmin = makeAdmin,
    link = if (hasLink()) link else null,
)

fun com.thehideout.v1.NotificationLevel.toModel(): NotificationLevel = when (this) {
    com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_INHERIT -> NotificationLevel.Inherit
    com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_ALL -> NotificationLevel.All
    com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_MENTIONS -> NotificationLevel.Mentions
    com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_NOTHING -> NotificationLevel.Nothing
    else -> NotificationLevel.Unspecified
}

fun com.thehideout.v1.NotificationOverride.toModel() =
    NotificationOverride(id, level.toModel(), if (hasMutedUntil()) mutedUntil.toInstant() else null)

fun com.thehideout.v1.NotificationSettings.toModel() = NotificationSettings(
    dmsEnabled = dmsEnabled,
    guildDefault = guildDefault.toModel(),
    showPreviews = showPreviews,
    guilds = guildsList.map { it.toModel() },
    channels = channelsList.map { it.toModel() },
)

fun NotificationLevel.toProto(): com.thehideout.v1.NotificationLevel = when (this) {
    NotificationLevel.Inherit -> com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_INHERIT
    NotificationLevel.All -> com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_ALL
    NotificationLevel.Mentions -> com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_MENTIONS
    NotificationLevel.Nothing -> com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_NOTHING
    NotificationLevel.Unspecified -> com.thehideout.v1.NotificationLevel.NOTIFICATION_LEVEL_UNSPECIFIED
}

fun NotificationOverride.toProto(): com.thehideout.v1.NotificationOverride {
    val b = com.thehideout.v1.NotificationOverride.newBuilder().setId(id).setLevel(level.toProto())
    mutedUntil?.let { b.mutedUntil = it.toTimestamp() }
    return b.build()
}

fun com.thehideout.v1.Passkey.toModel() = app.thehideout.core.security.Passkey(
    id = id,
    name = name,
    createdAt = createdAt.toInstant(),
    lastUsedAt = if (hasLastUsedAt()) lastUsedAt.toInstant() else null,
)

fun com.thehideout.v1.Session.toModel() = app.thehideout.core.security.DeviceSession(
    id = id,
    deviceName = deviceName,
    createdAt = createdAt.toInstant(),
    lastSeenAt = if (hasLastSeenAt()) lastSeenAt.toInstant() else null,
    current = current,
)

/** Maps a stream item to a domain event; `null` for a payload this client does not know yet. */
fun com.thehideout.v1.Event.toModel(): GatewayEvent? {
    val e = this
    return when (e.payloadCase) {
        com.thehideout.v1.Event.PayloadCase.MESSAGE_CREATED -> GatewayEvent.MessageCreated(e.messageCreated.toModel())
        com.thehideout.v1.Event.PayloadCase.CHANNEL_CREATED -> GatewayEvent.ChannelCreated(e.channelCreated.toModel())
        com.thehideout.v1.Event.PayloadCase.GUILD_MEMBER_JOINED -> GatewayEvent.MemberJoined(e.guildMemberJoined.guildId, e.guildMemberJoined.user.toModel())
        com.thehideout.v1.Event.PayloadCase.HEARTBEAT -> GatewayEvent.Heartbeat
        com.thehideout.v1.Event.PayloadCase.DM_OPENED -> GatewayEvent.DmOpened(e.dmOpened.toModel())
        com.thehideout.v1.Event.PayloadCase.GUILD_UPDATED -> GatewayEvent.GuildUpdated(e.guildUpdated.toModel())
        com.thehideout.v1.Event.PayloadCase.CHANNEL_UPDATED -> GatewayEvent.ChannelUpdated(e.channelUpdated.toModel())
        com.thehideout.v1.Event.PayloadCase.LAYOUT_UPDATED -> GatewayEvent.LayoutUpdated(e.layoutUpdated.guildId, e.layoutUpdated.layout.toModel())
        com.thehideout.v1.Event.PayloadCase.ROLES_UPDATED ->
            GatewayEvent.RolesUpdated(e.rolesUpdated.guildId, e.rolesUpdated.rolesList.map { it.toModel() }, e.rolesUpdated.version)
        com.thehideout.v1.Event.PayloadCase.MEMBER_UPDATED -> GatewayEvent.MemberUpdated(e.memberUpdated.guildId, e.memberUpdated.member.toModel())
        com.thehideout.v1.Event.PayloadCase.USER_UPDATED -> GatewayEvent.UserUpdated(e.userUpdated.toModel())
        com.thehideout.v1.Event.PayloadCase.EMOJIS_UPDATED ->
            GatewayEvent.EmojisUpdated(e.emojisUpdated.guildId, e.emojisUpdated.emojisList.map { it.toModel() })
        com.thehideout.v1.Event.PayloadCase.REACTION_ADDED ->
            e.reactionAdded.let { GatewayEvent.ReactionAdded(it.channelId, it.messageId, it.emoji.toModel(), it.userId) }
        com.thehideout.v1.Event.PayloadCase.REACTION_REMOVED ->
            e.reactionRemoved.let { GatewayEvent.ReactionRemoved(it.channelId, it.messageId, it.emoji.toModel(), it.userId) }
        com.thehideout.v1.Event.PayloadCase.MESSAGE_UPDATED -> GatewayEvent.MessageUpdated(e.messageUpdated.message.toModel())
        com.thehideout.v1.Event.PayloadCase.GUILD_ADDED -> GatewayEvent.GuildAdded(e.guildAdded.toModel())
        com.thehideout.v1.Event.PayloadCase.VOICE_STATE_UPDATE ->
            e.voiceStateUpdate.let { GatewayEvent.VoiceStateUpdate(it.guildId, it.userId, if (it.hasState()) it.state.toModel() else null) }
        com.thehideout.v1.Event.PayloadCase.VOICE_SNAPSHOT ->
            GatewayEvent.VoiceSnapshot(e.voiceSnapshot.guildIdsList.toList(), e.voiceSnapshot.statesList.map { it.toModel() })
        com.thehideout.v1.Event.PayloadCase.VOICE_MOVED -> GatewayEvent.VoiceMoved(e.voiceMoved.guildId, e.voiceMoved.channelId)
        com.thehideout.v1.Event.PayloadCase.MESSAGE_DELETED -> GatewayEvent.MessageDeleted(e.messageDeleted.channelId, e.messageDeleted.messageId)
        com.thehideout.v1.Event.PayloadCase.GUILD_MEMBER_REMOVED ->
            e.guildMemberRemoved.let {
                GatewayEvent.MemberRemoved(
                    it.guildId,
                    it.userId,
                    when (it.reason) {
                        com.thehideout.v1.MemberRemovalReason.MEMBER_REMOVAL_REASON_LEFT -> RemovalReason.Left
                        com.thehideout.v1.MemberRemovalReason.MEMBER_REMOVAL_REASON_KICKED -> RemovalReason.Kicked
                        com.thehideout.v1.MemberRemovalReason.MEMBER_REMOVAL_REASON_BANNED -> RemovalReason.Banned
                        else -> RemovalReason.Unspecified
                    },
                )
            }
        com.thehideout.v1.Event.PayloadCase.GUILD_REMOVED -> GatewayEvent.GuildRemoved(e.guildRemoved.guildId)
        com.thehideout.v1.Event.PayloadCase.CHANNEL_DELETED -> GatewayEvent.ChannelDeleted(e.channelDeleted.guildId, e.channelDeleted.channelId)
        com.thehideout.v1.Event.PayloadCase.NOTIFICATION_SETTINGS_UPDATED -> GatewayEvent.NotificationSettingsUpdated(e.notificationSettingsUpdated.toModel())
        com.thehideout.v1.Event.PayloadCase.PAYLOAD_NOT_SET, null -> null
    }
}

// ---- domain -> proto (what the file cache stores; the app never sends these) -----------------------

fun User.toProto(): PbUser = PbUser.newBuilder()
    .setId(id).setUsername(username).setCreatedAt(createdAt.toTimestamp()).setAvatar(avatar).setIsAdmin(isAdmin).setDeleted(deleted)
    .apply { this@toProto.email?.let { setEmail(it) } }
    .build()

fun Guild.toProto(): PbGuild = PbGuild.newBuilder()
    .setId(id).setName(name).setOwnerId(ownerId).setInviteCode(inviteCode).setCreatedAt(createdAt.toTimestamp()).setIcon(icon).build()

fun Channel.toProto(): PbChannel {
    val b = PbChannel.newBuilder()
        .setId(id).setGuildId(guildId).setName(name).setCreatedAt(createdAt.toTimestamp()).setPosition(position)
        .setColor(color).setTopic(topic)
        .setKind(if (kind == ChannelKind.Voice) com.thehideout.v1.ChannelKind.CHANNEL_KIND_VOICE else com.thehideout.v1.ChannelKind.CHANNEL_KIND_TEXT)
    sectionId?.let { b.sectionId = it }
    when (val i = icon) {
        ChannelIconRef.Default -> Unit
        is ChannelIconRef.Phosphor -> b.setIcon(i.name).setIconRef(com.thehideout.v1.ChannelIcon.newBuilder().setPhosphor(i.name))
        is ChannelIconRef.Unicode -> b.setIconRef(com.thehideout.v1.ChannelIcon.newBuilder().setUnicode(i.emoji))
        is ChannelIconRef.CustomEmoji -> b.setIconRef(com.thehideout.v1.ChannelIcon.newBuilder().setCustomEmojiId(i.id))
    }
    return b.build()
}

fun GuildLayout.toProto(): PbLayout = PbLayout.newBuilder()
    .setVersion(version)
    .addAllSections(sections.map { com.thehideout.v1.Section.newBuilder().setId(it.id).setGuildId(it.guildId).setName(it.name).setPosition(it.position).build() })
    .addAllEntries(entries.map { com.thehideout.v1.LayoutEntry.newBuilder().setSectionId(it.sectionId).addAllChannelIds(it.channelIds).build() })
    .build()

fun DmChannel.toProto(): PbDm = PbDm.newBuilder().setId(id).addAllParticipants(participants.map { it.toProto() }).setCreatedAt(createdAt.toTimestamp()).build()

fun Message.toProto(): PbMessage {
    val b = PbMessage.newBuilder()
        .setId(id).setChannelId(channelId).setGuildId(guildId ?: 0L).setAuthorId(authorId).setContent(content)
        .setCreatedAt(createdAt.toTimestamp()).setNonce(nonce)
        .addAllReactions(reactions.map { com.thehideout.v1.Reaction.newBuilder().setEmoji(it.emoji.toProto()).setCount(it.count).setMe(it.me).build() })
        .addAllAttachments(attachments.map { a ->
            com.thehideout.v1.Attachment.newBuilder().setId(a.id).setFilename(a.filename).setContentType(a.contentType).setSize(a.size).setFile(a.file).setRemoved(a.removed)
                .apply { a.width?.let { setWidth(it) }; a.height?.let { setHeight(it) }; a.thumbnail?.let { setThumbnail(it) } }
                .build()
        })
        .addAllEmbeds(embeds.map { e ->
            com.thehideout.v1.Embed.newBuilder().setUrl(e.url).setTitle(e.title).setDescription(e.description).setSiteName(e.siteName)
                .setKind(
                    when (e.kind) {
                        EmbedKind.Link -> com.thehideout.v1.EmbedKind.EMBED_KIND_LINK
                        EmbedKind.Image -> com.thehideout.v1.EmbedKind.EMBED_KIND_IMAGE
                        EmbedKind.Video -> com.thehideout.v1.EmbedKind.EMBED_KIND_VIDEO
                        EmbedKind.Rich -> com.thehideout.v1.EmbedKind.EMBED_KIND_RICH
                    },
                )
                .setProvider(e.provider).setAuthorName(e.authorName)
                .apply {
                    e.thumbnail?.let { setThumbnail(it) }
                    e.thumbnailWidth?.let { setThumbnailWidth(it) }
                    e.thumbnailHeight?.let { setThumbnailHeight(it) }
                    e.authorUrl?.let { setAuthorUrl(it) }
                    e.authorIcon?.let { setAuthorIcon(it) }
                    e.publishedAt?.let { setPublishedAt(it.toTimestamp()) }
                    addAllMedia(
                        e.media.map { m ->
                            com.thehideout.v1.EmbedMedia.newBuilder().setFile(m.file).setWidth(m.width).setHeight(m.height).setSize(m.size)
                                .setKind(
                                    when (m.kind) {
                                        EmbedMediaKind.Image -> com.thehideout.v1.EmbedMediaKind.EMBED_MEDIA_KIND_IMAGE
                                        EmbedMediaKind.Video -> com.thehideout.v1.EmbedMediaKind.EMBED_MEDIA_KIND_VIDEO
                                        EmbedMediaKind.Gifv -> com.thehideout.v1.EmbedMediaKind.EMBED_MEDIA_KIND_GIFV
                                    },
                                )
                                .apply { m.poster?.let { setPoster(it) }; m.durationMs?.let { setDurationMs(it) } }
                                .build()
                        },
                    )
                }
                .build()
        })
    editedAt?.let { b.setEditedAt(it.toTimestamp()) }
    return b.build()
}
