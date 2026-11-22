import Foundation

/// The core's event bus as one resumable stream (`GET /api/events`, heron-v1.md §3 "One event bus").
///
/// * It remembers the `seq` of the last event it yielded and, when the connection drops, reconnects with
///   `?since=<seq>`, so the bus replays what was missed.
/// * Events at or below that `seq` are dropped, so a replay can never show one twice.
/// * On `resync` (the client fell behind, or asked for a `since` the bus no longer holds, as after a core
///   restart) it yields `.resync` and forgets its position. The next connection carries no `since`: asking
///   again with the old one would earn another `resync` for ever. The consumer reloads its state on `.resync`.
/// * An event type this client doesn't know still advances `seq` and is otherwise ignored.
///
/// The connection is injected, so tests feed it chunks and no port is opened.
public struct EventStream: Sendable {
    /// Opens `GET /api/events` (with `?since=` when given) and returns its raw bytes in chunks of any size.
    /// Throws when the connection can't be made; the stream's end or error means it dropped.
    public typealias Connect = @Sendable (_ since: Int?) async throws -> AsyncThrowingStream<Data, Error>

    private let connect: Connect
    private let retryDelay: Duration
    private let sleep: @Sendable (Duration) async throws -> Void

    /// - Parameter retryDelay: pause before reconnecting after a drop (not after `resync`).
    public init(retryDelay: Duration = .seconds(1),
                sleep: @escaping @Sendable (Duration) async throws -> Void = { try await Task.sleep(for: $0) },
                connect: @escaping Connect) {
        self.connect = connect
        self.retryDelay = retryDelay
        self.sleep = sleep
    }

    /// Events from after `since` (or only new ones when nil) until the consumer stops iterating.
    public func events(since: Int?) -> AsyncStream<CoreEvent> {
        AsyncStream { continuation in
            let task = Task {
                await self.run(since: since, into: continuation)
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    private func run(since: Int?, into continuation: AsyncStream<CoreEvent>.Continuation) async {
        var last = since
        while !Task.isCancelled {
            var resynced = false
            do {
                let bytes = try await connect(last)
                var parser = SSEParser()
                read: for try await chunk in bytes {
                    for message in parser.feed(chunk) {
                        guard let envelope = try? CoreWire.envelope(Data(message.data.utf8)) else { continue }
                        if envelope.isResync {
                            if let event = envelope.event { continuation.yield(event) }
                            last = nil
                            resynced = true
                            break read
                        }
                        if let seq = envelope.seq {
                            if let last, seq <= last { continue }
                            last = seq
                        }
                        if let event = envelope.event { continuation.yield(event) }
                    }
                }
            } catch is CancellationError {
                return
            } catch {
                // Dropped or refused: fall through to the pause and try again.
            }
            if Task.isCancelled { return }
            if resynced { continue }
            do { try await sleep(retryDelay) } catch { return }
        }
    }
}
