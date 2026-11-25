import Foundation

// Message markup: the subset the desktop client renders (`**bold**`, `*italic*`, `__underline__`,
// `~~strike~~`, `` `code` ``, `||spoiler||`, links, `@mentions`, `<:name:id>` custom emoji, fenced code,
// quotes, lists, headings). A Swift port of `body.rs` / the Android `Markdown.kt` twin: a small AST,
// not `AttributedString(markdown:)` (a different dialect with no spoilers, custom emoji or mentions).

public indirect enum Inline: Sendable, Equatable {
    case text(String)
    case bold([Inline])
    case italic([Inline])
    case underline([Inline])
    case strike([Inline])
    case spoiler([Inline])
    case code(String)
    case link([Inline], url: String)
    /// `@name`, without the at sign.
    case mention(String)
    /// `<:name:id>`, or `<a:name:id>` when `animated`.
    case customEmoji(name: String, id: ID, animated: Bool)
}

public enum Block: Sendable, Equatable {
    case paragraph([Inline])
    case heading(level: Int, [Inline])
    case quote([Inline])
    case codeBlock(language: String, code: String)
    case list(ordered: Bool, items: [[Inline]])
}

public enum Markdown {
    /// `<:name:id>` (desktop/composer format); `animated` prefixes the name with `a`.
    public static func customEmojiToken(name: String, id: ID, animated: Bool) -> String {
        "<\(animated ? "a" : ""):\(name):\(id)>"
    }

    public static func parse(_ text: String) -> [Block] {
        var blocks: [Block] = []
        let lines = text.components(separatedBy: "\n")
        var i = 0
        var para: [String] = []
        func flush() {
            if !para.isEmpty { blocks.append(.paragraph(inline(para.joined(separator: "\n")))) }
            para.removeAll()
        }
        while i < lines.count {
            let line = lines[i]
            if line.hasPrefix("```") {
                flush()
                let lang = String(line.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                var code: [String] = []
                i += 1
                while i < lines.count, !lines[i].hasPrefix("```") { code.append(lines[i]); i += 1 }
                blocks.append(.codeBlock(language: lang, code: code.joined(separator: "\n")))
            } else if line.hasPrefix(">>> ") {
                flush()
                // `>>>` quotes to the end of the message (desktop's "block quote", MD-01).
                var quote: [String] = [String(line.dropFirst(4))]
                i += 1
                while i < lines.count { quote.append(lines[i]); i += 1 }
                i -= 1
                blocks.append(.quote(inline(quote.joined(separator: "\n"))))
            } else if line.hasPrefix("> ") {
                flush()
                var quote: [String] = []
                while i < lines.count, lines[i].hasPrefix("> ") { quote.append(String(lines[i].dropFirst(2))); i += 1 }
                i -= 1
                blocks.append(.quote(inline(quote.joined(separator: "\n"))))
            } else if let h = headingMatch(line) {
                flush()
                blocks.append(.heading(level: h.level, inline(h.text)))
            } else if isBullet(line) || orderedMatch(line) != nil {
                flush()
                let ordered = orderedMatch(line) != nil
                var items: [[Inline]] = []
                while i < lines.count {
                    if ordered, let m = orderedMatch(lines[i]) { items.append(inline(m)); i += 1 }
                    else if !ordered, isBullet(lines[i]) { items.append(inline(String(lines[i].dropFirst(2)))); i += 1 }
                    else { break }
                }
                i -= 1
                blocks.append(.list(ordered: ordered, items: items))
            } else if line.trimmingCharacters(in: .whitespaces).isEmpty {
                flush()
            } else {
                para.append(line)
            }
            i += 1
        }
        flush()
        return blocks
    }

    private static func isBullet(_ line: String) -> Bool { line.hasPrefix("- ") || line.hasPrefix("* ") }

    private static func orderedMatch(_ line: String) -> String? {
        guard let dot = line.firstIndex(of: "."), Int(line[line.startIndex..<dot]) != nil,
              line.index(after: dot) < line.endIndex, line[line.index(after: dot)] == " " else { return nil }
        return String(line[line.index(dot, offsetBy: 2)...])
    }

    private static func headingMatch(_ line: String) -> (level: Int, text: String)? {
        var level = 0
        var i = line.startIndex
        while i < line.endIndex, line[i] == "#", level < 3 { level += 1; i = line.index(after: i) }
        guard level > 0, i < line.endIndex, line[i] == " " else { return nil }
        return (level, String(line[line.index(after: i)...]))
    }

    /// Parses inline markup; unmatched delimiters stay literal text.
    public static func inline(_ s: String) -> [Inline] {
        var out: [Inline] = []
        var text = ""
        let chars = Array(s)
        var i = 0
        func flush() { if !text.isEmpty { out.append(.text(text)); text = "" } }

        /// Matches a symmetric span like `**bold**`: a closing occurrence after a non-empty run, preferring
        /// the last of a longer run of the same marker (`**a *b***` closes both at the end).
        func span(_ open: String, _ make: ([Inline]) -> Inline) -> Bool {
            let openChars = Array(open)
            guard i + openChars.count <= chars.count, Array(chars[i..<i + openChars.count]) == openChars else { return false }
            var searchFrom = i + openChars.count
            var end = -1
            while searchFrom + openChars.count <= chars.count {
                if Array(chars[searchFrom..<searchFrom + openChars.count]) == openChars { end = searchFrom; break }
                searchFrom += 1
            }
            guard end > i + openChars.count else { return false }
            while end + openChars.count < chars.count, chars[end + openChars.count] == openChars[0], openChars.count > 1 { end += 1 }
            flush()
            out.append(make(inline(String(chars[(i + openChars.count)..<end]))))
            i = end + openChars.count
            return true
        }

        while i < chars.count {
            let c = chars[i]
            if c == "\\", i + 1 < chars.count, "\\`*_~|[]@<".contains(chars[i + 1]) {
                text.append(chars[i + 1]); i += 2; continue
            }
            if c == "`" {
                if let end = chars[(i + 1)...].firstIndex(of: "`"), end > i + 1 {
                    flush(); out.append(.code(String(chars[(i + 1)..<end]))); i = end + 1; continue
                }
                text.append(c); i += 1; continue
            }
            if span("**", { .bold($0) }) { continue }
            if span("__", { .underline($0) }) { continue }
            if span("~~", { .strike($0) }) { continue }
            if span("||", { .spoiler($0) }) { continue }
            if c == "*" || c == "_" {
                if span(String(c), { .italic($0) }) { continue }
            }
            if c == "[" {
                let rest = String(chars[i...])
                if let closeRange = rest.range(of: "]("), let endParen = rest[closeRange.upperBound...].firstIndex(of: ")") {
                    let label = String(rest[rest.index(rest.startIndex, offsetBy: 1)..<closeRange.lowerBound])
                    let url = String(rest[closeRange.upperBound..<endParen])
                    if url.hasPrefix("http://") || url.hasPrefix("https://") {
                        flush(); out.append(.link(inline(label), url: url))
                        i += rest.distance(from: rest.startIndex, to: endParen) + 1
                        continue
                    }
                }
                text.append(c); i += 1; continue
            }
            if (c == "h"), let url = bareURL(chars, at: i) {
                flush(); out.append(.link([.text(url)], url: url)); i += url.count; continue
            }
            if c == "@", (i == 0 || !isWordChar(chars[i - 1])), let name = mentionName(chars, at: i) {
                flush(); out.append(.mention(name)); i += 1 + name.count; continue
            }
            if c == "<", let emoji = customEmojiMatch(chars, at: i) {
                flush(); out.append(.customEmoji(name: emoji.name, id: emoji.id, animated: emoji.animated)); i += emoji.length; continue
            }
            text.append(c); i += 1
        }
        flush()
        return out
    }

    private static func isWordChar(_ c: Character) -> Bool { c.isLetter || c.isNumber || c == "_" }

    private static func mentionName(_ chars: [Character], at i: Int) -> String? {
        var j = i + 1
        while j < chars.count, isWordChar(chars[j]) { j += 1 }
        return j > i + 1 ? String(chars[(i + 1)..<j]) : nil
    }

    private static func bareURL(_ chars: [Character], at i: Int) -> String? {
        let rest = String(chars[i...])
        guard rest.hasPrefix("https://") || rest.hasPrefix("http://") else { return nil }
        var end = rest.startIndex
        while end < rest.endIndex, !" \t\n<>()".contains(rest[end]) { end = rest.index(after: end) }
        var url = String(rest[rest.startIndex..<end])
        while let last = url.last, ".,;:!?'\"".contains(last) { url.removeLast() }
        return url.isEmpty ? nil : url
    }

    private static func customEmojiMatch(_ chars: [Character], at i: Int) -> (name: String, id: ID, animated: Bool, length: Int)? {
        var j = i + 1
        var animated = false
        if j < chars.count, chars[j] == "a" { animated = true; j += 1 }
        guard j < chars.count, chars[j] == ":" else { return nil }
        j += 1
        let nameStart = j
        while j < chars.count, isWordChar(chars[j]) { j += 1 }
        guard j > nameStart, j < chars.count, chars[j] == ":" else { return nil }
        let name = String(chars[nameStart..<j])
        j += 1
        let idStart = j
        while j < chars.count, chars[j].isNumber { j += 1 }
        guard j > idStart, j < chars.count, chars[j] == ">", let id = ID(String(chars[idStart..<j])) else { return nil }
        return (name, id, animated, j + 1 - i)
    }
}

/// A message of only emoji (desktop's jumbo rule, MD-07): standalone Unicode or custom-emoji, no other
/// text, at most 27 of them (a custom emoji inline, or each emoji grapheme of a text run, is one).
public enum Jumbo {
    public static func inlines(of blocks: [Block]) -> [Inline]? {
        guard blocks.count == 1, case .paragraph(let inlines) = blocks[0] else { return nil }
        guard !inlines.isEmpty else { return nil }
        var units = 0
        for inline in inlines {
            switch inline {
            case .customEmoji:
                units += 1
            case .text(let t):
                for ch in t {
                    if ch.isWhitespace { continue }
                    guard ch.isEmoji else { return nil }
                    units += 1
                }
            default: return nil
            }
        }
        guard units > 0, units <= 27 else { return nil }
        return inlines
    }
}

private extension Character {
    /// A single Unicode scalar that is emoji by itself (not a digit/`#`/`*`, which are emoji-eligible but
    /// plain text unless followed by a variation selector).
    var isSimpleEmoji: Bool {
        guard let first = unicodeScalars.first else { return false }
        return first.properties.isEmoji && (first.value > 0x238C || unicodeScalars.count > 1)
    }

    /// A multi-scalar grapheme built from joiners/variation selectors/skin tones into one emoji.
    var isEmojiCombination: Bool {
        unicodeScalars.count > 1 && unicodeScalars.contains { $0.properties.isEmoji }
    }

    var isEmoji: Bool { isSimpleEmoji || isEmojiCombination }
}
