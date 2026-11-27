package app.thehideout.ui

import app.thehideout.core.InMemoryTokenStore
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.model.Ban
import app.thehideout.core.model.User
import app.thehideout.core.store.Action
import app.thehideout.core.store.AppState
import app.thehideout.core.store.AppStore
import app.thehideout.core.store.Reducer
import app.thehideout.ui.shell.ServerSettingsPage
import app.thehideout.ui.shell.ServerSettingsScreen
import app.thehideout.ui.shell.ShellActions
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.runBlocking
import org.junit.Test
import org.robolectric.annotation.Config
import java.time.Instant

/** The world for the server settings screens: The Hideout with its roles, members and emoji loaded, plus one ban. */
private fun settingsState(): AppState = runBlocking {
    val fake = FakeBackend()
    var s = fakeWorldState()
    fun AppState.reduce(a: Action) = Reducer.reduce(this, a).state
    s = s.reduce(Action.EmojisLoaded(FakeBackend.HIDEOUT, Result.success(fake.listEmojis(FakeBackend.HIDEOUT))))
    s = s.reduce(
        Action.BansLoaded(
            FakeBackend.HIDEOUT,
            Result.success(listOf(Ban(User(9001, "troll", Instant.parse("2026-01-01T00:00:00Z")), FakeBackend.TIM, "spamming links", Instant.parse("2026-11-10T12:00:00Z")))),
        ),
    )
    s
}

private fun fakeActions(): ShellActions {
    val store = AppStore(FakeBackend(), CoroutineScope(SupervisorJob() + Dispatchers.Unconfined), InMemoryTokenStore())
    return ShellActions.of(store)
}

/** Server settings (SSET-*), every page, phone and tablet, dark and light. */
abstract class ServerSettingsScreenshots(private val device: Device) : ScreenshotTest() {
    private fun page(name: ServerSettingsPage, variant: Variant) = compose.snapshot("server-settings-${name.name.lowercase()}", device, variant) {
        ServerSettingsScreen(settingsState(), FakeBackend.HIDEOUT, fakeActions(), onDismiss = {}, initialPage = name)
    }

    @Test fun overviewDark() = page(ServerSettingsPage.Overview, Variant.Dark)
    @Test fun overviewLight() = page(ServerSettingsPage.Overview, Variant.Light)

    @Test fun rolesDark() = page(ServerSettingsPage.Roles, Variant.Dark)
    @Test fun rolesLight() = page(ServerSettingsPage.Roles, Variant.Light)

    @Test fun membersDark() = page(ServerSettingsPage.Members, Variant.Dark)
    @Test fun membersLight() = page(ServerSettingsPage.Members, Variant.Light)

    @Test fun bansDark() = page(ServerSettingsPage.Bans, Variant.Dark)
    @Test fun bansLight() = page(ServerSettingsPage.Bans, Variant.Light)

    @Test fun emojisDark() = page(ServerSettingsPage.Emojis, Variant.Dark)
    @Test fun emojisLight() = page(ServerSettingsPage.Emojis, Variant.Light)
}

@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class ServerSettingsPhoneScreenshots : ServerSettingsScreenshots(Device.Phone)

@Config(sdk = [35], qualifiers = "w1280dp-h800dp-xhdpi")
class ServerSettingsTabletScreenshots : ServerSettingsScreenshots(Device.Tablet)
