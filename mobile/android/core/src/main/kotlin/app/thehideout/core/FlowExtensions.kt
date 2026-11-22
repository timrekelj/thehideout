package app.thehideout.core

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first

/**
 * The flow's first real emission, falling back to [default] when it is empty. Always suspends for
 * that emission: unlike reading the `.value` of a `stateIn(scope, SharingStarted.Eagerly, default)`
 * StateFlow synchronously right after construction, which can observe the seeded [default] before
 * the source flow (e.g. a DataStore read) has produced anything at all.
 */
suspend fun Flow<String>.firstOrDefault(default: String): String = first().ifEmpty { default }
