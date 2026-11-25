package si.zabit.thehideout

import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.platform.LocalContext
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.LifecycleStartEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.thehideout.core.auth.inviteTokenFromUrl
import app.thehideout.core.store.SessionPhase
import app.thehideout.ui.gallery.GalleryScreen
import app.thehideout.ui.root.HideoutRoot
import app.thehideout.ui.shell.ShellActions
import app.thehideout.ui.theme.Hideout
import app.thehideout.ui.theme.HideoutTheme

/**
 * The single activity. Debug launch flags (intent extras, ignored in release): `fake=true` runs on
 * the in-memory fake world, `gallery=true` opens the design gallery.
 */
class MainActivity : ComponentActivity() {
    private lateinit var viewModel: AppViewModel
    private var mode = false to false

    override fun onCreate(savedInstanceState: Bundle?) {
        // No flash of the sign-in screen while a saved session is checked: the splash stays until the phase leaves Loading.
        val splash = installSplashScreen()
        super.onCreate(savedInstanceState)
        // A relaunch of this activity (rotation, `recreate()`) comes back with the original launch intent, not the
        // one `onNewIntent` saw, so the debug flags of the latest launch are kept in the process.
        if (savedInstanceState == null) debugMode = isFake(intent) to isGallery(intent)
        val (fake, gallery) = debugMode
        mode = debugMode
        val container = (application as HideoutApp).container
        // One ViewModel per mode, so a debug relaunch with fake=true never reuses the real session.
        viewModel = ViewModelProvider(this, AppViewModel.Factory(container, fake))[if (fake) "fake" else "real", AppViewModel::class.java]
        handleLink(intent)

        setContent {
            val session by viewModel.session.collectAsStateWithLifecycle()
            val look by viewModel.look.collectAsStateWithLifecycle()
            val serverUrl by viewModel.serverUrl.collectAsStateWithLifecycle()
            HideoutTheme(look) {
                SystemBars(Hideout.isDark)
                if (gallery) {
                    GalleryScreen(look, viewModel::setLook)
                } else if (session == null) {
                    // The session is still being opened (the real client awaits the persisted
                    // server URL before it starts): keep the splash up rather than flash this
                    // frame's empty content, which `setKeepOnScreenCondition` below would never
                    // run for since it is reached only once a session exists.
                    splash.setKeepOnScreenCondition { true }
                } else {
                    session?.let { s ->
                        val state by s.store.state.collectAsStateWithLifecycle()
                        val authState by s.auth.state.collectAsStateWithLifecycle()
                        splash.setKeepOnScreenCondition { state.phase == SessionPhase.Loading }
                        // No socket in the background: the gateway runs while the activity is started.
                        LifecycleStartEffect(s) {
                            s.store.startGateway()
                            onStopOrDispose { s.store.stopGateway() }
                        }
                        // A `thehideout://invite/<token>` or App Link that opened the app lands on the sign-in form.
                        LaunchedEffect(state.phase, viewModel.pendingInvite) {
                            val token = viewModel.pendingInvite
                            if (state.phase is SessionPhase.SignedOut && token != null) {
                                s.auth.openLink("thehideout://invite/$token")
                                viewModel.pendingInvite = null
                            }
                        }
                        HideoutRoot(
                            state = state,
                            look = look,
                            onLookChange = viewModel::setLook,
                            actions = ShellActions.of(s.store, useDemo = if (BuildConfig.DEBUG && !fake) ::openDemo else null),
                            authState = authState,
                            auth = s.auth,
                            serverUrl = serverUrl,
                            onServerChange = viewModel::changeServer,
                            onProbeServer = container::probeServer,
                            onOpenUrl = ::openUrl,
                            versionName = BuildConfig.VERSION_NAME,
                            // The fake backend has no server: its attachments are bundled assets (ATT-01..05).
                            mediaBase = if (fake) "file:///android_asset/fake_media" else serverUrl,
                        )
                    }
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // singleTask delivers a new launch here; a debug relaunch with other flags starts over in that mode.
        val next = isFake(intent) to isGallery(intent)
        if (next != mode) {
            debugMode = next
            recreate()
            return
        }
        handleLink(intent)
    }

    private fun isFake(intent: Intent) = BuildConfig.DEBUG && intent.getBooleanExtra(EXTRA_FAKE, false)
    private fun isGallery(intent: Intent) = BuildConfig.DEBUG && intent.getBooleanExtra(EXTRA_GALLERY, false)

    private fun handleLink(intent: Intent) {
        invitePath(intent.data)?.let { viewModel.pendingInvite = it }
    }

    /** Opens a server page (terms, privacy, the ceremony) in a Custom Tab. */
    private fun openUrl(url: String) {
        runCatching { CustomTabsIntent.Builder().build().launchUrl(this, Uri.parse(url)) }
    }

    private fun openDemo() {
        startActivity(Intent(this, MainActivity::class.java).putExtra(EXTRA_FAKE, true))
        finish()
    }

    companion object {
        @Volatile private var debugMode = false to false

        const val EXTRA_FAKE = "fake"
        const val EXTRA_GALLERY = "gallery"

        /** The invite token of a `thehideout://invite/<token>` or `https://<server>/invite/<token>` link. */
        fun invitePath(uri: Uri?): String? {
            val u = uri ?: return null
            if (u.scheme != "thehideout" && u.scheme != "https") return null
            return inviteTokenFromUrl(u.toString())
        }
    }
}

/** Edge-to-edge with transparent bars; their icons follow the look's appearance, not the system's. */
@Composable
internal fun SystemBars(dark: Boolean) {
    val activity = LocalContext.current as ComponentActivity
    DisposableEffect(dark) {
        val style = SystemBarStyle.auto(Color.TRANSPARENT, Color.TRANSPARENT) { dark }
        activity.enableEdgeToEdge(statusBarStyle = style, navigationBarStyle = style)
        onDispose {}
    }
}
