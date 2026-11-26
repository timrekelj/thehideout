import HideoutCore
import HideoutDesign
import SwiftUI

/// Every signed-out screen: one calm card (a centred card on regular widths, the bare page on a phone) with the brand,
/// one step of `AuthFlow`, one primary action and quiet links, and the server line under it.
public struct AuthScreen: View {
    @Environment(AuthFlow.self) private var flow
    @Environment(ServerSelection.self) private var server
    @Environment(\.theme) private var theme
    @FocusState private var focus: AuthFocus?
    @State private var showServer = false
    @State private var legal: LegalPage?

    public init() {}

    public var body: some View {
        AuthChrome(tagline: tagline, footer: { footerLines }) {
            if let s = flow.suspension {
                SuspendedCard(reason: s.reason, until: s.until, action: "Back to sign in") { flow.dismissSuspension() }
            } else {
                if let notice = flow.notice { NoticeText(message: notice) }
                stepContent
                if flow.topLevel { switchLine }
            }
        }
        .legalLinks($legal)
        .sheet(isPresented: $showServer) { ServerSheet().presentationDetents([.medium, .large]) }
        .onChange(of: flow.step) { _, step in
            // Move the keyboard to the first field of the new step.
            let next = Self.firstFocus(step)
            Task { focus = next }
        }
        .onChange(of: flow.issue) { _, issue in
            guard let issue else { return }
            AccessibilityNotification.Announcement(issue.message).post()
            if let f = Self.focus(for: issue.field) { focus = f }
        }
        .sensoryFeedback(trigger: flow.issue) { _, new in new == nil ? nil : .error }
        .animation(Motion.reflow, value: flow.step)
        .animation(Motion.control, value: flow.issue)
    }

    static func firstFocus(_ step: AuthStep) -> AuthFocus? {
        switch step {
        case .password: nil
        case .emailCode(sent: false), .reset(sent: false): .email
        case .emailCode(sent: true), .reset(sent: true): .code
        case .secondFactor(let s): s.locked || s.passkey ? nil : s.recovery ? .recovery : .code
        case .passkey(waiting: false): .login
        case .passkey(waiting: true): nil
        case .inviteCode: .invite
        case .inviteAccept: .username
        }
    }

    static func focus(for field: AuthField) -> AuthFocus? {
        switch field {
        case .login: .login
        case .password: .password
        case .email: .email
        case .code: .code
        case .invite: .invite
        case .username: .username
        case .form: nil
        }
    }

    // MARK: pieces

    private var tagline: String? {
        guard flow.suspension == nil else { return nil }
        switch flow.step {
        case .password: return "Welcome back. Your friends are waiting."
        case .inviteCode: return "Joining? Use the invite someone sent you."
        default: return nil
        }
    }

    private var switchLine: some View {
        HStack(spacing: Space.s1) {
            Text(flow.inviteMode ? "Already have an account?" : "Have an invite?").typeStyle(\.control, color: \.textMuted)
            QuietLink(title: flow.inviteMode ? "Sign in" : "Join with a code", emphasis: true) { flow.inviteMode ? flow.showSignIn() : flow.startInvite() }
        }
        .frame(maxWidth: .infinity)
    }

    private var footerLines: some View {
        VStack(spacing: Space.s2) {
            serverLine
            legalLine
        }
    }

    private var serverLine: some View {
        HStack(spacing: Space.s1) {
            Text("Server: \(ServerURL.display(server.url).replacingOccurrences(of: "https://", with: ""))")
                .typeStyle(\.caption, color: \.textMuted).lineLimit(1).truncationMode(.middle)
            Text("·").typeStyle(\.caption, color: \.textFaint)
            Button { showServer = true } label: {
                Text("Change").typeStyle(\.caption, tint: theme.colors.accentText).frame(minWidth: 44, minHeight: 44)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Change server")
            .accessibilityHint("Currently \(ServerURL.display(server.url))")
        }
        .frame(maxWidth: .infinity)
        .accessibilityElement(children: .contain)
    }

    /// "Terms · Privacy", faint, under the server line (desktop's `account::legal_links`).
    private var legalLine: some View {
        HStack(spacing: Space.s1) {
            Button { legal = LegalPage(url: server.url.appendingPathComponent("terms")) } label: {
                Text("Terms").typeStyle(\.caption, color: \.textFaint).frame(minHeight: 44)
            }
            .buttonStyle(.plain)
            Text("·").typeStyle(\.caption, color: \.textFaint)
            Button { legal = LegalPage(url: server.url.appendingPathComponent("privacy")) } label: {
                Text("Privacy").typeStyle(\.caption, color: \.textFaint).frame(minHeight: 44)
            }
            .buttonStyle(.plain)
        }
        .frame(maxWidth: .infinity)
        .accessibilityElement(children: .contain)
    }

    // MARK: steps

    @ViewBuilder private var stepContent: some View {
        switch flow.step {
        case .password: PasswordStep(focus: $focus)
        case .emailCode, .reset: EmailCodeStep(focus: $focus)
        case .secondFactor(let s): SecondFactorStep(state: s, focus: $focus)
        case .passkey(let waiting): PasskeyStep(waiting: waiting, focus: $focus)
        case .inviteCode: InviteCodeStep(focus: $focus)
        case .inviteAccept(let preview, _): InviteAcceptStep(preview: preview, focus: $focus, legal: $legal)
        }
    }
}

/// The page every signed-out and gate screen sits on: a centred card on regular widths, the bare page on a phone, the
/// brand on top, `footer` (the server line) under it, and a scroll view so large text never clips.
struct AuthChrome<Footer: View, Content: View>: View {
    @Environment(\.theme) private var theme
    @Environment(\.horizontalSizeClass) private var sizeClass
    var tagline: String?
    @ViewBuilder var footer: Footer
    @ViewBuilder var content: Content

    init(tagline: String? = nil, @ViewBuilder footer: () -> Footer, @ViewBuilder content: () -> Content) {
        self.tagline = tagline; self.footer = footer(); self.content = content()
    }

    private var card: Bool { sizeClass == .regular }

    var body: some View {
        GeometryReader { geo in
            ScrollView {
                VStack(spacing: Space.s6) {
                    Spacer(minLength: Space.s6)
                    VStack(alignment: .leading, spacing: Space.s5) {
                        header
                        content
                    }
                    .padding(card ? Space.s8 : 0)
                    .frame(maxWidth: 440, alignment: .leading)
                    .background {
                        if card {
                            RoundedRectangle(cornerRadius: Radius.xl, style: .continuous).fill(theme.colors.surface)
                                .overlay(RoundedRectangle(cornerRadius: Radius.xl, style: .continuous).strokeBorder(theme.colors.borderSubtle, lineWidth: 1))
                        }
                    }
                    Spacer(minLength: Space.s6)
                    footer
                }
                .padding(.horizontal, Space.s6)
                .padding(.vertical, Space.s4)
                .frame(maxWidth: .infinity, minHeight: geo.size.height)
            }
            .scrollDismissesKeyboard(.interactively)
            .scrollBounceBehavior(.basedOnSize)
        }
        .background(theme.colors.bg.ignoresSafeArea())
    }

    private var header: some View {
        VStack(spacing: Space.s3) {
            Image(systemName: "house.fill")
                .font(.system(size: 26, weight: .semibold))
                .foregroundStyle(theme.colors.accentFg)
                .frame(width: 56, height: 56)
                .background(theme.colors.accent, in: RoundedRectangle(cornerRadius: 15, style: .continuous))
                .accessibilityHidden(true)
            Text("thehideout").typeStyle(\.heading, tint: theme.colors.accentText)
            if let tagline { Text(tagline).typeStyle(\.control, color: \.textMuted).multilineTextAlignment(.center).fixedSize(horizontal: false, vertical: true) }
        }
        .frame(maxWidth: .infinity)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }
}

/// The heading of a second step.
struct StepHeading: View {
    let title: String
    var subtitle: String?
    var body: some View {
        VStack(alignment: .leading, spacing: Space.s1) {
            Text(title).typeStyle(\.title).accessibilityAddTraits(.isHeader)
            if let subtitle { Text(subtitle).typeStyle(\.control, color: \.textMuted).fixedSize(horizontal: false, vertical: true) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// The form-wide error, right above the step's button.
struct FormError: View {
    @Environment(AuthFlow.self) private var flow
    var body: some View {
        if let m = flow.issue(for: .form) { ErrorText(m) }
    }
}

// MARK: sign in

private struct PasswordStep: View {
    @Environment(AuthFlow.self) private var flow
    var focus: FocusState<AuthFocus?>.Binding

    var body: some View {
        @Bindable var flow = flow
        VStack(alignment: .leading, spacing: Space.s4) {
            LabelledField("Username or email", error: flow.issue(for: .login)) {
                TextField("", text: $flow.login, prompt: Text("ana or ana@example.com").foregroundStyle(.secondary))
                    .textContentType(.username)
                    .keyboardType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.next)
                    .focused(focus, equals: .login)
                    .modifier(FieldBox(focused: focus.wrappedValue == .login, invalid: flow.issue?.field == .login))
                    .accessibilityLabel("Username or email")
                    .onSubmit { focus.wrappedValue = .password }
            }
            LabelledField("Password", error: flow.issue(for: .password)) {
                SecureField("", text: $flow.password)
                    .textContentType(.password)
                    .submitLabel(.go)
                    .focused(focus, equals: .password)
                    .modifier(FieldBox(focused: focus.wrappedValue == .password, invalid: flow.issue?.field == .password))
                    .accessibilityLabel("Password")
                    .onSubmit { flow.submit() }
            } trailing: {
                QuietLink(title: "Forgot password?") { flow.startReset() }
            }
            FormError()
            PrimaryButton(title: "Sign in", busyTitle: "Signing in…", busy: flow.busy) { flow.submit() }
            HStack(spacing: Space.s2) {
                QuietLink(title: "Email me a code") { flow.startEmailCode() }
                Text("·").typeStyle(\.control, color: \.textFaint).accessibilityHidden(true)
                QuietLink(title: "Use a passkey") { flow.startPasskey() }
            }
            .frame(maxWidth: .infinity)
        }
    }
}

// MARK: email code and reset

private struct EmailCodeStep: View {
    @Environment(AuthFlow.self) private var flow
    @Environment(\.currentDate) private var currentDate
    var focus: FocusState<AuthFocus?>.Binding

    private var reset: Bool { if case .reset = flow.step { true } else { false } }
    private var sent: Bool {
        switch flow.step {
        case .emailCode(let s), .reset(let s): s
        default: false
        }
    }

    var body: some View {
        @Bindable var flow = flow
        VStack(alignment: .leading, spacing: Space.s4) {
            BackLink(title: sent ? "Use a different email" : "Back to sign in") { sent ? (reset ? flow.startReset() : flow.startEmailCode()) : flow.showSignIn() }
            if !sent {
                StepHeading(title: reset ? "Reset your password" : "Sign in with a code",
                            subtitle: reset ? "We'll email you a code to set a new one." : "We'll email you a 6-digit code. No password needed.")
                LabelledField("Email", error: flow.issue(for: .email)) {
                    TextField("", text: $flow.email, prompt: Text("ana@example.com").foregroundStyle(.secondary))
                        .textContentType(.emailAddress)
                        .keyboardType(.emailAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .submitLabel(.send)
                        .focused(focus, equals: .email)
                        .modifier(FieldBox(focused: focus.wrappedValue == .email, invalid: flow.issue?.field == .email))
                        .accessibilityLabel("Email")
                        .onSubmit { flow.submit() }
                }
                FormError()
                PrimaryButton(title: "Send code", busyTitle: "Sending…", busy: flow.busy) { flow.submit() }
            } else {
                StepHeading(title: "Check your email", subtitle: "Enter the 6-digit code we sent.")
                LabelledField("Code", error: flow.issue(for: .code)) {
                    CodeField(text: Binding(get: { flow.code }, set: { flow.setCode($0) }), focus: focus, label: "6-digit code",
                              invalid: flow.issue?.field == .code, submit: { flow.submit() })
                }
                if reset {
                    LabelledField("New password", error: flow.issue(for: .password), hint: "At least 8 characters") {
                        SecureField("", text: $flow.password)
                            .textContentType(.newPassword)
                            .submitLabel(.go)
                            .focused(focus, equals: .password)
                            .modifier(FieldBox(focused: focus.wrappedValue == .password, invalid: flow.issue?.field == .password))
                            .accessibilityLabel("New password")
                            .onSubmit { flow.submit() }
                    }
                }
                FormError()
                if reset || flow.busy {
                    PrimaryButton(title: "Set new password", busyTitle: "Saving…", busy: flow.busy) { flow.submit() }
                } else {
                    PrimaryButton(title: "Sign in", busyTitle: "Signing in…", busy: flow.busy) { flow.submit() }
                }
                resend
            }
        }
    }

    private var resend: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let wait = flow.resendWait(at: context.date)
            Group {
                if wait > 0 {
                    Text("Resend code in \(wait) s").typeStyle(\.control, color: \.textMuted)
                        .frame(minHeight: 44)
                } else {
                    QuietLink(title: "Resend code") { flow.resendCode() }
                }
            }
            .frame(maxWidth: .infinity)
        }
    }
}

// MARK: second factor

private struct SecondFactorStep: View {
    @Environment(AuthFlow.self) private var flow
    let state: SecondFactorState
    var focus: FocusState<AuthFocus?>.Binding

    var body: some View {
        @Bindable var flow = flow
        VStack(alignment: .leading, spacing: Space.s4) {
            if state.locked {
                StepHeading(title: "Two-factor authentication", subtitle: "This sign-in was stopped. Start again to get a new one.")
                FormError()
                PrimaryButton(title: "Back to sign in") { flow.showSignIn() }
            } else if state.passkey {
                BackLink(title: "Cancel") { flow.cancelPasskey() }
                StepHeading(title: "Use your passkey", subtitle: "Confirm with your fingerprint, face or security key to finish signing in.")
                WaitingLine()
                FormError()
            } else {
                BackLink(title: "Start again") { flow.showSignIn() }
                if state.recovery {
                    StepHeading(title: "Use a recovery code", subtitle: "Enter one of the codes you saved when you set up two-factor authentication. Each works once.")
                    LabelledField("Recovery code", error: flow.issue(for: .code)) {
                        TextField("", text: $flow.recovery, prompt: Text("xxxxx-xxxxx").foregroundStyle(.secondary))
                            .font(DesignFont.mono(size: 16))
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .submitLabel(.go)
                            .focused(focus, equals: .recovery)
                            .modifier(FieldBox(focused: focus.wrappedValue == .recovery, invalid: flow.issue?.field == .code))
                            .accessibilityLabel("Recovery code")
                            .onSubmit { flow.submit() }
                    }
                    FormError()
                    PrimaryButton(title: "Sign in", busyTitle: "Checking…", busy: flow.busy) { flow.submit() }
                } else {
                    StepHeading(title: "Two-factor authentication", subtitle: "Enter the 6-digit code from your authenticator app.")
                    LabelledField("Authenticator code", error: flow.issue(for: .code)) {
                        CodeField(text: Binding(get: { flow.code }, set: { flow.setCode($0) }), focus: focus, label: "Authenticator code",
                                  invalid: flow.issue?.field == .code, submit: { flow.submit() })
                    }
                    FormError()
                    PrimaryButton(title: "Sign in", busyTitle: "Checking…", busy: flow.busy) { flow.submit() }
                }
                HStack(spacing: Space.s2) {
                    if state.offersRecovery {
                        QuietLink(title: state.recovery ? "Use your authenticator app" : "Use a recovery code") { flow.toggleRecovery() }
                    }
                    if state.offersRecovery && state.offersPasskey { Text("·").typeStyle(\.control, color: \.textFaint).accessibilityHidden(true) }
                    if state.offersPasskey { QuietLink(title: "Use a passkey") { flow.useSecondFactorPasskey() } }
                }
                .frame(maxWidth: .infinity)
            }
        }
    }
}

private struct WaitingLine: View {
    var body: some View {
        HStack(spacing: Space.s3) {
            ProgressView()
            Text("Waiting…").typeStyle(\.control, color: \.textMuted)
        }
        .frame(maxWidth: .infinity, minHeight: 44)
        .accessibilityElement(children: .combine)
    }
}

// MARK: passkey

private struct PasskeyStep: View {
    @Environment(AuthFlow.self) private var flow
    let waiting: Bool
    var focus: FocusState<AuthFocus?>.Binding

    var body: some View {
        @Bindable var flow = flow
        VStack(alignment: .leading, spacing: Space.s4) {
            if waiting {
                BackLink(title: "Cancel") { flow.cancelPasskey() }
                StepHeading(title: flow.passkeysAreNative ? "Use your passkey" : "Finish in the browser",
                            subtitle: flow.passkeysAreNative ? "Confirm with your fingerprint, face or security key."
                                                             : "We opened a page in a sheet. Confirm with your passkey there and you'll be signed in.")
                WaitingLine()
            } else {
                BackLink(title: "Back to sign in") { flow.showSignIn() }
                StepHeading(title: "Sign in with a passkey",
                            subtitle: flow.passkeysAreNative ? "Confirm it with your fingerprint, face or security key."
                                                             : "A page opens here to confirm it with your fingerprint, face or security key.")
                LabelledField("Username", error: flow.issue(for: .login)) {
                    TextField("", text: $flow.login, prompt: Text("ana").foregroundStyle(.secondary))
                        .textContentType(.username)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .submitLabel(.go)
                        .focused(focus, equals: .login)
                        .modifier(FieldBox(focused: focus.wrappedValue == .login, invalid: flow.issue?.field == .login))
                        .accessibilityLabel("Username")
                        .onSubmit { flow.submit() }
                }
                FormError()
                PrimaryButton(title: "Continue with a passkey", busyTitle: "Opening…", busy: flow.busy) { flow.submit() }
            }
        }
    }
}

// MARK: invite

private struct InviteCodeStep: View {
    @Environment(AuthFlow.self) private var flow
    var focus: FocusState<AuthFocus?>.Binding

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s4) {
            LabelledField("Invite code", error: flow.issue(for: .invite), hint: "It's in your invite email.") {
                InviteCodeField(text: Binding(get: { flow.invite }, set: { flow.setInvite($0) }), focus: focus,
                                invalid: flow.issue?.field == .invite, submit: { flow.submit() })
            }
            FormError()
            PrimaryButton(title: "Continue", busyTitle: "Checking…", busy: flow.busy) { flow.submit() }
        }
    }
}

private struct InviteAcceptStep: View {
    @Environment(AuthFlow.self) private var flow
    @Environment(ServerSelection.self) private var server
    @Environment(\.theme) private var theme
    @Environment(\.timeZone) private var timeZone
    @Environment(\.currentDate) private var currentDate
    let preview: InvitePreview
    var focus: FocusState<AuthFocus?>.Binding
    @Binding var legal: LegalPage?

    private var who: String { preview.inviter ?? "thehideout" }
    private var places: String {
        switch preview.guildNames.count {
        case 0: "thehideout"
        case 1: preview.guildNames[0]
        default: preview.guildNames.dropLast().joined(separator: ", ") + " and " + preview.guildNames.last!
        }
    }

    var body: some View {
        @Bindable var flow = flow
        VStack(alignment: .leading, spacing: Space.s4) {
            BackLink(title: "Use a different invite") { flow.back() }
            VStack(alignment: .leading, spacing: Space.s1) {
                Text("\(who) invited you to join \(places)").typeStyle(\.controlStrong).fixedSize(horizontal: false, vertical: true)
                Text("For \(preview.maskedEmail) · expires \(expires)").typeStyle(\.caption, color: \.textMuted).fixedSize(horizontal: false, vertical: true)
            }
            .padding(Space.s3)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(theme.colors.accentSubtle, in: RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
            .accessibilityElement(children: .combine)

            LabelledField("Choose a username", error: flow.issue(for: .username)) {
                TextField("", text: $flow.username, prompt: Text("ana").foregroundStyle(.secondary))
                    .textContentType(.username)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.next)
                    .focused(focus, equals: .username)
                    .modifier(FieldBox(focused: focus.wrappedValue == .username, invalid: flow.issue?.field == .username))
                    .accessibilityLabel("Choose a username")
                    .onSubmit { focus.wrappedValue = .password }
            }
            LabelledField("Choose a password", error: flow.issue(for: .password), hint: "At least 8 characters") {
                SecureField("", text: $flow.password)
                    .textContentType(.newPassword)
                    .submitLabel(.go)
                    .focused(focus, equals: .password)
                    .modifier(FieldBox(focused: focus.wrappedValue == .password, invalid: flow.issue?.field == .password))
                    .accessibilityLabel("Choose a password")
                    .onSubmit { flow.submit() }
            }
            CheckRow(isOn: $flow.addPasskey, label: Text("Add a passkey after signing up"), spoken: "Add a passkey after signing up")
            CheckRow(isOn: $flow.acceptTerms, label: legalText(prefix: "I agree to the", server: server.url),
                     spoken: "I agree to the Terms and Privacy Policy")
                .accessibilityAction(named: "Read the Terms") { legal = LegalPage(url: server.url.appendingPathComponent("terms")) }
                .accessibilityAction(named: "Read the Privacy Policy") { legal = LegalPage(url: server.url.appendingPathComponent("privacy")) }
            CheckRow(isOn: $flow.confirmAge, label: Text("I'm \(preview.minimumAge) or older"), spoken: "I'm \(preview.minimumAge) or older")
            FormError()
            PrimaryButton(title: "Create account", busyTitle: "Creating account…", busy: flow.busy) { flow.submit() }
                .disabled(!flow.canCreateAccount)
            if !flow.acceptTerms || !flow.confirmAge {
                Text("Tick both boxes to create your account.").typeStyle(\.caption, color: \.textMuted)
            }
        }
    }

    private var expires: String {
        var cal = Calendar.current
        cal.timeZone = timeZone
        if cal.isDate(preview.expiresAt, inSameDayAs: currentDate) { return "today" }
        if cal.isDate(preview.expiresAt, inSameDayAs: cal.date(byAdding: .day, value: 1, to: currentDate) ?? currentDate) { return "tomorrow" }
        return preview.expiresAt.formatted(.dateTime.month(.abbreviated).day().hour().minute().locale(Locale.current))
    }
}
