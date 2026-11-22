import HideoutCore
import HideoutDesign
import SwiftUI

/// One message in a transcript: avatar, author (role-coloured), time, body, reactions. A follow-up from the same
/// author collapses into the gutter. Pending messages sit at 55 % opacity and a failed one says so with Retry.
public struct MessageRow: View {
    @Environment(\.theme) private var theme
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone
    let author: String
    let authorID: String
    let roleColor: String?
    let time: Date
    let text: String
    let reactions: [Reaction]
    let me: String?
    let grouped: Bool
    let delivery: Delivery?
    let edited: Bool
    var onRetry: (() -> Void)?

    public init(author: String, authorID: String? = nil, roleColor: String? = nil, time: Date, text: String, reactions: [Reaction] = [], me: String? = nil,
                grouped: Bool = false, delivery: Delivery? = nil, edited: Bool = false, onRetry: (() -> Void)? = nil) {
        self.author = author; self.authorID = authorID ?? author; self.roleColor = roleColor; self.time = time; self.text = text
        self.reactions = reactions; self.me = me; self.grouped = grouped; self.delivery = delivery; self.edited = edited; self.onRetry = onRetry
    }

    private var clock: Date.FormatStyle { Date.FormatStyle(date: .omitted, time: .shortened, locale: locale, timeZone: timeZone) }

    public var body: some View {
        let metrics = theme.metrics
        let pad = grouped ? metrics.msgPadFollow : metrics.msgPadHead
        HStack(alignment: .top, spacing: Space.s3) {
            if grouped {
                Text(time, format: clock)
                    .typeStyle(\.caption, color: \.textFaint)
                    .monospacedDigit()
                    .frame(width: metrics.msgAvatar + 8, alignment: .center)
                    .opacity(0)  // revealed on long-press via the accessibility value; the gutter keeps the alignment
            } else {
                Avatar(name: author, id: authorID, size: metrics.msgAvatar + 8)
            }
            VStack(alignment: .leading, spacing: 3) {
                if !grouped {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(author)
                            .typeStyle(\.bodyStrong, tint: roleColor.flatMap { theme.colors.channel($0)?.text } ?? theme.colors.accentText)
                        Text(time, format: clock)
                            .typeStyle(\.caption, color: \.textFaint)
                            .monospacedDigit()
                        if edited { Text("edited").typeStyle(\.caption, color: \.textFaint) }
                    }
                }
                MessageBody(text, me: me)
                if !reactions.isEmpty {
                    HStack(spacing: 6) {
                        ForEach(reactions, id: \.emoji) { r in
                            if case .unicode(let e) = r.emoji { ReactionChip(e, count: r.count, mine: r.me) }
                        }
                    }
                    .padding(.top, 4)
                }
                if delivery == .failed {
                    HStack(spacing: 6) {
                        Image(systemName: "exclamationmark.circle.fill").foregroundStyle(theme.colors.danger)
                        Text("Failed to send").typeStyle(\.caption, color: \.danger)
                        Button("Retry") { onRetry?() }
                            .font(DesignFont.ui(size: 13, weight: 650))
                            .foregroundStyle(theme.colors.accentText)
                    }
                    .padding(.top, 2)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, Space.s4)
        .padding(.top, pad.top + (grouped ? 0 : 8))
        .padding(.bottom, pad.bottom + 2)
        .opacity(delivery == .pending ? 0.55 : 1)
        .accessibilityElement(children: .combine)
        .accessibilityValue(Text(time, format: clock))
    }
}
