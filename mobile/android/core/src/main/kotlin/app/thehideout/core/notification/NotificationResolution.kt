package app.thehideout.core.notification

import app.thehideout.core.model.NotificationLevel
import app.thehideout.core.model.NotificationOverride
import app.thehideout.core.model.NotificationSettings
import java.time.Instant

/** Whether this override currently mutes (a `muted_until` in the future; unset or past = not muted). */
fun NotificationOverride.isMuted(now: Instant): Boolean = mutedUntil?.isAfter(now) == true

/**
 * The level that actually governs a channel: its own override wins unless it is INHERIT, then its
 * guild's (unless that guild has none, or a DM has no guild), then the global default
 * (`guild_default` for guild channels, `dms_enabled` for DMs). Mirrors the server's own precedence
 * (`server/src/push/settings.rs::Settings::effective`'s level half).
 */
fun NotificationSettings.effectiveLevel(guildId: Long?, channelId: Long): NotificationLevel {
    channels.firstOrNull { it.id == channelId }?.let { if (it.level != NotificationLevel.Inherit) return it.level }
    if (guildId != null) {
        guilds.firstOrNull { it.id == guildId }?.let { if (it.level != NotificationLevel.Inherit) return it.level }
        return guildDefault
    }
    return if (dmsEnabled) NotificationLevel.All else NotificationLevel.Nothing
}

/**
 * Whether a channel is muted right now, with the server's exact precedence
 * (`server/src/push/settings.rs::Settings::effective`): the channel's own mute always wins; failing
 * that, a channel with its own level (not INHERIT) "punches through" a muted guild — its own
 * decision stands even though the guild is muted; only when the channel has no opinion (no
 * override, or an INHERIT one with a lapsed or no mute of its own) does a muted guild make it
 * muted. A DM has no guild to fall through to.
 */
fun NotificationSettings.isMuted(guildId: Long?, channelId: Long, now: Instant): Boolean {
    val channel = channels.firstOrNull { it.id == channelId }
    if (channel != null) {
        if (channel.isMuted(now)) return true
        if (channel.level != NotificationLevel.Inherit) return false
    }
    if (guildId != null) guilds.firstOrNull { it.id == guildId }?.let { return it.isMuted(now) }
    return false
}
