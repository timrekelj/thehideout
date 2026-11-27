package app.thehideout.ui.auth

import android.content.Context
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import androidx.credentials.CreatePublicKeyCredentialRequest
import androidx.credentials.CredentialManager
import androidx.credentials.GetCredentialRequest
import androidx.credentials.GetPublicKeyCredentialOption
import androidx.credentials.PublicKeyCredential
import androidx.credentials.exceptions.CreateCredentialCancellationException
import androidx.credentials.exceptions.CreateCredentialException
import androidx.credentials.exceptions.GetCredentialException
import app.thehideout.core.auth.PasskeyCeremony

/** How a native passkey-creation attempt (`PasskeyLauncher.createCredential`) came back. */
sealed interface PasskeyCreateResult {
    data class Success(val json: String) : PasskeyCreateResult
    /** The user backed out of the system UI: stop, don't fall back to the browser ceremony. */
    data object Cancelled : PasskeyCreateResult
    /** No passkey provider, or this RP id isn't associated with the app yet: fall back to the browser ceremony. */
    data object Unsupported : PasskeyCreateResult
}

/**
 * Runs a WebAuthn ceremony natively through Credential Manager, or in a Custom Tab when the server's
 * host cannot have native passkeys.
 *
 * Native passkeys need the RP id (the host of the server URL) to be a real domain that serves
 * `/.well-known/assetlinks.json` for the app's signing cert — the server must host that file with the
 * SHA-256 fingerprint of the signing key (see mobile/android/README.md). For non-domain hosts
 * (localhost, 10.0.2.2, an IP), the server's `ceremony_url` is opened in a Custom Tab instead and the
 * caller polls `Finish*` with an empty credential every 1.5 s: the page's redirect is a 127.0.0.1
 * loopback, unusable as a callback from the browser.
 */
class PasskeyLauncher(private val context: Context) {
    private val manager = CredentialManager.create(context)

    /** Signs in (or answers a challenge) with a passkey: the `PublicKeyCredential` JSON of the ceremony. */
    suspend fun getCredential(ceremony: PasskeyCeremony): String? {
        val request = GetCredentialRequest(listOf(GetPublicKeyCredentialOption(ceremony.optionsJson)))
        return try {
            val result = manager.getCredential(context, request)
            (result.credential as? PublicKeyCredential)?.authenticationResponseJson
        } catch (e: GetCredentialException) {
            null // no passkey for this RP id on the device: fall back to the ceremony page
        }
    }

    /** Adds a passkey: the `PublicKeyCredential` JSON of the registration ceremony, or why not. */
    suspend fun createCredential(ceremony: PasskeyCeremony): PasskeyCreateResult {
        return try {
            val result = manager.createCredential(context, CreatePublicKeyCredentialRequest(ceremony.optionsJson))
            val json = (result as? androidx.credentials.CreatePublicKeyCredentialResponse)?.registrationResponseJson
            if (json != null) PasskeyCreateResult.Success(json) else PasskeyCreateResult.Unsupported
        } catch (e: CreateCredentialCancellationException) {
            PasskeyCreateResult.Cancelled
        } catch (e: CreateCredentialException) {
            PasskeyCreateResult.Unsupported
        }
    }

    /** Opens the server's ceremony page in a Custom Tab (the fallback for non-domain hosts). */
    fun openCeremonyTab(ceremony: PasskeyCeremony) {
        runCatching { CustomTabsIntent.Builder().build().launchUrl(context, Uri.parse(ceremony.ceremonyUrl)) }
    }

    companion object {
        /** Whether the server's host can have native passkeys (a domain, not localhost / 10.0.2.2 / an IP). */
        fun isDomainHost(serverUrl: String): Boolean {
            val host = Uri.parse(serverUrl)?.host ?: return false
            if (host == "localhost" || host.endsWith(".localhost")) return false
            if (host == "10.0.2.2" || host == "127.0.0.1") return false
            if (host.all { it.isDigit() || it == '.' }) return false
            if (host.contains(':')) return false // IPv6 literal
            return host.contains('.')
        }
    }
}
