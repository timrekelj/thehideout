import Foundation

/// Link-embed layout (ATT-06/07), a Swift port of desktop's `components::chat::embed` (and Android's
/// `core/embed/EmbedLayout.kt` twin): which provider a card is, the colour of its left rail, whether a
/// plain link's thumbnail sits beside the text or below it, and which embeds of the transcript are folded.
public enum EmbedProvider: Sendable, Equatable {
    case link, x, tiktok, instagram, reddit

    public var label: String {
        switch self {
        case .link: return "Link"
        case .x: return "X"
        case .tiktok: return "TikTok"
        case .instagram: return "Instagram"
        case .reddit: return "Reddit"
        }
    }

    /// The channel palette key of this provider's rail colour; nil for a plain link (the theme's
    /// neutral `borderStrong` stands in).
    public var paletteKey: String? {
        switch self {
        case .link: return nil
        case .x: return "slate"
        case .tiktok: return "cyan"
        case .instagram: return "pink"
        case .reddit: return "orange"
        }
    }

    /// Whether this provider draws the rich post body (author, text, media, "Open original") rather
    /// than a plain link card.
    public var isPost: Bool { self != .link }

    /// From the wire's provider key ("x", "tiktok", "instagram", "reddit"); anything else is a plain link.
    public static func from(key: String) -> EmbedProvider {
        switch key {
        case "x": return .x
        case "tiktok": return .tiktok
        case "instagram": return .instagram
        case "reddit": return .reddit
        default: return .link
        }
    }
}

/// Widest an embed card grows, in points.
public let embedMaxWidth: Double = 440
/// Lines of description a link card shows before it truncates.
public let embedDescriptionLines = 3
/// Side of a plain link's thumbnail when it sits beside the text, in points.
public let embedSideThumb: Double = 96
/// Widest/tallest a plain link's thumbnail grows when it sits below the text, in points.
public let embedBelowThumbMaxW: Double = 400
public let embedBelowThumbMaxH: Double = 225

/// Whether a plain link's thumbnail sits beside its text (a small square) instead of below it (a wide
/// picture): true for a roughly landscape-or-wider image (aspect < 1.4), same rule as desktop's
/// `embed.rs`. Unknown or missing dimensions default to below.
public func embedThumbnailSide(width: Int?, height: Int?) -> Bool {
    guard let w = width, let h = height, h > 0 else { return false }
    return Double(w) / Double(h) < 1.4
}

/// One embed of one message, identified the way the folded set remembers it.
public struct EmbedRef: Sendable, Equatable, Hashable {
    public var messageID: ID
    public var index: Int
    public init(messageID: ID, index: Int) { self.messageID = messageID; self.index = index }
}

/// How many folded embeds the app remembers at once (desktop's `EMBED_PREFS_CAP`).
public let embedCollapseCap = 400
