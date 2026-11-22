import HideoutDesign
import SwiftUI

/// The mention count on a row or tile: accent pill, numerals in the control face.
public struct UnreadPill: View {
    @Environment(\.theme) private var theme
    let count: Int

    public init(_ count: Int) { self.count = count }

    public var body: some View {
        Text(count > 99 ? "99+" : "\(count)")
            .font(DesignFont.ui(size: 12, weight: 650))
            .monospacedDigit()
            .foregroundStyle(theme.colors.accentFg)
            .padding(.horizontal, 6)
            .frame(minWidth: 20, minHeight: 20)
            .background(theme.colors.accent, in: Capsule())
            .accessibilityLabel("\(count) mentions")
    }
}

/// "There is something new here" without a count.
public struct UnreadDot: View {
    @Environment(\.theme) private var theme
    public init() {}
    public var body: some View {
        Circle().fill(theme.colors.text).frame(width: 8, height: 8).accessibilityLabel("Unread")
    }
}

/// A small status label: success, warning, danger, info or accent, on its own wash with a hairline border.
public struct StatusBadge: View {
    public enum Tone: Sendable { case accent, success, warning, danger, info }

    @Environment(\.theme) private var theme
    let text: String
    let tone: Tone

    public init(_ text: String, tone: Tone = .accent) {
        self.text = text
        self.tone = tone
    }

    public var body: some View {
        let c = theme.colors
        let (fg, bg, border): (Color, Color, Color) = switch tone {
        case .accent: (c.accentText, c.accentWash, c.accent.opacity(0.35))
        case .success: (c.success, c.successBg, c.successBorder)
        case .warning: (c.warning, c.warningBg, c.warningBorder)
        case .danger: (c.danger, c.dangerBg, c.dangerBorder)
        case .info: (c.info, c.infoBg, c.infoBorder)
        }
        Text(text)
            .font(DesignFont.ui(size: 12, weight: 530))
            .foregroundStyle(fg)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(bg, in: RoundedRectangle(cornerRadius: Radius.sm, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: Radius.sm, style: .continuous).strokeBorder(border, lineWidth: 1))
    }
}

/// A reaction chip: the emoji and a count; yours is washed in the accent.
public struct ReactionChip: View {
    @Environment(\.theme) private var theme
    let emoji: String
    let count: Int
    let mine: Bool

    public init(_ emoji: String, count: Int, mine: Bool = false) {
        self.emoji = emoji
        self.count = count
        self.mine = mine
    }

    public var body: some View {
        HStack(spacing: 5) {
            Text(emoji).font(.system(size: 14))
            Text("\(count)").font(DesignFont.ui(size: 13, weight: 530)).monospacedDigit()
                .foregroundStyle(mine ? theme.colors.accentText : theme.colors.textMuted)
        }
        .padding(.horizontal, 8)
        .frame(minHeight: 26)
        .background(mine ? theme.colors.accentWash : theme.colors.surfaceRaised, in: RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: Radius.md, style: .continuous).strokeBorder(mine ? theme.colors.accent : theme.colors.borderSubtle, lineWidth: 1))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(emoji), \(count)\(mine ? ", you reacted" : "")")
    }
}

/// A selectable filter chip (the gallery's and the emoji picker's tabs).
public struct FilterChip: View {
    @Environment(\.theme) private var theme
    let title: String
    let systemImage: String?
    @Binding var isOn: Bool

    public init(_ title: String, systemImage: String? = nil, isOn: Binding<Bool>) {
        self.title = title
        self.systemImage = systemImage
        self._isOn = isOn
    }

    public var body: some View {
        Button { isOn.toggle() } label: {
            HStack(spacing: 6) {
                if let systemImage { Image(systemName: systemImage).font(.system(size: 13, weight: .semibold)) }
                Text(title).font(DesignFont.ui(size: 14, weight: 530))
            }
            .foregroundStyle(isOn ? theme.colors.accentText : theme.colors.textMuted)
            .padding(.horizontal, 12)
            .frame(minHeight: 36)
            .background(isOn ? theme.colors.accentWash : theme.colors.surfaceRaised, in: Capsule())
            .overlay(Capsule().strokeBorder(isOn ? theme.colors.accent : theme.colors.borderSubtle, lineWidth: 1))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isOn ? .isSelected : [])
    }
}
