import HideoutCore
import HideoutDesign
import SwiftUI

/// A message's text, built from `HideoutCore.Markdown`'s AST (not `AttributedString(markdown:)`, a
/// different dialect): paragraphs with inline bold/italic/underline/strike/code, links, `@mentions` (always
/// the accent, never red: red is for errors only), spoilers (tap to reveal, one flag per message), jumbo
/// emoji, headings, quotes, lists and fenced code blocks (horizontal scroll). A custom emoji shows its
/// `:name:` text, the same fallback desktop uses for one it doesn't have the image for yet (ATT/MSG-09
/// image rendering is a later task).
public struct MessageBody: View {
    @Environment(\.theme) private var theme
    private let dynamic = DynamicScale()
    let text: String
    let me: String?
    @State private var spoilersRevealed = false

    public init(_ text: String, me: String? = nil) {
        self.text = text
        self.me = me
    }

    public var body: some View {
        let blocks = Markdown.parse(text)
        VStack(alignment: .leading, spacing: 5) {
            if let jumbo = Jumbo.inlines(of: blocks) {
                jumboText(jumbo)
            } else {
                ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in blockView(block) }
            }
        }
    }

    @ViewBuilder private func blockView(_ block: Block) -> some View {
        switch block {
        case .paragraph(let inlines):
            paragraph(inlines)
        case .heading(_, let inlines):
            paragraph(inlines, style: theme.type.heading)
        case .quote(let inlines):
            HStack(alignment: .top, spacing: 10) {
                RoundedRectangle(cornerRadius: 1.5).fill(theme.colors.borderStrong).frame(width: 3)
                paragraph(inlines, color: theme.colors.textMuted)
            }
        case .codeBlock(_, let code):
            ScrollView(.horizontal) {
                Text(code)
                    .font(theme.type.code.font(scale: dynamic.wrappedValue))
                    .foregroundStyle(theme.colors.text)
                    .fixedSize(horizontal: true, vertical: true)
            }
            .padding(Space.s3)
            .background(theme.colors.bg, in: RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: Radius.md, style: .continuous).strokeBorder(theme.colors.borderSubtle, lineWidth: 1))
        case .list(let ordered, let items):
            VStack(alignment: .leading, spacing: 2) {
                ForEach(Array(items.enumerated()), id: \.offset) { i, item in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        if ordered { Text("\(i + 1).").monospacedDigit().foregroundStyle(theme.colors.textFaint) }
                        else { Text("•").foregroundStyle(theme.colors.textFaint) }
                        paragraph(item)
                    }
                    .padding(.leading, 4).padding(.vertical, -2)
                }
            }
        }
    }

    private func jumboText(_ inlines: [Inline]) -> some View {
        let scale = dynamic.wrappedValue
        let hasCustom = inlines.contains { if case .customEmoji = $0 { true } else { false } }
        return Text(buildAttributed(inlines, traits: Traits(), style: theme.type.body, color: theme.colors.text, scale: scale))
            .font(.system(size: (hasCustom ? 48 : 40) * scale))
    }

    private func paragraph(_ inlines: [Inline], style: TypeStyle? = nil, color: Color? = nil) -> some View {
        let scale = dynamic.wrappedValue
        let s = style ?? theme.type.body
        let attributed = buildAttributed(inlines, traits: Traits(), style: s, color: color ?? theme.colors.text, scale: scale)
        // Not `.textSelection(.enabled)`: it installs its own long-press (the system's Copy/Select popover),
        // which would steal the press from `MessageRow`'s context menu (Reply, Edit, Delete) before it ever
        // opens. Copy text (MSG-02) is a menu action instead, on the same gesture as everything else.
        return Text(attributed)
            .lineSpacing(s.lineSpacing * scale)
            .fixedSize(horizontal: false, vertical: true)
            .applying { view in
                if hasSpoiler(inlines) {
                    view.onTapGesture { withAnimation(Motion.control) { spoilersRevealed = true } }
                } else {
                    view
                }
            }
    }

    private struct Traits { var bold = false; var italic = false; var underline = false; var strike = false }

    private func hasSpoiler(_ inlines: [Inline]) -> Bool {
        inlines.contains { inline in
            switch inline {
            case .spoiler: true
            case .bold(let c), .italic(let c), .underline(let c), .strike(let c): hasSpoiler(c)
            case .link(let c, _): hasSpoiler(c)
            default: false
            }
        }
    }

    private func font(_ style: TypeStyle, _ traits: Traits, scale: CGFloat) -> Font {
        let weight = traits.bold ? max(style.weight, 650) : style.weight
        let base = DesignFont.ui(size: style.size * scale, weight: weight)
        return traits.italic ? base.italic() : base
    }

    private func buildAttributed(_ inlines: [Inline], traits: Traits, style: TypeStyle, color: Color, scale: CGFloat) -> AttributedString {
        var out = AttributedString()
        for inline in inlines {
            switch inline {
            case .text(let t):
                var seg = AttributedString(t)
                seg.font = font(style, traits, scale: scale)
                seg.foregroundColor = color
                if traits.underline { seg.underlineStyle = .single }
                if traits.strike { seg.strikethroughStyle = .single }
                out += seg
            case .bold(let c):
                var t = traits; t.bold = true
                out += buildAttributed(c, traits: t, style: style, color: color, scale: scale)
            case .italic(let c):
                var t = traits; t.italic = true
                out += buildAttributed(c, traits: t, style: style, color: color, scale: scale)
            case .underline(let c):
                var t = traits; t.underline = true
                out += buildAttributed(c, traits: t, style: style, color: color, scale: scale)
            case .strike(let c):
                var t = traits; t.strike = true
                out += buildAttributed(c, traits: t, style: style, color: color, scale: scale)
            case .spoiler(let c):
                var seg = buildAttributed(c, traits: traits, style: style, color: color, scale: scale)
                if !spoilersRevealed {
                    // Masked: same foreground and background so the shape is hidden but the space stays
                    // (tapping the paragraph's `Text` reveals every spoiler in this message at once).
                    seg.foregroundColor = theme.colors.layer3
                    seg.backgroundColor = theme.colors.layer3
                }
                out += seg
            case .code(let c):
                var seg = AttributedString(c)
                seg.font = theme.type.code.font(scale: scale * 0.95)
                seg.foregroundColor = color
                seg.backgroundColor = theme.colors.bg
                out += seg
            case .link(let c, let url):
                var seg = buildAttributed(c, traits: traits, style: style, color: theme.colors.accentText, scale: scale)
                if let u = URL(string: url) { seg.link = u }
                out += seg
            case .mention(let name):
                var seg = AttributedString("@\(name)")
                seg.font = font(style, Traits(bold: true), scale: scale)
                // Never red: red is for errors only. A mention of me gets the wash too (the row highlight
                // desktop draws for "this mentions me" lives at the row level, see `MessageRow`).
                seg.foregroundColor = theme.colors.accentText
                if name.lowercased() == me?.lowercased() { seg.backgroundColor = theme.colors.accentWash }
                out += seg
            case .customEmoji(let name, _, _):
                var seg = AttributedString(":\(name):")
                seg.font = font(style, Traits(italic: true), scale: scale)
                seg.foregroundColor = theme.colors.textMuted
                out += seg
            }
        }
        return out
    }
}

private extension View {
    @ViewBuilder func applying<V: View>(@ViewBuilder _ transform: (Self) -> V) -> V { transform(self) }
}
