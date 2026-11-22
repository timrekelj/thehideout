import HideoutCore
import HideoutDesign
import SwiftUI

/// The brand mark: the hideout tile on its accent, then the name in the display face.
struct Wordmark: View {
    @Environment(\.theme) private var theme
    let size: CGFloat

    var body: some View {
        VStack(spacing: Space.s3) {
            Image(systemName: "house.fill")
                .font(.system(size: size * 0.46, weight: .semibold))
                .foregroundStyle(theme.colors.accentFg)
                .frame(width: size, height: size)
                .background(theme.colors.accent, in: RoundedRectangle(cornerRadius: size * 0.26, style: .continuous))
            Text("thehideout").typeStyle(\.heading)
        }
        .accessibilityElement(children: .combine)
    }
}
