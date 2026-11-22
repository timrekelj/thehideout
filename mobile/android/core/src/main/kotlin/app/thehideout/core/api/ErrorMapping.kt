package app.thehideout.core.api

import app.thehideout.core.model.toInstant
import com.connectrpc.Code
import com.connectrpc.ConnectException

private const val CODE_ERROR = "thehideout.v1.CodeError"
private const val ACCOUNT_SUSPENDED = "thehideout.v1.AccountSuspended"
private const val OWNED_GUILDS = "thehideout.v1.OwnedGuilds"
private const val TERMS_REQUIRED = "you must accept the terms"
private const val INVITE_REQUIRED = "invite required"
private const val CLIENT_TOO_OLD = "client too old"

/** One `google.protobuf.Any` of an error: its type name and its serialized message. */
class ErrorDetail(val type: String, val payload: ByteArray)

fun ConnectException.toAppError(): AppError =
    mapError(code, message, details.map { ErrorDetail(it.type, it.payload.toByteArray()) })

/**
 * The twin of desktop's `From<ConnectError> for ApiError`: typed details (`CodeError`,
 * `AccountSuspended`, `OwnedGuilds`) win over the status code, which decides the rest.
 */
fun mapError(code: Code, message: String?, details: List<ErrorDetail> = emptyList()): AppError {
    detailError(details)?.let { return it }
    val msg = message.orEmpty()
    return when (code) {
        Code.UNAUTHENTICATED -> AppError.Unauthenticated
        Code.INVALID_ARGUMENT ->
            if (msg.startsWith(TERMS_REQUIRED)) AppError.TermsUpdateRequired else AppError.InvalidArgument(msg)
        Code.OUT_OF_RANGE -> AppError.InvalidArgument(msg)
        Code.NOT_FOUND -> AppError.NotFound
        Code.PERMISSION_DENIED -> AppError.PermissionDenied
        Code.ABORTED -> AppError.Conflict
        Code.UNIMPLEMENTED -> AppError.Unimplemented
        Code.RESOURCE_EXHAUSTED -> AppError.RateLimited
        Code.ALREADY_EXISTS -> AppError.AlreadyExists
        Code.FAILED_PRECONDITION -> when {
            msg == INVITE_REQUIRED -> AppError.InviteRequired
            msg.lowercase().startsWith(CLIENT_TOO_OLD) -> AppError.ClientTooOld
            else -> AppError.Precondition(msg)
        }
        Code.UNAVAILABLE, Code.DEADLINE_EXCEEDED -> AppError.Unavailable(msg)
        else -> AppError.Other(msg)
    }
}

private fun detailError(details: List<ErrorDetail>): AppError? {
    fun find(name: String) = details.firstOrNull { it.type.removePrefix("type.googleapis.com/") == name }
    find(CODE_ERROR)?.let { d ->
        val e = runCatching { com.thehideout.v1.CodeError.parseFrom(d.payload) }.getOrNull()
        if (e != null) when (e.reason) {
            com.thehideout.v1.CodeErrorReason.CODE_ERROR_REASON_WRONG -> return AppError.WrongCode(e.attemptsRemaining)
            com.thehideout.v1.CodeErrorReason.CODE_ERROR_REASON_EXPIRED -> return AppError.CodeExpired
            com.thehideout.v1.CodeErrorReason.CODE_ERROR_REASON_TOO_MANY_ATTEMPTS -> return AppError.TooManyAttempts
            else -> Unit
        }
    }
    find(ACCOUNT_SUSPENDED)?.let { d ->
        runCatching { com.thehideout.v1.AccountSuspended.parseFrom(d.payload) }.getOrNull()?.let {
            return AppError.Suspended(it.reason, if (it.hasUntil()) it.until.toInstant() else null)
        }
    }
    find(OWNED_GUILDS)?.let { d ->
        runCatching { com.thehideout.v1.OwnedGuilds.parseFrom(d.payload) }.getOrNull()?.let { og ->
            return AppError.OwnsGuilds(og.guildsList.map { OwnedGuild(it.id, it.name, it.memberCount) })
        }
    }
    return null
}
