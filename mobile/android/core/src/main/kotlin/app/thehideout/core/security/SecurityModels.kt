package app.thehideout.core.security

import java.time.Instant

/** The caller's security overview (`GetSecurity`), for the Security page's section summaries. */
data class SecurityInfo(
    val totpEnabled: Boolean,
    val hasPassword: Boolean,
    val passkeyCount: Int,
    val recoveryCodesRemaining: Int,
    val emailVerified: Boolean,
)

/** A WebAuthn credential on the caller's account (`Passkey`). */
data class Passkey(val id: Long, val name: String, val createdAt: Instant, val lastUsedAt: Instant? = null)

/** One of the caller's signed-in sessions, a.k.a. a device (`Session` in auth_service.proto). */
data class DeviceSession(
    val id: Long,
    val deviceName: String,
    val createdAt: Instant,
    val lastSeenAt: Instant? = null,
    val current: Boolean = false,
)

/** `BeginTotpSetup`'s answer: render [otpauthUri] as a QR code, show [secret] for manual entry. */
data class TotpSetup(val otpauthUri: String, val secret: String)
