package app.thehideout.core.api

import java.time.Instant

data class OwnedGuild(val id: Long, val name: String, val memberCount: Int)

/** Every way a call can fail, as the UI needs to tell them apart (desktop's `ApiError`). */
sealed class AppError(message: String) : Exception(message) {
    data object Unauthenticated : AppError("not signed in or invalid credentials")
    data class WrongCode(val attemptsRemaining: Int) : AppError(
        if (attemptsRemaining == 1) "wrong code, 1 try left" else "wrong code, $attemptsRemaining tries left",
    )
    data object CodeExpired : AppError("expired, start again")
    data object TooManyAttempts : AppError("too many wrong attempts, start again")
    data class InvalidArgument(val detail: String) : AppError("invalid request: $detail")
    data object NotFound : AppError("not found")
    data object PermissionDenied : AppError("permission denied")
    data object Conflict : AppError("changed by someone else, refresh and try again")
    data object Unimplemented : AppError("not supported by this server")
    data object RateLimited : AppError("too many attempts, try again later")
    data object AlreadyExists : AppError("already exists")
    data object InviteRequired : AppError("an invite is required to create an account")
    data class Precondition(val detail: String) : AppError(if (detail.isEmpty()) "not allowed right now" else "not allowed: $detail")
    data object TooLarge : AppError("file is too large")
    data object ClientTooOld : AppError("this version of thehideout is too old, update required")
    data class Suspended(val reason: String, val until: Instant?) : AppError(
        if (reason.isEmpty()) "your account is suspended" else "your account is suspended: $reason",
    )
    data object TermsUpdateRequired : AppError("the terms must be accepted first")
    data class OwnsGuilds(val guilds: List<OwnedGuild>) : AppError(
        "transfer or delete the servers you own first: ${guilds.joinToString { it.name }}",
    )
    /** Unavailable / deadline exceeded / no connection. */
    data class Unavailable(val detail: String) : AppError("network error: $detail")
    data class Other(val detail: String) : AppError(detail)

    /** Worth retrying silently (desktop's `is_transient`). */
    val isTransient: Boolean get() = this is Unavailable || this is RateLimited
}
