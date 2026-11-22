import AuthenticationServices
import HideoutCore
import SafariServices
import UIKit

/// Passkeys through the system (`AuthenticationServices`), with the server's ceremony page in Safari as the fallback.
///
/// The server's `options_json` is webauthn-rs' `{"publicKey": {...}}` (base64url binary fields); the credential that
/// goes back is the browser's `PublicKeyCredential` shape (base64url), exactly what its ceremony page posts.
/// The system sheet only works when the app is associated with the server's domain (`webcredentials:` in the
/// entitlements plus `/.well-known/apple-app-site-association` on the server): `canUseNative` checks the host
/// against `associatedDomains` (false for a dev server on localhost or an IP, and for a domain not on the list,
/// even a real one), and a refusal that means "not associated with this domain" becomes `.unavailable` so the
/// caller falls back to the page.
@MainActor
public final class SystemPasskeys: NSObject, PasskeyPlatform {
    private var controller: ASAuthorizationController?
    private var continuation: CheckedContinuation<ASAuthorization, Error>?
    private var safari: SFSafariViewController?
    private var onDismiss: (@MainActor () -> Void)?

    public override init() { super.init() }

    /// The domains this app is associated with (`Hideout.entitlements`' `webcredentials:` list, kept in sync by
    /// hand with `project.yml`). The system sheet fails for any other host, so skip straight to the browser page
    /// for one instead of round-tripping a request we already know will be refused.
    static let associatedDomains: Set<String> = ["thehideout.zabit.si"]

    public func canUseNative(host: String) -> Bool { Self.associatedDomains.contains(host.lowercased()) }

    // MARK: system sheet

    public func assertion(optionsJSON: String) async throws -> String {
        let pk = try Self.publicKey(optionsJSON)
        guard let rp = pk["rpId"] as? String, let challenge = Self.data(pk["challenge"]) else { throw PasskeyFailure.failed("bad options") }
        let provider = ASAuthorizationPlatformPublicKeyCredentialProvider(relyingPartyIdentifier: rp)
        let request = provider.createCredentialAssertionRequest(challenge: challenge)
        let allowed = (pk["allowCredentials"] as? [[String: Any]] ?? []).compactMap { Self.data($0["id"]) }
        if !allowed.isEmpty { request.allowedCredentials = allowed.map { ASAuthorizationPlatformPublicKeyCredentialDescriptor(credentialID: $0) } }
        request.userVerificationPreference = Self.verification(pk["userVerification"] as? String)
        let result = try await perform(request)
        guard let c = result.credential as? ASAuthorizationPlatformPublicKeyCredentialAssertion else { throw PasskeyFailure.failed("unexpected credential") }
        return try Self.json([
            "id": Self.b64url(c.credentialID), "rawId": Self.b64url(c.credentialID), "type": "public-key",
            "extensions": [String: String](),
            "response": [
                "authenticatorData": Self.b64url(c.rawAuthenticatorData),
                "clientDataJSON": Self.b64url(c.rawClientDataJSON),
                "signature": Self.b64url(c.signature),
                "userHandle": Self.b64url(c.userID),
            ],
        ])
    }

    public func registration(optionsJSON: String) async throws -> String {
        let pk = try Self.publicKey(optionsJSON)
        guard let rp = (pk["rp"] as? [String: Any])?["id"] as? String, let challenge = Self.data(pk["challenge"]),
              let user = pk["user"] as? [String: Any], let userID = Self.data(user["id"]), let name = user["name"] as? String
        else { throw PasskeyFailure.failed("bad options") }
        let provider = ASAuthorizationPlatformPublicKeyCredentialProvider(relyingPartyIdentifier: rp)
        let request = provider.createCredentialRegistrationRequest(challenge: challenge, name: name, userID: userID)
        // `authenticatorSelection.userVerification` (registration), not the top-level field assertion options use.
        let authenticatorSelection = pk["authenticatorSelection"] as? [String: Any]
        request.userVerificationPreference = Self.verification(authenticatorSelection?["userVerification"] as? String)
        request.attestationPreference = Self.attestation(pk["attestation"] as? String)
        // There is no public API to pass `excludeCredentials` to a platform registration request (only the web
        // browser and security-key variants take one): a re-registration on an already-enrolled device is left to
        // the server to reject, same as it would for a duplicate from any other client.
        let result = try await perform(request)
        guard let c = result.credential as? ASAuthorizationPlatformPublicKeyCredentialRegistration, let attestation = c.rawAttestationObject
        else { throw PasskeyFailure.failed("unexpected credential") }
        return try Self.json([
            "id": Self.b64url(c.credentialID), "rawId": Self.b64url(c.credentialID), "type": "public-key",
            "extensions": [String: String](),
            "response": [
                "attestationObject": Self.b64url(attestation),
                "clientDataJSON": Self.b64url(c.rawClientDataJSON),
                // Optional per the WebAuthn spec; this is always a platform (built-in) authenticator here, and
                // there is no public API on this credential type to report whether hybrid was actually used.
            ],
        ])
    }

    private func perform(_ request: ASAuthorizationRequest) async throws -> ASAuthorization {
        let controller = ASAuthorizationController(authorizationRequests: [request])
        controller.delegate = self
        controller.presentationContextProvider = self
        self.controller = controller
        defer { self.controller = nil }
        return try await withCheckedThrowingContinuation { (c: CheckedContinuation<ASAuthorization, Error>) in
            // A pending continuation here would mean two ceremonies overlapped; reject the older one instead of
            // silently dropping it (a `CheckedContinuation` resumed twice, or never, is a crash / a hung caller).
            if let previous = continuation {
                continuation = nil
                previous.resume(throwing: PasskeyFailure.cancelled)
            }
            continuation = c
            controller.performRequests()
        }
    }

    // MARK: browser page

    public func openCeremonyPage(_ url: URL, onDismiss: @escaping @MainActor () -> Void) {
        let vc = SFSafariViewController(url: url)
        vc.delegate = self
        vc.dismissButtonStyle = .cancel
        safari = vc
        self.onDismiss = onDismiss
        Self.topViewController()?.present(vc, animated: true)
    }

    public func closeCeremonyPage() {
        onDismiss = nil
        safari?.dismiss(animated: true)
        safari = nil
    }

    // MARK: helpers

    private static func publicKey(_ json: String) throws -> [String: Any] {
        guard let obj = try? JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any], let pk = obj["publicKey"] as? [String: Any]
        else { throw PasskeyFailure.failed("bad options") }
        return pk
    }

    private static func verification(_ s: String?) -> ASAuthorizationPublicKeyCredentialUserVerificationPreference {
        switch s {
        case "required": .required
        case "discouraged": .discouraged
        default: .preferred
        }
    }

    private static func attestation(_ s: String?) -> ASAuthorizationPublicKeyCredentialAttestationKind {
        switch s {
        case "direct": .direct
        case "indirect": .indirect
        case "enterprise": .enterprise
        default: .none
        }
    }

    static func data(_ any: Any?) -> Data? {
        guard var s = any as? String else { return nil }
        s = s.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while s.count % 4 != 0 { s += "=" }
        return Data(base64Encoded: s)
    }

    static func b64url(_ d: Data) -> String {
        d.base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }

    private static func json(_ object: [String: Any]) throws -> String {
        let data = try JSONSerialization.data(withJSONObject: object)
        return String(decoding: data, as: UTF8.self)
    }

    private static func topViewController() -> UIViewController? {
        let scene = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first { $0.activationState == .foregroundActive }
        var top = scene?.keyWindow?.rootViewController
        while let next = top?.presentedViewController { top = next }
        return top
    }
}

extension SystemPasskeys: ASAuthorizationControllerDelegate {
    public func authorizationController(controller: ASAuthorizationController, didCompleteWithAuthorization authorization: ASAuthorization) {
        continuation?.resume(returning: authorization)
        continuation = nil
    }

    public func authorizationController(controller: ASAuthorizationController, didCompleteWithError error: Error) {
        let failure: PasskeyFailure
        if let e = error as? ASAuthorizationError {
            switch e.code {
            case .canceled: failure = .cancelled
            // No passkey provider, or not a supported context here: the page can still do it.
            case .notHandled, .unknown, .notInteractive, .invalidResponse: failure = .unavailable(e.localizedDescription)
            // `.failed` covers everything from "not associated with this domain" to a real authenticator refusal;
            // only the former should fall back quietly, so look at what is actually wrong before deciding.
            case .failed where Self.isDomainAssociationFailure(e): failure = .unavailable(e.localizedDescription)
            case .failed: failure = .failed("That passkey wasn't accepted.")
            default: failure = .failed("That passkey wasn't accepted.")
            }
        } else {
            failure = .failed("That passkey wasn't accepted.")
        }
        continuation?.resume(throwing: failure)
        continuation = nil
    }

    /// Whether `error` means "this app isn't associated with the relying party's domain" rather than a real
    /// authenticator refusal (a wrong/missing passkey, user declined, hardware issue, ...), which should surface
    /// to the person instead of silently falling back to the browser page.
    private static func isDomainAssociationFailure(_ error: ASAuthorizationError) -> Bool {
        let ns = error as NSError
        let underlying = ns.userInfo[NSUnderlyingErrorKey] as? NSError
        let text = (underlying?.localizedDescription ?? ns.localizedDescription).lowercased()
        return text.contains("not associated with domain") || text.contains("webcredentials") || text.contains("not eligible")
    }
}

extension SystemPasskeys: ASAuthorizationControllerPresentationContextProviding {
    public func presentationAnchor(for controller: ASAuthorizationController) -> ASPresentationAnchor {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        // Prefer the foreground-active scene, but a brief gap during a scene transition should not crash on an
        // out-of-bounds `scenes[0]`: fall back to any connected scene.
        guard let scene = scenes.first(where: { $0.activationState == .foregroundActive }) ?? scenes.first else {
            preconditionFailure("no window scene to present the passkey sheet on")
        }
        return scene.keyWindow ?? UIWindow(windowScene: scene)
    }
}

extension SystemPasskeys: @MainActor SFSafariViewControllerDelegate {
    public func safariViewControllerDidFinish(_ controller: SFSafariViewController) {
        safari = nil
        let done = onDismiss
        onDismiss = nil
        done?()
    }
}
