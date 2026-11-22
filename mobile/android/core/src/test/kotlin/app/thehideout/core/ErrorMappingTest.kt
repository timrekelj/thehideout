package app.thehideout.core

import app.thehideout.core.api.AppError
import app.thehideout.core.api.ErrorDetail
import app.thehideout.core.api.OwnedGuild
import app.thehideout.core.api.mapError
import com.connectrpc.Code
import com.google.protobuf.Timestamp
import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test
import java.time.Instant

class ErrorMappingTest {
    @Test fun `status codes map as desktop does`() {
        assertEquals(AppError.Unauthenticated, mapError(Code.UNAUTHENTICATED, "x"))
        assertEquals(AppError.InvalidArgument("bad"), mapError(Code.INVALID_ARGUMENT, "bad"))
        assertEquals(AppError.InvalidArgument("range"), mapError(Code.OUT_OF_RANGE, "range"))
        assertEquals(AppError.NotFound, mapError(Code.NOT_FOUND, "x"))
        assertEquals(AppError.PermissionDenied, mapError(Code.PERMISSION_DENIED, "x"))
        assertEquals(AppError.Conflict, mapError(Code.ABORTED, "x"))
        assertEquals(AppError.Unimplemented, mapError(Code.UNIMPLEMENTED, "x"))
        assertEquals(AppError.RateLimited, mapError(Code.RESOURCE_EXHAUSTED, "x"))
        assertEquals(AppError.AlreadyExists, mapError(Code.ALREADY_EXISTS, "x"))
        assertEquals(AppError.Unavailable("down"), mapError(Code.UNAVAILABLE, "down"))
        assertEquals(AppError.Unavailable("slow"), mapError(Code.DEADLINE_EXCEEDED, "slow"))
        assertEquals(AppError.Other("boom"), mapError(Code.INTERNAL_ERROR, "boom"))
        assertEquals(AppError.Other(""), mapError(Code.UNKNOWN, null))
    }

    @Test fun `failed preconditions that mean something`() {
        assertEquals(AppError.InviteRequired, mapError(Code.FAILED_PRECONDITION, "invite required"))
        assertEquals(AppError.ClientTooOld, mapError(Code.FAILED_PRECONDITION, "Client too old: need 0.2.0"))
        assertEquals(AppError.Precondition("nope"), mapError(Code.FAILED_PRECONDITION, "nope"))
        assertEquals(AppError.NotFound, mapError(Code.NOT_FOUND, "client too old"), "only FAILED_PRECONDITION, and only that prefix")
    }

    @Test fun `unaccepted terms are told apart from a bad request`() {
        assertEquals(AppError.TermsUpdateRequired, mapError(Code.INVALID_ARGUMENT, "you must accept the terms v2"))
    }

    private fun detail(type: String, msg: com.google.protobuf.MessageLite) = ErrorDetail("type.googleapis.com/$type", msg.toByteArray())

    @Test fun `code error details carry the attempts left`() {
        val wrong = com.thehideout.v1.CodeError.newBuilder().setReason(com.thehideout.v1.CodeErrorReason.CODE_ERROR_REASON_WRONG).setAttemptsRemaining(3).build()
        assertEquals(AppError.WrongCode(3), mapError(Code.INVALID_ARGUMENT, "boom", listOf(detail("thehideout.v1.CodeError", wrong))))
        val expired = com.thehideout.v1.CodeError.newBuilder().setReason(com.thehideout.v1.CodeErrorReason.CODE_ERROR_REASON_EXPIRED).build()
        assertEquals(AppError.CodeExpired, mapError(Code.FAILED_PRECONDITION, "boom", listOf(detail("thehideout.v1.CodeError", expired))))
        val many = com.thehideout.v1.CodeError.newBuilder().setReason(com.thehideout.v1.CodeErrorReason.CODE_ERROR_REASON_TOO_MANY_ATTEMPTS).build()
        assertEquals(AppError.TooManyAttempts, mapError(Code.RESOURCE_EXHAUSTED, "boom", listOf(detail("thehideout.v1.CodeError", many))))
        val unknown = com.thehideout.v1.CodeError.getDefaultInstance()
        assertEquals(AppError.InvalidArgument("boom"), mapError(Code.INVALID_ARGUMENT, "boom", listOf(detail("thehideout.v1.CodeError", unknown))), "an unknown reason falls back to the code")
        assertEquals("wrong code, 1 try left", AppError.WrongCode(1).message)
        assertFalse(AppError.TooManyAttempts.isTransient)
        assertTrue(AppError.RateLimited.isTransient)
    }

    @Test fun `lifecycle details`() {
        val until = Instant.ofEpochSecond(1_800_000_000)
        val susp = com.thehideout.v1.AccountSuspended.newBuilder().setReason("spam").setUntil(Timestamp.newBuilder().setSeconds(until.epochSecond)).build()
        assertEquals(AppError.Suspended("spam", until), mapError(Code.PERMISSION_DENIED, "x", listOf(detail("thehideout.v1.AccountSuspended", susp))))
        val forever = com.thehideout.v1.AccountSuspended.newBuilder().setReason("").build()
        assertEquals(AppError.Suspended("", null), mapError(Code.PERMISSION_DENIED, "x", listOf(detail("thehideout.v1.AccountSuspended", forever))))
        val owned = com.thehideout.v1.OwnedGuilds.newBuilder().addGuilds(com.thehideout.v1.OwnedGuild.newBuilder().setId(7).setName("Den").setMemberCount(4)).build()
        assertEquals(AppError.OwnsGuilds(listOf(OwnedGuild(7, "Den", 4))), mapError(Code.FAILED_PRECONDITION, "x", listOf(detail("thehideout.v1.OwnedGuilds", owned))))
        assertEquals("transfer or delete the servers you own first: Den", AppError.OwnsGuilds(listOf(OwnedGuild(7, "Den", 4))).message)
    }

    @Test fun `a garbled detail falls back to the code`() {
        assertEquals(AppError.NotFound, mapError(Code.NOT_FOUND, "x", listOf(ErrorDetail("thehideout.v1.CodeError", byteArrayOf(0x7f, 0x01)))))
    }
}
