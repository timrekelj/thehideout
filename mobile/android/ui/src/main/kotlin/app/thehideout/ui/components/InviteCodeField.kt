package app.thehideout.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import app.thehideout.core.invite.InviteCode
import app.thehideout.design.Radius
import app.thehideout.ui.theme.Hideout

/**
 * An invite code (`XXXX-XXXX`), one character per box, a dash between the fourth and the fifth —
 * the Compose twin of desktop's `InviteCodeInput`. A transparent, invisible [BasicTextField] lies
 * over the boxes and owns the keys, the caret and the accessibility node, so typing fills the next
 * box, backspace clears the previous one, and a pasted link yields just its code
 * ([InviteCode.normalize]); the boxes only draw what it holds.
 */
@Suppress("DEPRECATION") // the TextFieldValue-based BasicTextField is deprecated in Compose 1.13; the state-based overload has no `onValueChange` hook to re-filter a paste through InviteCode.normalize before it reaches the field
@Composable
fun InviteCodeField(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    label: String = "Invite code",
    error: String? = null,
    enabled: Boolean = true,
    imeAction: ImeAction = ImeAction.Done,
    onSubmit: (() -> Unit)? = null,
) {
    val c = Hideout.colors
    val chars = value.filter { it != '-' }
    var field by remember(chars) { mutableStateOf(TextFieldValue(chars, TextRange(chars.length))) }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Box(Modifier.fillMaxWidth()) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (i in 0 until InviteCode.LEN) {
                    if (i == InviteCode.GROUP) {
                        Box(Modifier.width(12.dp).height(52.dp), contentAlignment = Alignment.Center) {
                            Box(Modifier.width(8.dp).height(2.dp).background(c.textMuted))
                        }
                    }
                    val here = enabled && i == chars.length.coerceAtMost(InviteCode.LEN - 1)
                    val edge = if (error != null) c.danger else if (here) c.accent else c.controlBorder
                    Box(
                        Modifier
                            .weight(1f)
                            .height(52.dp)
                            .clip(RoundedCornerShape(Radius.md))
                            .background(c.surfaceRaised)
                            .border(if (here) 2.dp else 1.dp, edge, RoundedCornerShape(Radius.md)),
                        contentAlignment = Alignment.Center,
                    ) {
                        chars.getOrNull(i)?.let { Text(it.toString(), style = MaterialTheme.typography.titleLarge.copy(fontFamily = FontFamily.Monospace), color = c.text) }
                    }
                }
            }
            BasicTextField(
                value = field,
                onValueChange = { tfv ->
                    val normalized = InviteCode.normalize(tfv.text)
                    field = TextFieldValue(normalized, TextRange(normalized.length))
                    onValueChange(InviteCode.format(normalized))
                },
                modifier = Modifier.fillMaxWidth().height(52.dp).alpha(0f)
                    .testTag("invite-code-input")
                    .semantics { contentDescription = label },
                enabled = enabled,
                singleLine = true,
                textStyle = LocalTextStyle.current,
                keyboardOptions = KeyboardOptions(
                    keyboardType = KeyboardType.Text,
                    imeAction = imeAction,
                    capitalization = KeyboardCapitalization.Characters,
                    autoCorrectEnabled = false,
                ),
                keyboardActions = KeyboardActions(
                    onDone = { onSubmit?.invoke() },
                    onGo = { onSubmit?.invoke() },
                ),
            )
        }
        error?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
    }
}
