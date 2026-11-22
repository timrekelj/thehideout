import HideoutCore
import HideoutDesign
import CoreText
import SwiftUI

/// A user's avatar: a rounded square in a deterministic fill (FNV-1a of the id, like desktop) with their initials.
/// Crisp corners follow `Radius.avatar(size:)`. Uploaded avatars arrive with the media cache (phase 3).
public struct Avatar: View {
    @Environment(\.theme) private var theme
    let name: String
    let id: String
    let size: CGFloat

    public init(name: String, id: String? = nil, size: CGFloat = 32) {
        self.name = name
        self.id = id ?? name
        self.size = size
    }

    public init(user: User, size: CGFloat = 32) {
        self.init(name: user.username, id: String(user.id), size: size)
    }

    public var body: some View {
        let fill = AvatarStyle.fill(for: id, accent: theme.spec.accent)
        Text(AvatarStyle.initials(name))
            .font(DesignFont.ui(size: size * 0.42, weight: 650))
            .foregroundStyle(AvatarStyle.ink)
            .frame(width: size, height: size)
            .background(fill, in: RoundedRectangle(cornerRadius: Radius.avatar(size: size), style: .continuous))
            .accessibilityHidden(true)
    }
}

/// A server's tile: its initials on an avatar fill (icons arrive with the media cache).
public struct ServerBadge: View {
    @Environment(\.theme) private var theme
    let name: String
    let id: String
    let size: CGFloat

    public init(name: String, id: String? = nil, size: CGFloat = 44) {
        self.name = name
        self.id = id ?? name
        self.size = size
    }

    public init(guild: Guild, size: CGFloat = 44) {
        self.init(name: guild.name, id: String(guild.id), size: size)
    }

    public var body: some View {
        // "The Hideout" -> TH, "Garage Band" -> GB: first letters of the first two words.
        Text(AvatarStyle.initials(name))
            .font(DesignFont.ui(size: size * 0.4, weight: 650))
            .foregroundStyle(AvatarStyle.ink)
            .frame(width: size, height: size)
            .background(AvatarStyle.fill(for: id, accent: theme.spec.accent), in: RoundedRectangle(cornerRadius: Radius.xl + size / 14, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: Radius.xl + size / 14, style: .continuous).strokeBorder(.white.opacity(0.08), lineWidth: 1))
            .accessibilityHidden(true)
    }
}

/// Font helper for custom-drawn text that is not a `TypeStyle` (avatar initials, badge numerals): the bundled faces
/// at an exact weight, the same way `TypeStyle.font` builds them.
public enum DesignFont {
    private static let wght = 0x7767_6874  // the 'wght' variation axis tag

    public static func ui(size: CGFloat, weight: CGFloat = 450) -> Font {
        let attrs: [CFString: Any] = [kCTFontFamilyNameAttribute: DesignFonts.uiFamily, kCTFontVariationAttribute: [wght: weight]]
        return Font(CTFontCreateWithFontDescriptor(CTFontDescriptorCreateWithAttributes(attrs as CFDictionary), size, nil))
    }

    public static func mono(size: CGFloat, bold: Bool = false) -> Font {
        var attrs: [CFString: Any] = [kCTFontFamilyNameAttribute: DesignFonts.monoFamily]
        if bold { attrs[kCTFontStyleNameAttribute] = "Bold" }
        return Font(CTFontCreateWithFontDescriptor(CTFontDescriptorCreateWithAttributes(attrs as CFDictionary), size, nil))
    }
}
