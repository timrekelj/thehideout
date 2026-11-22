package app.thehideout.core.auth

import app.thehideout.core.api.Session
import app.thehideout.core.model.InviteStatus
import app.thehideout.core.model.User
import java.time.Instant

/** How a second-factor challenge can be answered (the proto's `SecondFactorMethod`). */
enum class SecondFactorMethod { Unspecified, Totp, RecoveryCode, Passkey }

/**
 * A WebAuthn ceremony the server started (`PasskeyCeremony`): [optionsJson] is the
 * `CredentialCreationOptions` / `CredentialRequestOptions` JSON (base64url binary fields), exactly
 * what Credential Manager's `CreatePublicKeyCredentialRequest` / `GetPublicKeyCredentialOption`
 * take; [ceremonyUrl] is the server's page that does the same in a browser.
 */
data class PasskeyCeremony(
    val ceremonyId: String,
    val ceremonyUrl: String,
    val optionsJson: String,
)

/** What an invite is for, before accepting it (`LookupInvite`). */
data class InvitePreview(
    val maskedEmail: String,
    val inviter: User?,
    val guildNames: List<String>,
    val expiresAt: Instant,
    val status: InviteStatus,
    val termsVersion: String,
    val minimumAge: Int,
)

/**
 * The answer of `FinishPasskeyLogin` / `FinishSecondFactorPasskey`: a session, or [pending] while
 * the ceremony page (opened in a browser) has no result yet — poll again with an empty credential.
 */
data class PasskeyFinish(val session: Session?, val pending: Boolean)

/** The answer of `FinishPasskeyRegistration`: the added passkey's name, or [pending]. */
data class PasskeyRegistered(val name: String, val pending: Boolean)
