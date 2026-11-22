package app.thehideout.ui.auth

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.autofill.ContentType
import androidx.compose.ui.autofill.contentType
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.thehideout.ui.theme.Hideout

/** The autofill content type (Credential Manager and the system autofill read it). */
fun Modifier.withAutofill(contentType: ContentType): Modifier = this.contentType(contentType)

/**
 * An auth field: a label row (with an optional trailing link), the field and its error under it
 * (desktop's `labelled`). The IME action moves the focus on.
 */
@Suppress("DEPRECATION") // the visual-transformation TextField flavors are deprecated in Material 3 1.5 but still the outlined look we want
@Composable
fun AuthField(
    label: String,
    value: String,
    onValue: (String) -> Unit,
    modifier: Modifier = Modifier,
    aside: (@Composable () -> Unit)? = null,
    placeholder: String? = null,
    error: String? = null,
    secure: Boolean = false,
    keyboardType: KeyboardType = KeyboardType.Text,
    imeAction: ImeAction = ImeAction.Next,
    autofillType: ContentType? = null,
    enabled: Boolean = true,
    singleLine: Boolean = true,
    onSubmit: (() -> Unit)? = null,
) {
    val focus = LocalFocusManager.current
    val c = Hideout.colors
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
            Text(label, modifier = Modifier.weight(1f), style = MaterialTheme.typography.labelLarge, color = c.textMuted)
            aside?.invoke()
        }
        OutlinedTextField(
            value = value,
            onValueChange = onValue,
            modifier = Modifier.fillMaxWidth().then(if (autofillType != null) Modifier.withAutofill(autofillType) else Modifier),
            placeholder = placeholder?.let { { Text(it, color = c.textFaint) } },
            isError = error != null,
            supportingText = error?.let { { Text(it, color = MaterialTheme.colorScheme.error) } },
            visualTransformation = if (secure) PasswordVisualTransformation() else VisualTransformation.None,
            keyboardOptions = KeyboardOptions(keyboardType = keyboardType, imeAction = imeAction),
            keyboardActions = KeyboardActions(
                onNext = { focus.moveFocus(FocusDirection.Next) },
                onDone = { onSubmit?.invoke() ?: focus.clearFocus() },
                onGo = { onSubmit?.invoke() },
                onSend = { onSubmit?.invoke() },
            ),
            enabled = enabled,
            singleLine = singleLine,
            textStyle = LocalTextStyle.current,
        )
    }
}

/** The 6-digit code field: digits only, centred, with the SMS/email OTP autofill hint. */
@Composable
fun CodeField(
    label: String,
    code: String,
    onCode: (String) -> Unit,
    modifier: Modifier = Modifier,
    error: String? = null,
    enabled: Boolean = true,
    onSubmit: (() -> Unit)? = null,
) {
    val focus = LocalFocusManager.current
    val c = Hideout.colors
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(label, style = MaterialTheme.typography.labelLarge, color = c.textMuted)
        OutlinedTextField(
            value = code,
            onValueChange = onCode,
            modifier = Modifier.fillMaxWidth().withAutofill(ContentType.SmsOtpCode),
            placeholder = { Text("123456", color = c.textFaint) },
            isError = error != null,
            supportingText = error?.let { { Text(it, color = MaterialTheme.colorScheme.error) } },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { onSubmit?.invoke() ?: focus.clearFocus() }),
            enabled = enabled,
            singleLine = true,
            textStyle = LocalTextStyle.current.copy(textAlign = TextAlign.Center, letterSpacing = 4.sp),
        )
    }
}

/** A quiet text link in the accent ink. */
@Composable
fun AccentLink(label: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    TextButton(onClick = onClick, modifier = modifier) {
        Text(label, color = Hideout.colors.accentText, style = MaterialTheme.typography.bodyMedium)
    }
}

/** A quiet text link in the muted ink, for the ways in that are not the main one. */
@Composable
fun QuietLink(label: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    TextButton(onClick = onClick, modifier = modifier) {
        Text(label, color = Hideout.colors.textMuted, style = MaterialTheme.typography.bodyMedium)
    }
}

/** A text link in the faint ink (the server line's "Change"). */
@Composable
fun FaintLink(label: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    TextButton(onClick = onClick, modifier = modifier) {
        Text(label, color = Hideout.colors.textFaint, style = MaterialTheme.typography.bodySmall)
    }
}
