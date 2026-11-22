import HideoutCore
import HideoutDesign
import SwiftUI

/// The branded screen while a saved session is checked: the same colour as the launch screen, so the first frame of the
/// app is this one and sign-in never flashes. A Retry appears when the server could not be reached and nothing is cached.
struct SplashScreen: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme
    @State private var showSpinner = false

    var body: some View {
        VStack(spacing: Space.s6) {
            Wordmark(size: 72)
            if let e = store.bootstrapError {
                VStack(spacing: Space.s4) {
                    Text(bootstrapMessage(e)).typeStyle(\.control, color: \.textMuted).multilineTextAlignment(.center)
                    PrimaryButton(title: "Try again") { Task { await store.bootstrap() } }
                        .frame(maxWidth: 280)
                    QuietLink(title: "Sign out") { store.signOut() }
                }
                .transition(.opacity)
            } else {
                ProgressView().tint(theme.colors.textMuted).opacity(showSpinner ? 1 : 0)
                    .accessibilityLabel("Loading")
            }
        }
        .padding(Space.s8)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(theme.colors.bg.ignoresSafeArea())
        .task {
            try? await Task.sleep(for: .milliseconds(700))
            withAnimation(Motion.control) { showSpinner = true }
        }
    }

    private func bootstrapMessage(_ e: AppError) -> String {
        if case .network = e { return "Can't reach the server. Check your connection and try again." }
        return sentence(e.description)
    }
}

/// "Your account is suspended" with the reason, how long, and who to ask. Used on the auth card and as a gate.
struct SuspendedCard: View {
    @Environment(\.theme) private var theme
    let reason: String
    let until: Date?
    let action: String
    let perform: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s4) {
            StepHeading(title: "Your account is suspended")
            if !reason.trimmingCharacters(in: .whitespaces).isEmpty {
                VStack(alignment: .leading, spacing: Space.s1) {
                    Text("Reason").typeStyle(\.caption, color: \.textMuted)
                    Text(reason).typeStyle(\.control).fixedSize(horizontal: false, vertical: true)
                }
                .padding(Space.s3)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(theme.colors.warningBg, in: RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
                .accessibilityElement(children: .combine)
            }
            Text(until.map { "Your access is paused until \($0.formatted(date: .long, time: .shortened))." }
                 ?? "Your access is paused indefinitely, until an admin lifts the suspension.")
                .typeStyle(\.control, color: \.textMuted).fixedSize(horizontal: false, vertical: true)
            Text("If you think this is a mistake, contact the server admin.")
                .typeStyle(\.caption, color: \.textMuted).fixedSize(horizontal: false, vertical: true)
            PrimaryButton(title: action, action: perform)
        }
    }
}

/// Suspended after the session was already restored (or the sign-in came back suspended): sign out is the way out.
struct SuspendedGate: View {
    @Environment(AppStore.self) private var store
    let reason: String
    let until: Date?
    var body: some View {
        AuthChrome(footer: { EmptyView() }) {
            SuspendedCard(reason: reason, until: until, action: "Sign out") { store.signOut() }
        }
    }
}

/// "We updated our Terms": accept or sign out, nothing else until then.
struct TermsGate: View {
    @Environment(AppStore.self) private var store
    @Environment(ServerSelection.self) private var server
    @Environment(\.theme) private var theme
    @State private var legal: LegalPage?

    var body: some View {
        AuthChrome(footer: { EmptyView() }) {
            VStack(alignment: .leading, spacing: Space.s4) {
                StepHeading(title: "We updated our Terms and Privacy Policy",
                            subtitle: "Please read them, then accept to keep using thehideout. Nothing else changes: your servers and messages are as you left them.")
                VStack(alignment: .leading, spacing: 0) {
                    QuietLink(title: "Read the Terms") { legal = LegalPage(url: server.url.appendingPathComponent("terms")) }
                    QuietLink(title: "Read the Privacy Policy") { legal = LegalPage(url: server.url.appendingPathComponent("privacy")) }
                }
                if let e = store.termsError { ErrorText(e) }
                PrimaryButton(title: "Accept and continue", busyTitle: "Accepting…", busy: store.termsBusy) { Task { await store.acceptTerms() } }
                QuietLink(title: "Sign out") { store.signOut() }.frame(maxWidth: .infinity)
            }
        }
        .legalLinks($legal)
    }
}

/// The server refuses this build: the way forward is a newer one.
struct UpdateGate: View {
    @Environment(AppStore.self) private var store
    @Environment(ServerSelection.self) private var server
    @Environment(\.theme) private var theme

    var body: some View {
        AuthChrome(footer: { EmptyView() }) {
            VStack(alignment: .leading, spacing: Space.s4) {
                StepHeading(title: "Update required",
                            subtitle: "This version of thehideout is too old for \(server.host). Install the latest version to continue.")
                // TODO: point at the App Store / TestFlight listing once there is one; until then the server's own page.
                Link(destination: server.url) {
                    Text("Get the latest version").typeStyle(\.controlStrong, tint: theme.colors.accentFg)
                        .frame(maxWidth: .infinity, minHeight: 28)
                }
                .buttonStyle(.glassProminent)
                .controlSize(.large)
                QuietLink(title: "Back to sign in") { store.signOut() }.frame(maxWidth: .infinity)
            }
        }
    }
}

/// The sheet after an account was made with "Add a passkey after signing up" ticked.
public struct WelcomeSheet: View {
    @Environment(AuthFlow.self) private var flow
    @Environment(\.theme) private var theme

    public init() {}

    public var body: some View {
        VStack(spacing: Space.s5) {
            Image(systemName: "person.badge.key.fill")
                .font(.system(size: 34, weight: .semibold))
                .foregroundStyle(theme.colors.accentFg)
                .frame(width: 72, height: 72)
                .background(theme.colors.accent, in: RoundedRectangle(cornerRadius: 19, style: .continuous))
                .accessibilityHidden(true)
            VStack(spacing: Space.s2) {
                Text("Welcome to thehideout").typeStyle(\.title).multilineTextAlignment(.center)
                Text("Your account is ready. Protect it now with a passkey: sign in with your fingerprint or face, nothing to remember.")
                    .typeStyle(\.control, color: \.textMuted).multilineTextAlignment(.center).fixedSize(horizontal: false, vertical: true)
            }
            if case .failed(let m) = flow.welcome { ErrorText(m) }
            VStack(spacing: Space.s2) {
                PrimaryButton(title: "Add a passkey", busyTitle: "Adding…", busy: flow.welcome == .busy) { flow.addPasskeyNow() }
                QuietLink(title: "Skip") { flow.skipWelcome() }.disabled(flow.welcome == .busy)
            }
        }
        .padding(Space.s6)
        .frame(maxWidth: 440)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(theme.colors.bg.ignoresSafeArea())
        .interactiveDismissDisabled(flow.welcome == .busy)
        .sensoryFeedback(.success, trigger: flow.welcome == .hidden)
    }
}

/// "Server: … · Change" opens this: the address of the server to use.
public struct ServerSheet: View {
    @Environment(ServerSelection.self) private var server
    @Environment(\.dismiss) private var dismiss
    @Environment(\.theme) private var theme
    @State private var text = ""
    @State private var error: String?
    @State private var checking = false
    @FocusState private var focused: Bool

    public init() {}

    public var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: Space.s4) {
                    Text("The address of the thehideout server you use. Your admin or your invite email has it.")
                        .typeStyle(\.control, color: \.textMuted).fixedSize(horizontal: false, vertical: true)
                    LabelledField("Server address", error: error, hint: "https://… (http:// works for a server on your own network)") {
                        TextField("", text: $text, prompt: Text(ServerURL.release).foregroundStyle(.secondary))
                            .textContentType(.URL)
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .submitLabel(.done)
                            .focused($focused)
                            .modifier(FieldBox(focused: focused, invalid: error != nil))
                            .accessibilityLabel("Server address")
                            .onSubmit { Task { await save() } }
                    }
                    PrimaryButton(title: "Use this server", busyTitle: "Checking…", busy: checking) { Task { await save() } }
                    QuietLink(title: "Use the default (\(ServerURL.display(URL(string: ServerURL.default)!).replacingOccurrences(of: "https://", with: "")))") {
                        server.reset(); dismiss()
                    }
                    .frame(maxWidth: .infinity)
                }
                .padding(Space.s6)
            }
            .scrollDismissesKeyboard(.interactively)
            .background(theme.colors.bg.ignoresSafeArea())
            .navigationTitle("Server")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } } }
            .onAppear { text = ServerURL.display(server.url) }
        }
    }

    private func save() async {
        checking = true; error = nil
        defer { checking = false }
        do {
            try await server.change(to: text)
            dismiss()
        } catch let e as ServerSelection.ChangeError {
            error = e.message
        } catch {
            self.error = "Something went wrong. Try again."
        }
    }
}
