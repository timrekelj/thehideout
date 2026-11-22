package app.thehideout.core.api

import com.connectrpc.Idempotency
import com.connectrpc.MethodSpec
import com.connectrpc.StreamType
import com.thehideout.v1.GetMeRequest
import com.thehideout.v1.GetMeResponse
import com.thehideout.v1.SubscribeRequest
import com.thehideout.v1.SubscribeResponse
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import kotlin.time.Duration.Companion.seconds

class ConnectApiClientTest {
    @Test
    fun `unary calls keep the library's 10s default`() {
        val spec = MethodSpec(
            path = "/thehideout.v1.AuthService/GetMe",
            requestClass = GetMeRequest::class,
            responseClass = GetMeResponse::class,
            streamType = StreamType.UNARY,
            idempotency = Idempotency.UNKNOWN,
        )
        assertEquals(10.seconds, apiTimeoutOracle(spec))
    }

    @Test
    fun `the gateway's server stream has no library-imposed timeout`() {
        val spec = MethodSpec(
            path = "/thehideout.v1.GatewayService/Subscribe",
            requestClass = SubscribeRequest::class,
            responseClass = SubscribeResponse::class,
            streamType = StreamType.SERVER,
            idempotency = Idempotency.UNKNOWN,
        )
        assertEquals(null, apiTimeoutOracle(spec))
    }
}
