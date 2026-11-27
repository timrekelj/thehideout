package app.thehideout.ui.shell

import app.thehideout.core.api.AppError
import app.thehideout.core.auth.PasskeyCeremony
import app.thehideout.core.auth.PasskeyRegistered
import app.thehideout.core.model.Ban
import app.thehideout.core.model.Channel
import app.thehideout.core.model.ChannelKind
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.Guild
import app.thehideout.core.model.Invite
import app.thehideout.core.model.Member
import app.thehideout.core.model.NotificationLevel
import app.thehideout.core.model.NotificationOverride
import app.thehideout.core.model.NotificationSettings
import app.thehideout.core.model.ReactionEmoji
import app.thehideout.core.model.Role
import app.thehideout.core.model.Section
import app.thehideout.core.model.User
import app.thehideout.core.security.DeviceSession
import app.thehideout.core.security.Passkey
import app.thehideout.core.security.SecurityInfo
import app.thehideout.core.security.TotpSetup
import app.thehideout.core.store.AppStore
import app.thehideout.core.store.PickedFile

private val unavailable = Result.failure<Nothing>(AppError.Other("unavailable"))

/** What the screens can ask for. A plain bag of lambdas so screens are previewable and testable without a store. */
data class ShellActions(
    val openHome: () -> Unit = {},
    val openGuild: (Long) -> Unit = {},
    val openChannel: (Long) -> Unit = {},
    val startDm: (Long) -> Unit = {},
    val loadOlder: (Long) -> Unit = {},
    val send: (String) -> Unit = {},
    val retry: (String) -> Unit = {},
    val react: (Long, Long, ReactionEmoji) -> Unit = { _, _, _ -> },
    val editMessage: (Long, Long, String) -> Unit = { _, _, _ -> },
    val deleteMessage: (Long, Long) -> Unit = { _, _ -> },
    val setDraft: (Long, String) -> Unit = { _, _ -> },
    /** An embed card's header was tapped: its fold flips (ATT-07). */
    val toggleEmbedCollapsed: (Long, Int) -> Unit = { _, _ -> },
    /** The composer's "+": files just picked, camera-captured, pasted or shared in (ATT-09). */
    val attachFiles: (Long, List<PickedFile>) -> Unit = { _, _ -> },
    val removeAttachment: (Long) -> Unit = {},
    val retryAttachment: (Long) -> Unit = {},
    val setChannelForeground: (Boolean) -> Unit = {},
    /** Switches tabs (set by [SignedInShell]; plain UI actions, not store intents). */
    val navigateToDms: () -> Unit = {},
    val navigateToServers: () -> Unit = {},
    val signOut: () -> Unit = {},
    /** The terms gate: accept the version the server asked for. */
    val acceptTerms: (String) -> Unit = {},
    val dismissNotice: (Long) -> Unit = {},
    /** Signed-out screen: continue with the built-in demo data (debug builds). */
    val useDemo: (() -> Unit)? = null,

    // ---- management: create/join servers, channels, sections, invites (phase 4a) ----
    val createGuild: suspend (String) -> Result<Guild> = { unavailable },
    val joinGuild: suspend (String) -> Result<Guild> = { unavailable },
    val createChannel: suspend (Long, String, ChannelKind, Long?) -> Result<Channel> = { _, _, _, _ -> unavailable },
    val createSection: suspend (Long, String) -> Result<Section> = { _, _ -> unavailable },
    val renameSection: suspend (Long, Long, String) -> Result<Section> = { _, _, _ -> unavailable },
    val deleteSection: suspend (Long, Long) -> Result<Unit> = { _, _ -> unavailable },
    val rotateInviteCode: suspend (Long) -> Result<String> = { unavailable },
    val createInvite: suspend (String, Long) -> Result<Invite> = { _, _ -> unavailable },
    val listInvites: suspend (Long) -> Result<List<Invite>> = { unavailable },
    val revokeInvite: suspend (Long) -> Result<Invite> = { unavailable },
    val resendInvite: suspend (Long) -> Result<Invite> = { unavailable },

    // ---- management: server settings, roles, moderation, emojis (phase 4b-1) ----
    val renameGuild: suspend (Long, String) -> Result<Guild> = { _, _ -> unavailable },
    val uploadGuildIcon: suspend (Long, ByteArray, String) -> Result<Guild> = { _, _, _ -> unavailable },
    val removeGuildIcon: suspend (Long) -> Result<Guild> = { unavailable },
    val leaveGuild: suspend (Long) -> Result<Unit> = { unavailable },
    val deleteGuild: suspend (Long, String) -> Result<Unit> = { _, _ -> unavailable },
    val transferOwnership: suspend (Long, Long, String) -> Result<Guild> = { _, _, _ -> unavailable },
    val kickMember: suspend (Long, Long) -> Result<Unit> = { _, _ -> unavailable },
    val banMember: suspend (Long, Long, String, Int?) -> Result<Unit> = { _, _, _, _ -> unavailable },
    val unbanMember: suspend (Long, Long) -> Result<Unit> = { _, _ -> unavailable },
    val loadBans: suspend (Long) -> Result<List<Ban>> = { unavailable },
    val createRole: suspend (Long, String, String, Long) -> Result<Role> = { _, _, _, _ -> unavailable },
    val updateRole: suspend (Long, Long, String?, String?, Long?) -> Result<Role> = { _, _, _, _, _ -> unavailable },
    val deleteRole: suspend (Long, Long) -> Result<Unit> = { _, _ -> unavailable },
    val moveRole: suspend (Long, Long, Int) -> Result<Unit> = { _, _, _ -> unavailable },
    val setMemberRole: suspend (Long, Long, Long, Boolean) -> Result<Member> = { _, _, _, _ -> unavailable },
    val createEmoji: suspend (Long, String, ByteArray, String) -> Result<Emoji> = { _, _, _, _ -> unavailable },
    val renameEmoji: suspend (Long, Long, String) -> Result<Emoji> = { _, _, _ -> unavailable },
    val deleteEmoji: suspend (Long, Long) -> Result<Unit> = { _, _ -> unavailable },

    // ---- account security: email, password, two-factor, passkeys, devices (phase 5) ----
    val getSecurity: suspend () -> Result<SecurityInfo> = { unavailable },
    val changePassword: suspend (String, String) -> Result<Unit> = { _, _ -> unavailable },
    val changeEmail: suspend (String, String) -> Result<Unit> = { _, _ -> unavailable },
    val confirmEmailChange: suspend (String) -> Result<User> = { unavailable },
    val beginTotpSetup: suspend () -> Result<TotpSetup> = { unavailable },
    val confirmTotp: suspend (String) -> Result<List<String>> = { unavailable },
    val disableTotp: suspend (String) -> Result<Unit> = { unavailable },
    val regenerateRecoveryCodes: suspend (String) -> Result<List<String>> = { unavailable },
    val listPasskeys: suspend () -> Result<List<Passkey>> = { unavailable },
    val renamePasskey: suspend (Long, String) -> Result<Passkey> = { _, _ -> unavailable },
    val deletePasskey: suspend (Long) -> Result<Unit> = { unavailable },
    val beginPasskeyRegistration: suspend (String) -> Result<PasskeyCeremony> = { unavailable },
    val finishPasskeyRegistration: suspend (String, String) -> Result<PasskeyRegistered> = { _, _ -> unavailable },
    val listSessions: suspend () -> Result<List<DeviceSession>> = { unavailable },
    val revokeSession: suspend (Long) -> Result<Unit> = { unavailable },
    val revokeOtherSessions: suspend () -> Result<Int> = { unavailable },

    // ---- account: export, deletion (phase 5) ----
    val requestAccountDeletionCode: suspend () -> Result<Unit> = { unavailable },
    val deleteAccount: suspend (String?, String?, String?) -> Result<Unit> = { _, _, _ -> unavailable },
    val exportData: suspend () -> Result<String> = { unavailable },

    // ---- notification settings (MOB-02) ----
    val loadNotificationSettings: suspend () -> Result<NotificationSettings> = { unavailable },
    val updateNotificationSettings: suspend (Boolean?, NotificationLevel?, Boolean?, List<NotificationOverride>, List<NotificationOverride>) -> Result<NotificationSettings> =
        { _, _, _, _, _ -> unavailable },
) {
    companion object {
        fun of(store: AppStore, useDemo: (() -> Unit)? = null) = ShellActions(
            openHome = store::openHome,
            openGuild = store::openGuild,
            openChannel = store::openChannel,
            startDm = store::startDm,
            loadOlder = store::loadOlder,
            send = store::send,
            retry = store::retry,
            react = store::react,
            editMessage = store::editMessage,
            deleteMessage = store::deleteMessage,
            setDraft = store::setDraft,
            toggleEmbedCollapsed = store::toggleEmbedCollapsed,
            attachFiles = store::attachFiles,
            removeAttachment = store::removeAttachment,
            retryAttachment = store::retryAttachment,
            setChannelForeground = store::setChannelForeground,
            signOut = store::signOut,
            acceptTerms = store::acceptTerms,
            dismissNotice = store::dismissNotice,
            useDemo = useDemo,
            createGuild = store::createGuild,
            joinGuild = store::joinGuild,
            createChannel = store::createChannel,
            createSection = store::createSection,
            renameSection = store::renameSection,
            deleteSection = store::deleteSection,
            rotateInviteCode = store::rotateInviteCode,
            createInvite = store::createInvite,
            listInvites = store::listInvites,
            revokeInvite = store::revokeInvite,
            resendInvite = store::resendInvite,
            renameGuild = store::renameGuild,
            uploadGuildIcon = store::uploadGuildIcon,
            removeGuildIcon = store::removeGuildIcon,
            leaveGuild = store::leaveGuild,
            deleteGuild = store::deleteGuild,
            transferOwnership = store::transferOwnership,
            kickMember = store::kickMember,
            banMember = store::banMember,
            unbanMember = store::unbanMember,
            loadBans = store::loadBans,
            createRole = store::createRole,
            updateRole = store::updateRole,
            deleteRole = store::deleteRole,
            moveRole = store::moveRole,
            setMemberRole = store::setMemberRole,
            createEmoji = store::createEmoji,
            renameEmoji = store::renameEmoji,
            deleteEmoji = store::deleteEmoji,
            getSecurity = store::getSecurity,
            changePassword = store::changePassword,
            changeEmail = store::changeEmail,
            confirmEmailChange = store::confirmEmailChange,
            beginTotpSetup = store::beginTotpSetup,
            confirmTotp = store::confirmTotp,
            disableTotp = store::disableTotp,
            regenerateRecoveryCodes = store::regenerateRecoveryCodes,
            listPasskeys = store::listPasskeys,
            renamePasskey = store::renamePasskey,
            deletePasskey = store::deletePasskey,
            beginPasskeyRegistration = store::beginPasskeyRegistration,
            finishPasskeyRegistration = store::finishPasskeyRegistration,
            listSessions = store::listSessions,
            revokeSession = store::revokeSession,
            revokeOtherSessions = store::revokeOtherSessions,
            requestAccountDeletionCode = store::requestAccountDeletionCode,
            deleteAccount = store::deleteAccount,
            exportData = store::exportData,
            loadNotificationSettings = store::loadNotificationSettings,
            updateNotificationSettings = store::updateNotificationSettings,
        )
    }
}
