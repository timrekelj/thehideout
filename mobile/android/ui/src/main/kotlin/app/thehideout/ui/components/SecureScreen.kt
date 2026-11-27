package app.thehideout.ui.components

import android.view.WindowManager
import androidx.activity.compose.LocalActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect

/**
 * Flags the current window `FLAG_SECURE` while composed: screenshots, screen recording and the
 * recent-apps thumbnail are blocked. Used by screens that show a TOTP secret or recovery codes.
 */
@Composable
fun SecureScreenEffect() {
    val activity = LocalActivity.current
    DisposableEffect(activity) {
        activity?.window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onDispose { activity?.window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE) }
    }
}
