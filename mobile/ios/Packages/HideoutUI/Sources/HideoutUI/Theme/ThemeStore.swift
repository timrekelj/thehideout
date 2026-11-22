import HideoutDesign
import Observation
import SwiftUI

/// Whether to follow the system's Reduce Motion setting or force it (desktop's `ReduceMotion`).
public enum ReduceMotionChoice: String, CaseIterable, Codable, Sendable, Identifiable {
    case system, on, off
    public var id: String { rawValue }
    public var displayName: String { rawValue.capitalized }
}

/// Everything on the Appearance page, persisted as the same JSON desktop writes under `appearance`:
/// the `ThemeSpec` fields flattened, plus `reduce_motion`. Unknown or missing fields fall back to defaults,
/// so a file from another version still loads.
public struct Look: Codable, Equatable, Sendable {
    public var spec = ThemeSpec()
    public var reduceMotion: ReduceMotionChoice = .system

    public init(spec: ThemeSpec = ThemeSpec(), reduceMotion: ReduceMotionChoice = .system) {
        self.spec = spec
        self.reduceMotion = reduceMotion
    }

    private enum CodingKeys: String, CodingKey { case reduceMotion = "reduce_motion" }

    public init(from decoder: Decoder) throws {
        spec = try ThemeSpec(from: decoder)
        let c = try decoder.container(keyedBy: CodingKeys.self)
        reduceMotion = (try? c.decodeIfPresent(ReduceMotionChoice.self, forKey: .reduceMotion)) ?? .system
    }

    public func encode(to encoder: Encoder) throws {
        try spec.encode(to: encoder)
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(reduceMotion, forKey: .reduceMotion)
    }
}

/// The user's look, saved to `UserDefaults` on every change and re-resolved into a `Theme` against the system
/// appearance by `.themed(_:)`.
@MainActor @Observable
public final class ThemeStore {
    public static let defaultsKey = "appearance"

    public var look: Look {
        didSet { if look != oldValue { save() } }
    }

    @ObservationIgnored private let defaults: UserDefaults?

    public var spec: ThemeSpec {
        get { look.spec }
        set { look.spec = newValue }
    }

    /// `defaults == nil` keeps the look in memory only (previews and snapshots).
    public init(defaults: UserDefaults? = .standard, look: Look? = nil) {
        self.defaults = defaults
        if let look {
            self.look = look
        } else if let data = defaults?.data(forKey: Self.defaultsKey), let saved = try? JSONDecoder().decode(Look.self, from: data) {
            self.look = saved
        } else {
            self.look = Look()
        }
    }

    private func save() {
        if let data = try? JSONEncoder().encode(look) { defaults?.set(data, forKey: Self.defaultsKey) }
    }

    /// Changes the look with the colours cross-fading on `Motion.reflow`, unless motion is reduced.
    public func update(reducedMotion: Bool = false, _ change: (inout Look) -> Void) {
        var new = look
        change(&new)
        let off = reducedMotion || new.reduceMotion == .on
        if off { look = new } else { withAnimation(Motion.reflow) { look = new } }
    }

    public func reset() { look = Look() }
}
