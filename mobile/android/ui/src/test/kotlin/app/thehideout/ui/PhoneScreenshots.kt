package app.thehideout.ui

import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.longClick
import app.thehideout.ui.gallery.GalleryScreen
import app.thehideout.ui.root.HideoutRoot
import app.thehideout.ui.shell.Destination
import app.thehideout.ui.shell.ShellActions
import app.thehideout.ui.shell.SignedInShell
import app.thehideout.ui.theme.Look
import org.junit.Test
import org.robolectric.annotation.Config

/** The screens on a 411 x 891 dp phone. */
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class PhoneScreenshots : ScreenshotTest() {
    private fun shell(variant: Variant, tab: Destination = Destination.Servers) =
        compose.snapshot("shell-${tab.name.lowercase()}", Device.Phone, variant) { look ->
            SignedInShell(world, look, {}, ShellActions(), "0.1.0", initial = tab)
        }

    @Test fun serversDark() = shell(Variant.Dark)
    @Test fun serversLight() = shell(Variant.Light)
    @Test fun messagesDark() = shell(Variant.Dark, Destination.Messages)
    @Test fun youDark() = shell(Variant.Dark, Destination.You)
    @Test fun youLight() = shell(Variant.Light, Destination.You)

    private fun transcript(variant: Variant) {
        // Bootstrapping already lands on The Hideout's #general (the reducer's first-guild/first-channel
        // rule), and the list-detail navigator follows it onto the detail pane on this phone width.
        compose.setThemed(variant) { look -> SignedInShell(world, look, {}, ShellActions(), "0.1.0") }
        compose.capture("transcript", Device.Phone, variant)
    }

    @Test fun transcriptDark() = transcript(Variant.Dark)
    @Test fun transcriptLight() = transcript(Variant.Light)

    @Test fun mentionsDark() = shell(Variant.Dark, Destination.Mentions)
    @Test fun mentionsLight() = shell(Variant.Light, Destination.Mentions)

    @Test fun messageActionSheetDark() {
        compose.setThemed(Variant.Dark) { look -> SignedInShell(world, look, {}, ShellActions(), "0.1.0") }
        compose.onAllNodesWithText("Release notes are up: v0.0.3 adds a live gateway, polling and much better reconnects.")[0].performTouchInput { longClick() }
        compose.capture("message-action-sheet", Device.Phone, Variant.Dark)
    }

    @Test fun emojiPickerDark() {
        compose.setThemed(Variant.Dark) { look -> SignedInShell(world, look, {}, ShellActions(), "0.1.0") }
        compose.onNodeWithContentDescription("Emoji").performClick()
        compose.capture("emoji-picker", Device.Phone, Variant.Dark)
    }

    @Test fun composerMentionAutocompleteDark() {
        compose.setThemed(Variant.Dark) { look -> SignedInShell(world, look, {}, ShellActions(), "0.1.0") }
        compose.onNodeWithTag("composer-field").performTextInput("hey @a")
        compose.capture("composer-mention-autocomplete", Device.Phone, Variant.Dark)
    }

    @Test fun signedOutDark() = compose.snapshot("signed-out", Device.Phone, Variant.Dark) { _ ->
        HideoutRoot(
            app.thehideout.core.store.AppState(phase = app.thehideout.core.store.SessionPhase.SignedOut()),
            Look(), {}, ShellActions(useDemo = {}),
            app.thehideout.core.auth.AuthUiState(), StubAuth(), "http://10.0.2.2:8080", {}, { true }, {}, "0.1.0",
        )
    }

    @Test fun galleryTopDark() = compose.snapshot("gallery-top", Device.Phone, Variant.Dark) { look -> GalleryScreen(look, {}) }
}
