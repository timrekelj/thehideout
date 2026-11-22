import HideoutDesign
import SwiftUI

/// A message's text: paragraphs with inline markdown (bold, italic, strike, code, links, @mentions), headings,
/// quotes, lists and fenced code blocks. A deliberately small renderer; the full parser (spoilers, underline,
/// jumbo emoji) with desktop's contract vectors lands in HideoutCore in phase 3.
public struct MessageBody: View {
    @Environment(\.theme) private var theme
    @Environment(\.colorScheme) private var scheme
    private let dynamic = DynamicScale()
    let text: String
    let me: String?

    public init(_ text: String, me: String? = nil) {
        self.text = text
        self.me = me
    }

    enum Block: Equatable {
        case paragraph(String)
        case heading(String)
        case quote(String)
        case bullet(String)
        case number(Int, String)
        case code(String)
    }

    static func parse(_ text: String) -> [Block] {
        var blocks: [Block] = []
        var para: [String] = []
        var code: [String]?
        func flush() { if !para.isEmpty { blocks.append(.paragraph(para.joined(separator: "\n"))); para = [] } }
        for raw in text.components(separatedBy: "\n") {
            if raw.hasPrefix("```") {
                if let c = code { blocks.append(.code(c.joined(separator: "\n"))); code = nil } else { flush(); code = [] }
                continue
            }
            if code != nil { code!.append(raw); continue }
            if raw.hasPrefix("# ") { flush(); blocks.append(.heading(String(raw.dropFirst(2)))) }
            else if raw.hasPrefix("> ") { flush(); blocks.append(.quote(String(raw.dropFirst(2)))) }
            else if raw.hasPrefix("- ") { flush(); blocks.append(.bullet(String(raw.dropFirst(2)))) }
            else if let dot = raw.firstIndex(of: "."), let n = Int(raw[..<dot]), raw[raw.index(after: dot)...].hasPrefix(" ") {
                flush(); blocks.append(.number(n, String(raw[raw.index(dot, offsetBy: 2)...])))
            } else { para.append(raw) }
        }
        if let c = code { blocks.append(.code(c.joined(separator: "\n"))) }
        flush()
        return blocks
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            ForEach(Array(Self.parse(text).enumerated()), id: \.offset) { _, block in
                switch block {
                case .paragraph(let t): inline(t)
                case .heading(let t): inline(t, style: theme.type.heading)
                case .quote(let t):
                    HStack(alignment: .top, spacing: 10) {
                        RoundedRectangle(cornerRadius: 1.5).fill(theme.colors.borderStrong).frame(width: 3)
                        inline(t, color: theme.colors.textMuted)
                    }
                case .bullet(let t):
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("•").foregroundStyle(theme.colors.textFaint)
                        inline(t)
                    }.padding(.leading, 4).padding(.vertical, -2)
                case .number(let n, let t):
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("\(n).").monospacedDigit().foregroundStyle(theme.colors.textFaint)
                        inline(t)
                    }.padding(.leading, 4).padding(.vertical, -2)
                case .code(let t):
                    Text(t)
                        .font(theme.type.code.font(scale: dynamic.wrappedValue))
                        .foregroundStyle(theme.colors.text)
                        .padding(Space.s3)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(theme.colors.bg, in: RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
                        .overlay(RoundedRectangle(cornerRadius: Radius.md, style: .continuous).strokeBorder(theme.colors.borderSubtle, lineWidth: 1))
                        .textSelection(.enabled)
                }
            }
        }
    }

    private func inline(_ t: String, style: TypeStyle? = nil, color: Color? = nil) -> some View {
        Text(attributed(t, style: style ?? theme.type.body, color: color ?? theme.colors.text))
            .lineSpacing(((style ?? theme.type.body).lineSpacing) * dynamic.wrappedValue)
            .fixedSize(horizontal: false, vertical: true)
    }

    /// Splits `source` on `__underline__` and `||spoiler||` markers; the rest is parsed as inline markdown.
    static func segments(_ source: String) -> [(text: String, underline: Bool, spoiler: Bool)] {
        var out: [(String, Bool, Bool)] = []
        var current = ""
        var underline = false, spoiler = false
        var i = source.startIndex
        func flush() { if !current.isEmpty { out.append((current, underline, spoiler)); current = "" } }
        while i < source.endIndex {
            let rest = source[i...]
            if rest.hasPrefix("__"), underline || source[source.index(i, offsetBy: 2)...].contains("__") {
                flush(); underline.toggle(); i = source.index(i, offsetBy: 2); continue
            }
            if rest.hasPrefix("||"), spoiler || source[source.index(i, offsetBy: 2)...].contains("||") {
                flush(); spoiler.toggle(); i = source.index(i, offsetBy: 2); continue
            }
            current.append(source[i]); i = source.index(after: i)
        }
        flush()
        return out
    }

    func attributed(_ source: String, style: TypeStyle, color: Color) -> AttributedString {
        let scale = dynamic.wrappedValue
        let options = AttributedString.MarkdownParsingOptions(allowsExtendedAttributes: false, interpretedSyntax: .inlineOnlyPreservingWhitespace)
        var out = AttributedString()
        for seg in Self.segments(source) {
            var part = (try? AttributedString(markdown: seg.text, options: options)) ?? AttributedString(seg.text)
            part.font = style.font(scale: scale)
            part.foregroundColor = color
            for run in part.runs {
                let intent = run.inlinePresentationIntent ?? []
                if intent.contains(.code) {
                    part[run.range].font = theme.type.code.font(scale: scale * 0.95)
                    part[run.range].backgroundColor = theme.colors.bg
                } else if intent.contains(.stronglyEmphasized) {
                    part[run.range].font = DesignFont.ui(size: style.size * scale, weight: 650)
                } else if intent.contains(.emphasized) {
                    part[run.range].font = style.font(scale: scale).italic()
                }
                if intent.contains(.strikethrough) { part[run.range].strikethroughStyle = .single }
                if run.link != nil { part[run.range].foregroundColor = theme.colors.accentText }
            }
            if seg.underline { part.underlineStyle = .single }
            if seg.spoiler {
                // Hidden until revealed (tap to reveal arrives with the transcript): a flat block in the layer colour.
                part.foregroundColor = theme.colors.layer3
                part.backgroundColor = theme.colors.layer3
            }
            out += part
        }
        highlightMentions(&out)
        linkBareURLs(&out)
        return out
    }

    private func highlightMentions(_ s: inout AttributedString) {
        let plain = String(s.characters)
        var i = plain.startIndex
        while let at = plain[i...].firstIndex(of: "@") {
            let start = plain.index(after: at)
            var end = start
            while end < plain.endIndex, plain[end].isLetter || plain[end].isNumber || plain[end] == "_" { end = plain.index(after: end) }
            if end > start {
                let name = String(plain[start..<end])
                let isMe = name.lowercased() == me?.lowercased()
                if let lo = AttributedString.Index(at, within: s), let hi = AttributedString.Index(end, within: s) {
                    s[lo..<hi].foregroundColor = isMe ? theme.colors.accentText : theme.colors.danger
                    s[lo..<hi].backgroundColor = isMe ? theme.colors.accentWash : theme.colors.dangerBg
                }
            }
            i = end > start ? end : start
        }
    }

    private func linkBareURLs(_ s: inout AttributedString) {
        let plain = String(s.characters)
        guard let detector = try? NSDataDetector(types: NSTextCheckingResult.CheckingType.link.rawValue) else { return }
        for m in detector.matches(in: plain, range: NSRange(plain.startIndex..., in: plain)) {
            guard let r = Range(m.range, in: plain), let url = m.url,
                  let lo = AttributedString.Index(r.lowerBound, within: s), let hi = AttributedString.Index(r.upperBound, within: s),
                  s[lo..<hi].link == nil else { continue }
            s[lo..<hi].link = url
            s[lo..<hi].foregroundColor = theme.colors.accentText
        }
    }
}
