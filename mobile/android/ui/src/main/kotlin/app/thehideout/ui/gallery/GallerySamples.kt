package app.thehideout.ui.gallery

import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.model.*
import java.time.Instant

/** Static sample data for the gallery and previews: the same people as the fake world, no coroutines needed. */
internal object GallerySamples {
    private val base: Instant = FakeBackend.DEFAULT_NOW
    private val joined = Instant.parse("2026-01-05T12:00:00Z")

    val tim = User(FakeBackend.TIM, "tim", joined)
    val ana = User(FakeBackend.ANA, "ana", joined)
    val bruno = User(FakeBackend.BRUNO, "bruno", joined)
    val cvetka = User(FakeBackend.CVETKA, "cvetka", joined)
    val people = listOf(tim, ana, bruno, cvetka)

    private fun m(id: Long, author: User, text: String, minutes: Long, vararg extra: (Message) -> Message) =
        extra.fold(Message(id, FakeBackend.GENERAL, FakeBackend.HIDEOUT, author.id, text, base.minusSeconds(minutes * 60))) { acc, f -> f(acc) }

    val messages = listOf(
        m(1, ana, "Release notes are up: **v0.0.3** adds a live gateway, ~~polling~~ and *much* better reconnects.", 42, { it.copy(reactions = listOf(Reaction(ReactionEmoji.Unicode("🎉"), 3, true), Reaction(ReactionEmoji.Unicode("🚀"), 1, false))) }),
        m(2, ana, "Draft is in [the wiki](https://example.com/wiki/release). Plan for Friday:\n- cut the release\n- update the `README`\n- tell @tim", 41),
        m(3, bruno, "Looks right to me. Is `backoff_ms` capped?", 36),
        m(4, tim, "Yep, capped at 30s.\n```rust\nlet delay = (base << attempt).min(30_000);\n```", 33, { it.copy(editedAt = base.minusSeconds(31 * 60)) }),
        m(5, cvetka, "> staging disk is at 80%\nThat is the first thing to fix. ||it was the old snapshots||", 19, {
            it.copy(
                embeds = listOf(Embed("https://example.com/changelog", "Changelog", "Everything that changed in v0.0.3, in one place.", "example.com")),
            )
        }),
    )

    val authors: Map<Long, User> = people.associateBy { it.id }

    val hideout = Guild(FakeBackend.HIDEOUT, "The Hideout", FakeBackend.TIM, "HIDEOUT1", joined)

    fun channel(id: Long, name: String, color: String = "", kind: ChannelKind = ChannelKind.Text) =
        Channel(id, FakeBackend.HIDEOUT, name, joined, color = color, kind = kind)

    val channelGeneral = channel(10, "general", "teal")
    val channelDev = channel(11, "dev", "blue")
    val channelDesign = channel(15, "design", "pink")
    val channelBooks = channel(19, "books", "slate")
    val channelLounge = channel(22, "lounge", kind = ChannelKind.Voice)
    val loungeStates = listOf(
        VoiceState(FakeBackend.HIDEOUT, 22, FakeBackend.ANA, joinedAt = base),
        VoiceState(FakeBackend.HIDEOUT, 22, FakeBackend.BRUNO, selfMute = true, joinedAt = base),
    )
}
