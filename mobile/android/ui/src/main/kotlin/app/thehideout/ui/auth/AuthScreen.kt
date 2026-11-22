package app.thehideout.ui.auth

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.thehideout.core.auth.AuthField
import app.thehideout.core.auth.AuthMode
import app.thehideout.core.auth.AuthPresenter
import app.thehideout.core.auth.AuthStep
import app.thehideout.core.auth.AuthUiState
import app.thehideout.core.auth.InvitePreview
import app.thehideout.core.auth.SecondFactorMethod
import app.thehideout.ui.components.HideoutCard
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import app.thehideout.ui.theme.Look
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/**
 * The auth screen: a centred card (400 dp like desktop) with the wordmark, the step's fields, one
 * primary action and quiet links to the other ways in; the server line sits small under the card.
 * Errors land under the field they belong to, a form-wide one above the button, and neither says
 * whether an account exists.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AuthScreen(
    state: AuthUiState,
    auth: AuthPresenter,
    serverUrl: String,
    onServerChange: (String) -> Unit,
    onProbeServer: suspend (String) -> Boolean,
    onOpenUrl: (String) -> Unit,
    useDemo: (() -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val c = Hideout.colors
    val scope = rememberCoroutineScope()
    val haptic = LocalHapticFeedback.current
    // Predictive back between the steps.
    val canGoBack = when (state.step) {
        is AuthStep.EmailCode, is AuthStep.Reset, is AuthStep.Passkey, is AuthStep.SecondFactor, is AuthStep.InviteAccept -> true
        else -> false
    }
    BackHandler(enabled = canGoBack && !state.busy) { auth.back() }

    // The passkey ceremony under way: run it (Credential Manager, or the ceremony page in a Custom Tab).
    val context = LocalContext.current
    val launcher = remember { PasskeyLauncher(context) }
    LaunchedEffect(state.ceremony) {
        val ceremony = state.ceremony ?: return@LaunchedEffect
        val secondFactor = state.step is AuthStep.SecondFactor
        val finish: (String) -> Unit = { json ->
            if (secondFactor) auth.finishSecondFactorPasskey(ceremony.ceremonyId, json) else auth.finishPasskey(ceremony.ceremonyId, json)
        }
        // Native passkeys need the RP id to be a domain with the app's assetlinks.json; otherwise the ceremony page.
        val native = if (PasskeyLauncher.isDomainHost(serverUrl)) {
            runCatching { if (secondFactor) launcher.getCredential(ceremony) else launcher.getCredential(ceremony) }.getOrNull()
        } else null
        if (native != null) {
            finish(native)
        } else {
            launcher.openCeremonyTab(ceremony)
            // Poll the Finish* RPC with an empty credential every 1.5 s (the page's 127.0.0.1 redirect is not a callback).
            var polls = 0
            while (polls < 80 && state.ceremony == ceremony) {
                delay(1500)
                finish("")
                polls++
            }
        }
    }

    Box(modifier.fillMaxSize().background(c.bg).safeDrawingPadding().imePadding()) {
        Column(
            Modifier.align(Alignment.Center).widthIn(max = 400.dp).fillMaxWidth().verticalScroll(rememberScrollState()).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            HideoutCard(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text("thehideout", style = MaterialTheme.typography.headlineMedium, color = c.accentText, fontWeight = FontWeight(700))
                        Text(
                            when (state.mode) {
                                AuthMode.Login -> "Welcome back. Your friends are waiting."
                                AuthMode.Invite -> "Joining? Use the invite someone sent you."
                            },
                            style = MaterialTheme.typography.bodyMedium,
                            color = c.textMuted,
                        )
                    }
                    val suspension = state.error?.suspension
                    if (suspension != null) {
                        SuspendedCard(suspension, onDismiss = auth::dismissSuspension)
                    } else {
                        StepContent(state, auth, haptic, serverUrl, onOpenUrl)
                    }
                    if (state.topLevel) {
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
                            if (state.mode == AuthMode.Login) {
                                Text("Have an invite?", style = MaterialTheme.typography.bodyMedium, color = c.textMuted)
                                AccentLink("Join with a code", { auth.setMode(AuthMode.Invite) })
                            } else {
                                Text("Already have an account?", style = MaterialTheme.typography.bodyMedium, color = c.textMuted)
                                AccentLink("Sign in", { auth.setMode(AuthMode.Login) })
                            }
                        }
                    }
                }
            }
            ServerLine(state, auth, serverUrl, onServerChange, onProbeServer, onOpenUrl)
            useDemo?.let { demo ->
                OutlinedButton(onClick = demo, modifier = Modifier.fillMaxWidth(), shapes = ButtonDefaults.shapes()) { Text("Explore with demo data") }
            }
        }
    }

    if (state.serverOpen) ServerSheet(state, auth, onServerChange, onProbeServer, onClose = { auth.setServerOpen(false) })
}

/** The step's heading, fields, button and links. */
@Composable
private fun StepContent(state: AuthUiState, auth: AuthPresenter, haptic: androidx.compose.ui.hapticfeedback.HapticFeedback, serverUrl: String, onOpenUrl: (String) -> Unit) {
    val c = Hideout.colors
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        when (val step = state.step) {
            AuthStep.Password -> PasswordStep(state, auth, haptic)
            is AuthStep.EmailCode -> EmailCodeStep(state, auth, step.sent, haptic, isReset = false)
            is AuthStep.Reset -> EmailCodeStep(state, auth, step.sent, haptic, isReset = true)
            is AuthStep.Passkey -> PasskeyStep(state, auth, step.waiting, haptic)
            is AuthStep.SecondFactor -> SecondFactorStep(state, auth, step, haptic)
            AuthStep.InviteCode -> InviteCodeStep(state, auth, haptic)
            is AuthStep.InviteAccept -> InviteAcceptStep(state, auth, step, haptic, serverUrl, onOpenUrl)
        }
    }
}

@Composable
private fun PrimaryButton(label: String, busy: Boolean, haptic: androidx.compose.ui.hapticfeedback.HapticFeedback, enabled: Boolean = true, onClick: () -> Unit) {
    Button(
        onClick = { haptic.performHapticFeedback(HapticFeedbackType.ContextClick); onClick() },
        enabled = enabled && !busy,
        modifier = Modifier.fillMaxWidth(),
        shapes = ButtonDefaults.shapes(),
    ) {
        if (busy) {
            CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
            Text(label, modifier = Modifier.padding(start = 8.dp))
        } else {
            Text(label)
        }
    }
}

@Composable
private fun NoticeBox(state: AuthUiState) {
    val c = Hideout.colors
    state.notice?.let {
        Box(Modifier.fillMaxWidth().background(c.infoBg, shape = MaterialTheme.shapes.medium).padding(10.dp)) {
            Text(it, style = MaterialTheme.typography.bodyMedium, color = c.text)
        }
    }
}

@Composable
private fun FormError(state: AuthUiState) {
    state.errorFor(AuthField.Form)?.let {
        Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error, modifier = Modifier.fillMaxWidth())
    }
}

@Composable
private fun Heading(title: String, body: String) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(title, style = MaterialTheme.typography.titleMedium, color = Hideout.colors.text)
        Text(body, style = MaterialTheme.typography.bodyMedium, color = Hideout.colors.textMuted)
    }
}

@Composable
private fun BackLink(label: String, onClick: () -> Unit) {
    QuietLink("‹ $label", onClick)
}

@Composable
private fun PasswordStep(state: AuthUiState, auth: AuthPresenter, haptic: androidx.compose.ui.hapticfeedback.HapticFeedback) {
    NoticeBox(state)
    AuthField(
        "Username or email", state.username, auth::updateUsername,
        error = state.errorFor(AuthField.Username),
        keyboardType = KeyboardType.Email,
        imeAction = ImeAction.Next,
        autofillType = androidx.compose.ui.autofill.ContentType.Username,
        onSubmit = auth::submit,
    )
    AuthField(
        "Password", state.password, auth::updatePassword,
        aside = { QuietLink("Forgot password?", { auth.go(AuthStep.Reset(sent = false)) }) },
        error = state.errorFor(AuthField.Password),
        secure = true,
        imeAction = ImeAction.Done,
        autofillType = androidx.compose.ui.autofill.ContentType.Password,
        onSubmit = auth::submit,
    )
    FormError(state)
    PrimaryButton(if (state.busy) "Signing in…" else "Sign in", state.busy, haptic, onClick = auth::submit)
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
        QuietLink("Email me a code", { auth.go(AuthStep.EmailCode(sent = false)) })
        Text("·", color = Hideout.colors.textFaint, modifier = Modifier.padding(horizontal = 4.dp))
        QuietLink("Use a passkey", { auth.go(AuthStep.Passkey(waiting = false)) })
    }
}

@Composable
private fun EmailCodeStep(state: AuthUiState, auth: AuthPresenter, sent: Boolean, haptic: androidx.compose.ui.hapticfeedback.HapticFeedback, isReset: Boolean) {
    val c = Hideout.colors
    if (!sent) {
        BackLink("Back to sign in", { auth.go(AuthStep.Password) })
        Heading(
            if (isReset) "Reset your password" else "Sign in with a code",
            if (isReset) "We'll email you a code to set a new one." else "We'll email you a 6-digit code. No password needed.",
        )
        AuthField(
            "Email", state.email, auth::updateEmail,
            placeholder = "you@example.com",
            error = state.errorFor(AuthField.Email),
            keyboardType = KeyboardType.Email,
            imeAction = ImeAction.Go,
            autofillType = androidx.compose.ui.autofill.ContentType.EmailAddress,
            onSubmit = auth::submit,
        )
        FormError(state)
        PrimaryButton(if (state.busy) "Sending…" else "Send code", state.busy, haptic, onClick = auth::submit)
    } else {
        BackLink("Use a different email", { auth.go(if (isReset) AuthStep.Reset(sent = false) else AuthStep.EmailCode(sent = false)) })
        Heading("Check your email", "Enter the 6-digit code we sent.")
        NoticeBox(state)
        CodeField("Code", state.code, auth::updateCode, error = state.errorFor(AuthField.Code), enabled = !state.busy, onSubmit = auth::submit)
        if (isReset) {
            AuthField(
                "New password", state.password, auth::updatePassword,
                placeholder = "At least 8 characters",
                error = state.errorFor(AuthField.Password),
                secure = true,
                imeAction = ImeAction.Done,
                autofillType = androidx.compose.ui.autofill.ContentType.NewPassword,
                onSubmit = auth::submit,
            )
        }
        FormError(state)
        PrimaryButton(
            when {
                state.busy && isReset -> "Saving…"
                state.busy -> "Signing in…"
                isReset -> "Set new password"
                else -> "Sign in"
            },
            state.busy, haptic, onClick = auth::submit,
        )
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center) {
            ResendRow(state, auth)
        }
    }
}

@Composable
private fun ResendRow(state: AuthUiState, auth: AuthPresenter) {
    var left by remember(state.resendAt) { mutableIntStateOf(auth.resendSecondsLeft()) }
    LaunchedEffect(state.resendAt) {
        while (true) {
            delay(1000)
            left = auth.resendSecondsLeft()
            if (left == 0) return@LaunchedEffect
        }
    }
    if (left > 0) {
        Text("Resend code in ${left}s", style = MaterialTheme.typography.bodyMedium, color = Hideout.colors.textFaint)
    } else {
        AccentLink("Resend code", { auth.resend() })
    }
}

@Composable
private fun PasskeyStep(state: AuthUiState, auth: AuthPresenter, waiting: Boolean, haptic: androidx.compose.ui.hapticfeedback.HapticFeedback) {
    if (waiting) {
        WaitingCeremony("Finish in your browser", "We opened a page in your browser. Confirm with your passkey there and this window signs you in.", state, auth)
    } else {
        BackLink("Back to sign in", { auth.go(AuthStep.Password) })
        Heading("Sign in with a passkey", "Your browser opens to confirm it with your fingerprint, face or security key.")
        AuthField(
            "Username", state.username, auth::updateUsername,
            error = state.errorFor(AuthField.Username),
            imeAction = ImeAction.Go,
            autofillType = androidx.compose.ui.autofill.ContentType.Username,
            onSubmit = auth::submit,
        )
        FormError(state)
        PrimaryButton(if (state.busy) "Opening your browser…" else "Continue with a passkey", state.busy, haptic, onClick = auth::submit)
    }
}

@Composable
private fun SecondFactorStep(state: AuthUiState, auth: AuthPresenter, step: AuthStep.SecondFactor, haptic: androidx.compose.ui.hapticfeedback.HapticFeedback) {
    val c = Hideout.colors
    if (step.passkeyWaiting) {
        WaitingCeremony("Finish in your browser", "We opened a page in your browser. Confirm with your passkey there to finish signing in.", state, auth)
        return
    }
    BackLink(if (step.locked) "Start again" else "Back to sign in", { auth.go(AuthStep.Password) })
    if (step.locked) {
        Heading("Two-factor authentication", "This sign-in was stopped. Start again to get a new one.")
        FormError(state)
        PrimaryButton("Start again", busy = false, haptic, onClick = { auth.go(AuthStep.Password) })
    } else if (step.recovery) {
        Heading("Use a recovery code", "Enter one of the codes you saved when you set up two-factor authentication. Each works once.")
        AuthField(
            "Recovery code", state.recovery, auth::updateRecovery,
            placeholder = "XXXX-XXXX",
            error = state.errorFor(AuthField.Code),
            imeAction = ImeAction.Done,
            onSubmit = auth::submit,
        )
        FormError(state)
        PrimaryButton(if (state.busy) "Checking…" else "Sign in", state.busy, haptic, onClick = auth::submit)
    } else {
        Heading("Two-factor authentication", "Enter the 6-digit code from your authenticator app.")
        CodeField("Authenticator code", state.code, auth::updateCode, error = state.errorFor(AuthField.Code), enabled = !state.busy, onSubmit = auth::submit)
        FormError(state)
        PrimaryButton(if (state.busy) "Checking…" else "Sign in", state.busy, haptic, onClick = auth::submit)
    }
    if (!step.locked) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
            QuietLink(if (step.recovery) "Use your authenticator app" else "Use a recovery code", auth::toggleRecovery)
            if (SecondFactorMethod.Passkey in step.methods) {
                Text("·", color = c.textFaint, modifier = Modifier.padding(horizontal = 4.dp))
                QuietLink("Use a passkey", { (auth as? app.thehideout.core.auth.AuthFlow)?.startSecondFactorPasskey() })
            }
        }
    }
}

@Composable
private fun WaitingCeremony(title: String, body: String, state: AuthUiState, auth: AuthPresenter) {
    val c = Hideout.colors
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Box(Modifier.size(48.dp).background(c.accentSubtle, shape = MaterialTheme.shapes.large), contentAlignment = Alignment.Center) {
            SymIcon(Sym.Fingerprint, null, size = 28.dp, tint = c.accentText)
        }
        Text(title, style = MaterialTheme.typography.titleMedium, color = c.text)
        Text(body, style = MaterialTheme.typography.bodyMedium, color = c.textMuted, textAlign = TextAlign.Center)
        Text("Waiting…", style = MaterialTheme.typography.bodySmall, color = c.textFaint)
    }
    FormError(state)
    OutlinedButton(onClick = auth::cancelPasskey, modifier = Modifier.fillMaxWidth(), shapes = ButtonDefaults.shapes()) { Text("Cancel") }
}

@Composable
private fun InviteCodeStep(state: AuthUiState, auth: AuthPresenter, haptic: androidx.compose.ui.hapticfeedback.HapticFeedback) {
    val c = Hideout.colors
    AuthField(
        "Invite code", state.invite, auth::updateInvite,
        placeholder = "XXXX-XXXX",
        error = state.errorFor(AuthField.Invite),
        imeAction = ImeAction.Go,
        onSubmit = auth::submit,
    )
    Text("It's in your invite email.", style = MaterialTheme.typography.bodySmall, color = c.textMuted)
    FormError(state)
    PrimaryButton(if (state.busy) "Checking…" else "Continue", state.busy, haptic, onClick = auth::submit)
}

@Composable
private fun InviteAcceptStep(state: AuthUiState, auth: AuthPresenter, step: AuthStep.InviteAccept, haptic: androidx.compose.ui.hapticfeedback.HapticFeedback, serverUrl: String, onOpenUrl: (String) -> Unit) {
    val c = Hideout.colors
    val preview = step.preview
    BackLink("Use a different invite", { auth.go(AuthStep.InviteCode) })
    InviteSummary(preview)
    AuthField(
        "Choose a username", state.username, auth::updateUsername,
        placeholder = "ana",
        error = state.errorFor(AuthField.Username),
        imeAction = ImeAction.Next,
        autofillType = androidx.compose.ui.autofill.ContentType.Username,
        onSubmit = auth::submit,
    )
    AuthField(
        "Choose a password", state.password, auth::updatePassword,
        placeholder = "At least 8 characters",
        error = state.errorFor(AuthField.Password),
        secure = true,
        imeAction = ImeAction.Done,
        autofillType = androidx.compose.ui.autofill.ContentType.NewPassword,
        onSubmit = auth::submit,
    )
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Switch(checked = state.addPasskey, onCheckedChange = { auth.toggleAddPasskey() }, modifier = Modifier.semantics { contentDescription = "Add a passkey after signing up" })
        Text("Add a passkey after signing up", style = MaterialTheme.typography.bodyMedium, color = c.text)
    }
    CheckRow(
        checked = state.acceptTerms,
        onToggle = auth::toggleTerms,
        name = "I agree to the Terms and Privacy Policy",
    ) {
        // The two names are links to the server's pages (LinkAnnotation.Url opens them in the browser).
        val linkStyle = androidx.compose.ui.text.TextLinkStyles(
            style = androidx.compose.ui.text.SpanStyle(color = c.accentText),
        )
        Text(
            buildAnnotatedString {
                append("I agree to the ")
                withLink(androidx.compose.ui.text.LinkAnnotation.Url("$serverUrl/terms", linkStyle)) { append("Terms") }
                append(" and ")
                withLink(androidx.compose.ui.text.LinkAnnotation.Url("$serverUrl/privacy", linkStyle)) { append("Privacy Policy") }
            },
            modifier = Modifier.weight(1f),
            style = MaterialTheme.typography.bodyMedium,
            color = c.text,
        )
    }
    CheckRow(checked = state.confirmAge, onToggle = auth::toggleAge, name = "I'm ${preview.minimumAge} or older") {
        Text("I'm ${preview.minimumAge} or older", style = MaterialTheme.typography.bodyMedium, color = c.text)
    }
    val ready = state.acceptTerms && state.confirmAge
    if (!ready && !state.busy) {
        Text("Tick both boxes to create your account.", style = MaterialTheme.typography.bodySmall, color = c.textMuted)
    }
    FormError(state)
    PrimaryButton(if (state.busy) "Creating account…" else "Create account", state.busy, haptic, enabled = ready, onClick = auth::submit)
}

@Composable
private fun InviteSummary(preview: InvitePreview) {
    val c = Hideout.colors
    val who = preview.inviter?.username ?: "thehideout"
    val places = when (preview.guildNames.size) {
        0 -> "thehideout"
        1 -> preview.guildNames[0]
        else -> preview.guildNames.dropLast(1).joinToString(", ") + " and " + preview.guildNames.last()
    }
    val expires = DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).format(preview.expiresAt.atZone(ZoneId.systemDefault()))
    Column(
        Modifier.fillMaxWidth().background(c.accentSubtle, shape = MaterialTheme.shapes.medium).padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text("$who invited you to join $places", style = MaterialTheme.typography.bodyLarge, color = c.text, fontWeight = FontWeight(650))
        Text("For ${preview.maskedEmail} · expires $expires", style = MaterialTheme.typography.bodySmall, color = c.textMuted)
    }
}

/** A checkbox with its label to the right (the box carries the name for TalkBack). */
@Composable
private fun CheckRow(checked: Boolean, onToggle: () -> Unit, name: String, label: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Checkbox(checked = checked, onCheckedChange = { onToggle() }, modifier = Modifier.semantics { contentDescription = name })
        label()
    }
}

/** The auth card's content when the sign-in was refused because the account is suspended. */
@Composable
private fun SuspendedCard(suspension: app.thehideout.core.auth.Suspension, onDismiss: () -> Unit) {
    val c = Hideout.colors
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("Your account is suspended", style = MaterialTheme.typography.titleMedium, color = c.text)
        if (suspension.reason.isNotBlank()) {
            Column(Modifier.fillMaxWidth().background(c.dangerBg, shape = MaterialTheme.shapes.medium).padding(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text("Reason", style = MaterialTheme.typography.labelSmall, color = c.textMuted)
                Text(suspension.reason, style = MaterialTheme.typography.bodyMedium, color = c.text)
            }
        }
        Text(
            suspension.until?.let {
                "Your access is paused until ${DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).format(it.atZone(ZoneId.systemDefault()))}."
            } ?: "Your access is paused indefinitely, until an admin lifts the suspension.",
            style = MaterialTheme.typography.bodyMedium,
            color = c.text,
        )
        Text("If you think this is a mistake, contact the server admin.", style = MaterialTheme.typography.bodyMedium, color = c.textMuted)
        OutlinedButton(onClick = onDismiss, modifier = Modifier.fillMaxWidth(), shapes = ButtonDefaults.shapes()) { Text("Back to sign in") }
    }
}

/** "Server: … · Change", which opens the server sheet under the card. */
@Composable
private fun ServerLine(
    state: AuthUiState,
    auth: AuthPresenter,
    serverUrl: String,
    onServerChange: (String) -> Unit,
    onProbeServer: suspend (String) -> Boolean,
    onOpenUrl: (String) -> Unit,
) {
    val c = Hideout.colors
    Column(Modifier.widthIn(max = 400.dp).fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Text("Server: $serverUrl", style = MaterialTheme.typography.bodySmall, color = c.textMuted)
            Text("·", color = c.textFaint)
            FaintLink(
                if (state.serverOpen) "Done" else "Change",
                {
                    if (!state.serverOpen) auth.updateServerDraft(serverUrl)
                    auth.setServerOpen(!state.serverOpen)
                },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
            FaintLink("Terms", { onOpenUrl("$serverUrl/terms") })
            Text("·", color = c.textFaint)
            FaintLink("Privacy", { onOpenUrl("$serverUrl/privacy") })
        }
    }
}

/** The sheet that edits the server URL: probed with a `HEAD /` before it is saved. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ServerSheet(
    state: AuthUiState,
    auth: AuthPresenter,
    onServerChange: (String) -> Unit,
    onProbeServer: suspend (String) -> Boolean,
    onClose: () -> Unit,
) {
    val c = Hideout.colors
    val scope = rememberCoroutineScope()
    var probing by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = androidx.compose.material3.rememberBottomSheetState(androidx.compose.material3.SheetValue.Hidden),
    ) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 8.dp).imePadding(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Server URL", style = MaterialTheme.typography.titleMedium, color = c.text)
            OutlinedTextField(
                value = state.serverDraft,
                onValueChange = { auth.updateServerDraft(it); problem = null },
                label = { Text("Server URL") },
                placeholder = { Text("http://10.0.2.2:8080") },
                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Done),
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
                isError = problem != null,
                supportingText = problem?.let { { Text(it, color = MaterialTheme.colorScheme.error) } },
            )
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = onClose) { Text("Cancel") }
                Button(
                    enabled = !probing && state.serverDraft.isNotBlank(),
                    onClick = {
                        scope.launch {
                            probing = true
                            problem = null
                            val url = state.serverDraft.trim().trimEnd('/')
                            val ok = Regex("^https?://[^\\s]+$").matches(url) && onProbeServer(url)
                            if (ok) {
                                onServerChange(url)
                                onClose()
                            } else {
                                problem = if (Regex("^https?://").containsMatchIn(url)) "Can't reach that server." else "Use a full URL, like http://10.0.2.2:8080."
                            }
                            probing = false
                        }
                    },
                ) { Text(if (probing) "Checking…" else "Save") }
            }
        }
    }
}
