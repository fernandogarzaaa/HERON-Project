import XCTest
@testable import HeronCore

/// A scripted core: each call to `connect` takes the next plan. Records the `since` of every call.
final class ScriptedConnections: @unchecked Sendable {
    enum Plan {
        /// Serve these chunks, then end the connection (a drop).
        case chunks([String])
        /// Refuse the connection.
        case refuse
    }

    private let lock = NSLock()
    private var plans: [Plan]
    private(set) var sinces: [Int?] = []

    init(_ plans: [Plan]) { self.plans = plans }

    func connect(since: Int?) async throws -> AsyncThrowingStream<Data, Error> {
        let plan: Plan? = lock.withLock {
            sinces.append(since)
            return plans.isEmpty ? nil : plans.removeFirst()
        }
        switch plan {
        case .refuse:
            throw CoreError.failed("refused")
        case .chunks(let chunks):
            return AsyncThrowingStream { continuation in
                for chunk in chunks { continuation.yield(Data(chunk.utf8)) }
                continuation.finish()
            }
        case nil:
            // Nothing more scripted: a connection that stays open and silent.
            return AsyncThrowingStream { _ in }
        }
    }

    var calls: [Int?] { lock.withLock { sinces } }

    /// Asserts the scripted connections exactly, plus the one reconnect that may follow them.
    ///
    /// Once the plans run out, the stream reconnects again (as it should: it resumes from the last
    /// sequence it saw) and that connection stays open and silent. Whether that extra call lands
    /// before or after the test stops listening is a race, so it is allowed but never required;
    /// and when it is there it must carry `resumeFrom`, and it can only be one call.
    func assertCalls(
        _ scripted: [Int?], thenResumingFrom resumeFrom: Int?, _ message: String,
        file: StaticString = #filePath, line: UInt = #line
    ) {
        let calls = self.calls
        XCTAssertEqual(Array(calls.prefix(scripted.count)), scripted, message, file: file, line: line)
        XCTAssertLessThanOrEqual(calls.count, scripted.count + 1, "at most one reconnect after the script ends", file: file, line: line)
        XCTAssertTrue(
            calls.dropFirst(scripted.count).allSatisfy { $0 == resumeFrom },
            "a reconnect after the script must resume from \(String(describing: resumeFrom)), got \(calls)", file: file, line: line)
    }
}

/// One SSE frame as the core writes it: `id: <seq>` and the envelope as `data:`.
func frame(_ seq: Int, _ type: String, _ fields: String = "") -> String {
    "id: \(seq)\ndata: {\"seq\":\(seq),\"session_id\":\"s1\",\"run_id\":\"r1\",\"type\":\"\(type)\"\(fields.isEmpty ? "" : "," + fields)}\n\n"
}

func textFrame(_ seq: Int, _ text: String) -> String { frame(seq, "text", "\"text\":\"\(text)\"") }

func resyncFrame(oldest: Int) -> String { "data: {\"type\":\"resync\",\"oldest\":\(oldest)}\n\n" }

/// The first `count` events of a stream, then stops listening (which cancels the stream's task).
func take(_ stream: AsyncStream<CoreEvent>, _ count: Int, timeout: Duration = .seconds(10)) async throws -> [CoreEvent] {
    try await withThrowingTaskGroup(of: [CoreEvent].self) { group in
        group.addTask {
            var out: [CoreEvent] = []
            for await event in stream {
                out.append(event)
                if out.count == count { break }
            }
            return out
        }
        group.addTask {
            try await Task.sleep(for: timeout)
            throw CoreError.failed("timed out waiting for \(count) events")
        }
        let first = try await group.next()!
        group.cancelAll()
        return first
    }
}

final class EventStreamResumeTests: XCTestCase {
    private func stream(_ plans: [ScriptedConnections.Plan]) -> (EventStream, ScriptedConnections) {
        let connections = ScriptedConnections(plans)
        // The pause between reconnects is a no-op, so nothing here waits.
        let stream = EventStream(retryDelay: .zero, sleep: { _ in }, connect: { try await connections.connect(since: $0) })
        return (stream, connections)
    }

    private func texts(_ events: [CoreEvent]) -> [String] {
        events.compactMap { (event: CoreEvent) -> String? in
            if case .text(let t) = event.kind { return t }
            return nil
        }
    }

    func testReconnectsFromTheLastSequence() async throws {
        let (events, connections) = stream([
            .chunks([textFrame(1, "a"), textFrame(2, "b")]),
            // The bus replays from after `since`; a replay that repeats seq 2 must not show it twice.
            .chunks([textFrame(2, "b"), textFrame(3, "c")]),
        ])
        let got = try await take(events.events(since: nil), 3)
        XCTAssertEqual(got.map(\.seq), [1, 2, 3])
        XCTAssertEqual(texts(got), ["a", "b", "c"])
        connections.assertCalls([nil, 2], thenResumingFrom: 3, "the first connection has no since; the second resumes from seq 2")
    }

    func testStartsFromTheSinceItWasGiven() async throws {
        let (events, connections) = stream([.chunks([textFrame(41, "x"), textFrame(42, "y")])])
        let got = try await take(events.events(since: 40), 2)
        XCTAssertEqual(got.map(\.seq), [41, 42])
        XCTAssertEqual(connections.calls.first, 40)
    }

    func testEventsAtOrBelowTheLastSequenceAreDropped() async throws {
        let (events, _) = stream([.chunks([textFrame(5, "five"), textFrame(5, "again"), textFrame(4, "old"), textFrame(6, "six")])])
        let got = try await take(events.events(since: nil), 2)
        XCTAssertEqual(texts(got), ["five", "six"])
    }

    func testResyncIsYieldedAndTheNextConnectionAsksForNoSince() async throws {
        let (events, connections) = stream([
            .chunks([textFrame(8, "a"), resyncFrame(oldest: 100)]),
            // A restarted core numbers from 1 again: those are new events, not stale ones.
            .chunks([textFrame(1, "fresh")]),
        ])
        let got = try await take(events.events(since: 7), 3)
        guard case .resync(let oldest) = got[1].kind else { return XCTFail("expected resync, got \(got[1])") }
        XCTAssertEqual(oldest, 100)
        XCTAssertEqual(texts(got), ["a", "fresh"])
        connections.assertCalls([7, nil], thenResumingFrom: 1, "asking again with the old since would earn another resync for ever")
    }

    func testAResyncEndsTheConnectionEvenWithEventsAfterIt() async throws {
        let (events, _) = stream([.chunks([resyncFrame(oldest: 3) + textFrame(9, "ignored")]), .chunks([textFrame(1, "new")])])
        let got = try await take(events.events(since: 2), 2)
        XCTAssertEqual(texts(got), ["new"])
    }

    func testAnUnknownEventTypeStillAdvancesTheSequence() async throws {
        let (events, connections) = stream([
            .chunks([textFrame(1, "a"), frame(2, "something_new", "\"x\":1")]),
            .chunks([textFrame(3, "c")]),
        ])
        let got = try await take(events.events(since: nil), 2)
        XCTAssertEqual(texts(got), ["a", "c"])
        connections.assertCalls([nil, 2], thenResumingFrom: 3, "the unknown seq 2 counts as seen")
    }

    func testRetriesAfterARefusedConnection() async throws {
        let (events, connections) = stream([.refuse, .chunks([textFrame(1, "a")])])
        let got = try await take(events.events(since: nil), 1)
        XCTAssertEqual(texts(got), ["a"])
        connections.assertCalls([nil, nil], thenResumingFrom: 1, "nothing was seen, so there is nothing to resume from")
    }

    func testChunksMayEndMidLine() async throws {
        let whole = textFrame(1, "split") + textFrame(2, "tail")
        let cut = whole.index(whole.startIndex, offsetBy: 30)
        let (events, _) = stream([.chunks([String(whole[..<cut]), String(whole[cut...])])])
        let got = try await take(events.events(since: nil), 2)
        XCTAssertEqual(texts(got), ["split", "tail"])
    }

    func testKeepAliveCommentsAreIgnored() async throws {
        let (events, _) = stream([.chunks([":\n\n", textFrame(1, "a"), ": keep-alive\n\n", textFrame(2, "b")])])
        let got = try await take(events.events(since: nil), 2)
        XCTAssertEqual(texts(got), ["a", "b"])
    }

    func testStoppingTheConsumerStopsTheStream() async throws {
        let (events, connections) = stream([.chunks([textFrame(1, "a")])])
        _ = try await take(events.events(since: nil), 1)
        try await Task.sleep(for: .milliseconds(100))
        let calls = connections.calls.count
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertEqual(connections.calls.count, calls, "no reconnecting after the consumer is gone")
    }
}
