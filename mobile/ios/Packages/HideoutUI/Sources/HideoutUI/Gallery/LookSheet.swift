import HideoutCore
import HideoutDesign
import SwiftUI

/// The look switcher: accent, background, appearance, contrast, density and text size with native controls and a
/// live preview. Every change re-themes the whole app at once.
public struct LookSheet: View {
    @Environment(ThemeStore.self) private var store
    @Environment(\.theme) private var theme
    @Environment(\.motionReduced) private var reduced
    @Environment(\.dismiss) private var dismiss

    public init() {}

    public var body: some View {
        @Bindable var store = store
        NavigationStack {
            List {
                Section {
                    LookPreview()
                        .listRowInsets(EdgeInsets())
                        .listRowBackground(theme.colors.bg)
                } header: { SectionLabel("Preview") }

                Section {
                    AccentGrid(selection: spec(\.accent))
                } header: { SectionLabel("Accent") }
                .listRowBackground(theme.colors.surface)

                Section {
                    Picker("Background", selection: spec(\.background)) {
                        ForEach(BackgroundPreset.allCases) { Text($0.displayName).tag($0) }
                    }
                    Picker("Appearance", selection: spec(\.appearance)) {
                        ForEach(AppearanceChoice.allCases) { Text($0.displayName).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    Toggle("Tinted backgrounds", isOn: spec(\.tint))
                    Toggle("High contrast", isOn: Binding(
                        get: { store.spec.contrast == .high },
                        set: { on in store.update(reducedMotion: reduced) { $0.spec.contrast = on ? .high : .standard } }
                    ))
                } header: { SectionLabel("Surfaces") }
                .listRowBackground(theme.colors.surface)

                Section {
                    Picker("Density", selection: spec(\.density)) {
                        ForEach(Density.allCases) { Text($0.displayName).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    Picker("Text size", selection: spec(\.textSize)) {
                        ForEach(TextSize.allCases) { Text($0.displayName).tag($0) }
                    }
                    .pickerStyle(.segmented)
                } header: { SectionLabel("Size") } footer: {
                    Text("Text also follows Dynamic Type from iOS Settings, on top of this.").typeStyle(\.caption, color: \.textMuted)
                }
                .listRowBackground(theme.colors.surface)

                Section {
                    Picker("Reduce motion", selection: Binding(
                        get: { store.look.reduceMotion },
                        set: { v in store.update(reducedMotion: true) { $0.reduceMotion = v } }
                    )) {
                        ForEach(ReduceMotionChoice.allCases) { Text($0.displayName).tag($0) }
                    }
                } header: { SectionLabel("Motion") }
                .listRowBackground(theme.colors.surface)

                Section {
                    Button("Reset to default", role: .destructive) { store.update(reducedMotion: reduced) { $0 = Look() } }
                }
                .listRowBackground(theme.colors.surface)
            }
            .scrollContentBackground(.hidden)
            .background(theme.colors.bg)
            .navigationTitle("Appearance")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    private func spec<T>(_ keyPath: WritableKeyPath<ThemeSpec, T>) -> Binding<T> {
        Binding(
            get: { store.spec[keyPath: keyPath] },
            set: { v in store.update(reducedMotion: reduced) { $0.spec[keyPath: keyPath] = v } }
        )
    }
}

/// Eight accent swatches; the selected one carries a ring.
struct AccentGrid: View {
    @Environment(\.theme) private var theme
    @Binding var selection: AccentPreset

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 64), spacing: Space.s3)], spacing: Space.s3) {
            ForEach(AccentPreset.allCases) { preset in
                let color = Theme(spec: ThemeSpec.with(accent: preset, base: theme.spec), systemDark: theme.isDark).colors.accent
                Button { selection = preset } label: {
                    VStack(spacing: 6) {
                        Circle().fill(color).frame(width: 36, height: 36)
                            .overlay { if selection == preset { Image(systemName: "checkmark").font(.system(size: 14, weight: .bold)).foregroundStyle(theme.colors.accentFg.opacity(0.0)).foregroundStyle(.white) } }
                            .padding(3)
                            .overlay(Circle().strokeBorder(selection == preset ? theme.colors.text : .clear, lineWidth: 2))
                        Text(preset.displayName).typeStyle(\.caption, color: selection == preset ? \.text : \.textMuted).lineLimit(1)
                    }
                    .frame(maxWidth: .infinity)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(preset.displayName)
                .accessibilityAddTraits(selection == preset ? .isSelected : [])
            }
        }
        .padding(.vertical, Space.s2)
    }
}

extension ThemeSpec {
    static func with(accent: AccentPreset, base: ThemeSpec) -> ThemeSpec {
        var s = base
        s.accent = accent
        return s
    }
}

/// A small slice of the app in the current look, so a change is seen where it matters.
struct LookPreview: View {
    @Environment(\.theme) private var theme
    @Environment(\.currentDate) private var now

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s3) {
            MessageRow(author: "ana", time: now, text: "Lunch? The new ramen place opens at noon. @tim you in?", reactions: [Reaction(emoji: .unicode("👍"), count: 3, me: true)], me: "tim")
            HStack(spacing: Space.s2) {
                Button { } label: { Text("Join").onAccent() }.buttonStyle(.glassProminent)
                Button("Later") {}.buttonStyle(.glass)
                Spacer()
                UnreadPill(3)
            }
            .padding(.horizontal, Space.s4)
            .padding(.bottom, Space.s3)
        }
        .padding(.top, Space.s2)
        .background(theme.colors.surface, in: RoundedRectangle(cornerRadius: Radius.xl, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: Radius.xl, style: .continuous).strokeBorder(theme.colors.borderSubtle, lineWidth: 1))
        .padding(.horizontal, Space.s4)
        .padding(.vertical, Space.s2)
    }
}
