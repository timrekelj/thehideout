import Foundation
import HideoutCore
@testable import HideoutUI
import Testing

/// `SystemPasskeys`'s pure helpers: the base64url codec and the `options_json` parsing. The ceremony itself
/// (`assertion`/`registration`) needs a real `ASAuthorizationController` and a UI, so it is exercised by hand
/// (`ios/README.md` "Auth and onboarding") rather than here.
@MainActor
@Suite("SystemPasskeys helpers")
struct SystemPasskeysTests {
    // MARK: base64url round trip

    @Test func roundTripsArbitraryBytesIncludingThoseThatNeedPadding() {
        // 1, 2 and 3-byte-remainder cases exercise every amount of `=` padding standard base64 would add.
        for length in [0, 1, 2, 3, 4, 5, 16, 31, 32] {
            let bytes = Data((0..<length).map { UInt8(($0 * 37 + 11) % 256) })
            let encoded = SystemPasskeys.b64url(bytes)
            #expect(!encoded.contains("="), "base64url has no padding")
            #expect(!encoded.contains("+") && !encoded.contains("/"), "base64url substitutes - and _")
            #expect(SystemPasskeys.data(encoded) == bytes)
        }
    }

    @Test func decodesWithAndWithoutTheDashAndUnderscoreSubstitution() {
        // Standard base64 for 0xFB 0xFF 0xBE: "+/++" becomes "-_--" in base64url.
        let bytes = Data([0xFB, 0xFF, 0xBE])
        #expect(SystemPasskeys.data("-_--") == bytes)
        #expect(SystemPasskeys.data("+/++") == bytes, "plain base64 still decodes (padding added back)")
    }

    @Test func decodeRefusesNonBase64() {
        #expect(SystemPasskeys.data("not base64!!") == nil)
        #expect(SystemPasskeys.data(123) == nil, "a non-string value")
    }

    // MARK: options_json parsing

    @Test func unwrapsThePublicKeyEnvelope() throws {
        let pk = try SystemPasskeys.publicKey(#"{"publicKey": {"challenge": "abc", "rpId": "example.com"}}"#)
        #expect(pk["challenge"] as? String == "abc")
        #expect(pk["rpId"] as? String == "example.com")
    }

    @Test func rejectsJSONWithoutThePublicKeyKey() {
        #expect(throws: PasskeyFailure.self) { try SystemPasskeys.publicKey(#"{"challenge": "abc"}"#) }
        #expect(throws: PasskeyFailure.self) { try SystemPasskeys.publicKey("not json") }
        #expect(throws: PasskeyFailure.self) { try SystemPasskeys.publicKey("") }
    }

    @Test func parsesAnAssertionOptionsPayload() throws {
        // Shape of webauthn-rs' login options: no `rp` object, an `allowCredentials` list.
        let json = """
        {"publicKey": {
            "challenge": "AQIDBA", "rpId": "thehideout.zabit.si", "userVerification": "preferred",
            "allowCredentials": [{"id": "AQID", "type": "public-key"}]
        }}
        """
        let pk = try SystemPasskeys.publicKey(json)
        #expect(pk["rpId"] as? String == "thehideout.zabit.si")
        #expect(SystemPasskeys.data(pk["challenge"]) == Data([1, 2, 3, 4]))
        let allowed = (pk["allowCredentials"] as? [[String: Any]])?.compactMap { SystemPasskeys.data($0["id"]) }
        #expect(allowed == [Data([1, 2, 3])])
        #expect(SystemPasskeys.verification(pk["userVerification"] as? String) == .preferred)
    }

    @Test func parsesARegistrationOptionsPayload() throws {
        // Shape of webauthn-rs' registration options: a nested `rp` and `user`, `authenticatorSelection`.
        let json = """
        {"publicKey": {
            "challenge": "AQIDBA", "rp": {"id": "thehideout.zabit.si", "name": "thehideout"},
            "user": {"id": "BQYHCA", "name": "tim", "displayName": "tim"},
            "authenticatorSelection": {"userVerification": "required"}, "attestation": "direct"
        }}
        """
        let pk = try SystemPasskeys.publicKey(json)
        let rp = try #require(pk["rp"] as? [String: Any])
        #expect(rp["id"] as? String == "thehideout.zabit.si")
        let user = try #require(pk["user"] as? [String: Any])
        #expect(user["name"] as? String == "tim")
        #expect(SystemPasskeys.data(user["id"]) == Data([5, 6, 7, 8]))
        let authenticatorSelection = pk["authenticatorSelection"] as? [String: Any]
        #expect(SystemPasskeys.verification(authenticatorSelection?["userVerification"] as? String) == .required)
        #expect(SystemPasskeys.attestation(pk["attestation"] as? String) == .direct)
    }

    @Test func verificationAndAttestationDefaultWhenUnspecified() {
        #expect(SystemPasskeys.verification(nil) == .preferred)
        #expect(SystemPasskeys.verification("discouraged") == .discouraged)
        #expect(SystemPasskeys.verification("something-new") == .preferred, "an unknown value is not fatal")
        #expect(SystemPasskeys.attestation(nil) == .none)
        #expect(SystemPasskeys.attestation("indirect") == .indirect)
        #expect(SystemPasskeys.attestation("enterprise") == .enterprise)
    }
}
