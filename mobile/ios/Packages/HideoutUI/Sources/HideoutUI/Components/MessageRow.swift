import HideoutCore
import HideoutDesign
import SwiftUI
import UIKit

/// Quick reactions offered at the top of a message's context menu (desktop has no fixed set; these match
/// the common choices in the emoji picker's "frequently used" feel).
public let quickReactionEmoji = ["👍", "❤️", "😂", "😮", "😢", "🎉"]

/// One message in a transcript: avatar, author (role-coloured), time, body, reactions. A follow-up from the
/// same author collapses into the gutter. Pending messages sit at 55 % opacity and a failed one says so with
/// Retry. Long-press opens a context menu (quick react, Reply, Copy, Edit (own), Delete); dragging right
/// replies (swipe-to-reply).
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
    let attachments: [Attachment]
    let me: String?
    let grouped: Bool
    let delivery: Delivery?
    let edited: Bool
    let mentionsMe: Bool
    let isOwn: Bool
    let canModerate: Bool
    var onRetry: (() -> Void)?
    var onReact: ((ReactionEmoji, Bool) -> Void)?
    var onReply: (() -> Void)?
    var onEdit: (() -> Void)?
    var onDelete: (() -> Void)?
    var onMoreReactions: (() -> Void)?
    var onOpenLightbox: (([LightboxItem], Int) -> Void)?

    @State private var dragX: CGFloat = 0
    @State private var confirmDelete = false

    public init(author: String, authorID: String? = nil, roleColor: String? = nil, time: Date, text: String, reactions: [Reaction] = [], attachments: [Attachment] = [], me: String? = nil,
                grouped: Bool = false, delivery: Delivery? = nil, edited: Bool = false, mentionsMe: Bool = false, isOwn: Bool = false, canModerate: Bool = false,
                onRetry: (() -> Void)? = nil, onReact: ((ReactionEmoji, Bool) -> Void)? = nil, onReply: (() -> Void)? = nil,
                onEdit: (() -> Void)? = nil, onDelete: (() -> Void)? = nil, onMoreReactions: (() -> Void)? = nil,
                onOpenLightbox: (([LightboxItem], Int) -> Void)? = nil) {
        self.author = author; self.authorID = authorID ?? author; self.roleColor = roleColor; self.time = time; self.text = text
        self.reactions = reactions; self.attachments = attachments; self.me = me; self.grouped = grouped; self.delivery = delivery; self.edited = edited
        self.mentionsMe = mentionsMe; self.isOwn = isOwn; self.canModerate = canModerate
        self.onRetry = onRetry; self.onReact = onReact; self.onReply = onReply; self.onEdit = onEdit; self.onDelete = onDelete; self.onMoreReactions = onMoreReactions
        self.onOpenLightbox = onOpenLightbox
    }

    private var clock: Date.FormatStyle { Date.FormatStyle(date: .omitted, time: .shortened, locale: locale, timeZone: timeZone) }

    public var body: some View {
        let metrics = theme.metrics
        let pad = grouped ? metrics.msgPadFollow : metrics.msgPadHead
        ZStack(alignment: .leading) {
            if dragX > 4 {
                Image(systemName: "arrowshape.turn.up.left.fill")
                    .foregroundStyle(theme.colors.accentText)
                    .opacity(Double(min(1, dragX / 56)))
                    .padding(.leading, Space.s4)
            }
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
                    if !text.isEmpty { MessageBody(text, me: me) }
                    if !attachments.isEmpty {
                        MessageAttachmentsView(attachments: attachments, onOpenLightbox: { items, i in onOpenLightbox?(items, i) })
                    }
                    if !reactions.isEmpty {
                        FlowReactions(reactions: reactions, onToggle: onReact, onMore: onMoreReactions)
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
            .offset(x: dragX)
        }
        .padding(.horizontal, Space.s4)
        .padding(.top, pad.top + (grouped ? 0 : 8))
        .padding(.bottom, pad.bottom + 2)
        .opacity(delivery == .pending ? 0.55 : 1)
        .background(mentionsMe ? theme.colors.accentWash : .clear)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityValue(Text(time, format: clock))
        // `.simultaneousGesture`, not `.gesture`: claiming the touch exclusively would compete with the
        // system's own long-press recognizer for `.contextMenu` below and could cancel or destabilize it.
        .simultaneousGesture(
            DragGesture(minimumDistance: 16)
                .onChanged { value in if value.translation.width > 0 { dragX = min(72, value.translation.width) } }
                .onEnded { value in
                    let replied = value.translation.width > 56
                    withAnimation(Motion.control) { dragX = 0 }
                    if replied { onReply?() }
                }
        )
        .contextMenu {
            if onReact != nil {
                ControlGroup {
                    ForEach(quickReactionEmoji, id: \.self) { e in
                        Button { onReact?(.unicode(e), true) } label: { Text(e) }
                    }
                }
                .controlGroupStyle(.compactMenu)
            }
            if let onReply { Button("Reply", systemImage: "arrowshape.turn.up.left", action: onReply) }
            Button("Copy", systemImage: "doc.on.doc") { UIPasteboard.general.string = text }
            if isOwn, let onEdit { Button("Edit", systemImage: "pencil", action: onEdit) }
            if (isOwn || canModerate), onDelete != nil { Button("Delete", systemImage: "trash", role: .destructive) { confirmDelete = true } }
        } preview: {
            MessageBody(text, me: me).padding(Space.s4).frame(maxWidth: 320, alignment: .leading)
        }
        .confirmationDialog(
            isOwn ? "Delete this message?" : "Delete \(author)'s message?",
            isPresented: $confirmDelete, titleVisibility: .visible
        ) {
            Button("Delete", role: .destructive) { onDelete?() }
        } message: {
            Text("This message will be deleted for everyone.")
        }
    }
}

/// A wrapping row of reaction chips: tap toggles, "+" opens the full emoji picker.
struct FlowReactions: View {
    @Environment(\.theme) private var theme
    let reactions: [Reaction]
    var onToggle: ((ReactionEmoji, Bool) -> Void)?
    var onMore: (() -> Void)?

    var body: some View {
        FlowWrap(spacing: 6) {
            ForEach(reactions, id: \.emoji) { r in
                Button {
                    onToggle?(r.emoji, !r.me)
                } label: {
                    switch r.emoji {
                    case .unicode(let e): ReactionChip(e, count: r.count, mine: r.me)
                    case .custom: ReactionChip("🔸", count: r.count, mine: r.me) // image rendering is a later task
                    }
                }
                .buttonStyle(.plain)
            }
            if let onMore {
                Button(action: onMore) {
                    Image(systemName: "plus")
                        .font(.system(size: 12, weight: .bold))
                        .foregroundStyle(theme.colors.textMuted)
                        .frame(width: 26, height: 26)
                        .background(theme.colors.surfaceRaised, in: Circle())
                        .overlay(Circle().strokeBorder(theme.colors.borderSubtle, lineWidth: 1))
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Add a reaction")
            }
        }
    }
}

/// A left-to-right, top-to-bottom wrap of same-height children (reaction chips, filter chips).
public struct FlowWrap: SwiftUI.Layout {
    public var spacing: CGFloat
    public init(spacing: CGFloat = 6) { self.spacing = spacing }

    public func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0, y: CGFloat = 0, rowHeight: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > 0, x + size.width > width { x = 0; y += rowHeight + spacing; rowHeight = 0 }
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
        return CGSize(width: width == .infinity ? x : width, height: y + rowHeight)
    }

    public func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x: CGFloat = bounds.minX, y: CGFloat = bounds.minY, rowHeight: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > bounds.minX, x + size.width > bounds.maxX { x = bounds.minX; y += rowHeight + spacing; rowHeight = 0 }
            subview.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
    }
}
