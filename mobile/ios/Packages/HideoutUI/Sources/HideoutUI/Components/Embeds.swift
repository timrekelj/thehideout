import HideoutCore
import HideoutDesign
import SafariServices
import SwiftUI

/// A link embed (ATT-06/07): a left rail in the provider's colour (or the theme's `borderStrong` for a
/// plain link), a header with a site icon, the site name and a fold chevron, and — folded away by the
/// chevron — either a plain link's title/description/thumbnail, or a rich provider's post (author, text,
/// media, "Open original").
public struct EmbedCard: View {
    @Environment(\.theme) private var theme
    @Environment(ServerSelection.self) private var server
    @Environment(\.allowsFakeAssets) private var allowsFakeAssets
    let embed: Embed
    let collapsed: Bool
    var onToggleCollapsed: () -> Void
    var onOpenLightbox: ([LightboxItem], Int) -> Void

    public init(embed: Embed, collapsed: Bool, onToggleCollapsed: @escaping () -> Void, onOpenLightbox: @escaping ([LightboxItem], Int) -> Void = { _, _ in }) {
        self.embed = embed; self.collapsed = collapsed; self.onToggleCollapsed = onToggleCollapsed; self.onOpenLightbox = onOpenLightbox
    }

    @State private var safariURL: URL?

    private var provider: EmbedProvider { EmbedProvider.from(key: embed.provider) }
    private var isPost: Bool { provider != .link || embed.kind == .rich }
    private var railColor: Color { provider.paletteKey.flatMap { theme.colors.channel($0)?.ink } ?? theme.colors.borderStrong }
    private var site: String { provider == .link ? (embed.siteName.isEmpty ? "Link" : embed.siteName) : provider.label }
    private var title: String { embed.title.isEmpty && provider == .link ? embed.url : embed.title }

    public var body: some View {
        HStack(alignment: .top, spacing: 0) {
            RoundedRectangle(cornerRadius: Radius.xs, style: .continuous).fill(railColor).frame(width: 3)
            VStack(alignment: .leading, spacing: 0) {
                Button(action: onToggleCollapsed) {
                    HStack(spacing: 6) {
                        providerGlyph
                        Text(site).typeStyle(\.label, color: \.textMuted).lineLimit(1)
                        if collapsed, !title.isEmpty {
                            Text(title).typeStyle(\.label, color: \.textFaint).lineLimit(1)
                        } else {
                            Spacer(minLength: 0)
                        }
                        Image(systemName: "chevron.right")
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundStyle(theme.colors.textFaint)
                            .rotationEffect(.degrees(collapsed ? 0 : 90))
                    }
                    .padding(.horizontal, 4)
                    .frame(minHeight: 28)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(site) preview")
                if !collapsed {
                    VStack(alignment: .leading, spacing: 8) {
                        if isPost {
                            PostBody(embed: embed, title: title, onOpenLightbox: onOpenLightbox) { openOriginal() }
                        } else {
                            LinkBody(embed: embed, title: title) { openOriginal() }
                        }
                    }
                    .padding(.horizontal, 4)
                    .padding(.bottom, 6)
                }
            }
            .padding(.horizontal, 4)
            .background(theme.colors.hover, in: RoundedRectangle(cornerRadius: Radius.lg, style: .continuous))
        }
        .padding(.top, 4)
        .frame(maxWidth: CGFloat(embedMaxWidth), alignment: .leading)
        .sheet(item: Binding(get: { safariURL.map(SafariItem.init) }, set: { safariURL = $0?.url })) { item in SafariView(url: item.url) }
    }

    private func openOriginal() {
        guard let url = URL(string: embed.url) else { return }
        safariURL = url
    }

    @ViewBuilder private var providerGlyph: some View {
        if provider == .link {
            Image(systemName: "globe").font(.system(size: 12)).foregroundStyle(theme.colors.textMuted)
        } else {
            // Desktop shows the provider's brand mark; Phosphor's brand set isn't bundled on iOS either
            // (see mobile/README.md "Icons"), so the rail's own colour stands in for it here, like Android.
            Circle().fill(railColor).frame(width: 12, height: 12)
        }
    }
}

private struct SafariItem: Identifiable { let url: URL; var id: String { url.absoluteString } }

/// A plain link card: the title (opens the URL), a 3-line description, and a thumbnail beside or below.
private struct LinkBody: View {
    @Environment(\.theme) private var theme
    @Environment(ServerSelection.self) private var server
    @Environment(\.allowsFakeAssets) private var allowsFakeAssets
    let embed: Embed
    let title: String
    let onOpenLink: () -> Void

    private var side: Bool { embedThumbnailSide(width: embed.thumbnailWidth, height: embed.thumbnailHeight) }

    private var texts: some View {
        VStack(alignment: .leading, spacing: 4) {
            if !title.isEmpty {
                Button(action: onOpenLink) {
                    Text(title).typeStyle(\.bodyStrong, color: \.accentText).multilineTextAlignment(.leading)
                }
                .buttonStyle(.plain)
            }
            if !embed.description.isEmpty {
                Text(embed.description).typeStyle(\.body, color: \.textMuted)
                    .lineLimit(embedDescriptionLines)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    var body: some View {
        if let key = embed.thumbnail {
            let source = try? mediaSource(base: server.url, key: key, allowFakeAssets: allowsFakeAssets)
            if side {
                HStack(alignment: .top, spacing: 12) {
                    texts.frame(maxWidth: .infinity, alignment: .leading)
                    AttachmentAsyncImage(source: source)
                        .frame(width: embedSideThumb, height: embedSideThumb)
                        .clipShape(RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
                }
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    texts.frame(maxWidth: .infinity, alignment: .leading)
                    let (w, h) = fitSize(natural: embed.thumbnailWidth.flatMap { ww in embed.thumbnailHeight.map { hh in (ww, hh) } }, maxW: embedBelowThumbMaxW, maxH: embedBelowThumbMaxH)
                    AttachmentAsyncImage(source: source)
                        .frame(width: w, height: h)
                        .clipShape(RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
                }
            }
        } else {
            texts.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// A rich post (X/TikTok/Instagram/Reddit): author, title/text, media, published time, "Open original".
private struct PostBody: View {
    @Environment(\.theme) private var theme
    @Environment(ServerSelection.self) private var server
    @Environment(\.allowsFakeAssets) private var allowsFakeAssets
    let embed: Embed
    let title: String
    var onOpenLightbox: ([LightboxItem], Int) -> Void
    let onOpenOriginal: () -> Void

    private var images: [Attachment] {
        embed.media.enumerated().filter { $0.element.kind == .image }.map { i, m in
            Attachment(id: ID(i), filename: "picture", contentType: "image/jpeg", size: m.size, file: m.file, width: m.width, height: m.height)
        }
    }

    private var videos: [Attachment] {
        embed.media.enumerated().filter { $0.element.kind != .image }.map { i, m in
            Attachment(id: ID(i), filename: "video", contentType: "video/mp4", size: m.size, file: m.file, width: m.width, height: m.height, thumbnail: m.poster)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !embed.authorName.isEmpty {
                HStack(spacing: 8) {
                    if let icon = embed.authorIcon, let source = try? mediaSource(base: server.url, key: icon, allowFakeAssets: allowsFakeAssets) {
                        AttachmentAsyncImage(source: source).frame(width: 20, height: 20).clipShape(Circle())
                    }
                    Text(embed.authorName).typeStyle(\.bodyStrong)
                }
            }
            if !title.isEmpty { Text(title).typeStyle(\.bodyStrong) }
            if !embed.description.isEmpty { MessageBody(embed.description) }
            if !images.isEmpty {
                AttachmentImagesView(images: images) { i in
                    onOpenLightbox(images.map { LightboxItem(attachment: $0) }, i)
                }
            }
            ForEach(videos) { video in
                AttachmentVideoCard(attachment: video) { pos in onOpenLightbox([LightboxItem(attachment: video, startSeconds: pos)], 0) }
            }
            HStack(spacing: 12) {
                if let at = embed.publishedAt {
                    Text(at, format: .dateTime.year().month().day().hour().minute()).typeStyle(\.caption, color: \.textMuted)
                }
                Button(action: onOpenOriginal) {
                    HStack(spacing: 4) {
                        Image(systemName: "arrow.up.right.square").font(.system(size: 12))
                        Text("Open original").typeStyle(\.label, color: \.accentText)
                    }
                }
                .buttonStyle(.plain)
            }
        }
    }
}
