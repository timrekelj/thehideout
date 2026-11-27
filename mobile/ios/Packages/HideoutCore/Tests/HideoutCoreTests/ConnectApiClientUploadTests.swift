import Foundation
import Network
import Testing
@testable import HideoutCore

/// A small lock around one value, for state a non-`Sendable` closure (`NWListener.stateUpdateHandler`)
/// needs to flip from whatever queue it runs on.
private final class Locked<Value>: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Value
    init(_ value: Value) { self.value = value }
    func withLock<R>(_ body: (inout Value) -> R) -> R { lock.lock(); defer { lock.unlock() }; return body(&value) }
}

/// A tiny loopback TCP server for one connection: sends a 200 whose declared `Content-Length` is
/// longer than what it actually writes, then closes the socket — the same "response starts, then the
/// connection dies mid-body" shape as Android's `ConnectApiClientUploadTest`'s `SocketEffect.CloseSocket`.
private final class TruncatingServer: @unchecked Sendable {
    private let listener: NWListener
    private var connection: NWConnection?

    init() throws {
        listener = try NWListener(using: .tcp, on: .any)
    }

    /// Starts listening and returns the port once it is bound.
    func start() async throws -> UInt16 {
        let resumed = Locked(false)
        return try await withCheckedThrowingContinuation { cont in
            listener.stateUpdateHandler = { [listener] state in
                // `.ready`/`.failed` are each terminal for our purposes; the lock just guards against
                // either firing more than once and resuming the continuation twice.
                switch state {
                case .ready:
                    resumed.withLock { already in if !already { already = true; cont.resume(returning: listener.port!.rawValue) } }
                case .failed(let error):
                    resumed.withLock { already in if !already { already = true; cont.resume(throwing: error) } }
                default: break
                }
            }
            listener.newConnectionHandler = { [weak self] conn in
                self?.connection = conn
                conn.start(queue: .main)
                // A complete HTTP response line + headers claiming 50 bytes of JSON body, but only the
                // first few are ever written: the client's read of the declared-length body must fail,
                // not hang, once the connection closes under it.
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 50\r\n\r\n{\"id\":\"1\""
                conn.send(content: Data(response.utf8), completion: .contentProcessed { _ in
                    conn.forceCancel()
                })
            }
            listener.start(queue: .main)
        }
    }

    func stop() { listener.cancel(); connection?.forceCancel() }
}

/// `ConnectApiClient.uploadAttachment` (ATT-09) over a real loopback socket: the plain-HTTP request it
/// builds, and the specific failure mode the Android review found (reading the response body can itself
/// throw) resolving rather than hanging.
@Suite("ConnectApiClient: POST /attachments")
struct ConnectApiClientUploadTests {
    @Test func percentEncodesTheFilenameLikeDesktopsUploadRs() {
        #expect(ConnectApiClient.encodeUploadComponent("a b/ü.png") == "a%20b%2F%C3%BC.png")
        #expect(ConnectApiClient.encodeUploadComponent("ok-1_2.~x") == "ok-1_2.~x")
    }

    /// A response that starts, then the connection dies before the declared body arrives: the upload
    /// must fail within a bounded time (not hang) with some mapped `AppError`, which `uploadWithRetry`
    /// can then retry if it judges the error transient.
    @Test func aResponseThatDiesMidBodyFailsBoundedInsteadOfHanging() async throws {
        let server = try TruncatingServer()
        let port = try await server.start()
        defer { server.stop() }
        let client = ConnectApiClient(baseURL: URL(string: "http://127.0.0.1:\(port)")!, token: { "tok" })
        let file = tempFile(bytes: 1)

        let task = Task {
            try await client.uploadAttachment(channel: 1, filename: "x", contentType: "text/plain", size: 1, fileURL: file) { _ in }
        }
        let raced = await withTaskGroup(of: Bool.self) { group in
            group.addTask {
                _ = try? await task.value
                return true
            }
            group.addTask {
                try? await Task.sleep(for: .seconds(10))
                return false
            }
            let first = await group.next() ?? false
            group.cancelAll()
            return first
        }
        #expect(raced, "the upload call must resolve (throw) well before the 10 s guard, not hang")
        var threw = false
        do { _ = try await task.value } catch { threw = true }
        #expect(threw)
    }
}
