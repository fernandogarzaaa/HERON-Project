import XCTest
import HeronCore
@testable import HeronUI

/// The §4 transition table and Reduce Motion, driven directly: events and dates go straight into the
/// machine, so nothing waits for a real 5 seconds.
final class AvatarStateMachineTests: XCTestCase {
    private let t0 = Date(timeIntervalSince1970: 1_000)

    private func event(_ kind: CoreEvent.Kind, run: String = "r1", session: String = "s1") -> CoreEvent {
        CoreEvent(seq: 1, sessionID: session, runID: run, kind: kind)
    }

    private func started(run: String = "r1") -> CoreEvent { event(.session(id: "s1", runID: run), run: run) }

    private func approval(_ id: String = "a1", run: String = "r1") -> CoreEvent {
        event(.approval(ApprovalRequest(id: id, tool: "write_file", summary: "s", risk: "write", detail: "d", alwaysRule: nil, canAlways: false,
                                        canSession: false, tainted: false, outsideSource: nil, reason: nil, sessionID: "s1")), run: run)
    }

    private func error(run: String = "r1") -> CoreEvent {
        event(.error(message: "boom", code: nil, provider: nil, version: nil), run: run)
    }

    // MARK: The §4 table

    func testStartsIdle() {
        XCTAssertEqual(AvatarStateMachine().state, .idle)
    }

    func testSessionStartsThinking() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        XCTAssertEqual(m.state, .thinking)
        XCTAssertEqual(m.activeRuns, ["r1"])
    }

    func testFirstTextSpeaksAndLaterTextKeepsSpeaking() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.handle(event(.text("Hel")), at: t0)
        XCTAssertEqual(m.state, .speaking)
        m.handle(event(.text("lo")), at: t0)
        XCTAssertEqual(m.state, .speaking)
    }

    func testToolStartThinksAgainEvenWhileSpeaking() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.handle(event(.text("Let me check.")), at: t0)
        m.handle(event(.toolStart(id: "t", name: "read_file", summary: "", risk: "safe")), at: t0)
        XCTAssertEqual(m.state, .thinking)
        m.handle(event(.toolEnd(id: "t", name: "read_file", ok: true, preview: "")), at: t0)
        XCTAssertEqual(m.state, .thinking, "tool_end changes nothing")
        m.handle(event(.text("Done.")), at: t0)
        XCTAssertEqual(m.state, .speaking, "text after a tool call speaks again")
    }

    func testApprovalWaitsUntilResolved() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.handle(approval(), at: t0)
        XCTAssertEqual(m.state, .awaitingApproval)
        m.handle(event(.text("ignored while waiting")), at: t0)
        XCTAssertEqual(m.state, .awaitingApproval, "text can't end the wait")
        m.handle(event(.toolStart(id: "t", name: "x", summary: "", risk: "safe")), at: t0)
        XCTAssertEqual(m.state, .awaitingApproval, "tool_start can't end the wait")
        m.handle(event(.approvalResolved(id: "a1", decision: "once")), at: t0)
        XCTAssertEqual(m.state, .thinking, "the run goes on after an answer")
    }

    func testApprovalParkedEndsTheWaitToo() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.handle(approval(), at: t0)
        m.handle(event(.approvalParked(id: "a1", parkedID: 7)), at: t0)
        XCTAssertEqual(m.state, .thinking)
        XCTAssertTrue(m.pendingApprovals.isEmpty)
    }

    func testResolvingAnApprovalAfterTheRunEndedGoesIdle() {
        var m = AvatarStateMachine()
        m.handle(approval(run: "bg"), at: t0)   // a background run with no session event seen
        XCTAssertEqual(m.state, .awaitingApproval)
        m.handle(event(.approvalResolved(id: "a1", decision: "deny"), run: "bg"), at: t0)
        XCTAssertEqual(m.state, .idle)
    }

    func testTwoApprovalsNeedBothAnswers() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.handle(approval("a1"), at: t0)
        m.handle(approval("a2"), at: t0)
        m.handle(event(.approvalResolved(id: "a1", decision: "once")), at: t0)
        XCTAssertEqual(m.state, .awaitingApproval)
        m.handle(event(.approvalResolved(id: "a2", decision: "once")), at: t0)
        XCTAssertEqual(m.state, .thinking)
    }

    func testDoneAndCancelledGoIdle() {
        for end in [CoreEvent.Kind.done(text: "ok", title: nil), .cancelled] {
            var m = AvatarStateMachine()
            m.handle(started(), at: t0)
            m.handle(event(.text("x")), at: t0)
            m.handle(event(end), at: t0)
            XCTAssertEqual(m.state, .idle, "\(end)")
            XCTAssertTrue(m.activeRuns.isEmpty)
        }
    }

    func testCancelWhileWaitingForApprovalGoesIdle() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.handle(approval(), at: t0)
        m.handle(event(.cancelled), at: t0)
        XCTAssertEqual(m.state, .idle)
        XCTAssertTrue(m.pendingApprovals.isEmpty, "a cancelled run no longer waits")
    }

    func testRunEndAfterDoneStaysIdle() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.handle(event(.done(text: "x", title: nil)), at: t0)
        m.handle(event(.runEnd), at: t0)
        XCTAssertEqual(m.state, .idle)
    }

    func testOtherEventsChangeNothing() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        let before = m
        for kind in [CoreEvent.Kind.usage(prompt: 1, completion: 2), .model(name: "m", why: "w"), .notice("n"), .learned(["x"]), .resync(oldest: 4)] {
            m.handle(event(kind), at: t0)
        }
        XCTAssertEqual(m, before)
    }

    // MARK: Errors

    func testErrorGoesBackToIdleAfterFiveSeconds() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.handle(error(), at: t0)
        XCTAssertEqual(m.state, .error)
        XCTAssertEqual(m.errorDeadline, t0.addingTimeInterval(5))
        m.handle(event(.runEnd), at: t0)
        XCTAssertEqual(m.state, .error, "run_end after error keeps the error showing")
        m.tick(at: t0.addingTimeInterval(4.99))
        XCTAssertEqual(m.state, .error)
        m.tick(at: t0.addingTimeInterval(5))
        XCTAssertEqual(m.state, .idle)
        XCTAssertNil(m.errorDeadline)
    }

    func testErrorEndsOnTheNextSessionEvent() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.handle(error(), at: t0)
        m.handle(started(run: "r2"), at: t0.addingTimeInterval(1))
        XCTAssertEqual(m.state, .thinking)
        XCTAssertNil(m.errorSince)
        m.tick(at: t0.addingTimeInterval(60))
        XCTAssertEqual(m.state, .thinking, "the old error's timer doesn't fire on the new run")
    }

    func testTickDoesNothingOutsideError() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.tick(at: t0.addingTimeInterval(100))
        XCTAssertEqual(m.state, .thinking)
    }

    func testErrorIgnoresStrayTextAndToolEvents() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.handle(error(), at: t0)
        m.handle(event(.text("late")), at: t0)
        m.handle(event(.toolStart(id: "t", name: "x", summary: "", risk: "safe")), at: t0)
        XCTAssertEqual(m.state, .error)
    }

    func testErrorAfterApprovalClearsTheWait() {
        var m = AvatarStateMachine()
        m.handle(started(), at: t0)
        m.handle(approval(), at: t0)
        m.handle(error(), at: t0)
        XCTAssertTrue(m.pendingApprovals.isEmpty)
        m.tick(at: t0.addingTimeInterval(5))
        XCTAssertEqual(m.state, .idle)
    }

    func testErrorKeepsThinkingWhenAnotherRunContinues() {
        var m = AvatarStateMachine()
        m.handle(started(run: "r1"), at: t0)
        m.handle(started(run: "r2"), at: t0)
        m.handle(error(run: "r1"), at: t0)
        m.tick(at: t0.addingTimeInterval(5))
        XCTAssertEqual(m.state, .thinking, "r2 is still running")
    }

    func testOneRunEndingDoesNotIdleAnotherRun() {
        var m = AvatarStateMachine()
        m.handle(started(run: "r1"), at: t0)
        m.handle(started(run: "r2"), at: t0)
        m.handle(event(.done(text: "", title: nil), run: "r1"), at: t0)
        XCTAssertEqual(m.state, .thinking)
        m.handle(event(.done(text: "", title: nil), run: "r2"), at: t0)
        XCTAssertEqual(m.state, .idle)
    }

    // MARK: The observable wrapper, with injected time

    @MainActor
    func testModelResetsErrorWithoutWaitingFiveRealSeconds() async {
        final class Clock: @unchecked Sendable { var now = Date(timeIntervalSince1970: 5_000) }
        let clock = Clock()
        let slept = Slept()
        let model = AvatarModel(clock: { clock.now }, sleep: { secs in
            await slept.record(secs)
            clock.now = clock.now.addingTimeInterval(secs)
        })
        model.handle(error())
        XCTAssertEqual(model.state, .error)
        await model.resetTask?.value
        let seconds = await slept.values
        XCTAssertEqual(seconds, [5])
        XCTAssertEqual(model.state, .idle)
    }

    @MainActor
    func testModelCancelsTheResetWhenANewSessionStarts() async {
        final class Clock: @unchecked Sendable { var now = Date(timeIntervalSince1970: 5_000) }
        let clock = Clock()
        let model = AvatarModel(clock: { clock.now }, sleep: { _ in try? await Task.sleep(for: .seconds(60)) })
        model.handle(error())
        let pending = model.resetTask
        XCTAssertNotNil(pending)
        model.handle(started(run: "r2"))
        XCTAssertEqual(model.state, .thinking)
        XCTAssertNil(model.resetTask)
        XCTAssertTrue(pending?.isCancelled ?? false)
    }

    // MARK: Look and Reduce Motion

    func testEveryStateHasItsColourAndBadge() {
        let table: [(AvatarState, AvatarLook.Tone, String?)] = [
            (.idle, .accent, nil), (.thinking, .accent, nil), (.speaking, .accent, nil),
            (.awaitingApproval, .waiting, "hand.raised"), (.error, .danger, "exclamationmark.triangle"),
        ]
        for (state, tone, badge) in table {
            for reduce in [false, true] {
                let look = AvatarLook.make(state: state, reduceMotion: reduce)
                XCTAssertEqual(look.tone, tone, "\(state) reduce=\(reduce)")
                XCTAssertEqual(look.badge, badge, "colour and badge carry the state with or without motion")
            }
        }
    }

    func testLoopsMatchTheMotionTable() {
        XCTAssertEqual(AvatarLook.make(state: .idle, reduceMotion: false).loop, .breathe(period: 6, scale: 1.06))
        XCTAssertEqual(AvatarLook.make(state: .awaitingApproval, reduceMotion: false).loop, .breathe(period: 6, scale: 1.06))
        XCTAssertEqual(AvatarLook.make(state: .thinking, reduceMotion: false).loop, .ripple(period: 1.6, rings: 3))
        XCTAssertEqual(AvatarLook.make(state: .speaking, reduceMotion: false).loop, .pulse(period: 0.9, from: 0.85, to: 1.1))
        XCTAssertEqual(AvatarLook.make(state: .error, reduceMotion: false).loop, .none)
    }

    func testReduceMotionStopsEveryLoopAndCrossfadesIn120ms() {
        for state in AvatarState.allCases {
            let still = AvatarLook.make(state: state, reduceMotion: true)
            XCTAssertEqual(still.loop, .none, "\(state)")
            XCTAssertFalse(still.animates)
            XCTAssertEqual(still.transitionDuration, 0.12, accuracy: 0.0001)
            XCTAssertEqual(still.widenedInnerRing, state == .thinking, "thinking is shown by a widened inner ring")
            let moving = AvatarLook.make(state: state, reduceMotion: false)
            XCTAssertFalse(moving.widenedInnerRing)
            XCTAssertEqual(moving.transitionDuration, 0.22, accuracy: 0.0001)
            XCTAssertEqual(moving.animates, state != .error)
        }
    }

    func testVoiceOverLabelNamesTheState() {
        XCTAssertEqual(AvatarLook.accessibilityLabel(name: "Heron", state: .awaitingApproval), "Heron: Waiting for your approval")
        XCTAssertEqual(AvatarLook.accessibilityLabel(name: "Wren", state: .idle), "Wren: Ready")
        XCTAssertEqual(AvatarState.speaking.statusLine, "Replying")
        XCTAssertEqual(AvatarState.thinking.statusLine, "Thinking…")
        XCTAssertEqual(AvatarState.error.statusLine, "Something went wrong")
    }
}

private actor Slept {
    var values: [Double] = []
    func record(_ s: Double) { values.append(s) }
}
