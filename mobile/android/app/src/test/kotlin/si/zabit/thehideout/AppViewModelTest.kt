package si.zabit.thehideout

import androidx.test.core.app.ApplicationProvider
import app.thehideout.core.api.ConnectApiClient
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import kotlinx.coroutines.withTimeout
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class AppViewModelTest {
    @Before fun setUp() {
        Dispatchers.setMain(Dispatchers.Unconfined)
    }

    @After fun tearDown() {
        Dispatchers.resetMain()
    }

    @Test fun `cold start opens the session against the persisted server, not the build default`() = runBlocking {
        val container = AppContainer(ApplicationProvider.getApplicationContext())
        container.server.save("http://127.0.0.1:9090")
        check(container.defaultServer() != "http://127.0.0.1:9090")

        val vm = AppViewModel(container, fake = false)
        withTimeout(5_000) { vm.session.filterNotNull().first() }

        val api = vm.session.value?.api as ConnectApiClient
        assertEquals("http://127.0.0.1:9090", api.baseUrl)
    }

    @Test fun `fake mode opens a session synchronously, with no DataStore round trip to await`() {
        val container = AppContainer(ApplicationProvider.getApplicationContext())

        val vm = AppViewModel(container, fake = true)

        assertEquals(true, vm.session.value != null)
    }
}
