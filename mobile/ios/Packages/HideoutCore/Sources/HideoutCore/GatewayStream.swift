import Foundation

/// Reconnect and liveness tuning (desktop's `GatewayConfig`).
public struct GatewayConfig: Sendable, Equatable {
    /// Delay before the first retry; doubles on each consecutive failure.
    public var initialBackoff: TimeInterval = 0.5
    /// Upper bound of the retry delay (before jitter).
    public var maxBackoff: TimeInterval = 30
    /// The server's heartbeat interval.
    public var heartbeatInterval: TimeInterval = 30
    /// The stream is declared dead after `heartbeatInterval * silenceFactor` without any event.
    public var silenceFactor: Double = 2.5
    /// Random spread of the retry delay, as a fraction (0.2 = +-20 %).
    public var jitter: Double = 0.2

    public init() {}

    public var silenceTimeout: TimeInterval { heartbeatInterval * silenceFactor }

    /// The delay before retry number `attempt` (0 = first), without jitter.
    public func backoff(attempt: Int) -> TimeInterval {
        min(maxBackoff, initialBackoff * pow(2, Double(min(attempt, 30))))
    }
}

/// Owns the server stream: opens `Subscribe`, forwards events, watches heartbeats for a dead stream and
/// reconnects with backoff. It reports `.connected` when a stream delivers its first event and
/// `.disconnected` on every break; the store marks the stream `interrupted` and refetches on reconnect,
/// because the server does not replay. It stops for good on `unauthenticated` and `clientTooOld`.
public actor GatewayStream {
    private let source: any GatewaySource
    private let config: GatewayConfig
    private let sleep: @Sendable (TimeInterval) async throws -> Void
    private var task: Task<Void, Never>?
    private var lastEvent = ContinuousClock.now

    public init(source: any GatewaySource, config: GatewayConfig = GatewayConfig(),
                sleep: @escaping @Sendable (TimeInterval) async throws -> Void = { try await Task.sleep(for: .seconds($0)) }) {
        self.source = source
        self.config = config
        self.sleep = sleep
    }

    public var isRunning: Bool { task != nil }

    /// Starts (or restarts) the stream; `handler` is awaited for every update, in order.
    public func start(_ handler: @escaping @Sendable (GatewayUpdate) async -> Void) {
        stop()
        task = Task { await self.run(handler) }
    }

    public func stop() {
        task?.cancel()
        task = nil
    }

    private func touch() { lastEvent = .now }

    private enum Outcome { case ended, failed(AppError), silent }

    private func run(_ handler: @escaping @Sendable (GatewayUpdate) async -> Void) async {
        var attempt = 0
        while !Task.isCancelled {
            await handler(.connecting)
            var delivered = false
            let outcome = await consume(source.subscribe(), handler: handler, delivered: &delivered)
            if Task.isCancelled { return }
            if delivered { attempt = 0 }
            let reason: String
            switch outcome {
            case .failed(.unauthenticated): await handler(.authExpired); return
            case .failed(.clientTooOld): await handler(.updateRequired); return
            case .failed(let e): reason = e.description
            case .silent: reason = "no heartbeat"
            case .ended: reason = "stream ended"
            }
            let base = config.backoff(attempt: attempt)
            let spread = config.jitter > 0 ? Double.random(in: -config.jitter...config.jitter) : 0
            let delay = max(0, base * (1 + spread))
            attempt += 1
            await handler(.disconnected(retryIn: delay, reason: reason))
            do { try await sleep(delay) } catch { return }
        }
    }

    private func consume(_ stream: AsyncThrowingStream<GatewayEvent, Error>, handler: @escaping @Sendable (GatewayUpdate) async -> Void, delivered: inout Bool) async -> Outcome {
        lastEvent = .now
        let silence = config.silenceTimeout
        let box = Delivered()
        let outcome = await withTaskGroup(of: Outcome.self) { group in
            group.addTask {
                do {
                    for try await event in stream {
                        await self.touch()
                        if await box.markFirst() { await handler(.connected) }
                        await handler(.event(event))
                    }
                    return .ended
                } catch {
                    return .failed(AppError.from(error))
                }
            }
            group.addTask {
                // Watchdog: wakes when the stream might have been silent for `silence`.
                while !Task.isCancelled {
                    let idle = await self.idleTime()
                    let wait = silence - idle
                    if wait <= 0 { return .silent }
                    do { try await Task.sleep(for: .seconds(wait)) } catch { return .ended }
                }
                return .ended
            }
            let first = await group.next() ?? .ended
            group.cancelAll()
            return first
        }
        delivered = await box.value
        return outcome
    }

    private func idleTime() -> TimeInterval {
        let d = ContinuousClock.now - lastEvent
        return Double(d.components.seconds) + Double(d.components.attoseconds) / 1e18
    }

    private actor Delivered {
        var value = false
        func markFirst() -> Bool { defer { value = true }; return !value }
    }
}
