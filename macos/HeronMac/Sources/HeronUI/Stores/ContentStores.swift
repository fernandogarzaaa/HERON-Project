import Foundation
import Observation
import HeronCore

// MARK: Today

/// The Today screen's data and the sentences built from it. The summary sentence comes from counts in
/// `/api/today`, never from the model.
@MainActor
@Observable
public final class TodayStore {
    public let client: any CoreClientProtocol
    public private(set) var summary: TodaySummary?
    public private(set) var loadState: LoadState = .idle

    /// The first name from the Mac's full name; empty when unknown.
    public var userFirstName: String

    public init(client: any CoreClientProtocol, userFirstName: String? = nil) {
        self.client = client
        self.userFirstName = userFirstName ?? Self.systemFirstName()
    }

    public static func systemFirstName() -> String {
        NSFullUserName().split(separator: " ").first.map(String.init) ?? ""
    }

    public func load() async {
        loadState = .loading
        do { summary = try await client.today(); loadState = .loaded } catch { loadState = .failed(describe(error)) }
    }

    /// The prompt "Prep my day" sends (heron-mac-visual.md, D6).
    public static let prepPrompt = "Prep my day. Look at today's calendar, my open and overdue tasks, and anything waiting for me in the inbox. Then give me: the three things that matter most today and why, a suggested order for the day around my events, and anything I should prepare before a meeting. Keep it short. Where it would help, draft replies to emails that need one and add tasks for things I should do, but ask me before saving any of them. Never send, delete or schedule anything."

    /// "Prep · Tue 18 Nov".
    public static func prepTitle(now: Date) -> String { "Prep · " + format("EEE d MMM", now) }

    public func greeting(now: Date, calendar: Calendar = .current) -> String {
        let hour = calendar.component(.hour, from: now)
        let part = hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening"
        return userFirstName.isEmpty ? "\(part)." : "\(part), \(userFirstName)."
    }

    /// "Tuesday 18 November. Three events, two tasks due, and one thing waiting for you."
    public func subtitle(now: Date) -> String {
        guard let s = summary else { return Self.format("EEEE d MMMM", now) + "." }
        return Self.format("EEEE d MMMM", now) + ". " + Self.summarySentence(events: s.events.count, dueTasks: s.dueTasks.count, waiting: s.waiting)
    }

    static func summarySentence(events: Int, dueTasks: Int, waiting: Int) -> String {
        var parts: [String] = []
        if events > 0 { parts.append("\(words(events)) event\(events == 1 ? "" : "s")") }
        if dueTasks > 0 { parts.append("\(words(dueTasks)) task\(dueTasks == 1 ? "" : "s") due") }
        if waiting > 0 { parts.append("\(words(waiting)) \(waiting == 1 ? "thing" : "things") waiting for you") }
        let joined: String
        switch parts.count {
        case 0: return "Nothing on the calendar, nothing due, and nothing waiting for you."
        case 1: joined = parts[0]
        case 2: joined = parts[0] + " and " + parts[1]
        default: joined = parts.dropLast().joined(separator: ", ") + ", and " + parts.last!
        }
        return joined.prefix(1).uppercased() + joined.dropFirst() + "."
    }

    private static func words(_ n: Int) -> String {
        let w = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"]
        return n < w.count ? w[n] : String(n)
    }

    static func format(_ pattern: String, _ date: Date) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = pattern
        return f.string(from: date)
    }
}

// MARK: Inbox

/// One Inbox destination fed by three sources: parked approvals (the Approvals store), suggestions and
/// delivered items.
@MainActor
@Observable
public final class InboxStore {
    public enum Filter: String, CaseIterable, Sendable { case all = "All", waiting = "Waiting", delivered = "Delivered" }
    public enum Selection: Hashable, Sendable {
        case parked(Int), suggestion(Int), delivered(Int), live(String)
    }

    public let client: any CoreClientProtocol
    public let approvals: ApprovalsStore
    public private(set) var items: [InboxItem] = []
    public private(set) var suggestions: [Suggestion] = []
    public var filter: Filter = .all
    public var selection: Selection?
    public private(set) var loadState: LoadState = .idle

    public init(client: any CoreClientProtocol, approvals: ApprovalsStore) {
        self.client = client
        self.approvals = approvals
    }

    public var unreadCount: Int { items.filter { !$0.read }.count + suggestions.filter { $0.status == "new" }.count }
    public var showsWaiting: Bool { filter != .delivered }
    public var showsDelivered: Bool { filter != .waiting }
    public var isEmpty: Bool { approvals.waitingCount == 0 && suggestions.isEmpty && items.isEmpty }

    public func load() async {
        loadState = .loading
        await approvals.load()
        do {
            items = try await client.inbox()
            suggestions = try await client.suggestions().filter { $0.status == "new" || $0.status == "prepared" }
            loadState = .loaded
        } catch { loadState = .failed(describe(error)) }
    }

    public func markAllRead() async {
        try? await client.markInboxRead()
        items = items.map { var i = $0; i.read = true; return i }
    }

    public func answer(_ s: Suggestion, _ answer: SuggestionAnswer) async {
        try? await client.answerSuggestion(id: s.id, answer: answer)
        suggestions.removeAll { $0.id == s.id }
    }
}

// MARK: Memory

@MainActor
@Observable
public final class MemoryStore {
    public enum Filter: String, CaseIterable, Sendable {
        case all = "All", toCheck = "To check", aboutMe = "About me", people = "People", projects = "Projects", preferences = "Preferences"
    }

    public let client: any CoreClientProtocol
    public private(set) var memories: [Memory] = []
    public var filter: Filter = .all
    public var query = ""
    public private(set) var loadState: LoadState = .idle
    /// The memory just forgotten, so the view can offer Undo for 5 seconds.
    public private(set) var lastForgotten: Memory?

    public init(client: any CoreClientProtocol) { self.client = client }

    public func load() async {
        loadState = .loading
        do { memories = try await client.memories(query: query.isEmpty ? nil : query); loadState = .loaded } catch { loadState = .failed(describe(error)) }
    }

    public var visible: [Memory] {
        memories.filter { m in
            switch filter {
            case .all: true
            case .toCheck: !m.confirmed
            case .aboutMe: m.kind == "fact"
            case .people: m.kind == "person"
            case .projects: m.kind == "project"
            case .preferences: m.kind == "preference"
            }
        }
    }
    public var toCheck: [Memory] { visible.filter { !$0.confirmed } }
    public var confirmed: [Memory] { visible.filter { $0.confirmed } }
    public var unconfirmedCount: Int { memories.filter { !$0.confirmed }.count }
    public var isFirstUse: Bool { memories.isEmpty && query.isEmpty && loadState == .loaded }
    public var noResults: Bool { visible.isEmpty && !query.isEmpty && loadState == .loaded }

    public func confirm(_ m: Memory) async {
        try? await client.confirmMemory(id: m.id)
        if let i = memories.firstIndex(where: { $0.id == m.id }) { memories[i].confirmed = true }
    }

    public func forget(_ m: Memory) async {
        do { try await client.deleteMemory(id: m.id); memories.removeAll { $0.id == m.id }; lastForgotten = m } catch {}
    }

    public func undoForget() async {
        guard let m = lastForgotten else { return }
        lastForgotten = nil
        await restore(m, content: m.content)
        await load()
    }

    public func clearUndo() { lastForgotten = nil }

    /// The core has no update route, so an edit is a forget plus an add (DESIGN-CHOICES.md).
    public func edit(_ m: Memory, content: String) async {
        let text = content.trimmingCharacters(in: .whitespacesAndNewlines)
        guard text.count >= 3, text != m.content else { return }
        // A save that only changes case or punctuation updates `m` itself; the old row goes only
        // once a different one holds the new words.
        if let id = await restore(m, content: text), id != m.id { try? await client.deleteMemory(id: m.id) }
        await load()
    }

    /// Saves `content` with `m`'s source and standing, so a fact from outside, or one still waiting
    /// for the owner, comes back as it was.
    @discardableResult
    private func restore(_ m: Memory, content: String) async -> Int? {
        guard let id = try? await client.addMemory(content: content, kind: m.kind, source: m.source, confirmed: m.confirmed) else { return nil }
        if m.confirmed { try? await client.confirmMemory(id: id) }
        return id
    }

    public func add(_ content: String, kind: String = "fact") async {
        let text = content.trimmingCharacters(in: .whitespacesAndNewlines)
        guard text.count >= 3 else { return }
        _ = try? await client.addMemory(content: text, kind: kind)
        await load()
    }

    public func clearSearch() async { query = ""; await load() }
}

// MARK: Tasks

public enum DueTag: Equatable, Sendable {
    case overdue, today, weekday(String), none

    /// `due` is the core's `YYYY-MM-DD` (or longer) string.
    public static func make(due: String?, now: Date, calendar: Calendar = .current) -> DueTag {
        guard let due, due.count >= 10 else { return .none }
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd"
        f.timeZone = calendar.timeZone
        guard let day = f.date(from: String(due.prefix(10))) else { return .none }
        let a = calendar.startOfDay(for: day), b = calendar.startOfDay(for: now)
        if a < b { return .overdue }
        if a == b { return .today }
        let out = DateFormatter()
        out.locale = Locale(identifier: "en_US_POSIX")
        out.dateFormat = "EEE"
        out.timeZone = calendar.timeZone
        return .weekday(out.string(from: day))
    }

    public var title: String {
        switch self {
        case .overdue: "Overdue"
        case .today: "Today"
        case .weekday(let d): d
        case .none: "No date"
        }
    }
}

@MainActor
@Observable
public final class TasksStore {
    public let client: any CoreClientProtocol
    public private(set) var tasks: [TaskItem] = []
    public private(set) var loadState: LoadState = .idle
    public var newTitle = ""
    @ObservationIgnored private let now: () -> Date

    public init(client: any CoreClientProtocol, now: @escaping () -> Date = Date.init) {
        self.client = client
        self.now = now
    }

    public var yours: [TaskItem] { tasks.filter { $0.status == "open" && $0.owner != "heron" } }
    public var heronDoing: [TaskItem] { tasks.filter { $0.status == "open" && $0.owner == "heron" } }
    /// Done in the last 7 days. A task with no completion time (the core doesn't send one yet) counts.
    public var doneThisWeek: [TaskItem] {
        tasks.filter { $0.status == "done" && ($0.completedAt.map { now().timeIntervalSince($0) < 7 * 86_400 } ?? true) }
    }
    public var openCount: Int { yours.count }

    public func load() async {
        loadState = .loading
        do { tasks = try await client.tasks(); loadState = .loaded } catch { loadState = .failed(describe(error)) }
    }

    public func add(due: String? = nil) async {
        let title = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty else { return }
        newTitle = ""
        _ = try? await client.addTask(title: title, due: due)
        await load()
    }

    public func toggle(_ task: TaskItem) async {
        try? await client.setTaskDone(id: task.id, done: task.status != "done")
        await load()
    }
}

// MARK: Schedules

@MainActor
@Observable
public final class SchedulesStore {
    public let client: any CoreClientProtocol
    public private(set) var schedules: [ScheduleItem] = []
    public private(set) var loadState: LoadState = .idle

    public init(client: any CoreClientProtocol) { self.client = client }

    public var active: [ScheduleItem] { schedules.filter(\.enabled) }
    public var held: [ScheduleItem] { schedules.filter { !$0.enabled } }

    public func load() async {
        loadState = .loading
        do { schedules = try await client.schedules(); loadState = .loaded } catch { loadState = .failed(describe(error)) }
    }

    public func runNow(_ s: ScheduleItem) async { try? await client.runSchedule(id: s.id); await load() }
    public func setEnabled(_ s: ScheduleItem, _ enabled: Bool) async { try? await client.setScheduleEnabled(id: s.id, enabled: enabled); await load() }
}

// MARK: Activity

@MainActor
@Observable
public final class ActivityStore {
    public enum Filter: String, CaseIterable, Sendable { case all = "All", changes = "Changes", refused = "Refused" }

    public let client: any CoreClientProtocol
    public private(set) var log = AuditLog(entries: [], intact: true)
    public var filter: Filter = .all
    public private(set) var loadState: LoadState = .idle

    public init(client: any CoreClientProtocol) { self.client = client }

    public var entries: [AuditEntry] {
        log.entries.filter { e in
            switch filter {
            case .all: true
            case .changes: e.isChange && !e.isRefused
            case .refused: e.isRefused
            }
        }
    }

    public func load() async {
        loadState = .loading
        do { log = try await client.audit(); loadState = .loaded } catch { loadState = .failed(describe(error)) }
    }

    /// "Wrote ~/Work/drafts/northwind-reply.md".
    public static func sentence(_ e: AuditEntry) -> String {
        let verb: String = switch e.tool {
        case "write_file": "Wrote"
        case "read_file": "Read"
        case "list_dir": "Listed"
        case "search_files", "find_files": "Searched"
        case "shell": "Ran"
        case "send_email": "Sent an email:"
        case "web_search": "Searched the web for"
        default: "Used \(e.tool):"
        }
        return e.args.isEmpty ? verb : "\(verb) \(e.args)"
    }

    /// Who allowed it: allowed by mode, allowed by grant, approved or refused.
    public static func decisionLabel(_ e: AuditEntry) -> String {
        switch e.decision {
        case "allowed": "Allowed by mode"
        case "approved-always": "Allowed by grant"
        case "approved", "approved-once", "approved-later": "Approved"
        case "approved-for-conversation": "Approved for the conversation"
        case "refused", "denied": "Refused"
        default: e.decision.capitalized
        }
    }
}
