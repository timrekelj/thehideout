import HideoutCore
import HideoutDesign
import SwiftUI

/// What the gallery shows, grouped so snapshot tests can capture each at a readable scale.
public enum GallerySection: String, CaseIterable, Identifiable, Sendable {
    case colors, type, buttons, fields, badges, avatars, messages, channels, glass, motion
    public var id: String { rawValue }
    var title: String {
        switch self {
        case .colors: "Colour"
        case .type: "Type"
        case .buttons: "Buttons"
        case .fields: "Fields and controls"
        case .badges: "Chips and badges"
        case .avatars: "Avatars and servers"
        case .messages: "Messages"
        case .channels: "Channels"
        case .glass: "Glass"
        case .motion: "Motion"
        }
    }
}

/// The mobile twin of `client/gallery`: every token and every component built so far, in the current look, with
/// the look switcher in the toolbar. Debug builds reach it from You > Design gallery or `--gallery`.
public struct GalleryView: View {
    @Environment(\.theme) private var theme
    @State private var showLook = false

    public init() {}

    public var body: some View {
        page
    }

    /// A top-level `ScrollView` (not inside a `NavigationStack`) proposes an unbounded width to its content,
    /// so the column must be capped explicitly to the `GeometryReader`'s size, not left to size itself.
    private var page: some View {
        GeometryReader { geo in
            ScrollView {
                VStack(spacing: 0) {
                    HStack(spacing: Space.s3) {
                        Text("Gallery").typeStyle(\.heading)
                        Spacer()
                        Button("Look", systemImage: "paintpalette") { showLook = true }
                            .buttonStyle(.glass)
                            .accessibilityLabel("Change the look")
                    }
                    .padding(.horizontal, Space.s4)
                    .padding(.vertical, Space.s3)
                    GalleryContent(sections: GallerySection.allCases)
                }
                .frame(maxWidth: geo.size.width, minHeight: geo.size.height)
            }
            .scrollContentBackground(.hidden)
            .background(theme.colors.bg)
        }
        .sheet(isPresented: $showLook) { LookSheet() }
    }
}

/// The gallery's sections stacked; no scroll view, so a test can lay it out at full height.
public struct GalleryContent: View {
    let sections: [GallerySection]
    let showHero: Bool

    public init(sections: [GallerySection], showHero: Bool = true) {
        self.sections = sections
        self.showHero = showHero
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: Space.s8) {
            if showHero { LookHero() }
            ForEach(sections) { section in
                VStack(alignment: .leading, spacing: Space.s3) {
                    SectionLabel(section.title)
                    switch section {
                    case .colors: ColorSection()
                    case .type: TypeSection()
                    case .buttons: ButtonSection()
                    case .fields: FieldSection()
                    case .badges: BadgeSection()
                    case .avatars: AvatarSection()
                    case .messages: MessageSection()
                    case .channels: ChannelSection()
                    case .glass: GlassSection()
                    case .motion: MotionSection()
                    }
                }
            }
        }
        .padding(.horizontal, Space.s4)
        .padding(.top, Space.s2)
        .padding(.bottom, Space.s16)
    }
}

// MARK: - hero

struct LookHero: View {
    @Environment(\.theme) private var theme

    var body: some View {
        let spec = theme.spec
        HStack(alignment: .center, spacing: Space.s4) {
            ServerBadge(name: "The Hideout", size: 64)
            VStack(alignment: .leading, spacing: 6) {
                Text("The look").typeStyle(\.display)
                Text("\(spec.accent.displayName) · \(spec.background.displayName) · \(theme.isDark ? "Dark" : "Light")")
                    .typeStyle(\.control, color: \.textMuted)
                HStack(spacing: 6) {
                    StatusBadge(spec.contrast == .high ? "High contrast" : "Standard", tone: .accent)
                    StatusBadge(spec.density.displayName, tone: .info)
                    StatusBadge("Text \(spec.textSize.displayName)", tone: .warning)
                }
            }
        }
        .card(padding: Space.s4)
        .accessibilityElement(children: .combine)
    }
}

// MARK: - colour

struct Swatch: View {
    @Environment(\.theme) private var theme
    let name: String
    let color: Color

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            RoundedRectangle(cornerRadius: Radius.md, style: .continuous)
                .fill(color)
                .frame(height: 44)
                .overlay(RoundedRectangle(cornerRadius: Radius.md, style: .continuous).strokeBorder(theme.colors.border, lineWidth: 1))
            Text(name).typeStyle(\.caption).lineLimit(1).minimumScaleFactor(0.8)
            Text(color.hexString).font(DesignFont.mono(size: 10)).foregroundStyle(theme.colors.textFaint)
        }
        .accessibilityElement(children: .combine)
    }
}

struct ColorSection: View {
    @Environment(\.theme) private var theme

    var body: some View {
        let c = theme.colors
        VStack(alignment: .leading, spacing: Space.s6) {
            group("Surfaces", [("bg", c.bg), ("surface", c.surface), ("surfaceRaised", c.surfaceRaised), ("surfaceOverlay", c.surfaceOverlay), ("layer1", c.layer1), ("layer2", c.layer2), ("layer3", c.layer3), ("scrim", c.scrim)])
            group("Text", [("text", c.text), ("textMuted", c.textMuted), ("textFaint", c.textFaint)])
            group("Accent", [("accent", c.accent), ("accentFg", c.accentFg), ("accentText", c.accentText), ("accentSubtle", c.accentSubtle), ("accentWash", c.accentWash), ("accentHover", c.accentHover)])
            group("Borders", [("borderSubtle", c.borderSubtle), ("border", c.border), ("borderStrong", c.borderStrong), ("controlBorder", c.controlBorder), ("focus", c.focus)])
            group("Status", [("success", c.success), ("successBg", c.successBg), ("warning", c.warning), ("warningBg", c.warningBg), ("danger", c.danger), ("dangerBg", c.dangerBg), ("dangerSolid", c.dangerSolid), ("info", c.info), ("infoBg", c.infoBg)])
            VStack(alignment: .leading, spacing: Space.s2) {
                Text("Channel palette").typeStyle(\.bodyStrong)
                let keys = c.channels.keys.sorted()
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 72), spacing: Space.s3)], spacing: Space.s3) {
                    ForEach(keys, id: \.self) { key in
                        VStack(spacing: 6) {
                            Image(systemName: "number").font(.system(size: 16, weight: .bold))
                                .foregroundStyle(c.channels[key]!.ink)
                                .frame(maxWidth: .infinity, minHeight: 40)
                                .background(c.channels[key]!.wash, in: RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
                            Text(key).typeStyle(\.caption, color: \.textMuted)
                        }
                        .accessibilityElement(children: .combine)
                    }
                }
            }
        }
        .card()
    }

    private func group(_ title: String, _ items: [(String, Color)]) -> some View {
        VStack(alignment: .leading, spacing: Space.s2) {
            Text(title).typeStyle(\.bodyStrong)
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 92), spacing: Space.s3, alignment: .top)], alignment: .leading, spacing: Space.s3) {
                ForEach(items, id: \.0) { Swatch(name: $0.0, color: $0.1) }
            }
        }
    }
}

// MARK: - type

struct TypeSection: View {
    @Environment(\.theme) private var theme

    var body: some View {
        let t = theme.type
        let rows: [(String, KeyPath<TypeRamp, TypeStyle>, TypeStyle)] = [
            ("display", \.display, t.display), ("heading", \.heading, t.heading), ("title", \.title, t.title),
            ("body", \.body, t.body), ("bodyStrong", \.bodyStrong, t.bodyStrong), ("control", \.control, t.control),
            ("controlStrong", \.controlStrong, t.controlStrong), ("label", \.label, t.label), ("caption", \.caption, t.caption), ("code", \.code, t.code),
        ]
        VStack(alignment: .leading, spacing: Space.s4) {
            ForEach(rows, id: \.0) { name, path, style in
                VStack(alignment: .leading, spacing: 2) {
                    Text(name == "code" ? "let hideout = \"warm\"" : "Sphinx of black quartz").typeStyle(path)
                    Text("\(name) · \(String(format: "%g", (Double(style.size) * 10).rounded() / 10)) pt · wght \(Int(style.weight))\(style.mono ? " · mono" : "")")
                        .typeStyle(\.caption, color: \.textFaint)
                }
                .accessibilityElement(children: .combine)
            }
            Divider().overlay(theme.colors.borderSubtle)
            Text("Instrument Sans is a variable font: the weights 450, 530 and 650 are exact axis values, not the nearest named cut.")
                .typeStyle(\.caption, color: \.textMuted)
                .fixedSize(horizontal: false, vertical: true)
        }
        .card()
    }
}

// MARK: - buttons

struct ButtonSection: View {
    @Environment(\.theme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s5) {
            VStack(alignment: .leading, spacing: Space.s3) {
                Text("Liquid Glass").typeStyle(\.bodyStrong)
                HStack(spacing: Space.s3) {
                    Button { } label: { Text("Primary").onAccent() }.buttonStyle(.glassProminent)
                    Button("Secondary") {}.buttonStyle(.glass)
                    Button("Disabled") {}.buttonStyle(.glass).disabled(true)
                }
                HStack(spacing: Space.s3) {
                    Button(role: .destructive) { } label: { Text("Delete").foregroundStyle(theme.colors.onDanger) }.buttonStyle(.glassProminent).tint(theme.colors.dangerSolid)
                    Button { } label: { Label("Invite", systemImage: "person.badge.plus") }.buttonStyle(.glass)
                    Button {} label: { Image(systemName: "plus") }.buttonStyle(.glass).buttonBorderShape(.circle)
                        .accessibilityLabel("Add")
                }
            }
            VStack(alignment: .leading, spacing: Space.s3) {
                Text("Tinted and plain").typeStyle(\.bodyStrong)
                HStack(spacing: Space.s3) {
                    Button { } label: { Text("Primary").onAccent() }.buttonStyle(.borderedProminent)
                    Button("Secondary") {}.buttonStyle(.bordered)
                    Button("Ghost") {}.buttonStyle(.borderless)
                }
                HStack(spacing: Space.s3) {
                    Button("Danger", role: .destructive) {}.buttonStyle(.bordered).tint(theme.colors.danger)
                    Button(role: .destructive) { } label: { Text("Danger").foregroundStyle(theme.colors.onDanger) }.buttonStyle(.borderedProminent).tint(theme.colors.dangerSolid)
                }
            }
            VStack(alignment: .leading, spacing: Space.s3) {
                Text("Sizes").typeStyle(\.bodyStrong)
                HStack(spacing: Space.s3) {
                    Button("Mini") {}.buttonStyle(.glass).controlSize(.mini)
                    Button("Small") {}.buttonStyle(.glass).controlSize(.small)
                    Button("Regular") {}.buttonStyle(.glass)
                    Button { } label: { Text("Large").onAccent() }.buttonStyle(.glassProminent).controlSize(.large)
                }
            }
        }
        .card()
    }
}

// MARK: - fields

struct FieldSection: View {
    @Environment(\.theme) private var theme
    @State private var name = "ana"
    @State private var empty = ""
    @State private var password = "hunter2"
    @State private var notify = true
    @State private var quiet = false
    @State private var scope = 0

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s4) {
            ThemedField("Username or email", text: $name)
            ThemedField("Search messages", text: $empty)
            ThemedField("Password", text: $password, secure: true)
            VStack(spacing: 0) {
                Toggle("Notify me about mentions", isOn: $notify).padding(.vertical, 6)
                Divider().overlay(theme.colors.borderSubtle)
                Toggle("Quiet hours", isOn: $quiet).padding(.vertical, 6)
            }
            .typeStyle(\.control)
            Picker("Scope", selection: $scope) {
                Text("All").tag(0); Text("Mentions").tag(1); Text("Unread").tag(2)
            }
            .pickerStyle(.segmented)
        }
        .card()
    }
}

// MARK: - chips and badges

struct BadgeSection: View {
    @State private var filters: [Bool] = [true, false, false]

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s5) {
            VStack(alignment: .leading, spacing: Space.s3) {
                Text("Filters").typeStyle(\.bodyStrong)
                HStack(spacing: Space.s2) {
                    FilterChip("All", systemImage: "tray.full", isOn: $filters[0])
                    FilterChip("Mentions", systemImage: "at", isOn: $filters[1])
                    FilterChip("Unread", isOn: $filters[2])
                }
            }
            VStack(alignment: .leading, spacing: Space.s3) {
                Text("Reactions").typeStyle(\.bodyStrong)
                HStack(spacing: Space.s2) {
                    ReactionChip("👍", count: 3, mine: true)
                    ReactionChip("😂", count: 12)
                    ReactionChip("🎉", count: 1)
                    ReactionChip("👀", count: 4)
                }
            }
            VStack(alignment: .leading, spacing: Space.s3) {
                Text("Status").typeStyle(\.bodyStrong)
                HStack(spacing: Space.s2) {
                    StatusBadge("Owner", tone: .accent); StatusBadge("Online", tone: .success); StatusBadge("Muted", tone: .warning)
                    StatusBadge("Banned", tone: .danger); StatusBadge("Bot", tone: .info)
                }
            }
            VStack(alignment: .leading, spacing: Space.s3) {
                Text("Unread").typeStyle(\.bodyStrong)
                HStack(spacing: Space.s4) {
                    UnreadPill(1); UnreadPill(12); UnreadPill(142); UnreadDot()
                }
            }
        }
        .card()
    }
}

// MARK: - avatars

struct AvatarSection: View {
    @Environment(\.theme) private var theme
    private let names = ["tim", "ana", "bruno", "cvetka", "dora", "emil", "fran", "gus"]

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s5) {
            VStack(alignment: .leading, spacing: Space.s3) {
                Text("Fills are deterministic per id").typeStyle(\.bodyStrong)
                HStack(spacing: Space.s3) {
                    ForEach(names, id: \.self) { Avatar(name: $0, size: 36) }
                }
            }
            VStack(alignment: .leading, spacing: Space.s3) {
                Text("Sizes").typeStyle(\.bodyStrong)
                HStack(alignment: .bottom, spacing: Space.s4) {
                    Avatar(name: "ana", size: 24); Avatar(name: "ana", size: 32); Avatar(name: "ana", size: 40)
                    Avatar(name: "ana", size: 56); Avatar(name: "ana", size: 72)
                }
            }
            VStack(alignment: .leading, spacing: Space.s3) {
                Text("Servers").typeStyle(\.bodyStrong)
                HStack(spacing: Space.s4) {
                    ServerBadge(name: "The Hideout", id: "1"); ServerBadge(name: "Garage Band", id: "2"); ServerBadge(name: "Cooking Club", id: "3")
                    ServerBadge(name: "Chess", id: "4", size: 36)
                }
            }
        }
        .card()
    }
}

// MARK: - messages

struct MessageSection: View {
    @Environment(\.theme) private var theme
    @Environment(\.currentDate) private var now

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            MessageRow(author: "ana", roleColor: "pink", time: now.addingTimeInterval(-3600), text: "Release notes are up: **v0.0.3** adds a live gateway, ~~polling~~ and *much* better reconnects.\nDraft is in [the wiki](https://example.com/wiki), @tim take a look.", reactions: [Reaction(emoji: .unicode("😂"), count: 3, me: false), Reaction(emoji: .unicode("👍"), count: 2, me: true)], me: "tim")
            MessageRow(author: "ana", time: now.addingTimeInterval(-3500), text: "Plan for Friday:\n- cut the release\n- update the `README`\n- tell @cvetka", me: "tim", grouped: true)
            MessageRow(author: "bruno", roleColor: "teal", time: now.addingTimeInterval(-3000), text: "```sh\ncargo build --release\n```", me: "tim")
            MessageRow(author: "tim", time: now, text: "On it. Will report back.", me: "tim", delivery: .pending)
            MessageRow(author: "tim", time: now, text: "This one did not go out.", me: "tim", delivery: .failed)
        }
        .padding(.vertical, Space.s2)
        .background(theme.colors.surface, in: RoundedRectangle(cornerRadius: Radius.xl, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: Radius.xl, style: .continuous).strokeBorder(theme.colors.borderSubtle, lineWidth: 1))
    }
}

// MARK: - channels

struct ChannelSection: View {
    @Environment(\.theme) private var theme

    private func channel(_ id: ID, _ name: String, _ icon: String, _ color: String, kind: ChannelKind = .text) -> Channel {
        Channel(id: id, guildID: 1, name: name, icon: .phosphor(icon), color: color, kind: kind)
    }

    var body: some View {
        let ana = User(id: 2, username: "ana"), bruno = User(id: 3, username: "bruno")
        VStack(alignment: .leading, spacing: 2) {
            ChannelRow(channel(10, "general", "chat-circle-dots", "teal"), selected: true)
            ChannelRow(channel(11, "dev", "code", "blue"), unread: Unread(count: 4, mentions: 2))
            ChannelRow(channel(12, "random", "dice-five", "amber"), unread: Unread(count: 1))
            ChannelRow(channel(14, "announcements", "megaphone", "orange"))
            ChannelRow(channel(15, "design", "paint-brush", "pink"))
            ChannelRow(channel(22, "lounge", "speaker-high", "", kind: .voice))
            VoiceParticipantRow(user: ana, state: VoiceState(guildID: 1, channelID: 22, userID: 2))
            VoiceParticipantRow(user: bruno, state: VoiceState(guildID: 1, channelID: 22, userID: 3, selfMute: true))
        }
        .padding(Space.s2)
        .background(theme.colors.surface, in: RoundedRectangle(cornerRadius: Radius.xl, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: Radius.xl, style: .continuous).strokeBorder(theme.colors.borderSubtle, lineWidth: 1))
    }
}

// MARK: - glass

/// Floating controls are glass; what they float over is a token surface, and text never sits on bare glass.
struct GlassSection: View {
    @Environment(\.theme) private var theme
    @Environment(\.currentDate) private var now

    var body: some View {
        ZStack(alignment: .bottom) {
            VStack(alignment: .leading, spacing: Space.s3) {
                MessageRow(author: "bruno", roleColor: "teal", time: now.addingTimeInterval(-600), text: "Anyone up for lunch? The ramen place opens at noon.")
                MessageRow(author: "ana", time: now.addingTimeInterval(-300), text: "@tim bring the speaker on Friday!", me: "tim")
                Spacer(minLength: 72)
            }
            .padding(.top, Space.s2)
            GlassEffectContainer(spacing: Space.s3) {
                HStack(spacing: Space.s3) {
                    Label("2 new", systemImage: "arrow.down")
                        .font(DesignFont.ui(size: 14, weight: 650))
                        .padding(.horizontal, Space.s4)
                        .frame(minHeight: 44)
                        .glassEffect(.regular.tint(theme.colors.accent.opacity(0.9)).interactive(), in: .capsule)
                        .foregroundStyle(theme.colors.accentFg)
                    Spacer()
                    Image(systemName: "mic.fill").frame(width: 44, height: 44).glassEffect(.regular.interactive(), in: .circle)
                    Image(systemName: "video.fill").frame(width: 44, height: 44).glassEffect(.regular.interactive(), in: .circle)
                    Image(systemName: "phone.down.fill").frame(width: 44, height: 44).glassEffect(.regular.tint(theme.colors.dangerSolid).interactive(), in: .circle)
                        .foregroundStyle(theme.colors.onDanger)
                }
                .padding(.horizontal, Space.s3)
                .padding(.bottom, Space.s3)
            }
        }
        .background(theme.colors.surface, in: RoundedRectangle(cornerRadius: Radius.xl, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: Radius.xl, style: .continuous).strokeBorder(theme.colors.borderSubtle, lineWidth: 1))
        .clipShape(RoundedRectangle(cornerRadius: Radius.xl, style: .continuous))
    }
}

// MARK: - motion

struct MotionSection: View {
    @Environment(\.theme) private var theme
    @Environment(\.motionReduced) private var reduced
    @State private var on = false

    private let springs: [(String, Animation)] = [
        ("press", Motion.press), ("control", Motion.control), ("overlay", Motion.overlay), ("pop", Motion.pop), ("disclosure", Motion.disclosure), ("reflow", Motion.reflow),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s3) {
            Button(on ? "Reset" : "Play springs") { on.toggle() }.buttonStyle(.glass)
            ForEach(springs, id: \.0) { name, spring in
                HStack(spacing: Space.s3) {
                    Text(name).typeStyle(\.caption, color: \.textMuted).frame(width: 76, alignment: .leading)
                    ZStack(alignment: on ? .trailing : .leading) {
                        Capsule().fill(theme.colors.surfaceRaised).frame(height: 14)
                        Circle().fill(theme.colors.accent).frame(width: 14, height: 14)
                            .animation(reduced ? nil : spring, value: on)
                    }
                }
                .accessibilityElement(children: .combine)
            }
            if reduced { Text("Reduce Motion is on: the springs snap.").typeStyle(\.caption, color: \.textMuted) }
        }
        .card()
    }
}
