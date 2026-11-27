package app.thehideout.core

import app.thehideout.core.model.NotificationLevel
import app.thehideout.core.model.NotificationOverride
import app.thehideout.core.model.NotificationSettings
import app.thehideout.core.notification.effectiveLevel
import app.thehideout.core.notification.isMuted
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import java.time.Instant

/**
 * Mirrors `server/src/push/settings.rs`'s own `Settings::effective` test cases, so the client's
 * notion of "what level/mute applies" never drifts from what the server actually decides.
 */
class NotificationResolutionTest {
    private val now = Instant.parse("2026-11-27T12:00:00Z")
    private val guild = 1L
    private val channel = 2L

    private fun settings(
        dmsEnabled: Boolean = true,
        guildDefault: NotificationLevel = NotificationLevel.Nothing,
        guilds: List<NotificationOverride> = emptyList(),
        channels: List<NotificationOverride> = emptyList(),
    ) = NotificationSettings(dmsEnabled, guildDefault, true, guilds, channels)

    @Test
    fun `climbs from channel to guild to the default`() {
        var s = settings(guildDefault = NotificationLevel.Nothing)
        assertEquals(NotificationLevel.Nothing, s.effectiveLevel(guild, channel))
        assertFalse(s.isMuted(guild, channel, now))

        s = settings(guildDefault = NotificationLevel.Nothing, guilds = listOf(NotificationOverride(guild, NotificationLevel.Mentions)))
        assertEquals(NotificationLevel.Mentions, s.effectiveLevel(guild, channel))

        s = settings(
            guildDefault = NotificationLevel.Nothing,
            guilds = listOf(NotificationOverride(guild, NotificationLevel.Mentions)),
            channels = listOf(NotificationOverride(channel, NotificationLevel.All)),
        )
        assertEquals(NotificationLevel.All, s.effectiveLevel(guild, channel))

        // An INHERIT channel override defers back to the guild.
        s = settings(
            guildDefault = NotificationLevel.Nothing,
            guilds = listOf(NotificationOverride(guild, NotificationLevel.Mentions)),
            channels = listOf(NotificationOverride(channel, NotificationLevel.Inherit)),
        )
        assertEquals(NotificationLevel.Mentions, s.effectiveLevel(guild, channel))

        // A channel in another guild is untouched.
        assertEquals(NotificationLevel.Nothing, s.effectiveLevel(guild + 1, channel + 1))
    }

    @Test
    fun `mutes silence until they lapse, but a channel's own level punches through a guild mute`() {
        var s = settings(guildDefault = NotificationLevel.All, guilds = listOf(NotificationOverride(guild, NotificationLevel.Inherit, now.plusSeconds(3600))))
        assertTrue(s.isMuted(guild, channel, now))

        // A channel with its own level is not muted by the guild's mute...
        s = settings(
            guildDefault = NotificationLevel.All,
            guilds = listOf(NotificationOverride(guild, NotificationLevel.Inherit, now.plusSeconds(3600))),
            channels = listOf(NotificationOverride(channel, NotificationLevel.All)),
        )
        assertFalse(s.isMuted(guild, channel, now))
        assertEquals(NotificationLevel.All, s.effectiveLevel(guild, channel))

        // ...but a muted channel stays silent whatever its level.
        s = settings(
            guildDefault = NotificationLevel.All,
            guilds = listOf(NotificationOverride(guild, NotificationLevel.Inherit, now.plusSeconds(3600))),
            channels = listOf(NotificationOverride(channel, NotificationLevel.All, now.plusSeconds(3600))),
        )
        assertTrue(s.isMuted(guild, channel, now))

        // Lapsed mutes are ignored; a lapsed, otherwise-opinionless channel override does not mask
        // the guild's own (still active) mute.
        s = settings(
            guildDefault = NotificationLevel.All,
            guilds = listOf(NotificationOverride(guild, NotificationLevel.Inherit, now.plusSeconds(3600))),
            channels = listOf(NotificationOverride(channel, NotificationLevel.Inherit, now.minusSeconds(5))),
        )
        assertTrue(s.isMuted(guild, channel, now))

        // Once both lapse, nothing is muted.
        s = settings(
            guildDefault = NotificationLevel.All,
            guilds = listOf(NotificationOverride(guild, NotificationLevel.Inherit, now.minusSeconds(5))),
            channels = listOf(NotificationOverride(channel, NotificationLevel.Inherit, now.minusSeconds(5))),
        )
        assertFalse(s.isMuted(guild, channel, now))
        assertEquals(NotificationLevel.All, s.effectiveLevel(guild, channel))
    }

    @Test
    fun `a dm channel override wins over dms_enabled`() {
        var s = settings(dmsEnabled = false, channels = listOf(NotificationOverride(channel, NotificationLevel.All)))
        assertEquals(NotificationLevel.All, s.effectiveLevel(null, channel))
        assertFalse(s.isMuted(null, channel, now))

        s = settings(dmsEnabled = false, channels = listOf(NotificationOverride(channel, NotificationLevel.All, now.plusSeconds(60))))
        assertTrue(s.isMuted(null, channel, now))
    }
}
