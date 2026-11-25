import HideoutCore
import HideoutDesign
import SwiftUI

/// MSG-07: search, this server's custom emoji, then the bundled Unicode categories, one scrolling grid with
/// section headers so every category is reachable (not a paged carousel). `serverEmojis` is empty in a DM
/// (DM-06: Unicode-only scope).
public struct EmojiPickerSheet: View {
    @Environment(\.theme) private var theme
    @Environment(\.dismiss) private var dismiss
    let serverEmojis: [Emoji]
    let recents: [EmojiEntry]
    let onPick: (EmojiEntry) -> Void
    @State private var query = ""

    public init(serverEmojis: [Emoji] = [], recents: [EmojiEntry] = [], onPick: @escaping (EmojiEntry) -> Void) {
        self.serverEmojis = serverEmojis
        self.recents = recents
        self.onPick = onPick
    }

    private let columns = Array(repeating: GridItem(.flexible(), spacing: Space.s2), count: 8)

    public var body: some View {
        NavigationStack {
            ScrollView {
                LazyVGrid(columns: columns, spacing: Space.s2) {
                    if !query.isEmpty {
                        section("Results", Autocomplete.emojiMatches(query, serverEmojis: serverEmojis, limit: 64))
                    } else {
                        if !recents.isEmpty { section("Recently used", recents) }
                        if !serverEmojis.isEmpty {
                            section("This server", serverEmojis.map { EmojiEntry.custom(id: $0.id, name: $0.name, image: $0.image, animated: $0.animated) })
                        }
                        ForEach(EmojiCategory.allCases, id: \.self) { category in
                            section(category.label, EmojiData.inCategory(category))
                        }
                    }
                }
                .padding(Space.s4)
            }
            .background(theme.colors.bg)
            .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: "Search emoji")
            .navigationTitle("Emoji")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }
            }
        }
        .presentationDetents([.medium, .large])
    }

    @ViewBuilder
    private func section(_ title: String, _ entries: [EmojiEntry]) -> some View {
        if !entries.isEmpty {
            Section {
                ForEach(entries) { entry in
                    Button {
                        onPick(entry)
                        dismiss()
                    } label: {
                        cell(entry)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(entry.name.replacingOccurrences(of: "_", with: " "))
                }
            } header: {
                Text(title.uppercased())
                    .typeStyle(\.label, color: \.textMuted)
                    .tracking(0.8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .gridCellColumns(columns.count)
                    .padding(.top, Space.s2)
            }
        }
    }

    @ViewBuilder
    private func cell(_ entry: EmojiEntry) -> some View {
        switch entry {
        case .unicode(let emoji, _):
            Text(emoji).font(.system(size: 28)).frame(width: 40, height: 40)
        case .custom(_, let name, _, _):
            // Image rendering (animated-first-frame) is a later task; show the name so the cell is still usable.
            Text(String(name.prefix(2)).uppercased())
                .typeStyle(\.caption, color: \.accentText)
                .frame(width: 40, height: 40)
                .background(theme.colors.accentWash, in: RoundedRectangle(cornerRadius: Radius.sm, style: .continuous))
        }
    }
}
