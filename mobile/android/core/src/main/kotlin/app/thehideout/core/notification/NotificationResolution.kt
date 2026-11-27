package app.thehideout.core.notification

import app.thehideout.core.model.NotificationLevel
import app.thehideout.core.model.NotificationOverride
import app.thehideout.core.model.NotificationSettings
import java.time.Instant

/** Whether this override currently mutes (a `muted_until` in the future; unset or past = not muted). */
fun NotificationOverride.isMuted(now: Instant): Boolean = mutedUntil?.isAfter(now) == true

/**
 * The level that actually governs a channel for the Notifications page's summaries: its own
 * override wins unless it is INHERIT, then its guild's (unless that guild has none, or a DM has no
 * guild), then the global default (`guild_default` for guild channels, `dms_enabled` for DMs).
 * Mirrors the server's own precedence (`notification_service.proto`).
 */
fun NotificationSettings.effectiveLevel(guildId: Long?, channelId: Long): NotificationLevel {
    channels.firstOrNull { it.id == channelId }?.let { if (it.level != NotificationLevel.Inherit) return it.level }
    if (guildId != null) {
        guilds.firstOrNull { it.id == guildId }?.let { if (it.level != NotificationLevel.Inherit) return it.level }
        return guildDefault
    }
    return if (dmsEnabled) NotificationLevel.All else NotificationLevel.Nothing
}

/** Whether a channel is muted right now: its own override wins, else its guild's (a DM has none). */
fun NotificationSettings.isMuted(guildId: Long?, channelId: Long, now: Instant): Boolean {
    channels.firstOrNull { it.id == channelId }?.let { return it.isMuted(now) }
    if (guildId != null) guilds.firstOrNull { it.id == guildId }?.let { return it.isMuted(now) }
    return false
}
