package app.thehideout.ui

import app.thehideout.core.auth.AuthError
import app.thehideout.core.auth.AuthField
import app.thehideout.core.auth.AuthMode
import app.thehideout.core.auth.AuthStep
import app.thehideout.core.auth.AuthUiState
import app.thehideout.core.auth.InvitePreview
import app.thehideout.core.model.InviteStatus
import app.thehideout.core.auth.SecondFactorMethod
import app.thehideout.core.auth.Suspension
import app.thehideout.core.model.User
import app.thehideout.core.store.AppState
import app.thehideout.core.store.SessionPhase
import app.thehideout.ui.auth.AuthScreen
import app.thehideout.ui.root.HideoutRoot
import app.thehideout.ui.shell.ShellActions
import app.thehideout.ui.theme.Look
import org.junit.Test
import org.robolectric.annotation.Config
import java.time.Instant

/** The auth screens' states, one per desktop scene (`auth-*`). */
object AuthStates {
    val server = "http://10.0.2.2:8080"

    private val preview = InvitePreview(
        maskedEmail = "e***@example.com",
        inviter = User(1, "tim", Instant.parse("2026-01-05T12:00:00Z")),
        guildNames = listOf("The Hideout"),
        expiresAt = Instant.parse("2026-11-28T10:00:00Z"),
        status = InviteStatus.Pending,
        termsVersion = "2026-11-22",
        minimumAge = 16,
    )

    fun signIn(error: AuthError? = null) = AuthUiState(username = "tim", password = "hunter2", error = error)
    val signInFormError = signIn(AuthError(AuthField.Form, "Wrong username, email or password."))
    val emailCode = AuthUiState(step = AuthStep.EmailCode(sent = false), email = "tim@example.com")
    val codeSent = AuthUiState(
        step = AuthStep.EmailCode(sent = true),
        email = "tim@example.com",
        notice = "If an account uses tim@example.com, a code is on its way. It works for 10 minutes.",
        resendAt = System.currentTimeMillis() + 30_000L,
    )
    val codeWrong = codeSent.copy(code = "000000", error = AuthError(AuthField.Code, "That code didn't work. Check it and try again."))
    val secondFactor = AuthUiState(
        step = AuthStep.SecondFactor(
            challenge = "fake-challenge-1",
            methods = setOf(SecondFactorMethod.Totp, SecondFactorMethod.RecoveryCode, SecondFactorMethod.Passkey),
            recovery = false, remaining = 4, locked = false, passkeyWaiting = false,
        ),
        code = "123456",
        error = AuthError(AuthField.Code, "That code didn't work. 4 tries left."),
    )
    val secondFactorRecovery = AuthUiState(
        step = AuthStep.SecondFactor(
            challenge = "fake-challenge-1",
            methods = setOf(SecondFactorMethod.Totp, SecondFactorMethod.RecoveryCode),
            recovery = true, remaining = null, locked = false, passkeyWaiting = false,
        ),
        recovery = "7KQ2-M9XD",
    )
    val secondFactorLocked = AuthUiState(
        step = AuthStep.SecondFactor(
            challenge = "fake-challenge-1",
            methods = setOf(SecondFactorMethod.Totp),
            recovery = false, remaining = null, locked = true, passkeyWaiting = false,
        ),
        error = AuthError(AuthField.Form, "Too many wrong attempts. Start signing in again."),
    )
    val passkey = AuthUiState(step = AuthStep.Passkey(waiting = false), username = "tim")
    val passkeyWaiting = AuthUiState(step = AuthStep.Passkey(waiting = true))
    val reset = AuthUiState(step = AuthStep.Reset(sent = false), email = "tim@example.com")
    val resetSent = AuthUiState(step = AuthStep.Reset(sent = true), email = "tim@example.com", code = "123456", password = "brand-new-password", resendAt = System.currentTimeMillis() + 30_000L)
    val inviteCode = AuthUiState(mode = AuthMode.Invite, step = AuthStep.InviteCode, invite = "WXYZ-2345")
    val inviteLinkError = AuthUiState(
        mode = AuthMode.Invite,
        step = AuthStep.InviteCode,
        invite = "https://thehideout.zabit.si/invite/tok-wxyz",
        error = AuthError(AuthField.Invite, "Paste just the code from your invite email, not the link."),
    )
    val inviteAccept = AuthUiState(
        mode = AuthMode.Invite,
        step = AuthStep.InviteAccept(preview, "WXYZ-2345"),
        username = "erin",
        password = "password123",
        addPasskey = true,
    )
    val suspended = signIn(AuthError(AuthField.Form, "Your account is suspended.", Suspension("spam", Instant.parse("2026-12-01T00:00:00Z"))))
}

/** Every auth screen on a 411 x 891 dp phone. */
@Config(sdk = [35], qualifiers = "w411dp-h891dp-xxhdpi")
class AuthPhoneScreenshots : ScreenshotTest() {

    private fun shot(name: String, variant: Variant, state: AuthUiState) = compose.snapshot("auth-$name", Device.Phone, variant) { _ ->
        AuthScreen(state, StubAuth(state), AuthStates.server, {}, { true }, {})
    }

    private fun gate(name: String, variant: Variant, phase: SessionPhase) = compose.snapshot("auth-$name", Device.Phone, variant) { _ ->
        HideoutRoot(
            AppState(phase = phase), Look(), {}, ShellActions(),
            AuthUiState(), StubAuth(), AuthStates.server, {}, { true }, {}, "0.1.0",
        )
    }

    @Test fun signInDark() = shot("signin", Variant.Dark, AuthStates.signIn())
    @Test fun signInLight() = shot("signin", Variant.Light, AuthStates.signIn())
    @Test fun signInErrorDark() = shot("signin-error", Variant.Dark, AuthStates.signInFormError)
    @Test fun signInErrorLight() = shot("signin-error", Variant.Light, AuthStates.signInFormError)
    @Test fun emailCodeDark() = shot("code", Variant.Dark, AuthStates.emailCode)
    @Test fun emailCodeLight() = shot("code", Variant.Light, AuthStates.emailCode)
    @Test fun codeSentDark() = shot("code-sent", Variant.Dark, AuthStates.codeSent)
    @Test fun codeSentLight() = shot("code-sent", Variant.Light, AuthStates.codeSent)
    @Test fun codeWrongDark() = shot("code-wrong", Variant.Dark, AuthStates.codeWrong)
    @Test fun codeWrongLight() = shot("code-wrong", Variant.Light, AuthStates.codeWrong)
    @Test fun secondFactorDark() = shot("second-factor", Variant.Dark, AuthStates.secondFactor)
    @Test fun secondFactorLight() = shot("second-factor", Variant.Light, AuthStates.secondFactor)
    @Test fun secondFactorRecoveryDark() = shot("second-factor-recovery", Variant.Dark, AuthStates.secondFactorRecovery)
    @Test fun secondFactorRecoveryLight() = shot("second-factor-recovery", Variant.Light, AuthStates.secondFactorRecovery)
    @Test fun secondFactorLockedDark() = shot("second-factor-locked", Variant.Dark, AuthStates.secondFactorLocked)
    @Test fun secondFactorLockedLight() = shot("second-factor-locked", Variant.Light, AuthStates.secondFactorLocked)
    @Test fun passkeyDark() = shot("passkey", Variant.Dark, AuthStates.passkey)
    @Test fun passkeyLight() = shot("passkey", Variant.Light, AuthStates.passkey)
    @Test fun passkeyWaitingDark() = shot("passkey-waiting", Variant.Dark, AuthStates.passkeyWaiting)
    @Test fun passkeyWaitingLight() = shot("passkey-waiting", Variant.Light, AuthStates.passkeyWaiting)
    @Test fun resetDark() = shot("reset", Variant.Dark, AuthStates.reset)
    @Test fun resetLight() = shot("reset", Variant.Light, AuthStates.reset)
    @Test fun resetSentDark() = shot("reset-sent", Variant.Dark, AuthStates.resetSent)
    @Test fun resetSentLight() = shot("reset-sent", Variant.Light, AuthStates.resetSent)
    @Test fun inviteCodeDark() = shot("invite-code", Variant.Dark, AuthStates.inviteCode)
    @Test fun inviteCodeLight() = shot("invite-code", Variant.Light, AuthStates.inviteCode)
    @Test fun inviteLinkErrorDark() = shot("invite-link-error", Variant.Dark, AuthStates.inviteLinkError)
    @Test fun inviteLinkErrorLight() = shot("invite-link-error", Variant.Light, AuthStates.inviteLinkError)
    @Test fun inviteAcceptDark() = shot("invite-accept", Variant.Dark, AuthStates.inviteAccept)
    @Test fun inviteAcceptLight() = shot("invite-accept", Variant.Light, AuthStates.inviteAccept)
    @Test fun suspendedDark() = shot("suspended", Variant.Dark, AuthStates.suspended)
    @Test fun suspendedLight() = shot("suspended", Variant.Light, AuthStates.suspended)
    @Test fun termsGateDark() = gate("terms-gate", Variant.Dark, SessionPhase.TermsRequired("2026-11-22"))
    @Test fun termsGateLight() = gate("terms-gate", Variant.Light, SessionPhase.TermsRequired("2026-11-22"))
    @Test fun updateGateDark() = gate("update-gate", Variant.Dark, SessionPhase.UpdateRequired)
    @Test fun updateGateLight() = gate("update-gate", Variant.Light, SessionPhase.UpdateRequired)
    @Test fun suspendedGateDark() = gate("suspended-gate", Variant.Dark, SessionPhase.Suspended("spam", Instant.parse("2026-12-01T00:00:00Z")))
    @Test fun suspendedGateLight() = gate("suspended-gate", Variant.Light, SessionPhase.Suspended("spam", Instant.parse("2026-12-01T00:00:00Z")))
}

/** The auth screens on a 1280 x 800 dp tablet in landscape: the card stays centred and 400 dp wide. */
@Config(sdk = [35], qualifiers = "w1280dp-h800dp-xhdpi")
class AuthTabletScreenshots : ScreenshotTest() {

    private fun shot(name: String, variant: Variant, state: AuthUiState) = compose.snapshot("auth-$name", Device.Tablet, variant) { _ ->
        AuthScreen(state, StubAuth(state), AuthStates.server, {}, { true }, {})
    }

    private fun gate(name: String, variant: Variant, phase: SessionPhase) = compose.snapshot("auth-$name", Device.Tablet, variant) { _ ->
        HideoutRoot(
            AppState(phase = phase), Look(), {}, ShellActions(),
            AuthUiState(), StubAuth(), AuthStates.server, {}, { true }, {}, "0.1.0",
        )
    }

    @Test fun signInDark() = shot("signin", Variant.Dark, AuthStates.signIn())
    @Test fun signInLight() = shot("signin", Variant.Light, AuthStates.signIn())
    @Test fun codeSentDark() = shot("code-sent", Variant.Dark, AuthStates.codeSent)
    @Test fun codeSentLight() = shot("code-sent", Variant.Light, AuthStates.codeSent)
    @Test fun secondFactorDark() = shot("second-factor", Variant.Dark, AuthStates.secondFactor)
    @Test fun secondFactorLight() = shot("second-factor", Variant.Light, AuthStates.secondFactor)
    @Test fun passkeyWaitingDark() = shot("passkey-waiting", Variant.Dark, AuthStates.passkeyWaiting)
    @Test fun passkeyWaitingLight() = shot("passkey-waiting", Variant.Light, AuthStates.passkeyWaiting)
    @Test fun resetSentDark() = shot("reset-sent", Variant.Dark, AuthStates.resetSent)
    @Test fun resetSentLight() = shot("reset-sent", Variant.Light, AuthStates.resetSent)
    @Test fun inviteCodeDark() = shot("invite-code", Variant.Dark, AuthStates.inviteCode)
    @Test fun inviteCodeLight() = shot("invite-code", Variant.Light, AuthStates.inviteCode)
    @Test fun inviteAcceptDark() = shot("invite-accept", Variant.Dark, AuthStates.inviteAccept)
    @Test fun inviteAcceptLight() = shot("invite-accept", Variant.Light, AuthStates.inviteAccept)
    @Test fun suspendedDark() = shot("suspended", Variant.Dark, AuthStates.suspended)
    @Test fun suspendedLight() = shot("suspended", Variant.Light, AuthStates.suspended)
    @Test fun termsGateDark() = gate("terms-gate", Variant.Dark, SessionPhase.TermsRequired("2026-11-22"))
    @Test fun termsGateLight() = gate("terms-gate", Variant.Light, SessionPhase.TermsRequired("2026-11-22"))
}
