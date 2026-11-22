package app.thehideout.core.model

import java.time.Duration

/** What the gateway tells the store: the connection's life and the events on it. */
sealed interface GatewayUpdate {
    data object Connecting : GatewayUpdate
    data object Connected : GatewayUpdate
    /** The stream ended or went silent; the next attempt is in [retryIn]. Events may have been missed. */
    data class Disconnected(val retryIn: Duration) : GatewayUpdate
    /** The server no longer accepts the token. */
    data object AuthExpired : GatewayUpdate
    data class Event(val event: GatewayEvent) : GatewayUpdate
}
