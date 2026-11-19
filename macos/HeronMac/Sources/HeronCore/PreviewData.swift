import Foundation

struct PreviewSession {
    var summary: SessionSummary
    var detail: SessionDetail

    init(id: String, title: String, now: Date) {
        summary = SessionSummary(id: id, title: title, updatedAt: now, messages: 0)
        detail = SessionDetail(id: id, title: title, messages: [], busy: false, interrupted: false, tainted: false)
    }

    mutating func append(role: String, content: String, now: Date) {
        detail.messages.append(SessionMessage(id: detail.messages.count, role: role, content: content, toolCalls: []))
        summary.messages = detail.messages.count
        summary.updatedAt = now
    }
}

/// Long demo content, so overflow can be seen and snapshot-tested (heron-v1.md item 41b): a long risk list,
/// long tool output, long approval details and a long transcript. Nothing here is shown in the running app.
public enum PreviewDemo {
    /// A risk list far taller than the 230 pt risk box, so the box scrolls.
    public static let longRiskNotice = RiskNotice(version: 3, text: longRiskText, acceptedVersion: nil)

    /// A risk list that fits in the box with room to spare, so the box is as tall as its text.
    public static let shortRiskNotice = RiskNotice(
        version: 3,
        text: "**C35-8. Heron can act on your files and accounts.** It reads and changes files in your working folder when you allow it.",
        acceptedVersion: nil)

    public static let longRiskText: String = (1...14).map { n in
        "**R-\(n). Risk number \(n).** This is one entry of a long risk list. It says what can go wrong, why Heron can't prevent it completely, and what Heron does to reduce it, in a few plain sentences so the entry wraps over more than one line in the box."
    }.joined(separator: "\n\n")

    /// What a command printed: dozens of lines, as a tool's preview in the transcript.
    public static let longToolOutput: String = (1...40).map { n in
        "src/module\(n)/handler.swift:\(n * 7): note: processed item \(n) of 40 and wrote the result to ~/Work/out/item-\(n).json"
    }.joined(separator: "\n")

    /// An action text far taller than the approval sheet allows, so "Show details" scrolls.
    public static let longApprovalDetail: String = (1...30).map { n in
        "To: person\(n)@northwind.example   Subject: Friday summary, part \(n)"
    }.joined(separator: "\n")

    /// A delivered Inbox item's text, far taller than the detail pane, so the pane scrolls.
    public static let longInboxBody: String = (1...30).map { n in
        "Result \(n): the Northwind account changed in a way that needs a decision, and Heron wrote down what it found and what it suggests, in a few plain sentences so the entry wraps over more than one line."
    }.joined(separator: "\n\n")

    /// A short exchange whose reply holds `longToolOutput`: the output is longer than the transcript is tall.
    static func toolOutputSession(id: String, now: Date) -> PreviewSession {
        var s = PreviewSession(id: id, title: "Build output", now: now)
        s.append(role: "user", content: "Run the build and show me what it printed.", now: now)
        s.detail.messages.append(SessionMessage(
            id: s.detail.messages.count, role: "assistant", content: "The build finished. Here is what it printed.",
            toolCalls: [ToolCallInfo(id: "t-out", name: "shell", summary: longToolOutput, ok: true, durationMs: 1_240, untrusted: false)]))
        s.summary.messages = s.detail.messages.count
        return s
    }

    /// A conversation of 16 messages, with the long tool output in one reply.
    static func longSession(id: String, now: Date) -> PreviewSession {
        var s = PreviewSession(id: id, title: "Quarterly review", now: now)
        for n in 1...8 {
            s.append(role: "user", content: "Question \(n): what changed in the Northwind account this quarter, and what should I do about item \(n)?", now: now)
            var reply = SessionMessage(
                id: s.detail.messages.count, role: "assistant",
                content: "Answer \(n): the account grew, one invoice is late, and item \(n) needs a decision from you before Friday. I checked the notes and the calendar to be sure, and nothing else is blocked.",
                toolCalls: [])
            if n == 4 {
                reply.toolCalls = [ToolCallInfo(id: "t-long", name: "shell", summary: longToolOutput, ok: true, durationMs: 840, untrusted: false)]
            }
            s.detail.messages.append(reply)
            s.summary.messages = s.detail.messages.count
        }
        return s
    }
}

/// The sample data behind `PreviewCoreClient`: the content of the mockups in docs/designs/heron-mac-visual/.
struct PreviewData {
    var sessions: [String: PreviewSession] = [:]
    var mode: PermissionMode = .ask
    var paused = false
    var onboarded = false
    var sandbox = "seatbelt"
    var workspace = "~/Work"
    var activeModel = "Claude Code"
    var persona = Persona(name: "Heron", text: "Warm, brief and practical.", isDefault: true, maxCharacters: 2000)
    var riskNotice = RiskNotice(version: 3, text: PreviewData.riskText, acceptedVersion: nil)
    var events: [CalendarEvent] = []
    var inbox: [InboxItem] = []
    var suggestions: [Suggestion] = []
    var parked: [ParkedApproval] = []
    var memories: [Memory] = []
    var tasks: [TaskItem] = []
    var schedules: [ScheduleItem] = []
    var audit: [AuditEntry] = []
    var providers: [ProviderStatus] = []
    var grants: [Grant] = []
    var ruleNotices: [RuleNotice] = []
    var connections: [ConnectionInfo] = []
    var usage = UsageReport(days: [], monthTokens: 0)
    var allowSubscriptionUnattended = false
    var awareness = true

    /// Stands in for `docs/ACCEPTED_RISKS.md`. The real text comes from `GET /api/risk-notice` and is never rewritten.
    static let riskText = """
    **C35-8. Heron can act on your files and accounts.** When you allow it, Heron reads and changes files in your working folder, runs commands inside the macOS sandbox, and uses the accounts you connect. A mistaken or manipulated action can change or delete your data.

    **R-1. Text from outside can steer a model.** An email, web page or document can contain instructions meant for the model, not for you. Heron marks conversations that have read outside content and asks before it writes, sends or reads outside your folder, but no check is perfect.
    """

    /// `YYYY-MM-DD` in the local time zone, as the core writes a task's `due`.
    static func isoDay(_ now: Date, offset: Int) -> String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        f.locale = Locale(identifier: "en_US_POSIX")
        return f.string(from: Calendar.current.date(byAdding: .day, value: offset, to: now) ?? now)
    }

    static func sample(now: Date) -> PreviewData {
        var d = PreviewData()
        let h: (Double) -> Date = { now.addingTimeInterval($0 * 3600) }
        d.events = [
            CalendarEvent(id: "e1", title: "Team stand-up", start: h(1), place: "Zoom"),
            CalendarEvent(id: "e2", title: "Northwind call", start: h(4), place: "Meeting room 2"),
            CalendarEvent(id: "e3", title: "Dentist", start: h(8), place: "Elm Street"),
        ]
        d.inbox = [
            InboxItem(id: 1, timestamp: h(-2), title: "Friday summary is ready", body: "Three things shipped, one is blocked on review.", read: false),
            InboxItem(id: 2, timestamp: h(-26), title: "Reminder: renew the domain", body: "It expires on 30 November.", read: true),
        ]
        d.suggestions = [
            Suggestion(id: 1, timestamp: h(-3), title: "Collect October receipts", why: "You forwarded six receipts this week.",
                       prompt: "Collect my October receipts into one folder.", status: "new"),
        ]
        d.parked = [
            ParkedApproval(id: 1, timestamp: h(-5), sessionID: "s-friday", tool: "send_email", summary: "Send the Friday summary to the team",
                           detail: "To: team@northwind.example\nSubject: Friday summary", risk: "external", tainted: false,
                           origin: "Friday summary", status: "pending"),
        ]
        d.memories = [
            Memory(id: 1, kind: "fact", content: "Works Tuesday to Saturday; Mondays are for rest.", source: "user", confirmed: true, uses: 41, createdAt: h(-500)),
            Memory(id: 2, kind: "person", content: "Maria Alvarez is the contact at Northwind.", source: "user", confirmed: true, uses: 12, createdAt: h(-300)),
            Memory(id: 3, kind: "project", content: "The Northwind rollout starts on 1 December.", source: "email", confirmed: false, uses: 0, createdAt: h(-20)),
            Memory(id: 4, kind: "preference", content: "Prefers short replies with the decision first.", source: "agent", confirmed: true, uses: 8, createdAt: h(-200)),
        ]
        d.tasks = [
            TaskItem(id: 1, title: "Pay the electricity bill", notes: "", status: "open", due: PreviewData.isoDay(now, offset: -2), owner: "user", progress: nil, completedAt: nil),
            TaskItem(id: 2, title: "Send Maria the revised quote", notes: "", status: "open", due: PreviewData.isoDay(now, offset: 0), owner: "user", progress: nil, completedAt: nil),
            TaskItem(id: 3, title: "Book flights for the conference", notes: "", status: "open", due: nil, owner: "user", progress: nil, completedAt: nil),
            TaskItem(id: 4, title: "Collect October receipts", notes: "", status: "open", due: nil, owner: "heron", progress: "14 of 22 receipts", completedAt: nil),
            TaskItem(id: 5, title: "Renew the parking permit", notes: "", status: "done", due: nil, owner: "user", progress: nil, completedAt: h(-30)),
        ]
        d.schedules = [
            ScheduleItem(id: 1, name: "Morning briefing", prompt: "Brief me on my day.", trigger: "Every day at 07:30", enabled: true,
                         nextRun: h(18), lastRun: h(-6), lastResult: "Sent to your Inbox.", tokens: 4200, heldReason: nil,
                         permissionsNote: "May only read and look things up"),
            ScheduleItem(id: 2, name: "Friday summary", prompt: "Summarise the week.", trigger: "Every Friday at 17:00", enabled: true,
                         nextRun: h(60), lastRun: h(-100), lastResult: "One step waited for you.", tokens: 6100, heldReason: nil,
                         permissionsNote: "May read, and ask before sending"),
            ScheduleItem(id: 3, name: "Watch for invoices", prompt: "Tell me when an invoice arrives.", trigger: "When an email arrives from billing@",
                         enabled: false, nextRun: nil, lastRun: nil, lastResult: nil, tokens: 0,
                         heldReason: "Held: Claude Code is a subscription login, and unattended runs through a subscription are off.",
                         permissionsNote: "May only read and look things up"),
        ]
        d.audit = [
            AuditEntry(id: 1, timestamp: h(-1), sessionID: "s1", tool: "write_file", args: "~/Work/drafts/northwind-reply.md", decision: "approved",
                       result: "Saved", provider: "claude-cli", cliVersion: "2.1.4"),
            AuditEntry(id: 2, timestamp: h(-2), sessionID: "s1", tool: "read_file", args: "~/Work/notes.md", decision: "allowed",
                       result: "Read 40 lines", provider: "claude-cli", cliVersion: "2.1.4"),
            AuditEntry(id: 3, timestamp: h(-3), sessionID: "s1", tool: "shell", args: "rm -rf ~/Library", decision: "refused",
                       result: "Refused", provider: "claude-cli", cliVersion: "2.1.4"),
        ]
        d.providers = [
            ProviderStatus(id: "claude-cli", displayName: "Claude Code", installed: true, version: "2.1.4", testedRange: "2.0 to 2.1",
                           inRange: true, loggedIn: true, experimental: false, versionAccepted: false,
                           loginCommand: "claude auth login", authKind: "subscription login", isActive: true),
            ProviderStatus(id: "codex-cli", displayName: "Codex", installed: true, version: "0.7.0", testedRange: "0.6 to 0.8",
                           inRange: true, loggedIn: false, experimental: false, versionAccepted: false,
                           loginCommand: "codex login", authKind: "subscription login", isActive: false),
            ProviderStatus(id: "gemini-cli", displayName: "Gemini CLI", installed: true, version: "0.9.2", testedRange: "0.6 to 0.8",
                           inRange: false, loggedIn: true, experimental: false, versionAccepted: false,
                           loginCommand: "gemini", authKind: "API key", isActive: false),
            ProviderStatus(id: "qwen-cli", displayName: "Qwen Code", installed: false, version: nil, testedRange: nil,
                           inRange: false, loggedIn: false, experimental: true, versionAccepted: false,
                           loginCommand: "qwen", authKind: "subscription login", isActive: false),
        ]
        d.grants = [
            Grant(rule: "read:/Users/sam/Notes/**", kind: "read"),
            Grant(rule: "shell:git *", kind: "tool"),
        ]
        d.ruleNotices = [
            RuleNotice(rule: "read_file:*", notice: "From your settings file. It no longer opens reads outside your working folder. Add a folder above instead."),
        ]
        d.connections = [
            ConnectionInfo(id: "email", name: "Email", connected: true, detail: "sam@northwind.example"),
            ConnectionInfo(id: "calendar", name: "Calendar", connected: true, detail: "3 events today"),
            ConnectionInfo(id: "documents", name: "Documents", connected: false, detail: "Not connected"),
        ]
        d.usage = UsageReport(days: [
            UsageDay(day: "2026-11-18", model: "Claude Code", promptTokens: 18_400, completionTokens: 3_100),
            UsageDay(day: "2026-11-17", model: "Claude Code", promptTokens: 22_900, completionTokens: 4_800),
        ], monthTokens: 412_000)
        var s = PreviewSession(id: "s-friday", title: "Friday summary", now: h(-5))
        s.append(role: "user", content: "Summarise the week for the team.", now: h(-5))
        s.append(role: "assistant", content: "Three things shipped and one is blocked on review.", now: h(-5))
        d.sessions[s.summary.id] = s
        return d
    }
}
