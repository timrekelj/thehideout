package app.thehideout.ui

import androidx.activity.ComponentActivity
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.thehideout.core.model.Embed
import app.thehideout.core.model.EmbedKind
import app.thehideout.core.model.EmbedMedia
import app.thehideout.core.model.EmbedMediaKind
import app.thehideout.ui.components.EmbedCard
import app.thehideout.ui.components.LocalMediaBase
import app.thehideout.ui.theme.HideoutTheme
import app.thehideout.ui.theme.Look
import java.time.Instant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode

private const val MEDIA = "file:///android_asset/fake_media/"

private val linkCard = Embed(
    url = "https://example.com/wiki/release",
    title = "Release notes: v0.0.3",
    description = "Adds a live gateway, drops polling in favor of push updates.",
    siteName = "example wiki",
    thumbnail = "$MEDIA/landscape-3.jpg",
    thumbnailWidth = 1200,
    thumbnailHeight = 800,
    kind = EmbedKind.Link,
)

private val richPost = Embed(
    url = "https://x.example.com/cvetka/status/1",
    provider = "x",
    authorName = "cvetka",
    description = "Shipped the new icon set today.",
    kind = EmbedKind.Rich,
    media = listOf(EmbedMedia(kind = EmbedMediaKind.Image, file = "$MEDIA/landscape-4.jpg", width = 1200, height = 800)),
    publishedAt = Instant.parse("2026-11-18T04:00:00Z"),
)

/** Drives the compose clock and the paused main looper until [until] holds. */
private fun AndroidComposeTestRule<*, ComponentActivity>.settle(until: () -> Boolean) {
    repeat(50) {
        waitForIdle()
        shadowOf(android.os.Looper.getMainLooper()).idle()
        if (until()) return
        Thread.sleep(20)
    }
}

/** The header chevron folds a card away (and back): the body's description and Open-link title
 *  leave the tree, while the folded header keeps the site name and the title (ATT-07). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@LooperMode(LooperMode.Mode.PAUSED)
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class EmbedUiTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    private fun setCard(embed: Embed, collapsed: androidx.compose.runtime.MutableState<Boolean>) {
        compose.setContent {
            HideoutTheme(Look(), systemDark = true) {
                CompositionLocalProvider(LocalMediaBase provides "") {
                    EmbedCard(embed, collapsed = collapsed.value, onToggleCollapsed = { collapsed.value = !collapsed.value })
                }
            }
        }
    }

    @Test fun `header chevron folds and unfolds a link card`() {
        val collapsed = mutableStateOf(false)
        setCard(linkCard, collapsed)
        compose.onNodeWithText("Release notes: v0.0.3").assertIsDisplayed()
        compose.onNodeWithText("Adds a live gateway", substring = true).assertIsDisplayed()

        compose.onNodeWithText("example wiki").performClick()
        compose.waitForIdle()
        assert(collapsed.value) { "the header click should have folded the card" }
        compose.onAllNodesWithText("Adds a live gateway", substring = true).fetchSemanticsNodes().let {
            assert(it.isEmpty()) { "the folded card should hide its body, found ${it.size} description nodes" }
        }
        // Folded, the header still shows the site name and the title.
        compose.onNodeWithText("example wiki").assertIsDisplayed()
        compose.onNodeWithText("Release notes: v0.0.3").assertIsDisplayed()

        compose.onNodeWithText("example wiki").performClick()
        compose.waitForIdle()
        assert(!collapsed.value) { "the second header click should have unfolded the card" }
        compose.onNodeWithText("Adds a live gateway", substring = true).assertIsDisplayed()
    }

    @Test fun `header chevron folds a rich post away`() {
        val collapsed = mutableStateOf(false)
        setCard(richPost, collapsed)
        compose.onNodeWithText("Open original").assertIsDisplayed()

        compose.onNodeWithText("X").performClick()
        compose.waitForIdle()
        compose.onAllNodesWithText("Open original").fetchSemanticsNodes().let {
            assert(it.isEmpty()) { "the folded post should hide its body, found ${it.size} 'Open original' nodes" }
        }
        compose.onNodeWithText("X").assertIsDisplayed()
    }

    @Test fun `link card title opens the embed URL`() {
        val collapsed = mutableStateOf(false)
        setCard(linkCard, collapsed)
        compose.onNodeWithText("Release notes: v0.0.3").performClick()
        compose.settle { shadowOf(compose.activity).peekNextStartedActivity() != null }
        val started = shadowOf(compose.activity).peekNextStartedActivity()
        assertNotNull("expected an activity to be started for the link", started)
        assertEquals(android.content.Intent.ACTION_VIEW, started!!.action)
        assertEquals(linkCard.url, started.dataString)
    }

    @Test fun `rich post Open original opens the post URL`() {
        val collapsed = mutableStateOf(false)
        setCard(richPost, collapsed)
        compose.onNodeWithText("Open original").performClick()
        compose.settle { shadowOf(compose.activity).peekNextStartedActivity() != null }
        val started = shadowOf(compose.activity).peekNextStartedActivity()
        assertNotNull("expected an activity to be started for the post", started)
        assertEquals(android.content.Intent.ACTION_VIEW, started!!.action)
        assertEquals(richPost.url, started.dataString)
    }
}
