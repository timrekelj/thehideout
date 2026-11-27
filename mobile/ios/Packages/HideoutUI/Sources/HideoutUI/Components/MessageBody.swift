import HideoutCore
import HideoutDesign
import SwiftUI
import UIKit

/// A message's text, built from `HideoutCore.Markdown`'s AST (not `AttributedString(markdown:)`, a
/// different dialect): paragraphs with inline bold/italic/underline/strike/code, links, `@mentions` (always
/// the accent, never red: red is for errors only), spoilers (tap to reveal, one flag per message), jumbo
/// emoji, headings, quotes, lists and fenced code blocks (horizontal scroll). Custom emoji (MD-06) draw as
/// pictures when `customEmojis` resolves their id, `:name:` text otherwise (a deleted emoji, or none to
/// resolve against in a DM); inline ones never animate (so they can sit in a single flowing `Text`, same
/// as desktop), only a jumbo message's (MD-07) do, as real views in a wrapping layout.
public struct MessageBody: View {
    @Environment(\.theme) private var theme
    @Environment(ServerSelection.self) private var server
    @Environment(\.allowsFakeAssets) private var allowsFakeAssets
    private let dynamic = DynamicScale()
    let text: String
    let me: String?
    let customEmojis: [ID: Emoji]
    @State private var spoilersRevealed = false
    /// Emoji whose picture has been fetched into `MediaImageCache` since this view last rendered: a
    /// `:name:` token flips to its picture once this changes, the same "cached at creation, else fetch
    /// and settle" shape `AttachmentAsyncImage` uses.
    @State private var loadedEmoji: Set<ID> = []
    /// The whole message's custom emoji names, shown briefly on a long press — one flag for the whole
    /// message (the same simplification `spoilersRevealed` already makes): SwiftUI's `Text` has no way
    /// to attach a gesture to one inline run, only to the paragraph as a whole.
    @State private var showEmojiNames = false

    public init(_ text: String, me: String? = nil, customEmojis: [ID: Emoji] = [:]) {
        self.text = text
        self.me = me
        self.customEmojis = customEmojis
    }

    public var body: some View {
        let blocks = Markdown.parse(text)
        VStack(alignment: .leading, spacing: 5) {
            if let jumbo = Jumbo.inlines(of: blocks) {
                jumboRow(jumbo)
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

    // MARK: jumbo (MD-07): real views, so a custom emoji can draw large and animate

    private func jumboRow(_ inlines: [Inline]) -> some View {
        let scale = dynamic.wrappedValue
        // Desktop's rule: 48pt once the message has any custom emoji (even mixed with Unicode ones),
        // 40pt only for a message that is nothing but Unicode.
        let hasCustom = inlines.contains { if case .customEmoji = $0 { true } else { false } }
        let unicodeSize = (hasCustom ? EMOJI_JUMBO_PT : 40) * scale
        return FlowWrap(spacing: 4) {
            ForEach(Array(inlines.enumerated()), id: \.offset) { _, inline in
                switch inline {
                case .text(let t):
                    ForEach(Array(graphemeClusters(t).enumerated()), id: \.offset) { _, cluster in
                        if !cluster.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            Text(cluster).font(.system(size: unicodeSize))
                        }
                    }
                case .customEmoji(let name, let id, let animated):
                    jumboEmojiCell(id: id, name: name, animated: animated, size: EMOJI_JUMBO_PT * scale)
                default: EmptyView()
                }
            }
        }
    }

    @ViewBuilder private func jumboEmojiCell(id: ID, name: String, animated: Bool, size: CGFloat) -> some View {
        if let emoji = customEmojis[id] {
            CustomEmojiImage(emoji: emoji, size: size)
                .onLongPressGesture(minimumDuration: 0.3) { withAnimation(Motion.control) { showEmojiNames = true } }
                .overlay(alignment: .bottom) {
                    if showEmojiNames {
                        Text(":\(name):").typeStyle(\.caption, color: \.textMuted).fixedSize()
                            .offset(y: 16)
                            .onAppear { Task { try? await Task.sleep(for: .seconds(1.5)); withAnimation(Motion.control) { showEmojiNames = false } } }
                    }
                }
        } else {
            Text(":\(name):").typeStyle(\.bodyStrong, color: \.textMuted)
        }
    }

    private func graphemeClusters(_ s: String) -> [String] { s.map(String.init) }

    // MARK: ordinary paragraphs

    private func paragraph(_ inlines: [Inline], style: TypeStyle? = nil, color: Color? = nil) -> some View {
        let s = style ?? theme.type.body
        let hasEmoji = inlines.contains { if case .customEmoji = $0 { true } else { false } }
        return Group {
            if hasEmoji {
                inlineWithEmoji(inlines, style: s, color: color ?? theme.colors.text)
            } else {
                plainText(inlines, style: s, color: color ?? theme.colors.text)
            }
        }
        .lineSpacing(s.lineSpacing * dynamic.wrappedValue)
        .fixedSize(horizontal: false, vertical: true)
        .applying { view in
            if hasSpoiler(inlines) {
                view.onTapGesture { withAnimation(Motion.control) { spoilersRevealed = true } }
            } else {
                view
            }
        }
    }

    private func plainText(_ inlines: [Inline], style: TypeStyle, color: Color) -> Text {
        Text(buildAttributed(inlines, traits: Traits(), style: style, color: color, scale: dynamic.wrappedValue))
    }

    /// Builds one flowing `Text` for a paragraph/heading/quote/list item that has a custom-emoji token
    /// among its *top-level* inlines: plain runs between them go through the same `buildAttributed` as
    /// ever, each resolvable emoji becomes an inline `Image` (baseline-aligned automatically, the same
    /// as any `Text` string interpolation of an `Image`) at a cached static frame — inline emoji never
    /// animate, matching desktop. A custom emoji nested inside bold/italic/a link falls back to its
    /// `:name:` text, like an unknown token: `buildAttributed`'s own recursive case still covers that,
    /// since only the *outer* split happens here. `Text`'s own `+` operator is deprecated as of iOS 26
    /// in favour of string interpolation, which is what dynamically assembling `interpolation` below
    /// does — the same mechanism `Text("\(image)")` uses, just built up a segment at a time.
    private func inlineWithEmoji(_ inlines: [Inline], style: TypeStyle, color: Color) -> Text {
        var segments: [Inline] = []
        var interpolation = LocalizedStringKey.StringInterpolation(literalCapacity: 0, interpolationCount: inlines.count)
        func flush() {
            guard !segments.isEmpty else { return }
            interpolation.appendInterpolation(plainText(segments, style: style, color: color))
            segments = []
        }
        for inline in inlines {
            if case .customEmoji(let name, let id, _) = inline, let emoji = customEmojis[id] {
                flush()
                if let image = cachedEmojiImage(emoji) {
                    // `Text`'s image interpolation draws the `UIImage` at its own pixel size, not
                    // scaled to the font's line height: resize it to the inline glyph size ourselves
                    // first (`Image(uiImage:).resizable()` has no effect once interpolated into `Text`).
                    interpolation.appendInterpolation(Image(uiImage: image.resized(toPoints: EMOJI_INLINE_PT)))
                } else {
                    interpolation.appendInterpolation(Text(":\(name):").foregroundColor(theme.colors.textMuted))
                    scheduleEmojiLoad(emoji)
                }
            } else {
                segments.append(inline)
            }
        }
        flush()
        return Text(LocalizedStringKey(stringInterpolation: interpolation))
    }

    private func emojiSource(_ emoji: Emoji) -> MediaSource? {
        try? mediaSource(base: server.url, key: emoji.image, allowFakeAssets: allowsFakeAssets)
    }

    private func cachedEmojiImage(_ emoji: Emoji) -> UIImage? {
        guard let source = emojiSource(emoji) else { return nil }
        return MediaImageCache.shared.cachedImage(for: source)
    }

    /// Fetches `emoji`'s picture into `MediaImageCache` once, then flips `loadedEmoji` so this view's
    /// next render picks the now-cached image up (mirrors `AttachmentAsyncImage`'s own settle-in).
    private func scheduleEmojiLoad(_ emoji: Emoji) {
        guard !loadedEmoji.contains(emoji.id), let source = emojiSource(emoji) else { return }
        Task {
            if case .loaded = await MediaImageCache.shared.outcome(for: source) { loadedEmoji.insert(emoji.id) }
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
                // Only reached when a token is nested too deep to be split out by `inlineWithEmoji` (bold,
                // a link, ...): the same `:name:` fallback an unresolved token always shows.
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

private extension UIImage {
    /// A square image fit within `points` x `points` at the image's own scale, aspect-preserved: what
    /// an inline custom emoji's `Text` interpolation needs, since the interpolated `Image` draws at
    /// the `UIImage`'s own pixel size rather than scaling to the surrounding text.
    func resized(toPoints points: CGFloat) -> UIImage {
        let factor = min(points / max(size.width, 1), points / max(size.height, 1))
        guard factor < 1 else { return self }
        let target = CGSize(width: size.width * factor, height: size.height * factor)
        let renderer = UIGraphicsImageRenderer(size: target, format: imageRendererFormat)
        return renderer.image { _ in draw(in: CGRect(origin: .zero, size: target)) }
    }
}
