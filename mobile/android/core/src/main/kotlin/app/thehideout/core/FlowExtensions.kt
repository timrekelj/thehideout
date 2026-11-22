package app.thehideout.core

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first

/**
 * The flow's first emission, or [default] if that emission is the empty string (e.g. `ServerStore`'s
 * persisted-but-never-set server URL). Always suspends for that emission: unlike reading the
 * `.value` of a `stateIn(scope, SharingStarted.Eagerly, default)` StateFlow synchronously right
 * after construction, which can observe the seeded [default] before the source flow (e.g. a
 * DataStore read) has produced anything at all. The source flow itself must emit at least once;
 * one that completes without emitting makes `first()` throw `NoSuchElementException`.
 */
suspend fun Flow<String>.firstOrBlankDefault(default: String): String = first().ifEmpty { default }
