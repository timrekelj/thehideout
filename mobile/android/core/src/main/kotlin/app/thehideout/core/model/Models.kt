package app.thehideout.core.model

import java.time.Instant

/** Domain models, mapped from the generated protobuf-lite messages (see `Mapping.kt`). Ids are uint64 snowflakes. */

data class User(
    val id: Long,
    val username: String,
    val createdAt: Instant,
    /** Opaque media key; "" = no avatar. */
    val avatar: String = "",
    val isAdmin: Boolean = false,
    val email: String? = null,
    val deleted: Boolean = false,
) {
    /** What to show: deleted accounts are anonymised by the server already, but never trust a name alone. */
    val displayName: String get() = if (deleted) DELETED_USER else username

    companion object {
        const val DELETED_USER = "Deleted user"
    }
}

data class Guild(
    val id: Long,
    val name: String,
    val ownerId: Long,
    val inviteCode: String,
    val createdAt: Instant,
    /** Opaque media key; "" = no icon. */
    val icon: String = "",
)

enum class ChannelKind { Text, Voice }

/** The three-way channel icon (Phosphor name / Unicode emoji / custom emoji); `Default` = derive one. */
sealed interface ChannelIconRef {
    data object Default : ChannelIconRef
    data class Phosphor(val name: String) : ChannelIconRef
    data class Unicode(val emoji: String) : ChannelIconRef
    data class CustomEmoji(val id: Long) : ChannelIconRef
}

data class Channel(
    val id: Long,
    val guildId: Long,
    val name: String,
    val createdAt: Instant,
    val sectionId: Long? = null,
    val position: Int = 0,
    val icon: ChannelIconRef = ChannelIconRef.Default,
    /** Palette key (`slate` .. `pink`) or "". */
    val color: String = "",
    val topic: String = "",
    val kind: ChannelKind = ChannelKind.Text,
)

data class Section(val id: Long, val guildId: Long, val name: String, val position: Int)

/** The channels of one section in display order; `sectionId == 0` is the top level. */
data class LayoutEntry(val sectionId: Long, val channelIds: List<Long>)

data class GuildLayout(
    val version: Long,
    val sections: List<Section> = emptyList(),
    val entries: List<LayoutEntry> = emptyList(),
) {
    /** Every channel id in sidebar order: top level first, then each section's. */
    fun channelOrder(): List<Long> = entries.sortedBy { e -> if (e.sectionId == 0L) -1 else sections.indexOfFirst { it.id == e.sectionId } }
        .flatMap { it.channelIds }

    /** Keeps the invariants desktop relies on: a top-level entry exists and every section has one. */
    fun normalized(): GuildLayout {
        val byId = entries.associateBy { it.sectionId }
        val top = byId[0L] ?: LayoutEntry(0L, emptyList())
        val rest = sections.map { s -> byId[s.id] ?: LayoutEntry(s.id, emptyList()) }
        return copy(entries = listOf(top) + rest)
    }
}

data class Role(
    val id: Long,
    val guildId: Long,
    val name: String,
    val color: String,
    val position: Int,
    val permissions: Long,
    val createdAt: Instant,
)

data class Member(val user: User, val roleIds: List<Long>, val joinedAt: Instant)

sealed interface ReactionEmoji {
    data class Unicode(val emoji: String) : ReactionEmoji
    data class Custom(val id: Long) : ReactionEmoji
}

data class Reaction(val emoji: ReactionEmoji, val count: Int, val me: Boolean)

data class Attachment(
    val id: Long,
    val filename: String,
    val contentType: String,
    val size: Long,
    val file: String,
    val width: Int? = null,
    val height: Int? = null,
    val thumbnail: String? = null,
    val removed: Boolean = false,
)

enum class EmbedKind { Link, Image, Video, Rich }
enum class EmbedMediaKind { Image, Video, Gifv }

data class EmbedMedia(
    val kind: EmbedMediaKind,
    val file: String,
    val poster: String? = null,
    val width: Int = 0,
    val height: Int = 0,
    val durationMs: Int? = null,
    val size: Long = 0,
)

data class Embed(
    val url: String,
    val title: String = "",
    val description: String = "",
    val siteName: String = "",
    val thumbnail: String? = null,
    val thumbnailWidth: Int? = null,
    val thumbnailHeight: Int? = null,
    val kind: EmbedKind = EmbedKind.Link,
    val provider: String = "",
    val authorName: String = "",
    val authorUrl: String? = null,
    val authorIcon: String? = null,
    val media: List<EmbedMedia> = emptyList(),
    val publishedAt: Instant? = null,
)

data class Message(
    val id: Long,
    val channelId: Long,
    /** `null` in a direct-message channel. */
    val guildId: Long?,
    val authorId: Long,
    val content: String,
    val createdAt: Instant,
    val editedAt: Instant? = null,
    val nonce: String = "",
    val reactions: List<Reaction> = emptyList(),
    val attachments: List<Attachment> = emptyList(),
    val embeds: List<Embed> = emptyList(),
)

data class DmChannel(val id: Long, val participants: List<User>, val createdAt: Instant)

data class VoiceState(
    val guildId: Long,
    val channelId: Long,
    val userId: Long,
    val selfMute: Boolean = false,
    val selfDeaf: Boolean = false,
    val serverMute: Boolean = false,
    val selfVideo: Boolean = false,
    val selfStream: Boolean = false,
    val joinedAt: Instant,
)

enum class InviteStatus { Unspecified, Pending, Accepted, Expired, Revoked }

data class Invite(
    val id: Long,
    val email: String,
    val code: String?,
    val status: InviteStatus,
    val inviter: User?,
    val guildIds: List<Long>,
    val createdAt: Instant,
    val expiresAt: Instant,
    val makeAdmin: Boolean,
    val link: String?,
)

enum class RemovalReason { Unspecified, Left, Kicked, Banned }

/** Permission bits (`Permission` in types.proto). */
object Perm {
    const val ADMINISTRATOR = 1L
    const val MANAGE_SERVER = 2L
    const val MANAGE_CHANNELS = 4L
    const val MANAGE_ROLES = 8L
    const val MANAGE_EMOJIS = 16L
    const val INVITE_PEOPLE = 32L
    const val CONNECT = 64L
    const val SPEAK = 128L
    const val VIDEO = 256L
    const val MUTE_MEMBERS = 512L
    const val MOVE_MEMBERS = 1024L
    const val MANAGE_MESSAGES = 2048L
    const val KICK_MEMBERS = 4096L
    const val BAN_MEMBERS = 8192L
    private const val BASELINE = CONNECT or SPEAK or VIDEO

    /** The owner has everything; a member has the OR of their roles plus the voice baseline; ADMINISTRATOR implies all. */
    fun effective(guild: Guild, user: Long, roleIds: List<Long>, roles: List<Role>): Long {
        if (guild.ownerId == user) return Long.MAX_VALUE
        val bits = roles.filter { it.id in roleIds }.fold(BASELINE) { acc, r -> acc or r.permissions }
        return if (bits and ADMINISTRATOR != 0L) Long.MAX_VALUE else bits
    }

    fun has(set: Long, bit: Long): Boolean = set and bit == bit
}

data class Emoji(
    val id: Long,
    val guildId: Long,
    val name: String,
    val animated: Boolean,
    val image: String,
    val creatorId: Long,
    val createdAt: Instant,
)
