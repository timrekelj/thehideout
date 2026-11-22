import AuthenticationServices
import HideoutCore
import SafariServices
import UIKit

/// Passkeys through the system (`AuthenticationServices`), with the server's ceremony page in Safari as the fallback.
///
/// The server's `options_json` is webauthn-rs' `{"publicKey": {...}}` (base64url binary fields); the credential that
/// goes back is the browser's `PublicKeyCredential` shape (base64url), exactly what its ceremony page posts.
/// The system sheet only works when the app is associated with the server's domain (`webcredentials:` in the
/// entitlements plus `/.well-known/apple-app-site-association` on the server): `canUseNative` is false for a dev
/// server on localhost or an IP, and a refusal ("not associated with domain") becomes `.unavailable` so the caller
/// falls back to the page.
@MainActor
public final class SystemPasskeys: NSObject, PasskeyPlatform {
    private var controller: ASAuthorizationController?
    private var continuation: CheckedContinuation<ASAuthorization, Error>?
    private var safari: SFSafariViewController?
    private var onDismiss: (@MainActor () -> Void)?

    public override init() { super.init() }

    public func canUseNative(host: String) -> Bool { ServerURL.isDomain(host: host) }

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
        let result = try await perform(request)
        guard let c = result.credential as? ASAuthorizationPlatformPublicKeyCredentialRegistration, let attestation = c.rawAttestationObject
        else { throw PasskeyFailure.failed("unexpected credential") }
        return try Self.json([
            "id": Self.b64url(c.credentialID), "rawId": Self.b64url(c.credentialID), "type": "public-key",
            "extensions": [String: String](),
            "response": [
                "attestationObject": Self.b64url(attestation),
                "clientDataJSON": Self.b64url(c.rawClientDataJSON),
                "transports": ["internal", "hybrid"],
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
            // Not associated with the domain, no passkey provider, or not supported here: the page can still do it.
            case .failed, .notHandled, .unknown, .notInteractive, .invalidResponse: failure = .unavailable(e.localizedDescription)
            default: failure = .failed(e.localizedDescription)
            }
        } else {
            failure = .failed(error.localizedDescription)
        }
        continuation?.resume(throwing: failure)
        continuation = nil
    }
}

extension SystemPasskeys: ASAuthorizationControllerPresentationContextProviding {
    public func presentationAnchor(for controller: ASAuthorizationController) -> ASPresentationAnchor {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        let scene = scenes.first { $0.activationState == .foregroundActive } ?? scenes[0]
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
