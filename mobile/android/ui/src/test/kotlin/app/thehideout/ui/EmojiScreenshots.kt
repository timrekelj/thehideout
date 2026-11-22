package app.thehideout.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.thehideout.core.model.Emoji
import app.thehideout.core.model.Message
import app.thehideout.core.model.Reaction
import app.thehideout.core.model.ReactionEmoji
import app.thehideout.core.model.User
import app.thehideout.ui.components.EmojiPickerSheet
import app.thehideout.ui.components.LocalMediaBase
import app.thehideout.ui.components.MessageRow
import app.thehideout.ui.components.MessageUi
import app.thehideout.ui.theme.Hideout
import java.time.Instant
import org.junit.Test
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

// The same three guild emoji the fake backend seeds into The Hideout (FakeBackend.seedEmoji, MSG-09).
private const val EMOJI = "file:///android_asset/fake_emoji/"

private val wave = Emoji(9001, 1, "wave", animated = false, image = "${EMOJI}wave.webp", creatorId = 2, createdAt = Instant.parse("2026-09-24T10:00:00Z"))
private val thumbsUp = Emoji(9002, 1, "thumbs_up", animated = false, image = "${EMOJI}thumbs_up.webp", creatorId = 3, createdAt = Instant.parse("2026-09-29T10:00:00Z"))
private val partyParrot = Emoji(9003, 1, "party_parrot", animated = true, image = "${EMOJI}party_parrot.gif", creatorId = 4, createdAt = Instant.parse("2026-10-14T10:00:00Z"))
private val serverEmojis = listOf(wave, thumbsUp, partyParrot)

private fun row(content: String, reactions: List<Reaction> = emptyList()) = MessageUi(
    Message(1, 100, 1, 2, content, Instant.parse("2026-11-22T10:00:00Z"), reactions = reactions),
    User(2, "ana", Instant.parse("2026-01-01T00:00:00Z")),
    head = true,
    me = "tim",
)

// Short window: the cards are well under 640dp tall, and a smaller capture buffer keeps the
// single test worker's heap (shared with the huge gallery snapshots) from tipping over.
private const val PHONE_Q = "w411dp-h640dp-xxhdpi"

/** Dedicated custom-emoji previews (MSG-09, MD-06/07): inline in a message row (with a custom
 *  reaction chip), a jumbo-only message, and the picker with this server's emoji, each in dark
 *  and light. */
class EmojiScreenshots : ScreenshotTest() {
    @Composable
    private fun Frame(content: @Composable () -> Unit) {
        val c = Hideout.colors
        Box(Modifier.background(c.bg).padding(16.dp).width(340.dp)) { content() }
    }

    // Coil's fetch of the bundled asset hops onto a real IO thread and posts back to the paused
    // main looper; poll both so the capture doesn't race the emoji pictures (same as EmbedScreenshots).
    private fun settle() {
        repeat(50) {
            compose.waitForIdle()
            shadowOf(android.os.Looper.getMainLooper()).idle()
            Thread.sleep(20)
        }
    }

    private fun runRow(screen: String, variant: Variant, ui: app.thehideout.ui.components.MessageUi) {
        compose.setThemed(variant) {
            CompositionLocalProvider(LocalMediaBase provides "") {
                Frame { MessageRow(ui, serverEmojis = serverEmojis) }
            }
        }
        settle()
        compose.capture(screen, Device.Phone, variant)
    }

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun inlineDark() = runRow(
        "emoji-inline",
        Variant.Dark,
        row(
            "nice work team <:thumbs_up:9002> see you at standup <:wave:9001>",
            reactions = listOf(Reaction(ReactionEmoji.Custom(9001), 2, me = true), Reaction(ReactionEmoji.Unicode("🚀"), 1, me = false)),
        ),
    )

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun inlineLight() = runRow(
        "emoji-inline",
        Variant.Light,
        row(
            "nice work team <:thumbs_up:9002> see you at standup <:wave:9001>",
            reactions = listOf(Reaction(ReactionEmoji.Custom(9001), 2, me = true), Reaction(ReactionEmoji.Unicode("🚀"), 1, me = false)),
        ),
    )

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun jumboDark() = runRow("emoji-jumbo", Variant.Dark, row("<a:party_parrot:9003> <:wave:9001> 🎉"))
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun jumboLight() = runRow("emoji-jumbo", Variant.Light, row("<a:party_parrot:9003> <:wave:9001> 🎉"))

    private fun runPicker(screen: String, variant: Variant) {
        compose.setThemed(variant) {
            CompositionLocalProvider(LocalMediaBase provides "") {
                EmojiPickerSheet(onDismiss = {}, onPick = {}, serverEmojis = serverEmojis)
            }
        }
        settle()
        compose.capture(screen, Device.Phone, variant)
    }

    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun pickerDark() = runPicker("emoji-picker-sheet", Variant.Dark)
    @Config(sdk = [35], qualifiers = PHONE_Q) @Test fun pickerLight() = runPicker("emoji-picker-sheet", Variant.Light)
}
