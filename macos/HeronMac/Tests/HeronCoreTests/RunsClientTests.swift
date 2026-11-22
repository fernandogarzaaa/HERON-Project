import XCTest
@testable import HeronCore

/// F3's route and event on the wire: `GET /api/runs`, the `notification` event, and `?notify=1` on the stream.
final class RunsClientTests: XCTestCase {
    private let endpoint = CoreEndpoint(port: 4321, token: "tok", backgroundOwner: true, version: "test")

    private func client() -> RunsClient { RunsClient(endpoint: endpoint, session: StubProtocol.session()) }

    // MARK: The recorded route

    func test_theRecordedFixtureDecodesToItsOneRun() throws {
        let runs = try RunsWire.runs(try Fixtures.data("runs.json"))
        XCTAssertEqual(runs.count, 1)
        let run = try XCTUnwrap(runs.first)
        XCTAssertEqual(run.runID, "0ac6cafd-f4e7-4a24-b391-6a4b814982e4")
        XCTAssertEqual(run.sessionID, "efc252c28ffa")
        XCTAssertEqual(run.title, "Save a plan as plan.md")
        XCTAssertEqual(run.origin, "user")
        XCTAssertEqual(run.status, "done")
        XCTAssertEqual(run.toolCalls, 1)
        XCTAssertEqual(run.started, Date(timeIntervalSince1970: 1795340997))
        XCTAssertEqual(run.ended, Date(timeIntervalSince1970: 1795340997))
        XCTAssertTrue(run.isFinished)
        XCTAssertFalse(run.isWaiting)
    }

    func test_aRunStillGoingHasNoEndAndKeepsItsOrder() throws {
        let json = #"""
        {"runs":[
          {"run_id":"r2","session_id":"s2","title":"Second","origin":"telegram","status":"waiting","started":200,"ended":null,"tool_calls":3},
          {"run_id":"r1","session_id":"s1","title":"First","origin":"user","status":"running","started":100,"ended":null,"tool_calls":0}
        ]}
        """#
        let runs = try RunsWire.runs(Data(json.utf8))
        XCTAssertEqual(runs.map(\.runID), ["r2", "r1"], "the core's order, newest first, is kept")
        XCTAssertNil(runs[0].ended)
        XCTAssertTrue(runs[0].isWaiting)
        XCTAssertFalse(runs[0].isFinished)
        XCTAssertEqual(runs[0].toolCalls, 3)
        XCTAssertFalse(runs[1].isWaiting)
    }

    func test_aRunWithoutItsStatusIsRefused() {
        let json = #"{"runs":[{"run_id":"r","session_id":"s","title":"t","origin":"user","started":1,"ended":null,"tool_calls":0}]}"#
        XCTAssertThrowsError(try RunsWire.runs(Data(json.utf8)))
    }

    func test_onlyInteractiveRunsSendAFinishedNotice() {
        func run(_ origin: String) -> RunRecord { RunRecord(runID: "r", sessionID: "s", title: "t", origin: origin, status: "done", started: Date()) }
        XCTAssertTrue(run("user").sendsFinishedNotice)
        for origin in ["schedule", "automation", "proactive", "telegram"] {
            XCTAssertFalse(run(origin).sendsFinishedNotice, origin)
        }
    }

    // MARK: The request

    func test_getRunsSendsTheTokenAndDecodesTheFixture() async throws {
        let body = try Fixtures.text("runs.json")
        StubProtocol.reset { _ in .init(200, json: body) }
        let runs = try await client().runs()
        XCTAssertEqual(runs, try RunsWire.runs(try Fixtures.data("runs.json")))
        let seen = try XCTUnwrap(StubProtocol.seen.first)
        XCTAssertEqual(seen.method, "GET")
        XCTAssertEqual(seen.path, "/api/runs")
        XCTAssertEqual(seen.token, "tok")
        XCTAssertEqual(StubProtocol.seen.count, 1)
    }

    func test_aFailedAnswerBecomesACoreError() async {
        StubProtocol.reset { _ in .init(500, json: #"{"error":"boom","message":"it broke"}"#) }
        do {
            _ = try await client().runs()
            XCTFail("expected a failure")
        } catch {
            XCTAssertEqual(error as? CoreError, .failed("it broke"))
        }
    }

    // MARK: The notification event

    func test_aNotificationEventDecodesWithNoSession() throws {
        let json = #"{"seq":5,"session_id":null,"run_id":null,"type":"notification","title":"Digest","body":"Three new items","source":"automation"}"#
        let envelope = try CoreWire.envelope(Data(json.utf8))
        XCTAssertEqual(envelope.seq, 5)
        let event = try XCTUnwrap(envelope.event)
        XCTAssertEqual(event.kind, .notification(title: "Digest", body: "Three new items", source: "automation"))
        XCTAssertNil(event.sessionID)
        XCTAssertNil(event.runID)
    }

    func test_aNotificationOfARunKeepsItsIds() throws {
        let json = #"{"seq":9,"session_id":"s1","run_id":"r1","type":"notification","title":"Friday summary","body":"Done","source":"schedule"}"#
        let event = try XCTUnwrap(try CoreWire.envelope(Data(json.utf8)).event)
        XCTAssertEqual(event.sessionID, "s1")
        XCTAssertEqual(event.runID, "r1")
    }

    // MARK: ?notify=1

    func test_theStreamAsksForNoticesOnlyWhenToldTo() async throws {
        let stream = "id: 1\ndata: {\"seq\":1,\"type\":\"run_end\"}\n\n"
        StubProtocol.reset { _ in .init(200, json: stream, contentType: "text/event-stream") }
        let core = CoreClient(endpoint: endpoint, session: StubProtocol.session())
        for try await _ in try await core.openEventConnection(since: nil) {}
        for try await _ in try await core.openEventConnection(since: 2, notify: true) {}
        for try await _ in try await core.openEventConnection(since: nil, notify: true) {}
        XCTAssertEqual(StubProtocol.seen.map(\.query), [nil, "since=2&notify=1", "notify=1"])
        XCTAssertEqual(Set(StubProtocol.seen.map(\.path)), ["/api/events"])
    }
}
