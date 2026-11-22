import Foundation

/// What the composer is suggesting, derived from the text before the caret.
public enum Trigger: Sendable, Equatable {
    case mention(query: String, start: Int)
    case emojiShortcode(query: String, start: Int)
    case none
}

/// Mobile-only niceties (desktop has no `@` autocomplete, MSG-12): `@mention` and `:emoji:` suggestions
/// while typing, and the emoji search the picker shares.
public enum Autocomplete {
    /// What the composer should suggest for `text` with the caret at `caret` (both in UTF-16 offsets, as
    /// `NSRange`/`UITextView` give them): the current word (the unbroken run of non-whitespace back from the
    /// caret) triggers only when *it itself* starts with `@` or `:` - so `12:3`, `re:sm` and `user@ex` are
    /// plain text, not `3`/`sm`/`ex` queries.
    public static func trigger(_ text: String, caret: Int) -> Trigger {
        let units = Array(text.utf16)
        guard caret > 0, caret <= units.count else { return .none }
        var start = caret - 1
        while start >= 0, !isBoundary(units[start]) { start -= 1 }
        let wordStart = start + 1
        guard wordStart < caret else { return .none }
        let queryUnits = Array(units[(wordStart + 1)..<caret])
        let query = String(utf16CodeUnits: queryUnits, count: queryUnits.count)
        switch units[wordStart] {
        case 0x3A: return .emojiShortcode(query: query, start: wordStart) // ":"
        case 0x40: return .mention(query: query, start: wordStart) // "@"
        default: return .none
        }
    }

    private static func isBoundary(_ u: UInt16) -> Bool {
        guard let scalar = Unicode.Scalar(u) else { return true }
        return Character(scalar).isWhitespace || Character(scalar).isNewline
    }

    /// Members whose username starts with `query` (case-insensitive), up to `limit`, as desktop's `@` chip does.
    public static func mentionMatches(_ query: String, members: [User], limit: Int = 8) -> [User] {
        let sorted = members.sorted { $0.username.lowercased() < $1.username.lowercased() }
        guard !query.isEmpty else { return Array(sorted.prefix(limit)) }
        return Array(sorted.filter { $0.username.lowercased().hasPrefix(query.lowercased()) }.prefix(limit))
    }

    /// `:sm` -> up to 8 rows, server emoji first (MSG-10).
    public static func emojiMatches(_ query: String, serverEmojis: [Emoji], limit: Int = 8) -> [EmojiEntry] {
        let custom = serverEmojis.filter { $0.name.localizedCaseInsensitiveContains(query) }
            .sorted { $0.name.lowercased() < $1.name.lowercased() }
            .map { EmojiEntry.custom(id: $0.id, name: $0.name, image: $0.image, animated: $0.animated) }
        let unicode = EmojiData.search(query)
        return Array((custom + unicode).prefix(limit))
    }
}
