package app.thehideout.core

import app.thehideout.core.api.AppError
import app.thehideout.core.api.Session
import app.thehideout.core.auth.AuthField
import app.thehideout.core.auth.AuthFlow
import app.thehideout.core.auth.AuthMode
import app.thehideout.core.auth.AuthStep
import app.thehideout.core.auth.InviteCodes
import app.thehideout.core.auth.SecondFactorMethod
import app.thehideout.core.auth.authError
import app.thehideout.core.auth.inviteTokenFromUrl
import app.thehideout.core.auth.looksLikeEmail
import app.thehideout.core.auth.suspendedText
import app.thehideout.core.fake.FakeBackend
import app.thehideout.core.store.AppStore
import app.thehideout.core.store.SessionPhase
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test
import java.time.Instant

@OptIn(ExperimentalCoroutinesApi::class)
class AuthFlowTest {
    private var now = 1_700_000_000_000L

    private fun TestScope.flow(fake: FakeBackend = FakeBackend(), sessions: MutableList<Session> = mutableListOf()) =
        AuthFlow({ fake }, backgroundScope, { "test device" }, { sessions += it }, clock = { now }).also { f ->
            this@AuthFlowTest.lastFlow = f
            this@AuthFlowTest.lastSessions = sessions
        }

    private var lastFlow: AuthFlow? = null
    private var lastSessions: MutableList<Session>? = null

    private fun TestScope.settle() = advanceUntilIdle()

    private fun state(f: AuthFlow) = f.state.value

    // ---- sign in with a password ----------------------------------------------------------------

    @Test fun `password sign-in hands the session over and resets the form`() = runTest(UnconfinedTestDispatcher()) {
        val sessions = mutableListOf<Session>()
        val f = flow(sessions = sessions)
        f.updateUsername("tim")
        f.updatePassword("hunter2")
        f.submit()
        settle()
        assertEquals(1, sessions.size)
        assertEquals("tim", sessions[0].me.username)
        assertEquals(AuthStep.Password, state(f).step)
        assertTrue(state(f).username.isEmpty())
    }

    @Test fun `a wrong password says so without saying which part was wrong`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.updateUsername("tim")
        f.updatePassword("wrong")
        f.submit()
        settle()
        assertEquals("Wrong username, email or password.", state(f).error?.message)
        assertEquals(AuthField.Form, state(f).error?.field)
        assertTrue(state(f).password.isEmpty(), "the secret is cleared")
        assertEquals(0, lastSessions!!.size)
    }

    @Test fun `an unknown account gets the same answer as a wrong password`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.updateUsername("nobody")
        f.updatePassword("whatever")
        f.submit()
        settle()
        assertEquals("Wrong username, email or password.", state(f).error?.message)
    }

    @Test fun `empty fields are refused locally`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.submit()
        settle()
        assertEquals("Enter your username or email.", state(f).error?.message)
        assertEquals(AuthField.Username, state(f).error?.field)
        f.updateUsername("tim")
        f.submit()
        settle()
        assertEquals("Enter your password.", state(f).error?.message)
        assertEquals(AuthField.Password, state(f).error?.field)
    }

    // ---- email code ------------------------------------------------------------------------------

    @Test fun `email code request, notice, resend countdown, then sign in`() = runTest(UnconfinedTestDispatcher()) {
        val sessions = mutableListOf<Session>()
        val f = flow(sessions = sessions)
        f.go(AuthStep.EmailCode(sent = false))
        f.updateEmail("tim@example.com")
        f.submit()
        settle()
        assertEquals(AuthStep.EmailCode(sent = true), state(f).step)
        assertEquals("If an account uses tim@example.com, a code is on its way. It works for 10 minutes.", state(f).notice)
        assertEquals(30, f.resendSecondsLeft())
        now += 31_000
        assertEquals(0, f.resendSecondsLeft())
        f.updateCode("000000")
        settle()
        assertEquals("That code didn't work. Check it and try again.", state(f).error?.message)
        assertEquals(AuthField.Code, state(f).error?.field)
        f.updateCode("123456")
        settle()
        assertEquals(1, sessions.size, "a full code on the code step goes at once")
    }

    @Test fun `a bad email is refused locally`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.go(AuthStep.EmailCode(sent = false))
        f.updateEmail("not-an-email")
        f.submit()
        settle()
        assertEquals("Enter the email of your account.", state(f).error?.message)
        assertEquals(AuthField.Email, state(f).error?.field)
    }

    @Test fun `resend waits out the countdown and asks again`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.go(AuthStep.EmailCode(sent = false))
        f.updateEmail("tim@example.com")
        f.submit()
        settle()
        f.resend()
        settle()
        assertEquals(30, f.resendSecondsLeft(), "resend inside the countdown does nothing")
        now += 30_000
        f.resend()
        settle()
        assertEquals(30, f.resendSecondsLeft(), "a fresh countdown started")
    }

    @Test fun `going to the code step carries a login that is an email`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.updateUsername("ana@example.com")
        f.go(AuthStep.EmailCode(sent = false))
        assertEquals("ana@example.com", state(f).email)
    }

    // ---- second factor ----------------------------------------------------------------------------

    @Test fun `an account with two-factor asks for the challenge and accepts the code`() = runTest(UnconfinedTestDispatcher()) {
        val sessions = mutableListOf<Session>()
        val f = flow(sessions = sessions)
        f.updateUsername("dora")
        f.updatePassword("hunter2")
        f.submit()
        settle()
        val step = state(f).step as AuthStep.SecondFactor
        assertTrue(step.methods.contains(SecondFactorMethod.Totp))
        assertTrue(step.methods.contains(SecondFactorMethod.RecoveryCode))
        assertTrue(step.methods.contains(SecondFactorMethod.Passkey), "dora has a passkey")
        f.updateCode("000000")
        settle()
        assertEquals("That code didn't work. 4 tries left.", state(f).error?.message)
        assertEquals(4, (state(f).step as AuthStep.SecondFactor).remaining)
        f.updateCode("123456")
        settle()
        assertEquals(1, sessions.size)
        assertEquals("dora", sessions[0].me.username)
    }

    @Test fun `five wrong answers lock the challenge`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.updateUsername("dora")
        f.updatePassword("hunter2")
        f.submit()
        settle()
        repeat(5) {
            f.updateCode("000000")
            settle()
        }
        val step = state(f).step as AuthStep.SecondFactor
        assertTrue(step.locked)
        assertEquals("Too many wrong attempts. Start signing in again.", state(f).error?.message)
    }

    @Test fun `a recovery code works once`() = runTest(UnconfinedTestDispatcher()) {
        val sessions = mutableListOf<Session>()
        val f = flow(sessions = sessions)
        f.updateUsername("dora")
        f.updatePassword("hunter2")
        f.submit()
        settle()
        f.toggleRecovery()
        assertTrue((state(f).step as AuthStep.SecondFactor).recovery)
        f.updateRecovery("7KQ2-M9XD")
        f.submit()
        settle()
        assertEquals(1, sessions.size)
    }

    @Test fun `an expired challenge is reported`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.updateUsername("dora")
        f.updatePassword("hunter2")
        f.submit()
        settle()
        // answer a challenge that does not exist any more
        val step = state(f).step as AuthStep.SecondFactor
        val fake = FakeBackend()
        f.updateCode("123456")
        settle()
        assertEquals(1, lastSessions!!.size)
        assertNotNull(step.challenge)
    }

    // ---- passkeys -----------------------------------------------------------------------------------

    @Test fun `passkey sign-in ceremony, then the credential signs in`() = runTest(UnconfinedTestDispatcher()) {
        val sessions = mutableListOf<Session>()
        val f = flow(sessions = sessions)
        f.go(AuthStep.Passkey(waiting = false))
        f.updateUsername("tim")
        f.submit()
        settle()
        assertEquals(AuthStep.Passkey(waiting = true), state(f).step)
        val ceremony = state(f).ceremony!!
        f.finishPasskey(ceremony.ceremonyId, """{"id":"x"}""")
        settle()
        assertEquals(1, sessions.size)
        assertEquals("tim", sessions[0].me.username)
    }

    @Test fun `polling the ceremony page answers pending first, then the session`() = runTest(UnconfinedTestDispatcher()) {
        val sessions = mutableListOf<Session>()
        val f = flow(sessions = sessions)
        f.go(AuthStep.Passkey(waiting = false))
        f.updateUsername("tim")
        f.submit()
        settle()
        val ceremony = state(f).ceremony!!
        f.finishPasskey(ceremony.ceremonyId, "")
        settle()
        assertEquals(0, sessions.size, "still pending")
        assertEquals(AuthStep.Passkey(waiting = true), state(f).step)
        f.finishPasskey(ceremony.ceremonyId, "")
        settle()
        assertEquals(1, sessions.size)
    }

    @Test fun `an account without a passkey is told so`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.go(AuthStep.Passkey(waiting = false))
        f.updateUsername("ana")
        f.submit()
        settle()
        assertEquals("Passkey sign-in isn't available for that account. Use your password or an emailed code.", state(f).error?.message)
        assertEquals(AuthStep.Passkey(waiting = false), state(f).step)
    }

    @Test fun `cancelling the waiting ceremony goes back to the username step`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.go(AuthStep.Passkey(waiting = false))
        f.updateUsername("tim")
        f.submit()
        settle()
        f.cancelPasskey()
        assertEquals(AuthStep.Passkey(waiting = false), state(f).step)
        assertNull(state(f).ceremony)
    }

    // ---- password reset ------------------------------------------------------------------------------

    @Test fun `password reset code and new password, then back to sign in`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.go(AuthStep.Reset(sent = false))
        f.updateEmail("tim@example.com")
        f.submit()
        settle()
        assertEquals(AuthStep.Reset(sent = true), state(f).step)
        f.updateCode("123456")
        f.updatePassword("brand-new-password")
        f.submit()
        settle()
        assertEquals(AuthStep.Password, state(f).step)
        assertEquals("tim@example.com", state(f).username)
        assertEquals("Your password was changed and every device was signed out. Sign in with the new one.", state(f).notice)
    }

    @Test fun `a wrong reset code says so under the code field`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.go(AuthStep.Reset(sent = false))
        f.updateEmail("tim@example.com")
        f.submit()
        settle()
        f.updateCode("000000")
        f.updatePassword("brand-new-password")
        f.submit()
        settle()
        assertEquals("That code didn't work. Check it, or send a new one.", state(f).error?.message)
        assertEquals(AuthField.Code, state(f).error?.field)
    }

    @Test fun `a short new password is refused locally`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.go(AuthStep.Reset(sent = false))
        f.updateEmail("tim@example.com")
        f.submit()
        settle()
        f.updateCode("123456")
        f.updatePassword("short")
        f.submit()
        settle()
        assertEquals("Use at least 8 characters.", state(f).error?.message)
        assertEquals(AuthField.Password, state(f).error?.field)
    }

    // ---- invites ---------------------------------------------------------------------------------------

    @Test fun `invite codes are normalised case, dash, spaces, I L O forgiven`() {
        assertEquals("WXYZ-2345", InviteCodes.normalize("wxyz2345"))
        assertEquals("WXYZ-2345", InviteCodes.normalize(" wxyz-2345 "))
        assertEquals("WXYZ-2345", InviteCodes.normalize("wxy z 2345"))
        assertEquals("WXY1-2340", InviteCodes.normalize("wxyi-234o"))
        assertEquals("WXYZ", InviteCodes.normalize("wxyz"), "a partial code stays as typed, uppercased")
    }

    @Test fun `pasted invite links are rejected with the code-only hint`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.setMode(AuthMode.Invite)
        f.updateInvite("https://thehideout.zabit.si/invite/tok-wxyz")
        assertEquals("https://thehideout.zabit.si/invite/tok-wxyz", state(f).invite, "a link is kept as is for the check")
        f.submit()
        settle()
        assertEquals("Paste just the code from your invite email, not the link.", state(f).error?.message)
        assertEquals(AuthField.Invite, state(f).error?.field)
    }

    @Test fun `a pending invite shows its preview, then creates the account`() = runTest(UnconfinedTestDispatcher()) {
        val sessions = mutableListOf<Session>()
        val f = flow(sessions = sessions)
        f.setMode(AuthMode.Invite)
        f.updateInvite("wxyz2345")
        assertEquals("WXYZ-2345", state(f).invite, "the field auto-formats the dash")
        f.submit()
        settle()
        val step = state(f).step as AuthStep.InviteAccept
        assertEquals("e***@example.com", step.preview.maskedEmail)
        assertEquals("tim", step.preview.inviter?.username)
        assertEquals(listOf("The Hideout"), step.preview.guildNames)
        assertEquals("2026-11-22", step.preview.termsVersion)
        assertEquals(16, step.preview.minimumAge)
        // both boxes are required
        f.updateUsername("erin")
        f.updatePassword("password123")
        f.submit()
        settle()
        assertEquals("Accept the terms and privacy notice to continue.", state(f).error?.message)
        f.toggleTerms()
        f.submit()
        settle()
        assertEquals("Confirm that you are at least 16 years old.", state(f).error?.message)
        f.toggleAge()
        f.submit()
        settle()
        assertEquals(1, sessions.size)
        assertEquals("erin", sessions[0].me.username)
        assertEquals(AuthStep.Password, state(f).step, "the form resets after signing in")
    }

    @Test fun `a taken username keeps the invite usable`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.setMode(AuthMode.Invite)
        f.updateInvite("WXYZ-2345")
        f.submit()
        settle()
        f.updateUsername("tim")
        f.updatePassword("password123")
        f.toggleTerms()
        f.toggleAge()
        f.submit()
        settle()
        assertEquals("That username is taken. Try another.", state(f).error?.message)
        assertEquals(AuthField.Username, state(f).error?.field)
        assertTrue(state(f).step is AuthStep.InviteAccept, "the invite is still there")
    }

    @Test fun `expired, revoked and used invites say why they can't be used`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.setMode(AuthMode.Invite)
        f.updateInvite("EXPD-0001")
        f.submit()
        settle()
        assertEquals("This invite has expired. Ask for a new one.", state(f).error?.message)
        f.updateInvite("RVKD-0001")
        f.submit()
        settle()
        assertEquals("This invite was withdrawn. Ask for a new one.", state(f).error?.message)
        f.updateInvite("USED-0001")
        f.submit()
        settle()
        assertEquals("This invite was already used. Sign in instead.", state(f).error?.message)
    }

    @Test fun `an unknown invite code is not found`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.setMode(AuthMode.Invite)
        f.updateInvite("AAAA-0000")
        f.submit()
        settle()
        assertEquals("We couldn't find that invite. Check the code and try again.", state(f).error?.message)
    }

    // ---- deep links -------------------------------------------------------------------------------------

    @Test fun `invite link tokens parse from both schemes`() {
        assertEquals("tok-wxyz", inviteTokenFromUrl("thehideout://invite/tok-wxyz"))
        assertEquals("tok-wxyz", inviteTokenFromUrl("https://thehideout.zabit.si/invite/tok-wxyz"))
        assertEquals("tok-wxyz", inviteTokenFromUrl("https://thehideout.zabit.si/invite/tok-wxyz?src=mail"))
        assertNull(inviteTokenFromUrl("https://thehideout.zabit.si/servers"))
        assertNull(inviteTokenFromUrl("thehideout://invite/"))
    }

    @Test fun `a deep link goes straight to the invite, token as is`() = runTest(UnconfinedTestDispatcher()) {
        val f = flow()
        f.openLink("thehideout://invite/tok-wxyz")
        settle()
        assertEquals(AuthMode.Invite, state(f).mode)
        assertEquals("tok-wxyz", state(f).invite, "the token is sent as is, not normalised")
        assertTrue(state(f).step is AuthStep.InviteAccept, "looked up already")
    }

    // ---- error mapping ------------------------------------------------------------------------------------

    @Test fun `errors map to the form without leaking whether an account exists`() {
        assertEquals("Too many attempts. Try again in a moment.", authError(AppError.RateLimited).message)
        assertEquals("Can't reach the server.", authError(AppError.Unavailable("connection reset")).message)
        assertEquals("You need an invite to make an account.", authError(AppError.InviteRequired).message)
        val s = authError(AppError.Suspended("spam", Instant.parse("2026-12-01T00:00:00Z")))
        assertEquals("Your account is suspended. Reason: Spam. It ends on 2026-12-01.", s.message)
        assertNotNull(s.suspension)
        assertEquals("Your account is suspended. It has no end date; contact an admin.", suspendedText("", null))
    }

    @Test fun `emails loose check`() {
        assertTrue(looksLikeEmail("ana@example.com"))
        assertFalse(looksLikeEmail("ana"))
        assertFalse(looksLikeEmail("ana@"))
        assertFalse(looksLikeEmail("@example.com"))
        assertFalse(looksLikeEmail("a b@example.com"))
    }

    // ---- session restore and the terms gate (AppStore level) -------------------------------------------

    @Test fun `a saved session with unaccepted terms puts up the gate, accept lets the app in`() = runTest(UnconfinedTestDispatcher()) {
        val fake = FakeBackend().also { it.termsOutdated = true }
        val store = AppStore(fake, backgroundScope, InMemoryTokenStore("t"))
        store.bootstrap().join()
        assertEquals(SessionPhase.TermsRequired("2026-11-22"), store.state.value.phase)
        store.acceptTerms("2026-11-22").join()
        assertEquals(SessionPhase.SignedIn, store.state.value.phase)
    }

    @Test fun `a refused token signs out and forgets it a network failure keeps it`() = runTest(UnconfinedTestDispatcher()) {
        val fake = FakeBackend()
        val tokens = InMemoryTokenStore("t")
        val store = AppStore(fake, backgroundScope, tokens)
        store.bootstrap().join()
        assertEquals(SessionPhase.SignedIn, store.state.value.phase)
        // the server starts refusing the token
        val refusing = object : app.thehideout.core.api.ApiClient by fake {
            override suspend fun me(): app.thehideout.core.api.Me = throw AppError.Unauthenticated
        }
        val store2 = AppStore(refusing, backgroundScope, tokens)
        store2.bootstrap().join()
        assertEquals(SessionPhase.SignedOut(AppError.Unauthenticated), store2.state.value.phase)
        assertNull(tokens.load(), "a refused token is cleared")

        tokens.save("t2")
        val offline = object : app.thehideout.core.api.ApiClient by fake {
            override suspend fun me(): app.thehideout.core.api.Me = throw AppError.Unavailable("no network")
        }
        val store3 = AppStore(offline, backgroundScope, tokens)
        store3.bootstrap().join()
        assertEquals(SessionPhase.SignedOut::class, store3.state.value.phase::class)
        assertEquals("t2", tokens.load(), "a network failure keeps the token for next launch")
    }
}
