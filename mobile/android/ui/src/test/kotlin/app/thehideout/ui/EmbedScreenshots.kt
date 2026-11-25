package app.thehideout.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.Embed
import app.thehideout.core.model.EmbedKind
import app.thehideout.core.model.EmbedMedia
import app.thehideout.core.model.EmbedMediaKind
import app.thehideout.ui.components.EmbedCard
import app.thehideout.ui.components.LocalMediaBase
import app.thehideout.ui.theme.Hideout
import java.time.Instant
import org.junit.Test
import org.robolectric.annotation.Config

private const val MEDIA = "file:///android_asset/fake_media/"

// The same shapes the fake backend seeds into #design (FakeBackend.seedEmbeds, ATT-06).
private val linkCard = Embed(
    url = "https://example.com/wiki/release",
    title = "Release notes: v0.0.3",
    description = "Adds a live gateway, drops polling in favor of push updates, and tightens the reconnect backoff so flaky networks recover faster without hammering the server.",
    siteName = "example wiki",
    thumbnail = "$MEDIA/landscape-3.jpg",
    thumbnailWidth = 1200,
    thumbnailHeight = 800,
    kind = EmbedKind.Link,
)

private val richImagePost = Embed(
    url = "https://x.example.com/cvetka/status/1",
    provider = "x",
    authorName = "cvetka",
    authorIcon = "$MEDIA/square-1.jpg",
    description = "Shipped the new icon set today. Been a long couple of weeks but it's finally in the wiki.",
    kind = EmbedKind.Rich,
    media = listOf(EmbedMedia(kind = EmbedMediaKind.Image, file = "$MEDIA/landscape-4.jpg", width = 1200, height = 800)),
    publishedAt = Instant.parse("2026-11-18T04:00:00Z"),
)

private val richVideoPost = Embed(
    url = "https://tiktok.example.com/@ana/video/1",
    provider = "tiktok",
    authorName = "ana",
    description = "weekend hike recap #trail",
    kind = EmbedKind.Rich,
    media = listOf(EmbedMedia(kind = EmbedMediaKind.Video, file = "$MEDIA/clip-1.mp4", poster = "$MEDIA/clip-1-poster.jpg", width = 960, height = 540, durationMs = 6_000, size = 590_746)),
    publishedAt = Instant.parse("2026-11-18T03:56:00Z"),
)

// Short window: the cards are at most ~460dp tall, and a smaller capture buffer keeps the
// single test worker's heap (shared with the huge gallery snapshots) from tipping over.
private const val PHONE_Q = "w411dp-h640dp-xxhdpi"

/** Dedicated link-embed previews (ATT-06/07): an open and a folded basic card, and two rich posts
 *  (image and video media), each in dark and light. */
class EmbedScreenshots : ScreenshotTest() {
    @Composable
    private fun Frame(content: @Composable () -> Unit) {
        val c = Hideout.colors
        Box(Modifier.background(c.bg).padding(16.dp).width(340.dp)) { content() }
    }

    private fun run(screen: String, variant: Variant, collapsed: Boolean = false, content: @Composable () -> Embed) {
        compose.setThemed(variant) {
            CompositionLocalProvider(LocalMediaBase provides "") {
                Frame { EmbedCard(content(), collapsed = collapsed, onToggleCollapsed = {}) }
            }
        }
        compose.capture(screen, Device.Phone, variant)
    }

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun linkOpenDark() = run("embed-link-open", Variant.Dark) { linkCard }
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun linkOpenLight() = run("embed-link-open", Variant.Light) { linkCard }
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun linkFoldedDark() = run("embed-link-folded", Variant.Dark, collapsed = true) { linkCard }
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun linkFoldedLight() = run("embed-link-folded", Variant.Light, collapsed = true) { linkCard }
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun richImageDark() = run("embed-rich-image", Variant.Dark) { richImagePost }
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun richImageLight() = run("embed-rich-image", Variant.Light) { richImagePost }
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun richVideoDark() = run("embed-rich-video", Variant.Dark) { richVideoPost }
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun richVideoLight() = run("embed-rich-video", Variant.Light) { richVideoPost }
}
