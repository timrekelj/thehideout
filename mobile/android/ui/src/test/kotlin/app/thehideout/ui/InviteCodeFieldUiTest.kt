package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import app.thehideout.ui.components.InviteCodeField
import app.thehideout.ui.theme.HideoutTheme
import app.thehideout.ui.theme.Look
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode

/** The 8-box invite code field's paste/backspace/forgiven-character behaviour (AUTH-08, DLG-02's email invites). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class InviteCodeFieldUiTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    private fun harness(): () -> String {
        var captured = ""
        compose.setContent {
            HideoutTheme(look = Look(), systemDark = true) {
                var v by remember { mutableStateOf("") }
                InviteCodeField(v, { v = it; captured = it })
            }
        }
        return { captured }
    }

    @Test fun `a pasted link yields just its code`() {
        val value = harness()
        compose.onNodeWithTag("invite-code-input").performTextInput("https://thehideout.zabit.si/invite/N0X6-PFNB")
        compose.waitForIdle()
        assertEquals("N0X6-PFNB", value())
    }

    @Test fun `I L and O are forgiven as 1 1 and 0`() {
        val value = harness()
        compose.onNodeWithTag("invite-code-input").performTextInput("iIlLoO12")
        compose.waitForIdle()
        assertEquals("1111-0012", value())
    }

    @Test fun `shortening the text (backspace) drops the dash until it is complete again`() {
        val value = harness()
        compose.onNodeWithTag("invite-code-input").performTextInput("WXYZ2345")
        compose.waitForIdle()
        assertEquals("WXYZ-2345", value())
        // Simulates a backspace: the field's own text (undashed internally) one character shorter.
        compose.onNodeWithTag("invite-code-input").performTextReplacement("WXYZ234")
        compose.waitForIdle()
        assertEquals("WXYZ234", value())
    }

    @Test fun `an error shows under the boxes`() {
        compose.setContent {
            HideoutTheme(look = Look(), systemDark = true) {
                InviteCodeField("WXYZ-2345", {}, error = "We couldn't find that invite.")
            }
        }
        compose.onNodeWithText("We couldn't find that invite.").assertIsDisplayed()
    }
}
