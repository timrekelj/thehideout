package app.thehideout.core.auth

import app.thehideout.core.api.ApiClient
import app.thehideout.core.api.AppError
import app.thehideout.core.api.LoginResult
import app.thehideout.core.api.Session
import app.thehideout.core.model.InviteStatus
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter

/** The auth card's two top-level modes. */
enum class AuthMode { Login, Invite }

/** Where the auth card is within its mode (desktop's `AuthStep`). */
sealed interface AuthStep {
    /** Username or email, and the password. */
    data object Password : AuthStep
    /** "Email me a code instead": the email, then (`sent`) the 6-digit code. */
    data class EmailCode(val sent: Boolean) : AuthStep
    /** "Use a passkey": the username, then (`waiting`) the ceremony. */
    data class Passkey(val waiting: Boolean) : AuthStep
    /** "Forgot password?": the email, then (`sent`) the code and a new password. */
    data class Reset(val sent: Boolean) : AuthStep
    /** The account has two-factor authentication on. */
    data class SecondFactor(
        val challenge: String,
        val methods: Set<SecondFactorMethod>,
        val recovery: Boolean,
        val remaining: Int?,
        val locked: Boolean,
        val passkeyWaiting: Boolean,
    ) : AuthStep
    /** "I have an invite": the code (or the token of a link). */
    data object InviteCode : AuthStep
    /** The invite is valid: choose a username and a password. */
    data class InviteAccept(val preview: InvitePreview, val code: String) : AuthStep
}

/** Which field an error belongs under. */
enum class AuthField { Username, Password, Email, Code, Invite, Form }

/** Why and until when a suspended account is locked out. */
data class Suspension(val reason: String, val until: Instant?)

data class AuthError(val field: AuthField, val message: String, val suspension: Suspension? = null)

/**
 * The whole auth form as an immutable snapshot (desktop's `Auth`): the step, what is typed, what is
 * busy, the error under its field and the notice, the resend countdown and the passkey ceremony
 * under way. The UI renders it; [AuthPresenter] changes it.
 */
data class AuthUiState(
    val mode: AuthMode = AuthMode.Login,
    val step: AuthStep = AuthStep.Password,
    val username: String = "",
    val password: String = "",
    val email: String = "",
    val code: String = "",
    val recovery: String = "",
    val invite: String = "",
    val addPasskey: Boolean = false,
    val acceptTerms: Boolean = false,
    val confirmAge: Boolean = false,
    val busy: Boolean = false,
    val error: AuthError? = null,
    val notice: String? = null,
    /** Epoch millis when "Resend code" works again. */
    val resendAt: Long? = null,
    val serverOpen: Boolean = false,
    val serverDraft: String = "",
    /** The passkey ceremony to run (the waiting steps). */
    val ceremony: PasskeyCeremony? = null,
) {
    val topLevel: Boolean get() = error?.suspension == null && (step is AuthStep.Password || step is AuthStep.InviteCode)
    fun errorFor(field: AuthField): String? = error?.takeIf { it.field == field }?.message
}

/** What the auth screens can ask for. Plain methods (they launch their own coroutines), so screens are previewable and testable. */
interface AuthPresenter {
    val state: StateFlow<AuthUiState>
    fun setMode(mode: AuthMode) {}
    fun go(step: AuthStep) {}
    /** Predictive back between steps. */
    fun back() {}
    fun updateUsername(text: String) {}
    fun updatePassword(text: String) {}
    fun updateEmail(text: String) {}
    /** Digits only; a full code on an auto-submit step goes at once. */
    fun updateCode(text: String) {}
    fun updateRecovery(text: String) {}
    /** Auto-formats the dash; pasted links are rejected on submit. */
    fun updateInvite(text: String) {}
    fun toggleAddPasskey() {}
    fun toggleTerms() {}
    fun toggleAge() {}
    /** Enter or the primary button, whatever the step. */
    fun submit() {}
    fun resend() {}
    fun toggleRecovery() {}
    /** Finishes the ceremony under way with the credential's JSON, or "" to poll the ceremony page. */
    fun finishPasskey(ceremonyId: String, credentialJson: String) {}
    fun finishSecondFactorPasskey(ceremonyId: String, credentialJson: String) {}
    fun cancelPasskey() {}
    /** A `thehideout://invite/<token>` or `https://<server>/invite/<token>` link: straight to the invite. */
    fun openLink(url: String) {}
    fun dismissSuspension() {}
    fun setServerOpen(open: Boolean) {}
    fun updateServerDraft(text: String) {}
    /** Seconds until "Resend code" works again (0: now), from the state's deadline. */
    fun resendSecondsLeft(): Int {
        val at = state.value.resendAt ?: return 0
        val left = at - System.currentTimeMillis()
        return if (left <= 0) 0 else ((left + 999) / 1000).toInt()
    }
}

/** Seconds before "Resend code" may be pressed again. */
const val RESEND_SECS = 30

private const val MIN_PASSWORD = 8

/** A loose email check: something@something.something, no spaces. */
fun looksLikeEmail(s: String): Boolean {
    val t = s.trim()
    val at = t.indexOf('@')
    if (at <= 0 || at == t.length - 1) return false
    val domain = t.substring(at + 1)
    return domain.contains('.') && !domain.startsWith('.') && !domain.endsWith('.') && t.none { it.isWhitespace() }
}

/** Keeps the digits of [text], at most [max]. */
fun digitsOnly(text: String, max: Int): String = text.filter { it.isDigit() }.take(max)

/** Capitalised, ending in a full stop (desktop's `sentence`). */
fun sentence(m: String): String {
    val t = m.trim()
    if (t.isEmpty()) return t
    val s = t.replaceFirstChar { it.uppercase() }
    return if (s.last() in ".!?") s else "$s."
}

/** "Your account is suspended. Reason: … It ends on 2026-12-01." (desktop's `suspended_text`). */
fun suspendedText(reason: String, until: Instant?): String {
    var text = "Your account is suspended."
    if (reason.isNotBlank()) text += " Reason: ${sentence(reason)}"
    text += if (until != null) {
        " It ends on ${LocalDate.ofInstant(until, ZoneId.systemDefault())}."
    } else {
        " It has no end date; contact an admin."
    }
    return text
}

object InviteCodes {
    /** Whether the invite field holds a link rather than a code. */
    fun looksLikeLink(raw: String): Boolean = raw.contains("://") || raw.contains("/invite/")

    /**
     * The code as typed or pasted, normalised: uppercase, the dash after the fourth character,
     * and I/L/O forgiven as 1/1/0 (the server's alphabet has none of them). A link token is
     * returned untouched — deep links send it as is.
     */
    fun normalize(raw: String): String {
        val t = raw.trim()
        if (looksLikeLink(t)) return t
        val cleaned = buildString {
            for (c in t.uppercase()) {
                val d = when (c) { 'I', 'L' -> '1'; 'O' -> '0'; else -> c }
                if (d.isLetterOrDigit()) append(d)
            }
        }
        return if (cleaned.length == 8) "${cleaned.take(4)}-${cleaned.drop(4)}" else cleaned
    }

    /** `XXXX-XXXX` in the server's alphabet. */
    fun isCodeShape(normalized: String): Boolean = Regex("[A-Z0-9]{4}-[A-Z0-9]{4}").matches(normalized)
}

/** The token at the end of an invite link the OS opened (`thehideout://invite/<token>`, `https://<server>/invite/<token>`). */
fun inviteTokenFromUrl(url: String): String? {
    val at = url.indexOf("/invite/")
    if (at < 0) return null
    val token = url.substring(at + "/invite/".length).split('?', '#', '/').firstOrNull().orEmpty()
    return token.ifEmpty { null }
}

/**
 * What an [AppError] means on the auth form (desktop's `auth_error`). Wrong credentials never say
 * which part was wrong, so they reveal nothing about whether an account exists.
 */
fun authError(e: AppError): AuthError = when (e) {
    AppError.Unauthenticated -> AuthError(AuthField.Form, "Those details didn't work. Check them and try again.")
    AppError.AlreadyExists -> AuthError(AuthField.Username, "That username is taken.")
    AppError.InviteRequired -> AuthError(AuthField.Form, "You need an invite to make an account.")
    is AppError.InvalidArgument -> AuthError(AuthField.Form, sentence(e.detail))
    AppError.RateLimited -> AuthError(AuthField.Form, "Too many attempts. Try again in a moment.")
    is AppError.Suspended -> AuthError(AuthField.Form, suspendedText(e.reason, e.until), Suspension(e.reason, e.until))
    AppError.TermsUpdateRequired -> AuthError(AuthField.Form, "Accept the terms and privacy notice to continue.")
    is AppError.Unavailable -> AuthError(AuthField.Form, "Can't reach the server.")
    else -> AuthError(AuthField.Form, sentence(e.message ?: "Something went wrong."))
}

private fun triesLeft(n: Int): String = if (n == 1) "1 try left." else "$n tries left."

/**
 * The auth state machine (desktop's `Auth` in `forms.rs` plus the transitions in `account.rs`): one
 * immutable [AuthUiState] in a [StateFlow], changed only through the [AuthPresenter] methods. Pure
 * Kotlin — no Android — so `:core:test` drives every transition against the [FakeBackend].
 *
 * A successful sign-in hands the session to [onSession] (the app saves the token and bootstraps the
 * store) and resets the form.
 */
class AuthFlow(
    private val api: () -> ApiClient,
    private val scope: CoroutineScope,
    private val deviceName: () -> String,
    private val onSession: (Session) -> Unit,
    private val clock: () -> Long = { System.currentTimeMillis() },
) : AuthPresenter {
    private val _state = MutableStateFlow(AuthUiState())
    override val state: StateFlow<AuthUiState> = _state.asStateFlow()

    private fun update(f: (AuthUiState) -> AuthUiState) = _state.update(f)

    private fun fail(field: AuthField, message: String, suspension: Suspension? = null) = update {
        it.copy(busy = false, error = AuthError(field, message, suspension))
    }

    // ---- navigation ------------------------------------------------------------------------------

    override fun setMode(mode: AuthMode) {
        if (state.value.busy) return
        cancelPasskey()
        update { it.copy(mode = mode) }
        go(if (mode == AuthMode.Login) AuthStep.Password else AuthStep.InviteCode)
    }

    /** Moves to [step] with nothing typed for it yet (desktop's `auth_go`). */
    override fun go(step: AuthStep) {
        if (state.value.busy && !passkeyWaiting()) return
        cancelPasskey()
        val login = state.value.username.trim()
        update {
            val email = if (step is AuthStep.EmailCode && !step.sent && it.email.isEmpty() && login.contains('@')) login else it.email
            it.copy(
                step = step,
                email = email,
                password = "",
                code = "",
                recovery = "",
                error = null,
                notice = null,
                busy = false,
                ceremony = null,
                resendAt = if (step is AuthStep.EmailCode || step is AuthStep.Reset) it.resendAt else null,
            )
        }
        if (step is AuthStep.InviteAccept) update { it.copy(acceptTerms = false, confirmAge = false, username = "", password = "") }
    }

    override fun back() {
        when (val s = state.value.step) {
            is AuthStep.EmailCode -> if (s.sent) go(AuthStep.EmailCode(sent = false)) else go(AuthStep.Password)
            is AuthStep.Reset -> if (s.sent) go(AuthStep.Reset(sent = false)) else go(AuthStep.Password)
            is AuthStep.Passkey -> go(AuthStep.Password)
            is AuthStep.SecondFactor -> if (s.locked || s.passkeyWaiting) go(AuthStep.Password) else go(AuthStep.Password)
            is AuthStep.InviteAccept -> go(AuthStep.InviteCode)
            else -> Unit
        }
    }

    private fun passkeyWaiting(): Boolean {
        val s = state.value.step
        return s is AuthStep.Passkey && s.waiting || s is AuthStep.SecondFactor && s.passkeyWaiting
    }

    override fun cancelPasskey() {
        if (!passkeyWaiting()) return
        update {
            val step = when (val s = it.step) {
                is AuthStep.SecondFactor -> s.copy(passkeyWaiting = false)
                is AuthStep.Passkey -> AuthStep.Passkey(waiting = false)
                else -> s
            }
            it.copy(step = step, busy = false, ceremony = null)
        }
    }

    // ---- fields ------------------------------------------------------------------------------------

    override fun updateUsername(text: String) = update { it.copy(username = text, error = it.error?.takeUnless { e -> e.field == AuthField.Username }) }
    override fun updatePassword(text: String) = update { it.copy(password = text, error = it.error?.takeUnless { e -> e.field == AuthField.Password }) }
    override fun updateEmail(text: String) = update { it.copy(email = text, error = it.error?.takeUnless { e -> e.field == AuthField.Email }) }
    override fun updateRecovery(text: String) = update { it.copy(recovery = text, error = it.error?.takeUnless { e -> e.field == AuthField.Code }) }

    override fun updateCode(text: String) {
        val clean = digitsOnly(text, 6)
        update { it.copy(code = clean, error = it.error?.takeUnless { e -> e.field == AuthField.Code }) }
        val auto = state.value.step.let { s -> s is AuthStep.EmailCode && s.sent || s is AuthStep.SecondFactor && !s.recovery && !s.locked && !s.passkeyWaiting }
        if (clean.length == 6 && auto) submit()
    }

    override fun updateInvite(text: String) = update { it.copy(invite = InviteCodes.normalize(text), error = it.error?.takeUnless { e -> e.field == AuthField.Invite }) }

    override fun toggleAddPasskey() = update { it.copy(addPasskey = !it.addPasskey) }
    override fun toggleTerms() = update { it.copy(acceptTerms = !it.acceptTerms) }
    override fun toggleAge() = update { it.copy(confirmAge = !it.confirmAge) }
    override fun toggleRecovery() {
        val s = state.value.step
        if (s is AuthStep.SecondFactor && !s.locked && !s.passkeyWaiting) {
            update { it.copy(step = s.copy(recovery = !s.recovery), code = "", recovery = "", error = null) }
        }
    }

    override fun dismissSuspension() = update { it.copy(error = null) }
    override fun setServerOpen(open: Boolean) = update { it.copy(serverOpen = open) }
    override fun updateServerDraft(text: String) = update { it.copy(serverDraft = text) }

    override fun resendSecondsLeft(): Int {
        val at = state.value.resendAt ?: return 0
        val left = at - clock()
        return if (left <= 0) 0 else ((left + 999) / 1000).toInt()
    }

    // ---- links -------------------------------------------------------------------------------------

    override fun openLink(url: String) {
        val token = inviteTokenFromUrl(url) ?: return
        if (state.value.busy) return
        setMode(AuthMode.Invite)
        update { it.copy(invite = token) }
        submit()
    }

    // ---- submitting ----------------------------------------------------------------------------------

    /** Enter or the primary button, whatever the step (desktop's `submit_auth`). */
    override fun submit() {
        if (state.value.busy) return
        val s = state.value
        when (val step = s.step) {
            AuthStep.Password -> {
                if (s.username.trim().isEmpty()) return fail(AuthField.Username, "Enter your username or email.")
                if (s.password.isEmpty()) return fail(AuthField.Password, "Enter your password.")
                launch { login(s.username.trim(), s.password) }
            }
            is AuthStep.EmailCode -> {
                if (!step.sent) {
                    if (!looksLikeEmail(s.email)) return fail(AuthField.Email, "Enter the email of your account.")
                    launch { codeRequested(s.email.trim(), isReset = false) }
                } else {
                    if (s.code.length != 6) return fail(AuthField.Code, "Enter the 6-digit code from the email.")
                    launch { verifyCode(s.email.trim(), s.code) }
                }
            }
            is AuthStep.Reset -> {
                if (!step.sent) {
                    if (!looksLikeEmail(s.email)) return fail(AuthField.Email, "Enter the email of your account.")
                    launch { codeRequested(s.email.trim(), isReset = true) }
                } else {
                    if (s.code.length != 6) return fail(AuthField.Code, "Enter the 6-digit code from the email.")
                    if (s.password.length < MIN_PASSWORD) return fail(AuthField.Password, "Use at least $MIN_PASSWORD characters.")
                    launch { resetPassword(s.email.trim(), s.code, s.password) }
                }
            }
            is AuthStep.Passkey -> {
                if (step.waiting) return
                if (s.username.trim().isEmpty()) return fail(AuthField.Username, "Enter your username.")
                launch { beginPasskeyLogin(s.username.trim()) }
            }
            is AuthStep.SecondFactor -> {
                if (step.locked || step.passkeyWaiting) return
                val code = if (step.recovery) s.recovery.trim() else s.code
                if (step.recovery && code.isEmpty()) return fail(AuthField.Code, "Enter one of your recovery codes.")
                if (!step.recovery && code.length != 6) return fail(AuthField.Code, "Enter the 6-digit code from your authenticator app.")
                launch { completeSecondFactor(step.challenge, if (step.recovery) "" else code, if (step.recovery) code else "") }
            }
            AuthStep.InviteCode -> {
                val code = s.invite.trim()
                if (code.isEmpty()) return fail(AuthField.Invite, "Enter the code from your invite email.")
                if (InviteCodes.looksLikeLink(code)) return fail(AuthField.Invite, "Paste just the code from your invite email, not the link.")
                launch { lookupInvite(code) }
            }
            is AuthStep.InviteAccept -> {
                val name = s.username.trim()
                if (name.length !in 3..32) return fail(AuthField.Username, "Choose a username of 3 to 32 characters.")
                if (s.password.length < MIN_PASSWORD) return fail(AuthField.Password, "Use at least $MIN_PASSWORD characters.")
                if (!s.acceptTerms) return fail(AuthField.Form, "Accept the terms and privacy notice to continue.")
                if (!s.confirmAge) return fail(AuthField.Form, "Confirm that you are at least ${step.preview.minimumAge} years old.")
                launch { acceptInvite(step.code, name, s.password, step.preview) }
            }
        }
    }

    override fun resend() {
        if (state.value.busy || resendSecondsLeft() > 0) return
        val s = state.value
        when (val step = s.step) {
            is AuthStep.EmailCode -> if (step.sent) launch { codeRequested(s.email.trim(), isReset = false) }
            is AuthStep.Reset -> if (step.sent) launch { codeRequested(s.email.trim(), isReset = true) }
            else -> Unit
        }
    }

    private fun launch(block: suspend () -> Unit) {
        update { it.copy(busy = true, error = null, notice = null) }
        scope.launch {
            try {
                block()
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                update { it.copy(busy = false, error = AuthError(AuthField.Form, e.message ?: "Something went wrong.")) }
            }
        }
    }

    // ---- the calls and their replies (desktop's `auth_refused` and friends) --------------------------

    /** One server call: [onSuccess] with the answer, [onFailure] with the typed error. */
    private suspend fun <T> call(block: suspend () -> T, onSuccess: suspend (T) -> Unit, onFailure: suspend (AppError) -> Unit) {
        val r = runCatching { block() }
        val v = r.getOrNull()
        if (v != null) onSuccess(v) else onFailure(r.exceptionOrNull() as? AppError ?: AppError.Other("unknown"))
    }

    private suspend fun login(username: String, password: String) = call(
        { api().login(username, password, deviceName()) },
        onSuccess = { lr ->
            when (lr) {
                is LoginResult.SignedIn -> signedIn(lr.session)
                is LoginResult.SecondFactor -> update {
                    it.copy(
                        busy = false,
                        password = "",
                        step = AuthStep.SecondFactor(lr.challengeId, lr.methods, recovery = false, remaining = null, locked = false, passkeyWaiting = false),
                    )
                }
            }
        },
        onFailure = { refused(it, wasPasskey = false) },
    )

    private suspend fun verifyCode(email: String, code: String) = call(
        { api().verifyEmailCode(email, code, deviceName()) },
        onSuccess = { lr ->
            when (lr) {
                is LoginResult.SignedIn -> signedIn(lr.session)
                is LoginResult.SecondFactor -> update {
                    it.copy(
                        busy = false,
                        step = AuthStep.SecondFactor(lr.challengeId, lr.methods, recovery = false, remaining = null, locked = false, passkeyWaiting = false),
                    )
                }
            }
        },
        onFailure = { e ->
            if (e is AppError.Unauthenticated) {
                update { it.copy(busy = false, code = "") }
                fail(AuthField.Code, "That code didn't work. Check it and try again.")
            } else refused(e, wasPasskey = false)
        },
    )

    private suspend fun completeSecondFactor(challenge: String, totp: String, recovery: String) = call(
        { api().completeSecondFactor(challenge, totp, recovery) },
        onSuccess = { signedIn(it) },
        onFailure = { refused(it, wasPasskey = false, recovery = state.value.step.let { s -> s is AuthStep.SecondFactor && s.recovery }) },
    )

    private suspend fun codeRequested(email: String, isReset: Boolean) = call(
        { if (isReset) api().requestPasswordReset(email) else api().requestEmailCode(email) },
        onSuccess = {
            val s = state.value
            update {
                it.copy(
                    busy = false,
                    step = if (isReset) AuthStep.Reset(sent = true) else AuthStep.EmailCode(sent = true),
                    notice = "If an account uses ${s.email.trim()}, a code is on its way. It works for 10 minutes.",
                    resendAt = clock() + RESEND_SECS * 1000L,
                )
            }
        },
        onFailure = { refused(it, wasPasskey = false) },
    )

    private suspend fun resetPassword(email: String, code: String, newPassword: String) = call(
        { api().resetPassword(email, code, newPassword) },
        onSuccess = {
            update {
                it.copy(
                    busy = false,
                    step = AuthStep.Password,
                    username = email,
                    password = "",
                    code = "",
                    notice = "Your password was changed and every device was signed out. Sign in with the new one.",
                    resendAt = null,
                )
            }
        },
        onFailure = { e ->
            if (e is AppError.Unauthenticated) {
                update { it.copy(busy = false, code = "") }
                fail(AuthField.Code, "That code didn't work. Check it, or send a new one.")
            } else refused(e, wasPasskey = false)
        },
    )

    private suspend fun beginPasskeyLogin(username: String) = call(
        { api().beginPasskeyLogin(username, deviceName()) },
        onSuccess = { ceremony -> update { it.copy(busy = false, step = AuthStep.Passkey(waiting = true), ceremony = ceremony) } },
        onFailure = { e ->
            val message = when (e) {
                is AppError.Precondition, AppError.NotFound -> "Passkey sign-in isn't available for that account. Use your password or an emailed code."
                AppError.Unauthenticated -> "The passkey wasn't accepted. Try again."
                else -> authError(e).message
            }
            fail(AuthField.Form, message)
        },
    )

    /** Finishes the passkey sign-in: a session, still waiting, or refused. */
    override fun finishPasskey(ceremonyId: String, credentialJson: String) {
        scope.launch {
            call(
                { api().finishPasskeyLogin(ceremonyId, credentialJson) },
                onSuccess = { r -> if (r.session != null) signedIn(r.session) /* pending: the caller polls again */ },
                onFailure = { refused(it, wasPasskey = true) },
            )
        }
    }

    /** Finishes answering the second factor with a passkey. */
    override fun finishSecondFactorPasskey(ceremonyId: String, credentialJson: String) {
        scope.launch {
            call(
                { api().finishSecondFactorPasskey(ceremonyId, credentialJson) },
                onSuccess = { r -> if (r.session != null) signedIn(r.session) },
                onFailure = { refused(it, wasPasskey = true) },
            )
        }
    }

    /** "Use a passkey" on the second-factor step: the ceremony answers this challenge. */
    fun startSecondFactorPasskey() {
        val s = state.value
        val step = s.step
        if (s.busy || step !is AuthStep.SecondFactor || step.locked || step.passkeyWaiting) return
        launch {
            call(
                { api().beginSecondFactorPasskey(step.challenge) },
                onSuccess = { ceremony -> update { it.copy(busy = false, step = step.copy(passkeyWaiting = true), ceremony = ceremony) } },
                onFailure = { refused(it, wasPasskey = false) },
            )
        }
    }

    private suspend fun lookupInvite(code: String) = call(
        { api().lookupInvite(code) },
        onSuccess = { preview ->
            val problem = when (preview.status) {
                InviteStatus.Pending -> null
                InviteStatus.Accepted -> "This invite was already used. Sign in instead."
                InviteStatus.Expired -> "This invite has expired. Ask for a new one."
                InviteStatus.Revoked -> "This invite was withdrawn. Ask for a new one."
                else -> "This invite can't be used."
            }
            if (problem != null) fail(AuthField.Invite, problem)
            else update { it.copy(busy = false, step = AuthStep.InviteAccept(preview, code)) }
        },
        onFailure = { e ->
            if (e is AppError.NotFound || e is AppError.InvalidArgument) fail(AuthField.Invite, "We couldn't find that invite. Check the code and try again.")
            else refused(e, wasPasskey = false)
        },
    )

    private suspend fun acceptInvite(code: String, username: String, password: String, preview: InvitePreview) {
        val addPasskey = state.value.addPasskey
        call(
            { api().acceptInvite(code, username, password, deviceName(), preview.termsVersion, ageConfirmed = true) },
            onSuccess = { session ->
                signedIn(session)
                if (addPasskey) {
                    // "Add a passkey after signing up": the registration ceremony, once the session is in.
                    scope.launch {
                        call(
                            { api().beginPasskeyRegistration("Passkey") },
                            onSuccess = { c -> call({ api().finishPasskeyRegistration(c.ceremonyId, "") }, {}, { }) },
                            onFailure = {},
                        )
                    }
                }
            },
            onFailure = { e ->
                when (e) {
                    AppError.AlreadyExists -> fail(AuthField.Username, "That username is taken. Try another.")
                    is AppError.Precondition -> fail(AuthField.Form, "This invite can't be used any more (${sentence(e.detail)}). Ask for a new one.")
                    AppError.NotFound -> fail(AuthField.Form, "This invite can't be used any more. Ask for a new one.")
                    is AppError.InvalidArgument -> fail(AuthField.Form, sentence(e.detail))
                    else -> refused(e, wasPasskey = false)
                }
            },
        )
    }

    private fun signedIn(session: Session) {
        update { AuthUiState() }
        onSession(session)
    }

    /** A sign-in was refused: the error goes where the user is looking (desktop's `auth_refused`). */
    private fun refused(e: AppError, wasPasskey: Boolean, recovery: Boolean = false) {
        val s = state.value
        when (val step = s.step) {
            is AuthStep.SecondFactor -> {
                var remaining = step.remaining
                var locked = step.locked
                when (e) {
                    is AppError.WrongCode -> remaining = e.attemptsRemaining
                    AppError.TooManyAttempts, AppError.CodeExpired -> locked = true
                    else -> Unit
                }
                val wasPasskeyStep = step.passkeyWaiting
                update { it.copy(busy = false, code = "", recovery = "", step = step.copy(remaining = remaining, locked = locked, passkeyWaiting = false), ceremony = null) }
                when (e) {
                    AppError.TooManyAttempts -> fail(AuthField.Form, "Too many wrong attempts. Start signing in again.")
                    AppError.CodeExpired -> fail(AuthField.Form, "This sign-in expired. Start again.")
                    is AppError.WrongCode -> {
                        val (field, what) = when {
                            wasPasskeyStep || wasPasskey -> AuthField.Form to "That passkey wasn't accepted."
                            recovery -> AuthField.Code to "That recovery code didn't work."
                            else -> AuthField.Code to "That code didn't work."
                        }
                        fail(field, "$what ${triesLeft(e.attemptsRemaining)}")
                    }
                    is AppError.Precondition -> if (wasPasskeyStep || wasPasskey) fail(AuthField.Form, "A passkey can't be used for this sign-in. Use a code instead.") else fail(AuthField.Form, authError(e).message)
                    else -> fail(AuthField.Form, authError(e).message)
                }
            }
            is AuthStep.Passkey -> {
                update { it.copy(busy = false, step = AuthStep.Passkey(waiting = false), ceremony = null) }
                val message = when (e) {
                    is AppError.Precondition, AppError.NotFound -> "Passkey sign-in isn't available for that account. Use your password or an emailed code."
                    AppError.Unauthenticated -> "The passkey wasn't accepted. Try again."
                    else -> authError(e).message
                }
                fail(AuthField.Form, message)
            }
            is AuthStep.InviteAccept -> {
                when (e) {
                    AppError.AlreadyExists -> fail(AuthField.Username, "That username is taken. Try another.")
                    is AppError.Precondition -> fail(AuthField.Form, "This invite can't be used any more (${sentence(e.detail)}). Ask for a new one.")
                    AppError.NotFound -> fail(AuthField.Form, "This invite can't be used any more. Ask for a new one.")
                    is AppError.InvalidArgument -> fail(AuthField.Form, sentence(e.detail))
                    else -> fail(AuthField.Form, authError(e).message)
                }
            }
            AuthStep.Password -> if (e is AppError.Unauthenticated) {
                update { it.copy(busy = false, password = "") }
                fail(AuthField.Form, "Wrong username, email or password.")
            } else {
                fail(AuthField.Form, authError(e).message)
            }
            else -> fail(AuthField.Form, authError(e).message)
        }
    }
}
