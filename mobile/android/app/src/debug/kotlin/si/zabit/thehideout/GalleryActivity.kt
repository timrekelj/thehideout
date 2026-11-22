package si.zabit.thehideout

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import app.thehideout.ui.gallery.GalleryScreen
import app.thehideout.ui.theme.Hideout
import app.thehideout.ui.theme.HideoutTheme
import app.thehideout.ui.theme.Look
import androidx.compose.runtime.rememberCoroutineScope
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch

/** The debug launcher's "Hideout gallery": the mobile twin of `client/gallery`, with the look persisted like the app's. */
class GalleryActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val looks = (application as HideoutApp).container.looks
        setContent {
            val look by looks.look.collectAsStateWithLifecycle(initialValue = Look())
            val scope = rememberCoroutineScope()
            HideoutTheme(look) {
                SystemBars(Hideout.isDark)
                GalleryScreen(look, { scope.launch { looks.save(it) } })
            }
        }
    }
}
