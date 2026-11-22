package app.thehideout.ui.root

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.thehideout.core.store.AppState
import app.thehideout.core.store.SessionPhase
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.shell.ShellActions
import app.thehideout.ui.shell.SignedInShell
import app.thehideout.ui.theme.Hideout
import app.thehideout.ui.theme.Look
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/** The root view: switches on the session phase and shows the store's notices as snackbars. */
@Composable
fun HideoutRoot(
    state: AppState,
    look: Look,
    onLookChange: (Look) -> Unit,
    actions: ShellActions,
    versionName: String,
    modifier: Modifier = Modifier,
) {
    val c = Hideout.colors
    val snackbar = remember { SnackbarHostState() }
    val notice = state.notices.firstOrNull()
    LaunchedEffect(notice?.id) {
        if (notice != null) {
            snackbar.showSnackbar(notice.text, duration = SnackbarDuration.Short)
            actions.dismissNotice(notice.id)
        }
    }
    Scaffold(
        modifier = modifier,
        containerColor = c.bg,
        contentWindowInsets = androidx.compose.foundation.layout.WindowInsets(0),
        snackbarHost = { SnackbarHost(snackbar) },
    ) { _ ->
        when (val phase = state.phase) {
            SessionPhase.Loading -> Box(Modifier.fillMaxSize().background(c.bg), contentAlignment = Alignment.Center) { LoadingIndicator() }
            is SessionPhase.SignedOut -> SignedOutScreen(actions)
            SessionPhase.SignedIn -> SignedInShell(state, look, onLookChange, actions, versionName)
            is SessionPhase.TermsRequired -> GateScreen(Sym.Lock, "Terms updated", "Read and accept the new terms to keep using thehideout.", actions.signOut, "Sign out")
            SessionPhase.UpdateRequired -> GateScreen(Sym.Stack, "Update required", "This version of thehideout is too old for the server. Update the app to continue.", null, null)
            is SessionPhase.Suspended -> GateScreen(
                Sym.Lock,
                "Account suspended",
                buildString {
                    append(if (phase.reason.isBlank()) "Your account is suspended." else "Your account is suspended: ${phase.reason}.")
                    phase.until?.let { append(" Until ").append(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).format(it.atZone(ZoneId.systemDefault()))).append('.') }
                },
                actions.signOut, "Sign out",
            )
        }
    }
}

/** Placeholder until phase 2 brings the real sign-in: the wordmark, a line, and a way into the demo data on debug builds. */
@Composable
fun SignedOutScreen(actions: ShellActions, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    Box(modifier.fillMaxSize().background(c.bg).safeDrawingPadding().padding(24.dp)) {
        Column(Modifier.align(Alignment.Center).fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("thehideout", style = MaterialTheme.typography.displayMedium, color = c.accentText, fontWeight = FontWeight(700))
            Text("A warm place for your people to talk.", style = MaterialTheme.typography.bodyLarge, color = c.textMuted, textAlign = TextAlign.Center)
            Box(Modifier.padding(top = 16.dp))
            Button(onClick = {}, enabled = false, modifier = Modifier.fillMaxWidth(), shapes = ButtonDefaults.shapes()) { Text("Sign in") }
            Text("Sign in arrives with the next phase.", style = MaterialTheme.typography.bodySmall, color = c.textFaint)
            actions.useDemo?.let { demo ->
                OutlinedButton(onClick = demo, modifier = Modifier.fillMaxWidth(), shapes = ButtonDefaults.shapes()) { Text("Explore with demo data") }
            }
        }
    }
}

/** A full-screen stop: terms to accept, an update to install, an account on hold. */
@Composable
fun GateScreen(sym: Sym, title: String, body: String, onAction: (() -> Unit)?, actionLabel: String?, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    Box(modifier.fillMaxSize().background(c.bg).safeDrawingPadding().padding(24.dp)) {
        Column(Modifier.align(Alignment.Center), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SymIcon(sym, null, size = 48.dp, tint = c.accentText)
            Text(title, style = MaterialTheme.typography.headlineMedium, color = c.text, textAlign = TextAlign.Center)
            Text(body, style = MaterialTheme.typography.bodyLarge, color = c.textMuted, textAlign = TextAlign.Center)
            if (onAction != null && actionLabel != null) OutlinedButton(onClick = onAction, shapes = ButtonDefaults.shapes()) { Text(actionLabel) }
        }
    }
}
