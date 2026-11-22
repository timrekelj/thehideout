import HideoutCore
import HideoutDesign
import SwiftUI

/// A channel in the list: glyph, name (bold when unread), mention pill or unread dot; selected rows are washed.
public struct ChannelRow: View {
    @Environment(\.theme) private var theme
    let channel: Channel
    let unread: Unread
    let selected: Bool

    public init(_ channel: Channel, unread: Unread = Unread(), selected: Bool = false) {
        self.channel = channel
        self.unread = unread
        self.selected = selected
    }

    public var body: some View {
        HStack(spacing: Space.s3) {
            ChannelGlyph(channel, size: 17)
            Text(channel.name)
                .typeStyle(unread.count > 0 ? \.bodyStrong : \.body, color: unread.count > 0 ? \.text : \.textMuted)
                .lineLimit(1)
            Spacer(minLength: Space.s2)
            if unread.mentions > 0 { UnreadPill(unread.mentions) }
            else if unread.count > 0 { UnreadDot() }
        }
        .padding(.horizontal, Space.s3)
        .frame(minHeight: 44)
        .background(selected ? theme.colors.accentWash : .clear, in: RoundedRectangle(cornerRadius: Radius.lg, style: .continuous))
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityText)
    }

    private var accessibilityText: String {
        var t = (channel.kind == .voice ? "Voice channel " : "Channel ") + channel.name
        if unread.mentions > 0 { t += ", \(unread.mentions) mentions" } else if unread.count > 0 { t += ", unread" }
        return t
    }
}

/// A person in a voice channel under its row: avatar, name, muted and deafened marks.
public struct VoiceParticipantRow: View {
    @Environment(\.theme) private var theme
    let user: User
    let state: VoiceState

    public init(user: User, state: VoiceState) {
        self.user = user
        self.state = state
    }

    public var body: some View {
        HStack(spacing: Space.s3) {
            Avatar(user: user, size: 24)
            Text(user.username).typeStyle(\.body, color: \.textMuted).lineLimit(1)
            Spacer()
            if state.selfDeaf || state.serverMute || state.selfMute {
                Image(systemName: state.selfDeaf ? "speaker.slash.fill" : "mic.slash.fill")
                    .font(.system(size: 13))
                    .foregroundStyle(state.serverMute ? theme.colors.danger : theme.colors.textFaint)
            }
            if state.selfVideo { Image(systemName: "video.fill").font(.system(size: 13)).foregroundStyle(theme.colors.textFaint) }
        }
        .padding(.leading, Space.s6 + 2)
        .padding(.trailing, Space.s3)
        .frame(minHeight: 36)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(user.username)\(state.selfMute ? ", muted" : "")\(state.selfDeaf ? ", deafened" : "")")
    }
}
