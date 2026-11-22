import HideoutDesign
import SwiftUI

extension View {
    /// A content surface from the palette: `surface` with a hairline, crisp corners. Content sits on tokens,
    /// chrome (bars, sheets, menus) stays the system's glass.
    public func card(padding: CGFloat = Space.s4, raised: Bool = false) -> some View {
        modifier(CardModifier(padding: padding, raised: raised))
    }
}

private struct CardModifier: ViewModifier {
    @Environment(\.theme) private var theme
    let padding: CGFloat
    let raised: Bool

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: Radius.xl, style: .continuous)
        content
            .padding(padding)
            // A finite cap, not `.infinity`: an infinite ideal width makes a scroll view's content grow past
            // the viewport, which pushes the surrounding horizontal insets off-screen (the gallery's cards).
            .frame(maxWidth: 1000, alignment: .leading)
            .background(raised ? theme.colors.surfaceRaised : theme.colors.surface, in: shape)
            .overlay(shape.strokeBorder(theme.colors.borderSubtle, lineWidth: 1))
    }
}

/// An overline label above a group of content.
public struct SectionLabel: View {
    @Environment(\.theme) private var theme
    let text: String
    public init(_ text: String) { self.text = text }
    public var body: some View {
        Text(text.uppercased())
            .typeStyle(\.label, color: \.textMuted)
            .tracking(0.9)
            .accessibilityAddTraits(.isHeader)
    }
}

/// A field in the palette: `surfaceRaised`, the control border, and the accent ring while focused.
public struct ThemedFieldStyle: TextFieldStyle {
    @Environment(\.theme) private var theme
    var focused: Bool

    public init(focused: Bool = false) { self.focused = focused }

    public func _body(configuration: TextField<Self._Label>) -> some View {
        let shape = RoundedRectangle(cornerRadius: Radius.lg, style: .continuous)
        configuration
            .font(DesignFont.ui(size: 16, weight: 450))
            .padding(.horizontal, Space.s3)
            .frame(minHeight: max(44, theme.metrics.fieldHeight))
            .background(theme.colors.surfaceRaised, in: shape)
            .overlay(shape.strokeBorder(focused ? theme.colors.focus : theme.colors.controlBorder, lineWidth: focused ? Space.ringWidth : 1))
    }
}

/// A text field that knows its own focus.
public struct ThemedField: View {
    let title: String
    @Binding var text: String
    var secure = false
    @FocusState private var focused: Bool

    public init(_ title: String, text: Binding<String>, secure: Bool = false) {
        self.title = title
        self._text = text
        self.secure = secure
    }

    public var body: some View {
        Group {
            if secure { SecureField(title, text: $text) } else { TextField(title, text: $text) }
        }
        .textFieldStyle(ThemedFieldStyle(focused: focused))
        .focused($focused)
        .animation(Motion.control, value: focused)
    }
}
