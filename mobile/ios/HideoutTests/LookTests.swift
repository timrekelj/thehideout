import Foundation
import HideoutDesign
import Testing
@testable import HideoutUI

@MainActor
@Suite("Look persistence")
struct LookTests {
    @Test func decodesTheJSONDesktopWrites() throws {
        let json = #"{"accent":"ocean","background":"cool","appearance":"dark","tint":true,"contrast":"high","density":"compact","text_size":"125","reduce_motion":"on"}"#
        let look = try JSONDecoder().decode(Look.self, from: Data(json.utf8))
        #expect(look.spec.accent == .ocean && look.spec.background == .cool && look.spec.appearance == .dark)
        #expect(look.spec.tint && look.spec.contrast == .high && look.spec.density == .compact && look.spec.textSize == .p125)
        #expect(look.reduceMotion == .on)
    }

    @Test func missingAndUnknownFieldsFallBackToDefaults() throws {
        let look = try JSONDecoder().decode(Look.self, from: Data(#"{"accent":"coral","from_the_future":1}"#.utf8))
        #expect(look.spec.accent == .coral && look.spec.background == .warm && look.reduceMotion == .system)
    }

    @Test func roundTripsAndPersists() throws {
        let suite = UserDefaults(suiteName: "look-tests-\(UUID().uuidString)")!
        let store = ThemeStore(defaults: suite)
        store.update { $0.spec.accent = .brass; $0.spec.textSize = .p110; $0.reduceMotion = .off }
        let again = ThemeStore(defaults: suite)
        #expect(again.spec.accent == .brass && again.spec.textSize == .p110 && again.look.reduceMotion == .off)
        let data = try JSONEncoder().encode(again.look)
        let object = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        #expect(object["text_size"] as? String == "110" && object["reduce_motion"] as? String == "off", "the same shape desktop writes")
    }

    @Test func defaultLookIsTheWarmDarkTokens() {
        let theme = Theme(spec: ThemeSpec(), systemDark: true)
        #expect(theme.colors.bg.hexString == "#16120e" && theme.colors.surface.hexString == "#1e1a16" && theme.colors.accent.hexString == "#68bea8")
        let light = Theme(spec: ThemeSpec(), systemDark: false)
        #expect(light.colors.bg.hexString == "#f6f3ef" && light.colors.accentText.hexString == "#187363")
    }

    @Test func avatarFillsMatchDesktop() {
        #expect(AccentPreset.sageTeal.avatarFills[0].hexString == "#8f4244" && AccentPreset.sageTeal.avatarFills[1].hexString == "#136d5b")
    }
}
