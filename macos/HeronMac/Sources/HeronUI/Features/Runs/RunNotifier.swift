import Foundation
import AppKit
import HeronCore

/// What clicking a notice opens. The identifier a notice is posted under says which, so the click needs no state.
public enum RunNoticeTarget: Equatable, Sendable {
    /// The approval sheet for this approval.
    case approval(id: String)
    /// The conversation of this run.
    case run(id: String)
    /// A notice that opens nothing in particular (a core notice: the app just comes forward).
    case none

    static let approvalPrefix = "heron.run.approval."
    static let finishedPrefix = "heron.run.finished."

    public static func approvalIdentifier(_ approvalID: String) -> String { approvalPrefix + approvalID }
    public static func finishedIdentifier(_ runID: String) -> String { finishedPrefix + runID }

    public static func parse(_ identifier: String) -> RunNoticeTarget {
        if identifier.hasPrefix(approvalPrefix) { return .approval(id: String(identifier.dropFirst(approvalPrefix.count))) }
        if identifier.hasPrefix(finishedPrefix) { return .run(id: String(identifier.dropFirst(finishedPrefix.count))) }
        return .none
    }
}

/// Posts F3's notices through `NotificationPoster` (heron-v1-features.md, "Notifications have one owner").
///
/// * a `notification` event from the core (a schedule, an automation or a suggestion) is posted always, whether
///   or not the window is frontmost: the core posts none of its own while this app asks for them;
/// * while the main window isn't frontmost, an `approval` posts "Heron needs your OK: tool", and a run's end posts
///   "Heron finished: title", except for runs whose origin is `schedule`, `automation`, `proactive` or `telegram`.
///   The first three already send a notice through the core. The fourth is the owner on the phone.
///
/// A notice has a title and a body and nothing else. It never offers approve or deny: an approval is answered only
/// in the sheet, the terminal or Telegram. The title is the owner's own words, and the body never holds content.
@MainActor
public final class RunNotifier {
    private let poster: any NotificationPoster
    private let runs: any RunsClientProtocol
    private let isEnabled: @MainActor () -> Bool
    private let windowIsFrontmost: @MainActor () -> Bool
    private var handled: Set<String> = []

    public init(poster: any NotificationPoster,
                runs: any RunsClientProtocol,
                isEnabled: @escaping @MainActor () -> Bool,
                windowIsFrontmost: @escaping @MainActor () -> Bool = { RunNotifier.mainWindowIsFrontmost() }) {
        self.poster = poster
        self.runs = runs
        self.isEnabled = isEnabled
        self.windowIsFrontmost = windowIsFrontmost
    }

    /// The app is active and one of its windows is key. `NSApp` doesn't exist in a test host, so then this is false.
    public static func mainWindowIsFrontmost() -> Bool {
        guard let app = NSApp, app.isActive else { return false }
        return app.keyWindow?.isVisible ?? false
    }

    /// Handles one event. Returns once any notice has been handed to the poster, so a test can wait for it.
    public func handle(_ event: CoreEvent) async {
        guard isEnabled() else { return }
        switch event.kind {
        case .notification(let title, let body, _):
            // Not remembered: `seq` starts over with the core, and the stream never replays an event it has yielded.
            await post(title: title, body: body, identifier: "heron.notice.\(event.seq)", once: false)
        case .approval(let request):
            guard !windowIsFrontmost() else { return }
            await post(title: "Heron needs your OK: \(request.tool)",
                       body: "Open Heron to answer. Nothing happens until you do.",
                       identifier: RunNoticeTarget.approvalIdentifier(request.id), once: false)
        case .runEnd:
            guard !windowIsFrontmost(), let runID = event.runID else { return }
            await finished(runID: runID)
        default:
            break
        }
    }

    /// The core wrote the run's record before it published `run_end`, so the list already holds the final status
    /// and the origin. If it can't be read, nothing is posted: an unknown origin might be one that already notified.
    private func finished(runID: String) async {
        let identifier = RunNoticeTarget.finishedIdentifier(runID)
        guard !handled.contains(identifier) else { return }
        guard let records = try? await runs.runs(), let record = records.first(where: { $0.runID == runID }),
              record.sendsFinishedNotice else { return }
        let title = record.title.isEmpty ? "your request" : record.title
        await post(title: "Heron finished: \(title)", body: "Open Heron to see the result.", identifier: identifier, once: true)
    }

    /// Posts a notice; with `once`, only the first time its identifier is seen (a run id is unique, so a run's end is
    /// announced once). macOS is asked for permission the first time a notice needs it, never at launch.
    private func post(title: String, body: String, identifier: String, once: Bool) async {
        if once, !handled.insert(identifier).inserted { return }
        if !(await poster.isAuthorized()) {
            guard await poster.requestAuthorization() else { return }
        }
        try? await poster.post(title: title, body: body, identifier: identifier)
    }
}
