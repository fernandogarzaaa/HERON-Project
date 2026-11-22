import Foundation
import Observation
import HeronCore

/// The key of the "Runs and notifications" switch, apart from `RunsStore` so the Features rows (which aren't on the
/// main actor) can name it.
public enum RunsSetting {
    /// The `UserDefaults` key. On unless the owner turned it off (QF3).
    public static let defaultsKey = "heron.feature.runs"
}

/// The Runs screen's state, and the number the menu bar badge shows (heron-v1-features.md, F3).
///
/// The core keeps one record per run (`GET /api/runs`). This store loads that list and follows the event stream:
/// each event that changes a run's status updates the row at once and then reloads, so the core's own record is
/// what the screen settles on.
@MainActor
@Observable
public final class RunsStore {
    public static let defaultsKey = RunsSetting.defaultsKey

    public let client: any RunsClientProtocol
    public private(set) var runs: [RunRecord] = []
    public private(set) var loadState: LoadState = .idle
    /// Whether the switch is on. Off: no badge, no screen, and the app doesn't ask the core to hand over its notices.
    public private(set) var enabled: Bool

    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private var generation = 0

    public init(client: any RunsClientProtocol, defaults: UserDefaults = .standard) {
        defaults.register(defaults: [RunsSetting.defaultsKey: true])
        self.client = client
        self.defaults = defaults
        self.enabled = defaults.bool(forKey: RunsSetting.defaultsKey)
    }

    /// Reads the switch again. Returns true when it changed.
    @discardableResult
    public func refreshEnabled() -> Bool {
        let now = defaults.bool(forKey: Self.defaultsKey)
        guard now != enabled else { return false }
        enabled = now
        return true
    }

    // MARK: Reading

    /// Runs that haven't ended: running, or waiting for an approval.
    public var inProgress: [RunRecord] { runs.filter { !$0.isFinished } }
    public var finished: [RunRecord] { runs.filter { $0.isFinished } }
    /// Runs in `waiting`.
    public var waitingCount: Int { runs.filter { $0.isWaiting && !$0.isFinished }.count }
    /// What the menu bar badge shows: the runs waiting for you, and nothing while the switch is off.
    public var badgeCount: Int { enabled ? waitingCount : 0 }

    // MARK: Loading

    public func load() async {
        generation += 1
        let mine = generation
        if runs.isEmpty { loadState = .loading }
        do {
            let loaded = try await client.runs()
            guard mine == generation else { return }
            runs = loaded
            loadState = .loaded
        } catch {
            guard mine == generation else { return }
            loadState = .failed(describe(error))
        }
    }

    // MARK: Events

    /// Follows one event from the bus. Ignored while the switch is off.
    public func handle(_ event: CoreEvent) {
        guard enabled else { return }
        switch event.kind {
        case .approval:
            setStatus("waiting", forRun: event.runID)
            reload()
        case .approvalResolved, .approvalParked:
            setStatus("running", forRun: event.runID)
            reload()
        case .session, .runEnd, .resync:
            reload()
        default:
            break
        }
    }

    private func setStatus(_ status: String, forRun runID: String?) {
        guard let runID, let i = runs.firstIndex(where: { $0.runID == runID }), !runs[i].isFinished else { return }
        runs[i].status = status
    }

    private func reload() {
        Task { await load() }
    }

    // MARK: Words

    /// Where a run came from, in the owner's terms.
    public static func originPhrase(_ origin: String) -> String {
        switch origin {
        case "user": "You asked"
        case "telegram": "From Telegram"
        case "schedule": "A schedule"
        case "automation": "An automation"
        case "proactive": "Heron's own idea"
        default: "Another source"
        }
    }

    /// The status tag's words. Colour is never the only signal.
    public static func statusLabel(_ status: String) -> String {
        switch status {
        case "running": "Running"
        case "waiting": "Waiting"
        case "done": "Done"
        case "error": "Stopped by an error"
        case "cancelled": "Cancelled"
        default: status.capitalized
        }
    }

    /// "Waiting for you · 3 tool calls · 2 min ago".
    public static func detailLine(_ run: RunRecord, now: Date = Date()) -> String {
        let calls = run.toolCalls == 1 ? "1 tool call" : "\(run.toolCalls) tool calls"
        return "\(originPhrase(run.origin)) · \(calls) · \(relative(run.ended ?? run.started, now: now))"
    }

    private static func relative(_ date: Date, now: Date) -> String {
        if abs(now.timeIntervalSince(date)) < 60 { return "just now" }
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .short
        formatter.locale = Locale(identifier: "en_US_POSIX")
        return formatter.localizedString(for: date, relativeTo: now)
    }
}
