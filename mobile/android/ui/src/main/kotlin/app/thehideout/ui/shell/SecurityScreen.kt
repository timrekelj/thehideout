package app.thehideout.ui.shell

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import app.thehideout.core.api.AppError
import app.thehideout.core.auth.PasskeyCeremony
import app.thehideout.core.security.DeviceSession
import app.thehideout.core.security.Passkey
import app.thehideout.core.security.SecurityInfo
import app.thehideout.core.store.AppState
import app.thehideout.ui.auth.PasskeyCreateResult
import app.thehideout.ui.auth.PasskeyLauncher
import app.thehideout.ui.components.GroupLabel
import app.thehideout.ui.components.HideoutCard
import app.thehideout.ui.components.SecureScreenEffect
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

private sealed interface SecurityRoute {
    data object TwoFactor : SecurityRoute
    data object TotpSetup : SecurityRoute
    data class RecoveryCodes(val codes: List<String>) : SecurityRoute
    data object Passkeys : SecurityRoute
    data object Devices : SecurityRoute
    data object DeleteAccount : SecurityRoute
}

/** The route keys that are safe to persist across a process restart (data-bearing ones are not). */
private enum class SecurityRouteKey { TwoFactor, TotpSetup, Passkeys, Devices, DeleteAccount }

/**
 * A [SecurityRoute] is not `Parcelable`/`Serializable` (`rememberSaveable`'s default `Saver` would
 * crash trying to marshal it — "Parcel: unable to marshal value"), so this saves just the simple
 * case's key and rebuilds it on restore. [SecurityRoute.RecoveryCodes] carries the account's
 * recovery codes: it is deliberately never written to saved state (they are shown once and are not
 * retrievable again), so a process restart while that screen is up lands back on the Security list.
 */
private val SecurityRouteSaver = Saver<SecurityRoute?, String>(
    save = { route ->
        when (route) {
            SecurityRoute.TwoFactor -> SecurityRouteKey.TwoFactor.name
            SecurityRoute.TotpSetup -> SecurityRouteKey.TotpSetup.name
            SecurityRoute.Passkeys -> SecurityRouteKey.Passkeys.name
            SecurityRoute.Devices -> SecurityRouteKey.Devices.name
            SecurityRoute.DeleteAccount -> SecurityRouteKey.DeleteAccount.name
            is SecurityRoute.RecoveryCodes, null -> ""
        }
    },
    restore = { key ->
        when (runCatching { SecurityRouteKey.valueOf(key) }.getOrNull()) {
            SecurityRouteKey.TwoFactor -> SecurityRoute.TwoFactor
            SecurityRouteKey.TotpSetup -> SecurityRoute.TotpSetup
            SecurityRouteKey.Passkeys -> SecurityRoute.Passkeys
            SecurityRouteKey.Devices -> SecurityRoute.Devices
            SecurityRouteKey.DeleteAccount -> SecurityRoute.DeleteAccount
            null -> null
        }
    },
)

/** SEC-01..08, ACCT-01..02: Email, Password, Two-factor, Passkeys, Devices, then Export/Delete. */
@Composable
fun SecurityScreen(state: AppState, actions: ShellActions, serverUrl: String, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    BackHandler(onBack = onBack)
    var info by remember { mutableStateOf<SecurityInfo?>(null) }
    var loadError by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    fun reload() {
        scope.launch { actions.getSecurity().fold({ info = it; loadError = null }, { e -> loadError = e.message }) }
    }
    LaunchedEffect(Unit) { reload() }

    var showEmail by rememberSaveable { mutableStateOf(false) }
    var showPassword by rememberSaveable { mutableStateOf(false) }
    var showExportConfirm by remember { mutableStateOf(false) }
    var route by rememberSaveable(stateSaver = SecurityRouteSaver) { mutableStateOf<SecurityRoute?>(null) }

    Box(modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onBack, modifier = Modifier.testTag("security-back")) { SymIcon(Sym.ArrowBack, "Back") }
                Text("Security", style = MaterialTheme.typography.headlineSmall, color = c.text)
            }
            if (loadError != null) Text(loadError!!, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(horizontal = 20.dp))

            GroupLabel("Email")
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                SettingRow(
                    Sym.AlternateEmail, "Email",
                    if (info == null) "…" else if (info!!.emailVerified) "Verified" else "No email yet",
                    onClick = { showEmail = true },
                )
            }

            GroupLabel("Password")
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                SettingRow(Sym.Lock, "Password", if (info?.hasPassword == true) "Change your password" else "Not set", onClick = { showPassword = true })
            }

            GroupLabel("Two-factor authentication")
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                SettingRow(
                    Sym.Lock, "Authenticator app",
                    if (info == null) "…" else if (info!!.totpEnabled) "On · ${info!!.recoveryCodesRemaining} recovery codes left" else "Off",
                    onClick = { route = SecurityRoute.TwoFactor },
                )
            }

            GroupLabel("Passkeys")
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                SettingRow(Sym.Fingerprint, "Passkeys", if (info == null) "…" else "${info!!.passkeyCount} on this account", onClick = { route = SecurityRoute.Passkeys })
            }

            GroupLabel("Devices")
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                SettingRow(Sym.Devices, "Sessions", "Where you're signed in", onClick = { route = SecurityRoute.Devices })
            }

            GroupLabel("Account")
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                Column {
                    SettingRow(
                        Sym.Download, "Export my data",
                        state.dataExportMaskedEmail?.let { "Link emailed to $it" } ?: "Emails you a download link (once every 24 hours)",
                        onClick = { showExportConfirm = true },
                    )
                    HorizontalDivider(color = c.borderSubtle)
                    SettingRow(Sym.Warning, "Delete account", "This can't be undone", destructive = true, onClick = { route = SecurityRoute.DeleteAccount })
                }
            }
        }

        if (showEmail) ChangeEmailFlow(actions, hasPassword = info?.hasPassword ?: true, onDismiss = { showEmail = false; reload() })
        if (showPassword) ChangePasswordDialog(actions, hasPassword = info?.hasPassword ?: true, onDismiss = { showPassword = false; reload() })
        if (showExportConfirm) ExportDataDialog(onDismiss = { showExportConfirm = false }, onConfirm = actions.exportData)

        when (val r = route) {
            SecurityRoute.TwoFactor -> {
                BackHandler { route = null }
                TwoFactorScreen(
                    actions, info,
                    onBack = { route = null },
                    onSetUp = { route = SecurityRoute.TotpSetup },
                    onReload = ::reload,
                )
            }
            SecurityRoute.TotpSetup -> {
                BackHandler { route = SecurityRoute.TwoFactor }
                TotpSetupScreen(
                    actions,
                    onBack = { route = SecurityRoute.TwoFactor },
                    onEnabled = { codes -> route = SecurityRoute.RecoveryCodes(codes) },
                )
            }
            is SecurityRoute.RecoveryCodes -> RecoveryCodesScreen(r.codes, title = "Save your recovery codes", onDone = { route = null; reload() })
            SecurityRoute.Passkeys -> {
                BackHandler { route = null }
                PasskeysScreen(actions, serverUrl, onBack = { route = null; reload() })
            }
            SecurityRoute.Devices -> {
                BackHandler { route = null }
                DevicesScreen(actions, onBack = { route = null })
            }
            SecurityRoute.DeleteAccount -> DeleteAccountScreen(actions, info, onBack = { route = null })
            null -> Unit
        }
    }
}

@Composable
private fun SettingRow(sym: Sym, title: String, subtitle: String?, destructive: Boolean = false, onClick: () -> Unit) {
    val c = Hideout.colors
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SymIcon(sym, null, tint = if (destructive) c.danger else c.accentText)
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = if (destructive) c.danger else c.text)
            if (subtitle != null) Text(subtitle, style = MaterialTheme.typography.bodySmall, color = c.textMuted)
        }
        SymIcon(Sym.ChevronRight, null, tint = c.textFaint)
    }
}

/** A password field with a show/hide toggle (never left in plain text by default). */
@Suppress("DEPRECATION") // the visual-transformation TextField flavors are deprecated in Material 3 1.5 but still the outlined look we want
@Composable
private fun PasswordField(value: String, onValue: (String) -> Unit, label: String, modifier: Modifier = Modifier, testTag: String? = null) {
    var visible by remember { mutableStateOf(false) }
    OutlinedTextField(
        value = value,
        onValueChange = onValue,
        label = { Text(label) },
        singleLine = true,
        visualTransformation = if (visible) VisualTransformation.None else PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
        trailingIcon = {
            IconButton(onClick = { visible = !visible }) {
                SymIcon(if (visible) Sym.VisibilityOff else Sym.Visibility, if (visible) "Hide password" else "Show password")
            }
        },
        modifier = modifier.then(if (testTag != null) Modifier.testTag(testTag) else Modifier),
    )
}

// ---- email (SEC-03) ---------------------------------------------------------------------------

@Composable
private fun ChangeEmailFlow(actions: ShellActions, hasPassword: Boolean, onDismiss: () -> Unit) {
    var step by remember { mutableStateOf(0) } // 0 = new email + password, 1 = code
    var newEmail by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    if (step == 0) {
        AlertDialog(
            onDismissRequest = { if (!busy) onDismiss() },
            title = { Text("Change email") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    OutlinedTextField(newEmail, { newEmail = it; error = null }, label = { Text("New email") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                    if (hasPassword) PasswordField(password, { password = it; error = null }, "Current password", modifier = Modifier.fillMaxWidth())
                    error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
                }
            },
            confirmButton = {
                TextButton(
                    enabled = !busy && newEmail.contains("@"),
                    onClick = {
                        scope.launch {
                            busy = true
                            actions.changeEmail(newEmail.trim(), password).fold(
                                { busy = false; step = 1 },
                                { e -> busy = false; error = e.message ?: "Couldn't send the code." },
                            )
                        }
                    },
                ) { Text(if (busy) "Sending…" else "Send code") }
            },
            dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
        )
    } else {
        AlertDialog(
            onDismissRequest = { if (!busy) onDismiss() },
            title = { Text("Enter the code") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("We emailed a code to $newEmail.")
                    OutlinedTextField(code, { code = it; error = null }, label = { Text("6-digit code") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                    error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
                }
            },
            confirmButton = {
                TextButton(
                    enabled = !busy && code.length >= 4,
                    onClick = {
                        scope.launch {
                            busy = true
                            actions.confirmEmailChange(code.trim()).fold(
                                { onDismiss() },
                                { e -> busy = false; error = e.message ?: "Wrong code." },
                            )
                        }
                    },
                ) { Text(if (busy) "Confirming…" else "Confirm") }
            },
            dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
        )
    }
}

// ---- password (SEC-02) -------------------------------------------------------------------------

@Composable
private fun ChangePasswordDialog(actions: ShellActions, hasPassword: Boolean, onDismiss: () -> Unit) {
    var current by remember { mutableStateOf("") }
    var new by remember { mutableStateOf("") }
    var confirm by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(if (hasPassword) "Change password" else "Set a password") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                if (hasPassword) PasswordField(current, { current = it; error = null }, "Current password", modifier = Modifier.fillMaxWidth())
                PasswordField(new, { new = it; error = null }, "New password", modifier = Modifier.fillMaxWidth())
                PasswordField(confirm, { confirm = it; error = null }, "Confirm new password", modifier = Modifier.fillMaxWidth())
                Text("Signs out your other devices.", style = MaterialTheme.typography.bodySmall, color = Hideout.colors.textMuted)
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy && new.length >= 8 && new == confirm,
                onClick = {
                    scope.launch {
                        busy = true
                        actions.changePassword(current, new).fold(
                            { onDismiss() },
                            { e -> busy = false; error = e.message ?: "Couldn't change the password." },
                        )
                    }
                },
            ) { Text(if (busy) "Saving…" else "Save") }
        },
        dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
    )
}

// ---- export (ACCT-02) --------------------------------------------------------------------------

@Composable
private fun ExportDataDialog(onDismiss: () -> Unit, onConfirm: suspend () -> Result<String>) {
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text("Export my data") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("We'll email you a link to download a copy of your data. This works once every 24 hours.")
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy,
                onClick = {
                    scope.launch {
                        busy = true
                        onConfirm().fold(
                            { onDismiss() },
                            { e ->
                                busy = false
                                error = if (e is AppError.RateLimited) "You can request one export per 24 hours." else (e as? AppError)?.message ?: "Couldn't request the export."
                            },
                        )
                    }
                },
            ) { Text(if (busy) "Requesting…" else "Request export") }
        },
        dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
    )
}

// ---- two-factor authentication (SEC-04/05) -----------------------------------------------------

@Composable
private fun TwoFactorScreen(actions: ShellActions, info: SecurityInfo?, onBack: () -> Unit, onSetUp: () -> Unit, onReload: () -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    var showDisable by remember { mutableStateOf(false) }
    var showRegenerate by remember { mutableStateOf(false) }
    var regenerated by remember { mutableStateOf<List<String>?>(null) }
    Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack, modifier = Modifier.testTag("totp-back")) { SymIcon(Sym.ArrowBack, "Back") }
            Text("Authenticator app", style = MaterialTheme.typography.headlineSmall, color = c.text)
        }
        if (info?.totpEnabled == true) {
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                Column {
                    SettingRow(Sym.Lock, "Recovery codes", "${info.recoveryCodesRemaining} left · tap to make new ones", onClick = { showRegenerate = true })
                    HorizontalDivider(color = c.borderSubtle)
                    SettingRow(Sym.Close, "Turn off two-factor authentication", null, destructive = true, onClick = { showDisable = true })
                }
            }
        } else {
            Text(
                "An authenticator app (like Google Authenticator or 1Password) generates a code every sign-in, on top of your password.",
                color = c.textMuted, style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp),
            )
            Button(onClick = onSetUp, shapes = ButtonDefaults.shapes(), modifier = Modifier.padding(horizontal = 20.dp)) { Text("Set up") }
        }
    }
    if (showDisable) {
        CodeConfirmDialog(
            title = "Turn off two-factor authentication",
            body = "Enter a current code from your app, or one of your recovery codes.",
            confirmLabel = "Turn off",
            onDismiss = { showDisable = false },
            onConfirm = { code -> actions.disableTotp(code) },
            onSuccess = { showDisable = false; onReload() },
        )
    }
    if (showRegenerate) {
        CodeConfirmDialog(
            title = "Make new recovery codes",
            body = "Your old codes will stop working. Enter a current code from your authenticator app.",
            confirmLabel = "Make new codes",
            onDismiss = { showRegenerate = false },
            onConfirm = { code -> actions.regenerateRecoveryCodes(code) },
            onSuccess = { codes -> showRegenerate = false; regenerated = codes },
        )
    }
    regenerated?.let { codes -> RecoveryCodesScreen(codes, title = "Your new recovery codes", onDone = { regenerated = null; onReload() }) }
}

/** A single code field that confirms a destructive/sensitive action; [onConfirm] may return a value [onSuccess] receives. */
@Composable
private fun <T> CodeConfirmDialog(title: String, body: String, confirmLabel: String, onDismiss: () -> Unit, onConfirm: suspend (String) -> Result<T>, onSuccess: (T) -> Unit) {
    var code by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(title) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(body)
                OutlinedTextField(code, { code = it; error = null }, label = { Text("Code") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy && code.isNotBlank(),
                onClick = {
                    scope.launch {
                        busy = true
                        onConfirm(code.trim()).fold({ onSuccess(it) }, { e -> busy = false; error = e.message ?: "That code didn't work." })
                    }
                },
            ) { Text(if (busy) "Working…" else confirmLabel) }
        },
        dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
    )
}

/** A black-on-white QR bitmap of [content] (the TOTP `otpauth://` URI), computed off the main thread. */
@Composable
private fun rememberQrBitmap(content: String): android.graphics.Bitmap? {
    val state = produceState<android.graphics.Bitmap?>(initialValue = null, content) {
        value = withContext(Dispatchers.Default) { qrBitmap(content) }
    }
    return state.value
}

@Suppress("DEPRECATION") // LocalClipboardManager is deprecated for the suspend-based LocalClipboard; a plain synchronous copy fits a short code better than threading a CoroutineScope through
@Composable
private fun TotpSetupScreen(actions: ShellActions, onBack: () -> Unit, onEnabled: (List<String>) -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    SecureScreenEffect()
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    var uri by remember { mutableStateOf<String?>(null) }
    var secret by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(Unit) {
        actions.beginTotpSetup().fold({ uri = it.otpauthUri; secret = it.secret }, { e -> error = e.message })
    }
    Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { SymIcon(Sym.ArrowBack, "Back") }
            Text("Set up two-factor", style = MaterialTheme.typography.headlineSmall, color = c.text)
        }
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("Scan this with your authenticator app, or enter the key manually.", color = c.textMuted)
            val u = uri
            if (u != null) {
                val bitmap = rememberQrBitmap(u)
                if (bitmap != null) {
                    Image(bitmap.asImageBitmap(), "QR code", modifier = Modifier.size(220.dp).align(Alignment.CenterHorizontally))
                } else {
                    LoadingIndicator(modifier = Modifier.align(Alignment.CenterHorizontally))
                }
                Row(Modifier.fillMaxWidth().clickable { clipboard.setText(AnnotatedString(secret)) }, horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(secret, style = MaterialTheme.typography.bodyLarge.copy(fontFamily = app.thehideout.design.DesignFonts.mono), color = c.text, modifier = Modifier.weight(1f))
                    SymIcon(Sym.ContentCopy, "Copy key", tint = c.accentText)
                }
                TextButton(onClick = { runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(u))) } }) { Text("Open in authenticator app") }
            } else if (error == null) {
                LoadingIndicator()
            }
            OutlinedTextField(code, { code = it; error = null }, label = { Text("Code from the app") }, singleLine = true, modifier = Modifier.fillMaxWidth())
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            Button(
                enabled = !busy && code.isNotBlank() && uri != null,
                shapes = ButtonDefaults.shapes(),
                onClick = {
                    scope.launch {
                        busy = true
                        actions.confirmTotp(code.trim()).fold({ codes -> onEnabled(codes) }, { e -> busy = false; error = e.message ?: "That code didn't work." })
                    }
                },
            ) { Text(if (busy) "Confirming…" else "Turn on") }
        }
    }
}

@Suppress("DEPRECATION") // LocalClipboardManager is deprecated for the suspend-based LocalClipboard; a plain synchronous copy fits a short code better than threading a CoroutineScope through
@Composable
private fun RecoveryCodesScreen(codes: List<String>, title: String, onDone: () -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    SecureScreenEffect()
    val clipboard = LocalClipboardManager.current
    val context = LocalContext.current
    var saved by remember { mutableStateOf(false) }
    var confirmLeave by remember { mutableStateOf(false) }
    val tryLeave = { if (saved) onDone() else confirmLeave = true }
    // Self-contained: the only way out is "I saved them" (the Done button already requires it) or
    // explicitly confirming "leave without saving" on the system/gesture back, never a silent skip.
    BackHandler(onBack = tryLeave)
    Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState()).padding(20.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Text(title, style = MaterialTheme.typography.headlineSmall, color = c.text)
        Text("Each code works once, if you lose access to your authenticator app. We won't show these again.", color = c.textMuted)
        HideoutCard(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                codes.forEach { code -> Text(code, style = MaterialTheme.typography.bodyLarge.copy(fontFamily = app.thehideout.design.DesignFonts.mono), color = c.text) }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            TextButton(onClick = { clipboard.setText(AnnotatedString(codes.joinToString("\n"))) }) {
                SymIcon(Sym.ContentCopy, null, tint = c.accentText); Text(" Copy", color = c.accentText)
            }
            TextButton(onClick = {
                runCatching {
                    context.startActivity(
                        Intent.createChooser(Intent(Intent.ACTION_SEND).apply { type = "text/plain"; putExtra(Intent.EXTRA_TEXT, codes.joinToString("\n")) }, null),
                    )
                }
            }) { SymIcon(Sym.Share, null, tint = c.accentText); Text(" Share", color = c.accentText) }
        }
        Row(Modifier.clickable { saved = !saved }, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            RadioButton(selected = saved, onClick = { saved = !saved })
            Text("I saved these codes somewhere safe", color = c.text)
        }
        Button(enabled = saved, onClick = onDone, shapes = ButtonDefaults.shapes()) { Text("Done") }
    }
    if (confirmLeave) {
        AlertDialog(
            onDismissRequest = { confirmLeave = false },
            title = { Text("Leave without saving?") },
            text = { Text("You won't be able to see these recovery codes again.") },
            confirmButton = { TextButton(onClick = { confirmLeave = false; onDone() }) { Text("Leave") } },
            dismissButton = { TextButton(onClick = { confirmLeave = false }) { Text("Stay") } },
        )
    }
}

// ---- passkeys (SEC-06) --------------------------------------------------------------------------

@Composable
private fun PasskeysScreen(actions: ShellActions, serverUrl: String, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    var passkeys by remember { mutableStateOf<List<Passkey>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var renaming by remember { mutableStateOf<Passkey?>(null) }
    var deleting by remember { mutableStateOf<Passkey?>(null) }
    var adding by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun reload() {
        scope.launch { actions.listPasskeys().fold({ passkeys = it }, { e -> error = e.message }) }
    }
    LaunchedEffect(Unit) { reload() }
    val formatter = remember { DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withZone(ZoneId.systemDefault()) }
    Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { SymIcon(Sym.ArrowBack, "Back") }
            Text("Passkeys", style = MaterialTheme.typography.headlineSmall, color = c.text, modifier = Modifier.weight(1f))
            TextButton(onClick = { adding = true }) { Text("Add") }
        }
        if (error != null) Text(error!!, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(horizontal = 20.dp))
        passkeys?.forEach { pk ->
            HideoutCard(Modifier.padding(horizontal = 16.dp, vertical = 4.dp).fillMaxWidth()) {
                Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    SymIcon(Sym.Fingerprint, null, tint = c.accentText)
                    Column(Modifier.weight(1f).clickable { renaming = pk }) {
                        Text(pk.name, style = MaterialTheme.typography.bodyLarge, color = c.text)
                        Text("Added ${formatter.format(pk.createdAt)}", style = MaterialTheme.typography.bodySmall, color = c.textMuted)
                    }
                    IconButton(onClick = { deleting = pk }) { SymIcon(Sym.Delete, "Delete ${pk.name}", tint = c.danger) }
                }
            }
        }
        if (passkeys?.isEmpty() == true) {
            Text("No passkeys yet.", color = c.textMuted, modifier = Modifier.padding(20.dp))
        }
    }
    if (adding) {
        AddPasskeyFlow(actions, serverUrl, onDismiss = { adding = false; reload() })
    }
    renaming?.let { pk ->
        RenamePasskeyDialog(pk, actions, onDismiss = { renaming = null }, onRenamed = { renaming = null; reload() })
    }
    deleting?.let { pk ->
        var busy by remember(pk) { mutableStateOf(false) }
        var dialogError by remember(pk) { mutableStateOf<String?>(null) }
        AlertDialog(
            onDismissRequest = { if (!busy) deleting = null },
            title = { Text("Delete passkey") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("\"${pk.name}\" won't be able to sign in anymore.")
                    dialogError?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
                }
            },
            confirmButton = {
                TextButton(
                    enabled = !busy,
                    onClick = {
                        scope.launch {
                            busy = true
                            actions.deletePasskey(pk.id).fold({ deleting = null; reload() }, { e -> busy = false; dialogError = e.message ?: "Couldn't delete it." })
                        }
                    },
                ) { Text(if (busy) "Deleting…" else "Delete") }
            },
            dismissButton = { TextButton(enabled = !busy, onClick = { deleting = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun RenamePasskeyDialog(passkey: Passkey, actions: ShellActions, onDismiss: () -> Unit, onRenamed: () -> Unit) {
    var name by remember { mutableStateOf(passkey.name) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text("Rename passkey") },
        text = {
            Column {
                OutlinedTextField(name, { name = it; error = null }, singleLine = true, modifier = Modifier.fillMaxWidth().testTag("rename-passkey-name"))
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy && name.isNotBlank(),
                onClick = {
                    scope.launch {
                        busy = true
                        actions.renamePasskey(passkey.id, name.trim()).fold({ onRenamed() }, { e -> busy = false; error = e.message ?: "Couldn't rename it." })
                    }
                },
            ) { Text("Save") }
        },
        dismissButton = { TextButton(enabled = !busy, onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun AddPasskeyFlow(actions: ShellActions, serverUrl: String, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val launcher = remember { PasskeyLauncher(context) }
    var name by remember { mutableStateOf("") }
    var ceremony by remember { mutableStateOf<PasskeyCeremony?>(null) }
    var busy by remember { mutableStateOf(false) }
    var polling by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(ceremony) {
        val c = ceremony ?: return@LaunchedEffect
        if (PasskeyLauncher.isDomainHost(serverUrl)) {
            when (val result = launcher.createCredential(c)) {
                is PasskeyCreateResult.Success -> {
                    actions.finishPasskeyRegistration(c.ceremonyId, result.json).fold(
                        { onDismiss() },
                        { e -> busy = false; ceremony = null; error = e.message ?: "Couldn't add the passkey." },
                    )
                    return@LaunchedEffect
                }
                PasskeyCreateResult.Cancelled -> {
                    // The user backed out of the system UI on purpose: stop, no browser fallback.
                    busy = false
                    ceremony = null
                    return@LaunchedEffect
                }
                PasskeyCreateResult.Unsupported -> Unit // fall through to the browser ceremony below
            }
        }
        launcher.openCeremonyTab(c)
        busy = false
        polling = true
        var polls = 0
        while (polls < 80 && ceremony == c) {
            delay(1500)
            if (ceremony != c) return@LaunchedEffect // cancelled while waiting out the delay
            actions.finishPasskeyRegistration(c.ceremonyId, "").fold(
                { finished -> if (!finished.pending) { polling = false; onDismiss(); return@LaunchedEffect } },
                { e -> polling = false; ceremony = null; error = e.message ?: "Couldn't add the passkey."; return@LaunchedEffect },
            )
            polls++
        }
        if (ceremony == c && polling) {
            polling = false
            ceremony = null
            error = "Timed out waiting for the browser. Try again."
        }
    }
    AlertDialog(
        onDismissRequest = { if (!busy) { ceremony = null; onDismiss() } },
        title = { Text("Add a passkey") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                if (polling) {
                    Text("Finish adding the passkey in the browser tab, then come back here.")
                    LoadingIndicator()
                } else {
                    Text("Uses your device's screen lock, fingerprint or face.")
                    OutlinedTextField(name, { name = it; error = null }, label = { Text("Name (optional)") }, singleLine = true, enabled = !busy, modifier = Modifier.fillMaxWidth())
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            }
        },
        confirmButton = {
            if (!polling) {
                TextButton(
                    enabled = !busy,
                    onClick = {
                        scope.launch {
                            busy = true
                            actions.beginPasskeyRegistration(name.trim()).fold({ ceremony = it }, { e -> busy = false; error = e.message ?: "Couldn't start." })
                        }
                    },
                ) { Text(if (busy) "Working…" else "Continue") }
            }
        },
        // Never disabled: a passkey ceremony (native prompt or the browser tab) can always be called off.
        dismissButton = { TextButton(onClick = { ceremony = null; onDismiss() }) { Text("Cancel") } },
    )
}

// ---- devices / sessions (SEC-07) -----------------------------------------------------------------

@Composable
private fun DevicesScreen(actions: ShellActions, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    var sessions by remember { mutableStateOf<List<DeviceSession>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var revoking by remember { mutableStateOf<DeviceSession?>(null) }
    var revokingOthers by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun reload() {
        scope.launch { actions.listSessions().fold({ sessions = it }, { e -> error = e.message }) }
    }
    LaunchedEffect(Unit) { reload() }
    val formatter = remember { DateTimeFormatter.ofLocalizedDateTime(FormatStyle.SHORT).withZone(ZoneId.systemDefault()) }
    Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack, modifier = Modifier.testTag("devices-back")) { SymIcon(Sym.ArrowBack, "Back") }
            Text("Devices", style = MaterialTheme.typography.headlineSmall, color = c.text, modifier = Modifier.weight(1f))
            if ((sessions?.size ?: 0) > 1) TextButton(onClick = { revokingOthers = true }) { Text("Sign out others") }
        }
        if (error != null) Text(error!!, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(horizontal = 20.dp))
        sessions?.forEach { s ->
            HideoutCard(Modifier.padding(horizontal = 16.dp, vertical = 4.dp).fillMaxWidth()) {
                Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    SymIcon(Sym.Devices, null, tint = c.accentText)
                    Column(Modifier.weight(1f)) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text(s.deviceName, style = MaterialTheme.typography.bodyLarge, color = c.text)
                            if (s.current) Text("This device", style = MaterialTheme.typography.labelSmall, color = c.accentText, fontWeight = FontWeight(650))
                        }
                        Text(
                            s.lastSeenAt?.let { "Last seen ${formatter.format(it)}" } ?: "Signed in ${formatter.format(s.createdAt)}",
                            style = MaterialTheme.typography.bodySmall, color = c.textMuted,
                        )
                    }
                    if (!s.current) IconButton(onClick = { revoking = s }) { SymIcon(Sym.Close, "Sign out", tint = c.danger) }
                }
            }
        }
    }
    revoking?.let { s ->
        var busy by remember(s) { mutableStateOf(false) }
        var dialogError by remember(s) { mutableStateOf<String?>(null) }
        AlertDialog(
            onDismissRequest = { if (!busy) revoking = null },
            title = { Text("Sign out this device?") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("\"${s.deviceName}\" will need to sign in again.")
                    dialogError?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
                }
            },
            confirmButton = {
                TextButton(
                    enabled = !busy,
                    modifier = Modifier.testTag("revoke-session-submit"),
                    onClick = {
                        scope.launch {
                            busy = true
                            actions.revokeSession(s.id).fold({ revoking = null; reload() }, { e -> busy = false; dialogError = e.message ?: "Couldn't sign it out." })
                        }
                    },
                ) { Text(if (busy) "Signing out…" else "Sign out") }
            },
            dismissButton = { TextButton(enabled = !busy, onClick = { revoking = null }) { Text("Cancel") } },
        )
    }
    if (revokingOthers) {
        var busy by remember { mutableStateOf(false) }
        var dialogError by remember { mutableStateOf<String?>(null) }
        AlertDialog(
            onDismissRequest = { if (!busy) revokingOthers = false },
            title = { Text("Sign out every other device?") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("Everywhere you're signed in except this device will be signed out.")
                    dialogError?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
                }
            },
            confirmButton = {
                TextButton(
                    enabled = !busy,
                    modifier = Modifier.testTag("revoke-others-submit"),
                    onClick = {
                        scope.launch {
                            busy = true
                            actions.revokeOtherSessions().fold({ revokingOthers = false; reload() }, { e -> busy = false; dialogError = e.message ?: "Couldn't sign them out." })
                        }
                    },
                ) { Text(if (busy) "Signing out…" else "Sign out others") }
            },
            dismissButton = { TextButton(enabled = !busy, onClick = { revokingOthers = false }) { Text("Cancel") } },
        )
    }
}

// ---- delete account (ACCT-01) ---------------------------------------------------------------------

@Composable
private fun DeleteAccountScreen(actions: ShellActions, info: SecurityInfo?, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    var password by remember { mutableStateOf("") }
    var emailCode by remember { mutableStateOf("") }
    var totpCode by remember { mutableStateOf("") }
    var codeSent by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var ownedGuilds by remember { mutableStateOf<List<app.thehideout.core.api.OwnedGuild>?>(null) }
    var deleted by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val hasPassword = info?.hasPassword ?: true
    val needsTotp = info?.totpEnabled == true
    // Once the account is actually gone (or the request is in flight), Back can't skip the
    // confirmation and leave the store's own delayed sign-out (see AppStore.deleteAccount) to run
    // unobserved — the store does the sign-out regardless, but this avoids a jarring back-out.
    val canLeave = !busy && !deleted
    BackHandler(enabled = canLeave, onBack = onBack)

    Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(enabled = canLeave, onClick = onBack, modifier = Modifier.testTag("delete-account-back")) { SymIcon(Sym.ArrowBack, "Back") }
            Text("Delete account", style = MaterialTheme.typography.headlineSmall, color = c.text)
        }
        if (deleted) {
            Text("Your account has been deleted. Signing you out…", color = c.text, modifier = Modifier.padding(20.dp))
            return@Column
        }
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Text("This deletes your account and signs you out everywhere. This can't be undone.", color = c.textMuted)
            val owned = ownedGuilds
            if (owned != null && owned.isNotEmpty()) {
                HideoutCard(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text("You still own these servers. Transfer or delete them from each server's settings first:", color = c.text)
                        owned.forEach { g -> Text("• ${g.name} (${g.memberCount} members)", color = c.textMuted) }
                    }
                }
            } else {
                if (hasPassword) {
                    PasswordField(password, { password = it; error = null }, "Current password", modifier = Modifier.fillMaxWidth(), testTag = "delete-account-password")
                } else {
                    if (!codeSent) {
                        TextButton(onClick = {
                            scope.launch { actions.requestAccountDeletionCode().fold({ codeSent = true }, { e -> error = e.message }) }
                        }) { Text("Email me a code") }
                    } else {
                        OutlinedTextField(emailCode, { emailCode = it; error = null }, label = { Text("Emailed code") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                    }
                }
                if (needsTotp) {
                    OutlinedTextField(totpCode, { totpCode = it; error = null }, label = { Text("Two-factor code") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                Button(
                    enabled = !busy && (if (hasPassword) password.isNotBlank() else emailCode.isNotBlank()) && (!needsTotp || totpCode.isNotBlank()),
                    colors = ButtonDefaults.buttonColors(containerColor = c.danger),
                    shapes = ButtonDefaults.shapes(),
                    modifier = Modifier.testTag("delete-account-submit"),
                    onClick = {
                        scope.launch {
                            busy = true
                            actions.deleteAccount(
                                if (hasPassword) password else null,
                                if (!hasPassword) emailCode.trim() else null,
                                if (needsTotp) totpCode.trim() else null,
                            ).fold(
                                // The store itself signs out shortly after (see AppStore.deleteAccount); this
                                // screen only shows the confirmation until the app's own phase flips away.
                                { deleted = true },
                                { e ->
                                    busy = false
                                    if (e is AppError.OwnsGuilds) ownedGuilds = e.guilds else error = e.message ?: "Couldn't delete the account."
                                },
                            )
                        }
                    },
                ) { Text(if (busy) "Deleting…" else "Delete my account") }
            }
        }
    }
}

/** A black-on-white QR bitmap of [content] (the TOTP `otpauth://` URI), no external QR scanner needed to read it back. */
private fun qrBitmap(content: String, size: Int = 480): android.graphics.Bitmap {
    val writer = com.google.zxing.qrcode.QRCodeWriter()
    val matrix = writer.encode(content, com.google.zxing.BarcodeFormat.QR_CODE, size, size)
    val pixels = IntArray(size * size)
    for (y in 0 until size) {
        val row = y * size
        for (x in 0 until size) pixels[row + x] = if (matrix.get(x, y)) android.graphics.Color.BLACK else android.graphics.Color.WHITE
    }
    val bmp = android.graphics.Bitmap.createBitmap(size, size, android.graphics.Bitmap.Config.ARGB_8888)
    bmp.setPixels(pixels, 0, size, 0, 0, size, size)
    return bmp
}
