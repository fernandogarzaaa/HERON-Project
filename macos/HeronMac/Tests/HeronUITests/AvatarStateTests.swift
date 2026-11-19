import XCTest
import HeronCore
@testable import HeronUI

/// The avatar machine driven by the core's recorded event streams (`tests/fixtures/api/events-*.jsonl`),
/// decoded by the same `CoreWire` the app uses, so the transitions of §4 are checked against what the core
/// really sends, not against hand-written events.
final class AvatarStateTests: XCTestCase {
    /// Tests/HeronUITests/<file> → HeronUITests → Tests → HeronMac → macos → repository root.
    private static let fixtures: URL = {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<5 { url.deleteLastPathComponent() }
        return url.appendingPathComponent("tests/fixtures/api", isDirectory: true)
    }()

    private func recording(_ name: String) throws -> [CoreEvent] {
        let text = try String(contentsOf: Self.fixtures.appendingPathComponent(name), encoding: .utf8)
        return try CoreWire.events(jsonLines: text)
    }

    private let start = Date(timeIntervalSince1970: 1_800_000_000)

    /// The state after each event of a recording, one second apart.
    private func replay(_ events: [CoreEvent]) -> (machine: AvatarStateMachine, states: [AvatarState]) {
        var machine = AvatarStateMachine()
        var out: [AvatarState] = []
        for (i, event) in events.enumerated() {
            machine.handle(event, at: start.addingTimeInterval(TimeInterval(i)))
            out.append(machine.state)
        }
        return (machine, out)
    }

    func testApprovalRunWalksThroughEveryState() throws {
        let events = try recording("events-approval.jsonl")
        XCTAssertEqual(events.count, 18)
        let run = replay(events)
        let states = run.states

        XCTAssertEqual(states[0], .thinking, "session")
        XCTAssertEqual(states[1], .thinking, "usage changes nothing")
        XCTAssertEqual(states[2], .awaitingApproval, "approval")
        XCTAssertEqual(states[3], .thinking, "approval_resolved: the run goes on")
        XCTAssertEqual(states[4], .thinking, "tool_start")
        XCTAssertEqual(states[5], .thinking, "tool_end")
        XCTAssertEqual(states[6], .speaking, "the first text of the reply")
        XCTAssertTrue(states[6...14].allSatisfy { $0 == .speaking }, "streamed text stays speaking")
        XCTAssertEqual(states[15], .speaking, "usage")
        XCTAssertEqual(states[16], .idle, "done")
        XCTAssertEqual(states[17], .idle, "run_end")
        XCTAssertEqual(run.machine.pendingApprovals, [:])
        XCTAssertEqual(run.machine.activeRuns, [], "the run is over")
    }

    func testTheApprovalIsWaitedOnByIDUntilItsResolution() throws {
        let events = try recording("events-approval.jsonl")
        var machine = AvatarStateMachine()
        for event in events.prefix(3) { machine.handle(event, at: start) }
        XCTAssertEqual(machine.state, .awaitingApproval)
        XCTAssertEqual(machine.pendingApprovals, ["approval-1": "run-1"])
        XCTAssertEqual(machine.activeRuns, ["run-1"])
        machine.handle(events[3], at: start)
        XCTAssertEqual(machine.pendingApprovals, [:])
    }

    func testErrorRunShowsErrorThenSettlesAfterFiveSeconds() throws {
        let events = try recording("events-error.jsonl")
        XCTAssertEqual(events.count, 3)
        var machine = AvatarStateMachine()
        machine.handle(events[0], at: start)
        XCTAssertEqual(machine.state, .thinking)
        machine.handle(events[1], at: start.addingTimeInterval(1))
        XCTAssertEqual(machine.state, .error)
        machine.handle(events[2], at: start.addingTimeInterval(1)) // run_end
        XCTAssertEqual(machine.state, .error, "run_end doesn't clear an error")

        machine.tick(at: start.addingTimeInterval(5))
        XCTAssertEqual(machine.state, .error, "4 seconds after the error")
        machine.tick(at: start.addingTimeInterval(6))
        XCTAssertEqual(machine.state, .idle, "5 seconds after the error")
    }

    func testANewSessionClearsTheError() throws {
        let error = try recording("events-error.jsonl")
        let approval = try recording("events-approval.jsonl")
        var machine = AvatarStateMachine()
        for event in error { machine.handle(event, at: start) }
        XCTAssertEqual(machine.state, .error)
        machine.handle(approval[0], at: start.addingTimeInterval(1))
        XCTAssertEqual(machine.state, .thinking)
        XCTAssertNil(machine.errorSince)
    }

    func testBothRecordingsInARowEndIdle() throws {
        var machine = AvatarStateMachine()
        for event in try recording("events-approval.jsonl") { machine.handle(event, at: start) }
        XCTAssertEqual(machine.state, .idle)
        for event in try recording("events-error.jsonl") { machine.handle(event, at: start) }
        XCTAssertEqual(machine.state, .error)
        machine.tick(at: start.addingTimeInterval(10))
        XCTAssertEqual(machine.state, .idle)
    }

    func testTheObservableModelReachesTheSameFinalStates() async throws {
        let model = await AvatarModel()
        for event in try recording("events-approval.jsonl") { await model.handle(event) }
        let state = await model.state
        XCTAssertEqual(state, .idle)
    }
}
