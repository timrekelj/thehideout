import HideoutCore
import HideoutDesign
import SwiftUI

/// The neutral screen while a saved session is checked, with a Retry when the server could not be reached.
struct LoadingScreen: View {
    @Environment(AppStore.self) private var store
    @Environment(\.theme) private var theme

    var body: some View {
        VStack(spacing: Space.s4) {
            Wordmark(size: 56)
            if let e = store.bootstrapError {
                Text(sentence(e.description)).typeStyle(\.body, color: \.textMuted).multilineTextAlignment(.center)
                Button { Task { await store.bootstrap() } } label: { Text("Try again").onAccent() }
                    .buttonStyle(.glassProminent)
            } else {
                ProgressView().tint(theme.colors.textMuted)
            }
        }
        .padding(Space.s8)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(theme.colors.bg)
    }
}

/// The brand mark: the hideout tile on its accent, then the name in the display face.
struct Wordmark: View {
    @Environment(\.theme) private var theme
    let size: CGFloat

    var body: some View {
        VStack(spacing: Space.s3) {
            Image(systemName: "house.fill")
                .font(.system(size: size * 0.46, weight: .semibold))
                .foregroundStyle(theme.colors.accentFg)
                .frame(width: size, height: size)
                .background(theme.colors.accent, in: RoundedRectangle(cornerRadius: size * 0.26, style: .continuous))
            Text("thehideout").typeStyle(\.heading)
        }
        .accessibilityElement(children: .combine)
    }
}

/// Phase 1 placeholder: sign-in arrives in phase 2.
struct SignedOutScreen: View {
    @Environment(\.theme) private var theme

    var body: some View {
        VStack(spacing: Space.s6) {
            Spacer()
            Wordmark(size: 72)
            Text("A small, warm place to talk with your people.")
                .typeStyle(\.body, color: \.textMuted)
                .multilineTextAlignment(.center)
            Spacer()
            VStack(spacing: Space.s3) {
                Button {} label: { Text("Sign in").onAccent().frame(maxWidth: .infinity) }
                    .buttonStyle(.glassProminent)
                    .controlSize(.large)
                    .disabled(true)
                Text("Sign-in arrives in the next phase.").typeStyle(\.caption, color: \.textFaint)
            }
        }
        .padding(Space.s8)
        .frame(maxWidth: 480)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(theme.colors.bg)
    }
}

/// A blocking full-screen state: terms, update required, suspended.
struct GateScreen: View {
    @Environment(\.theme) private var theme
    let symbol: String
    let title: String
    let message: String

    var body: some View {
        ContentUnavailableView {
            Label(title, systemImage: symbol).typeStyle(\.title)
        } description: {
            Text(message).typeStyle(\.body, color: \.textMuted)
        }
        .background(theme.colors.bg)
    }
}
