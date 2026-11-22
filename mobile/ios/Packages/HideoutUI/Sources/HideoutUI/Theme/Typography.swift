import HideoutDesign
import SwiftUI

private struct TypeModifier: ViewModifier {
    let style: KeyPath<TypeRamp, TypeStyle>
    let color: KeyPath<ThemeColors, Color>?
    let override: Color?
    @Environment(\.theme) private var theme
    // The in-app text size is already inside `theme.type`; Dynamic Type scales on top of it.
    @ScaledMetric(relativeTo: .body) private var dynamic: CGFloat = 1

    func body(content: Content) -> some View {
        let s = theme.type[keyPath: style]
        content
            .font(s.font(scale: dynamic))
            .lineSpacing(s.lineSpacing * dynamic)
            .tracking(s.tracking)
            .foregroundStyle(override ?? color.map { theme.colors[keyPath: $0] } ?? theme.colors.text)
    }
}

extension View {
    /// Instrument Sans (or JetBrains Mono for `code`) at the token's exact variable weight and size, scaled by
    /// Dynamic Type on top of the app's text-size setting. `color` is a palette token (default `text`).
    public func typeStyle(_ style: KeyPath<TypeRamp, TypeStyle>, color: KeyPath<ThemeColors, Color>? = nil) -> some View {
        modifier(TypeModifier(style: style, color: color, override: nil))
    }

    /// The same, in a colour that is not a palette token (a role colour, an avatar ink).
    public func typeStyle(_ style: KeyPath<TypeRamp, TypeStyle>, tint: Color) -> some View {
        modifier(TypeModifier(style: style, color: nil, override: tint))
    }
}

/// A tracked scale for code that builds its own `Font` (attributed text).
public struct DynamicScale: DynamicProperty {
    @ScaledMetric(relativeTo: .body) private var value: CGFloat = 1
    public init() {}
    public var wrappedValue: CGFloat { value }
}

extension Theme {
    /// The animation to use for a state change, or nil when motion is reduced.
    public func animation(_ a: Animation, reduced: Bool) -> Animation? { reduced ? nil : a }
}

extension Color {
    /// `#rrggbb` of the colour in sRGB (for the gallery's swatches).
    public var hexString: String {
        let ui = UIColor(self)
        var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        ui.getRed(&r, green: &g, blue: &b, alpha: &a)
        func h(_ v: CGFloat) -> Int { Int((max(0, min(1, v)) * 255).rounded()) }
        return String(format: "#%02x%02x%02x", h(r), h(g), h(b))
    }
}

extension View {
    /// Text and icons on an accent fill (prominent buttons, pills): `accentFg`, which keeps contrast on every accent
    /// in both appearances where the system's default white does not.
    public func onAccent() -> some View { modifier(OnAccent()) }
}

private struct OnAccent: ViewModifier {
    @Environment(\.theme) private var theme
    func body(content: Content) -> some View { content.foregroundStyle(theme.colors.accentFg) }
}
