import Foundation
import HeronCore

/// What F6 sends and stores. Plain constants and values, so any code can read them.
public enum BriefSchedule {
    /// The schedule's name. Only a label: the app never finds its schedule by this name, because the owner may have
    /// an automation of their own under it.
    public static let scheduleName = "Morning brief"
    /// The app setting the Settings row reads and writes.
    public static let defaultsKey = "heron.feature.morningBrief"
    /// Set once the first-launch creation has succeeded. Never cleared, so a deleted schedule stays deleted.
    public static let createdKey = "heron.morningBrief.created"
    /// The time of day, "HH:mm". `defaultTime` when unset or not a time.
    public static let timeKey = "heron.morningBrief.time"
    /// The id of the schedule this app made. Switching off and changing the time touch that id and no other.
    public static let scheduleIDKey = "heron.morningBrief.scheduleID"
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

    /// Today at "HH:mm" in the owner's calendar, for the time picker. Today at midnight when it isn't a time.
    public static func date(fromTime time: String) -> Date {
        let parts = time.split(separator: ":").compactMap { Int($0) }
        let cal = Calendar.current
        return cal.date(bySettingHour: parts.count == 2 ? parts[0] : 0, minute: parts.count == 2 ? parts[1] : 0,
                        second: 0, of: Date()) ?? Date()
    }

    /// The picker's date as "HH:mm".
    public static func time(from date: Date) -> String {
        let c = Calendar.current.dateComponents([.hour, .minute], from: date)
        return String(format: "%02d:%02d", c.hour ?? 0, c.minute ?? 0)
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
        return BriefController.isTime(stored) ? stored : BriefSchedule.defaultTime
    }

    static func isTime(_ text: String) -> Bool {
        let parts = text.split(separator: ":", omittingEmptySubsequences: false)
        guard parts.count == 2, parts[0].count == 2, parts[1].count == 2,
              let h = Int(parts[0]), let m = Int(parts[1]) else { return false }
        return (0..<24).contains(h) && (0..<60).contains(m)
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

    /// The id of the schedule this app made, or nil when it has made none (or switched it off).
    private var storedID: Int? { defaults.object(forKey: BriefSchedule.scheduleIDKey) as? Int }

    /// The switch. On makes the app's schedule unless the one it made still exists; off deletes that one, by its
    /// stored id, and never a schedule the owner made. Throws when the core refuses, and the setting is left as it was.
    public func set(_ on: Bool) async throws {
        if on {
            try await ensureSchedule()
            defaults.set(true, forKey: BriefSchedule.createdKey)
        } else {
            guard let id = storedID else { return }
            if try await client.scheduleRefs().contains(where: { $0.id == id }) {
                try await client.deleteSchedule(id: id)
            }
            defaults.removeObject(forKey: BriefSchedule.scheduleIDKey)
        }
    }

    /// The owner picks the time ("HH:mm"). The core can't change a schedule's time, so the app's schedule is made again
    /// under the new one: the new schedule first, then the old one is deleted, so a refusal never leaves the owner with
    /// none. When the app has no schedule now (the switch is off, or the owner deleted it) the time is only saved.
    /// Throws when the time isn't "HH:mm" or the core refuses; the stored time and schedule are then as they were.
    public func setTime(_ newTime: String) async throws {
        guard BriefController.isTime(newTime) else { throw CoreError.failed("\(newTime) is not a time of day.") }
        if let id = storedID, try await client.scheduleRefs().contains(where: { $0.id == id }) {
            let newID = try await client.createSchedule(jsonBody: try BriefSchedule.requestData(time: newTime))
            do {
                try await client.deleteSchedule(id: id)
            } catch {
                try? await client.deleteSchedule(id: newID)
                throw error
            }
            defaults.set(newID, forKey: BriefSchedule.scheduleIDKey)
        }
        defaults.set(newTime, forKey: BriefSchedule.timeKey)
    }

    /// Makes the schedule unless the one this app made is still there. Never adopts a schedule by its name.
    private func ensureSchedule() async throws {
        if let id = storedID, try await client.scheduleRefs().contains(where: { $0.id == id }) { return }
        let id = try await client.createSchedule(jsonBody: try BriefSchedule.requestData(time: time))
        defaults.set(id, forKey: BriefSchedule.scheduleIDKey)
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
