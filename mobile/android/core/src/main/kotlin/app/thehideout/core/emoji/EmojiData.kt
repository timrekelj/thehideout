package app.thehideout.core.emoji

/** One picker cell: a Unicode character, or a guild's custom emoji (MSG-07/MSG-09). */
sealed interface EmojiEntry {
    data class Unicode(val emoji: String, val name: String) : EmojiEntry
    data class Custom(val id: Long, val name: String, val image: String, val animated: Boolean) : EmojiEntry
}

enum class EmojiCategory(val label: String) {
    Smileys("Smileys & people"),
    Animals("Animals & nature"),
    Food("Food & drink"),
    Activities("Activities"),
    Travel("Travel & places"),
    Objects("Objects"),
    Symbols("Symbols"),
    Flags("Flags"),
}

/**
 * The bundled Unicode emoji set the picker and `:shortcode` autocomplete search, limited to
 * Unicode 15.1 (MSG-08): no 16.0+ additions the OS font may not draw.
 *
 * This is a curated subset, not the full `emojis` crate desktop uses (follow-up: export the same
 * data desktop's `emoji_picker.rs` reads to a JSON asset, so both apps' pickers show exactly the
 * same set and names). It still covers every desktop picker category with common entries.
 */
object EmojiData {
    val all: List<Pair<EmojiCategory, EmojiEntry.Unicode>> = buildList {
        fun add(category: EmojiCategory, vararg pairs: Pair<String, String>) {
            pairs.forEach { (emoji, name) -> add(category to EmojiEntry.Unicode(emoji, name)) }
        }
        add(
            EmojiCategory.Smileys,
            "😀" to "grinning face", "😃" to "grinning face with big eyes", "😄" to "grinning face with smiling eyes",
            "😁" to "beaming face with smiling eyes", "😆" to "grinning squinting face", "😅" to "grinning face with sweat",
            "🤣" to "rolling on the floor laughing", "😂" to "face with tears of joy", "🙂" to "slightly smiling face",
            "🙃" to "upside-down face", "😉" to "winking face", "😊" to "smiling face with smiling eyes",
            "😇" to "smiling face with halo", "🥰" to "smiling face with hearts", "😍" to "heart eyes",
            "🤩" to "star-struck", "😘" to "face blowing a kiss", "😗" to "kissing face",
            "😎" to "smiling face with sunglasses", "🤓" to "nerd face", "🧐" to "face with monocle",
            "🤔" to "thinking face", "🤨" to "face with raised eyebrow", "😐" to "neutral face",
            "😑" to "expressionless face", "😶" to "face without mouth", "🙄" to "face with rolling eyes",
            "😏" to "smirking face", "😣" to "persevering face", "😥" to "sad but relieved face",
            "😮" to "face with open mouth", "🤐" to "zipper-mouth face", "😯" to "hushed face",
            "😪" to "sleepy face", "😫" to "tired face", "🥱" to "yawning face", "😴" to "sleeping face",
            "😌" to "relieved face", "😛" to "face with tongue", "😜" to "winking face with tongue",
            "😝" to "squinting face with tongue", "🤤" to "drooling face", "😒" to "unamused face",
            "😓" to "downcast face with sweat", "😔" to "pensive face", "😕" to "confused face",
            "🙁" to "slightly frowning face", "😖" to "confounded face", "😞" to "disappointed face",
            "😟" to "worried face", "😤" to "face with steam from nose", "😢" to "crying face",
            "😭" to "loudly crying face", "😦" to "frowning face with open mouth", "😧" to "anguished face",
            "😨" to "fearful face", "😩" to "weary face", "🤯" to "exploding head", "😬" to "grimacing face",
            "😰" to "anxious face with sweat", "😱" to "face screaming in fear", "🥵" to "hot face",
            "🥶" to "cold face", "😳" to "flushed face", "🤪" to "zany face", "😵" to "dizzy face",
            "😡" to "pouting face", "😠" to "angry face", "🤬" to "face with symbols on mouth",
            "😷" to "face with medical mask", "🤒" to "face with thermometer", "🤕" to "face with head-bandage",
            "🤢" to "nauseated face", "🤮" to "face vomiting", "🤧" to "sneezing face", "🥳" to "partying face",
            "🥺" to "pleading face", "🤠" to "cowboy hat face", "🤡" to "clown face", "👻" to "ghost",
            "💀" to "skull", "👽" to "alien", "🤖" to "robot", "👍" to "thumbs up", "👎" to "thumbs down",
            "👏" to "clapping hands", "🙌" to "raising hands", "👋" to "waving hand", "🤝" to "handshake",
            "🙏" to "folded hands", "❤️" to "red heart", "💙" to "blue heart", "💚" to "green heart",
            "💛" to "yellow heart", "💜" to "purple heart", "🖤" to "black heart", "💔" to "broken heart",
            "✨" to "sparkles", "🎉" to "party popper", "🔥" to "fire", "💯" to "hundred points",
        )
        add(
            EmojiCategory.Animals,
            "🐶" to "dog face", "🐱" to "cat face", "🐭" to "mouse face", "🐹" to "hamster", "🐰" to "rabbit face",
            "🦊" to "fox", "🐻" to "bear", "🐼" to "panda", "🐨" to "koala", "🐯" to "tiger face",
            "🦁" to "lion", "🐮" to "cow face", "🐷" to "pig face", "🐸" to "frog", "🐵" to "monkey face",
            "🐔" to "chicken", "🐧" to "penguin", "🐦" to "bird", "🦆" to "duck", "🦉" to "owl",
            "🐴" to "horse face", "🦄" to "unicorn", "🐝" to "honeybee", "🦋" to "butterfly", "🐢" to "turtle",
            "🐍" to "snake", "🐙" to "octopus", "🐳" to "whale", "🐬" to "dolphin", "🦈" to "shark",
        )
        add(
            EmojiCategory.Food,
            "🍎" to "red apple", "🍌" to "banana", "🍇" to "grapes", "🍓" to "strawberry", "🍍" to "pineapple",
            "🥑" to "avocado", "🍕" to "pizza", "🍔" to "hamburger", "🍟" to "french fries", "🌭" to "hot dog",
            "🍿" to "popcorn", "🍣" to "sushi", "🍜" to "steaming bowl", "🍩" to "doughnut", "🍪" to "cookie",
            "🎂" to "birthday cake", "🍰" to "shortcake", "☕" to "hot beverage", "🍺" to "beer mug", "🍷" to "wine glass",
        )
        add(
            EmojiCategory.Activities,
            "⚽" to "soccer ball", "🏀" to "basketball", "🏈" to "american football", "⚾" to "baseball",
            "🎾" to "tennis", "🎮" to "video game", "🎲" to "game die", "🎸" to "guitar", "🎨" to "artist palette",
            "🎬" to "clapper board", "🏆" to "trophy", "🥇" to "first place medal", "🎯" to "direct hit",
        )
        add(
            EmojiCategory.Travel,
            "🚗" to "automobile", "🚕" to "taxi", "🚲" to "bicycle", "✈️" to "airplane", "🚀" to "rocket",
            "🏠" to "house", "🏢" to "office building", "🗽" to "statue of liberty", "🗻" to "mount fuji",
            "🏖️" to "beach with umbrella", "🌋" to "volcano", "🌙" to "crescent moon", "⭐" to "star", "☀️" to "sun",
        )
        add(
            EmojiCategory.Objects,
            "💡" to "light bulb", "📱" to "mobile phone", "💻" to "laptop", "⌨️" to "keyboard", "🖥️" to "desktop computer",
            "📷" to "camera", "🔑" to "key", "🔒" to "locked", "📎" to "paperclip", "✏️" to "pencil",
            "📚" to "books", "🎁" to "wrapped gift", "💰" to "money bag", "⏰" to "alarm clock",
        )
        add(
            EmojiCategory.Symbols,
            "✅" to "check mark button", "❌" to "cross mark", "⚠️" to "warning", "❓" to "question mark",
            "❗" to "exclamation mark", "➕" to "plus", "➖" to "minus", "🔄" to "counterclockwise arrows",
            "🔔" to "bell", "🔕" to "bell with slash", "💬" to "speech balloon", "💤" to "zzz",
        )
        add(
            EmojiCategory.Flags,
            "🏳️" to "white flag", "🏁" to "chequered flag", "🚩" to "triangular flag",
        )
    }

    /** `:sm` matches names that start with, then contain, `sm`; case-insensitive, no diacritics folding. */
    fun search(query: String, limit: Int = 24): List<EmojiEntry.Unicode> {
        if (query.isEmpty()) return all.take(limit).map { it.second }
        val q = query.lowercase()
        val starts = all.filter { it.second.name.lowercase().startsWith(q) }
        val contains = all.filter { it.second.name.lowercase().contains(q) && it !in starts }
        return (starts + contains).take(limit).map { it.second }
    }

    fun inCategory(category: EmojiCategory): List<EmojiEntry.Unicode> = all.filter { it.first == category }.map { it.second }
}
