package app.thehideout.ui.shell

import app.thehideout.core.model.ReactionEmoji
import app.thehideout.core.store.AppStore

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
    /** Switches tabs (set by [SignedInShell]; plain UI actions, not store intents). */
    val navigateToDms: () -> Unit = {},
    val navigateToServers: () -> Unit = {},
    val signOut: () -> Unit = {},
    /** The terms gate: accept the version the server asked for. */
    val acceptTerms: (String) -> Unit = {},
    val dismissNotice: (Long) -> Unit = {},
    /** Signed-out screen: continue with the built-in demo data (debug builds). */
    val useDemo: (() -> Unit)? = null,
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
            signOut = store::signOut,
            acceptTerms = store::acceptTerms,
            dismissNotice = store::dismissNotice,
            useDemo = useDemo,
        )
    }
}
