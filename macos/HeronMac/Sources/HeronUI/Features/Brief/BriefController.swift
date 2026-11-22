import Foundation
import HeronCore

/// What F6 sends and stores. Plain constants and values, so any code can read them.
public enum BriefSchedule {
    /// The schedule's name. Switching off deletes the schedules that carry it.
    public static let scheduleName = "Morning brief"
    /// The app setting the Settings row reads and writes.
    public static let defaultsKey = "heron.feature.morningBrief"
    /// Set once the first-launch creation has succeeded. Never cleared, so a deleted schedule stays deleted.
    public static let createdKey = "heron.morningBrief.created"
    /// The time of day, "HH:mm". `defaultTime` when unset or not a time.
    public static let timeKey = "heron.morningBrief.time"
    public static let defaultTime = "08:30"

    // MARK: The request

    /// The steps of the brief. `today_counts` answers two lines: the total, then "Today: …". The `if` compares the
    /// first line with 0, and the notice carries the second, which holds counts and fixed words only: it can appear
    /// on the lock screen. There is no `ask` and no `agent` step.
    public static var workflow: [[String: Any]] {
        [
            ["id": "counts", "do": "tool", "tool": "today_counts", "args": [String: Any]()],
            ["id": "check", "do": "if", "condition": "{{ steps.counts.out | lines | first }} > 0",
             "then": [["id": "tell", "do": "notify", "title": "Heron", "text": "{{ steps.counts.out | lines | last }}"]]],
        ]
    }

    /// The body of `POST /api/schedules`.
    public static func requestBody(time: String = defaultTime) -> [String: Any] {
        ["name": scheduleName, "when": "daily \(time)", "prompt": "Morning brief", "workflow": workflow]
    }

    /// `requestBody` as JSON, for the route.
    public static func requestData(time: String = defaultTime) throws -> Data {
        try JSONSerialization.data(withJSONObject: requestBody(time: time), options: [.sortedKeys])
    }
}

/// F6's "Morning brief" switch (heron-v1-features.md, F6). Switching it on makes one ordinary schedule on the core,
/// whose steps are fixed code: count what needs the owner today, and notify only when the count isn't zero.
/// Nothing in it calls a model, so Q6 holds nothing.
///
/// The switch ships on (QF3): the first launch after F6 lands creates the schedule, and records that in
/// `UserDefaults` so a schedule the owner deleted is not made again.
@MainActor
public final class BriefController {
    private let client: any ScheduleRoutesClientProtocol
    private let defaults: UserDefaults

    public init(client: any ScheduleRoutesClientProtocol, defaults: UserDefaults = .standard) {
        self.client = client
        self.defaults = defaults
    }

    /// The owner's time, or `defaultTime` when none is stored or it isn't "HH:mm".
    public var time: String {
        let stored = defaults.string(forKey: BriefSchedule.timeKey) ?? BriefSchedule.defaultTime
        let parts = stored.split(separator: ":", omittingEmptySubsequences: false)
        guard parts.count == 2, parts[0].count == 2, parts[1].count == 2,
              let h = Int(parts[0]), let m = Int(parts[1]), (0..<24).contains(h), (0..<60).contains(m) else { return BriefSchedule.defaultTime }
        return stored
    }

    // MARK: Switching

    /// The first launch after F6 lands: creates the schedule once and turns the setting on. A failure (the core
    /// isn't up, say) records nothing, so the next launch tries again; a success is never repeated.
    public func createOnFirstLaunch() async {
        guard !defaults.bool(forKey: BriefSchedule.createdKey) else { return }
        do {
            try await ensureSchedule()
            defaults.set(true, forKey: BriefSchedule.createdKey)
            defaults.set(true, forKey: BriefSchedule.defaultsKey)
        } catch {
            return
        }
    }

    /// The switch. On makes the schedule if the core has none under the name; off deletes every one that has it.
    /// Throws when the core refuses, and the setting is left as it was.
    public func set(_ on: Bool) async throws {
        if on {
            try await ensureSchedule()
            defaults.set(true, forKey: BriefSchedule.createdKey)
        } else {
            for ref in try await client.scheduleRefs() where ref.name == BriefSchedule.scheduleName {
                try await client.deleteSchedule(id: ref.id)
            }
        }
    }

    private func ensureSchedule() async throws {
        let existing = try await client.scheduleRefs()
        guard !existing.contains(where: { $0.name == BriefSchedule.scheduleName }) else { return }
        _ = try await client.createSchedule(jsonBody: try BriefSchedule.requestData(time: time))
    }
}

extension FeatureRow {
    /// F6's row. It names Notifications, so turning it on asks macOS for that permission once.
    public static let morningBrief = FeatureRow(
        key: "morning_brief", title: "Morning brief",
        kind: .app(defaultsKey: BriefSchedule.defaultsKey),
        permission: FeatureRow.notificationsPermission,
        settingsPane: URL(string: "x-apple.systempreferences:com.apple.preference.notifications"))
}
