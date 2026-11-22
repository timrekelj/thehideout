import HideoutDesign
import SwiftUI
import UIKit

extension EnvironmentValues {
    /// Whether motion is reduced: the system setting, or the app's own choice.
    @Entry public var motionReduced = false
    /// Whether the system asks for less transparency; glass is flattened onto token surfaces.
    @Entry public var transparencyReduced = false
    /// "Now" for relative labels (Today, Yesterday) and samples; tests pin it so snapshots do not drift.
    @Entry public var currentDate: Date = .now
}

private struct ThemeModifier: ViewModifier {
    let store: ThemeStore
    @Environment(\.colorScheme) private var systemScheme
    @Environment(\.colorSchemeContrast) private var systemContrast
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion
    @Environment(\.accessibilityReduceTransparency) private var systemReduceTransparency

    private struct Resolved {
        let theme: Theme
        let reduced: Bool
        let scheme: ColorScheme?
    }

    private func resolve() -> Resolved {
        var spec = store.spec
        // Increase Contrast in Settings means the app's High contrast looks, without changing the saved choice.
        if systemContrast == .increased { spec.contrast = .high }
        let reduced: Bool = switch store.look.reduceMotion {
        case .system: systemReduceMotion
        case .on: true
        case .off: false
        }
        let scheme: ColorScheme? = switch spec.appearance {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
        return Resolved(theme: Theme(spec: spec, systemDark: systemScheme == .dark), reduced: reduced, scheme: scheme)
    }

    func body(content: Content) -> some View {
        let r = resolve()
        let theme = r.theme
        // `.tint(_:)` does not reach a tab bar's `.badge()`: SwiftUI's `Tab` badge always renders in the
        // system's red regardless of the environment tint, which would read as an error state (mention
        // badges are never red; red is errors only). Only `badgeBackgroundColor` on each layout's item
        // appearance is touched (background/blur left nil) so the Liquid Glass bar itself is untouched.
        let badge = UITabBarAppearance()
        for item in [badge.stackedLayoutAppearance, badge.inlineLayoutAppearance, badge.compactInlineLayoutAppearance] {
            item.normal.badgeBackgroundColor = UIColor(theme.colors.accent)
            item.selected.badgeBackgroundColor = UIColor(theme.colors.accent)
        }
        UITabBar.appearance().standardAppearance = badge
        UITabBar.appearance().scrollEdgeAppearance = badge
        return content
            .environment(\.theme, theme)
            .environment(\.motionReduced, r.reduced)
            .environment(\.transparencyReduced, systemReduceTransparency || theme.spec.contrast == .high)
            // Glass for chrome, tokens for content: the tint reaches toolbar items, selection and prominent buttons.
            .tint(theme.colors.accent)
            .preferredColorScheme(r.scheme)
            .background(theme.colors.bg.ignoresSafeArea())
            .transaction { if r.reduced { $0.animation = nil } }
    }
}

extension View {
    /// Resolves the saved look against the system appearance and injects the `Theme` (colours, metrics, type),
    /// the accent tint, the window background and the motion settings. Apply once at the root.
    public func themed(_ store: ThemeStore) -> some View {
        modifier(ThemeModifier(store: store))
    }
}
