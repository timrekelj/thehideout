package si.zabit.thehideout

import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.LifecycleStartEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
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
            val state by viewModel.store.state.collectAsStateWithLifecycle()
            val look by viewModel.look.collectAsStateWithLifecycle()
            HideoutTheme(look) {
                SystemBars(Hideout.isDark)
                if (gallery) {
                    GalleryScreen(look, viewModel::setLook)
                } else {
                    // No socket in the background: the gateway runs while the activity is started.
                    LifecycleStartEffect(viewModel) {
                        viewModel.store.startGateway()
                        onStopOrDispose { viewModel.store.stopGateway() }
                    }
                    HideoutRoot(
                        state = state,
                        look = look,
                        onLookChange = viewModel::setLook,
                        actions = ShellActions.of(viewModel.store, useDemo = if (BuildConfig.DEBUG && !fake) ::openDemo else null),
                        versionName = BuildConfig.VERSION_NAME,
                    )
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

    private fun openDemo() {
        startActivity(Intent(this, MainActivity::class.java).putExtra(EXTRA_FAKE, true))
        finish()
    }

    companion object {
        @Volatile private var debugMode = false to false

        const val EXTRA_FAKE = "fake"
        const val EXTRA_GALLERY = "gallery"

        /** `thehideout://invite/<token>` -> `<token>`. */
        fun invitePath(uri: Uri?): String? =
            uri?.takeIf { it.scheme == "thehideout" && it.host == "invite" }?.lastPathSegment?.takeIf { it.isNotBlank() }
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
