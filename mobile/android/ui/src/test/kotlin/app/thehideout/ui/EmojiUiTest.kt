package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertHeightIsEqualTo
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertWidthIsEqualTo
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import app.thehideout.core.emoji.EmojiEntry
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.Message
import app.thehideout.core.model.Reaction
import app.thehideout.core.model.ReactionEmoji
import app.thehideout.core.model.User
import app.thehideout.ui.components.EmojiPickerSheet
import app.thehideout.ui.components.LocalMediaBase
import app.thehideout.ui.components.MarkdownText
import app.thehideout.ui.components.MessageRow
import app.thehideout.ui.components.MessageUi
import app.thehideout.ui.theme.HideoutTheme
import app.thehideout.ui.theme.Look
import java.time.Instant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode
import org.robolectric.shadows.ShadowToast

private const val EMOJI = "file:///android_asset/fake_emoji/"

// The same three guild emoji the fake backend seeds into The Hideout (FakeBackend.seedEmoji, MSG-09).
private val wave = Emoji(9001, 1, "wave", animated = false, image = "${EMOJI}wave.webp", creatorId = 2, createdAt = Instant.parse("2026-09-24T10:00:00Z"))
private val thumbsUp = Emoji(9002, 1, "thumbs_up", animated = false, image = "${EMOJI}thumbs_up.webp", creatorId = 3, createdAt = Instant.parse("2026-09-29T10:00:00Z"))
private val partyParrot = Emoji(9003, 1, "party_parrot", animated = true, image = "${EMOJI}party_parrot.gif", creatorId = 4, createdAt = Instant.parse("2026-10-14T10:00:00Z"))
private val serverEmojis = listOf(wave, thumbsUp, partyParrot)
private val byId = serverEmojis.associateBy { it.id }

/** Drives the compose clock and the paused main looper until [until] holds (Coil's fetch of the
 *  bundled asset hops onto a real IO thread and posts back to the paused main looper). */
private fun AndroidComposeTestRule<*, ComponentActivity>.settle(until: () -> Boolean) {
    repeat(50) {
        waitForIdle()
        shadowOf(android.os.Looper.getMainLooper()).idle()
        if (until()) return
        Thread.sleep(20)
    }
}

/**
 * Custom emoji rendering and picker interaction (MSG-09, MD-06/07): inline pictures in text,
 * jumbo-only messages, the graceful `:name:` fallback, the reaction chip, and the picker's
 * sections, search and pick callback.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class EmojiUiTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    private fun setText(text: String, customEmojis: Map<Long, Emoji> = byId) {
        compose.setContent {
            HideoutTheme(Look(), systemDark = true) {
                CompositionLocalProvider(LocalMediaBase provides "") {
                    MarkdownText(text, me = "tim", customEmojis = customEmojis)
                }
            }
        }
    }

    private fun setPicker(serverEmojis: List<Emoji>, onPick: (EmojiEntry) -> Unit = {}) {
        compose.setContent {
            HideoutTheme(Look(), systemDark = true) {
                CompositionLocalProvider(LocalMediaBase provides "") {
                    EmojiPickerSheet(onDismiss = {}, onPick = onPick, serverEmojis = serverEmojis)
                }
            }
        }
    }

    private fun imageDrawn(name: String) = compose.onAllNodesWithContentDescription(":$name:").fetchSemanticsNodes().isNotEmpty()

    @Test fun `an inline custom emoji draws as a 20dp picture and an unknown id stays literal text`() {
        setText("nice work <:thumbs_up:9002> and <:ghost:424242>")
        compose.settle { imageDrawn("thumbs_up") }
        compose.onNodeWithContentDescription(":thumbs_up:").assertIsDisplayed().assertWidthIsEqualTo(20.dp).assertHeightIsEqualTo(20.dp)
        // Unknown to this guild: the same literal `:name:` text desktop shows.
        compose.onNodeWithText(":ghost:", substring = true).assertIsDisplayed()
    }

    @Test fun `an emoji-only message draws jumbo 48dp pictures and glyphs`() {
        setText("<a:party_parrot:9003> <:wave:9001> 🎉")
        compose.settle { imageDrawn("party_parrot") && imageDrawn("wave") }
        compose.onAllNodesWithContentDescription(":party_parrot:")[0].assertWidthIsEqualTo(48.dp).assertHeightIsEqualTo(48.dp)
        compose.onAllNodesWithContentDescription(":wave:")[0].assertWidthIsEqualTo(48.dp).assertHeightIsEqualTo(48.dp)
        compose.onNodeWithText("🎉").assertIsDisplayed()
    }

    @Test fun `text around an emoji keeps it inline, not jumbo`() {
        setText("see you at standup <:wave:9001>")
        compose.settle { imageDrawn("wave") }
        compose.onNodeWithContentDescription(":wave:").assertIsDisplayed().assertWidthIsEqualTo(20.dp).assertHeightIsEqualTo(20.dp)
    }

    @Test fun `long-pressing an inline custom emoji toasts its shortcode`() {
        setText("<:thumbs_up:9002>")
        compose.settle { imageDrawn("thumbs_up") }
        compose.onNodeWithContentDescription(":thumbs_up:").performTouchInput { longClick() }
        compose.waitForIdle()
        shadowOf(android.os.Looper.getMainLooper()).idle()
        assertEquals(":thumbs_up:", ShadowToast.getTextOfLatestToast())
    }

    @Test fun `a message row shows the custom emoji inline and in its reaction chip`() {
        compose.setContent {
            HideoutTheme(Look(), systemDark = true) {
                CompositionLocalProvider(LocalMediaBase provides "") {
                    MessageRow(
                        MessageUi(
                            Message(7, 100, 1, 2, "nice <:wave:9001>", Instant.parse("2026-11-22T10:00:00Z"), reactions = listOf(Reaction(ReactionEmoji.Custom(9001), 2, me = true))),
                            User(2, "ana", Instant.parse("2026-01-01T00:00:00Z")),
                            head = true,
                            me = "tim",
                        ),
                        serverEmojis = serverEmojis,
                    )
                }
            }
        }
        compose.settle { compose.onAllNodesWithContentDescription(":wave:").fetchSemanticsNodes().size == 2 }
        assertEquals("one inline picture plus one reaction chip", 2, compose.onAllNodesWithContentDescription(":wave:").fetchSemanticsNodes().size)
        compose.onNodeWithText("2").assertIsDisplayed()
    }

    @Test fun `the picker lists the server's emoji under This server with every category`() {
        setPicker(serverEmojis)
        compose.settle { imageDrawn("party_parrot") }
        // Each label appears twice: once as a category-strip chip, once as the grid's section header.
        assertEquals(2, compose.onAllNodesWithText("This server").fetchSemanticsNodes().size)
        compose.onAllNodesWithText("This server")[0].assertIsDisplayed()
        compose.onNodeWithContentDescription(":wave:").assertIsDisplayed()
        compose.onNodeWithContentDescription(":thumbs_up:").assertIsDisplayed()
        compose.onNodeWithContentDescription(":party_parrot:").assertIsDisplayed()
        compose.onAllNodesWithText("Smileys & people").fetchSemanticsNodes().isNotEmpty()
        compose.onAllNodesWithText("Flags").fetchSemanticsNodes().isNotEmpty()
    }

    @Test fun `the dm picker has no server section`() {
        setPicker(emptyList())
        compose.waitForIdle()
        assertTrue(compose.onAllNodesWithText("This server").fetchSemanticsNodes().isEmpty())
        compose.onAllNodesWithText("Smileys & people").fetchSemanticsNodes().isNotEmpty()
    }

    @Test fun `picker search filters to the matching emoji and picking one reports the entry`() {
        var picked: EmojiEntry? = null
        setPicker(serverEmojis) { picked = it }
        compose.onNode(hasSetTextAction()).performTextInput("party")
        compose.settle { imageDrawn("party_parrot") }
        // Search mode is a flat result list: no section headers, custom match plus the Unicode one.
        assertTrue(compose.onAllNodesWithText("This server").fetchSemanticsNodes().isEmpty())
        compose.onNodeWithContentDescription(":party_parrot:").assertIsDisplayed()
        compose.onNodeWithText("🥳").assertIsDisplayed()
        compose.onNodeWithContentDescription(":party_parrot:").performClick()
        compose.waitForIdle()
        assertEquals(EmojiEntry.Custom(9003, "party_parrot", "${EMOJI}party_parrot.gif", animated = true), picked)
    }
}
