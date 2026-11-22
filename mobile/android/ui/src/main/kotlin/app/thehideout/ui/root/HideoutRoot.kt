package app.thehideout.ui.root

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.thehideout.core.auth.AuthPresenter
import app.thehideout.core.auth.AuthUiState
import app.thehideout.core.store.AppState
import app.thehideout.core.store.SessionPhase
import app.thehideout.ui.auth.AuthScreen
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.shell.ShellActions
import app.thehideout.ui.shell.SignedInShell
import app.thehideout.ui.theme.Hideout
import app.thehideout.ui.theme.Look
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/**
 * The root view: switches on the session phase — the auth screens while signed out, the shell while
 * signed in, and a full-screen gate for terms, an update or a suspension — and shows the store's
 * notices as snackbars.
 */
@Composable
fun HideoutRoot(
    state: AppState,
    look: Look,
    onLookChange: (Look) -> Unit,
    actions: ShellActions,
    authState: AuthUiState,
    auth: AuthPresenter,
    serverUrl: String,
    onServerChange: (String) -> Unit,
    onProbeServer: suspend (String) -> Boolean,
    onOpenUrl: (String) -> Unit,
    versionName: String,
    modifier: Modifier = Modifier,
    /** Where this session's attachments load from: the server, or the fake backend's bundled assets. */
    mediaBase: String = serverUrl,
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
            is SessionPhase.SignedOut -> AuthScreen(
                state = authState,
                auth = auth,
                serverUrl = serverUrl,
                onServerChange = onServerChange,
                onProbeServer = onProbeServer,
                onOpenUrl = onOpenUrl,
                useDemo = actions.useDemo,
            )
            SessionPhase.SignedIn -> androidx.compose.runtime.CompositionLocalProvider(app.thehideout.ui.components.LocalMediaBase provides mediaBase) {
                SignedInShell(state, look, onLookChange, actions, versionName)
            }
            is SessionPhase.TermsRequired -> TermsGate(
                version = phase.version,
                serverUrl = serverUrl,
                onAccept = actions.acceptTerms,
                onSignOut = actions.signOut,
                onOpenUrl = onOpenUrl,
            )
            SessionPhase.UpdateRequired -> GateScreen(
                Sym.Stack,
                "Update required",
                "This version of thehideout is too old for the server. Update the app to continue.",
                { onOpenUrl(serverUrl) },
                "Update",
            )
            is SessionPhase.Suspended -> GateScreen(
                Sym.Lock,
                "Account suspended",
                buildString {
                    append(if (phase.reason.isBlank()) "Your account is suspended." else "Your account is suspended: ${phase.reason}.")
                    phase.until?.let {
                        append(" Until ")
                            .append(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).format(it.atZone(ZoneId.systemDefault())))
                            .append('.')
                    }
                    append(" If you think this is a mistake, contact the server admin.")
                },
                actions.signOut,
                "Sign out",
            )
        }
    }
}

/**
 * The blocking terms card (desktop's `terms_screen`): nothing of the app is reachable until the new
 * terms are accepted.
 */
@Composable
fun TermsGate(
    version: String,
    serverUrl: String,
    onAccept: (String) -> Unit,
    onSignOut: () -> Unit,
    onOpenUrl: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val c = Hideout.colors
    Box(modifier.fillMaxSize().background(c.bg).safeDrawingPadding().padding(24.dp)) {
        Column(
            Modifier.align(Alignment.Center).widthIn(max = 400.dp).fillMaxWidth().verticalScroll(rememberScrollState()),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
          app.thehideout.ui.components.HideoutCard(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("thehideout", style = MaterialTheme.typography.displayMedium, color = c.accentText, fontWeight = androidx.compose.ui.text.font.FontWeight(700))
            Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("We updated our Terms and Privacy Policy", style = MaterialTheme.typography.titleMedium, color = c.text)
                Text(
                    "Please read them, then accept to keep using thehideout. Nothing else changes: your servers and messages are as you left them.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = c.textMuted,
                )
            }
            androidx.compose.foundation.layout.Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                TextButton(onClick = { onOpenUrl("$serverUrl/terms") }) { Text("Read the Terms", color = c.accentText) }
                TextButton(onClick = { onOpenUrl("$serverUrl/privacy") }) { Text("Read the Privacy Policy", color = c.accentText) }
            }
            Button(onClick = { onAccept(version) }, modifier = Modifier.fillMaxWidth(), shapes = ButtonDefaults.shapes()) { Text("Accept and continue") }
            TextButton(onClick = onSignOut) { Text("Sign out", color = c.textMuted) }
            }
          }
        }
    }
}

/** A full-screen stop: an update to install, an account on hold. */
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

@Composable
private fun TextButton(onClick: () -> Unit, content: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit) {
    androidx.compose.material3.TextButton(onClick = onClick, content = content)
}
