import HideoutCore
import HideoutDesign
import SafariServices
import SwiftUI

/// Which auth field has the keyboard.
enum AuthFocus: Hashable { case login, password, email, code, recovery, invite, username }

/// A field's box in the palette: `surfaceRaised`, the control border, and the accent ring while focused or in error.
struct FieldBox: ViewModifier {
    @Environment(\.theme) private var theme
    var focused: Bool
    var invalid: Bool

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: Radius.lg, style: .continuous)
        content
            .padding(.horizontal, Space.s3)
            .frame(minHeight: max(48, theme.metrics.fieldHeight))
            .background(theme.colors.surfaceRaised, in: shape)
            .overlay(shape.strokeBorder(invalid ? theme.colors.danger : focused ? theme.colors.focus : theme.colors.controlBorder,
                                        lineWidth: focused || invalid ? Space.ringWidth : 1))
            .contentShape(shape)
            .animation(Motion.control, value: focused)
    }
}

/// A label, its control and the error under it. `trailing` sits at the right of the label ("Forgot password?").
struct LabelledField<Control: View, Trailing: View>: View {
    @Environment(\.theme) private var theme
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    let title: String
    let error: String?
    let hint: String?
    let trailing: Trailing
    let control: Control
    @ScaledMetric(relativeTo: .body) private var rowHeight: CGFloat = 22

    init(_ title: String, error: String?, hint: String? = nil, @ViewBuilder control: () -> Control, @ViewBuilder trailing: () -> Trailing = { EmptyView() }) {
        self.title = title; self.error = error; self.hint = hint; self.control = control(); self.trailing = trailing()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s2) {
            // At accessibility text sizes "Password" and "Forgot password?" no longer fit on one line: stack them
            // instead of letting the trailing link truncate.
            if dynamicTypeSize.isAccessibilitySize {
                VStack(alignment: .leading, spacing: Space.s1) {
                    Text(title).typeStyle(\.controlStrong).accessibilityHidden(true)
                    trailing
                }
            } else {
                HStack(alignment: .firstTextBaseline) {
                    Text(title).typeStyle(\.controlStrong)
                        .accessibilityHidden(true)
                    Spacer(minLength: Space.s2)
                    // The link keeps its 44 pt target but does not make the label row taller than the label.
                    trailing.frame(height: rowHeight)
                }
                .frame(minHeight: rowHeight)
            }
            control
            if let error {
                ErrorText(error)
            } else if let hint {
                Text(hint).typeStyle(\.caption, color: \.textMuted).fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

struct ErrorText: View {
    @Environment(\.theme) private var theme
    let message: String
    init(_ message: String) { self.message = message }
    var body: some View {
        Label {
            Text(message).typeStyle(\.control, tint: theme.colors.danger).fixedSize(horizontal: false, vertical: true)
        } icon: {
            Image(systemName: "exclamationmark.circle.fill").foregroundStyle(theme.colors.danger)
                .font(.system(size: 14, weight: .semibold))
        }
        .labelStyle(TopAlignedLabel())
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Error: \(message)")
    }
}

private struct TopAlignedLabel: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Space.s2) {
            configuration.icon
            configuration.title
        }
    }
}

/// A calm notice above the fields (the code is on its way, the password was changed).
struct NoticeText: View {
    @Environment(\.theme) private var theme
    let message: String
    var body: some View {
        Text(message)
            .typeStyle(\.control, color: \.text)
            .fixedSize(horizontal: false, vertical: true)
            .padding(Space.s3)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(theme.colors.accentSubtle, in: RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
    }
}

/// The one primary action of a step.
struct PrimaryButton: View {
    @Environment(\.theme) private var theme
    let title: String
    var busyTitle: String?
    var busy = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: Space.s2) {
                if busy { ProgressView().controlSize(.small).tint(theme.colors.accentFg) }
                Text(busy ? (busyTitle ?? title) : title)
                    .typeStyle(\.controlStrong, tint: theme.colors.accentFg)
            }
            .frame(maxWidth: .infinity, minHeight: 28)
        }
        .buttonStyle(.glassProminent)
        .controlSize(.large)
        .disabled(busy)
        .accessibilityLabel(busy ? (busyTitle ?? title) : title)
    }
}

/// A quiet text button in the accent: the alternatives under the primary action.
struct QuietLink: View {
    @Environment(\.theme) private var theme
    let title: String
    var emphasis = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .typeStyle(emphasis ? \.controlStrong : \.control, tint: theme.colors.accentText)
                .frame(minHeight: 44)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// "Back to sign in" at the top of a second step.
struct BackLink: View {
    @Environment(\.theme) private var theme
    let title: String
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            Label(title, systemImage: "chevron.left")
                .typeStyle(\.control, tint: theme.colors.accentText)
                .frame(minHeight: 44, alignment: .leading)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// A square check box with a label that may contain links.
struct CheckRow: View {
    @Environment(\.theme) private var theme
    @Binding var isOn: Bool
    let label: Text
    let spoken: String

    var body: some View {
        HStack(alignment: .top, spacing: Space.s3) {
            Button { isOn.toggle() } label: {
                let shape = RoundedRectangle(cornerRadius: Radius.sm, style: .continuous)
                ZStack {
                    shape.fill(isOn ? theme.colors.accent : theme.colors.surfaceRaised)
                    shape.strokeBorder(isOn ? theme.colors.accent : theme.colors.controlBorder, lineWidth: 1.5)
                    if isOn { Image(systemName: "checkmark").font(.system(size: 13, weight: .bold)).foregroundStyle(theme.colors.accentFg) }
                }
                .frame(width: 22, height: 22)
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .padding(.leading, -11)
            .padding(.trailing, -11)
            .accessibilityLabel(spoken)
            .accessibilityValue(isOn ? "checked" : "unchecked")
            .accessibilityAddTraits(.isToggle)

            label
                .typeStyle(\.control)
                .tint(theme.colors.accentText)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, Space.s3 - 1)
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityHidden(true)
        }
        .sensoryFeedback(.selection, trigger: isOn)
    }
}

/// The six boxes of a one-time code, over a real text field so iOS autofills it from Mail and Messages.
struct CodeField: View {
    @Environment(\.theme) private var theme
    @ScaledMetric(relativeTo: .title) private var digitSize: CGFloat = 24
    @ScaledMetric(relativeTo: .title) private var boxHeight: CGFloat = 56
    @Binding var text: String
    var focus: FocusState<AuthFocus?>.Binding
    let label: String
    var invalid = false
    var submit: () -> Void

    var body: some View {
        let isFocused = focus.wrappedValue == .code
        ZStack {
            HStack(spacing: Space.s2) {
                ForEach(0..<6, id: \.self) { i in
                    let current = isFocused && i == min(text.count, 5)
                    let digit = i < text.count ? String(text[text.index(text.startIndex, offsetBy: i)]) : ""
                    let shape = RoundedRectangle(cornerRadius: Radius.lg, style: .continuous)
                    Text(digit)
                        .font(DesignFont.mono(size: digitSize))
                        .foregroundStyle(theme.colors.text)
                        .frame(maxWidth: .infinity, minHeight: boxHeight)
                        .background(theme.colors.surfaceRaised, in: shape)
                        .overlay(shape.strokeBorder(invalid ? theme.colors.danger : current ? theme.colors.focus : theme.colors.controlBorder,
                                                    lineWidth: current || invalid ? Space.ringWidth : 1))
                }
            }
            .accessibilityHidden(true)
            TextField("", text: $text)
                .textContentType(.oneTimeCode)
                .keyboardType(.numberPad)
                .focused(focus, equals: .code)
                .foregroundStyle(.clear)
                .tint(.clear)
                .textSelection(.disabled)
                .autocorrectionDisabled()
                .frame(maxWidth: .infinity, minHeight: boxHeight)
                .contentShape(Rectangle())
                .accessibilityLabel(label)
                .accessibilityValue(text.isEmpty ? "empty" : text.map(String.init).joined(separator: " "))
                .onSubmit(submit)
        }
        .animation(Motion.control, value: text.count)
    }
}

/// `SFSafariViewController` for the Terms and Privacy pages.
struct SafariView: UIViewControllerRepresentable {
    let url: URL
    func makeUIViewController(context: Context) -> SFSafariViewController {
        let c = SFSafariViewController(url: url)
        c.dismissButtonStyle = .close
        return c
    }
    func updateUIViewController(_ controller: SFSafariViewController, context: Context) {}
}

struct LegalPage: Identifiable, Hashable {
    let url: URL
    var id: URL { url }
}

/// A text with the two legal names as links to `<server>/terms` and `/privacy`.
func legalText(prefix: String, server: URL) -> Text {
    let terms = server.appendingPathComponent("terms").absoluteString
    let privacy = server.appendingPathComponent("privacy").absoluteString
    let md = "\(prefix) [Terms](\(terms)) and [Privacy Policy](\(privacy))"
    return Text((try? AttributedString(markdown: md)) ?? AttributedString(md))
}

extension View {
    /// Shows `thehideout`'s legal pages in Safari: Terms and Privacy links in this view's text open as a sheet.
    func legalLinks(_ page: Binding<LegalPage?>) -> some View {
        self
            .environment(\.openURL, OpenURLAction { url in
                if url.path == "/terms" || url.path == "/privacy" {
                    page.wrappedValue = LegalPage(url: url)
                    return .handled
                }
                return .systemAction
            })
            .sheet(item: page) { p in
                SafariView(url: p.url).ignoresSafeArea()
            }
    }
}
