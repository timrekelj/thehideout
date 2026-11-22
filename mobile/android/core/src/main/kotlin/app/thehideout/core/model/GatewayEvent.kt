package app.thehideout.core.model

/** One payload of `gateway_service.proto`'s `Event`, as domain values. Exhaustive: the reducer handles each. */
sealed interface GatewayEvent {
    data object Heartbeat : GatewayEvent
    data class MessageCreated(val message: Message) : GatewayEvent
    data class MessageUpdated(val message: Message) : GatewayEvent
    data class MessageDeleted(val channelId: Long, val messageId: Long) : GatewayEvent
    data class ChannelCreated(val channel: Channel) : GatewayEvent
    data class ChannelUpdated(val channel: Channel) : GatewayEvent
    data class ChannelDeleted(val guildId: Long, val channelId: Long) : GatewayEvent
    data class MemberJoined(val guildId: Long, val user: User) : GatewayEvent
    data class MemberUpdated(val guildId: Long, val member: Member) : GatewayEvent
    data class MemberRemoved(val guildId: Long, val userId: Long, val reason: RemovalReason) : GatewayEvent
    data class DmOpened(val dm: DmChannel) : GatewayEvent
    data class GuildUpdated(val guild: Guild) : GatewayEvent
    data class GuildAdded(val guild: Guild) : GatewayEvent
    data class GuildRemoved(val guildId: Long) : GatewayEvent
    data class LayoutUpdated(val guildId: Long, val layout: GuildLayout) : GatewayEvent
    data class RolesUpdated(val guildId: Long, val roles: List<Role>, val version: Long) : GatewayEvent
    data class UserUpdated(val user: User) : GatewayEvent
    data class EmojisUpdated(val guildId: Long, val emojis: List<Emoji>) : GatewayEvent
    data class ReactionAdded(val channelId: Long, val messageId: Long, val emoji: ReactionEmoji, val userId: Long) : GatewayEvent
    data class ReactionRemoved(val channelId: Long, val messageId: Long, val emoji: ReactionEmoji, val userId: Long) : GatewayEvent
    /** `state == null`: the user is no longer in a voice channel of this guild. */
    data class VoiceStateUpdate(val guildId: Long, val userId: Long, val state: VoiceState?) : GatewayEvent
    data class VoiceSnapshot(val guildIds: List<Long>, val states: List<VoiceState>) : GatewayEvent
    data class VoiceMoved(val guildId: Long, val channelId: Long) : GatewayEvent
    /** The caller changed their notification settings on another session; the full new settings. */
    data class NotificationSettingsUpdated(val settings: NotificationSettings) : GatewayEvent
}
