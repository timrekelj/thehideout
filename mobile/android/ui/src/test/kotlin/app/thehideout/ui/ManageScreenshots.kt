package app.thehideout.ui

import app.thehideout.core.model.Channel
import app.thehideout.core.model.ChannelKind
import app.thehideout.core.model.Guild
import app.thehideout.core.model.Invite
import app.thehideout.core.model.InviteStatus
import app.thehideout.core.model.Section
import app.thehideout.ui.shell.CreateChannelSheet
import app.thehideout.ui.shell.CreateServerSheet
import app.thehideout.ui.shell.InviteSheet
import app.thehideout.ui.shell.JoinServerSheet
import org.junit.Test
import org.robolectric.annotation.Config
import java.time.Instant

/** Fixtures for the create/join/invite sheets (phase 4a), one desktop scene each (`dialog-create-server`, `dialog-create-channel`, `invite-by-email`). */
private object ManageStates {
    val guild = Guild(1, "The Hideout", 1, "HIDEOUT1", Instant.parse("2026-01-06T09:00:00Z"))
    val sections = listOf(Section(101, 1, "Building", 0), Section(102, 1, "Hangout", 1))
    val pendingInvite = Invite(
        id = 1, email = "erin@example.com", code = null, status = InviteStatus.Pending, inviter = null,
        guildIds = listOf(1), createdAt = Instant.parse("2026-11-20T10:00:00Z"), expiresAt = Instant.parse("2026-11-28T10:00:00Z"),
        makeAdmin = false, link = null,
    )

    suspend fun createGuild(name: String) = Result.success(guild.copy(name = name))
    suspend fun joinGuild(code: String) = Result.success(guild)
    suspend fun createChannel(guild: Long, name: String, kind: ChannelKind, section: Long?) =
        Result.success(Channel(900, guild, name, Instant.parse("2026-11-22T10:00:00Z"), sectionId = section, kind = kind))
    suspend fun listInvites(guild: Long) = Result.success(listOf(pendingInvite))
    suspend fun createInvite(email: String, guild: Long) = Result.success(pendingInvite.copy(email = email, code = "N0X6-PFNB", link = "https://thehideout.zabit.si/invite/N0X6-PFNB"))
    suspend fun rotateInviteCode(guild: Long) = Result.success("ZZZZ-9999")
    suspend fun resendInvite(id: Long) = Result.success(pendingInvite)
    suspend fun revokeInvite(id: Long) = Result.success(pendingInvite.copy(status = InviteStatus.Revoked))
}

/** Create server, join server, create channel and invite sheets (DLG-01..04, SIDE-10), phone and tablet, dark and light. */
abstract class ManageScreenshots(private val device: Device) : ScreenshotTest() {
    @Test fun createServerDark() = compose.snapshot("dialog-create-server", device, Variant.Dark) { CreateServerSheet({}, ManageStates::createGuild) }
    @Test fun createServerLight() = compose.snapshot("dialog-create-server", device, Variant.Light) { CreateServerSheet({}, ManageStates::createGuild) }

    @Test fun joinServerDark() = compose.snapshot("dialog-join-server", device, Variant.Dark) { JoinServerSheet({}, ManageStates::joinGuild) }
    @Test fun joinServerLight() = compose.snapshot("dialog-join-server", device, Variant.Light) { JoinServerSheet({}, ManageStates::joinGuild) }

    @Test fun createChannelDark() = compose.snapshot("dialog-create-channel", device, Variant.Dark) { CreateChannelSheet(1, ManageStates.sections, {}, ManageStates::createChannel) }
    @Test fun createChannelLight() = compose.snapshot("dialog-create-channel", device, Variant.Light) { CreateChannelSheet(1, ManageStates.sections, {}, ManageStates::createChannel) }

    @Test fun inviteDark() = compose.snapshot("invite-by-email", device, Variant.Dark) {
        InviteSheet(ManageStates.guild, true, true, ManageStates::rotateInviteCode, ManageStates::createInvite, ManageStates::listInvites, ManageStates::revokeInvite, ManageStates::resendInvite, {})
    }
    @Test fun inviteLight() = compose.snapshot("invite-by-email", device, Variant.Light) {
        InviteSheet(ManageStates.guild, true, true, ManageStates::rotateInviteCode, ManageStates::createInvite, ManageStates::listInvites, ManageStates::revokeInvite, ManageStates::resendInvite, {})
    }
}

@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class ManagePhoneScreenshots : ManageScreenshots(Device.Phone)

@Config(sdk = [35], qualifiers = "w1280dp-h800dp-xhdpi")
class ManageTabletScreenshots : ManageScreenshots(Device.Tablet)
