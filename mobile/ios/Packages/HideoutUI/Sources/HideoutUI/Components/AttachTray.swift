import HideoutCore
import HideoutDesign
import SwiftUI
import UIKit

/// The composer's tray: a chip per attached file with its thumbnail or type icon, name, size, upload
/// progress, remove, and "Try again" on a failed one (ATT-09, desktop's `attach::tray`). One wide
/// horizontal scroll so it never wraps the composer taller; a note (too many, too large) sits below it.
public struct AttachTray: View {
    @Environment(\.theme) private var theme
    let draft: AttachDraft
    var onRemove: (ID) -> Void
    var onRetry: (ID) -> Void

    public init(draft: AttachDraft, onRemove: @escaping (ID) -> Void, onRetry: @escaping (ID) -> Void) {
        self.draft = draft; self.onRemove = onRemove; self.onRetry = onRetry
    }

    public var body: some View {
        if !draft.files.isEmpty || draft.note != nil {
            VStack(alignment: .leading, spacing: 4) {
                if !draft.files.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(draft.files) { file in
                                AttachChip(file: file, onRemove: { onRemove(file.id) }, onRetry: { onRetry(file.id) })
                            }
                        }
                        .padding(.horizontal, Space.s4)
                    }
                    .accessibilityIdentifier("attach-tray")
                }
                if let note = draft.note {
                    HStack(spacing: 6) {
                        Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 12)).foregroundStyle(theme.colors.danger)
                        Text(note).typeStyle(\.caption, color: \.danger)
                    }
                    .padding(.horizontal, Space.s4)
                }
            }
        }
    }
}

private func trayIconSymbol(_ kind: FileKind) -> String {
    switch kind {
    case .image: return "photo"
    case .video: return "video"
    case .audio: return "waveform"
    case .pdf: return "doc.richtext"
    case .archive: return "doc.zipper"
    case .text, .doc: return "doc.text"
    case .code: return "chevron.left.forwardslash.chevron.right"
    case .other: return "doc"
    }
}

private struct AttachChip: View {
    @Environment(\.theme) private var theme
    let file: DraftFile
    var onRemove: () -> Void
    var onRetry: () -> Void

    private var kind: FileKind { fileKind(contentType: file.contentType, filename: file.name) }
    private var failed: Bool { file.state.phase == .failed }

    var body: some View {
        HStack(spacing: 8) {
            thumbnail
            VStack(alignment: .leading, spacing: 1) {
                Text(file.name).typeStyle(\.label).lineLimit(1)
                Text(subtitle).typeStyle(\.caption, color: failed ? \.danger : \.textMuted).lineLimit(1)
                if file.state.phase == .uploading || file.state.phase == .queued {
                    ProgressBar(fraction: file.state.total > 0 ? Double(file.state.sent) / Double(file.state.total) : 0)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if failed {
                Button(action: onRetry) { Image(systemName: "arrow.clockwise").foregroundStyle(theme.colors.textMuted) }
                    .accessibilityLabel("Try again")
            }
            Button(action: onRemove) { Image(systemName: "xmark").foregroundStyle(theme.colors.textMuted) }
                .accessibilityLabel("Remove \(file.name)")
        }
        .padding(6)
        .frame(width: 220)
        .background(failed ? theme.colors.dangerBg : theme.colors.surfaceRaised, in: RoundedRectangle(cornerRadius: Radius.lg, style: .continuous))
        .accessibilityIdentifier("attach-chip-\(file.id)")
    }

    private var subtitle: String {
        switch file.state.phase {
        case .uploading: return "\(humanSize(file.state.sent)) / \(humanSize(file.state.total))"
        case .queued: return "Waiting…"
        case .ready: return humanSize(file.size)
        case .failed: return file.state.reason ?? "Failed"
        }
    }

    @ViewBuilder private var thumbnail: some View {
        ZStack {
            // A local file, loaded synchronously (it's on disk already, not fetched over the network):
            // no placeholder gap to show, and deterministic for a snapshot taken right away.
            if kind == .image, let preview = file.preview, let url = URL(string: preview), let uiImage = UIImage(contentsOfFile: url.path) {
                Image(uiImage: uiImage).resizable().aspectRatio(contentMode: .fill)
            } else {
                Rectangle().fill(theme.colors.accentSubtle)
                Image(systemName: trayIconSymbol(kind)).foregroundStyle(theme.colors.accentText)
            }
        }
        .frame(width: 32, height: 32)
        .clipShape(RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
    }
}

private struct ProgressBar: View {
    let fraction: Double
    @Environment(\.theme) private var theme
    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                Capsule().fill(theme.colors.pressed)
                Capsule().fill(theme.colors.accent).frame(width: geo.size.width * fraction.clamped(to: 0...1))
            }
        }
        .frame(height: 3)
    }
}

private extension Double {
    func clamped(to range: ClosedRange<Double>) -> Double { min(max(self, range.lowerBound), range.upperBound) }
}
