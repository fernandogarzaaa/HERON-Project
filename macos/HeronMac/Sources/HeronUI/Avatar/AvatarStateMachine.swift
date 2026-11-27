import Foundation
import Observation
import HeronCore

/// The avatar's five states (heron-v1.md §4 and heron-mac-visual.md).
public enum AvatarState: String, Sendable, CaseIterable, Equatable {
    case idle, thinking, awaitingApproval, speaking, error

    /// The sidebar's one-line status.
    public var statusLine: String {
        switch self {
        case .idle: "Ready"
        case .thinking: "Thinking…"
        case .awaitingApproval: "Waiting for your approval"
        case .speaking: "Replying"
        case .error: "Something went wrong"
        }
    }

    /// The VoiceOver label's state part: "Heron: Waiting for your approval".
    public var accessibilityText: String { statusLine }
}

/// The transition table of §4, as a value type driven directly by events and a clock:
///
/// | Event | New state |
/// |---|---|
/// | `session` | `thinking` |
/// | first `text` of a run | `speaking` |
/// | `tool_start` | `thinking` |
/// | `approval` | `awaitingApproval`, until its `approval_resolved` or `approval_parked` |
/// | `done`, `cancelled` | `idle` |
/// | `error` | `error`, back to `idle` after 5 seconds or on the next `session` |
///
/// It consumes the same `CoreEvent` the protocol's `events(since:)` yields, so U4b can replay recorded streams.
public struct AvatarStateMachine: Equatable, Sendable {
    public static let errorResetDelay: TimeInterval = 5

    public private(set) var state: AvatarState = .idle
    /// When the current `error` state began.
    public private(set) var errorSince: Date?
    /// Runs that started and haven't ended, by run id (or session id when an event has no run id).
    public private(set) var activeRuns: Set<String> = []
    /// Live approvals not yet resolved or parked, with the run each belongs to.
    public private(set) var pendingApprovals: [String: String] = [:]

    public init() {}

    /// When `tick(at:)` will move an `error` back to `idle`, or nil when not in `error`.
    public var errorDeadline: Date? { errorSince.map { $0.addingTimeInterval(Self.errorResetDelay) } }

    public mutating func handle(_ event: CoreEvent, at now: Date) {
        let run = event.runID ?? event.sessionID ?? "-"
        switch event.kind {
        case .session(_, let runID):
            activeRuns.insert(runID)
            errorSince = nil
            state = pendingApprovals.isEmpty ? .thinking : .awaitingApproval
        case .text:
            // The first text of a run starts the reply. Later text stays `speaking`; after a tool call
            // (`thinking`) the next text starts it again (DESIGN-CHOICES.md).
            if state != .error && pendingApprovals.isEmpty { state = .speaking }
        case .toolStart:
            if state != .error && pendingApprovals.isEmpty { state = .thinking }
        case .approval(let request):
            pendingApprovals[request.id] = run
            state = .awaitingApproval
        case .approvalResolved(let id, _):
            resolve(id)
        case .approvalParked(let id, _):
            resolve(id)
        case .done, .cancelled:
            endRun(run)
        case .error:
            activeRuns.remove(run)
            pendingApprovals = pendingApprovals.filter { $0.value != run }
            state = .error
            errorSince = now
        case .runEnd:
            endRun(run)
        case .toolEnd, .usage, .model, .notice, .notification, .learned, .resync:
            break
        }
    }

    /// Moves `error` back to `idle` once 5 seconds have passed.
    public mutating func tick(at now: Date) {
        guard state == .error, let deadline = errorDeadline, now >= deadline else { return }
        errorSince = nil
        state = settled()
    }

    private mutating func resolve(_ id: String) {
        pendingApprovals[id] = nil
        if state == .awaitingApproval && pendingApprovals.isEmpty { state = settled() }
    }

    private mutating func endRun(_ run: String) {
        activeRuns.remove(run)
        pendingApprovals = pendingApprovals.filter { $0.value != run }
        if state != .error { state = settled() }
    }

    /// The state when nothing special is going on: waiting if something still waits, thinking while
    /// another run goes on, otherwise idle.
    private func settled() -> AvatarState {
        if !pendingApprovals.isEmpty { return .awaitingApproval }
        return activeRuns.isEmpty ? .idle : .thinking
    }
}

/// An observable wrapper that feeds events to the machine and resets `error` after 5 seconds.
/// The sleep is injectable, so tests never wait.
@MainActor
@Observable
public final class AvatarModel {
    public private(set) var machine = AvatarStateMachine()
    public var state: AvatarState { machine.state }

    @ObservationIgnored private let clock: @Sendable () -> Date
    @ObservationIgnored private let sleep: @Sendable (TimeInterval) async -> Void
    @ObservationIgnored var resetTask: Task<Void, Never>?

    public init(clock: @escaping @Sendable () -> Date = { Date() },
                sleep: @escaping @Sendable (TimeInterval) async -> Void = { try? await Task.sleep(for: .seconds($0)) }) {
        self.clock = clock
        self.sleep = sleep
    }

    public func handle(_ event: CoreEvent) {
        machine.handle(event, at: clock())
        resetTask?.cancel()
        resetTask = nil
        guard machine.state == .error else { return }
        resetTask = Task { [weak self, sleep] in
            await sleep(AvatarStateMachine.errorResetDelay)
            guard !Task.isCancelled else { return }
            self?.tick()
        }
    }

    public func tick() { machine.tick(at: clock()) }
}

/// What the avatar looks like in a state: colour, badge and motion. A pure function, so Reduce Motion
/// is tested directly (heron-mac-visual.md, Motion).
public struct AvatarLook: Equatable, Sendable {
    public enum Tone: Sendable, Equatable { case accent, waiting, danger }

    public enum Loop: Sendable, Equatable {
        case none
        /// The outer ring breathes between 1.0 and `scale`.
        case breathe(period: Double, scale: Double)
        /// Three rings ripple outward, staggered by a third of `period`.
        case ripple(period: Double, rings: Int)
        /// The core pulses between `from` and `to`, alternating.
        case pulse(period: Double, from: Double, to: Double)
    }

    public var tone: Tone
    public var loop: Loop
    /// SF Symbol name of the badge, if any.
    public var badge: String?
    /// Thinking without motion: static rings with the inner ring widened.
    public var widenedInnerRing: Bool
    /// Seconds for a change of state: 0.12 crossfade under Reduce Motion.
    public var transitionDuration: Double
    public var animates: Bool { loop != .none }

    public static func make(state: AvatarState, reduceMotion: Bool) -> AvatarLook {
        let tone: Tone = state == .awaitingApproval ? .waiting : state == .error ? .danger : .accent
        let badge: String? = switch state {
        case .awaitingApproval: "hand.raised"
        case .error: "exclamationmark.triangle"
        default: nil
        }
        var loop: Loop = switch state {
        case .idle, .awaitingApproval: .breathe(period: 6, scale: 1.06)
        case .thinking: .ripple(period: 1.6, rings: 3)
        case .speaking: .pulse(period: 0.9, from: 0.85, to: 1.1)
        case .error: .none
        }
        if reduceMotion { loop = .none }
        return AvatarLook(tone: tone, loop: loop, badge: badge,
                          widenedInnerRing: reduceMotion && state == .thinking,
                          transitionDuration: reduceMotion ? 0.12 : 0.22)
    }

    /// "Heron: Waiting for your approval".
    public static func accessibilityLabel(name: String, state: AvatarState) -> String {
        "\(name): \(state.accessibilityText)"
    }
}
