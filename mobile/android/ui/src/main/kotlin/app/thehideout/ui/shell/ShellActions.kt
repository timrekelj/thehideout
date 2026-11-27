package app.thehideout.ui.shell

import app.thehideout.core.api.AppError
import app.thehideout.core.model.Ban
import app.thehideout.core.model.Channel
import app.thehideout.core.model.ChannelKind
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.Guild
import app.thehideout.core.model.Invite
import app.thehideout.core.model.Member
import app.thehideout.core.model.ReactionEmoji
import app.thehideout.core.model.Role
import app.thehideout.core.model.Section
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
        )
    }
}
