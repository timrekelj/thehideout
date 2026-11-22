package app.thehideout.ui.shell

import app.thehideout.core.store.AppStore

/** What the screens can ask for. A plain bag of lambdas so screens are previewable and testable without a store. */
data class ShellActions(
    val openHome: () -> Unit = {},
    val openGuild: (Long) -> Unit = {},
    val openChannel: (Long) -> Unit = {},
    val loadOlder: (Long) -> Unit = {},
    val send: (String) -> Unit = {},
    val retry: (String) -> Unit = {},
    val signOut: () -> Unit = {},
    val dismissNotice: (Long) -> Unit = {},
    /** Signed-out screen: continue with the built-in demo data (debug builds). */
    val useDemo: (() -> Unit)? = null,
) {
    companion object {
        fun of(store: AppStore, useDemo: (() -> Unit)? = null) = ShellActions(
            openHome = store::openHome,
            openGuild = store::openGuild,
            openChannel = store::openChannel,
            loadOlder = store::loadOlder,
            send = store::send,
            retry = store::retry,
            signOut = store::signOut,
            dismissNotice = store::dismissNotice,
            useDemo = useDemo,
        )
    }
}
