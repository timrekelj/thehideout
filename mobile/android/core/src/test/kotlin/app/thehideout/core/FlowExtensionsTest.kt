package app.thehideout.core

import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class FlowExtensionsTest {
    @Test
    fun `firstOrDefault awaits the real value, unlike an eagerly seeded StateFlow's value`() = runTest {
        val source = flow {
            delay(10)
            emit("http://127.0.0.1:8080")
        }
        // The exact shape of AppViewModel.init's bug: a StateFlow seeded with the default before
        // the source (a DataStore read) has emitted. Reading `.value` synchronously right after
        // construction observes the stale seed, not the persisted value.
        val seeded = source.stateIn(backgroundScope, SharingStarted.Eagerly, "http://10.0.2.2:8080")
        assertEquals("http://10.0.2.2:8080", seeded.value)

        assertEquals("http://127.0.0.1:8080", source.firstOrDefault("http://10.0.2.2:8080"))
    }

    @Test
    fun `firstOrDefault falls back to default when the first value is empty`() = runTest {
        assertEquals("http://10.0.2.2:8080", flowOf("").firstOrDefault("http://10.0.2.2:8080"))
    }
}
