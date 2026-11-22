import HideoutCore
import HideoutDesign
import SwiftUI

/// A channel's icon in its palette colour. Chrome uses SF Symbols; the user-chosen Phosphor *fill* glyphs map to
/// the closest symbol until the glyph generator lands (PARITY DS-04); unknown names fall back to `#`.
public struct ChannelGlyph: View {
    @Environment(\.theme) private var theme
    let channel: Channel
    let size: CGFloat

    public init(_ channel: Channel, size: CGFloat = 18) {
        self.channel = channel
        self.size = size
    }

    static let symbols: [String: String] = [
        "chat-circle-dots": "bubble.left.and.bubble.right.fill",
        "code": "chevron.left.forwardslash.chevron.right",
        "dice-five": "dice.fill",
        "calendar-check": "calendar.badge.checkmark",
        "megaphone": "megaphone.fill",
        "paint-brush": "paintbrush.pointed.fill",
        "music-notes": "music.note",
        "game-controller": "gamecontroller.fill",
        "smiley": "face.smiling.fill",
        "books": "books.vertical.fill",
        "speaker-high": "speaker.wave.2.fill",
        "hash": "number",
    ]

    public var body: some View {
        let ink = theme.colors.channel(channel.color)?.ink ?? (channel.kind == .voice ? theme.colors.textMuted : theme.colors.textMuted)
        Group {
            switch channel.icon {
            case .unicode(let e): Text(e).font(.system(size: size * 0.9))
            case .customEmoji: Image(systemName: "face.smiling.fill").font(.system(size: size))
            case .phosphor(let name): Image(systemName: Self.symbols[name] ?? "number").font(.system(size: size, weight: .semibold))
            case .default: Image(systemName: channel.kind == .voice ? "speaker.wave.2.fill" : "number").font(.system(size: size, weight: .semibold))
            }
        }
        .symbolRenderingMode(.monochrome)
        .foregroundStyle(ink)
        .frame(width: size + 6, height: size + 6)
        .accessibilityHidden(true)
    }
}
