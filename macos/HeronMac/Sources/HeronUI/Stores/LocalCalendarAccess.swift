import Foundation
import EventKit
import HeronCore

// MARK: The Mac's Calendar app (EventKit)

public enum CalendarAuthorization: Sendable, Equatable {
    case notDetermined, granted, denied
}

/// Read access to the calendars in the Mac's Calendar app. The store talks to this protocol so tests never
/// reach EventKit or the macOS permission prompt.
public protocol LocalCalendarAccess: Sendable {
    /// The current permission, without asking.
    func authorization() -> CalendarAuthorization
    /// Shows the macOS permission prompt when the user hasn't answered yet, and returns the answer.
    func requestAccess() async -> CalendarAuthorization
    /// Events from every calendar in the Calendar app (Google and iCloud accounts added there included), soonest first.
    func events() async throws -> [LocalCalendarEvent]
}

/// EventKit, read-only. Needs `NSCalendarsFullAccessUsageDescription` in Info.plist and, in a hardened build,
/// the calendars entitlement (`packaging/macos/Heron.entitlements`).
public struct SystemCalendarAccess: LocalCalendarAccess {
    /// A little history, so "what did I have on Tuesday" works, and a few months ahead.
    static let daysBack = 14
    static let daysAhead = 120
    /// The core refuses more than this in one sync.
    static let maxEvents = 2000

    public init() {}

    static func map(_ status: EKAuthorizationStatus) -> CalendarAuthorization {
        switch status {
        case .fullAccess: .granted
        case .notDetermined: .notDetermined
        default: .denied   // denied, restricted, or write-only: Heron can't read anything
        }
    }

    public func authorization() -> CalendarAuthorization { Self.map(EKEventStore.authorizationStatus(for: .event)) }

    public func requestAccess() async -> CalendarAuthorization {
        _ = try? await EKEventStore().requestFullAccessToEvents()
        return authorization()
    }

    public func events() async throws -> [LocalCalendarEvent] {
        await Task.detached(priority: .utility) {
            let store = EKEventStore()
            let calendar = Calendar.current
            let now = Date()
            let start = calendar.date(byAdding: .day, value: -Self.daysBack, to: now) ?? now
            let end = calendar.date(byAdding: .day, value: Self.daysAhead, to: now) ?? now
            let predicate = store.predicateForEvents(withStart: start, end: end, calendars: nil)
            return store.events(matching: predicate)
                .sorted { $0.startDate < $1.startDate }
                .prefix(Self.maxEvents)
                .map { e in
                    let title = e.title ?? ""
                    return LocalCalendarEvent(uid: e.calendarItemExternalIdentifier ?? e.eventIdentifier ?? UUID().uuidString,
                                       title: title.isEmpty ? "(No title)" : title,
                                       start: Int(e.startDate.timeIntervalSince1970),
                                       end: e.endDate.map { Int($0.timeIntervalSince1970) },
                                       allDay: e.isAllDay,
                                       location: e.location ?? "")
                }
        }.value
    }
}
