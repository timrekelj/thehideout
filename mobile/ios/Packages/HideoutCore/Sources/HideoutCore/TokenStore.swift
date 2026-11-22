import Foundation
import Security

/// Where the session token lives between launches.
public protocol TokenStore: Sendable {
    func load() -> String?
    func save(_ token: String) throws
    func clear()
}

/// Keychain generic password, `AfterFirstUnlock` (so a background wake can read it), never synced to iCloud.
public struct KeychainTokenStore: TokenStore {
    public struct KeychainError: Error, Equatable { public let status: OSStatus }

    private let service: String
    private let account: String

    public init(service: String = "si.zabit.thehideout.session", account: String = "token") {
        self.service = service
        self.account = account
    }

    private var base: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account,
         kSecAttrSynchronizable as String: false]
    }

    public func load() -> String? {
        var query = base
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    public func save(_ token: String) throws {
        let data = Data(token.utf8)
        let update = SecItemUpdate(base as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if update == errSecSuccess { return }
        guard update == errSecItemNotFound else { throw KeychainError(status: update) }
        var add = base
        add[kSecValueData as String] = data
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        let status = SecItemAdd(add as CFDictionary, nil)
        guard status == errSecSuccess else { throw KeychainError(status: status) }
    }

    public func clear() {
        SecItemDelete(base as CFDictionary)
    }
}

/// For tests, previews and `--fake`.
public final class MemoryTokenStore: TokenStore, @unchecked Sendable {
    private let lock = NSLock()
    private var token: String?
    public init(token: String? = nil) { self.token = token }
    public func load() -> String? { lock.withLock { token } }
    public func save(_ token: String) throws { lock.withLock { self.token = token } }
    public func clear() { lock.withLock { token = nil } }
}
