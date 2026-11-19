import XCTest
@testable import HeronCore

/// The stub's behaviour, which U4b keeps for previews and tests.
final class PreviewCoreClientTests: XCTestCase {
    private func client(accepted: Bool = true) async throws -> PreviewCoreClient {
        let c = PreviewCoreClient()
        if accepted { try await c.acceptRiskNotice(version: 3) }
        return c
    }

    /// Reads events until a run ends, calling `onApproval` when one arrives.
    private func collect(_ stream: AsyncStream<CoreEvent>, onApproval: ((String) async throws -> Void)? = nil) async throws -> [CoreEvent] {
        var out: [CoreEvent] = []
        for await e in stream {
            out.append(e)
            if case .approval(let r) = e.kind { try await onApproval?(r.id) }
            if case .runEnd = e.kind { break }
        }
        return out
    }

    func testAPlainRunPublishesTheDocumentedEventSequence() async throws {
        let c = try await client()
        let stream = c.events(since: nil)
        let start = try await c.sendMessage(sessionID: nil, message: "What is on today?", title: nil)
        let events = try await collect(stream)
        let kinds = events.map { e -> String in
            switch e.kind {
            case .session: "session"
            case .toolStart: "tool_start"
            case .toolEnd: "tool_end"
            case .text: "text"
            case .done: "done"
            case .runEnd: "run_end"
            default: "other"
            }
        }
        XCTAssertEqual(kinds, ["session", "tool_start", "tool_end", "text", "done", "run_end"])
        XCTAssertEqual(events.map(\.seq), Array(1...6), "seq starts at 1 and counts up")
        XCTAssertTrue(events.allSatisfy { $0.sessionID == start.sessionID })
        XCTAssertEqual(events.first?.runID, start.runID)
    }

    func testAnApprovalWaitsForTheAnswer() async throws {
        let c = try await client()
        let stream = c.events(since: nil)
        _ = try await c.sendMessage(sessionID: nil, message: "Draft a reply", title: nil)
        var resolved: String?
        let events = try await collect(stream) { id in try await c.approve(id: id, decision: .session) }
        for e in events { if case .approvalResolved(_, let decision) = e.kind { resolved = decision } }
        XCTAssertEqual(resolved, "session", "approval_resolved carries the decision that was posted")
        XCTAssertTrue(events.contains { if case .toolStart(_, "write_file", _, _) = $0.kind { true } else { false } })
    }

    func testFailureRunEndsWithAnErrorAndRunEnd() async throws {
        let c = try await client()
        let stream = c.events(since: nil)
        _ = try await c.sendMessage(sessionID: nil, message: "please fail", title: nil)
        let events = try await collect(stream)
        let error = events.compactMap { e -> (String?, String?)? in if case .error(_, let code, _, let version) = e.kind { (code, version) } else { nil } }.first
        XCTAssertEqual(error?.0, "cli_version_untested")
        XCTAssertEqual(error?.1, "3.0.1")
        if case .runEnd = events.last?.kind {} else { XCTFail("the last event of every run is run_end") }
    }

    func testASecondMessageOnABusySessionThrowsSessionBusy() async throws {
        let c = try await client()
        let first = try await c.sendMessage(sessionID: nil, message: "Draft a reply", title: nil)
        do {
            _ = try await c.sendMessage(sessionID: first.sessionID, message: "again", title: nil)
            XCTFail("expected sessionBusy")
        } catch { XCTAssertEqual(error as? CoreError, .sessionBusy) }
        let detail = try await c.session(id: first.sessionID)
        XCTAssertTrue(detail.busy)
        do { try await c.deleteSession(id: first.sessionID); XCTFail("a running reply can't be deleted") } catch {}
        let cancelled = try await c.cancel(sessionID: first.sessionID)
        XCTAssertTrue(cancelled)
        let again = try await c.cancel(sessionID: first.sessionID)
        XCTAssertFalse(again, "nothing was running")
    }

    func testChatIsRefusedUntilTheRiskNoticeIsAccepted() async throws {
        let c = try await client(accepted: false)
        do { _ = try await c.sendMessage(sessionID: nil, message: "hi", title: nil); XCTFail("expected a refusal") }
        catch { XCTAssertEqual(error as? CoreError, .riskNoticeNotAccepted) }
        do { try await c.acceptRiskNotice(version: 2); XCTFail("an old version is refused") }
        catch { XCTAssertEqual(error as? CoreError, .riskNoticeOutdated) }
        try await c.acceptRiskNotice(version: 3)
        let notice = try await c.riskNotice()
        XCTAssertEqual(notice.acceptedVersion, 3)
        c.simulateRiskNoticeChange()
        let changed = try await c.riskNotice()
        XCTAssertEqual(changed.version, 4)
        XCTAssertNil(changed.acceptedVersion, "a new version shows the notice again")
    }

    func testEventsReplayFromSince() async throws {
        let c = try await client()
        let live = c.events(since: nil)
        _ = try await c.sendMessage(sessionID: nil, message: "hello", title: nil)
        _ = try await collect(live)
        var seen: [Int] = []
        for await e in c.events(since: 4) { seen.append(e.seq); if case .runEnd = e.kind { break } }
        XCTAssertEqual(seen, [5, 6], "only events after `since` are replayed")
    }

    func testEndpointCarriesBackgroundOwner() {
        let other = PreviewCoreClient(endpoint: CoreEndpoint(port: 9, token: "t", backgroundOwner: false, version: "1"))
        XCTAssertFalse(other.endpoint.backgroundOwner)
        XCTAssertTrue(PreviewCoreClient().endpoint.backgroundOwner)
    }

    func testSandboxOnIsDerivedFromTheKindName() async throws {
        let c = try await client()
        var state = try await c.state()
        XCTAssertEqual(state.sandbox, "seatbelt")
        XCTAssertTrue(state.sandboxOn)
        c.setSandbox("none")
        state = try await c.state()
        XCTAssertFalse(state.sandboxOn)
    }

    func testApprovalRiskLabelsSeparateHighFromNormal() {
        func req(_ risk: String) -> ApprovalRequest {
            ApprovalRequest(id: "x", tool: "t", summary: "", risk: risk, detail: "", alwaysRule: nil, canAlways: false, canSession: false,
                            tainted: false, outsideSource: nil, reason: nil, sessionID: nil)
        }
        for high in ["sensitive", "sensitive-write", "DANGEROUS", "external", "egress"] { XCTAssertTrue(req(high).isHighRisk, high) }
        for normal in ["safe", "state", "write", "exec", "network", "outside"] { XCTAssertFalse(req(normal).isHighRisk, normal) }
    }

    func testMutationsRoundTrip() async throws {
        let c = try await client()
        let id = try await c.addTask(title: "New", due: nil)
        try await c.setTaskDone(id: id, done: true)
        let tasks = try await c.tasks()
        XCTAssertEqual(tasks.first { $0.id == id }?.status, "done")
        try await c.deleteTask(id: id)
        let after = try await c.tasks()
        XCTAssertNil(after.first { $0.id == id })
        do { try await c.deleteTask(id: id); XCTFail("deleting twice fails") } catch { XCTAssertEqual(error as? CoreError, .notFound) }
        let persona = try await c.setPersona(name: String(repeating: "x", count: 41), text: nil)
        XCTAssertEqual(persona.name, "Heron", "a name over 40 characters is ignored, as the core does")
    }
}
