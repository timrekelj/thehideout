import Foundation

/// One picker cell: a Unicode character, or a guild's custom emoji (MSG-07/MSG-09).
public enum EmojiEntry: Sendable, Equatable, Identifiable, Hashable {
    case unicode(emoji: String, name: String)
    case custom(id: ID, name: String, image: String, animated: Bool)

    public var id: String {
        switch self {
        case .unicode(let emoji, _): "u:\(emoji)"
        case .custom(let id, _, _, _): "c:\(id)"
        }
    }

    public var name: String {
        switch self {
        case .unicode(_, let name): name
        case .custom(_, let name, _, _): name
        }
    }

    /// What typing it inserts into the composer (desktop format): the glyph for Unicode, `<:name:id>` /
    /// `<a:name:id>` for a custom emoji.
    public var insertion: String {
        switch self {
        case .unicode(let emoji, _): emoji
        case .custom(let id, let name, _, let animated): Markdown.customEmojiToken(name: name, id: id, animated: animated)
        }
    }
}

public enum EmojiCategory: String, Sendable, CaseIterable {
    case smileys, animals, food, activities, travel, objects, symbols, flags

    public var label: String {
        switch self {
        case .smileys: "Smileys & people"
        case .animals: "Animals & nature"
        case .food: "Food & drink"
        case .activities: "Activities"
        case .travel: "Travel & places"
        case .objects: "Objects"
        case .symbols: "Symbols"
        case .flags: "Flags"
        }
    }
}

/// The bundled Unicode emoji set the picker and `:shortcode` autocomplete search, limited to Unicode 15.1
/// (MSG-08): no 16.0+ additions the OS font may not draw.
///
/// This is a curated subset, not the full `emojis` crate desktop uses (follow-up: export the same data
/// desktop's `emoji_picker.rs` reads to a JSON asset, so every client's picker shows exactly the same set
/// and names; `EmojiData.kt` on Android carries the identical list so the fake worlds match).
public enum EmojiData {
    public static let all: [(EmojiCategory, String, String)] = {
        var out: [(EmojiCategory, String, String)] = []
        func add(_ category: EmojiCategory, _ pairs: [(String, String)]) {
            out.append(contentsOf: pairs.map { (category, $0.0, $0.1) })
        }
        add(.smileys, [
            ("😀", "grinning face"), ("😃", "grinning face with big eyes"), ("😄", "grinning face with smiling eyes"),
            ("😁", "beaming face with smiling eyes"), ("😆", "grinning squinting face"), ("😅", "grinning face with sweat"),
            ("🤣", "rolling on the floor laughing"), ("😂", "face with tears of joy"), ("🙂", "slightly smiling face"),
            ("🙃", "upside-down face"), ("😉", "winking face"), ("😊", "smiling face with smiling eyes"),
            ("😇", "smiling face with halo"), ("🥰", "smiling face with hearts"), ("😍", "heart eyes"),
            ("🤩", "star-struck"), ("😘", "face blowing a kiss"), ("😗", "kissing face"),
            ("😎", "smiling face with sunglasses"), ("🤓", "nerd face"), ("🧐", "face with monocle"),
            ("🤔", "thinking face"), ("🤨", "face with raised eyebrow"), ("😐", "neutral face"),
            ("😑", "expressionless face"), ("😶", "face without mouth"), ("🙄", "face with rolling eyes"),
            ("😏", "smirking face"), ("😣", "persevering face"), ("😥", "sad but relieved face"),
            ("😮", "face with open mouth"), ("🤐", "zipper-mouth face"), ("😯", "hushed face"),
            ("😪", "sleepy face"), ("😫", "tired face"), ("🥱", "yawning face"), ("😴", "sleeping face"),
            ("😌", "relieved face"), ("😛", "face with tongue"), ("😜", "winking face with tongue"),
            ("😝", "squinting face with tongue"), ("🤤", "drooling face"), ("😒", "unamused face"),
            ("😓", "downcast face with sweat"), ("😔", "pensive face"), ("😕", "confused face"),
            ("🙁", "slightly frowning face"), ("😖", "confounded face"), ("😞", "disappointed face"),
            ("😟", "worried face"), ("😤", "face with steam from nose"), ("😢", "crying face"),
            ("😭", "loudly crying face"), ("😦", "frowning face with open mouth"), ("😧", "anguished face"),
            ("😨", "fearful face"), ("😩", "weary face"), ("🤯", "exploding head"), ("😬", "grimacing face"),
            ("😰", "anxious face with sweat"), ("😱", "face screaming in fear"), ("🥵", "hot face"),
            ("🥶", "cold face"), ("😳", "flushed face"), ("🤪", "zany face"), ("😵", "dizzy face"),
            ("😡", "pouting face"), ("😠", "angry face"), ("🤬", "face with symbols on mouth"),
            ("😷", "face with medical mask"), ("🤒", "face with thermometer"), ("🤕", "face with head-bandage"),
            ("🤢", "nauseated face"), ("🤮", "face vomiting"), ("🤧", "sneezing face"), ("🥳", "partying face"),
            ("🥺", "pleading face"), ("🤠", "cowboy hat face"), ("🤡", "clown face"), ("👻", "ghost"),
            ("💀", "skull"), ("👽", "alien"), ("🤖", "robot"), ("👍", "thumbs up"), ("👎", "thumbs down"),
            ("👏", "clapping hands"), ("🙌", "raising hands"), ("👋", "waving hand"), ("🤝", "handshake"),
            ("🙏", "folded hands"), ("❤️", "red heart"), ("💙", "blue heart"), ("💚", "green heart"),
            ("💛", "yellow heart"), ("💜", "purple heart"), ("🖤", "black heart"), ("💔", "broken heart"),
            ("✨", "sparkles"), ("🎉", "party popper"), ("🔥", "fire"), ("💯", "hundred points"),
        ])
        add(.animals, [
            ("🐶", "dog face"), ("🐱", "cat face"), ("🐭", "mouse face"), ("🐹", "hamster"), ("🐰", "rabbit face"),
            ("🦊", "fox"), ("🐻", "bear"), ("🐼", "panda"), ("🐨", "koala"), ("🐯", "tiger face"),
            ("🦁", "lion"), ("🐮", "cow face"), ("🐷", "pig face"), ("🐸", "frog"), ("🐵", "monkey face"),
            ("🐔", "chicken"), ("🐧", "penguin"), ("🐦", "bird"), ("🦆", "duck"), ("🦉", "owl"),
            ("🐴", "horse face"), ("🦄", "unicorn"), ("🐝", "honeybee"), ("🦋", "butterfly"), ("🐢", "turtle"),
            ("🐍", "snake"), ("🐙", "octopus"), ("🐳", "whale"), ("🐬", "dolphin"), ("🦈", "shark"),
        ])
        add(.food, [
            ("🍎", "red apple"), ("🍌", "banana"), ("🍇", "grapes"), ("🍓", "strawberry"), ("🍍", "pineapple"),
            ("🥑", "avocado"), ("🍕", "pizza"), ("🍔", "hamburger"), ("🍟", "french fries"), ("🌭", "hot dog"),
            ("🍿", "popcorn"), ("🍣", "sushi"), ("🍜", "steaming bowl"), ("🍩", "doughnut"), ("🍪", "cookie"),
            ("🎂", "birthday cake"), ("🍰", "shortcake"), ("☕", "hot beverage"), ("🍺", "beer mug"), ("🍷", "wine glass"),
        ])
        add(.activities, [
            ("⚽", "soccer ball"), ("🏀", "basketball"), ("🏈", "american football"), ("⚾", "baseball"),
            ("🎾", "tennis"), ("🎮", "video game"), ("🎲", "game die"), ("🎸", "guitar"), ("🎨", "artist palette"),
            ("🎬", "clapper board"), ("🏆", "trophy"), ("🥇", "first place medal"), ("🎯", "direct hit"),
        ])
        add(.travel, [
            ("🚗", "automobile"), ("🚕", "taxi"), ("🚲", "bicycle"), ("✈️", "airplane"), ("🚀", "rocket"),
            ("🏠", "house"), ("🏢", "office building"), ("🗽", "statue of liberty"), ("🗻", "mount fuji"),
            ("🏖️", "beach with umbrella"), ("🌋", "volcano"), ("🌙", "crescent moon"), ("⭐", "star"), ("☀️", "sun"),
        ])
        add(.objects, [
            ("💡", "light bulb"), ("📱", "mobile phone"), ("💻", "laptop"), ("⌨️", "keyboard"), ("🖥️", "desktop computer"),
            ("📷", "camera"), ("🔑", "key"), ("🔒", "locked"), ("📎", "paperclip"), ("✏️", "pencil"),
            ("📚", "books"), ("🎁", "wrapped gift"), ("💰", "money bag"), ("⏰", "alarm clock"),
        ])
        add(.symbols, [
            ("✅", "check mark button"), ("❌", "cross mark"), ("⚠️", "warning"), ("❓", "question mark"),
            ("❗", "exclamation mark"), ("➕", "plus"), ("➖", "minus"), ("🔄", "counterclockwise arrows"),
            ("🔔", "bell"), ("🔕", "bell with slash"), ("💬", "speech balloon"), ("💤", "zzz"),
        ])
        add(.flags, [
            ("🏳️", "white flag"), ("🏁", "chequered flag"), ("🚩", "triangular flag"),
        ])
        return out
    }()

    /// `:sm` matches names that start with, then contain, `sm`; case-insensitive.
    public static func search(_ query: String, limit: Int = 24) -> [EmojiEntry] {
        if query.isEmpty { return all.prefix(limit).map { .unicode(emoji: $0.1, name: $0.2) } }
        let q = query.lowercased()
        let starts = all.filter { $0.2.lowercased().hasPrefix(q) }
        let startIDs = Set(starts.map(\.1))
        let contains = all.filter { $0.2.lowercased().contains(q) && !startIDs.contains($0.1) }
        return (starts + contains).prefix(limit).map { .unicode(emoji: $0.1, name: $0.2) }
    }

    public static func inCategory(_ category: EmojiCategory) -> [EmojiEntry] {
        all.filter { $0.0 == category }.map { .unicode(emoji: $0.1, name: $0.2) }
    }
}
